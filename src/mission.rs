//! Missions: a simple fetch-and-deliver loop.
//! Pick up the package at the yellow marker, deliver it to the green marker
//! before the timer runs out. Reward scales with remaining time.

use crate::Rng;
use crate::city::City;

pub const DELIVERY_TIME: f64 = 75.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MissionPhase {
    ToPickup,
    ToDeliver,
    /// Brief pause after completion before the next mission spawns.
    Cooldown(f64),
}

/// Something notable happened (HUD message / money / audio).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MissionEvent {
    pub msg: &'static str,
    pub reward: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Mission {
    pub phase: MissionPhase,
    pub pickup: (f64, f64),
    pub deliver: (f64, f64),
    /// Seconds left in the current delivery (only meaningful in ToDeliver).
    pub time_left: f64,
    /// Missions completed so far.
    pub completed: u32,
}

impl Mission {
    pub fn new(rng: &mut Rng, city: &City, px: f64, py: f64) -> Self {
        let pickup = city.random_road_point(rng, px, py, 400.0);
        let mut m = Mission {
            phase: MissionPhase::ToPickup,
            pickup,
            deliver: (0.0, 0.0),
            time_left: 0.0,
            completed: 0,
        };
        m.new_delivery(rng, city);
        m
    }

    pub fn new_delivery(&mut self, rng: &mut Rng, city: &City) {
        self.deliver = city.random_road_point(rng, self.pickup.0, self.pickup.1, 700.0);
        self.time_left = DELIVERY_TIME;
    }

    /// Marker the player currently has to reach (None during cooldown).
    pub fn current_marker(&self) -> Option<(f64, f64)> {
        match self.phase {
            MissionPhase::ToPickup => Some(self.pickup),
            MissionPhase::ToDeliver => Some(self.deliver),
            MissionPhase::Cooldown(_) => None,
        }
    }

    /// Progress the mission. `px, py` is the player position.
    pub fn update(
        &mut self,
        dt: f64,
        rng: &mut Rng,
        city: &City,
        px: f64,
        py: f64,
    ) -> Option<MissionEvent> {
        match self.phase {
            MissionPhase::Cooldown(t) => {
                let t = t - dt;
                if t <= 0.0 {
                    self.phase = MissionPhase::ToPickup;
                    self.pickup = city.random_road_point(rng, px, py, 500.0);
                    self.new_delivery(rng, city);
                    Some(MissionEvent {
                        msg: "NEW PACKAGE AVAILABLE",
                        reward: 0,
                    })
                } else {
                    self.phase = MissionPhase::Cooldown(t);
                    None
                }
            }
            MissionPhase::ToPickup => {
                let d = dist(self.pickup, (px, py));
                if d < 60.0 {
                    self.phase = MissionPhase::ToDeliver;
                    Some(MissionEvent {
                        msg: "PACKAGE PICKED UP — DELIVER IT!",
                        reward: 0,
                    })
                } else {
                    None
                }
            }
            MissionPhase::ToDeliver => {
                self.time_left -= dt;
                let d = dist(self.deliver, (px, py));
                if d < 60.0 {
                    let reward = Mission::reward_for(self.time_left);
                    self.completed += 1;
                    self.phase = MissionPhase::Cooldown(4.0);
                    Some(MissionEvent {
                        msg: "DELIVERED!",
                        reward,
                    })
                } else if self.time_left <= 0.0 {
                    self.phase = MissionPhase::Cooldown(4.0);
                    Some(MissionEvent {
                        msg: "MISSION FAILED — DELIVERY TOO SLOW",
                        reward: 0,
                    })
                } else {
                    None
                }
            }
        }
    }

    pub fn reward_for(time_left: f64) -> u32 {
        400 + (time_left.max(0.0) as u32) * 10
    }
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn city() -> City {
        City::new(1)
    }

    #[test]
    fn mission_starts_with_pickup_marker() {
        let mut rng = Rng::new(1);
        let c = city();
        let m = Mission::new(&mut rng, &c, 400.0, 400.0);
        assert_eq!(m.phase, MissionPhase::ToPickup);
        assert_ne!(m.pickup, (0.0, 0.0));
        assert!(m.current_marker().is_some());
    }

    #[test]
    fn pickup_then_delivery_then_reward() {
        let mut rng = Rng::new(2);
        let c = city();
        let mut m = Mission::new(&mut rng, &c, 0.0, 0.0);
        let ev = m
            .update(1.0 / 60.0, &mut rng, &c, m.pickup.0, m.pickup.1)
            .unwrap();
        assert!(ev.reward == 0);
        assert_eq!(m.phase, MissionPhase::ToDeliver);
        assert!(m.time_left <= DELIVERY_TIME && m.time_left > 0.0);
        // Deliver immediately.
        let ev = m
            .update(1.0 / 60.0, &mut rng, &c, m.deliver.0, m.deliver.1)
            .unwrap();
        assert!(ev.msg == "DELIVERED!");
        assert!(ev.reward >= 400);
        match m.phase {
            MissionPhase::Cooldown(t) => assert!(t > 0.0),
            _ => panic!("expected cooldown"),
        }
        assert_eq!(m.completed, 1);
        // Cooldown ends -> new pickup phase.
        for _ in 0..600 {
            m.update(1.0 / 60.0, &mut rng, &c, 500.0, 500.0);
        }
        assert_eq!(m.phase, MissionPhase::ToPickup);
    }

    #[test]
    fn delivery_timeout_fails_mission() {
        let mut rng = Rng::new(3);
        let c = city();
        let mut m = Mission::new(&mut rng, &c, 0.0, 0.0);
        m.phase = MissionPhase::ToDeliver;
        m.time_left = 1.0;
        let mut ev = None;
        for _ in 0..120 {
            if let Some(e) = m.update(1.0 / 60.0, &mut rng, &c, 5000.0, 5000.0) {
                ev = Some(e);
            }
        }
        assert!(ev.is_some());
        assert!(ev.unwrap().msg.contains("FAILED"));
        assert_eq!(m.completed, 0);
    }

    #[test]
    fn reward_scales_with_time() {
        assert!(Mission::reward_for(70.0) > Mission::reward_for(10.0));
        assert_eq!(Mission::reward_for(0.0), 400);
        assert_eq!(Mission::reward_for(-5.0), 400);
    }
}
