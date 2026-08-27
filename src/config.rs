//! Player-configurable key bindings and mouse behavior. Pure (no DOM), so
//! it is unit-testable on the host.
//!
//! Everything on the in-game config page (press `ESC`) is backed by a
//! [`Config`], which round-trips to a plain `config.ini` file via
//! [`Config::to_ini`] / [`Config::from_ini`]. Saving downloads `config.ini`
//! (the game also keeps a copy in `localStorage`); on boot the game loads
//! `config.ini` from the page directory if it exists.

use std::collections::HashSet;

/// A key, normalized (lowercase, space written as " "). Matches the DOM
/// `KeyboardEvent.key` values the game listens for ("w", "shift", "f1", …).
pub type Key = String;

/// A mouse button: left, middle or right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Lmb,
    Mmb,
    Rmb,
}

impl MouseButton {
    pub const LMB: Self = Self::Lmb;
    pub const MMB: Self = Self::Mmb;
    pub const RMB: Self = Self::Rmb;

    pub const ALL: [Self; 3] = [Self::Lmb, Self::Mmb, Self::Rmb];

    pub fn index(self) -> u8 {
        match self {
            Self::Lmb => 0,
            Self::Mmb => 1,
            Self::Rmb => 2,
        }
    }

    /// INI name: `lmb` / `mmb` / `rmb`.
    pub fn name(self) -> &'static str {
        match self {
            Self::LMB => "lmb",
            Self::MMB => "mmb",
            _ => "rmb",
        }
    }

    /// INI / config-page display: `LMB` / `MMB` / `RMB`.
    pub fn display(self) -> &'static str {
        match self {
            Self::Lmb => "LMB",
            Self::Mmb => "MMB",
            Self::Rmb => "RMB",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "lmb" | "left" | "0" => Some(Self::LMB),
            "mmb" | "middle" | "1" => Some(Self::MMB),
            "rmb" | "right" | "2" => Some(Self::RMB),
            _ => None,
        }
    }

    /// Cycle to the next/previous button (used by the config page).
    pub fn cycle(self, dir: i32) -> Self {
        let n = Self::ALL.len() as i32;
        let i = (self.index() as i32 + dir).rem_euclid(n);
        Self::ALL[i as usize]
    }
}

/// Everything that can be changed in the config page. The variant order is
/// the row order of the on-screen list (movement, then mouse, then specials).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binding {
    // Movement
    Forward,
    Back,
    SteerLeft,
    SteerRight,
    BoostClimb,
    HandbrakeDive,
    // Mouse
    ThrottleButton,
    BrakeButton,
    FireballButton,
    WalkButton,
    Sensitivity,
    // Special actions
    EnterExit,
    SummonAirplane,
    SummonDragon,
    AutoLand,
    AutoMode,
    View,
    ResetCamera,
    Pause,
    Recenter,
    ConfigPage,
}

impl Binding {
    pub const COUNT: usize = 21;

    pub const ALL: [Binding; Self::COUNT] = [
        Binding::Forward,
        Binding::Back,
        Binding::SteerLeft,
        Binding::SteerRight,
        Binding::BoostClimb,
        Binding::HandbrakeDive,
        Binding::ThrottleButton,
        Binding::BrakeButton,
        Binding::FireballButton,
        Binding::WalkButton,
        Binding::Sensitivity,
        Binding::EnterExit,
        Binding::SummonAirplane,
        Binding::SummonDragon,
        Binding::AutoLand,
        Binding::AutoMode,
        Binding::View,
        Binding::ResetCamera,
        Binding::Pause,
        Binding::Recenter,
        Binding::ConfigPage,
    ];

    pub fn from_usize(i: usize) -> Binding {
        Self::ALL[i % Self::COUNT]
    }

    pub fn as_usize(self) -> usize {
        Self::ALL.iter().position(|&b| b == self).unwrap()
    }

