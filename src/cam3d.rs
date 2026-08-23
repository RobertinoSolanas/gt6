//! 3D camera & perspective projection math (pure, unit-testable).
//!
//! World frame: X/Y are the ground plane (same axes as the 2D top-down
//! view), Z is up. The camera looks horizontally along `yaw`; the chase cam
//! is simply elevated, so a ground-level target appears below screen center
//! (classic third-person view).

use std::f64::consts::PI;

/// Near-plane distance for projection/clipping (world units).
pub const NEAR: f64 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl V3 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        V3 { x, y, z }
    }
    pub const fn sub(self, o: V3) -> V3 {
        V3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
    pub const fn add(self, o: V3) -> V3 {
        V3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
    pub const fn mul(self, k: f64) -> V3 {
        V3::new(self.x * k, self.y * k, self.z * k)
    }
    pub const fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub const fn cross(self, o: V3) -> V3 {
        V3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn len(self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
    pub fn normalized(self) -> V3 {
        let l = self.len();
        if l > 1e-12 {
            self.mul(1.0 / l)
        } else {
            V3::new(0.0, 0.0, 0.0)
        }
    }
}

/// A pinhole camera at yaw `yaw` (ground-plane heading) and `pitch`
/// (radians, positive = looking up; 0 = horizontal).
#[derive(Clone, Copy, Debug)]
pub struct Cam3D {
    pub pos: V3,
    pub yaw: f64,
    pub pitch: f64,
    /// Vertical field of view in radians.
    pub fov: f64,
    /// Viewport size in px.
    pub w: f64,
    pub h: f64,
}

impl Cam3D {
    pub fn new(pos: V3, yaw: f64, w: f64, h: f64) -> Self {
        Cam3D { pos, yaw, pitch: 0.0, fov: 66.0 * PI / 180.0, w, h }
    }

    /// Set the pitch (radians, positive = looking up).
    pub fn with_pitch(mut self, pitch: f64) -> Self {
        self.pitch = pitch;
        self
    }

    /// Screen y of the horizon for this pitch (px from the top). Looking
    /// down (negative pitch) raises it toward the top of the frame.
    pub fn horizon(&self) -> f64 {
        self.h / 2.0 + self.focal() * self.pitch.tan()
    }

    /// Focal length in px for the vertical fov.
    pub fn focal(&self) -> f64 {
        (self.h / 2.0) / (self.fov / 2.0).tan()
    }

    /// World → camera space: x right, y up, z depth (+z = away from cam).
    pub fn to_cam(&self, p: V3) -> V3 {
        let f = self.forward();
        let r = self.right();
        let d = p.sub(self.pos);
        let (y0, z0) = (d.z, d.dot(f));
        let cp = self.pitch.cos();
        let sp = self.pitch.sin();
        V3::new(d.dot(r), y0 * cp - z0 * sp, z0 * cp + y0 * sp)
    }

    /// Project a world point to screen px. `None` if behind the near plane.
    /// Returns `(sx, sy, depth)`.
    pub fn project(&self, p: V3) -> Option<(f64, f64, f64)> {
        let c = self.to_cam(p);
        if c.z < NEAR {
            return None;
        }
        let f = self.focal();
        Some((self.w / 2.0 + c.x * f / c.z, self.h / 2.0 - c.y * f / c.z, c.z))
    }

    /// On-screen scale at `depth` (px per world unit).
    pub fn scale_at(&self, depth: f64) -> f64 {
        self.focal() / depth
    }

    /// Horizontal (ground-projected) forward direction.
    pub fn forward(&self) -> V3 {
        V3::new(self.yaw.cos(), self.yaw.sin(), 0.0)
    }
    /// Right unit vector (`forward × up`).
    pub fn right(&self) -> V3 {
        let f = self.forward();
        V3::new(f.y, -f.x, 0.0)
    }
}

/// Shortest-path angle interpolation from `a` to `b` by fraction `t` in [0,1].
pub fn lerp_angle(a: f64, b: f64, t: f64) -> f64 {
    let mut d = (b - a) % (2.0 * PI);
    if d > PI {
        d -= 2.0 * PI;
    }
    if d < -PI {
        d += 2.0 * PI;
    }
    a + d * t
}

/// Clip a camera-space polygon against the near plane `z >= NEAR`
/// (Sutherland–Hodgman against a single plane).
pub fn clip_near(verts: &[V3]) -> Vec<V3> {
    let mut out = Vec::new();
    let n = verts.len();
    if n < 2 {
        return out;
    }
    let mut prev = verts[0];
    for i in 0..n {
        let v = verts[i];
        let p_in = prev.z >= NEAR;
        let v_in = v.z >= NEAR;
        if v_in {
            if !p_in {
                let t = (NEAR - prev.z) / (v.z - prev.z);
                out.push(prev.add(v.sub(prev).mul(t)).with_z(NEAR));
            }
            out.push(v);
        } else if p_in {
            let t = (NEAR - prev.z) / (v.z - prev.z);
            out.push(prev.add(v.sub(prev).mul(t)).with_z(NEAR));
        }
        prev = v;
    }
    out
}

impl V3 {
    const fn with_z(self, z: f64) -> V3 {
        V3::new(self.x, self.y, z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> Cam3D {
        // At origin, facing +x, 800x600 viewport.
        Cam3D::new(V3::new(0.0, 0.0, 0.0), 0.0, 800.0, 600.0)
    }

    #[test]
    fn point_ahead_projects_to_center() {
        let c = cam();
        let (sx, sy, depth) = c.project(V3::new(100.0, 0.0, 0.0)).unwrap();
        assert!((sx - 400.0).abs() < 1e-6);
        assert!((sy - 300.0).abs() < 1e-6);
        assert!((depth - 100.0).abs() < 1e-6);
    }

    #[test]
    fn point_to_the_right_projects_right() {
        // Facing +x, right vector is -y.
        let c = cam();
        let (sx, _, _) = c.project(V3::new(100.0, -10.0, 0.0)).unwrap();
        assert!(sx > 400.0);
        let (sx2, _, _) = c.project(V3::new(100.0, 10.0, 0.0)).unwrap();
        assert!(sx2 < 400.0);
    }

    #[test]
    fn point_above_projects_up() {
        let c = cam();
        let (_, sy, _) = c.project(V3::new(100.0, 0.0, 10.0)).unwrap();
        assert!(sy < 300.0);
    }

    #[test]
    fn point_behind_camera_is_rejected() {
        let c = cam();
        assert!(c.project(V3::new(-10.0, 0.0, 0.0)).is_none());
        // Barely behind the near plane.
        assert!(c.project(V3::new(1.0, 0.0, 0.0)).is_none());
        // Just in front of it.
        assert!(c.project(V3::new(NEAR + 0.1, 0.0, 0.0)).is_some());
    }

    #[test]
    fn perspective_halves_offset_with_double_distance() {
        let c = cam();
        let (s1, _, _) = c.project(V3::new(100.0, -10.0, 0.0)).unwrap();
        let (s2, _, _) = c.project(V3::new(200.0, -10.0, 0.0)).unwrap();
        let o1 = s1 - 400.0;
        let o2 = s2 - 400.0;
        assert!((2.0 * o2 - o1).abs() < 1e-6, "{o1} vs {o2}");
    }

    #[test]
    fn pitch_up_moves_the_horizon_down() {
        // Camera 10 units up, 1 km ahead on the ground ≈ horizon.
        let flat = Cam3D::new(V3::new(0.0, 0.0, 10.0), 0.0, 800.0, 600.0);
        let (_, sy0, _) = flat.project(V3::new(10000.0, 0.0, 0.0)).unwrap();
        let up = flat.with_pitch(0.2);
        let (_, sy_up, _) = up.project(V3::new(10000.0, 0.0, 0.0)).unwrap();
        let down = flat.with_pitch(-0.2);
        let (_, sy_down, _) = down.project(V3::new(10000.0, 0.0, 0.0)).unwrap();
        assert!(sy_up > sy0, "looking up: horizon drops on screen");
        assert!(sy_down < sy0, "looking down: horizon rises on screen");
    }

    #[test]
    fn pitch_up_sees_zenith_at_center() {
        // Camera pitched fully up (π/2) looks straight at the point above it.
        let c = Cam3D::new(V3::new(0.0, 0.0, 0.0), 0.0, 800.0, 600.0).with_pitch(PI / 2.0);
        let (sx, sy, _) = c.project(V3::new(0.0, 0.0, 100.0)).unwrap();
        assert!((sx - 400.0).abs() < 1e-6);
        assert!((sy - 300.0).abs() < 1e-6);
    }

    #[test]
    fn horizon_matches_projection() {
        let c = Cam3D::new(V3::new(0.0, 0.0, 50.0), 0.0, 800.0, 600.0).with_pitch(-0.3);
        // Very far away the projection approaches the analytic horizon.
        let (_, sy, _) = c.project(V3::new(100000.0, 0.0, 0.0)).unwrap();
        assert!((sy - c.horizon()).abs() < 0.5, "sy={} h={}", sy, c.horizon());
    }

    #[test]
    fn yaw_rotates_the_view() {
        // A camera at the origin facing +y should see (0,100) centered.
        let c = Cam3D::new(V3::new(0.0, 0.0, 0.0), PI / 2.0, 800.0, 600.0);
        let (sx, sy, _) = c.project(V3::new(0.0, 100.0, 0.0)).unwrap();
        assert!((sx - 400.0).abs() < 1e-6);
        assert!((sy - 300.0).abs() < 1e-6);
    }

    #[test]
    fn lerp_angle_takes_the_short_way() {
        // π → -π is a zero step.
        assert!((lerp_angle(PI, -PI, 0.5) - PI).abs() < 1e-9);
        // 0 → π/4 halfway is π/8.
        assert!((lerp_angle(0.0, PI / 4.0, 0.5) - PI / 8.0).abs() < 1e-9);
        // 0 → -π/4 halfway is -π/8.
        assert!((lerp_angle(0.0, -PI / 4.0, 0.5) + PI / 8.0).abs() < 1e-9);
        // t=1 lands exactly on the target.
        assert!((lerp_angle(0.0, -PI / 2.0, 1.0) + PI / 2.0).abs() < 1e-9);
    }

    #[test]
    fn clip_near_keeps_and_clips_a_straddling_quad() {
        // Quad in cam space from z=-10 to z=100 at x=0.
        let q = [
            V3::new(0.0, 0.0, -10.0),
            V3::new(50.0, 0.0, -10.0),
            V3::new(50.0, 0.0, 100.0),
            V3::new(0.0, 0.0, 100.0),
        ];
        let c = clip_near(&q);
        assert!(c.len() >= 3, "got {}", c.len());
        for v in &c {
            assert!(v.z >= NEAR - 1e-9, "z={} < NEAR", v.z);
        }
    }

    #[test]
    fn clip_near_rejects_fully_behind_and_keeps_fully_front() {
        let behind = [
            V3::new(0.0, 0.0, -10.0),
            V3::new(5.0, 0.0, -20.0),
            V3::new(0.0, 0.0, -30.0),
        ];
        assert!(clip_near(&behind).is_empty());
        let front = [
            V3::new(0.0, 0.0, 10.0),
            V3::new(5.0, 0.0, 20.0),
            V3::new(0.0, 0.0, 30.0),
        ];
        assert_eq!(clip_near(&front).len(), 3);
    }
}
