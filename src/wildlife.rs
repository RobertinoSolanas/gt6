//! Wildlife: a small herd of elephants wandering the streets and a flock of
//! birds circling the sky. Pure Rust (no DOM) so it is unit-testable on the
//! host, like the rest of the game logic.

use crate::city::{City, CELL, LANES, ROAD, SIZE};
use crate::Rng;

const TAU: f64 = std::f64::consts::TAU;

/// A wandering street elephant. `scale` is 1.0 for adults, ~0.55 for a calf.
#[derive(Clone, Copy, Debug)]
pub struct Elephant {
    pub x: f64,
    pub y: f64,
    pub tx: f64,
    pub ty: f64,
    pub heading: f64,
    pub speed: f64,
    pub scale: f64,
    /// Walk-cycle phase in radians (advances with ground distance).
    pub phase: f64,
    /// Smoothed 0..1 "how much is this elephant actually walking". Drops to
    /// 0 when the herd freezes (e.g. a fast car barrels toward it).
    pub gait: f64,
    /// Per-individual timing offset so the herd is never in lockstep.
    pub seed: f64,
}

/// Collision radius (world units) for an elephant of this scale.
impl Elephant {
    pub fn radius(&self) -> f64 {
        17.0 * self.scale
    }
}

/// A random point on the road grid (roads are where the herd wanders).
fn road_target(rng: &mut Rng) -> (f64, f64) {
    let lane = rng.below(LANES) as f64 * CELL + ROAD / 2.0;
    let along = rng.range(30.0, SIZE - 30.0);
    if rng.below(2) == 0 {
        (lane, along)
    } else {
        (along, lane)
    }
}

impl Elephant {
    /// One step of elephant behaviour: wander toward road targets, freeze in
    /// front of a fast approaching car, and stay out of the buildings.
    pub fn update(
        &mut self,
        dt: f64,
        rng: &mut Rng,
        city: &City,
        threat_x: f64,
        threat_y: f64,
        threat_speed: f64,
    ) {
        let dx = threat_x - self.x;
        let dy = threat_y - self.y;
        let dp = (dx * dx + dy * dy).sqrt();
        let startled = dp < 150.0 && threat_speed > 60.0;
        let target_gait = if startled { 0.0 } else { 1.0 };
        self.gait += (target_gait - self.gait) * (dt * 5.0).min(1.0);
        if self.gait < 0.02 {
            return; // frozen mid-stride
        }

        let (tx, ty) = (self.tx - self.x, self.ty - self.y);
        let d = (tx * tx + ty * ty).sqrt();
        if d < 30.0 {
            let (nx, ny) = road_target(rng);
            self.tx = nx;
            self.ty = ny;
        } else {
            let want = ty.atan2(tx);
            self.heading = turn_toward(self.heading, want, 0.9 * dt);
        }

        let step = self.speed * self.gait * dt;
        self.x += self.heading.cos() * step;
        self.y += self.heading.sin() * step;
        self.phase += step * 0.42;

        // Never clip through buildings.
        if let Some((x, y, _, _)) = city.collide_circle(self.x, self.y, self.radius()) {
            self.x = x;
            self.y = y;
        }
    }
}

/// A bird in the sky: flaps most of the time, occasionally locks its wings
/// and glides in slow banked turns.
#[derive(Clone, Copy, Debug)]
pub struct Bird {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub z0: f64, // cruising altitude
    pub heading: f64,
    pub speed: f64,
    /// Wingbeat phase in radians.
    pub flap: f64,
    pub flap_rate: f64,
    /// Seconds remaining in a glide (wings held flat); 0 = flapping.
    pub glide: f64,
    pub span: f64, // full wingspan
    pub len: f64, // body length
    pub color: u32, // 0xRRGGBB body
    pub beak: u32,
    pub seed: f64,
}

/// Species table: (body, beak, wingspan, body length).
const SPECIES: [(u32, u32, f64, f64); 3] = [
    (0x7f8fa8, 0xe8a13c, 15.0, 11.0), // pigeon (iridescent blue-grey)
    (0xf5f7fa, 0xf2b13c, 26.0, 15.0), // gull
    (0x8a5630, 0xf0c419, 34.0, 16.0), // hawk (rich rust-brown)
];

