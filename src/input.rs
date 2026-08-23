//! Keyboard state: hold-to-press keys plus per-frame edge detection.
//! Pure (no DOM), so it is unit-testable on the host.

use std::collections::HashSet;

/// Mouse button state + drag deltas accumulated since the last frame.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Mouse {
    pub down: bool,
    pub dx: f64,
    pub dy: f64,
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

    /// Keys we care about (normalized to lowercase).
    pub const KEYS: [&str; 15] = [
        "w", "a", "s", "d",
        "arrowup", "arrowdown", "arrowleft", "arrowright",
        " ", "shift", "e", "r", "p", "v", "c",
    ];

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

    /// Called at the end of every update tick.
    pub fn end_frame(&mut self) {
        self.just_pressed.clear();
        self.mouse.dx = 0.0;
        self.mouse.dy = 0.0;
    }

    /// Primary mouse button pressed.
    pub fn mouse_down(&mut self) {
        self.mouse.down = true;
    }

    /// Primary mouse button released.
    pub fn mouse_up(&mut self) {
        self.mouse.down = false;
    }

    /// Mouse moved by `(dx, dy)` px; only counts while the button is down.
    pub fn mouse_move(&mut self, dx: f64, dy: f64) {
        if self.mouse.down {
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

    /// Car controls from the key set.
    pub fn car_controls(&self) -> crate::car::CarInput {
        let mut ci = crate::car::CarInput::default();
        if self.is_down("w") || self.is_down("arrowup") {
            ci.throttle = 1.0;
        }
        if self.is_down("s") || self.is_down("arrowdown") {
            ci.throttle = -1.0;
        }
        if self.is_down("a") || self.is_down("arrowleft") {
            ci.steer = -1.0;
        }
        if self.is_down("d") || self.is_down("arrowright") {
            ci.steer = 1.0;
        }
        ci.handbrake = self.is_down(" ");
        ci.boost = self.is_down("shift");
        ci
    }

    /// On-foot movement direction (unit-ish vector), Shift = run.
    pub fn foot_controls(&self) -> (f64, f64, bool) {
        let mut dx = 0.0;
        let mut dy = 0.0;
        if self.is_down("a") || self.is_down("arrowleft") {
            dx -= 1.0;
        }
        if self.is_down("d") || self.is_down("arrowright") {
            dx += 1.0;
        }
        if self.is_down("w") || self.is_down("arrowup") {
            dy -= 1.0;
        }
        if self.is_down("s") || self.is_down("arrowdown") {
            dy += 1.0;
        }
        (dx, dy, self.is_down("shift"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrow_keys_map_to_car_controls() {
        let mut inp = Input::new();
        // Up = throttle.
        inp.key_down("arrowup");
        let ci = inp.car_controls();
        assert_eq!(ci.throttle, 1.0);
        assert_eq!(ci.steer, 0.0);
        inp.key_up("arrowup");
        // Down = brake/reverse.
        inp.key_down("arrowdown");
        let ci = inp.car_controls();
        assert_eq!(ci.throttle, -1.0);
        inp.key_up("arrowdown");
        // Left = steer left.
        inp.key_down("arrowleft");
        let ci = inp.car_controls();
        assert_eq!(ci.steer, -1.0);
        assert_eq!(ci.throttle, 0.0);
        inp.key_up("arrowleft");
        // Right = steer right.
        inp.key_down("arrowright");
        let ci = inp.car_controls();
        assert_eq!(ci.steer, 1.0);
        inp.key_up("arrowright");
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
        inp.key_down("w");
        inp.key_down("d");
        inp.key_down(" ");
        let ci = inp.car_controls();
        assert_eq!(ci.throttle, 1.0);
        assert_eq!(ci.steer, 1.0);
        assert!(ci.handbrake);
        assert!(!ci.boost);

        let mut inp2 = Input::new();
        inp2.key_down("arrowup");
        inp2.key_down("arrowleft");
        let ci2 = inp2.car_controls();
        assert_eq!(ci2.throttle, 1.0);
        assert_eq!(ci2.steer, -1.0);
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
        let mut inp = Input::new();
        inp.key_down("w");
        inp.key_down("d");
        let (dx, dy, run) = inp.foot_controls();
        assert_eq!((dx, dy), (1.0, -1.0));
        assert!(!run);
    }
}
