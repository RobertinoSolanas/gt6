//! The overall game state and per-tick update. Pure Rust (no DOM), so the
//! whole simulation is unit-testable on the host.

use crate::Rng;
use crate::car::{Car, CarKind, collide_car_with_city, step_car, step_plane};
use crate::city::City;
use crate::fx::Fx;
use crate::input::Input;
use crate::mission::{Mission, MissionEvent};
use crate::ped::Ped;
use crate::traffic::TrafficCar;
use crate::wildlife::Wildlife;

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
    /// The city airplane (parked at the airfield). `in_plane` = the player
    /// is currently the pilot of it.
    pub plane: Car,
    pub in_plane: bool,
    /// The dragon. `in_dragon` = the player is riding the dragon and
    /// piloting it ("D" to toggle): the player's position/altitude become
    /// the dragon's and the ground is left behind.
    pub in_dragon: bool,
    pub on_foot: bool,
    pub foot_x: f64,
    pub foot_y: f64,
    pub foot_heading: f64,
    /// Index into `wildlife.elephants` of the elephant the player is riding
    /// (Z to mount / dismount). The elephant wanders on its own and carries
    /// the player on its back.
    pub riding: Option<usize>,

    // World
    pub peds: Vec<Ped>,
    pub traffic: Vec<TrafficCar>,
    pub police: Vec<Car>,
    pub wildlife: Wildlife,
    /// The dragon's GLB mesh, baked and loaded at runtime (see `boot.rs`
    /// and `glb.rs`). `None` until the async load completes (or on load
    /// failure — the renderers then fall back to a low-poly silhouette).
    pub dragon_mesh: Option<crate::wildlife::DragonMesh>,
    /// Particle effects (tire smoke, sparks, dust, glitter). Pure data;
    /// the renderers draw it.
    pub fx: Fx,

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

    // Plane mouse controls: cruise throttle (0..1) set by the mouse wheel,
    // and a decaying pitch stick fed by vertical mouse drag (a flick keeps
    // the nose moving for a moment instead of being a one-frame blip).
    pub mouse_throttle: f64,
    pub plane_pitch_stick: f64,
    /// M auto-landing: autopilot flying the plane to `landing_target`.
    pub landing: bool,
    pub landing_target: (f64, f64),

    // View mode (V toggles)
    pub view_3d: bool,
    /// Smoothed chase-cam yaw for 3D mode (lags behind the player heading so
    /// turns feel fluid).
    pub cam3d_yaw: f64,
    /// User camera orbit offset (mouse-drag in 3D mode), added to the chase
    /// cam yaw so the player can look around.
    pub cam3d_orbit: f64,
    /// User camera pitch (mouse vertical-drag in 3D mode); radians, positive
    /// = looking up.
    pub cam3d_pitch: f64,
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
        // The airplane is parked on the north side of intersection (5, 5).
        let (ax, ay) = City::intersection_pos(5, 5);
        let plane = Car::new(ax, ay + 24.0, -std::f64::consts::FRAC_PI_2, CarKind::Plane);
        let mission = Mission::new(&mut rng, &city, car.x, car.y);

        let mut peds = Vec::new();
        for _ in 0..PED_COUNT {
            peds.push(Ped::spawn(&mut rng));
        }
        let mut traffic = Vec::new();
        for _ in 0..TRAFFIC_COUNT {
            traffic.push(TrafficCar::spawn(&mut rng));
        }
        let wildlife = Wildlife::new(&mut rng, sx, sy, 1200.0);

        let mut s = GameState {
            city,
            rng,
            time: 0.0,
            car,
            plane,
            in_plane: false,
            in_dragon: false,
            on_foot: false,
            foot_x: sx,
            foot_y: sy,
            foot_heading: 0.0,
            riding: None,
            peds,
            traffic,
            police: Vec::new(),
            wildlife,
            dragon_mesh: None,
            fx: Fx::new(),
            heat: 0.0,
            last_crime: -100.0,
            money: 100,
            mission,
            msg: Some((String::from("GO GET THE YELLOW MARKER"), 8.0)),
            paused: false,
            busted_until: 0.0,
            cam_x: car.x,
            cam_y: car.y,
            mouse_throttle: 1.0,
            plane_pitch_stick: 0.0,
            landing: false,
            landing_target: (ax, ay),
            view_3d: false,
            cam3d_yaw: car.heading,
            cam3d_orbit: 0.0,
            cam3d_pitch: 0.0,
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

    /// The vehicle the player is currently in (the car, or the plane).
    pub fn active_vehicle(&self) -> &Car {
        if self.in_plane {
            &self.plane
        } else {
            &self.car
        }
    }

    /// Player altitude above the streets (dragon or plane; 0 otherwise).
    pub fn player_alt(&self) -> f64 {
        if self.in_dragon {
            self.wildlife.dragon.z
        } else if self.in_plane {
            self.plane.z
        } else {
            0.0
        }
    }

    /// Player position (dragon, car, plane, foot, or the elephant ridden).
    pub fn player_pos(&self) -> (f64, f64) {
        if self.in_dragon {
            let d = &self.wildlife.dragon;
            (d.x, d.y)
        } else if self.on_foot || self.riding.is_some() {
            (self.foot_x, self.foot_y)
        } else {
            let v = self.active_vehicle();
            (v.x, v.y)
        }
    }

    /// Player speed (px/s) — used for run-overs, bust checks and the HUD.
    pub fn player_speed(&self) -> f64 {
        if self.in_dragon {
            self.wildlife.dragon.speed
        } else if let Some(idx) = self.riding {
            let e = &self.wildlife.elephants[idx];
            e.speed * e.gait // the elephant's current ground speed
        } else if self.on_foot {
            RUN_SPEED // on foot we never "run over" peds
        } else {
            self.active_vehicle().speed()
        }
    }

    /// Commit one fixed 60 Hz tick. Returns events for the audio layer.
    pub fn tick(&mut self, input: &mut Input) -> Vec<Event> {
        let mut events = Vec::new();
        if self.paused {
            return events;
        }
        self.time += DT;

        // Advance particle FX (smoke keeps dissipating even while busted).
        self.fx.update(DT);

        // ---- View mode & 3D chase-cam (smooth, shortest path) ----
        if input.just_pressed("v") {
            self.view_3d = !self.view_3d;
        }
        // Mouse-drag camera control (3D mode): horizontal drag orbits around
        // the player, vertical drag tilts the pitch; C resets to chase view.
        // In the plane (or on the dragon) the drag steers the craft instead.
        let (mdx, mdy) = input.mouse_delta();
        if self.view_3d && !self.in_plane && !self.in_dragon {
            self.cam3d_orbit += mdx * 0.005;
            self.cam3d_pitch = (self.cam3d_pitch - mdy * 0.004).clamp(-1.2, 0.6);
        }
        if input.just_pressed("c") {
            self.cam3d_orbit = 0.0;
            self.cam3d_pitch = 0.0;
        }
        let heading = if self.in_dragon {
            self.wildlife.dragon.heading
        } else if self.on_foot || self.riding.is_some() {
            self.foot_heading
        } else {
            self.car.heading
        };
        let kk = (1.0 - (-7.0 * DT).exp()).clamp(0.0, 1.0);
        self.cam3d_yaw = crate::cam3d::lerp_angle(self.cam3d_yaw, heading + self.cam3d_orbit, kk);

        // Busted screen: world keeps running, player can't act.
        if self.time < self.busted_until {
            let (bpx, bpy) = self.player_pos();
            self.update_police_and_heat(&mut events, bpx, bpy);
            self.update_peds(0.0);
            input.end_frame();
            return events;
        }

        // ---- F: summon the airplane and take the controls, anywhere ----
        if input.just_pressed("f") && !self.in_plane {
            let (px, py, ph) = if self.on_foot {
                (self.foot_x, self.foot_y, self.foot_heading)
            } else {
                (self.car.x, self.car.y, self.car.heading)
            };
            self.plane.x = px;
            self.plane.y = py;
            self.plane.z = 0.0;
            self.plane.heading = ph;
            self.plane.vx = 0.0;
            self.plane.vy = 0.0;
            self.plane.vz = 160.0; // gentle ease up off the street
            self.plane_pitch_stick = 0.0;
            self.in_plane = true;
            self.on_foot = false;
            self.riding = None; // summons the plane off the elephant's back
            self.set_msg("PLANE SUMMONED — DRAG: steer · LMB: throttle · WHEEL: speed", 4.5);
        }

        // ---- D: take the dragon's reins (or release it) ----
        // "D" is already a movement key in most states (steer the car/plane,
        // walk right on foot), so it only summons the dragon where it is
        // otherwise free: riding an elephant, in a (nearly) stopped car, or
        // while already on the dragon (to release it).
        let d_is_free = self.in_dragon
            || self.riding.is_some()
            || (!self.on_foot && !self.in_plane && self.car.speed() < 5.0);
        if input.just_pressed("d") && d_is_free {
            if self.in_dragon {
                // Release: drop to the street below, and hand the dragon back
                // to its own meander from the altitude it was left at.
                let (dx, dy, dz, dh) = (
                    self.wildlife.dragon.x,
                    self.wildlife.dragon.y,
                    self.wildlife.dragon.z,
                    self.wildlife.dragon.heading,
                );
                self.in_dragon = false;
                self.wildlife.dragon.z0 = dz;
                self.wildlife.dragon.controlled = false;
                self.on_foot = true;
                let (mut fx, mut fy) = (dx, dy);
                if let Some((x, y, _, _)) = self.city.collide_circle(fx, fy, FOOT_RADIUS) {
                    (fx, fy) = (x, y);
                }
                self.foot_x = fx;
                self.foot_y = fy;
                self.foot_heading = dh;
                self.set_msg("BACK ON THE GROUND — D: take the dragon again", 3.5);
            } else {
                self.in_dragon = true;
                self.wildlife.dragon.controlled = true;
                self.wildlife.dragon.vz = 0.0;
                self.view_3d = true; // you fly it best from the 3D chase cam
                self.on_foot = false;
                self.in_plane = false;
                self.riding = None;
                self.set_msg(
                    "DRAGON — W/S: speed · A/D or DRAG: turn · SHIFT/SPACE: climb/dive · D: release",
                    5.0,
                );
            }
        }

        // ---- M: auto-land at the nearest safe space (toggle) ----
        if input.just_pressed("m") && self.in_plane {
            if self.landing {
                self.landing = false;
                self.set_msg("AUTO-LAND CANCELLED", 2.0);
            } else {
                self.landing = true;
                self.landing_target =
                    City::nearest_intersection(self.plane.x, self.plane.y);
                self.set_msg("AUTO-LANDING TO NEAREST SAFE SPACE — M: CANCEL", 3.0);
            }
        }

        // ---- Z: mount / dismount an elephant ----
        if input.just_pressed("z") {
            if let Some(idx) = self.riding {
                // Jump off to the street beside the elephant, on foot.
                let e = self.wildlife.elephants[idx];
                let side = e.heading + std::f64::consts::FRAC_PI_2;
                let mut fx = e.x + side.cos() * (e.radius() + 16.0);
                let mut fy = e.y + side.sin() * (e.radius() + 16.0);
                if let Some((x, y, _, _)) = self.city.collide_circle(fx, fy, FOOT_RADIUS) {
                    fx = x;
                    fy = y;
                }
                self.foot_x = fx;
                self.foot_y = fy;
                self.foot_heading = e.heading;
                self.riding = None;
                self.on_foot = true;
                events.push(Event::ExitCar);
                self.set_msg("DROPPED TO THE STREET — ON FOOT", 2.0);
            } else if self.on_foot {
                // Mount the nearest elephant in reach; it wanders on its own.
                let mut best: Option<(usize, f64)> = None;
                for (i, e) in self.wildlife.elephants.iter().enumerate() {
                    let d = dist((self.foot_x, self.foot_y), (e.x, e.y));
                    if d <= 45.0 + e.radius() && best.map_or(true, |(_, bd)| d < bd) {
                        best = Some((i, d));
                    }
                }
                match best {
                    Some((i, _)) => {
                        let e = self.wildlife.elephants[i];
                        self.riding = Some(i);
                        self.on_foot = false;
                        self.foot_x = e.x;
                        self.foot_y = e.y;
                        self.foot_heading = e.heading;
                        events.push(Event::EnterCar);
                        self.set_msg("RIDING THE ELEPHANT — IT WANDERS ON ITS OWN · Z: JUMP OFF", 4.0);
                    }
                    None => {
                        self.set_msg("WALK UP TO AN ELEPHANT, THEN PRESS Z", 2.0);
                    }
                }
            }
        }

        // ---- Player ----
        let (px, py) = if self.in_dragon {
            // Piloting the dragon. Keyboard + mouse, mirroring the airplane:
            // LMB = full throttle, RMB = brake, drag = yaw + pitch (up = climb),
            // wheel = cruise throttle; W/S/A/D/Shift/Space still work and win.
            let mut inp = input.car_controls();
            if inp.throttle == 0.0 {
                inp.throttle = if input.mouse_right_state() {
                    -1.0
                } else if input.mouse_down_state() {
                    1.0
                } else {
                    self.mouse_throttle
                };
            }
            if inp.steer == 0.0 {
                inp.steer = (mdx * 0.022).clamp(-1.0, 1.0);
            }
            if inp.pitch == 0.0 {
                self.plane_pitch_stick += -mdy * 0.05;
                self.plane_pitch_stick =
                    (self.plane_pitch_stick.clamp(-1.0, 1.0) / (1.0 + 4.0 * DT)).max(-1.0);
                inp.pitch = self.plane_pitch_stick;
            } else {
                self.plane_pitch_stick = 0.0;
            }
            self.mouse_throttle =
                (self.mouse_throttle + input.wheel_delta() * 0.15).clamp(0.0, 1.0);
            let d = &mut self.wildlife.dragon;
            d.step_controlled(&inp, DT);
            // A thin contrail off the wingtips when fast and high, like the plane.
            let sp = d.speed;
            if d.z > 60.0 && sp > 260.0 && self.rng.below(3) == 0 {
                let (cx, cy) = (d.heading.cos(), d.heading.sin());
                for sy in [-1.0, 1.0] {
                    self.fx.smoke(&mut self.rng, d.x - cx * 10.0 - cy * sy * 24.0, d.y - cy * 10.0 + cx * sy * 24.0, d.z + 5.0, -cx * sp * 0.1, -cy * sp * 0.1, 5.0, 1.5, 2.4, 0xf4f7fa, 0.3);
                }
            }
            (d.x, d.y)
        } else if let Some(idx) = self.riding {
            // The elephant carries the player: stay glued to its back.
            let e = self.wildlife.elephants[idx];
            self.foot_x = e.x;
            self.foot_y = e.y;
            self.foot_heading = e.heading;
            (e.x, e.y)
        } else if self.on_foot {
            self.update_foot(input);
            (self.foot_x, self.foot_y)
        } else if self.in_plane {
            let mut inp = input.car_controls();
            if self.landing {
                // Autopilot takes the controls until the plane is set down.
                self.update_landing(&mut inp);
            } else {
                // Mouse flight controls (keyboard still works and wins if used):
                // LMB = full throttle, RMB = brake, drag = yaw + pitch
                // (drag up = climb), wheel = cruise throttle.
                if inp.throttle == 0.0 {
                    inp.throttle = if input.mouse_right_state() {
                        -1.0
                    } else if input.mouse_down_state() {
                        1.0
                    } else {
                        self.mouse_throttle
                    };
                }
                if inp.steer == 0.0 {
                    inp.steer = (mdx * 0.015).clamp(-1.0, 1.0);
                }
                if inp.pitch == 0.0 {
                    // Joystick-style pitch stick: drag up feeds +, the stick
                    // decays toward level when the mouse stops moving.
                    self.plane_pitch_stick += -mdy * 0.05;
                    self.plane_pitch_stick =
                        (self.plane_pitch_stick.clamp(-1.0, 1.0) / (1.0 + 4.0 * DT)).max(-1.0);
                    inp.pitch = self.plane_pitch_stick;
                } else {
                    self.plane_pitch_stick = 0.0;
                }
                self.mouse_throttle =
                    (self.mouse_throttle + input.wheel_delta() * 0.15).clamp(0.0, 1.0);
            }
            step_plane(&mut self.plane, &inp, DT);
            // Contrail off the wingtips at altitude and speed; dust and
            // sparks on a hard touch-down, dust on a fast rollout.
            let p = self.plane;
            let sp = p.speed();
            if p.z > 30.0 && sp > 200.0 && self.rng.below(2) == 0 {
                let (cx, cy) = (p.heading.cos(), p.heading.sin());
                for sy in [-1.0, 1.0] {
                    self.fx.smoke(&mut self.rng, p.x - cx * 6.0 - cy * sy * 30.0, p.y - cy * 6.0 + cx * sy * 30.0, p.z + 7.0, -cx * sp * 0.15, -cy * sp * 0.15, 6.0, 1.7, 2.6, 0xf4f7fa, 0.35);
                }
            }
            if p.z < 28.0 {
                if p.vz < -50.0 {
                    self.fx.dust(&mut self.rng, p.x, p.y, 10, 0xb8ae97);
                    self.fx.sparks(&mut self.rng, p.x, p.y, 2.0, 10, 150.0, 0xffc94a);
                } else if sp > 140.0 && self.rng.below(3) == 0 {
                    let (cx, cy) = (p.heading.cos(), p.heading.sin());
                    self.fx.dust(&mut self.rng, p.x - cx * 26.0, p.y - cy * 26.0, 3, 0xb8ae97);
                }
            }
            (p.x, p.y)
        } else {
            let inp = input.car_controls();
            step_car(&mut self.car, &inp, DT);
            if collide_car_with_city(&mut self.car, &self.city) {
                if self.car.speed() > 150.0 {
                    // Crash FX: sparks and debris off the front, plus a puff
                    // of smoke where the car meets the wall.
                    let (cx, cy) = (
                        self.car.x + self.car.heading.cos() * 20.0,
                        self.car.y + self.car.heading.sin() * 20.0,
                    );
                    self.fx.sparks(&mut self.rng, cx, cy, 5.0, 14, 220.0, 0xffc94a);
                    self.fx.debris(&mut self.rng, cx, cy, 6, 0x3a3f45);
                    for _ in 0..5 {
                        self.fx.smoke(&mut self.rng, cx, cy, 4.0, 0.0, 0.0, 16.0, 0.9, 6.5, 0x565b62, 0.5);
                    }
                    events.push(Event::Crash);
                }
            }
            // Drift / handbrake tire smoke off the rear wheels.
            let c = self.car;
            let sp = c.speed();
            let slip = c.slip();
            if sp > 60.0 && slip.abs() > 40.0 {
                let (cx, cy) = (c.heading.cos(), c.heading.sin());
                for sy in [-1.0, 1.0] {
                    let wx = c.x - cx * 16.0 - cy * sy * 8.0;
                    let wy = c.y - cy * 16.0 + cx * sy * 8.0;
                    self.fx.smoke(&mut self.rng, wx, wy, 2.0, -cx * 30.0, -cy * 30.0, 14.0, 0.55, 6.0, 0x8b9099, 0.42);
                }
            }
            // Boost: a flash of exhaust glow behind the car.
            if inp.boost && sp > 180.0 && self.rng.below(2) == 0 {
                let (cx, cy) = (c.heading.cos(), c.heading.sin());
                self.fx.smoke(&mut self.rng, c.x - cx * 25.0, c.y - cy * 25.0, 3.0, -cx * 70.0, -cy * 70.0, 40.0, 0.4, 4.0, 0x7fd4ff, 0.6);
            }
            (c.x, c.y)
        };

        if input.just_pressed("p") {
            self.paused = true;
        }
        if input.just_pressed("r") {
            self.cam_x = px;
            self.cam_y = py;
        }

        // ---- Enter / exit vehicle (car or plane) ----
        if input.just_pressed("e") {
            if self.on_foot {
                let cd = dist((px, py), (self.car.x, self.car.y));
                let pd = dist((px, py), (self.plane.x, self.plane.y));
                if cd < 70.0 && cd <= pd {
                    self.in_plane = false;
                    self.on_foot = false;
                    events.push(Event::EnterCar);
                    self.set_msg("VEHICLE ACQUIRED", 2.0);
                } else if pd < 90.0 && self.plane.z < 30.0 {
                    self.in_plane = true;
                    self.on_foot = false;
                    events.push(Event::EnterCar);
                    self.set_msg("AIRPLANE — W throttle · A/D steer · SHIFT/SPACE climb/dive", 4.0);
                }
            } else if self.riding.is_none() {
                let (vx, vy, vh, vspeed) = if self.in_plane {
                    (self.plane.x, self.plane.y, self.plane.heading, self.plane.speed())
                } else {
                    (self.car.x, self.car.y, self.car.heading, self.car.speed())
                };
                if vspeed < 40.0 {
                    // Step out to the side of the vehicle (a plane drops you
                    // to the street below).
                    let side = vh + std::f64::consts::FRAC_PI_2;
                    let mut fx = vx + side.cos() * 36.0;
                    let mut fy = vy + side.sin() * 36.0;
                    if let Some((x, y, _, _)) = self.city.collide_circle(fx, fy, FOOT_RADIUS) {
                        fx = x;
                        fy = y;
                    }
                    self.foot_x = fx;
                    self.foot_y = fy;
                    self.foot_heading = vh;
                    self.on_foot = true;
                    if self.in_plane {
                        self.in_plane = false;
                        self.plane_pitch_stick = 0.0;
                        self.plane.vx = 0.0;
                        self.plane.vy = 0.0;
                        self.plane.vz = 0.0;
                    }
                    events.push(Event::ExitCar);
                } else {
                    self.set_msg("STOP THE VEHICLE FIRST", 1.5);
                }
            }
        }

        // ---- Pedestrians ----
        // A plane or dragon above the rooftops is no threat to the streets
        // below, and neither is a player strolling (or elephant-riding).
        let threat_speed = if self.on_foot
            || self.riding.is_some()
            || self.in_dragon
            || (self.in_plane && self.plane.z > 15.0)
        {
            0.0
        } else {
            self.active_vehicle().speed()
        };
        self.update_peds(threat_speed);
        if !self.in_dragon {
            self.ped_collisions(&mut events, px, py, threat_speed);
        }

        // ---- Traffic ----
        for t in self.traffic.iter_mut() {
            t.update(DT, &mut self.rng, &self.city, px, py);
        }
        if !self.in_dragon {
            self.traffic_collisions(&mut events, px, py);
        }

        // ---- Wildlife (elephants on the streets, birds overhead, the dragon) ----
        // Keep the dragon's control flag in sync with the mode each tick.
        self.wildlife.dragon.controlled = self.in_dragon;
        self.wildlife.update(
            DT,
            &mut self.rng,
            self.time,
            &self.city,
            px,
            py,
            threat_speed,
        );
        if !self.in_dragon {
            self.elephant_collisions(&mut events, px, py, threat_speed);
        }

        // Walking elephants kick up a trail of street dust.
        for e in self.wildlife.elephants.iter() {
            if e.gait > 0.25 && self.rng.below(5) == 0 {
                let (cx, cy) = (e.heading.cos(), e.heading.sin());
                self.fx.dust(
                    &mut self.rng,
                    e.x - cx * 12.0 * e.scale,
                    e.y - cy * 12.0 * e.scale,
                    2,
                    0xb0a58e,
                );
            }
        }

        // Re-glue the rider to the elephant (it moved during the wildlife
        // step above) so the camera and renderers see the final position.
        if let Some(idx) = self.riding {
            let e = self.wildlife.elephants[idx];
            self.foot_x = e.x;
            self.foot_y = e.y;
            self.foot_heading = e.heading;
        }

        // ---- Police & wanted ----
        self.update_police_and_heat(&mut events, px, py);

        // ---- Mission ----
        if let Some(ev) = self.mission.update(DT, &mut self.rng, &self.city, px, py) {
            if ev.reward > 0 {
                self.money = self.money.saturating_add(ev.reward);
                // Delivery: a shower of green sparkles.
                self.fx.glitter(&mut self.rng, px, py, 6.0, 26, 0x7dff9c);
                self.fx.glitter(&mut self.rng, px, py, 6.0, 10, 0xfff3b0);
            } else if matches!(self.mission.phase, crate::mission::MissionPhase::ToDeliver) {
                // Pickup: a burst of gold sparkles.
                self.fx.glitter(&mut self.rng, px, py, 6.0, 22, 0xffe27a);
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
        let (tx, ty) = if self.on_foot || self.riding.is_some() {
            (self.foot_x, self.foot_y)
        } else {
            (px + self.active_vehicle().vx * 0.25, py + self.active_vehicle().vy * 0.25)
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
        let pr = self.active_vehicle().radius;
        let mut hit_pt: Option<(f64, f64)> = None;
        let _ = self.peds.iter_mut().any(|p| {
            if !matches!(p.state, crate::ped::PedState::Alive) {
                return false;
            }
            let d = dist((px, py), (p.x, p.y));
            if d < pr + 8.0 {
                p.kill(self.time);
                hit_pt = Some((p.x, p.y));
                return true;
            }
            false
        });
        if let Some((hx, hy)) = hit_pt {
            // A red mist where the pedestrian went down.
            self.fx.sparks(&mut self.rng, hx, hy, 3.0, 8, 130.0, 0x9c2b1e);
            for _ in 0..4 {
                self.fx.smoke(&mut self.rng, hx, hy, 2.0, 0.0, 0.0, 10.0, 0.6, 5.0, 0x6e2418, 0.5);
            }
            self.add_crime(0.9);
            events.push(Event::PedHit);
            self.set_msg("CITIZEN HARMED", 2.0);
        }
    }

    /// Push traffic out of the player circle (or vice versa) and raise heat
    /// on fast impacts.
    fn traffic_collisions(&mut self, events: &mut Vec<Event>, px: f64, py: f64) {
        if self.in_plane && self.plane.z > 15.0 {
            return; // too high to clip the traffic below
        }
        let pr = if self.on_foot || self.riding.is_some() {
            FOOT_RADIUS
        } else {
            self.active_vehicle().radius
        };
        let on_foot = self.on_foot || self.riding.is_some();
        let impact = if on_foot { 0.0 } else { self.active_vehicle().speed() };
        let mut fast_hit = false;
        let mut hit_pt: Option<(f64, f64)> = None;
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
                    hit_pt = Some(((px + t.car.x) / 2.0, (py + t.car.y) / 2.0));
                }
            }
        }
        if fast_hit {
            if let Some((ix, iy)) = hit_pt {
                self.fx.sparks(&mut self.rng, ix, iy, 4.0, 10, 200.0, 0xffc94a);
                self.fx.debris(&mut self.rng, ix, iy, 4, 0x3a3f45);
                for _ in 0..4 {
                    self.fx.smoke(&mut self.rng, ix, iy, 3.0, 0.0, 0.0, 14.0, 0.7, 6.0, 0x565b62, 0.45);
                }
            }
            self.add_crime(0.4);
            events.push(Event::PoliceHit);
        }
    }

    /// Elephants are solid: push the player (and the elephant) apart, and
    /// treat a fast impact like hitting traffic.
    fn elephant_collisions(&mut self, events: &mut Vec<Event>, px: f64, py: f64, threat_speed: f64) {
        if self.in_plane && self.plane.z > 15.0 {
            return;
        }
        let pr = if self.on_foot || self.riding.is_some() {
            FOOT_RADIUS
        } else {
            self.active_vehicle().radius
        };
        let mut fast_hit = false;
        let mut hit_pt: Option<(f64, f64)> = None;
        for (i, e) in self.wildlife.elephants.iter_mut().enumerate() {
            if self.riding == Some(i) {
                continue; // the elephant carrying you can't shove you off
            }
            let d = dist((px, py), (e.x, e.y));
            let min = pr + e.radius();
            if d < min && d > 0.001 {
                let nx = (e.x - px) / d;
                let ny = (e.y - py) / d;
                let push = min - d;
                // The elephant is massive: the player takes most of the push.
                if self.on_foot || self.riding.is_some() {
                    self.foot_x -= nx * push;
                    self.foot_y -= ny * push;
                } else {
                    self.car.x -= nx * push * 0.7;
                    self.car.y -= ny * push * 0.7;
                    self.car.vx *= 0.75;
                    self.car.vy *= 0.75;
                    if threat_speed > 110.0 {
                        fast_hit = true;
                        hit_pt = Some(((px + e.x) / 2.0, (py + e.y) / 2.0));
                    }
                }
                e.x += nx * push * 0.3;
                e.y += ny * push * 0.3;
                if let Some((x, y, _, _)) = self.city.collide_circle(e.x, e.y, e.radius()) {
                    e.x = x;
                    e.y = y;
                }
            }
        }
        if fast_hit {
            if let Some((ix, iy)) = hit_pt {
                self.fx.sparks(&mut self.rng, ix, iy, 4.0, 10, 200.0, 0xffc94a);
                self.fx.dust(&mut self.rng, ix, iy, 8, 0xb0a58e);
                for _ in 0..4 {
                    self.fx.smoke(&mut self.rng, ix, iy, 3.0, 0.0, 0.0, 14.0, 0.7, 7.0, 0x565b62, 0.45);
                }
            }
            self.add_crime(0.4);
            events.push(Event::PoliceHit);
            self.set_msg("WILDLIFE COLLISION", 2.0);
        }
    }

    /// Auto-land autopilot: steer toward the target, shed altitude with the
    /// pitch, keep a low horizontal approach speed so it can always turn onto
    /// the spot, then brake to a stop on the tarmac. Overwrites the player
    /// input while active. (Nearest intersection is never >~200px away.)
    fn update_landing(&mut self, inp: &mut crate::car::CarInput) {
        let (tx, ty) = self.landing_target;
        let p = &mut self.plane;
        let dx = tx - p.x;
        let dy = ty - p.y;
        let d = (dx * dx + dy * dy).sqrt();
        // Heading error wrapped to [-pi, pi].
        let mut diff = dy.atan2(dx) - p.heading;
        while diff > std::f64::consts::PI {
            diff -= 2.0 * std::f64::consts::PI;
        }
        while diff < -std::f64::consts::PI {
            diff += 2.0 * std::f64::consts::PI;
        }
        inp.steer = diff.clamp(-1.0, 1.0);

        let sp = p.speed();

        // Set down: on the street inside the spot -> hold the brake to a stop.
        if p.z < 2.0 && d < 80.0 {
            let fwd = p.vx * p.heading.cos() + p.vy * p.heading.sin();
            inp.throttle = if fwd > 8.0 {
                -1.0
            } else if fwd <= -5.0 {
                0.2 // unwind a reverse overshoot
            } else {
                0.0
            };
            inp.pitch = 0.0;
            self.plane_pitch_stick = 0.0;
            if fwd.abs() < 8.0 && p.vz.abs() < 1.0 {
                self.landing = false;
                p.vx = 0.0;
                p.vy = 0.0;
                p.vz = 0.0;
                // Park it: cut the cruise throttle so it stays put.
                self.mouse_throttle = 0.0;
                self.set_msg("LANDED — SAFE SPACE", 3.0);
            }
            return;
        }

        // Vertical: full dive to shed altitude, then a gentle sink to the spot.
        inp.pitch = if p.z > 300.0 {
            -1.0
        } else if p.z > 60.0 {
            -0.4
        } else {
            -0.2
        };
        self.plane_pitch_stick = 0.0;

        // Facing away at speed: braking while the nose swings would turn the
        // brake into reverse thrust, so kill speed first, then swing around.
        if sp > 80.0 && diff.abs() > 1.2 {
            inp.throttle = -1.0;
            inp.steer = 0.0;
            return;
        }

        // Horizontal: slow near the spot, much slower when facing away (so the
        // turn radius stays small), hard brake if already over the limit.
        let align = (1.0 - diff.abs() / std::f64::consts::PI).max(0.2);
        let want = (3.0 * d).min(160.0 * align);
        inp.throttle = if sp > want + 30.0 {
            -1.0
        } else {
            (want / 1200.0).clamp(0.0, 1.0)
        };
    }

    fn update_police_and_heat(&mut self, events: &mut Vec<Event>, px: f64, py: f64) {
        let s = self.stars();

        // The dragon rides above the streets, where the patrol can't reach it:
        // no squad is kept up while the player is on it (heat still decays).
        if !self.in_dragon {
            // Keep the police squad sized to the star level.
            // (An elephant rider is a pedestrian as far as catching goes.)
            let want = s as usize;
            while self.police.len() < want {
                let extra = crate::police::spawn_police(&mut self.rng, &self.city, px, py, 1);
                self.police.extend(extra);
            }
            if self.police.len() > want {
                self.police.truncate(want);
            }

            if !self.police.is_empty() {
                let catch = if self.on_foot || self.riding.is_some() {
                    26.0
                } else {
                    40.0
                };
                let caught = crate::police::update_police(
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
        } else {
            self.police.clear();
        }

        // Heat decay.
        let nearest = crate::police::nearest_police(&self.police, px, py);
        self.heat = crate::police::decay_heat(
            self.heat,
            DT,
            self.time - self.last_crime,
            nearest,
        );
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
        let (ax, ay) = City::intersection_pos(5, 5);
        self.plane.x = ax;
        self.plane.y = ay + 24.0;
        self.plane.heading = -std::f64::consts::FRAC_PI_2;
        self.plane.vx = 0.0;
        self.plane.vy = 0.0;
        self.plane.vz = 0.0;
        self.plane.z = 0.0;
        self.in_plane = false;
        self.in_dragon = false; // a bust grounds you
        self.wildlife.dragon.controlled = false;
        self.wildlife.dragon.z0 = self.wildlife.dragon.z;
        self.on_foot = false;
        self.riding = None; // a bust unhorses you
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
    fn v_toggles_view_3d() {
        let mut s = idle_state();
        let mut inp = Input::new();
        assert!(!s.view_3d);
        inp.key_down("v");
        s.tick(&mut inp);
        assert!(s.view_3d, "pressing V enters 3D mode");
        inp.key_up("v");
        inp.key_down("v");
        s.tick(&mut inp);
        assert!(!s.view_3d, "pressing V again returns to top-down");
    }

    #[test]
    fn d_mounts_the_dragon_and_controls_it() {
        let mut s = idle_state();
        let mut inp = Input::new();
        // Let the world run so the dragon reaches its cruise altitude.
        for _ in 0..60 {
            s.tick(&mut inp);
        }
        let cruise_z = s.wildlife.dragon.z;
        assert!(cruise_z > 200.0, "dragon should be cruising high: {}", cruise_z);

        // The car is parked (speed 0), so D is free to summon the dragon.
        keypress(&mut inp, "d");
        s.tick(&mut inp);
        assert!(s.in_dragon, "D should mount the dragon");
        assert!(s.view_3d, "mounting the dragon should switch to 3D");
        assert!(s.wildlife.dragon.controlled, "the dragon should be player-controlled");
        inp.key_up("d");
        inp.end_frame();

        // Throttle + climb: the dragon should gain speed and altitude.
        let z0 = s.wildlife.dragon.z;
        inp.key_down("w");
        inp.key_down("shift");
        for _ in 0..120 {
            s.tick(&mut inp);
        }
        assert!(
            s.wildlife.dragon.z > z0 + 20.0,
            "climbing should raise the dragon: {} -> {}",
            z0,
            s.wildlife.dragon.z
        );
        assert!(s.wildlife.dragon.speed > 100.0, "should be flying fast");
        inp.key_up("w");
        inp.key_up("shift");
        inp.end_frame();

        // Release: drop to the street below the dragon, on foot.
        keypress(&mut inp, "d");
        s.tick(&mut inp);
        assert!(!s.in_dragon, "D should release the dragon");
        assert!(s.on_foot, "releasing the dragon should put you on foot");
        assert!(!s.wildlife.dragon.controlled, "the dragon should fly on its own again");
        let (px, py) = s.player_pos();
        let d = &s.wildlife.dragon;
        let dist = ((px - d.x).powi(2) + (py - d.y).powi(2)).sqrt();
        assert!(dist < 60.0, "should land near the dragon: {} px off", dist);
    }

    #[test]
    fn d_does_nothing_while_driving_the_car() {
        let mut s = idle_state();
        let mut inp = Input::new();
        // Drive the car up to speed, then press D: it should steer, not summon.
        inp.key_down("w");
        for _ in 0..120 {
            s.tick(&mut inp);
        }
        assert!(s.car.speed() > 100.0, "the car should be moving");
        inp.key_up("w");
        inp.end_frame();
        inp.key_down("d"); // steer right while moving fast
        s.tick(&mut inp);
        assert!(!s.in_dragon, "D should steer the car, not summon the dragon");
    }

    #[test]
    fn d_does_nothing_on_foot() {
        let mut s = idle_state();
        let mut inp = Input::new();
        // Get out of the car (on foot), then press D: it should walk, not summon.
        s.on_foot = true;
        s.foot_x = s.car.x;
        s.foot_y = s.car.y;
        keypress(&mut inp, "d");
        s.tick(&mut inp);
        assert!(!s.in_dragon, "D should walk right on foot, not summon the dragon");
        inp.key_up("d");
        inp.end_frame();
    }

    #[test]
    fn cam3d_yaw_smooths_toward_heading() {
        let mut s = idle_state();
        let mut inp = Input::new();
        s.car.heading = std::f64::consts::FRAC_PI_4;
        s.cam3d_yaw = 0.0;
        for _ in 0..120 {
            s.tick(&mut inp);
        }
        assert!(
            (s.cam3d_yaw - std::f64::consts::FRAC_PI_4).abs() < 0.01,
            "yaw {} should settle near {}",
            s.cam3d_yaw,
            std::f64::consts::FRAC_PI_4
        );
    }

    #[test]
    fn mouse_drag_orbits_and_pitches_the_3d_camera() {
        let mut s = GameState::new(42);
        let mut inp = Input::new();
        inp.key_down("v"); // enter 3D mode
        s.tick(&mut inp);
        assert!(s.view_3d);
        // Drag right + down: orbit positive, pitch goes negative (look down).
        inp.mouse_down();
        inp.mouse_move(200.0, 100.0);
        s.tick(&mut inp);
        inp.mouse_up();
        assert!(s.cam3d_orbit > 0.5, "orbit={}", s.cam3d_orbit);
        assert!(s.cam3d_pitch < 0.0, "pitch={}", s.cam3d_pitch);
        // Dragging in top-down mode does nothing.
        inp.key_up("v");
        inp.key_down("v");
        s.tick(&mut inp);
        let (o, p) = (s.cam3d_orbit, s.cam3d_pitch);
        inp.mouse_down();
        inp.mouse_move(100.0, 100.0);
        s.tick(&mut inp);
        inp.mouse_up();
        assert_eq!((s.cam3d_orbit, s.cam3d_pitch), (o, p));
        // Back in 3D, C resets the look camera.
        inp.key_up("v");
        inp.key_down("v");
        s.tick(&mut inp);
        inp.key_down("c");
        s.tick(&mut inp);
        assert_eq!(s.cam3d_orbit, 0.0);
        assert_eq!(s.cam3d_pitch, 0.0);
    }

    #[test]
    fn cam3d_yaw_wraps_shortest_path_across_pi() {
        let mut s = idle_state();
        let mut inp = Input::new();
        s.car.heading = -std::f64::consts::FRAC_PI_4;
        s.cam3d_yaw = std::f64::consts::PI; // opposite side of the circle
        for _ in 0..120 {
            s.tick(&mut inp);
        }
        // Shortest path from π to -π/4 passes through ±3π/4, not 0.
        // It settles at 7π/4 (≡ -π/4 modulo 2π), which would be a long way
        // off if the lerp had gone the long way through 0.
        assert!((s.cam3d_yaw - 7.0 * std::f64::consts::PI / 4.0).abs() < 0.01);
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
    fn arrow_left_and_right_steer_on_a_straight_road() {
        use crate::city::{CELL, ROAD};
        let mut s = idle_state();
        let mut inp = Input::new();
        // Teleport onto a long straight vertical road, facing north.
        s.car.x = 4.0 * CELL + ROAD / 2.0;
        s.car.y = SIZE - 200.0;
        s.car.heading = -std::f64::consts::FRAC_PI_2;
        s.cam_x = s.car.x;
        s.cam_y = s.car.y;
        // Build up speed using ArrowUp alone.
        inp.key_down("arrowup");
        for _ in 0..60 {
            s.tick(&mut inp);
        }
        assert!(s.car.speed() > 150.0, "ArrowUp should accelerate: {} px/s", s.car.speed());
        let h0 = s.car.heading;
        // ArrowRight turns right (heading increases).
        inp.key_down("arrowright");
        for _ in 0..30 {
            s.tick(&mut inp);
        }
        let h_right = s.car.heading;
        inp.key_up("arrowright");
        // ArrowLeft turns left (heading decreases) and steers back.
        inp.key_down("arrowleft");
        for _ in 0..60 {
            s.tick(&mut inp);
        }
        let h_left = s.car.heading;
        inp.key_up("arrowleft");
        assert!(h_right > h0 + 0.1, "ArrowRight should steer right: {h0} -> {h_right}");
        assert!(h_left < h_right - 0.1, "ArrowLeft should steer left: {h_right} -> {h_left}");
    }

    #[test]
    fn arrow_down_brakes_a_running_car() {
        use crate::city::{CELL, ROAD};
        let mut s = idle_state();
        let mut inp = Input::new();
        // Straight open lane, facing north (see the steer test above).
        s.car.x = 4.0 * CELL + ROAD / 2.0;
        s.car.y = SIZE - 200.0;
        s.car.heading = -std::f64::consts::FRAC_PI_2;
        s.cam_x = s.car.x;
        s.cam_y = s.car.y;
        keypress(&mut inp, "arrowup");
        for _ in 0..90 {
            s.tick(&mut inp);
        }
        let fast = s.car.speed();
        assert!(fast > 200.0);
        inp.key_up("arrowup");
        keypress(&mut inp, "arrowdown");
        for _ in 0..20 {
            s.tick(&mut inp);
        }
        assert!(
            s.car.speed() < fast * 0.5,
            "ArrowDown should brake hard: {} -> {} px/s",
            fast,
            s.car.speed()
        );
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
    fn f_summons_the_plane_from_anywhere() {
        let mut s = idle_state();
        let mut inp = Input::new();
        // On foot, far from the parked plane.
        s.on_foot = true;
        s.foot_x = 300.0;
        s.foot_y = 300.0;
        let parked = s.plane.x;
        keypress(&mut inp, "f");
        s.tick(&mut inp);
        assert!(s.in_plane, "F should summon + board the plane");
        assert!(!s.on_foot);
        assert!(
            (s.plane.x - 300.0).abs() < 1.0,
            "plane should appear at the player: {}",
            s.plane.x
        );
        assert!((s.plane.x - parked).abs() > 500.0);
        // It eases up off the street on its own.
        for _ in 0..60 {
            s.tick(&mut inp);
        }
        assert!(s.plane.z > 10.0, "summoned plane should lift off: z={}", s.plane.z);
        // F again does nothing (E is how you leave).
        let z = s.plane.z;
        keypress(&mut inp, "f");
        s.tick(&mut inp);
        assert!(s.in_plane);
        assert!((s.plane.z - z).abs() < 1.0);
    }

    /// Run the auto-land from `(x, y, z, heading)` and assert it ends parked
    /// on the street at the chosen safe space.
    fn run_autoland(s: &mut GameState, inp: &mut Input, x: f64, y: f64, z: f64, heading: f64) {
        inp.key_up("m"); // fresh key edge for each run
        s.in_plane = true;
        s.plane.x = x;
        s.plane.y = y;
        s.plane.z = z;
        s.plane.heading = heading;
        s.plane.vx = 0.0;
        s.plane.vy = 0.0;
        s.plane.vz = 0.0;
        s.cam_x = x;
        s.cam_y = y;
        keypress(inp, "m");
        s.tick(inp);
        assert!(s.landing, "M should start the auto-land");
        let (tx, ty) = s.landing_target;
        assert!(s.city.is_road(tx, ty), "safe space must be a road");
        let d0 = ((tx - x).powi(2) + (ty - y).powi(2)).sqrt();
        assert!(d0 < 200.0, "nearest intersection should be close, got {}", d0);
        for _ in 0..60 * 90 {
            s.tick(inp);
            if !s.landing {
                break;
            }
        }
        assert!(!s.landing, "auto-land should complete from ({}, {}, z={})", x, y, z);
        assert!(s.plane.z < 1.0, "plane should be on the street: z={}", s.plane.z);
        assert!(s.plane.speed() < 1.0, "plane should be stopped: {}", s.plane.speed());
        let d = ((s.plane.x - tx).powi(2) + (s.plane.y - ty).powi(2)).sqrt();
        assert!(d < 80.0, "plane should sit at the safe space: {}px off", d);
        assert!(s.city.is_road(s.plane.x, s.plane.y), "must be parked on a road");
    }

    #[test]
    fn m_autolands_the_plane_at_the_nearest_safe_space() {
        let mut s = idle_state();
        let mut inp = Input::new();
        // High over the blocks, facing away from the spot.
        run_autoland(&mut s, &mut inp, 1500.0, 1500.0, 800.0, 0.0);
        // Right next to the spot, at street level, facing into a wall.
        run_autoland(&mut s, &mut inp, 1440.0, 1500.0, 20.0, 0.0);
        // In the middle of a block, low altitude.
        run_autoland(&mut s, &mut inp, 400.0, 400.0, 50.0, std::f64::consts::FRAC_PI_4);
        // From the corner of the map, at max altitude.
        run_autoland(&mut s, &mut inp, 50.0, 2550.0, 1000.0, std::f64::consts::PI);
    }

    #[test]
    fn m_again_cancels_the_auto_land() {
        let mut s = idle_state();
        let mut inp = Input::new();
        s.in_plane = true;
        s.plane.z = 500.0;
        keypress(&mut inp, "m");
        s.tick(&mut inp);
        assert!(s.landing);
        inp.key_up("m");
        keypress(&mut inp, "m");
        s.tick(&mut inp);
        assert!(!s.landing, "second M should cancel the auto-land");
    }

    #[test]
    fn m_does_nothing_outside_the_plane() {
        let mut s = idle_state();
        let mut inp = Input::new();
        keypress(&mut inp, "m");
        for _ in 0..10 {
            s.tick(&mut inp);
        }
        assert!(!s.landing && !s.in_plane, "M is a plane-only control");
    }

    #[test]
    fn mouse_drag_stears_and_pitches_the_plane() {
        let mut s = idle_state();
        let mut inp = Input::new();
        s.in_plane = true;
        s.plane.x = 500.0;
        s.plane.y = 500.0;
        s.plane.heading = 0.0;
        // Drag right + up for half a second: yaw right and climb.
        inp.mouse_down();
        for _ in 0..30 {
            inp.mouse_move(20.0, -10.0);
            s.tick(&mut inp);
        }
        let z_at_release = s.plane.z;
        inp.mouse_up();
        // The pitch stick decays, so the climb persists a moment after the
        // mouse stops moving.
        for _ in 0..10 {
            s.tick(&mut inp);
        }
        assert!(s.plane.heading > 0.03, "drag right should yaw right: {}", s.plane.heading);
        assert!(s.plane.z > 5.0, "drag up should climb: z={}", s.plane.z);
        assert!(s.plane.z > z_at_release, "pitch stick should persist: {} -> {}", z_at_release, s.plane.z);
    }

    #[test]
    fn mouse_wheel_sets_cruise_throttle() {
        let mut s = idle_state();
        let mut inp = Input::new();
        s.in_plane = true;
        s.mouse_throttle = 0.5;
        inp.mouse_wheel(2.0); // two notches up
        s.tick(&mut inp);
        assert!((s.mouse_throttle - 0.8).abs() < 0.01, "got {}", s.mouse_throttle);
        inp.mouse_wheel(-20.0);
        s.tick(&mut inp);
        assert_eq!(s.mouse_throttle, 0.0, "throttle clamps at zero");
    }

    #[test]
    fn e_boards_the_plane_and_it_climbs_above_the_city() {
        let mut s = idle_state();
        let mut inp = Input::new();
        // Walk up to the parked plane and board it.
        s.on_foot = true;
        s.foot_x = s.plane.x + 40.0;
        s.foot_y = s.plane.y;
        inp.key_down("e");
        s.tick(&mut inp);
        assert!(s.in_plane, "E next to the plane should board it");
        assert!(!s.on_foot);
        // Full throttle + climb.
        inp.key_down("w");
        inp.key_down("shift");
        for _ in 0..180 {
            s.tick(&mut inp);
        }
        assert!(s.plane.z > 300.0, "plane should climb above the rooftops: z={}", s.plane.z);
        assert!(s.plane.speed() > 300.0, "plane should be at speed");
        // High above the streets, no wanted heat from the traffic below.
        assert_eq!(s.stars(), 0);
    }

    #[test]
    fn plane_flys_over_the_city_buildings_and_can_land() {
        let mut s = idle_state();
        let mut inp = Input::new();
        s.in_plane = true;
        s.plane.x = 200.0;
        s.plane.y = 200.0;
        s.plane.z = 40.0;
        s.plane.heading = 0.0;
        s.cam_x = s.plane.x;
        s.cam_y = s.plane.y;
        inp.key_down("w");
        let mut crashed = false;
        for _ in 0..600 {
            let evs = s.tick(&mut inp);
            if evs.iter().any(|e| matches!(e, Event::Crash)) {
                crashed = true;
            }
        }
        assert!(!crashed, "a plane at altitude must not crash into buildings");
        assert!(s.plane.x > 2000.0, "plane should fly across the city");
        assert!((s.plane.z - 40.0).abs() < 5.0, "no pitch should hold altitude: z={}", s.plane.z);
        // Land: dive to the street, then cut throttle and coast to a stop.
        inp.key_up("w");
        inp.key_down(" ");
        for _ in 0..120 {
            s.tick(&mut inp);
        }
        assert!(s.plane.z < 1.0, "should be back on the ground: z={}", s.plane.z);
        inp.key_up(" ");
        s.mouse_throttle = 0.0; // wheel the cruise throttle off
        for i in 0..900 {
            s.tick(&mut inp);
            if s.plane.speed() < 40.0 {
                break;
            }
            assert!(i < 890, "plane should coast to a stop");
        }
        inp.key_down("e");
        s.tick(&mut inp);
        assert!(s.on_foot, "E should drop the pilot to the street");
        assert!(!s.in_plane);
    }

    /// Put the player on the back of the first elephant and press Z (mount).
    fn mount_first_elephant(s: &mut GameState, inp: &mut Input) -> usize {
        s.on_foot = true;
        s.foot_x = s.wildlife.elephants[0].x;
        s.foot_y = s.wildlife.elephants[0].y;
        keypress(inp, "z");
        s.tick(inp);
        assert!(s.riding == Some(0), "Z near an elephant should mount it");
        assert!(!s.on_foot, "riding should take the player off the street");
        0
    }

    #[test]
    fn z_mounts_an_elephant_and_it_carries_the_player() {
        let mut s = idle_state();
        let mut inp = Input::new();
        let idx = mount_first_elephant(&mut s, &mut inp);
        // The rider is glued to the elephant, which wanders on its own.
        let (ex, ey) = (s.wildlife.elephants[idx].x, s.wildlife.elephants[idx].y);
        let (px0, py0) = s.player_pos();
        assert_eq!((px0, py0), (ex, ey), "rider should sit on the elephant");
        inp.key_up("z");
        for _ in 0..180 {
            s.tick(&mut inp);
        }
        let e = &s.wildlife.elephants[idx];
        assert_eq!(s.riding, Some(idx), "should stay mounted");
        assert_eq!(s.player_pos(), (e.x, e.y), "player should follow the elephant");
        let moved = ((e.x - ex).powi(2) + (e.y - ey).powi(2)).sqrt();
        assert!(moved > 20.0, "the elephant should keep wandering: moved {moved}");
        // A slow ride raises no wanted heat.
        assert_eq!(s.stars(), 0);
    }

    #[test]
    fn z_again_dismounts_back_to_the_street() {
        let mut s = idle_state();
        let mut inp = Input::new();
        mount_first_elephant(&mut s, &mut inp);
        inp.key_up("z");
        keypress(&mut inp, "z");
        s.tick(&mut inp);
        assert!(s.riding.is_none(), "second Z should dismount");
        assert!(s.on_foot, "dismounting should leave the player on foot");
        // Dropped onto the street right beside the elephant.
        let (ex, ey) = (s.wildlife.elephants[0].x, s.wildlife.elephants[0].y);
        let d = ((s.foot_x - ex).powi(2) + (s.foot_y - ey).powi(2)).sqrt();
        assert!(d < 70.0, "dismount point should be next to the elephant: {d}");
        assert!(
            !s.city.buildings().any(|b| b.contains(s.foot_x, s.foot_y)),
            "should not dismount inside a building"
        );
    }

    #[test]
    fn z_far_from_elephants_does_not_mount() {
        let mut s = idle_state();
        let mut inp = Input::new();
        s.on_foot = true;
        s.foot_x = 100.0;
        s.foot_y = 100.0;
        keypress(&mut inp, "z");
        s.tick(&mut inp);
        assert!(s.riding.is_none(), "Z far from the herd should not mount");
        assert!(s.on_foot, "player should stay on foot");
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