impl Bird {
    pub fn spawn(rng: &mut Rng) -> Self {
        let sp = SPECIES[rng.below(SPECIES.len())];
        Bird {
            x: rng.range(-200.0, SIZE + 200.0),
            y: rng.range(-200.0, SIZE + 200.0),
            z0: rng.range(80.0, 220.0),
            z: 0.0, // set on the first update
            heading: rng.range(0.0, TAU),
            speed: rng.range(38.0, 75.0),
            flap: rng.range(0.0, TAU),
            flap_rate: rng.range(6.0, 9.5),
            glide: 0.0,
            span: sp.2,
            len: sp.3,
            color: sp.0,
            beak: sp.1,
            seed: rng.range(0.0, 100.0),
        }
    }

    pub fn update(&mut self, dt: f64, rng: &mut Rng, time: f64) {
        // Lazy meandering: layered slow sines + occasional random turns.
        self.heading +=
            (time * 0.13 + self.seed).sin() * 0.55 * dt + (time * 0.041 + self.seed * 2.3).sin() * 0.35 * dt;
        if rng.f() < dt * 0.2 {
            self.heading += rng.range(-0.8, 0.8);
        }

        let d = self.speed * dt;
        self.x += self.heading.cos() * d;
        self.y += self.heading.sin() * d;

        // Wrap around the city (birds keep the whole sky covered).
        let m = 300.0;
        if self.x < -m {
            self.x += SIZE + 2.0 * m;
        }
        if self.x > SIZE + m {
            self.x -= SIZE + 2.0 * m;
        }
        if self.y < -m {
            self.y += SIZE + 2.0 * m;
        }
        if self.y > SIZE + m {
            self.y -= SIZE + 2.0 * m;
        }

        // Gentle bobbing around the cruising altitude.
        self.z = self.z0 + 12.0 * (time * 0.33 + self.seed).sin();

        // Flap / glide cycling.
        if self.glide > 0.0 {
            self.glide -= dt;
        } else if rng.f() < dt * 0.10 {
            self.glide = rng.range(1.0, 2.8);
        }
        self.flap += self.flap_rate * dt;
    }
}

/// A giant dragon circling the city high above the streets. Pure data; the
/// full 3D model (a GLB file, loaded at runtime — see `DragonMesh`) is
/// attached separately. Pure Rust, unit-testable like the rest of the
/// wildlife.
#[derive(Clone, Copy, Debug)]
pub struct Dragon {
    pub x: f64,
    pub y: f64,
    /// Altitude (world units above the street).
    pub z: f64,
    /// Cruising altitude (the dragon bobs around this).
    pub z0: f64,
    pub heading: f64,
    pub speed: f64,
    /// Wingbeat phase in radians.
    pub flap: f64,
    /// Smoothed bank angle (radians) for banked turns.
    pub bank: f64,
    /// Vertical velocity (px/s) — only used in player-controlled flight.
    pub vz: f64,
    /// Per-individual timing offset so the flight path is not metronomic.
    pub seed: f64,
    /// True while the player is piloting the dragon ("D"): the autonomous
    /// meander in `update` is skipped and `step_controlled` drives it instead.
    pub controlled: bool,
    /// The dragon owns its RNG so its flight never disturbs the shared
    /// per-tick stream (traffic, peds and police all draw from it, and the
    /// game is deterministic: one extra draw shifts every later decision).
    rng: Rng,
}

// --- Player-controlled dragon flight tuning (px/s, px/s^2, rad/s) ---
const DRAGON_MAX_SPEED: f64 = 560.0;
/// A dragon never truly hovers: at idle throttle it still creeps forward.
const DRAGON_MIN_SPEED: f64 = 45.0;
const DRAGON_YAW_RATE: f64 = 1.5;
/// Vertical speed (px/s) at full climb/dive stick.
const DRAGON_VSPEED: f64 = 230.0;
const DRAGON_MIN_Z: f64 = 26.0;
const DRAGON_MAX_Z: f64 = 900.0;

