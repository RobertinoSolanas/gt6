//! The overall game state and per-tick update. Pure Rust (no DOM), so the
//! whole simulation is unit-testable on the host.

use crate::Rng;
use crate::car::{Car, CarKind, collide_car_with_city, step_car};
use crate::city::City;
use crate::input::Input;
use crate::mission::{Mission, MissionEvent};
use crate::ped::Ped;
use crate::traffic::TrafficCar;

const DT: f64 = 1.0 / 60.0;
const PED_COUNT: usize = 50;
const TRAFFIC_COUNT: usize = 26;
const FOOT_SPEED: f64 = 130.0;
const RUN_SPEED: f64 = 250.0;
const FOOT_RADIUS: f64 = 10.0;
const ROAD_HALF: f64 = crate::city::ROAD / 2.0;

pub struct GameState {
    pub city: City,
    pub rng: Rng,
    pub time: f64,

    // Player
    pub car: Car,
    pub on_foot: bool,
    pub foot_x: f64,
    pub foot_y: f64,
    pub foot_heading: f64,

    // World
    pub peds: Vec<Ped>,
    pub traffic: Vec<TrafficCar>,
    pub police: Vec<Car>,

    // Wanted
    pub heat: f64,
    pub last_crime: f64,

    // Meta
    pub money: u32,
    pub mission: Mission,
    pub msg: Option<(String, f64)>, // text + expiry
    pub paused: bool,
    pub busted_until: f64, // > time while the "BUSTED" screen is shown

    // Camera (world coords)
    pub cam_x: f64,
    pub cam_y: f64,
}

/// Events that need DOM/audio side effects (emitted by update).
pub enum Event {
    Crash,
    PedHit,
    PoliceHit,
    Mission(MissionEvent),
    Busted,
    EnterCar,
    ExitCar,
}

impl GameState {
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let city = City::new(seed);
        let (sx, sy) = City::intersection_pos(2, 2);
        // Spawn on the road, just below the intersection.
        let car = Car::new(sx, sy + ROAD_HALF, 0.0, CarKind::Player);
        let mission = Mission::new(&mut rng, &city, car.x, car.y);

        let mut peds = Vec::new();
        for _ in 0..PED_COUNT {
            peds.push(Ped::spawn(&mut rng));
        }
        let mut traffic = Vec::new();
        for _ in 0..TRAFFIC_COUNT {
            traffic.push(TrafficCar::spawn(&mut rng));
        }

