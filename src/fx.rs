//! Particle effects: a small, pure-Rust particle pool (no DOM), so the
//! simulation is unit-testable on the host, like the rest of the game logic.
//! The top-down and 3D renderers each draw the particles every frame.

use crate::Rng;

const TAU: f64 = std::f64::consts::TAU;

/// What a particle looks like when drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PKind {
    /// Soft expanding blob: tire smoke, exhaust, dust puffs, plane contrails.
    Smoke,
    /// Bright short-lived fleck that falls: crash sparks, impact glints.
    Spark,
    /// Rising sparkle: mission pickups and deliveries.
    Glitter,
    /// Small hard chunk with gravity + bounce: debris from hard impacts.
    Debris,
    /// Arcing blue droplet thrown at a blaze by a citizen with water.
    Water,
}

#[derive(Clone, Copy, Debug)]
pub struct Particle {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    /// Seconds remaining (counts down to zero).
    pub life: f64,
    /// Total lifetime (for the fade curve).
    pub ttl: f64,
    pub size: f64,
    /// Size change per second (+ grows, - shrinks).
    pub grow: f64,
    /// 0xRRGGBB
    pub color: u32,
    pub alpha: f64,
    /// Gravity, world units/s^2, pulled down along -z.
    pub grav: f64,
    /// Velocity damping per second.
    pub drag: f64,
    pub kind: PKind,
}

/// The whole particle pool for the game.
pub struct Fx {
    pub particles: Vec<Particle>,
}

/// Hard cap so a sustained spin-out can't fill memory or tank the frame.
const CAP: usize = 700;