    /// Row label in the config page.
    pub fn label(self) -> &'static str {
        match self {
            Binding::Forward => "FORWARD / ACCELERATE",
            Binding::Back => "BACK / BRAKE / REVERSE",
            Binding::SteerLeft => "STEER LEFT",
            Binding::SteerRight => "STEER RIGHT",
            Binding::BoostClimb => "RUN · BOOST · CLIMB",
            Binding::HandbrakeDive => "HANDBRAKE · DIVE",
            Binding::ThrottleButton => "FULL THROTTLE (HOLD) — PLANE",
            Binding::BrakeButton => "BRAKE (HOLD) — PLANE / DRAGON",
            Binding::FireballButton => "FIREBALL (HOLD) — DRAGON",
            Binding::WalkButton => "WALK FORWARD (HOLD) — ON FOOT",
            Binding::Sensitivity => "MOUSE SENSITIVITY",
            Binding::EnterExit => "BOARD / EXIT",
            Binding::SummonAirplane => "SUMMON AIRPLANE",
            Binding::SummonDragon => "SUMMON DRAGON",
            Binding::AutoLand => "AUTO-LAND — PLANE",
            Binding::AutoMode => "AUTO MODE",
            Binding::View => "VIEW — TOP-DOWN / 3D",
            Binding::ResetCamera => "RESET CAMERA — 3D",
            Binding::Pause => "PAUSE",
            Binding::Recenter => "RE-CENTER CAMERA",
            Binding::ConfigPage => "CONFIG PAGE",
        }
    }

    /// Which section header the row is drawn under.
    pub fn section(self) -> Option<&'static str> {
        match self {
            Binding::Forward => Some("MOVEMENT"),
            Binding::ThrottleButton => Some("MOUSE"),
            Binding::EnterExit => Some("SPECIAL ACTIONS"),
            _ => None,
        }
    }
}

/// Normalize a raw key name: lowercase, "space" → " ", "esc" → "escape".
pub fn normalize_key(k: &str) -> Key {
    let k = k.trim().to_ascii_lowercase();
    if k == "space" {
        return " ".to_string();
    }
    if k == "esc" {
        return "escape".to_string();
    }
    k
}

/// Human-readable key name for the HUD / config page.
pub fn key_display(k: &str) -> String {
    match k {
        " " => "SPACE".to_string(),
        "escape" => "ESC".to_string(),
        "arrowup" => "ARROW UP".to_string(),
        "arrowdown" => "ARROW DOWN".to_string(),
        "arrowleft" => "ARROW LEFT".to_string(),
        "arrowright" => "ARROW RIGHT".to_string(),
        _ => k.to_ascii_uppercase(),
    }
}

/// All player-configurable controls: movement keys, mouse behavior and the
/// special actions. Written to / read from `config.ini`.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    // Movement (the arrow keys always work alongside these as a second set).
    pub forward: Key,
    pub back: Key,
    pub steer_left: Key,
    pub steer_right: Key,
    /// Run on foot, boost in the car, climb in the plane / dragon.
    pub boost_climb: Key,
    /// Handbrake in the car, dive in the plane / dragon.
    pub handbrake_dive: Key,
    // Mouse behavior.
    pub throttle_button: MouseButton,
    pub brake_button: MouseButton,
    pub fireball_button: MouseButton,
    pub walk_button: MouseButton,
    /// Multiplies the camera-orbit and flight-drag sensitivity (0.25..4).
    pub mouse_sensitivity: f64,
    // Special actions.
    pub enter_exit: Key,
    pub summon_airplane: Key,
    pub summon_dragon: Key,
    pub auto_land: Key,
    pub auto_mode: Key,
    pub view: Key,
    pub reset_camera: Key,
    pub pause: Key,
    pub recenter: Key,
    pub config_page: Key,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            forward: "w".into(),
            back: "s".into(),
            steer_left: "a".into(),
            steer_right: "d".into(),
            boost_climb: "shift".into(),
            handbrake_dive: " ".into(),
            throttle_button: MouseButton::LMB,
            brake_button: MouseButton::RMB,
            fireball_button: MouseButton::LMB,
            walk_button: MouseButton::RMB,
            mouse_sensitivity: 1.0,
            enter_exit: "e".into(),
            summon_airplane: "f".into(),
            summon_dragon: "g".into(),
            auto_land: "m".into(),
            auto_mode: "f1".into(),
            view: "v".into(),
            reset_camera: "c".into(),
            pause: "p".into(),
            recenter: "r".into(),
            config_page: "escape".into(),
        }
    }
}