impl Dragon {
    /// The dragon is a fixed landmark of the city, so its spawn is a
    /// constant: it must not draw from the world RNG (see `self.rng`).
    pub fn spawn() -> Self {
        Dragon {
            x: SIZE * 0.35,
            y: SIZE * 0.60,
            z0: 300.0,
            z: 0.0, // set on the first update
            heading: 1.1,
            speed: 70.0,
            flap: 0.0,
            bank: 0.0,
            vz: 0.0,
            seed: 12.0,
            controlled: false,
            rng: Rng::new(0x646C5F44), // "gltD" - the dragon's private stream
        }
    }

    /// One step of dragon flight: slow, wide, banked meanders around the
    /// city at high altitude.
    pub fn update(&mut self, dt: f64, time: f64) {
        // Layered slow sines + occasional turns: big lazy loops.
        let turn = (time * 0.06 + self.seed).sin() * 0.30
            + (time * 0.023 + self.seed * 2.7).sin() * 0.22;
        if self.rng.f() < dt * 0.08 {
            self.heading += self.rng.range(-0.5, 0.5);
        }
        let rate = turn; // rad/s this frame
        self.heading += rate * dt;
        // Bank into the turn, smoothed.
        let target_bank = (rate * 1.6).clamp(-0.55, 0.55);
        self.bank += (target_bank - self.bank) * (dt * 2.0).min(1.0);

        let d = self.speed * dt;
        self.x += self.heading.cos() * d;
        self.y += self.heading.sin() * d;

        // Wrap around the city (the dragon keeps its rounds above it all).
        let m = 400.0;
        if self.x < -m {
            self.x += SIZE + 2.0 * m;
        }
        if self.x > SIZE + m {
            self.x -= SIZE + 2.0 * m;
        }
        if self.y < -m {
            self.y += SIZE + 2.0 * m;
        }
        if self.y > SIZE + m {
            self.y -= SIZE + 2.0 * m;
        }

        // Gentle bobbing around the cruising altitude.
        self.z = self.z0 + 28.0 * (time * 0.05 + self.seed * 1.7).sin();

        // Slow, majestic wingbeats.
        self.flap += 4.2 * dt;
    }

    /// One step of player-controlled flight: the dragon eases its speed
    /// toward the throttle, banks into its turns, climbs/dives on the pitch
    /// stick, and flaps faster the harder it works. `inp` is the usual car
    /// control set (W/S throttle, A/D steer, Shift/Space pitch, + mouse).
    pub fn step_controlled(&mut self, inp: &crate::car::CarInput, dt: f64) {
        // Speed: ease toward the throttle target (dragons are smooth, not snappy).
        let t = inp.throttle;
        let target = if t >= 0.0 {
            DRAGON_MIN_SPEED + (DRAGON_MAX_SPEED - DRAGON_MIN_SPEED) * t
        } else {
            (DRAGON_MIN_SPEED * (1.0 + t)).max(0.0) // braking back toward a stop
        };
        let k = (1.0 - (-3.0 * dt).exp()).clamp(0.0, 1.0);
        self.speed += (target - self.speed) * k;

        // Yaw into the turn and bank, smoothed.
        self.heading += inp.steer * DRAGON_YAW_RATE * dt;
        let target_bank = (inp.steer * 0.5).clamp(-0.5, 0.5);
        self.bank += (target_bank - self.bank) * ((dt * 2.5).min(1.0));

        // Vertical: ease vz toward the pitch target (smooth climb/dive).
        let target_vz = inp.pitch.clamp(-1.0, 1.0) * DRAGON_VSPEED;
        self.vz += (target_vz - self.vz) * k;
        self.z += self.vz * dt;
        if self.z < DRAGON_MIN_Z {
            self.z = DRAGON_MIN_Z;
            self.vz = self.vz.max(0.0);
        }
        if self.z > DRAGON_MAX_Z {
            self.z = DRAGON_MAX_Z;
            self.vz = self.vz.min(0.0);
        }

        // Move along the heading.
        let (cx, cy) = (self.heading.cos(), self.heading.sin());
        self.x += cx * self.speed * dt;
        self.y += cy * self.speed * dt;

        // Wrap around the city (keep the round above it all).
        let m = 400.0;
        if self.x < -m {
            self.x += SIZE + 2.0 * m;
        }
        if self.x > SIZE + m {
            self.x -= SIZE + 2.0 * m;
        }
        if self.y < -m {
            self.y += SIZE + 2.0 * m;
        }
        if self.y > SIZE + m {
            self.y -= SIZE + 2.0 * m;
        }

        // Wingbeats: the harder it works (speed or climbing), the faster they beat.
        let flap_rate = 3.0
            + 6.0 * (self.speed / DRAGON_MAX_SPEED).clamp(0.0, 1.0)
            + if inp.throttle > 0.5 { 2.0 } else { 0.0 };
        self.flap += flap_rate * dt;
    }
}

