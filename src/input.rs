//! Keyboard state: hold-to-press keys plus per-frame edge detection.
//! Pure (no DOM), so it is unit-testable on the host.

use std::collections::HashSet;

use crate::config::{Config, MouseButton};

/// Mouse button state + drag deltas accumulated since the last frame.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Mouse {
    /// Left (primary) button held.
    pub down: bool,
    /// Middle button held.
    pub middle: bool,
    /// Right button held.
    pub right: bool,
    pub dx: f64,
    pub dy: f64,
    /// Accumulated wheel notches since the last frame (1 = one notch).
    pub wheel: f64,
}

#[derive(Default)]
pub struct Input {
    pressed: HashSet<String>,
    just_pressed: HashSet<String>,
    mouse: Mouse,
}

impl Input {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keys whose browser default behavior should be suppressed: everything
    /// that can be bound in the config plus the always-active second set of
    /// movement keys (arrows / space) and function keys.
    pub fn prevent_keys(cfg: &Config) -> HashSet<String> {
        let mut s = cfg.all_keys();
        s.insert("arrowup".to_string());
        s.insert("arrowdown".to_string());
        s.insert("arrowleft".to_string());
        s.insert("arrowright".to_string());
        s.insert(" ".to_string());
        s.insert("f1".to_string());
        s.insert("escape".to_string());
        s
    }

    pub fn key_down(&mut self, key: &str) {
        let k = key.to_ascii_lowercase();
        if self.pressed.insert(k.clone()) {
            self.just_pressed.insert(k);
        }
    }

    pub fn key_up(&mut self, key: &str) {
        self.pressed.remove(&key.to_ascii_lowercase());
    }

    pub fn is_down(&self, key: &str) -> bool {
        self.pressed.contains(&key.to_ascii_lowercase())
    }

    /// Was the key pressed since the last frame?
    pub fn just_pressed(&self, key: &str) -> bool {
        self.just_pressed.contains(&key.to_ascii_lowercase())
    }

    /// All keys pressed since the last frame (normalized), for the config
    /// page's "press any key to bind" logic.
    pub fn just_pressed_keys(&self) -> Vec<String> {
        self.just_pressed.iter().cloned().collect()
    }

    /// Called at the end of every update tick.
    pub fn end_frame(&mut self) {
        self.just_pressed.clear();
        self.mouse.dx = 0.0;
        self.mouse.dy = 0.0;
        self.mouse.wheel = 0.0;
    }

    /// Primary mouse button pressed.
    pub fn mouse_down(&mut self) {
        self.mouse.down = true;
    }

    /// Primary mouse button released.
    pub fn mouse_up(&mut self) {
        self.mouse.down = false;
    }

    /// Right mouse button pressed.
    pub fn mouse_right_down(&mut self) {
        self.mouse.right = true;
    }

    /// Right mouse button released.
    pub fn mouse_right_up(&mut self) {
        self.mouse.right = false;
    }

    /// Middle mouse button pressed.
    pub fn mouse_middle_down(&mut self) {
        self.mouse.middle = true;
    }

    /// Middle mouse button released.
    pub fn mouse_middle_up(&mut self) {
        self.mouse.middle = false;
    }

    /// Is the right mouse button currently down?
    pub fn mouse_right_state(&self) -> bool {
        self.mouse.right
    }

    /// Is a specific mouse button currently down (left/middle/right)?
    pub fn button_down(&self, b: MouseButton) -> bool {
        match b {
            MouseButton::LMB => self.mouse.down,
            MouseButton::MMB => self.mouse.middle,
            MouseButton::RMB => self.mouse.right,
        }
    }

    /// Is any mouse button currently down?
    pub fn any_button_down(&self) -> bool {
        self.mouse.down || self.mouse.middle || self.mouse.right
    }

    /// Mouse wheel turned. `notches` is already normalized (1 = one notch).
    pub fn mouse_wheel(&mut self, notches: f64) {
        self.mouse.wheel += notches;
    }

    /// Accumulated wheel notches since the last `end_frame`.
    pub fn wheel_delta(&self) -> f64 {
        self.mouse.wheel
    }

    /// Mouse moved by `(dx, dy)` px; only counts while a button is down.
    pub fn mouse_move(&mut self, dx: f64, dy: f64) {
        if self.any_button_down() {
            self.mouse.dx += dx;
            self.mouse.dy += dy;
        }
    }

    /// Accumulated drag delta since the last `end_frame` (0 if not dragging).
    pub fn mouse_delta(&self) -> (f64, f64) {
        (self.mouse.dx, self.mouse.dy)
    }

