//! The wanted system: heat (0..=5) drives star count and police count.
//! Police cars pursue the player with seek-steering; heat decays when the
//! player hides away from them.

use crate::Rng;
use crate::car::{Car, CarKind, drive_to};
use crate::city::City;

/// Add heat, clamped to [0, 5].
pub fn add_heat(heat: f64, amount: f64) -> f64 {
    (heat + amount).clamp(0.0, 5.0)
}

/// Stars = ceil of heat (0 for no heat).
pub fn stars(heat: f64) -> u32 {
    if heat <= 0.0 {
        0
    } else {
        heat.ceil() as u32
    }
}

/// Heat decay: slow if police can see you, fast if you've shaken them.
pub fn decay_heat(heat: f64, dt: f64, time_since_crime: f64, nearest_police_dist: f64) -> f64 {
    if heat <= 0.0 || time_since_crime < 5.0 {
        return heat;
    }
    let rate = if nearest_police_dist > 550.0 { 0.55 } else { 0.12 };
    (heat - rate * dt).max(0.0)
}

/// Spawn `n` police cars on roads 600–1100 px from the player.
pub fn spawn_police(rng: &mut Rng, city: &City, px: f64, py: f64, n: usize) -> Vec<Car> {
    let mut out = Vec::new();
    for _ in 0..n {
        let (x, y) = city.random_road_point(rng, px, py, 600.0);
        let heading = (py - y).atan2(px - x);
        let mut c = Car::new(x, y, heading, CarKind::Police);
        // Police are a bit faster than the player's cruise speed.
        c.vx = heading.cos() * 120.0;
        c.vy = heading.sin() * 120.0;
        out.push(c);
    }
    out
}

/// Step every police car toward the player. Returns true if any police car
/// is within `catch_dist` of the player (used for the bust check).
pub fn update_police(
    police: &mut [Car],
    city: &City,
    px: f64,
    py: f64,
    stars: u32,
    dt: f64,
    catch_dist: f64,
) -> bool {
    let mut caught = false;
    for p in police.iter_mut() {
        let max_speed = 300.0 + 35.0 * stars as f64;
        drive_to(p, px, py, max_speed, 2.4, dt);
        crate::car::collide_car_with_city(p, city);
        let d = ((p.x - px).powi(2) + (p.y - py).powi(2)).sqrt();
        if d < catch_dist {
            caught = true;
        }
    }
    caught
}

/// Distance from (px, py) to the nearest police car (f64::INFINITY if none).
pub fn nearest_police(police: &[Car], px: f64, py: f64) -> f64 {
    police
        .iter()
        .map(|p| ((p.x - px).powi(2) + (p.y - py).powi(2)).sqrt())
        .fold(f64::INFINITY, f64::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heat_clamps_and_stars_ceil() {
        assert_eq!(add_heat(0.0, 0.0), 0.0);
        assert_eq!(add_heat(0.0, 0.5), 0.5);
        assert_eq!(add_heat(4.9, 1.0), 5.0);
        assert_eq!(stars(0.0), 0);
        assert_eq!(stars(0.2), 1);
        assert_eq!(stars(1.0), 1);
        assert_eq!(stars(1.01), 2);
        assert_eq!(stars(5.0), 5);
    }

    #[test]
    fn heat_does_not_decay_soon_after_crime() {
        let h = decay_heat(3.0, 1.0, 1.0, 0.0);
        assert_eq!(h, 3.0);
    }

    #[test]
    fn heat_decays_faster_when_police_are_far() {
        let near = decay_heat(3.0, 1.0, 10.0, 100.0);
        let far = decay_heat(3.0, 1.0, 10.0, 2000.0);
        assert!(far < near);
    }

    #[test]
    fn heat_never_goes_negative() {
        assert_eq!(decay_heat(0.1, 100.0, 10.0, 9999.0), 0.0);
    }

    #[test]
    fn nearest_police_distance() {
        let a = Car::new(0.0, 0.0, 0.0, CarKind::Police);
        let b = Car::new(100.0, 0.0, 0.0, CarKind::Police);
        assert_eq!(nearest_police(&[a, b], 0.0, 0.0), 0.0);
        assert_eq!(nearest_police(&[], 5.0, 5.0), f64::INFINITY);
    }
}