        let mut s = GameState {
            city,
            rng,
            time: 0.0,
            car,
            on_foot: false,
            foot_x: sx,
            foot_y: sy,
            foot_heading: 0.0,
            peds,
            traffic,
            police: Vec::new(),
            heat: 0.0,
            last_crime: -100.0,
            money: 100,
            mission,
            msg: Some((String::from("GO GET THE YELLOW MARKER"), 8.0)),
            paused: false,
            busted_until: 0.0,
            cam_x: car.x,
            cam_y: car.y,
        };
        s.set_msg("DELIVER PACKAGES. DON'T GET CAUGHT.", 8.0);
        s
    }

    pub fn set_msg(&mut self, text: &str, dur: f64) {
        self.msg = Some((text.to_string(), self.time + dur));
    }

    pub fn stars(&self) -> u32 {
        crate::police::stars(self.heat)
    }

    /// Player position (car or foot).
    pub fn player_pos(&self) -> (f64, f64) {
        if self.on_foot {
            (self.foot_x, self.foot_y)
        } else {
            (self.car.x, self.car.y)
        }
    }

    /// Player speed (px/s) — used for run-overs and bust checks.
    pub fn player_speed(&self) -> f64 {
        if self.on_foot {
            RUN_SPEED // on foot we never "run over" peds
        } else {
            self.car.speed()
        }
    }

    /// Commit one fixed 60 Hz tick. Returns events for the audio layer.
    pub fn tick(&mut self, input: &mut Input) -> Vec<Event> {
        let mut events = Vec::new();
        if self.paused {
            return events;
        }
        self.time += DT;

        // Busted screen: world keeps running, player can't act.
        if self.time < self.busted_until {
            let (bpx, bpy) = self.player_pos();
            self.update_police_and_heat(&mut events, bpx, bpy);
            self.update_peds(0.0);
            input.end_frame();
            return events;
        }

        // ---- Player ----
        let (px, py) = if self.on_foot {
            self.update_foot(input);
            (self.foot_x, self.foot_y)
        } else {
            let inp = input.car_controls();
            step_car(&mut self.car, &inp, DT);
            if collide_car_with_city(&mut self.car, &self.city) {
                if self.car.speed() > 150.0 {
                    events.push(Event::Crash);
                }
            }
            (self.car.x, self.car.y)
        };

        if input.just_pressed("p") {
            self.paused = true;
        }
        if input.just_pressed("r") {
            self.cam_x = px;
            self.cam_y = py;
        }

        // ---- Enter / exit vehicle ----
        if input.just_pressed("e") {
            if self.on_foot {
                let d = dist((px, py), (self.car.x, self.car.y));
                if d < 70.0 {
                    self.on_foot = false;
                    events.push(Event::EnterCar);
                    self.set_msg("VEHICLE ACQUIRED", 2.0);
                }
            } else {
                if self.car.speed() < 40.0 {
                    // Step out to the side of the car.
                    let side = self.car.heading + std::f64::consts::FRAC_PI_2;
                    let mut fx = self.car.x + side.cos() * 36.0;
                    let mut fy = self.car.y + side.sin() * 36.0;
                    if let Some((x, y, _, _)) = self.city.collide_circle(fx, fy, FOOT_RADIUS) {
                        fx = x;
                        fy = y;
                    }
                    self.foot_x = fx;
                    self.foot_y = fy;
                    self.foot_heading = self.car.heading;
                    self.on_foot = true;
                    events.push(Event::ExitCar);
                } else {
                    self.set_msg("STOP THE VEHICLE FIRST", 1.5);
                }
            }
        }

        // ---- Pedestrians ----
        let threat_speed = if self.on_foot { 0.0 } else { self.car.speed() };
        self.update_peds(threat_speed);
        self.ped_collisions(&mut events, px, py, threat_speed);

        // ---- Traffic ----
        for t in self.traffic.iter_mut() {
            t.update(DT, &mut self.rng, &self.city, px, py);
        }
        self.traffic_collisions(&mut events, px, py);

        // ---- Police & wanted ----
        self.update_police_and_heat(&mut events, px, py);

        // ---- Mission ----
        if let Some(ev) = self.mission.update(DT, &mut self.rng, &self.city, px, py) {
            if ev.reward > 0 {
                self.money = self.money.saturating_add(ev.reward);
            }
            self.set_msg(ev.msg, 3.5);
            events.push(Event::Mission(ev));
        }

        // ---- Message expiry ----
        if let Some((_, exp)) = self.msg {
            if self.time > exp {
                self.msg = None;
            }
        }

        // ---- Camera (smooth follow, with a little lead while driving) ----
        let (tx, ty) = if self.on_foot {
            (px, py)
        } else {
            (px + self.car.vx * 0.25, py + self.car.vy * 0.25)
        };
        let k = (1.0 - (-5.0 * DT).exp()).clamp(0.0, 1.0);
        self.cam_x += (tx - self.cam_x) * k;
        self.cam_y += (ty - self.cam_y) * k.max(0.08);

        input.end_frame();
        events
    }

    fn update_foot(&mut self, input: &Input) {
        let (dx, dy, run) = input.foot_controls();
        if dx != 0.0 || dy != 0.0 {
            let l = (dx * dx + dy * dy).sqrt();
            let spd = if run { RUN_SPEED } else { FOOT_SPEED };
            self.foot_x += dx / l * spd * DT;
            self.foot_y += dy / l * spd * DT;
            self.foot_heading = dy.atan2(dx);
            if let Some((x, y, _, _)) = self.city.collide_circle(self.foot_x, self.foot_y, FOOT_RADIUS) {
                self.foot_x = x;
                self.foot_y = y;
            }
        }
        // Peds don't flee a walker; still keep them off the player a bit.
        for p in self.peds.iter_mut() {
            let d = dist((self.foot_x, self.foot_y), (p.x, p.y));
            if d < 16.0 && d > 0.001 {
                let push = (16.0 - d) / 2.0;
                let nx = (p.x - self.foot_x) / d;
                let ny = (p.y - self.foot_y) / d;
                p.x += nx * push;
                p.y += ny * push;
            }
        }
    }

    fn update_peds(&mut self, threat_speed: f64) {
        let (px, py) = self.player_pos();
        let tp = if threat_speed > 0.0 { (px, py) } else { (f64::INFINITY, 0.0) };
        for i in 0..self.peds.len() {
            self.peds[i].update(DT, &mut self.rng, tp.0, tp.1, threat_speed);
            // Nudge peds out of buildings (they shouldn't walk into them,
            // but fleeing can push them anywhere).
            if let Some((x, y, _, _)) = self.city.collide_circle(self.peds[i].x, self.peds[i].y, 6.0) {
                self.peds[i].x = x;
                self.peds[i].y = y;
            }
            // Recycle long-dead peds.
            if let crate::ped::PedState::Dead(t) = self.peds[i].state {
                if self.time - t > 20.0 {
                    self.peds[i] = Ped::spawn(&mut self.rng);
                }
            }
        }
    }

    /// Run-over check: fast car + close ped = dead ped + wanted star.
    fn ped_collisions(&mut self, events: &mut Vec<Event>, px: f64, py: f64, threat_speed: f64) {
        if self.on_foot || threat_speed < 90.0 {
            return;
        }
        let hit = self.peds.iter_mut().any(|p| {
            if !matches!(p.state, crate::ped::PedState::Alive) {
                return false;
            }
            let d = dist((px, py), (p.x, p.y));
            if d < self.car.radius + 8.0 {
                p.kill(self.time);
                return true;
            }
            false
        });
        if hit {
            self.add_crime(0.9);
            events.push(Event::PedHit);
            self.set_msg("CITIZEN HARMED", 2.0);
        }
    }

    /// Push traffic out of the player circle (or vice versa) and raise heat
    /// on fast impacts.
    fn traffic_collisions(&mut self, events: &mut Vec<Event>, px: f64, py: f64) {
        let pr = if self.on_foot { FOOT_RADIUS } else { self.car.radius };
        let on_foot = self.on_foot;
        let impact = if on_foot { 0.0 } else { self.car.speed() };
        let mut fast_hit = false;
        for t in self.traffic.iter_mut() {
            let d = dist((px, py), (t.car.x, t.car.y));
            let min = pr + t.car.radius;
            if d < min && d > 0.001 {
                let nx = (t.car.x - px) / d;
                let ny = (t.car.y - py) / d;
                let push = min - d;
                t.car.x += nx * push * 0.8;
                t.car.y += ny * push * 0.8;
                if !on_foot {
                    self.car.x -= nx * push * 0.4;
                    self.car.y -= ny * push * 0.4;
                    // Simple momentum exchange: dampen both.
                    self.car.vx *= 0.85;
                    self.car.vy *= 0.85;
                } else {
                    self.foot_x -= nx * push * 0.5;
                    self.foot_y -= ny * push * 0.5;
                }
                if impact > 120.0 {
                    fast_hit = true;
                }
            }
        }
        if fast_hit {
            self.add_crime(0.4);
            events.push(Event::PoliceHit);
        }
    }

    fn update_police_and_heat(&mut self, events: &mut Vec<Event>, px: f64, py: f64) {
        let s = self.stars();

        // Keep the police squad sized to the star level.
        let want = s as usize;
        while self.police.len() < want {
            let extra = crate::police::spawn_police(&mut self.rng, &self.city, px, py, 1);
            self.police.extend(extra);
        }
        if self.police.len() > want {
            self.police.truncate(want);
        }

        let mut caught = false;
        if !self.police.is_empty() {
            let catch = if self.on_foot { 26.0 } else { 40.0 };
            caught = crate::police::update_police(
                &mut self.police,
                &self.city,
                px,
                py,
                s,
                DT,
                catch,
            );
            // Bust: caught while slow (in car) or caught on foot at all.
            let slow = if self.on_foot { true } else { self.car.speed() < 40.0 };
            if caught && slow {
                self.busted();
                events.push(Event::Busted);
            }
        }

        // Heat decay.
        let nearest = crate::police::nearest_police(&self.police, px, py);
        self.heat = crate::police::decay_heat(
            self.heat,
            DT,
            self.time - self.last_crime,
            nearest,
        );
        let _ = caught;
    }

    fn busted(&mut self) {
        let fine = (self.money as f64 * 0.25).round() as u32;
        self.money -= fine.min(self.money);
        self.heat = 0.0;
        self.police.clear();
        let (sx, sy) = City::intersection_pos(2, 2);
        self.car.x = sx;
        self.car.y = sy + ROAD_HALF;
        self.car.vx = 0.0;
        self.car.vy = 0.0;
        self.on_foot = false;
        self.foot_x = sx;
        self.foot_y = sy;
        self.busted_until = self.time + 2.5;
        self.set_msg(&format!("BUSTED — FINE ${}", fine), 2.5);
    }

    fn add_crime(&mut self, amount: f64) {
        self.heat = crate::police::add_heat(self.heat, amount);
        self.last_crime = self.time;
    }
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Fixed-timestep loop: accumulate real time, run whole ticks.
/// Returns all events emitted this frame (for the audio layer).
pub fn step(
    state: &mut GameState,
    input: &mut Input,
    acc: &mut f64,
    real_dt: f64,
) -> Vec<Event> {
    *acc += real_dt.min(0.1);
    let mut events = Vec::new();
    while *acc >= DT {
        events.extend(state.tick(input));
        *acc -= DT;
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::city::SIZE;
    use crate::input::Input;

    fn idle_state() -> GameState {
        GameState::new(42)
    }

    fn keypress(input: &mut Input, k: &str) {
        input.key_down(k);
    }

    #[test]
    fn fresh_state_is_sane() {
        let s = idle_state();
        assert!(s.money > 0);
        assert_eq!(s.stars(), 0);
        assert!(!s.on_foot);
        assert!(s.peds.len() == PED_COUNT);
        assert!(s.traffic.len() == TRAFFIC_COUNT);
        assert!(s.city.is_road(s.car.x, s.car.y));
    }

    #[test]
    fn holding_w_makes_the_car_go() {
        let mut s = idle_state();
        let mut inp = Input::new();
        keypress(&mut inp, "w");
        let start_x = s.car.x;
        for _ in 0..120 {
            s.tick(&mut inp);
        }
        inp.key_up("w");
        assert!(s.car.x > start_x, "car should move forward (W)");
        assert!(s.car.speed() > 50.0);
    }

    #[test]
    fn arrow_keys_drive_too() {
        let mut s = idle_state();
        let mut inp = Input::new();
        keypress(&mut inp, "arrowup");
        let start = s.car.speed();
        for _ in 0..120 {
            s.tick(&mut inp);
        }
        assert!(s.car.speed() > start + 50.0, "arrow keys should throttle");
    }

    #[test]
    fn e_toggles_enter_exit_car() {
        let mut s = idle_state();
        let mut inp = Input::new();
        for _ in 0..30 {
            s.tick(&mut inp);
        }
        keypress(&mut inp, "e"); // exit
        for _ in 0..5 {
            s.tick(&mut inp);
        }
        assert!(s.on_foot, "E should put the player on foot");
        inp.key_up("e");
        keypress(&mut inp, "e"); // re-enter
        for _ in 0..5 {
            s.tick(&mut inp);
        }
        assert!(!s.on_foot, "E near a stopped car should re-enter it");
    }

    #[test]
    fn on_foot_walking_moves_player() {
        let mut s = idle_state();
        let mut inp = Input::new();
        for _ in 0..10 {
            s.tick(&mut inp);
        }
        keypress(&mut inp, "e");
        for _ in 0..5 {
            s.tick(&mut inp);
        }
        assert!(s.on_foot);
        keypress(&mut inp, "w");
        let sy = s.foot_y;
        for _ in 0..60 {
            s.tick(&mut inp);
        }
        assert!((s.foot_y - sy).abs() > 30.0, "walking with W should move");
    }

    #[test]
    fn running_over_a_ped_raises_wanted() {
        let mut s = idle_state();
        // Teleport the player car next to a living ped.
        let ped_idx = s.peds.iter().position(|p| matches!(p.state, crate::ped::PedState::Alive)).unwrap();
        let (px, py) = (s.peds[ped_idx].x, s.peds[ped_idx].y);
        s.car.x = px - 30.0;
        s.car.y = py;
        s.car.heading = 0.0;
        s.car.vx = 300.0;
        s.car.vy = 0.0;
        let mut inp = Input::new();
        for _ in 0..30 {
            s.tick(&mut inp);
        }
        assert!(s.heat > 0.0, "running over a ped should add heat, got {}", s.heat);
        assert_eq!(s.stars(), 1);
    }

    #[test]
    fn police_spawn_with_stars_and_heat_decays() {
        let mut s = idle_state();
        s.add_crime(1.0);
        let mut inp = Input::new();
        for _ in 0..60 {
            s.tick(&mut inp);
        }
        assert!(!s.police.is_empty(), "1 star should spawn police");
        // Drive far away (teleport) and wait: heat should eventually clear.
        s.car.x = SIZE - 100.0;
        s.car.y = SIZE - 100.0;
        for _ in 0..60 * 30 {
            s.tick(&mut inp);
        }
        assert!(s.heat == 0.0, "heat should decay when police are far, got {}", s.heat);
    }

    #[test]
    fn mission_payout_adds_money() {
        let mut s = idle_state();
        let mut inp = Input::new();
        // Teleport onto the pickup.
        let (mx, my) = s.mission.pickup;
        s.car.x = mx;
        s.car.y = my;
        for _ in 0..10 {
            s.tick(&mut inp);
        }
        assert_eq!(s.mission.phase, crate::mission::MissionPhase::ToDeliver);
        // Teleport onto the delivery point.
        let (dx, dy) = s.mission.deliver;
        s.car.x = dx;
        s.car.y = dy;
        let money_before = s.money;
        for _ in 0..10 {
            s.tick(&mut inp);
        }
        assert!(s.money > money_before, "delivery should pay money");
    }

    #[test]
    fn busted_loses_money_and_clears_heat() {
        let mut s = idle_state();
        s.add_crime(5.0);
        s.money = 1000;
        let mut inp = Input::new();
        // Put police on top of the stationary player -> bust.
        let (px, py) = s.player_pos();
        s.police = crate::police::spawn_police(&mut s.rng, &s.city, px, py, 1);
        s.police[0].x = px + 10.0;
        s.police[0].y = py;
        s.police[0].vx = 0.0;
        s.police[0].vy = 0.0;
        for _ in 0..30 {
            s.tick(&mut inp);
        }
        assert!(s.time < s.busted_until || s.heat == 0.0, "should have been busted");
        if s.time < s.busted_until {
            assert!(s.money < 1000);
        }
    }

    #[test]
    fn pause_freezes_time() {
        let mut s = idle_state();
        let mut inp = Input::new();
        for _ in 0..10 {
            s.tick(&mut inp);
        }
        keypress(&mut inp, "p");
        for _ in 0..5 {
            s.tick(&mut inp);
        }
        assert!(s.paused);
        let t = s.time;
        for _ in 0..10 {
            s.tick(&mut inp);
        }
        assert_eq!(s.time, t, "time must not advance while paused");
    }
}