impl Config {
    /// The key bound to a keyboard binding (`None` for mouse rows).
    pub fn key(&self, b: Binding) -> Option<&Key> {
        Some(match b {
            Binding::Forward => &self.forward,
            Binding::Back => &self.back,
            Binding::SteerLeft => &self.steer_left,
            Binding::SteerRight => &self.steer_right,
            Binding::BoostClimb => &self.boost_climb,
            Binding::HandbrakeDive => &self.handbrake_dive,
            Binding::EnterExit => &self.enter_exit,
            Binding::SummonAirplane => &self.summon_airplane,
            Binding::SummonDragon => &self.summon_dragon,
            Binding::AutoLand => &self.auto_land,
            Binding::AutoMode => &self.auto_mode,
            Binding::View => &self.view,
            Binding::ResetCamera => &self.reset_camera,
            Binding::Pause => &self.pause,
            Binding::Recenter => &self.recenter,
            Binding::ConfigPage => &self.config_page,
            Binding::ThrottleButton
            | Binding::BrakeButton
            | Binding::FireballButton
            | Binding::WalkButton
            | Binding::Sensitivity => return None,
        })
    }

    /// Bind a key, unbinding it from any other row first (the config-page key
    /// stays unique: nothing may steal it from `ConfigPage`).
    pub fn set_key(&mut self, b: Binding, k: Key) {
        let k = normalize_key(&k);
        if k.is_empty() || self.key(b).is_none() {
            return;
        }
        // The config-page key can never be stolen: the page must stay openable.
        if b != Binding::ConfigPage && k == self.config_page {
            return;
        }
        // The key may live on only one row: steal it from any other row.
        for other in Binding::ALL.iter().copied() {
            if other != b && self.key(other) == Some(&k) {
                *self.key_mut(other).unwrap() = String::new();
            }
        }
        *self.key_mut(b).unwrap() = k;
    }

    fn key_mut(&mut self, b: Binding) -> Option<&mut Key> {
        Some(match b {
            Binding::Forward => &mut self.forward,
            Binding::Back => &mut self.back,
            Binding::SteerLeft => &mut self.steer_left,
            Binding::SteerRight => &mut self.steer_right,
            Binding::BoostClimb => &mut self.boost_climb,
            Binding::HandbrakeDive => &mut self.handbrake_dive,
            Binding::EnterExit => &mut self.enter_exit,
            Binding::SummonAirplane => &mut self.summon_airplane,
            Binding::SummonDragon => &mut self.summon_dragon,
            Binding::AutoLand => &mut self.auto_land,
            Binding::AutoMode => &mut self.auto_mode,
            Binding::View => &mut self.view,
            Binding::ResetCamera => &mut self.reset_camera,
            Binding::Pause => &mut self.pause,
            Binding::Recenter => &mut self.recenter,
            Binding::ConfigPage => &mut self.config_page,
            Binding::ThrottleButton
            | Binding::BrakeButton
            | Binding::FireballButton
            | Binding::WalkButton
            | Binding::Sensitivity => return None,
        })
    }

    /// The mouse button bound to a mouse-button row (`None` otherwise).
    pub fn mouse_button(&self, b: Binding) -> Option<MouseButton> {
        Some(match b {
            Binding::ThrottleButton => self.throttle_button,
            Binding::BrakeButton => self.brake_button,
            Binding::FireballButton => self.fireball_button,
            Binding::WalkButton => self.walk_button,
            _ => return None,
        })
    }

    pub fn set_mouse_button(&mut self, b: Binding, m: MouseButton) {
        match b {
            Binding::ThrottleButton => self.throttle_button = m,
            Binding::BrakeButton => self.brake_button = m,
            Binding::FireballButton => self.fireball_button = m,
            Binding::WalkButton => self.walk_button = m,
            _ => {}
        }
    }

    /// Cycle the mouse button on a mouse-button row (config page ←/→).
    pub fn cycle_mouse_button(&mut self, b: Binding, dir: i32) {
        if let Some(m) = self.mouse_button(b) {
            self.set_mouse_button(b, m.cycle(dir));
        }
    }

    pub fn sensitivity(&self) -> f64 {
        self.mouse_sensitivity
    }

    pub fn set_sensitivity(&mut self, v: f64) {
        if v.is_finite() {
            self.mouse_sensitivity = v.clamp(0.25, 4.0);
        }
    }

    /// Every currently bound key (for the input layer to preventDefault on).
    pub fn all_keys(&self) -> HashSet<String> {
        let mut s = HashSet::new();
        for b in Binding::ALL.iter().copied() {
            if let Some(k) = self.key(b) {
                if !k.is_empty() {
                    s.insert(k.clone());
                }
            }
        }
        s
    }