    /// Is the primary mouse button currently down?
    pub fn mouse_down_state(&self) -> bool {
        self.mouse.down
    }

    /// Car controls from the key set (bindings from the player config; the
    /// arrow keys always work as a second set).
    pub fn car_controls(&self, cfg: &Config) -> crate::car::CarInput {
        let mut ci = crate::car::CarInput::default();
        if self.is_down(&cfg.forward) || self.is_down("arrowup") {
            ci.throttle = 1.0;
        }
        if self.is_down(&cfg.back) || self.is_down("arrowdown") {
            ci.throttle = -1.0;
        }
        if self.is_down(&cfg.steer_left) || self.is_down("arrowleft") {
            ci.steer = -1.0;
        }
        if self.is_down(&cfg.steer_right) || self.is_down("arrowright") {
            ci.steer = 1.0;
        }
        ci.handbrake = self.is_down(&cfg.handbrake_dive);
        ci.boost = self.is_down(&cfg.boost_climb);
        // Aircraft pitch stick: climb key = up, dive key = down (cars ignore it).
        ci.pitch = if self.is_down(&cfg.boost_climb) {
            1.0
        } else if self.is_down(&cfg.handbrake_dive) {
            -1.0
        } else {
            0.0
        };
        ci
    }

    /// On-foot movement direction (unit-ish vector), climb key = run.
    pub fn foot_controls(&self, cfg: &Config) -> (f64, f64, bool) {
        let mut dx = 0.0;
        let mut dy = 0.0;
        if self.is_down(&cfg.steer_left) || self.is_down("arrowleft") {
            dx -= 1.0;
        }
        if self.is_down(&cfg.steer_right) || self.is_down("arrowright") {
            dx += 1.0;
        }
        if self.is_down(&cfg.forward) || self.is_down("arrowup") {
            dy -= 1.0;
        }
        if self.is_down(&cfg.back) || self.is_down("arrowdown") {
            dy += 1.0;
        }
        (dx, dy, self.is_down(&cfg.boost_climb))
    }

    /// True if any movement key (config keys / arrows / shift / space) is
    /// held — i.e. the player is actively driving the vehicle they are in.
    pub fn vehicle_input(&self, cfg: &Config) -> bool {
        self.is_down(&cfg.forward)
            || self.is_down(&cfg.back)
            || self.is_down(&cfg.steer_left)
            || self.is_down(&cfg.steer_right)
            || self.is_down(&cfg.boost_climb)
            || self.is_down(&cfg.handbrake_dive)
            || self.is_down("arrowup")
            || self.is_down("arrowdown")
            || self.is_down("arrowleft")
            || self.is_down("arrowright")
            || self.is_down("shift")
            || self.is_down(" ")
    }