impl Fx {
    pub fn new() -> Self {
        Fx { particles: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.particles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.particles.is_empty()
    }

    /// Fade multiplier 0..1 over a particle's life: quick fade-in right after
    /// spawn, gentle fade-out at the end.
    pub fn fade(p: &Particle) -> f64 {
        let t = (p.life / p.ttl).clamp(0.0, 1.0);
        ((1.0 - t) * 6.0).clamp(0.0, 1.0) * (t * t).sqrt()
    }

    /// Advance every particle by `dt` seconds and reap the dead ones.
    pub fn update(&mut self, dt: f64) {
        for p in self.particles.iter_mut() {
            p.life -= dt;
            let d = (1.0 - p.drag * dt).max(0.0);
            p.vx *= d;
            p.vy *= d;
            p.vz *= d;
            p.vz -= p.grav * dt;
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.z += p.vz * dt;
            // Debris rests on the street.
            if p.kind == PKind::Debris && p.z < 0.5 && p.vz < 0.0 {
                p.z = 0.5;
                p.vz = p.vz.abs() * 0.3;
            }
            p.size = (p.size + p.grow * dt).max(0.1);
        }
        self.particles.retain(|p| p.life > 0.0);
    }

    fn push(&mut self, p: Particle) {
        if self.particles.len() >= CAP {
            self.particles.remove(0);
        }
        self.particles.push(p);
    }

    /// One soft smoke/dust puff drifting up from (x, y, z).
    pub fn smoke(
        &mut self,
        rng: &mut Rng,
        x: f64,
        y: f64,
        z: f64,
        vx: f64,
        vy: f64,
        vz: f64,
        ttl: f64,
        size: f64,
        color: u32,
        alpha: f64,
    ) {
        let life = ttl * rng.range(0.7, 1.3);
        self.push(Particle {
            x: x + rng.range(-2.5, 2.5),
            y: y + rng.range(-2.5, 2.5),
            z,
            vx: vx * rng.range(0.7, 1.3) + rng.range(-8.0, 8.0),
            vy: vy * rng.range(0.7, 1.3) + rng.range(-8.0, 8.0),
            vz: vz + rng.range(-2.0, 6.0),
            life,
            ttl: life,
            size: size * rng.range(0.8, 1.3),
            grow: size * 0.6, // smoke puffs swell as they age
            color,
            alpha,
            grav: 10.0,
            drag: 1.6,
            kind: PKind::Smoke,
        });
    }

    /// A burst of bright sparks kicked up around (x, y, z).
    pub fn sparks(&mut self, rng: &mut Rng, x: f64, y: f64, z: f64, n: usize, speed: f64, color: u32) {
        for _ in 0..n {
            let a = rng.f() * TAU;
            let sp = rng.range(0.25, 1.0) * speed;
            let life = rng.range(0.2, 0.6);
            self.push(Particle {
                x,
                y,
                z: z + rng.range(0.0, 4.0),
                vx: a.cos() * sp,
                vy: a.sin() * sp,
                vz: rng.range(10.0, 130.0),
                life,
                ttl: life,
                size: rng.range(1.2, 2.8),
                grow: -2.0,
                color,
                alpha: 0.95,
                grav: 320.0,
                drag: 0.9,
                kind: PKind::Spark,
            });
        }
    }

    /// A puff of ground dust kicked out horizontally (elephant feet, landings).
    pub fn dust(&mut self, rng: &mut Rng, x: f64, y: f64, n: usize, color: u32) {
        for _ in 0..n {
            let a = rng.f() * TAU;
            let sp = rng.range(8.0, 40.0);
            let life = rng.range(0.4, 1.0);
            self.push(Particle {
                x,
                y,
                z: 1.0 + rng.range(0.0, 2.0),
                vx: a.cos() * sp,
                vy: a.sin() * sp,
                vz: rng.range(4.0, 22.0),
                life,
                ttl: life,
                size: rng.range(3.0, 6.0),
                grow: 7.0,
                color,
                alpha: 0.35,
                grav: 14.0,
                drag: 2.0,
                kind: PKind::Smoke,
            });
        }
    }

    /// Rising celebratory sparkles (mission pickup/delivery).
    pub fn glitter(&mut self, rng: &mut Rng, x: f64, y: f64, z: f64, n: usize, color: u32) {
        for _ in 0..n {
            let a = rng.f() * TAU;
            let r = rng.range(2.0, 26.0);
            let life = rng.range(0.7, 1.6);
            self.push(Particle {
                x: x + a.cos() * r * 0.5,
                y: y + a.sin() * r * 0.5,
                z: z + rng.range(0.0, 6.0),
                vx: a.cos() * 12.0,
                vy: a.sin() * 12.0,
                vz: rng.range(40.0, 110.0),
                life,
                ttl: life,
                size: rng.range(1.6, 3.4),
                grow: 1.5,
                color,
                alpha: 1.0,
                grav: -40.0, // glitters float upward
                drag: 1.4,
                kind: PKind::Glitter,
            });
        }
    }

    /// A droplet of water hurled at a fire: arcs on a ballistic path and
    /// puffs into mist where it lands.
    pub fn water(&mut self, rng: &mut Rng, x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64) {
        let life = rng.range(0.35, 0.7);
        self.push(Particle {
            x,
            y,
            z,
            vx,
            vy,
            vz,
            life,
            ttl: life,
            size: rng.range(1.5, 2.6),
            grow: 2.0,
            color: 0x6cc4ff,
            alpha: 0.85,
            grav: 240.0,
            drag: 0.35,
            kind: PKind::Water,
        });
    }

    /// Small hard chunks that bounce off the street (car crashes).
    pub fn debris(&mut self, rng: &mut Rng, x: f64, y: f64, n: usize, color: u32) {
        for _ in 0..n {
            let a = rng.f() * TAU;
            let sp = rng.range(20.0, 90.0);
            let life = rng.range(0.5, 1.1);
            self.push(Particle {
                x,
                y,
                z: 3.0,
                vx: a.cos() * sp,
                vy: a.sin() * sp,
                vz: rng.range(40.0, 150.0),
                life,
                ttl: life,
                size: rng.range(1.4, 3.0),
                grow: 0.0,
                color,
                alpha: 0.9,
                grav: 420.0,
                drag: 0.4,
                kind: PKind::Debris,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particles_live_then_die() {
        let mut rng = Rng::new(7);
        let mut fx = Fx::new();
        fx.sparks(&mut rng, 100.0, 100.0, 2.0, 20, 200.0, 0xffc94a);
        assert_eq!(fx.len(), 20);
        for _ in 0..120 {
            fx.update(1.0 / 60.0);
        }
        assert!(fx.is_empty(), "all sparks should be gone after 2s");
    }

    #[test]
    fn smoke_rises_and_fades() {
        let mut rng = Rng::new(8);
        let mut fx = Fx::new();
        fx.smoke(&mut rng, 0.0, 0.0, 2.0, 0.0, 0.0, 20.0, 1.0, 5.0, 0x888888, 0.5);
        let p0 = fx.particles[0];
        for _ in 0..30 {
            fx.update(1.0 / 60.0);
        }
        let p = &fx.particles[0];
        assert!(p.z > p0.z, "smoke should drift up");
        assert!(p.size > p0.size, "smoke should swell");
        assert!((0.0..1.0).contains(&Fx::fade(p)));
    }

    #[test]
    fn pool_is_capped() {
        let mut rng = Rng::new(9);
        let mut fx = Fx::new();
        for _ in 0..CAP + 50 {
            fx.sparks(&mut rng, 0.0, 0.0, 0.0, 4, 100.0, 0xffffff);
        }
        assert!(fx.len() <= CAP);
    }

    #[test]
    fn debris_rests_on_the_street() {
        let mut rng = Rng::new(10);
        let mut fx = Fx::new();
        fx.debris(&mut rng, 0.0, 0.0, 5, 0x444444);
        for _ in 0..60 {
            fx.update(1.0 / 60.0);
        }
        assert!(!fx.is_empty());
        assert!(fx.particles.iter().all(|p| p.z >= 0.4), "debris should not sink");
    }
}
