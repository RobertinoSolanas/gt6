//! Arcade car physics: a bicycle-ish model with separated forward/lateral
//! friction, so the handbrake can break traction and make the car drift.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CarKind {
    Player,
    Traffic,
    Police,
}

#[derive(Clone, Copy, Debug)]
pub struct Car {
    pub x: f64,
    pub y: f64,
    /// Heading in radians (direction the front of the car points).
    pub heading: f64,
    pub vx: f64,
    pub vy: f64,
    pub kind: CarKind,
    pub color: u32, // 0xRRGGBB
    pub radius: f64,
}

impl Car {
    pub fn new(x: f64, y: f64, heading: f64, kind: CarKind) -> Self {
        let (color, radius) = match kind {
            CarKind::Player => (0xd62828, 24.0),
            CarKind::Traffic => (0x457b9d, 18.0),
            CarKind::Police => (0xf1f1f1, 19.0),
        };
        Car { x, y, heading, vx: 0.0, vy: 0.0, kind, color, radius }
    }

    pub fn speed(&self) -> f64 {
        (self.vx * self.vx + self.vy * self.vy).sqrt()
    }

    /// Signed speed along the heading (+ forward, - backward).
    pub fn forward_speed(&self) -> f64 {
        self.vx * self.heading.cos() + self.vy * self.heading.sin()
    }