    /// True if the mouse is being used in any way (a button held, a drag in
    /// flight this frame, or the wheel turned).
    pub fn mouse_in_use(&self) -> bool {
        self.mouse.down
            || self.mouse.right
            || self.mouse.dx != 0.0
            || self.mouse.dy != 0.0
            || self.mouse.wheel != 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn arrow_keys_map_to_car_controls() {
        let cfg = Config::default();
        let mut inp = Input::new();
        // Up = throttle.
        inp.key_down("arrowup");
        let ci = inp.car_controls(&cfg);
        assert_eq!(ci.throttle, 1.0);
        assert_eq!(ci.steer, 0.0);
        inp.key_up("arrowup");
        // Down = brake/reverse.
        inp.key_down("arrowdown");
        let ci = inp.car_controls(&cfg);
        assert_eq!(ci.throttle, -1.0);
        inp.key_up("arrowdown");
        // Left = steer left.
        inp.key_down("arrowleft");
        let ci = inp.car_controls(&cfg);
        assert_eq!(ci.steer, -1.0);
        assert_eq!(ci.throttle, 0.0);
        inp.key_up("arrowleft");
        // Right = steer right.
        inp.key_down("arrowright");
        let ci = inp.car_controls(&cfg);
        assert_eq!(ci.steer, 1.0);
        inp.key_up("arrowright");
    }

    #[test]
    fn rebinding_movement_keys_works() {
        let mut cfg = Config::default();
        cfg.set_key(crate::config::Binding::Forward, "k".into());
        let mut inp = Input::new();
        inp.key_down("w"); // unbound now
        let ci = inp.car_controls(&cfg);
        assert_eq!(ci.throttle, 0.0, "W is no longer forward");
        inp.key_up("w");
        inp.key_down("k");
        let ci = inp.car_controls(&cfg);
        assert_eq!(ci.throttle, 1.0, "K is forward now");
    }

    #[test]
    fn press_and_release_tracking() {
        let mut inp = Input::new();
        inp.key_down("W"); // case-insensitive
        assert!(inp.just_pressed("w"));
        assert!(inp.is_down("w"));
        inp.end_frame();
        assert!(!inp.just_pressed("w"));
        assert!(inp.is_down("w"));
        inp.key_up("w");
        assert!(!inp.is_down("w"));
    }

    #[test]
    fn repeated_keydown_does_not_retrigger_edge() {
        let mut inp = Input::new();
        inp.key_down("e");
        assert!(inp.just_pressed("e"));
        inp.end_frame();
        inp.key_down("e"); // auto-repeat while held
        assert!(!inp.just_pressed("e"));
    }

    #[test]
    fn car_controls_map_keys() {
        let mut inp = Input::new();
        let cfg = Config::default();
        inp.key_down("w");
        inp.key_down("d");
        inp.key_down(" ");
        let ci = inp.car_controls(&cfg);
        assert_eq!(ci.throttle, 1.0);
        assert_eq!(ci.steer, 1.0);
        assert!(ci.handbrake);
        assert!(!ci.boost);

        let mut inp2 = Input::new();
        inp2.key_down("arrowup");
        inp2.key_down("arrowleft");
        let ci2 = inp2.car_controls(&cfg);
        assert_eq!(ci2.throttle, 1.0);
        assert_eq!(ci2.steer, -1.0);
    }

    #[test]
    fn right_button_and_wheel_are_tracked() {
        let mut inp = Input::new();
        inp.mouse_right_down();
        assert!(inp.mouse_right_state());
        // Dragging counts while the right button is down too.
        inp.mouse_move(5.0, 3.0);
        assert_eq!(inp.mouse_delta(), (5.0, 3.0));
        inp.end_frame();
        inp.mouse_right_up();
        assert!(!inp.mouse_right_state());
        inp.mouse_move(5.0, 3.0); // no button down -> ignored
        assert_eq!(inp.mouse_delta(), (0.0, 0.0));
        inp.mouse_wheel(1.0);
        inp.mouse_wheel(-1.0);
        inp.mouse_wheel(1.0);
        assert_eq!(inp.wheel_delta(), 1.0);
        inp.end_frame();
        assert_eq!(inp.wheel_delta(), 0.0);
    }

    #[test]
    fn mouse_drag_accumulates_and_resets_each_frame() {
        let mut inp = Input::new();
        inp.mouse_move(10.0, 5.0); // not dragging → ignored
        assert_eq!(inp.mouse_delta(), (0.0, 0.0));
        inp.mouse_down();
        inp.mouse_move(10.0, 5.0);
        inp.mouse_move(2.0, -1.0);
        assert_eq!(inp.mouse_delta(), (12.0, 4.0));
        inp.end_frame();
        assert_eq!(inp.mouse_delta(), (0.0, 0.0));
        assert!(inp.mouse_down_state());
        inp.mouse_up();
        inp.mouse_move(10.0, 10.0); // released → ignored again
        assert_eq!(inp.mouse_delta(), (0.0, 0.0));
        assert!(!inp.mouse_down_state());
    }

    #[test]
    fn foot_controls_diagonal_is_normalized_enough() {
        let cfg = Config::default();
        let mut inp = Input::new();
        inp.key_down("w");
        inp.key_down("d");
        let (dx, dy, run) = inp.foot_controls(&cfg);
        assert_eq!((dx, dy), (1.0, -1.0));
        assert!(!run);
    }

    #[test]
    fn all_three_mouse_buttons_are_tracked() {
        let mut inp = Input::new();
        inp.mouse_down();
        inp.mouse_middle_down();
        inp.mouse_right_down();
        assert!(inp.button_down(MouseButton::LMB));
        assert!(inp.button_down(MouseButton::MMB));
        assert!(inp.button_down(MouseButton::RMB));
        assert!(inp.any_button_down());
        inp.mouse_move(3.0, 2.0); // middle-button drag counts too
        assert_eq!(inp.mouse_delta(), (3.0, 2.0));
        inp.end_frame();
        inp.mouse_up();
        inp.mouse_middle_up();
        inp.mouse_right_up();
        assert!(!inp.any_button_down());
        inp.mouse_move(3.0, 2.0);
        assert_eq!(inp.mouse_delta(), (0.0, 0.0));
    }
}