/// Model units of the dragon GLB (node transforms applied) -> world units
/// (wingspan ends up ~48, body ~34).
pub const DRAGON_SCALE: f64 = 13.5;

/// The dragon's GLB material is a translucent "attenuation" variant; its
/// actual surface color (KHR_materials_volume attenuationColor, also used
/// as the base color by the solid-color variant) is baked as a flat tint
/// for the software rasterizer.
const DRAGON_TINT: [u8; 3] = [0xeb, 0xa3, 0x10]; // bronze-gold

/// A baked, render-ready dragon mesh built from a parsed GLB. Local frame:
/// x = forward (head), y = left, z = up, origin at the body center, scaled
/// to world units. Vertex colors and per-triangle average colors are baked
/// from the material (base color factor x texture) so the software
/// rasterizer only has to light and project each frame.
#[derive(Clone, Debug)]
pub struct DragonMesh {
    pub vpos: Vec<[f64; 3]>,
    pub vnorm: Vec<[f64; 3]>,
    /// Baked vertex color, 0xRRGGBB.
    pub vcol: Vec<u32>,
    /// Per-vertex 0..1 wingtip weight for the flap animation (0 = body).
    pub vwing: Vec<f64>,
    pub tris: Vec<[u32; 3]>,
    /// Baked per-triangle average color, 0xRRGGBB.
    pub tric: Vec<u32>,
    pub half_span: f64,
    pub z_min: f64,
    pub z_max: f64,
}

impl DragonMesh {
    pub fn tri_count(&self) -> usize {
        self.tris.len()
    }

