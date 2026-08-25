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
        Wildlife { elephants, birds }
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
    fn turn_toward_takes_the_short_way() {
        use std::f64::consts::PI;
        // -π is the same as +π: never a full spin (at most a step, either way).
        assert!(turn_toward(0.0, PI, 0.1).abs() <= 0.1 + 1e-9);
        assert!(turn_toward(0.0, -PI, 0.1).abs() <= 0.1 + 1e-9);
        assert!((turn_toward(0.0, -PI / 2.0, 0.1) + 0.1).abs() < 1e-9);
        assert!((turn_toward(0.0, PI / 4.0, 0.1) - 0.1).abs() < 1e-9);
    }
}