    /// Serialize to the `config.ini` format.
    pub fn to_ini(&self) -> String {
        let mut s = String::new();
        s.push_str("# GTA VI — Web Edition: key bindings & mouse behavior\n");
        s.push_str("# Generated by the in-game config page (ESC). Edit by hand, or\n");
        s.push_str("# use the config page (S = save, L = load, X = defaults).\n\n");
        s.push_str("[movement]\n");
        s.push_str(&format!("forward = {}\n", self.forward.trim_start_matches(' ')));
        s.push_str(&format!("back = {}\n", ini_key(&self.back)));
        s.push_str(&format!("steer_left = {}\n", ini_key(&self.steer_left)));
        s.push_str(&format!("steer_right = {}\n", ini_key(&self.steer_right)));
        s.push_str(&format!("boost_climb = {}\n", ini_key(&self.boost_climb)));
        s.push_str(&format!("handbrake_dive = {}\n", ini_key(&self.handbrake_dive)));
        s.push_str("\n[mouse]\n");
        s.push_str(&format!("throttle_button = {}\n", self.throttle_button.name()));
        s.push_str(&format!("brake_button = {}\n", self.brake_button.name()));
        s.push_str(&format!("fireball_button = {}\n", self.fireball_button.name()));
        s.push_str(&format!("walk_button = {}\n", self.walk_button.name()));
        s.push_str(&format!("sensitivity = {:.2}\n", self.mouse_sensitivity));
        s.push_str("\n[specials]\n");
        s.push_str(&format!("enter_exit = {}\n", ini_key(&self.enter_exit)));
        s.push_str(&format!("summon_airplane = {}\n", ini_key(&self.summon_airplane)));
        s.push_str(&format!("summon_dragon = {}\n", ini_key(&self.summon_dragon)));
        s.push_str(&format!("auto_land = {}\n", ini_key(&self.auto_land)));
        s.push_str(&format!("auto_mode = {}\n", ini_key(&self.auto_mode)));
        s.push_str(&format!("view = {}\n", ini_key(&self.view)));
        s.push_str(&format!("reset_camera = {}\n", ini_key(&self.reset_camera)));
        s.push_str(&format!("pause = {}\n", ini_key(&self.pause)));
        s.push_str(&format!("recenter = {}\n", ini_key(&self.recenter)));
        s.push_str(&format!("config_page = {}\n", ini_key(&self.config_page)));
        s
    }

    /// Parse `config.ini` text. Lenient: unknown lines/sections are ignored,
    /// missing values keep the defaults.
    pub fn from_ini(text: &str) -> Self {
        let mut c = Self::default();
        let mut section = String::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                section = inner.trim().to_ascii_lowercase();
                continue;
            }
            let (name, value) = match line.split_once('=') {
                Some(t) => (t.0.trim().to_ascii_lowercase(), t.1.trim()),
                None => continue,
            };
            match (section.as_str(), name.as_str()) {
                ("movement", "forward") => c.forward = normalize_key(value),
                ("movement", "back") => c.back = normalize_key(value),
                ("movement", "steer_left") => c.steer_left = normalize_key(value),
                ("movement", "steer_right") => c.steer_right = normalize_key(value),
                ("movement", "boost_climb" | "boost" | "run") => {
                    c.boost_climb = normalize_key(value)
                }
                ("movement", "handbrake_dive" | "handbrake" | "dive") => {
                    c.handbrake_dive = normalize_key(value)
                }
                ("mouse", "throttle_button") => {
                    if let Some(m) = MouseButton::from_name(value) {
                        c.throttle_button = m;
                    }
                }
                ("mouse", "brake_button") => {
                    if let Some(m) = MouseButton::from_name(value) {
                        c.brake_button = m;
                    }
                }
                ("mouse", "fireball_button") => {
                    if let Some(m) = MouseButton::from_name(value) {
                        c.fireball_button = m;
                    }
                }
                ("mouse", "walk_button" | "walk_forward_button") => {
                    if let Some(m) = MouseButton::from_name(value) {
                        c.walk_button = m;
                    }
                }
                ("mouse", "sensitivity") => {
                    if let Ok(v) = value.parse::<f64>() {
                        c.set_sensitivity(v);
                    }
                }
                ("specials", "enter_exit" | "enter" | "exit") => c.enter_exit = normalize_key(value),
                ("specials", "summon_airplane" | "summon_plane" | "plane") => {
                    c.summon_airplane = normalize_key(value)
                }
                ("specials", "summon_dragon" | "dragon") => {
                    c.summon_dragon = normalize_key(value)
                }
                ("specials", "auto_land" | "autoland") => c.auto_land = normalize_key(value),
                ("specials", "auto_mode" | "auto") => c.auto_mode = normalize_key(value),
                ("specials", "view") => c.view = normalize_key(value),
                ("specials", "reset_camera") => c.reset_camera = normalize_key(value),
                ("specials", "pause") => c.pause = normalize_key(value),
                ("specials", "recenter" | "re_center") => c.recenter = normalize_key(value),
                ("specials", "config_page" | "config") => c.config_page = normalize_key(value),
                _ => {}
            }
        }
        c
    }
}