    /// Bake the dragon mesh out of a parsed GLB model: finds the mesh whose
    /// name contains "dragon" (skipping the backdrop cloth), remaps the
    /// glTF frame (node transforms applied by the loader: x' right,
    /// y' along the body with the head at the low end, z' up) into the game
    /// local frame (x forward = -y', y left = x', z up = z'), centers it,
    /// scales it, and bakes vertex/triangle colors.
    pub fn from_gltf(model: &crate::glb::GltfModel) -> Option<DragonMesh> {
        let mesh = model
            .meshes
            .iter()
            .find(|m| m.name.to_ascii_lowercase().contains("dragon"))?;
        let n = mesh.positions.len();
        if n == 0 || mesh.indices.is_empty() {
            return None;
        }

        // Remap + scale, tracking bounds.
        let mut vpos: Vec<[f64; 3]> = Vec::with_capacity(n);
        for &p in &mesh.positions {
            vpos.push([-p[1] * DRAGON_SCALE, p[0] * DRAGON_SCALE, p[2] * DRAGON_SCALE]);
        }
        let cx = (vpos.iter().map(|v| v[0]).fold(f64::INFINITY, f64::min)
            + vpos.iter().map(|v| v[0]).fold(f64::NEG_INFINITY, f64::max))
            / 2.0;
        let cz = (vpos.iter().map(|v| v[2]).fold(f64::INFINITY, f64::min)
            + vpos.iter().map(|v| v[2]).fold(f64::NEG_INFINITY, f64::max))
            / 2.0;
        let z_min = vpos.iter().map(|v| v[2] - cz).fold(f64::INFINITY, f64::min);
        let z_max = vpos.iter().map(|v| v[2] - cz).fold(f64::NEG_INFINITY, f64::max);
        for v in vpos.iter_mut() {
            v[0] -= cx;
            v[2] -= cz;
        }
        let half_span = vpos.iter().map(|v| v[1].abs()).fold(0.0f64, f64::max).max(1.0e-6);

        // Normals in the local frame (no flap applied at bake time).
        let vnorm: Vec<[f64; 3]> = mesh
            .normals
            .iter()
            .map(|m| [-m[1], m[0], m[2]])
            .collect();

        // Baked vertex colors (tinted bronze when the model has no base
        // color texture, like the dragon's translucent variant does).
        let vcol: Vec<u32> = (0..n)
            .map(|i| {
                let c = mesh.vertex_color(i);
                let (r, g, b) = if mesh.texture.is_some() {
                    (c[0] as u32, c[1] as u32, c[2] as u32)
                } else {
                    (
                        c[0] as u32 * DRAGON_TINT[0] as u32 / 255,
                        c[1] as u32 * DRAGON_TINT[1] as u32 / 255,
                        c[2] as u32 * DRAGON_TINT[2] as u32 / 255,
                    )
                };
                (r << 16) | (g << 8) | b
            })
            .collect();

        // Wingtip weight: 0 over the body, 1 at the wingtips.
        let vwing: Vec<f64> = vpos
            .iter()
            .map(|v| {
                let w = (v[1].abs() / half_span - 0.25) / 0.75;
                let w = w.clamp(0.0, 1.0);
                w * w
            })
            .collect();

        // Triangles + baked average color.
        let mut tris = Vec::with_capacity(mesh.indices.len() / 3);
        let mut tric = Vec::with_capacity(tris.capacity());
        for t in mesh.indices.chunks_exact(3) {
            let [a, b, c] = [t[0], t[1], t[2]];
            if a as usize >= n || b as usize >= n || c as usize >= n {
                continue;
            }
            let avg = (vcol[a as usize] + vcol[b as usize] + vcol[c as usize]) / 3;
            tris.push([a, b, c]);
            tric.push(avg);
        }

        Some(DragonMesh {
            vpos,
            vnorm,
            vcol,
            vwing,
            tris,
            tric,
            half_span,
            z_min,
            z_max,
        })
    }
}

/// Rotate `current` toward `target` by at most `max_step` radians.
fn turn_toward(current: f64, target: f64, max_step: f64) -> f64 {
    let mut diff = (target - current).rem_euclid(TAU);
    if diff > std::f64::consts::PI {
        diff -= TAU;
    }
    // A target exactly π away is ambiguous — don't spin in place.
    if (diff - std::f64::consts::PI).abs() < 1e-9 {
        return current;
    }
    let step = diff.clamp(-max_step, max_step);
    current + step
}

/// The whole wildlife population.
pub struct Wildlife {
    pub elephants: Vec<Elephant>,
    pub birds: Vec<Bird>,
    /// The one giant dragon that circles the city.
    pub dragon: Dragon,
}

pub const ELEPHANT_COUNT: usize = 4;
pub const BIRD_COUNT: usize = 14;

impl Elephant {
    /// Spawn at a specific point (used by `Wildlife::new` to place the herd).
    pub fn spawn_at(rng: &mut Rng, scale: f64, x: f64, y: f64) -> Self {
        let (tx, ty) = road_target(rng);
        Elephant {
            x,
            y,
            tx,
            ty,
            heading: rng.range(0.0, TAU),
            speed: rng.range(15.0, 24.0) * scale.max(0.85),
            scale,
            phase: rng.range(0.0, TAU),
            gait: 1.0,
            seed: rng.range(0.0, 100.0),
        }
    }
}

