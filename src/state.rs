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

/// A fireball hurled from the dragon's mouth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fireball {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    pub r: f64,
    pub life: f64,
}

/// A building that has taken dragonfire: it collapses (`collapse` runs 0→1
/// over ~1.5 s and leaves rubble) and burns (`burn` = seconds of flames
/// left). Citizens rush in and hose it down to cut the burn short.
#[derive(Clone, Copy, Debug)]
pub struct BuildingFire {
    pub id: u32,
    pub collapse: f64,
    pub burn: f64,
}

/// What E boards while on foot: the nearest of the car, the (low) plane, or
/// an elephant in reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoardTarget {
    Car,
    Plane,
    Elephant(usize),
}

/// One row of the top-right "SPECIALS" HUD panel: a key and what it does in
/// the current situation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpecialAction {
    pub key: &'static str,
    pub label: &'static str,
}

/// Stable id for the `lot`-th building slot of block `block` (j * N + i).
pub const fn building_id(block: usize, lot: usize) -> u32 {
    (block * 4 + lot) as u32
}

/// Reverse of [`building_id`]: (block, lot) from an id.
pub const fn block_lot(id: u32) -> (usize, usize) {
    ((id / 4) as usize, (id % 4) as usize)
}

pub struct GameState {
    pub city: City,
    pub rng: Rng,
    pub time: f64,

    // Dragonfire
    /// Fireballs in flight from the dragon's mouth.
    pub fireballs: Vec<Fireball>,
    /// Buildings that are collapsing and/or burning (rubble persists).
    pub building_fires: Vec<BuildingFire>,

    // Player
    pub car: Car,
    /// The city airplane (parked at the airfield). `in_plane` = the player
    /// is currently the pilot of it.
    pub plane: Car,
    pub in_plane: bool,
    /// The dragon. `in_dragon` = the player is riding the dragon and
    /// piloting it ("G" to summon it, "E" to get off): the player's
    /// position/altitude become the dragon's and the ground is left behind.
    pub in_dragon: bool,
    pub on_foot: bool,
    pub foot_x: f64,
    pub foot_y: f64,
    pub foot_heading: f64,
    /// Index into `wildlife.elephants` of the elephant the player is riding
    /// (E to mount / dismount). The elephant wanders on its own and carries
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
    /// `time` until the dragon may breathe fire again (click cadence).
    fire_cd: f64,

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
    Fireball,
    BuildingDown,
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
            fireballs: Vec::new(),
            building_fires: Vec::new(),
            fx: Fx::new(),
            heat: 0.0,
            last_crime: -100.0,
            money: 100,
            mission,
            msg: Some((String::from("GO GET THE YELLOW MARKER"), 8.0)),
            paused: false,
            busted_until: 0.0,
            fire_cd: 0.0,
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

