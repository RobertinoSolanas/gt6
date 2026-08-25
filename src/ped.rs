//! Pedestrians: they walk the sidewalk band around a block (one side at a
//! time), wander to new targets, flee a fast approaching car, and can be
//! run over (which raises the wanted level).

use crate::Rng;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PedState {
    Alive,
    Dead(f64), // death time (for fade-out)
}

#[derive(Clone, Copy, Debug)]
pub struct Ped {
    pub x: f64,
    pub y: f64,
    pub tx: f64,
    pub ty: f64,
    pub heading: f64,
    pub speed: f64,
    /// Which block side this ped is walking on: (i, j, side 0..=3).
    pub route: (usize, usize, u8),
    pub state: PedState,
    pub color: u32,
}

/// Bright shirt palette so the sidewalk crowd pops against the grey slabs.
const SHIRT: [u32; 10] = [
    0xf4d35e, 0x2a9d8f, 0xe76f51, 0x4ea8de, 0xf28482,
    0x9b5de5, 0x06d6a0, 0xef476f, 0xffd166, 0x118ab2,
];

/// Center point of the sidewalk line for block side `side` (0=N, 1=E, 2=S, 3=W).
pub fn side_point(i: usize, j: usize, side: u8, t: f64) -> (f64, f64) {
    use crate::city::{BLOCK, CELL, ROAD, SIDEWALK};
    let bx = i as f64 * CELL + ROAD;
    let by = j as f64 * CELL + ROAD;
    let in1 = SIDEWALK / 2.0; // walk line just inside the block edge
    match side {
        0 => (bx + t * BLOCK, by + in1),
        1 => (bx + BLOCK - in1, by + t * BLOCK),
        2 => (bx + (1.0 - t) * BLOCK, by + BLOCK - in1),
        _ => (bx + in1, by + (1.0 - t) * BLOCK),
    }
}

impl Ped {
    pub fn spawn(rng: &mut Rng) -> Self {
        let i = rng.below(crate::city::N);
        let j = rng.below(crate::city::N);
        let side = rng.below(4) as u8;
        let t = rng.f();
        let (x, y) = side_point(i, j, side, t);
        Ped {
            x,
            y,
            tx: x,
            ty: y,
            heading: rng.range(0.0, std::f64::consts::TAU),
            speed: rng.range(30.0, 46.0),
            route: (i, j, side),
            state: PedState::Alive,
            color: SHIRT[rng.below(SHIRT.len())],
        }
    }

    fn pick_target(&mut self, rng: &mut Rng) {
        // Usually continue on this side; sometimes turn a corner.
        let turn = rng.f() < 0.45;
        let (i, j, side) = self.route;
        let side = if turn { (side as usize + 1 + rng.below(2)) as u8 % 4 } else { side };
        self.route = (i, j, side);
        let t = rng.f();
        let (tx, ty) = side_point(i, j, side, t);
        self.tx = tx;
        self.ty = ty;
    }

    /// Update one ped. `threat_pos`/`threat_speed` is the player's car.
    pub fn update(&mut self, dt: f64, rng: &mut Rng, threat_x: f64, threat_y: f64, threat_speed: f64) {
        if !matches!(self.state, PedState::Alive) {
            return;
        }
        let dpx = threat_x - self.x;
        let dpy = threat_y - self.y;
        let dp = (dpx * dpx + dpy * dpy).sqrt();
        let fleeing = dp < 130.0 && threat_speed > 90.0;

        if fleeing {
            // Run straight away from the threat.
            let inv = if dp > 1.0 { 1.0 / dp } else { 0.0 };
            self.tx = self.x - dpx * inv * 80.0;
            self.ty = self.y - dpy * inv * 80.0;
        }
        let dx = self.tx - self.x;
        let dy = self.ty - self.y;
        let d = (dx * dx + dy * dy).sqrt();
        if !fleeing && d < 6.0 {
            self.pick_target(rng);
        } else {
            self.heading = angle_to(self.heading, dy.atan2(dx));
        }

        let spd = if fleeing { 150.0 } else { self.speed };
        self.x += self.heading.cos() * spd * dt;
        self.y += self.heading.sin() * spd * dt;
    }

    pub fn kill(&mut self, now: f64) {
        if matches!(self.state, PedState::Alive) {
            self.state = PedState::Dead(now);
        }
    }
}

/// Rotate `current` heading toward `target` (shortest way, no 180° spins).
fn angle_to(current: f64, target: f64) -> f64 {
    let mut diff = (target - current).rem_euclid(std::f64::consts::TAU);
    if diff > std::f64::consts::PI {
        diff -= std::f64::consts::TAU;
    }
    current + diff
}