impl Wildlife {
    /// Create the wildlife, placing the elephant herd on a road at least
    /// `avoid_r` away from `(avoid_x, avoid_y)` (the player's spawn area,
    /// so the game doesn't open on a wall of tusks).
    pub fn new(rng: &mut Rng, avoid_x: f64, avoid_y: f64, avoid_r: f64) -> Self {
        let (hx, hy) = {
            let (mut hx, mut hy) = road_target(rng);
            for _ in 0..64 {
                let (x, y) = road_target(rng);
                if ((x - avoid_x).powi(2) + (y - avoid_y).powi(2)).sqrt() > avoid_r {
                    (hx, hy) = (x, y);
                    break;
                }
            }
            (hx, hy)
        };
        let mut elephants: Vec<Elephant> = Vec::with_capacity(ELEPHANT_COUNT);
        for i in 0..ELEPHANT_COUNT {
            // Three adults plus a calf, loosely clustered at the herd point
            // (they wander as a group, not solo wanderers).
            let scale = if i + 1 == ELEPHANT_COUNT { 0.55 } else { 1.0 };
            let (ox, oy) = (rng.range(-45.0, 45.0), rng.range(-45.0, 45.0));
            let mut e = Elephant::spawn_at(rng, scale, hx + ox, hy + oy);
            if i > 0 {
                let (ax, ay) = (elephants[i - 1].x, elephants[i - 1].y);
                e.x += (ax - e.x) * 0.5;
                e.y += (ay - e.y) * 0.5;
                e.phase = elephants[i - 1].phase + rng.range(0.2, 0.8);
            }
            elephants.push(e);
        }
        let birds: Vec<Bird> = (0..BIRD_COUNT).map(|_| Bird::spawn(rng)).collect();
        let dragon = Dragon::spawn();
        Wildlife { elephants, birds, dragon }
    }