/// Write a key into the INI (space is written as `space`).
fn ini_key(k: &str) -> &str {
    if k == " " {
        "space"
    } else {
        k
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ini_round_trip() {
        let c = Config::default();
        let parsed = Config::from_ini(&c.to_ini());
        assert_eq!(c, parsed);
    }

    #[test]
    fn custom_ini_parses() {
        let ini = r#"
# my bindings
[movement]
forward = k
back = j
steer_left = h
steer_right = l
boost_climb = r
handbrake_dive = space

[mouse]
throttle_button = mmb
brake_button = lmb
sensitivity = 2.5

[specials]
view = z
config_page = f1
"#;
        let c = Config::from_ini(ini);
        assert_eq!(c.forward, "k");
        assert_eq!(c.back, "j");
        assert_eq!(c.steer_left, "h");
        assert_eq!(c.steer_right, "l");
        assert_eq!(c.boost_climb, "r");
        assert_eq!(c.handbrake_dive, " ");
        assert_eq!(c.throttle_button, MouseButton::MMB);
        assert_eq!(c.brake_button, MouseButton::LMB);
        assert_eq!(c.mouse_sensitivity, 2.5);
        assert_eq!(c.view, "z");
        assert_eq!(c.config_page, "f1");
        // Untouched values keep the defaults.
        assert_eq!(c.pause, "p");
        assert_eq!(c.fireball_button, MouseButton::LMB);
    }

    #[test]
    fn unknown_lines_are_ignored_and_clamped() {
        let ini = "[mouse]\nsensitivity = 99\n[bogus]\nwhatever = 1\n";
        let c = Config::from_ini(ini);
        assert_eq!(c.mouse_sensitivity, 4.0, "sensitivity clamps to max");
        let mut d = Config::default();
        d.set_sensitivity(4.0);
        assert_eq!(c, d, "unknown lines are ignored, the rest stays default");
    }

    #[test]
    fn set_key_unbinds_from_other_rows() {
        let mut c = Config::default();
        // Bind FORWARD to the E key: ENTER/EXIT must give it up.
        c.set_key(Binding::Forward, "e".into());
        assert_eq!(c.forward, "e");
        assert_eq!(c.enter_exit, "", "E was stolen from ENTER/EXIT");
        // Bind it back.
        c.set_key(Binding::EnterExit, "e".into());
        assert_eq!(c.enter_exit, "e");
        assert_eq!(c.forward, "");
        // Normalization.
        c.set_key(Binding::Forward, "SPACE".into());
        assert_eq!(c.forward, " ");
    }

    #[test]
    fn mouse_button_cycle_and_names() {
        let mut c = Config::default();
        assert_eq!(c.brake_button, MouseButton::RMB);
        c.cycle_mouse_button(Binding::BrakeButton, -1);
        assert_eq!(c.brake_button, MouseButton::MMB);
        c.cycle_mouse_button(Binding::BrakeButton, -1);
        assert_eq!(c.brake_button, MouseButton::LMB);
        assert_eq!(MouseButton::from_name("LMB").unwrap(), MouseButton::LMB);
        assert_eq!(MouseButton::from_name("middle").unwrap(), MouseButton::MMB);
        assert_eq!(MouseButton::from_name("nope"), None);
    }

    #[test]
    fn key_display_and_normalize() {
        assert_eq!(key_display(" "), "SPACE");
        assert_eq!(key_display("escape"), "ESC");
        assert_eq!(key_display("f1"), "F1");
        assert_eq!(key_display("arrowup"), "ARROW UP");
        assert_eq!(normalize_key("Esc"), "escape");
        assert_eq!(normalize_key("Space"), " ");
        assert_eq!(normalize_key(" W "), "w");
    }

    #[test]
    fn all_keys_collects_bound_keys() {
        let c = Config::default();
        let ks = c.all_keys();
        assert!(ks.contains("w") && ks.contains("e") && ks.contains("escape"));
        assert!(!ks.contains("q"));
    }
}