    /// Lateral slip (px/s); large |slip| means the car is drifting.
    pub fn slip(&self) -> f64 {
        -self.vx * self.heading.sin() + self.vy * self.heading.cos()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CarInput {
    /// -1..=1 (negative = reverse/brake)
    pub throttle: f64,
    /// -1..=1 (negative = left)
    pub steer: f64,
    pub handbrake: bool,
    pub boost: bool,
}

// --- Tuning constants (px/s, px/s^2, 1/s) ---
pub const ACCEL: f64 = 520.0;
pub const BOOST_ACCEL: f64 = 1.6;
pub const BRAKE: f64 = 780.0;
pub const REVERSE_ACCEL: f64 = 300.0;
pub const MAX_SPEED: f64 = 540.0;
pub const MAX_REVERSE: f64 = 170.0;
pub const DRAG: f64 = 0.9;
pub const GRIP: f64 = 9.0; // lateral friction (grippy)
pub const HANDBRAKE_GRIP: f64 = 1.6; // lateral friction (sliding)
pub const HANDBRAKE_DRAG: f64 = 340.0;
pub const STEER_RATE: f64 = 2.7; // rad/s at full steering authority
pub const STEER_FULL_SPEED: f64 = 160.0; // speed at which steering is full
/// Minimum steering authority at any non-zero speed (parking manoeuvres).
pub const PARK_STEER: f64 = 0.25;

/// Integrate car physics for `dt` seconds.
pub fn step_car(c: &mut Car, inp: &CarInput, dt: f64) {
    let (fx, fy) = (c.heading.cos(), c.heading.sin());
    let mut fwd = c.vx * fx + c.vy * fy;
    let mut lat = -c.vx * fy + c.vy * fx;

    let max_f = MAX_SPEED * if inp.boost { 1.28 } else { 1.0 };

    if inp.throttle > 0.0 {
        fwd += ACCEL * inp.throttle * if inp.boost { BOOST_ACCEL } else { 1.0 } * dt;
    } else if inp.throttle < 0.0 {
        if fwd > 0.0 {
            // Brake all the way to a full stop before building reverse,
            // so holding S from a crawl doesn't lurch the car backwards.
            fwd = (fwd - BRAKE * dt).max(0.0);
        } else {
            fwd += REVERSE_ACCEL * inp.throttle * dt; // reversing
        }
    }

    if inp.handbrake {
        let d = HANDBRAKE_DRAG * dt;
        if fwd > 0.0 {
            fwd = (fwd - d).max(0.0);
        } else {
            fwd = (fwd + d).min(0.0);
        }
    }

    fwd = fwd.clamp(-MAX_REVERSE, max_f);
    // Rolling drag: exponential + a constant roll so the car fully stops.
    fwd *= 1.0 / (1.0 + DRAG * dt);
    let roll = 40.0 * dt;
    if fwd.abs() <= roll {
        fwd = 0.0;
    } else {
        fwd -= fwd.signum() * roll;
    }
    // Lateral tire friction (where drift lives).
    let grip = if inp.handbrake { HANDBRAKE_GRIP } else { GRIP };
    lat *= 1.0 / (1.0 + grip * dt);

    c.vx = fx * fwd - fy * lat;
    c.vy = fy * fwd + fx * lat;

    // Steering authority scales with speed and flips when reversing. A
    // floor (PARK_STEER) keeps the car steerable at crawl speeds so you
    // can park; a fully stopped car still can't spin its wheels.
    let authority = if fwd.abs() < 2.0 {
        0.0
    } else {
        (fwd / STEER_FULL_SPEED).clamp(-1.0, 1.0) * (1.0 - PARK_STEER) + fwd.signum() * PARK_STEER
    };
    c.heading += inp.steer * STEER_RATE * authority * dt;

    c.x += c.vx * dt;
    c.y += c.vy * dt;
}

/// Resolve a car's circle against the city, with a small bounce.
/// Returns true if a collision happened.
pub fn collide_car_with_city(c: &mut Car, city: &crate::city::City) -> bool {
    if let Some((nx, ny, ux, uy)) = city.collide_circle(c.x, c.y, c.radius) {
        c.x = nx;
        c.y = ny;
        // Reflect the velocity component along the normal.
        let vn = c.vx * ux + c.vy * uy;
        if vn < 0.0 {
            c.vx -= 1.6 * vn * ux;
            c.vy -= 1.6 * vn * uy;
            c.vx *= 0.7;
            c.vy *= 0.7;
        }
        return true;
    }
    false
}

/// Simple seek-steering used by police cars (and anything else that just
/// needs to drive to a point): turn toward the target, throttle if aligned.
pub fn drive_to(c: &mut Car, tx: f64, ty: f64, max_speed: f64, turn_rate: f64, dt: f64) {
    let dx = tx - c.x;
    let dy = ty - c.y;
    let dist = (dx * dx + dy * dy).sqrt();
    let target = dx.atan2(dy);

    // Angle difference wrapped to [-pi, pi].
    let mut diff = target - c.heading;
    while diff > std::f64::consts::PI {
        diff -= 2.0 * std::f64::consts::PI;
    }
    while diff < -std::f64::consts::PI {
        diff += 2.0 * std::f64::consts::PI;
    }

    let steer = diff.clamp(-1.0, 1.0);
    // Cut in as we approach so we don't orbit the target forever.
    let arrive = (dist / 90.0).min(1.0);
    let throttle = if dist > 12.0 { arrive } else { 0.0 };

    let inp = CarInput { throttle, steer: steer.min(1.0), handbrake: false, boost: false };
    // Temporarily clamp speed: run normal physics, then soft-clamp speed.
    step_car(c, &inp, dt);
    let cap = max_speed * (turn_rate / 2.2).clamp(0.4, 1.1);
    let s = c.speed();
    if s > cap {
        let k = cap / s;
        c.vx *= k;
        c.vy *= k;
    }
    c.heading += steer.clamp(-turn_rate, turn_rate).signum() * 0.0; // (no-op; clarity)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn car() -> Car {
        Car {
            x: 400.0,
            y: 400.0,
            heading: 0.0,
            vx: 0.0,
            vy: 0.0,
            kind: CarKind::Player,
            color: 0,
            radius: 24.0,
        }
    }

    #[test]
    fn throttle_accelerates_forward() {
        let mut c = car();
        let inp = CarInput { throttle: 1.0, ..Default::default() };
        for _ in 0..60 {
            step_car(&mut c, &inp, 1.0 / 60.0);
        }
        assert!(c.forward_speed() > 100.0, "should be moving forward: {}", c.forward_speed());
        assert!(c.x > 400.0);
        assert!((c.y - 400.0).abs() < 1.0, "should not slide sideways");
    }

    #[test]
    fn no_input_rolls_to_a_stop() {
        let mut c = car();
        let inp = CarInput { throttle: 1.0, ..Default::default() };
        for _ in 0..120 {
            step_car(&mut c, &inp, 1.0 / 60.0);
        }
        let fast = c.speed();
        assert!(fast > 100.0);
        let stop = CarInput::default();
        for _ in 0..240 {
            step_car(&mut c, &stop, 1.0 / 60.0);
        }
        assert!(c.speed() < 2.0, "drag should stop the car, got {}", c.speed());
    }

    #[test]
    fn brake_then_reverse() {
        let mut c = car();
        let inp = CarInput { throttle: 1.0, ..Default::default() };
        for _ in 0..90 {
            step_car(&mut c, &inp, 1.0 / 60.0);
        }
        let rev = CarInput { throttle: -1.0, ..Default::default() };
        for _ in 0..180 {
            step_car(&mut c, &rev, 1.0 / 60.0);
        }
        assert!(c.forward_speed() < -50.0, "should be reversing: {}", c.forward_speed());
        assert!(c.forward_speed() > -MAX_REVERSE - 1.0);
    }

    #[test]
    fn steering_turns_the_car_and_reverses_when_backing() {
        let mut c = car();
        let fwd = CarInput { throttle: 1.0, steer: 1.0, ..Default::default() };
        for _ in 0..90 {
            step_car(&mut c, &fwd, 1.0 / 60.0);
        }
        let after_forward = c.heading;
        assert!(after_forward > 0.3, "should turn right: {}", after_forward);

        let mut r = car();
        // Get some forward speed first, then back up with the same steering.
        let go = CarInput { throttle: 1.0, ..Default::default() };
        for _ in 0..90 {
            step_car(&mut r, &go, 1.0 / 60.0);
        }
        let back = CarInput { throttle: -1.0, steer: 1.0, ..Default::default() };
        for _ in 0..90 {
            step_car(&mut r, &back, 1.0 / 60.0);
        }
        assert!(r.heading < 0.0, "reverse steering should flip, got {}", r.heading);
    }

    #[test]
    fn handbrake_breaks_traction_and_drifts() {
        let mut with_brake = car();
        let mut without_brake = car();
        let accel = CarInput { throttle: 1.0, ..Default::default() };
        for _ in 0..60 {
            step_car(&mut with_brake, &accel, 1.0 / 60.0);
            step_car(&mut without_brake, &accel, 1.0 / 60.0);
        }
        // Enter a full lock while sliding: one car handbrakes, one grips.
        let drifting = CarInput { throttle: 0.2, steer: 1.0, handbrake: true, ..Default::default() };
        let gripping = CarInput { throttle: 0.2, steer: 1.0, handbrake: false, ..Default::default() };
        for _ in 0..30 {
            step_car(&mut with_brake, &drifting, 1.0 / 60.0);
            step_car(&mut without_brake, &gripping, 1.0 / 60.0);
        }
        assert!(
            with_brake.slip().abs() > without_brake.slip().abs() + 10.0,
            "handbrake should slide more ({} vs {})",
            with_brake.slip(),
            without_brake.slip()
        );
    }

    #[test]
    fn full_brake_stops_a_crawl_within_two_frames() {
        let mut c = car();
        // Creeping forward at 12 px/s (parking speed).
        c.vx = 12.0;
        let brake = CarInput { throttle: -1.0, ..Default::default() };
        step_car(&mut c, &brake, 1.0 / 60.0);
        // A full brake must kill a 12 px/s crawl in a single frame. The old
        // 0-20 px/s dead zone routed crawls through the slow reverse ramp
        // (12 -> ~6 px/s) and could not.
        assert!(
            c.forward_speed() <= 0.0,
            "crawl should be braked to a stop in one frame (fwd {})",
            c.forward_speed()
        );
    }

    #[test]
    fn brake_from_speed_reaches_a_stop() {
        let mut c = car();
        let go = CarInput { throttle: 1.0, ..Default::default() };
        for _ in 0..60 {
            step_car(&mut c, &go, 1.0 / 60.0);
        }
        assert!(c.forward_speed() > 200.0, "need speed first: {}", c.forward_speed());
        let brake = CarInput { throttle: -1.0, ..Default::default() };
        for _ in 0..24 {
            step_car(&mut c, &brake, 1.0 / 60.0);
        }
        // Full brake must take the car from highway speed down through zero
        // (a little reverse creep is expected: S is still held).
        assert!(
            c.forward_speed() < 20.0,
            "full brake should stop the car (fwd {})",
            c.forward_speed()
        );
    }

    #[test]
    fn car_stays_steerable_at_crawl_speed() {
        let mut c = car();
        // Light throttle = a parking-like crawl (~15 px/s) with full lock.
        let crawl = CarInput { throttle: 0.1, steer: 1.0, ..Default::default() };
        for _ in 0..60 {
            step_car(&mut c, &crawl, 1.0 / 60.0);
        }
        assert!(c.heading > 0.5, "crawl-speed steering should turn the car ({} rad)", c.heading);
        assert!(c.speed() < 40.0, "should still be crawling: {} px/s", c.speed());
    }

    #[test]
    fn stopped_car_cannot_spin_in_place() {
        let mut c = car();
        let spin = CarInput { throttle: 0.0, steer: 1.0, ..Default::default() };
        for _ in 0..60 {
            step_car(&mut c, &spin, 1.0 / 60.0);
        }
        assert_eq!(c.heading, 0.0, "a stopped car must not rotate");
        assert_eq!(c.speed(), 0.0, "a stopped car must not move");
    }

    #[test]
    fn speed_cannot_exceed_max() {
        let mut c = car();
        let inp = CarInput { throttle: 1.0, boost: true, ..Default::default() };
        for _ in 0..2400 {
            step_car(&mut c, &inp, 1.0 / 60.0);
        }
        assert!(c.speed() <= MAX_SPEED * 1.28 + 1.0);
    }

    #[test]
    fn car_cannot_drive_through_buildings() {
        use crate::city::{City, ROAD};
        let city = City::new(1);
        // Start on a road and drive east into the first block.
        let mut c = car();
        c.x = ROAD / 2.0;
        c.y = ROAD / 2.0;
        c.heading = 0.0;
        let inp = CarInput { throttle: 1.0, ..Default::default() };
        for _ in 0..600 {
            step_car(&mut c, &inp, 1.0 / 60.0);
            collide_car_with_city(&mut c, &city);
            // Car center must never end up inside a building.
            for b in city.buildings() {
                assert!(
                    !b.contains(c.x, c.y),
                    "car tunneled into building at ({},{})",
                    c.x,
                    c.y
                );
            }
        }
    }
}