    /// One simulation step. `threat_*` is the player (used to startle the
    /// elephants with a fast car).
    pub fn update(
        &mut self,
        dt: f64,
        rng: &mut Rng,
        time: f64,
        city: &City,
        threat_x: f64,
        threat_y: f64,
        threat_speed: f64,
    ) {
        for e in self.elephants.iter_mut() {
            e.update(dt, rng, city, threat_x, threat_y, threat_speed);
        }
        // Simple herd separation so the elephants don't stack.
        for i in 0..self.elephants.len() {
            for j in i + 1..self.elephants.len() {
                let (left, right) = self.elephants.split_at_mut(i + 1);
                let a = &mut left[i];
                let b = &mut right[j - i - 1];
                let dx = b.x - a.x;
                let dy = b.y - a.y;
                let d = (dx * dx + dy * dy).sqrt();
                let min = (a.radius() + b.radius()) * 0.9;
                if d < min && d > 0.001 {
                    let push = (min - d) * 0.25;
                    let (nx, ny) = (dx / d, dy / d);
                    a.x -= nx * push;
                    a.y -= ny * push;
                    b.x += nx * push;
                    b.y += ny * push;
                }
            }
        }
        for b in self.birds.iter_mut() {
            b.update(dt, rng, time);
        }
        // The dragon only meanders on its own when the player isn't piloting it.
        if !self.dragon.controlled {
            self.dragon.update(dt, time);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::city::City;

    fn setup() -> (Wildlife, City, Rng) {
        let mut rng = Rng::new(7);
        let wildlife = Wildlife::new(&mut rng, 0.0, 0.0, 0.0);
        let city = City::new(7);
        (wildlife, city, rng)
    }

    #[test]
    fn herd_spawns_away_from_the_avoid_point() {
        let mut rng = Rng::new(99);
        let w = Wildlife::new(&mut rng, 600.0, 600.0, 1200.0);
        for e in &w.elephants {
            let d = ((e.x - 600.0).powi(2) + (e.y - 600.0).powi(2)).sqrt();
            assert!(d > 1100.0, "elephant at ({},{}) is too close ({})", e.x, e.y, d);
        }
    }

    #[test]
    fn spawns_the_expected_counts() {
        let (w, _, _) = setup();
        assert_eq!(w.elephants.len(), ELEPHANT_COUNT);
        assert_eq!(w.birds.len(), BIRD_COUNT);
        // Exactly one calf, rest adults.
        let calves = w.elephants.iter().filter(|e| e.scale < 0.8).count();
        assert_eq!(calves, 1);
    }

    #[test]
    fn elephants_walk_the_streets() {
        let (mut w, city, mut rng) = setup();
        let (x0, y0) = (w.elephants[0].x, w.elephants[0].y);
        for _ in 0..300 {
            w.update(1.0 / 60.0, &mut rng, 5.0, &city, f64::INFINITY, f64::INFINITY, 0.0);
        }
        let e = &w.elephants[0];
        let moved = ((e.x - x0).powi(2) + (e.y - y0).powi(2)).sqrt();
        assert!(moved > 40.0, "elephant should wander, moved {moved}");
        // Still on (or at the edge of) the grid, never inside a building.
        assert!(!city
            .buildings()
            .any(|b| b.contains(e.x, e.y)), "elephant walked into a building");
    }

    #[test]
    fn elephants_freeze_for_a_fast_car() {
        let (mut w, city, mut rng) = setup();
        // Park a "car" right on top of the first elephant and go fast.
        let (tx, ty) = (w.elephants[0].x, w.elephants[0].y);
        for _ in 0..120 {
            w.update(1.0 / 60.0, &mut rng, 5.0, &city, tx, ty, 300.0);
        }
        assert!(w.elephants[0].gait < 0.05, "gait = {}", w.elephants[0].gait);
    }

    #[test]
    fn birds_fly_above_the_ground_and_wrap() {
        let (mut w, city, mut rng) = setup();
        for t in 0..600 {
            w.update(1.0 / 60.0, &mut rng, t as f64 / 60.0, &city, 0.0, 0.0, 0.0);
        }
        let m = 300.0;
        for b in &w.birds {
            assert!(b.z > 40.0, "bird altitude too low: {}", b.z);
            assert!(b.x >= -m && b.x <= SIZE + m, "bird x out of wrap band: {}", b.x);
            assert!(b.y >= -m && b.y <= SIZE + m, "bird y out of wrap band: {}", b.y);
        }
        // Wingbeats advanced.
        assert!(w.birds[0].flap > 10.0);
    }

    #[test]
    fn dragon_flies_high_and_stays_in_the_wrap_band() {
        let (mut w, city, mut rng) = setup();
        for t in 0..600 {
            w.update(1.0 / 60.0, &mut rng, t as f64 / 60.0, &city, 0.0, 0.0, 0.0);
        }
        let d = &w.dragon;
        let m = 400.0;
        assert!(d.z > 100.0, "dragon too low: {}", d.z);
        assert!(
            d.z0 - 60.0 < d.z && d.z < d.z0 + 60.0,
            "dragon altitude {} not near cruise {}",
            d.z,
            d.z0
        );
        assert!(d.x >= -m && d.x <= SIZE + m, "dragon x out of wrap band: {}", d.x);
        assert!(d.y >= -m && d.y <= SIZE + m, "dragon y out of wrap band: {}", d.y);
        assert!(d.flap > 10.0, "wings should be beating");
    }

    #[test]
    fn controlled_dragon_flies_where_the_player_says() {
        let (mut w, _city, _rng) = setup();
        let d = &mut w.dragon;
        let (x0, y0) = (d.x, d.y);

        // Full throttle + turn right + climb.
        let up = crate::car::CarInput {
            throttle: 1.0,
            steer: 1.0,
            pitch: 1.0,
            ..Default::default()
        };
        let h0 = d.heading;
        let z0 = d.z;
        for _ in 0..120 {
            d.step_controlled(&up, 1.0 / 60.0);
        }
        assert!(d.speed > 200.0, "should be flying fast: {}", d.speed);
        assert!(d.heading > h0, "should have turned right");
        assert!(d.z > z0, "should have climbed: {} vs {}", d.z, z0);
        assert!(d.bank > 0.1, "should bank into the turn: {}", d.bank);
        // Actually moved.
        let moved = ((d.x - x0).powi(2) + (d.y - y0).powi(2)).sqrt();
        assert!(moved > 100.0, "should have moved: {}", moved);

        // Dive clamps at the street and can't go below it.
        let dn = crate::car::CarInput {
            pitch: -1.0,
            ..Default::default()
        };
        for _ in 0..600 {
            d.step_controlled(&dn, 1.0 / 60.0);
        }
        assert!(
            (d.z - DRAGON_MIN_Z).abs() < 1.0,
            "should rest at the floor: {}",
            d.z
        );

        // Braking eases the dragon back toward a stop.
        let br = crate::car::CarInput {
            throttle: -1.0,
            ..Default::default()
        };
        for _ in 0..600 {
            d.step_controlled(&br, 1.0 / 60.0);
        }
        assert!(d.speed < 5.0, "should be nearly stopped: {}", d.speed);
    }

    #[test]
    fn controlled_dragon_is_skipped_by_autonomous_update() {
        let (mut w, city, mut rng) = setup();
        let d = &w.dragon;
        let (x0, z0) = (d.x, d.z);
        // With the player piloting, the autonomous meander must not move it.
        w.dragon.controlled = true;
        for t in 0..120 {
            w.update(1.0 / 60.0, &mut rng, t as f64 / 60.0, &city, 0.0, 0.0, 0.0);
        }
        assert_eq!(w.dragon.x, x0, "controlled dragon x must not drift");
        assert_eq!(w.dragon.z, z0, "controlled dragon z must not drift");
    }

    #[test]
    fn dragon_mesh_bakes_from_gltf() {
        // glTF frame (node transforms applied): x' right, y' along the body
        // (head at the low end), z' up.
        let model = crate::glb::GltfModel {
            meshes: vec![crate::glb::GltfMesh {
                name: "Dragon".into(),
                positions: vec![[0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]],
                normals: vec![[0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
                uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]],
                indices: vec![0, 1, 2],
                base_color: [1.0, 1.0, 1.0],
                texture: None,
            }],
        };
        let dm = DragonMesh::from_gltf(&model).expect("bakes");
        assert_eq!(dm.tri_count(), 1);
        assert_eq!(dm.vpos.len(), 3);
        // -y' is the game forward (x): the head (y'=-1) ends up in front of
        // the tail (y'=+1), with the full model length between them.
        assert!((dm.vpos[1][0] - DRAGON_SCALE).abs() < 1e-9, "head: {:?}", dm.vpos[1]);
        assert!((dm.vpos[0][0] + DRAGON_SCALE).abs() < 1e-9, "tail: {:?}", dm.vpos[0]);
        assert!((dm.vpos[1][0] - dm.vpos[0][0] - 2.0 * DRAGON_SCALE).abs() < 1e-9);
        // Wing weight: 1 at the widest vertex (|y| == half_span), 0 on the body.
        assert!((dm.vwing[2] - 1.0).abs() < 1e-9, "wing vertex has full flap weight");
        assert!(dm.vwing[0] < 0.01, "body vertex has no flap weight");
        // No texture -> the bronze tint is baked in (not white).
        assert_ne!(dm.tric[0] & 0xff0000, 0xff0000, "should be tinted bronze");
        assert!(dm.tric[0] >> 16 > 0xcc, "reddish bronze: {:#08x}", dm.tric[0]);
    }

    #[test]
    fn dragon_mesh_ignores_non_dragon_meshes() {
        let model = crate::glb::GltfModel {
            meshes: vec![crate::glb::GltfMesh {
                name: "Cloth Backdrop".into(),
                positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                normals: vec![[0.0, 0.0, 1.0]; 3],
                uvs: vec![[0.0, 0.0]; 3],
                indices: vec![0, 1, 2],
                base_color: [1.0; 3],
                texture: None,
            }],
        };
        assert!(DragonMesh::from_gltf(&model).is_none());
    }

    #[test]
    fn turn_toward_takes_the_short_way() {
        use std::f64::consts::PI;
        // -π is the same as +π: never a full spin (at most a step, either way).
        assert!(turn_toward(0.0, PI, 0.1).abs() <= 0.1 + 1e-9);
        assert!(turn_toward(0.0, -PI, 0.1).abs() <= 0.1 + 1e-9);
        assert!((turn_toward(0.0, -PI / 2.0, 0.1) + 0.1).abs() < 1e-9);
        assert!((turn_toward(0.0, PI / 4.0, 0.1) - 0.1).abs() < 1e-9);
    }
}