    /// The fire state of a building (`None` = untouched): used by the
    /// renderers for the collapse animation, flames and rubble.
    pub fn building_fire(&self, id: u32) -> Option<&BuildingFire> {
        self.building_fires.iter().find(|f| f.id == id)
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

    /// Player heading (px-plane angle) in the current mode.
    pub fn player_heading(&self) -> f64 {
        if self.in_dragon {
            self.wildlife.dragon.heading
        } else if self.on_foot || self.riding.is_some() {
            self.foot_heading
        } else if self.in_plane {
            self.plane.heading
        } else {
            self.car.heading
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

    /// What a player on foot would board with E: the closest of the car,
    /// the airplane (when low) or an elephant in reach.
    pub fn board_target(&self) -> Option<BoardTarget> {
        if !self.on_foot {
            return None;
        }
        let p = (self.foot_x, self.foot_y);
        let mut best: Option<(BoardTarget, f64)> = None;
        let cd = dist(p, (self.car.x, self.car.y));
        if cd < 70.0 {
            best = Some((BoardTarget::Car, cd));
        }
        if self.plane.z < 30.0 {
            let pd = dist(p, (self.plane.x, self.plane.y));
            if pd < 90.0 && best.map_or(true, |(_, bd)| pd < bd) {
                best = Some((BoardTarget::Plane, pd));
            }
        }
        for (i, e) in self.wildlife.elephants.iter().enumerate() {
            let d = dist(p, (e.x, e.y));
            if d <= 45.0 + e.radius() && best.map_or(true, |(_, bd)| d < bd) {
                best = Some((BoardTarget::Elephant(i), d));
            }
        }
        best.map(|(t, _)| t)
    }

    /// The special actions available right now, with their keys — drawn as
    /// the always-on "SPECIALS" panel in the top-right corner of the HUD.
    pub fn special_actions(&self) -> Vec<SpecialAction> {
        let mut a = Vec::new();
        // E — board / exit the nearest rideable thing.
        if self.in_dragon {
            a.push(SpecialAction { key: "E", label: "EXIT DRAGON (DROP DOWN)" });
            a.push(SpecialAction { key: "LMB", label: "FIREBALL" });
        } else if self.riding.is_some() {
            a.push(SpecialAction { key: "E", label: "JUMP OFF ELEPHANT" });
        } else if self.on_foot {
            match self.board_target() {
                Some(BoardTarget::Car) => a.push(SpecialAction { key: "E", label: "ENTER CAR" }),
                Some(BoardTarget::Plane) => a.push(SpecialAction { key: "E", label: "ENTER AIRPLANE" }),
                Some(BoardTarget::Elephant(_)) => a.push(SpecialAction { key: "E", label: "BOARD ELEPHANT" }),
                None => {}
            }
        } else {
            a.push(SpecialAction {
                key: "E",
                label: if self.in_plane { "EXIT AIRPLANE (WHEN SLOW)" } else { "EXIT CAR (WHEN SLOW)" },
            });
            if self.in_plane {
                a.push(SpecialAction {
                    key: "M",
                    label: if self.landing { "CANCEL AUTO-LAND" } else { "AUTO-LAND" },
                });
            }
        }
        // Summon keys — they work from anywhere and never collide with movement.
        if !self.in_plane {
            a.push(SpecialAction { key: "F", label: "SUMMON AIRPLANE" });
        }
        if !self.in_dragon {
            a.push(SpecialAction { key: "G", label: "SUMMON DRAGON" });
        }
        // View & camera.
        a.push(SpecialAction {
            key: "V",
            label: if self.view_3d { "TOP-DOWN VIEW" } else { "3D CHASE-CAM" },
        });
        if self.view_3d {
            a.push(SpecialAction { key: "C", label: "RESET CAMERA" });
        }
        a.push(SpecialAction { key: "P", label: "PAUSE" });
        a.push(SpecialAction { key: "R", label: "RE-CENTER CAMERA" });
        a
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
        let heading = self.player_heading();
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
            let (px, py, ph, pz) = if self.in_dragon {
                let d = self.wildlife.dragon;
                (d.x, d.y, d.heading, d.z)
            } else if self.on_foot {
                (self.foot_x, self.foot_y, self.foot_heading, 0.0)
            } else {
                (self.car.x, self.car.y, self.car.heading, 0.0)
            };
            self.plane.x = px;
            self.plane.y = py;
            self.plane.z = pz.max(0.0);
            self.plane.heading = ph;
            self.plane.vx = 0.0;
            self.plane.vy = 0.0;
            self.plane.vz = if self.plane.z < 30.0 {
                160.0 // gentle ease up off the street
            } else {
                0.0
            };
            self.plane_pitch_stick = 0.0;
            // Get off the dragon and hand it back to its own meander.
            if self.in_dragon {
                self.in_dragon = false;
                self.wildlife.dragon.z0 = self.wildlife.dragon.z;
                self.wildlife.dragon.controlled = false;
            }
            self.in_plane = true;
            self.on_foot = false;
            self.riding = None; // summons the plane off the elephant's back
            self.set_msg("PLANE SUMMONED — DRAG: steer · LMB: throttle · WHEEL: speed · M: auto-land", 4.5);
        }

        // ---- G: summon the dragon and take its reins, anywhere ----
        if input.just_pressed("g") && !self.in_dragon {
            let (px, py, ph, pz) = if self.in_plane {
                (self.plane.x, self.plane.y, self.plane.heading, self.plane.z)
            } else if self.on_foot {
                (self.foot_x, self.foot_y, self.foot_heading, 0.0)
            } else {
                (self.car.x, self.car.y, self.car.heading, 0.0)
            };
            let d = &mut self.wildlife.dragon;
            d.x = px;
            d.y = py;
            d.z = pz.max(8.0);
            d.heading = ph;
            d.speed = 0.0;
            d.vz = 0.0;
            d.controlled = true;
            self.in_dragon = true;
            self.view_3d = true; // you fly it best from the 3D chase cam
            self.on_foot = false;
            if self.in_plane {
                // Leave the plane behind, parked in the air.
                self.in_plane = false;
                self.plane_pitch_stick = 0.0;
                self.plane.vx = 0.0;
                self.plane.vy = 0.0;
                self.plane.vz = 0.0;
            }
            self.riding = None;
            self.set_msg(
                "DRAGON — W/S: speed · A/D or DRAG: turn · LMB: FIREBALL · SHIFT/SPACE: climb/dive · E: exit",
                6.0,
            );
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

        // ---- Player ----
        let (px, py) = if self.in_dragon {
            // Piloting the dragon. Keyboard + mouse, mirroring the airplane:
            // RMB = brake, drag = yaw + pitch (up = climb), wheel = cruise
            // throttle, LMB = breathe fire; W/S/A/D/Shift/Space win.
            let mut inp = input.car_controls();
            if inp.throttle == 0.0 {
                inp.throttle = if input.mouse_right_state() {
                    -1.0
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
            let pos = (d.x, d.y);
            // LMB: the dragon breathes fire.
            if input.mouse_down_state() {
                self.breathe_fireball(&mut events);
            }
            pos
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

        // ---- E: board / exit (car, plane, elephant or dragon) ----
        if input.just_pressed("e") {
            if self.in_dragon {
                // Exit: drop to the street below, and hand the dragon back
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
                events.push(Event::ExitCar);
                self.set_msg("BACK ON THE GROUND — G: summon the dragon again", 3.5);
            } else if let Some(idx) = self.riding {
                // Jump off the elephant to the street beside it, on foot.
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
                // Board the nearest rideable thing in reach.
                match self.board_target() {
                    Some(BoardTarget::Car) => {
                        self.in_plane = false;
                        self.on_foot = false;
                        events.push(Event::EnterCar);
                        self.set_msg("VEHICLE ACQUIRED", 2.0);
                    }
                    Some(BoardTarget::Plane) => {
                        self.in_plane = true;
                        self.on_foot = false;
                        events.push(Event::EnterCar);
                        self.set_msg("AIRPLANE — W throttle · A/D steer · SHIFT/SPACE climb/dive · M auto-land", 4.0);
                    }
                    Some(BoardTarget::Elephant(i)) => {
                        // Mount the elephant; it wanders on its own.
                        let e = self.wildlife.elephants[i];
                        self.riding = Some(i);
                        self.on_foot = false;
                        self.foot_x = e.x;
                        self.foot_y = e.y;
                        self.foot_heading = e.heading;
                        events.push(Event::EnterCar);
                        self.set_msg("RIDING THE ELEPHANT — IT WANDERS ON ITS OWN · E: JUMP OFF", 4.0);
                    }
                    None => {
                        self.set_msg("NOTHING IN REACH TO BOARD — F: plane · G: dragon", 2.0);
                    }
                }
            } else {
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

        // ---- Dragonfire: fireballs, collapsing & burning buildings, and
        //     the citizens that rush the blaze with buckets and hoses ----
        self.update_fireballs(&mut events);
        self.update_building_fires();
        self.update_firefighters();

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
            // Firefighters ignore the streets (and the threat): they are
            // driven by `update_firefighters` instead.
            if self.peds[i].firefight.is_some() {
                continue;
            }
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

    /// Hurl one fireball from the dragon's mouth (LMB while piloting).
    /// A click fires a single shot; holding the button streams them (a
    /// small reload gate between shots).
    pub fn breathe_fireball(&mut self, events: &mut Vec<Event>) {
        if self.time < self.fire_cd {
            return;
        }
        self.fire_cd = self.time + 0.18;
        let d = &self.wildlife.dragon;
        let (cx, cy) = (d.heading.cos(), d.heading.sin());
        let (mx, my, mz) = (d.x + cx * 22.0, d.y + cy * 22.0, d.z + 4.0);
        let sp = 560.0 + d.speed * 0.25;
        self.fireballs.push(Fireball {
            x: mx,
            y: my,
            z: mz,
            vx: cx * sp,
            vy: cy * sp,
            vz: -12.0,
            r: 7.0,
            life: 1.8,
        });
        self.fx.smoke(&mut self.rng, mx, my, mz, cx * 40.0, cy * 40.0, 15.0, 0.4, 5.0, 0xffb347, 0.9);
        events.push(Event::Fireball);
    }

    /// Advance fireballs; explode them against the street, a wall, or on
    /// timeout. Every blast knocks down the buildings it touches and sets
    /// them alight.
    fn update_fireballs(&mut self, events: &mut Vec<Event>) {
        if self.fireballs.is_empty() {
            return;
        }
        let mut blasts: Vec<Fireball> = Vec::new();
        for b in self.fireballs.iter_mut() {
            b.life -= DT;
            b.vz -= 30.0 * DT; // flames sag a little as they fly
            b.x += b.vx * DT;
            b.y += b.vy * DT;
            b.z += b.vz * DT;
            // Flaming trail.
            if self.rng.below(2) == 0 {
                self.fx.smoke(&mut self.rng, b.x, b.y, b.z, 0.0, 0.0, 24.0, 0.35, b.r * 0.75, 0xff9c2e, 0.8);
            }
            let ground = b.z <= 1.0;
            let mut wall = false;
            if !ground {
                for blk in &self.city.blocks {
                    if blk.kind != crate::city::BlockKind::Buildings {
                        continue;
                    }
                    for lot in blk.buildings.iter().flatten() {
                        if b.x > lot.x - b.r && b.x < lot.x + lot.w + b.r
                            && b.y > lot.y - b.r && b.y < lot.y + lot.h + b.r
                            && b.z < lot.height
                        {
                            wall = true;
                            break;
                        }
                    }
                    if wall {
                        break;
                    }
                }
            }
            if ground || wall || b.life <= 0.0 {
                blasts.push(*b);
            }
        }
        if blasts.is_empty() {
            return;
        }
        self.fireballs.retain(|b| !blasts.contains(b));
        for b in blasts {
            self.explode(b.x, b.y, b.z.max(1.0), events);
        }
    }

    /// One fireball blast: FX, plus knock-down-and-ignite every building
    /// whose footprint lies within the blast radius.
    fn explode(&mut self, x: f64, y: f64, z: f64, events: &mut Vec<Event>) {
        self.fx.sparks(&mut self.rng, x, y, z, 26, 330.0, 0xffc94a);
        self.fx.sparks(&mut self.rng, x, y, z, 16, 230.0, 0xff7a1a);
        self.fx.debris(&mut self.rng, x, y, 12, 0x4a413a);
        for _ in 0..7 {
            self.fx.smoke(&mut self.rng, x, y, z, 0.0, 0.0, 30.0, 0.7, 9.0, 0x6b6258, 0.5);
        }
        let mut destroyed = false;
        for (block, blk) in self.city.blocks.iter().enumerate() {
            if blk.kind != crate::city::BlockKind::Buildings {
                continue;
            }
            for (lot, b) in blk.buildings.iter().enumerate() {
                let b = match b {
                    Some(b) => *b,
                    None => continue,
                };
                let px = x.clamp(b.x, b.x + b.w);
                let py = y.clamp(b.y, b.y + b.h);
                let dd = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
                if dd <= 46.0 {
                    let id = building_id(block, lot);
                    match self.building_fires.iter_mut().find(|f| f.id == id) {
                        Some(f) => {
                            // Re-lit: re-ignite, and a fresh blast on a
                            // half-ruined slab knocks it further down.
                            f.burn = f.burn.max(26.0);
                            f.collapse = (f.collapse + 0.25).min(1.0);
                        }
                        None => self.building_fires.push(BuildingFire {
                            id,
                            collapse: 0.0,
                            burn: 26.0,
                        }),
                    }
                    destroyed = true;
                }
            }
        }
        if destroyed {
            events.push(Event::BuildingDown);
            self.set_msg("BUILDING DOWN — THE CROWD RALLIES WITH WATER", 3.0);
        }
    }

    /// Drive the collapse + fire of every hit building: crumbling rubble and
    /// dust while it comes down, roaring flames and rolling black smoke
    /// while it burns.
    fn update_building_fires(&mut self) {
        if self.building_fires.is_empty() {
            return;
        }
        for f in self.building_fires.iter_mut() {
            let (block, lot) = block_lot(f.id);
            let b = match self.city.blocks.get(block).and_then(|bl| bl.buildings[lot]) {
                Some(b) => b,
                None => continue,
            };
            if f.collapse < 1.0 {
                f.collapse = (f.collapse + DT / 1.6).min(1.0);
                // Rubble tumbling down from the coming-down slab.
                if self.rng.below(3) == 0 {
                    let dx = b.x + self.rng.range(0.0, b.w);
                    let dy = b.y + self.rng.range(0.0, b.h);
                    self.fx.debris(&mut self.rng, dx, dy, 3, 0x4a413a);
                }
                if self.rng.below(4) == 0 {
                    self.fx.dust(&mut self.rng, b.x + b.w / 2.0, b.y + b.h / 2.0, 2, 0xb0a58e);
                }
            }
            if f.burn > 0.0 {
                f.burn -= DT;
                let (fx, fy) = (
                    b.x + self.rng.range(2.0, b.w - 2.0),
                    b.y + self.rng.range(2.0, b.h - 2.0),
                );
                // Roaring flames (once collapsed they lick the rubble).
                let fh = if f.collapse >= 1.0 {
                    8.0
                } else {
                    4.0 + b.height * (1.0 - f.collapse) * 0.4
                };
                if self.rng.below(2) == 0 {
                    self.fx.smoke(&mut self.rng, fx, fy, fh, 0.0, 0.0, 60.0, 0.55, 7.0, 0xff8c1e, 0.9);
                    self.fx.smoke(&mut self.rng, fx, fy, fh, 0.0, 0.0, 70.0, 0.4, 5.0, 0xffd23c, 0.9);
                }
                // Rolling black smoke.
                if self.rng.below(3) == 0 {
                    let (sx, sy) = (
                        b.x + self.rng.range(4.0, b.w - 4.0),
                        b.y + self.rng.range(4.0, b.h - 4.0),
                    );
                    self.fx.smoke(&mut self.rng, sx, sy, 10.0, 0.0, 0.0, 55.0, 1.8, 9.0, 0x2b2622, 0.5);
                }
            }
        }
    }

    /// Citizens come running to the streets: nearby pedestrians drop what
    /// they are doing, sprint to the burning building, and hose the flames
    /// down with water until the fire is out.
    fn update_firefighters(&mut self) {
        // Release peds whose fire has been doused.
        for p in self.peds.iter_mut() {
            if let Some(id) = p.firefight {
                if !self.building_fires.iter().any(|f| f.id == id && f.burn > 0.0) {
                    p.firefight = None;
                }
            }
        }
        for fi in 0..self.building_fires.len() {
            let (id, burning) = {
                let f = &self.building_fires[fi];
                (f.id, f.burn > 0.0)
            };
            if !burning {
                continue;
            }
            let (block, lot) = block_lot(id);
            let b = match self.city.blocks.get(block).and_then(|bl| bl.buildings[lot]) {
                Some(b) => b,
                None => continue,
            };
            let (cx, cy) = b.center();
            let crew = self.peds.iter().filter(|p| p.firefight == Some(id)).count();
            let mut enlisted = 0;
            let mut doused = 0.0;
            let rng = &mut self.rng;
            let fx = &mut self.fx;
            let city = &self.city;
            for i in 0..self.peds.len() {
                let p = &mut self.peds[i];
                if !matches!(p.state, crate::ped::PedState::Alive) {
                    continue;
                }
                if p.firefight != Some(id) {
                    // Maybe recruit this passerby into the crew.
                    if crew + enlisted >= 6 {
                        continue;
                    }
                    if p.firefight.is_some() {
                        continue; // busy at another blaze
                    }
                    let d = ((p.x - cx).powi(2) + (p.y - cy).powi(2)).sqrt();
                    if d > 620.0 {
                        continue; // too far to hear it
                    }
                    p.firefight = Some(id);
                    enlisted += 1;
                }
                // Run to the wall, then hose the flames down.
                let tx = p.x.clamp(b.x, b.x + b.w);
                let ty = p.y.clamp(b.y, b.y + b.h);
                let ddx = tx - p.x;
                let ddy = ty - p.y;
                let dd = (ddx * ddx + ddy * ddy).sqrt();
                if dd > 30.0 {
                    let (ux, uy) = (ddx / dd.max(0.001), ddy / dd.max(0.001));
                    let (mut nx, mut ny) = (p.x + ux * 150.0 * DT, p.y + uy * 150.0 * DT);
                    if let Some((x, y, _, _)) = city.collide_circle(nx, ny, 6.0) {
                        (nx, ny) = (x, y);
                    }
                    p.x = nx;
                    p.y = ny;
                    p.heading = ddy.atan2(ddx);
                } else {
                    // At the wall: an arcing spray of water at the fire.
                    let (ax, ay) = (cx - p.x, cy - p.y);
                    let d = (ax * ax + ay * ay).sqrt().max(1.0);
                    p.heading = ay.atan2(ax);
                    let (ux, uy) = (ax / d, ay / d);
                    for _ in 0..3 {
                        let sp = 130.0 + rng.range(0.0, 60.0);
                        let vz = 55.0 + rng.range(-15.0, 15.0);
                        fx.water(rng, p.x + ux * 8.0, p.y + uy * 8.0, 10.0, ux * sp, uy * sp, vz);
                    }
                    doused += 1.0;
                }
            }
            // The water cools the fire (each citizen at the wall helps).
            if doused > 0.0 {
                self.building_fires[fi].burn =
                    (self.building_fires[fi].burn - doused * DT * 0.18).max(0.0);
            }
        }
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
    fn g_summons_the_dragon_and_controls_it() {
        let mut s = idle_state();
        let mut inp = Input::new();
        for _ in 0..60 {
            s.tick(&mut inp);
        }

        // G is a dedicated summon key: it works from a parked car.
        keypress(&mut inp, "g");
        s.tick(&mut inp);
        assert!(s.in_dragon, "G should summon + mount the dragon");
        assert!(s.view_3d, "mounting the dragon should switch to 3D");
        assert!(s.wildlife.dragon.controlled, "the dragon should be player-controlled");
        // The dragon drops in at the player's position.
        let (px, py) = (s.car.x, s.car.y);
        let d = s.wildlife.dragon;
        let dd = ((px - d.x).powi(2) + (py - d.y).powi(2)).sqrt();
        assert!(dd < 1.0, "dragon should appear at the player: {} px off", dd);
        inp.key_up("g");
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

        // Exit: E drops to the street below the dragon, on foot.
        keypress(&mut inp, "e");
        s.tick(&mut inp);
        assert!(!s.in_dragon, "E should exit the dragon");
        assert!(s.on_foot, "exiting the dragon should put you on foot");
        assert!(!s.wildlife.dragon.controlled, "the dragon should fly on its own again");
        let (px, py) = s.player_pos();
        let d = &s.wildlife.dragon;
        let dist = ((px - d.x).powi(2) + (py - d.y).powi(2)).sqrt();
        assert!(dist < 60.0, "should land near the dragon: {} px off", dist);
    }

    #[test]
    fn g_summons_the_dragon_even_while_driving_the_car() {
        let mut s = idle_state();
        let mut inp = Input::new();
        // G is its own key (not a movement key), so it summons at any speed.
        inp.key_down("w");
        for _ in 0..120 {
            s.tick(&mut inp);
        }
        assert!(s.car.speed() > 100.0, "the car should be moving");
        inp.key_up("w");
        inp.end_frame();
        keypress(&mut inp, "g");
        s.tick(&mut inp);
        assert!(s.in_dragon, "G should summon the dragon even at speed");
    }

    #[test]
    fn g_summons_the_dragon_on_foot() {
        let mut s = idle_state();
        let mut inp = Input::new();
        s.on_foot = true;
        s.foot_x = s.car.x;
        s.foot_y = s.car.y;
        keypress(&mut inp, "g");
        s.tick(&mut inp);
        assert!(s.in_dragon, "G should summon the dragon on foot");
        assert!(!s.on_foot, "the player should be riding the dragon");
        inp.key_up("g");
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

    /// Put the player on the back of the first elephant and press E (board).
    fn mount_first_elephant(s: &mut GameState, inp: &mut Input) -> usize {
        s.on_foot = true;
        s.foot_x = s.wildlife.elephants[0].x;
        s.foot_y = s.wildlife.elephants[0].y;
        keypress(inp, "e");
        s.tick(inp);
        assert!(s.riding == Some(0), "E next to an elephant should board it");
        assert!(!s.on_foot, "riding should take the player off the street");
        0
    }

    #[test]
    fn e_mounts_an_elephant_and_it_carries_the_player() {
        let mut s = idle_state();
        let mut inp = Input::new();
        let idx = mount_first_elephant(&mut s, &mut inp);
        // The rider is glued to the elephant, which wanders on its own.
        let (ex, ey) = (s.wildlife.elephants[idx].x, s.wildlife.elephants[idx].y);
        let (px0, py0) = s.player_pos();
        assert_eq!((px0, py0), (ex, ey), "rider should sit on the elephant");
        inp.key_up("e");
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
    fn e_again_dismounts_back_to_the_street() {
        let mut s = idle_state();
        let mut inp = Input::new();
        mount_first_elephant(&mut s, &mut inp);
        inp.key_up("e");
        keypress(&mut inp, "e");
        s.tick(&mut inp);
        assert!(s.riding.is_none(), "second E should dismount");
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
    fn e_far_from_everything_boards_nothing() {
        let mut s = idle_state();
        let mut inp = Input::new();
        s.on_foot = true;
        s.foot_x = 100.0;
        s.foot_y = 100.0;
        keypress(&mut inp, "e");
        s.tick(&mut inp);
        assert!(s.riding.is_none(), "E far from everything should not board");
        assert!(s.on_foot, "player should stay on foot");
        assert!(!s.in_plane && !s.in_dragon, "nothing else should trigger");
    }

    #[test]
    fn special_actions_list_what_is_available() {
        let mut s = idle_state();
        let mut inp = Input::new();
        for _ in 0..30 {
            s.tick(&mut inp);
        }
        // In the car: exit + summon keys + view keys.
        let a = s.special_actions();
        assert!(a.iter().any(|x| x.key == "E" && x.label == "EXIT CAR (WHEN SLOW)"));
        assert!(a.iter().any(|x| x.key == "F" && x.label == "SUMMON AIRPLANE"));
        assert!(a.iter().any(|x| x.key == "G" && x.label == "SUMMON DRAGON"));
        assert!(a.iter().any(|x| x.key == "V" && x.label == "3D CHASE-CAM"));
        assert!(a.iter().all(|x| x.key != "M"), "auto-land is plane-only");
        // On foot next to the car: E boards the car.
        keypress(&mut inp, "e");
        s.tick(&mut inp);
        assert!(s.on_foot);
        assert_eq!(s.board_target(), Some(BoardTarget::Car));
        let a = s.special_actions();
        assert!(a.iter().any(|x| x.key == "E" && x.label == "ENTER CAR"), "{:?}", a);
        // In the plane: M appears, F disappears.
        s.in_plane = true;
        s.on_foot = false;
        let a = s.special_actions();
        assert!(a.iter().any(|x| x.key == "M" && x.label == "AUTO-LAND"));
        assert!(a.iter().any(|x| x.key == "E" && x.label == "EXIT AIRPLANE (WHEN SLOW)"));
        assert!(a.iter().all(|x| x.key != "F"), "no summon-plane while in the plane");
        // On the dragon: exit + fireball, G disappears.
        s.in_plane = false;
        s.in_dragon = true;
        let a = s.special_actions();
        assert!(a.iter().any(|x| x.key == "E" && x.label == "EXIT DRAGON (DROP DOWN)"));
        assert!(a.iter().any(|x| x.key == "LMB" && x.label == "FIREBALL"));
        assert!(a.iter().all(|x| x.key != "G"), "no summon-dragon while on the dragon");
        // Riding an elephant: E jumps off.
        s.in_dragon = false;
        s.riding = Some(0);
        let a = s.special_actions();
        assert!(a.iter().any(|x| x.key == "E" && x.label == "JUMP OFF ELEPHANT"));
    }

    /// Put the player on the dragon, low in the air just in front of the
    /// first real building, facing it. A fireball from the mouth then
    /// detonates against the building's front wall. Returns the building
    /// and its id.
    fn dragon_over_a_building(s: &mut GameState) -> (crate::city::Building, u32) {
        let (block, lot) = s
            .city
            .blocks
            .iter()
            .enumerate()
            .find_map(|(k, b)| {
                b.buildings
                    .iter()
                    .enumerate()
                    .find(|(_, l)| l.is_some())
                    .map(|(l, _)| (k, l))
            })
            .unwrap();
        let b = s.city.blocks[block].buildings[lot].unwrap();
        let (cx, cy) = b.center();
        s.in_dragon = true;
        s.wildlife.dragon.controlled = true;
        s.wildlife.dragon.x = cx;
        s.wildlife.dragon.y = cy - b.h / 2.0 - 14.0; // just in front of the front edge
        s.wildlife.dragon.z = 30.0; // low enough that the shot hits the facade
        s.wildlife.dragon.heading = std::f64::consts::FRAC_PI_2; // toward the building
        s.cam_x = s.wildlife.dragon.x;
        s.cam_y = s.wildlife.dragon.y;
        let id = crate::state::building_id(block, lot);
        (b, id)
    }

    #[test]
    fn dragon_fireball_destroys_and_ignites_buildings() {
        let mut s = idle_state();
        let (_, id) = dragon_over_a_building(&mut s);
        assert!(s.building_fire(id).is_none(), "building starts clean");
        let mut inp = Input::new();
        // Click the left mouse button: a fireball leaves the dragon's mouth
        // and blasts the facade on the very first tick.
        inp.mouse_down();
        let evs = s.tick(&mut inp);
        assert!(
            evs.iter().any(|e| matches!(e, Event::Fireball)),
            "click should fire a fireball"
        );
        assert!(
            evs.iter().any(|e| matches!(e, Event::BuildingDown)),
            "the blast should bring the building down"
        );
        let fire0 = *s.building_fire(id).expect("the building should be hit");
        assert!(fire0.burn > 0.0, "the building should be burning: {fire0:?}");
        // Hold the button: a reload gate streams fireballs (~5/s).
        let mut shots = 0;
        for _ in 0..180 {
            shots += s.tick(&mut inp).iter().filter(|e| matches!(e, Event::Fireball)).count();
        }
        inp.mouse_up();
        assert!(shots >= 5, "holding LMB should stream fireballs, got {shots}");
        assert!(s.fireballs.is_empty(), "all fireballs should have landed");
        let fire1 = *s.building_fire(id).expect("the building should still be hit");
        assert!(fire1.collapse > 0.0, "the building should be coming down: {fire1:?}");
        // The collapse finishes and the fire burns out on its own; the
        // rubble persists as a ruin.
        for _ in 0..60 * 40 {
            s.tick(&mut inp);
            if s.building_fires.iter().find(|f| f.id == id).map_or(false, |f| f.burn <= 0.0) {
                break;
            }
        }
        let fire = s.building_fire(id).expect("the ruin should be remembered");
        assert_eq!(fire.collapse, 1.0, "the building should be fully collapsed");
        assert!(fire.burn <= 0.0, "the fire should burn out on its own");
    }

    #[test]
    fn citizens_rush_to_fight_the_fire_with_water() {
        let mut s = idle_state();
        let (b, id) = dragon_over_a_building(&mut s);
        let (cx, cy) = b.center();
        // Put a living ped on the sidewalk in front of the burning building.
        let mut ped_i = None;
        for (i, p) in s.peds.iter_mut().enumerate() {
            if matches!(p.state, crate::ped::PedState::Alive) {
                p.x = cx;
                p.y = cy - b.h / 2.0 - 12.0;
                p.firefight = None;
                ped_i = Some(i);
                break;
            }
        }
        let pi = ped_i.unwrap();
        s.building_fires.push(crate::state::BuildingFire { id, collapse: 0.0, burn: 26.0 });
        let mut inp = Input::new();
        // The ped notices the fire and joins the water crew.
        for _ in 0..300 {
            s.tick(&mut inp);
            if s.peds[pi].firefight == Some(id) {
                break;
            }
        }
        assert_eq!(s.peds[pi].firefight, Some(id), "ped should have enlisted to fight the fire");
        // The crew works at the wall until the water cuts the fire out.
        let start_burn = s.building_fires.iter().find(|f| f.id == id).unwrap().burn;
        assert!(start_burn > 0.0);
        let mut water_seen = false;
        for _ in 0..60 * 60 {
            s.tick(&mut inp);
            if s.fx.particles.iter().any(|p| p.kind == crate::fx::PKind::Water) {
                water_seen = true;
            }
            let f = s.building_fires.iter().find(|f| f.id == id).unwrap();
            if f.burn <= 0.0 {
                break;
            }
        }
        assert!(water_seen, "the crew should be throwing water at the fire");
        let f = s.building_fires.iter().find(|f| f.id == id).unwrap();
        assert!(f.burn <= 0.0, "the crew should hose the fire out");
        assert_eq!(s.peds[pi].firefight, None, "peds stand down once the fire is out");
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

