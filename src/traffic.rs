//! AI traffic: cars that follow the road grid in right-hand lanes and make
//! random decisions at intersections. Kinematic (lane-following), so they
//! never clip buildings.
//!
//! Road indices 0..=N run along both axes. The intersection of vertical road
//! `i` and horizontal road `j` sits at `(i*CELL+ROAD/2, j*CELL+ROAD/2)`.

use crate::Rng;
use crate::car::{Car, CarKind};
use crate::city::{City, CELL, LANES, ROAD, SIZE};

/// 0 = North, 1 = East, 2 = South, 3 = West
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    N,
    E,
    S,
    W,
}

impl Dir {
    pub fn from_usize(d: usize) -> Dir {
        match d % 4 {
            0 => Dir::N,
            1 => Dir::E,
            2 => Dir::S,
            _ => Dir::W,
        }
    }
    pub fn as_usize(self) -> usize {
        match self {
            Dir::N => 0,
            Dir::E => 1,
            Dir::S => 2,
            Dir::W => 3,
        }
    }
    /// Heading radians.
    pub fn angle(self) -> f64 {
        match self {
            Dir::N => -std::f64::consts::FRAC_PI_2,
            Dir::E => 0.0,
            Dir::S => std::f64::consts::FRAC_PI_2,
            Dir::W => std::f64::consts::PI,
        }
    }
    /// Right-hand lane offset, perpendicular to the direction of travel.
    fn lane_offset(self) -> f64 {
        match self {
            Dir::N => 20.0,
            Dir::E => 20.0,
            Dir::S => -20.0,
            Dir::W => -20.0,
        }
    }
    pub fn vertical(self) -> bool {
        matches!(self, Dir::N | Dir::S)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TrafficCar {
    pub car: Car,
    pub dir: Dir,
    /// Road index of the road this car currently drives on.
    pub road: usize,
    pub base_speed: f64,
    /// True while the car is stopped (e.g. player in the way).
    pub stopped: bool,
}

const TRAFFIC_COLORS: [u32; 7] = [
    0x457b9d, 0xe9c46a, 0x8d99ae, 0xf4a261, 0x6a994e, 0x9d8189, 0x264653,
];

/// Road-center coordinate for index `i`.
fn road_center(i: usize) -> f64 {
    i as f64 * CELL + ROAD / 2.0
}

/// Index of the road whose centerline is closest to coordinate `c`.
fn nearest_road(c: f64) -> usize {
    (((c - ROAD / 2.0) / CELL).round() as isize).clamp(0, LANES as isize - 1) as usize
}

impl TrafficCar {
    pub fn spawn(rng: &mut Rng) -> Self {
        let dir = Dir::from_usize(rng.below(4));
        let road = rng.below(LANES);
        let lane = dir.lane_offset();
        let (x, y) = if dir.vertical() {
            (road_center(road) + lane, rng.range(ROAD, SIZE - ROAD))
        } else {
            (rng.range(ROAD, SIZE - ROAD), road_center(road) + lane)
        };
        let mut car = Car::new(x, y, dir.angle(), CarKind::Traffic);
        car.color = TRAFFIC_COLORS[rng.below(TRAFFIC_COLORS.len())];
        TrafficCar {
            car,
            dir,
            road,
            base_speed: rng.range(120.0, 180.0),
            stopped: false,
        }
    }

    pub fn update(&mut self, dt: f64, rng: &mut Rng, _city: &City, player_x: f64, player_y: f64) {
        // Smoothly rotate the sprite toward the lane heading.
        let target = self.dir.angle();
        let mut diff = (target - self.car.heading).rem_euclid(std::f64::consts::TAU);
        if diff > std::f64::consts::PI {
            diff -= std::f64::consts::TAU;
        }
        let max_turn = 6.0 * dt;
        self.car.heading += diff.clamp(-max_turn, max_turn);

        // Stop if the player is close ahead ("honor" check).
        let dx = player_x - self.car.x;
        let dy = player_y - self.car.y;
        let ahead = dx * self.car.heading.cos() + dy * self.car.heading.sin();
        let side = -dx * self.car.heading.sin() + dy * self.car.heading.cos();
        let in_way = ahead > 0.0 && ahead < 110.0 && side.abs() < 55.0;
        self.stopped = in_way;
        let speed = if in_way { 0.0 } else { self.base_speed };

        let old_dir_is_vertical = self.dir.vertical();
        let step = speed * dt;
        let (mut x, mut y) = (self.car.x, self.car.y);
        match self.dir {
            Dir::N => y -= step,
            Dir::E => x += step,
            Dir::S => y += step,
            Dir::W => x -= step,
        }

        // Which intersection are we on the road of? It's the crossing index.
        let crossing = if self.dir.vertical() {
            nearest_road(self.car.y) // horizontal road index
        } else {
            nearest_road(self.car.x) // vertical road index
        };
        let cross_center = road_center(crossing);

        // Did we cross the intersection center on this move?
        let old_along = if self.dir.vertical() { self.car.y } else { self.car.x };
        let new_along = if self.dir.vertical() { y } else { x };
        let passed = match self.dir {
            Dir::N => old_along > cross_center && new_along <= cross_center,
            Dir::S => old_along < cross_center && new_along >= cross_center,
            Dir::E => old_along < cross_center && new_along >= cross_center,
            Dir::W => old_along > cross_center && new_along <= cross_center,
        };

        if passed {
            let r = rng.f();
            let turn = if r < 0.22 { -1i8 } else if r < 0.45 { 1 } else { 0 };
            if turn != 0 {
                // Turn around this intersection: the new road is the crossing
                // road, and we snap onto its right-hand lane.
                let old_road = self.road;
                let new_dir =
                    Dir::from_usize((self.dir.as_usize() + if turn > 0 { 1 } else { 3 }) % 4);
                self.dir = new_dir;
                self.road = crossing;
                // Center of the intersection we are turning around.
                let (ix, iy) = if old_dir_is_vertical {
                    (road_center(old_road), road_center(crossing))
                } else {
                    (road_center(crossing), road_center(old_road))
                };
                let lane = self.dir.lane_offset();
                if self.dir.vertical() {
                    self.car.x = ix + lane;
                    self.car.y = iy;
                } else {
                    self.car.x = ix;
                    self.car.y = iy + lane;
                }
            } else {
                // Continue straight: re-center on our lane.
                let lane = self.dir.lane_offset();
                if self.dir.vertical() {
                    self.car.x = road_center(self.road) + lane;
                } else {
                    self.car.y = road_center(self.road) + lane;
                }
            }
            return;
        }

        self.car.x = x;
        self.car.y = y;
        // Keep kinematic velocity in sync (for collision responses).
        self.car.vx = self.car.heading.cos() * speed;
        self.car.vy = self.car.heading.sin() * speed;

        // Wrap around the city edges.
        if self.car.x < 10.0 || self.car.x > SIZE - 10.0 || self.car.y < 10.0 || self.car.y > SIZE - 10.0 {
            *self = TrafficCar::spawn(rng);
        }
    }
}
