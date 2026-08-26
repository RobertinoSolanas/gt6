//! 3D mode: a software perspective renderer drawn on the same 2D canvas.
//!
//! Chase camera behind the player (yaw smoothed in `GameState`), the city
//! extruded into boxes, painter's-algorithm depth sorting, Sutherland–
//! Hodgman near-plane clipping, and a cheap fixed-sun face shading.

#![allow(unused_must_use)]
#![allow(deprecated)] // web-sys gradient setters

use wasm_bindgen::JsValue;
use web_sys::CanvasRenderingContext2d;

use crate::cam3d::{clip_near, Cam3D, NEAR, V3 as V};
use crate::city::{BLOCK, CELL, N, ROAD, SIZE};
use crate::render::{draw_hud, draw_overlays};
use crate::state::GameState;

/// Fixed "sun" direction for face shading (normalized at use site).
const SUN: V = V::new(0.45, -0.55, 0.70);

type Pts = Vec<(f64, f64)>;

/// One projected triangle of the dragon mesh: `(depth, color key, 3 screen
/// points)`. Faces sharing a color key are drawn as one batched path.
struct DragonFace {
    depth: f64,
    key: u32,
    pts: [f64; 6],
}

/// Renderer state for one frame (css-px coordinates).
struct R3<'a> {
    ctx: &'a CanvasRenderingContext2d,
    cam: Cam3D,
    /// The dragon's baked GLB mesh, if the async load finished.
    dmesh: Option<&'a crate::wildlife::DragonMesh>,
    /// Scratch: transformed dragon vertex positions (x, y, z triples).
    vtx: Vec<f64>,
    /// Scratch: projected dragon faces for this frame.
    faces: Vec<DragonFace>,
}

impl R3<'_> {
    /// Project a world-space polygon to screen, clipped against the near
    /// plane. Returns `(points, mean_depth)` or `None` if fully culled.
    fn screen(&self, verts: &[V]) -> Option<(Pts, f64)> {
        let cv: Vec<V> = verts.iter().map(|v| self.cam.to_cam(*v)).collect();
        if cv.iter().all(|c| c.z < NEAR) {
            return None;
        }
        let cc = clip_near(&cv);
        if cc.len() < 3 {
            return None;
        }
        let f = self.cam.focal();
        let (w, h) = (self.cam.w, self.cam.h);
        let pts = cc
            .iter()
            .map(|c| (w / 2.0 + c.x * f / c.z, h / 2.0 - c.y * f / c.z))
            .collect();
        let depth = cc.iter().map(|c| c.z).sum::<f64>() / cc.len() as f64;
        Some((pts, depth))
    }

    fn fill_poly(&self, verts: &[V], fill: &str) {
        if let Some((pts, _)) = self.screen(verts) {
            self.trace(&pts);
            self.ctx.set_fill_style_str(fill);
            self.ctx.fill();
        }
    }

    /// Fills a world polygon with a vertical screen-space sheen gradient and
    /// distance fog: the polished default for solid surfaces.
    fn fill_poly_grad(&self, verts: &[V], color: u32) {
        if let Some((pts, depth)) = self.screen(verts) {
            let base = fog_mix_c(color, depth);
            let mut y0 = f64::MAX;
            let mut y1 = f64::MIN;
            for &(_, y) in &pts {
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
            if y1 - y0 < 1.5 {
                self.trace(&pts);
                self.ctx.set_fill_style_str(&rgb_u(base));
                self.ctx.fill();
            } else {
                let g = self.ctx.create_linear_gradient(0.0, y0, 0.0, y1);
                let top = rgb_u(brighten(base, 1.06));
                let bot = rgb_u(brighten(base, 0.90));
                let _ = g.add_color_stop(0.0, &top);
                let _ = g.add_color_stop(1.0, &bot);
                self.trace(&pts);
                self.ctx.set_fill_style(&JsValue::from(g));
                self.ctx.fill();
            }
        }
    }

    fn trace(&self, pts: &Pts) {
        self.ctx.begin_path();
        let (x0, y0) = pts[0];
        self.ctx.move_to(x0, y0);
        for &(x, y) in &pts[1..] {
            self.ctx.line_to(x, y);
        }
        self.ctx.close_path();
    }

    /// A line in world space with a width that scales with depth.
    fn line3d(&self, a: V, b: V, base_w: f64, color: &str) {
        let pa = self.cam.project(a);
        let pb = self.cam.project(b);
        if let (Some((ax, ay, _)), Some((bx, by, z))) = (pa, pb) {
            self.ctx.begin_path();
            self.ctx.move_to(ax, ay);
            self.ctx.line_to(bx, by);
            self.ctx.set_stroke_style_str(color);
            self.ctx.set_line_width((base_w * self.cam.scale_at(z)).clamp(1.0, 80.0));
            self.ctx.stroke();
        }
    }
}

/// 0xRRGGBB scaled toward black/white by `k` (k=1 is unchanged), as a u32.
fn brighten(c: u32, k: f64) -> u32 {
    let k = k.clamp(0.0, 1.6);
    let r = (((c >> 16) & 0xff) as f64 * k).round().clamp(0.0, 255.0) as u8;
    let g = (((c >> 8) & 0xff) as f64 * k).round().clamp(0.0, 255.0) as u8;
    let b = ((c & 0xff) as f64 * k).round().clamp(0.0, 255.0) as u8;
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// 0xRRGGBB scaled toward black/white by `k` (k=1 is unchanged).
fn shade(c: u32, k: f64) -> String {
    let k = k.clamp(0.0, 1.6);
    let r = (((c >> 16) & 0xff) as f64 * k).round().clamp(0.0, 255.0) as u8;
    let g = (((c >> 8) & 0xff) as f64 * k).round().clamp(0.0, 255.0) as u8;
    let b = ((c & 0xff) as f64 * k).round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

fn rgb_u(c: u32) -> String {
    format!("#{:06x}", c & 0xffffff)
}

fn rgba_u(c: u32, a: f64) -> String {
    format!("rgba({}, {}, {}, {})", (c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff, a.clamp(0.0, 1.0))
}

/// Atmospheric haze the far city melts into.
const FOG_COLOR: u32 = 0xd8e7f2;

/// Mix a color toward the haze by distance from the camera.
fn fog_mix_c(c: u32, depth: f64) -> u32 {
    let f = ((depth - 400.0) / 2600.0).clamp(0.0, 0.55);
    let mix = |sh: u32| {
        let a = ((c >> sh) & 0xff) as f64;
        let b = ((FOG_COLOR >> sh) & 0xff) as f64;
        ((a * (1.0 - f) + b * f).round() as u8) as u32
    };
    (mix(16) << 16) | (mix(8) << 8) | mix(0)
}

/// Box corners: indices 0..3 = bottom ring, 4..7 = top ring (same XY order).
const FACES: [[usize; 4]; 5] = [
    [0, 1, 5, 4],
    [1, 2, 6, 5],
    [2, 3, 7, 6],
    [3, 0, 4, 7],
    [4, 5, 6, 7],
];

/// Axis-aligned box from (x, y) with footprint (w, h) and height `ht`.
fn box_ax(x: f64, y: f64, w: f64, h: f64, ht: f64) -> [V; 8] {
    let ring = [
        V::new(x, y, 0.0),
        V::new(x + w, y, 0.0),
        V::new(x + w, y + h, 0.0),
        V::new(x, y + h, 0.0),
    ];
    let mut c = [V::new(0.0, 0.0, 0.0); 8];
    for i in 0..4 {
        c[i] = ring[i];
        c[i + 4] = V::new(ring[i].x, ring[i].y, ht);
    }
    c
}

/// Rotated box centered at (cx, cy), `len` along `heading`, `wid` across.
fn box_rot(cx: f64, cy: f64, heading: f64, len: f64, wid: f64, ht: f64) -> [V; 8] {
    let (fx, fy) = (heading.cos(), heading.sin());
    let (rx, ry) = (-fy, fx);
    let (hl, hw) = (len / 2.0, wid / 2.0);
    let ring = [
        (cx + fx * hl + rx * hw, cy + fy * hl + ry * hw), // front-right
        (cx - fx * hl + rx * hw, cy - fy * hl + ry * hw), // back-right
        (cx - fx * hl - rx * hw, cy - fy * hl - ry * hw), // back-left
        (cx + fx * hl - rx * hw, cy + fy * hl - ry * hw), // front-left
    ];
    let mut c = [V::new(0.0, 0.0, 0.0); 8];
    for i in 0..4 {
        c[i] = V::new(ring[i].0, ring[i].1, 0.0);
        c[i + 4] = V::new(ring[i].0, ring[i].1, ht);
    }
    c
}

/// Draw a box's 5 visible-candidate faces (4 walls + roof) with sun shading.
fn draw_box(r: &R3, corners: &[V; 8], color: u32) {
    for face in FACES {
        let v0 = corners[face[0]];
        let n = corners[face[1]].sub(v0).cross(corners[face[2]].sub(v0));
        // Back-face cull: normal must point toward the camera.
        if n.dot(r.cam.pos.sub(v0)) <= 0.0 {
            continue;
        }
        let k = 0.60 + 0.40 * n.normalized().dot(SUN).abs();
        let verts = [corners[face[0]], corners[face[1]], corners[face[2]], corners[face[3]]];
        r.fill_poly_grad(&verts, brighten(color, k));
    }
}

/// Copy of a corner array with all z raised by `dz`.
fn box_z(c: &[V; 8], dz: f64) -> [V; 8] {
    let mut o = *c;
    for v in o.iter_mut() {
        v.z += dz;
    }
    o
}

/// Window texture on the 4 walls: individual windows when close enough to
/// resolve, horizontal floor-bands at mid range, nothing when far.
fn draw_windows(r: &R3, c: &[V; 8], ht: f64) {
    for f in 0..4 {
        let a0 = c[f];
        let a1 = c[(f + 1) % 4];
        let t0 = c[f + 4];
        let u = a1.sub(a0);
        let up0 = t0.sub(a0);
        let l = u.len();
        if l < 16.0 || ht < 26.0 {
            continue;
        }
        // Cull walls that are not reasonably facing the camera: windows on
        // edge-on walls project as slivers. Also skip walls right up close
        // to the camera, where clipping produces stray quads.
        let n = u.cross(up0);
        let mid0 = V::new((a0.x + a1.x) / 2.0, (a0.y + a1.y) / 2.0, ht / 2.0);
        let vdir = mid0.sub(r.cam.pos).normalized();
        if n.normalized().dot(vdir) < 0.30 {
            continue;
        }
        if r.cam.pos.sub(mid0).len() < 30.0 {
            continue;
        }
        // Face center, only used for a scale estimate.
        let Some((_, _, depth)) = r.cam.project(mid0) else { continue };
        let s = r.cam.scale_at(depth);
        if s < 0.30 {
            continue; // too far away: keep the flat box
        }
        let up = t0.sub(a0); // wall "up" vector
        // t = fraction along the wall, z = absolute height (0..ht).
        let pt = |t: f64, z: f64| {
            let zf = z / ht;
            V::new(
                a0.x + u.x * t + up.x * zf,
                a0.y + u.y * t + up.y * zf,
                a0.z + u.z * t + up.z * zf,
            )
        };
        // Draw a window quad unless its projection is degenerate
        // (blown-up extent = sliver) or behind the near plane.
        let quad = |r: &R3, q: [V; 4], col: &str| {
            let (w, h) = (r.cam.w, r.cam.h);
            let mut xs = [0.0f64; 4];
            let mut ys = [0.0f64; 4];
            for (i, v) in q.iter().enumerate() {
                let Some((sx, sy, _)) = r.cam.project(*v) else { return };
                xs[i] = sx;
                ys[i] = sy;
            }
            let ext_x = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
                - xs.iter().cloned().fold(f64::INFINITY, f64::min);
            let ext_y = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
                - ys.iter().cloned().fold(f64::INFINITY, f64::min);
            if ext_x > 2.0 * w || ext_y > 2.0 * h {
                return; // degenerate sliver
            }
            r.fill_poly(&q, col);
        };
        let cols = ((l - 12.0) / 12.0).floor() as i32;
        let rows = ((ht - 12.0) / 12.0).floor() as i32;
        if cols < 1 || rows < 1 {
            continue;
        }
        if cols * rows > 56 {
            // Floor bands.
            for iy in 0..rows {
                let z0 = 6.0 + iy as f64 * 12.0;
                let z1 = z0 + 7.0;
                quad(r, [pt(0.04, z0), pt(0.96, z0), pt(0.96, z1), pt(0.04, z1)], "#26313d");
            }
        } else {
            let seed = (a0.x * 131.7 + a0.y * 97.3 + f as f64 * 7.0) as i64;
            for iy in 0..rows {
                for ix in 0..cols {
                    let t0 = (6.0 + ix as f64 * 12.0) / l;
                    let t1 = (6.0 + ix as f64 * 12.0 + 7.0) / l;
                    let z0 = 6.0 + iy as f64 * 12.0;
                    let z1 = z0 + 7.0;
                    let hsh = ((seed + ix as i64 * 17 + iy as i64 * 101).wrapping_mul(2654435761)) as u32;
                    let col = match hsh % 9 {
                        0 => "#e6d9a8", // the occasional lit window
                        _ => match hsh % 4 {
                            0 => "#1c2530",
                            1 => "#232e3b",
                            2 => "#2a3846",
                            _ => "#1f2b38",
                        },
                    };
                    quad(r, [pt(t0, z0), pt(t1, z0), pt(t1, z1), pt(t0, z1)], col);
                }
            }
        }
    }
}

/// Building: shaded box + windows + parapet outline + base AO + rooftop
/// props (AC unit, water tower, helipad, antenna).
fn draw_building(r: &R3, x: f64, y: f64, w: f64, h: f64, ht: f64, color: u32, time: f64) {
    // Camera inside (or close around) this box: skip it entirely —
    // near-plane-clipped walls would smear fullscreen slivers across the view.
    let cp = r.cam.pos;
    if cp.x > x - 24.0 && cp.x < x + w + 24.0 && cp.y > y - 24.0 && cp.y < y + h + 24.0 && cp.z <= ht + 2.0 {
        return;
    }
    let corners = box_ax(x, y, w, h, ht);
    draw_box(r, &corners, color);
    draw_windows(r, &corners, ht);
    // Ambient occlusion where the walls meet the street.
    for f in 0..4 {
        let a0 = corners[f];
        let a1 = corners[(f + 1) % 4];
        let u = a1.sub(a0);
        let up = corners[f + 4].sub(a0);
        let n = u.cross(up);
        let mid = V::new(a0.x + u.x * 0.5, a0.y + u.y * 0.5, 1.5);
        if n.dot(cp.sub(mid)) <= 0.0 {
            continue;
        }
        r.fill_poly(
            &[
                V::new(a0.x + u.x * 0.02, a0.y + u.y * 0.02, 0.2),
                V::new(a0.x + u.x * 0.98, a0.y + u.y * 0.98, 0.2),
                V::new(a0.x + u.x * 0.98, a0.y + u.y * 0.98, 3.4),
                V::new(a0.x + u.x * 0.02, a0.y + u.y * 0.02, 3.4),
            ],
            "rgba(0,0,0,0.15)",
        );
    }
    // Parapet outline + rooftop AC unit: only when the roof itself is
    // visible from above. When the camera is lower than the roof, these
    // roofline edges project as long streaks across the sky (the wall
    // behind them is edge-on and culled).
    if cp.z > ht {
        let edge = shade(color, 0.78);
        for i in 0..4 {
            r.line3d(corners[i + 4], corners[(i + 1) % 4 + 4], 2.5, &edge);
        }
        // Sunk-in roof panel for depth.
        let ctr = V::new(x + w / 2.0, y + h / 2.0, ht);
        let mut top = [V::new(0.0, 0.0, 0.0); 4];
        for i in 0..4 {
            let p = corners[i + 4];
            top[i] = V::new(p.x + (ctr.x - p.x) * 0.14, p.y + (ctr.y - p.y) * 0.14, ht - 0.4);
        }
        r.fill_poly(&top, "rgba(0,0,0,0.08)");
        let hh = ((x * 131.7 + y * 97.3) as i64).unsigned_abs() as u32;
        if hh % 2 == 0 && w > 26.0 && h > 26.0 {
            let ac = box_ax(x + w * 0.60, y + h * 0.25, 12.0, 9.0, 5.0);
            draw_box(r, &box_z(&ac, ht - 1.0), 0x78828d);
        }
        // Varied rooftop props so no two roofs feel the same.
        match hh % 7 {
            0 if w > 56.0 && h > 56.0 => {
                // Water tower: base frame, tank and cap.
                let wx = x + w - 18.0;
                let wy = y + 18.0;
                draw_box(r, &box_z(&box_rot(wx - 2.0, wy - 2.0, 0.0, 10.0, 10.0, 3.0), ht), 0x5d3f2a);
                draw_box(r, &box_z(&box_rot(wx, wy, 0.0, 13.0, 13.0, 9.0), ht + 3.0), 0x8a5f42);
                draw_box(r, &box_z(&box_rot(wx, wy, 0.0, 14.5, 14.5, 2.0), ht + 12.0), 0x6f4a30);
            }
            1 if w > 64.0 && h > 64.0 => {
                // Helipad: painted ring + H.
                let hx = x + w * 0.30;
                let hy = y + h * 0.32;
                let mut ring = Vec::with_capacity(16);
                for k in 0..16 {
                    let a = k as f64 * (std::f64::consts::TAU / 16.0);
                    ring.push(V::new(hx + a.cos() * 16.0, hy + a.sin() * 16.0, ht + 0.3));
                }
                if let Some((pts, _)) = r.screen(&ring) {
                    r.trace(&pts);
                    r.ctx.set_stroke_style_str("rgba(220,228,235,0.75)");
                    r.ctx.set_line_width(2.0);
                    r.ctx.stroke();
                }
                for (ox, oy, lw, lh) in [(-8.0f64, -7.0f64, 3.0, 14.0), (5.0, -7.0, 3.0, 14.0), (-8.0, -1.5, 16.0, 3.0)] {
                    r.fill_poly(
                        &[
                            V::new(hx + ox, hy + oy, ht + 0.4),
                            V::new(hx + ox + lw, hy + oy, ht + 0.4),
                            V::new(hx + ox + lw, hy + oy + lh, ht + 0.4),
                            V::new(hx + ox, hy + oy + lh, ht + 0.4),
                        ],
                        "rgba(220,228,235,0.8)",
                    );
                }
            }
            2 | 3 => {
                // Antenna mast with a blinking red beacon.
                let ax = x + w * 0.78;
                let ay = y + 12.0;
                r.line3d(V::new(ax, ay, ht), V::new(ax + 4.0, ay + 4.0, ht + 16.0), 1.6, "#aeb6bf");
                let on = (time * 1.6).fract() < 0.5;
                dot3d(
                    r,
                    V::new(ax + 4.0, ay + 4.0, ht + 17.0),
                    1.6,
                    if on { "rgba(255,80,70,0.95)" } else { "rgba(255,80,70,0.3)" },
                );
            }
            _ => {}
        }
    }
}

/// Detailed car: wheels + rims, raised body, glass cabin, roof, head/tail
/// lights; police add a dark stripe and a flashing light bar.
fn draw_car(
    r: &R3,
    x: f64,
    y: f64,
    heading: f64,
    len: f64,
    wid: f64,
    ht: f64,
    color: u32,
    police: bool,
    time: f64,
) {
    let (fx, fy) = (heading.cos(), heading.sin());
    let (rx, ry) = (-fy, fx);
    shadow(r, x, y, len * 0.55, wid * 0.75);

    // Wheels + rim rings (rims protrude slightly from the outer face).
    let lx = len / 2.0 - (len * 0.22).max(8.0);
    let ly = wid / 2.0 - 1.0;
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            let wx = x + fx * lx * sx + rx * ly * sy;
            let wy = y + fy * lx * sx + ry * ly * sy;
            draw_box(r, &box_rot(wx, wy, heading, 8.5, 3.6, 8.0), 0x15171b);
            let rw = wx + rx * sy * 1.1;
            let rw2 = wy + ry * sy * 1.1;
            draw_box(r, &box_z(&box_rot(rw, rw2, heading, 6.5, 3.2, 6.2), 0.9), 0xb8c0cc);
        }
    }
    // Body, sitting on the wheels, with a sunlit top panel.
    draw_box(r, &box_z(&box_rot(x, y, heading, len, wid, ht), 4.0), color);
    draw_box(
        r,
        &box_z(&box_rot(x, y, heading, len * 0.96, wid * 0.9, 1.2), 4.0 + ht),
        brighten(color, 1.18),
    );
    if police {
        draw_box(
            r,
            &box_z(&box_rot(x, y, heading, len * 0.97, wid + 1.0, 3.4), 6.5),
            0x1e232b,
        );
    }
    // Glass cabin + body-colored roof slab.
    let (cx, cy) = (x - fx * len * 0.07, y - fy * len * 0.07);
    let cl = len * 0.55;
    let cz = 4.0 + ht;
    draw_box(r, &box_z(&box_rot(cx, cy, heading, cl, wid * 0.88, 8.5), cz), 0x1e2f42);
    // Glass glint cap on top of the cabin.
    draw_box(
        r,
        &box_z(&box_rot(cx, cy, heading, cl * 0.94, wid * 0.92, 1.2), cz + 7.4),
        0x7f9fbf,
    );
    draw_box(
        r,
        &box_z(&box_rot(cx, cy, heading, cl * 0.99, wid * 0.90, 2.0), cz + 6.5),
        brighten(color, 1.1),
    );
    // Headlights (front) and taillights (rear).
    for sy in [-1.0, 1.0] {
        let lxh = len / 2.0 + 0.6;
        let (hx, hy) = (
            x + fx * lxh + rx * wid * 0.30 * sy,
            y + fy * lxh + ry * wid * 0.30 * sy,
        );
        draw_box(r, &box_z(&box_rot(hx, hy, heading, 1.4, 4.5, 4.0), 5.5), 0xfff3c4);
        let (tx, ty) = (
            x - fx * lxh + rx * wid * 0.30 * sy,
            y - fy * lxh + ry * wid * 0.30 * sy,
        );
        draw_box(r, &box_z(&box_rot(tx, ty, heading, 1.4, 4.5, 3.5), 5.5), 0xff5340);
    }
    if police {
        let on = ((time * 8.0).floor() as u32) % 2 == 0;
        let c = if on { 0xff3b30 } else { 0x3478f6 };
        let bar = box_z(&box_rot(x, y, heading, 10.0, 5.0, 4.0), cz + 8.5);
        draw_box(r, &bar, c);
    }
}

/// The city airplane: fuselage, wings, tail, cockpit and a spinning
/// propeller — at altitude `z`.
fn draw_plane(r: &R3, x: f64, y: f64, z: f64, heading: f64, time: f64) {
    shadow(r, x, y, 34.0, 15.0);
    let (fx, fy) = (heading.cos(), heading.sin());
    // Fuselage (box centered at altitude z + half its height).
    draw_box(r, &box_z(&box_rot(x, y, heading, 56.0, 12.0, 9.0), z + 5.0), 0xeef1f5);
    // Nose cone.
    let (nx, ny) = (x + fx * 29.0, y + fy * 29.0);
    draw_box(r, &box_z(&box_rot(nx, ny, heading, 8.0, 10.0, 7.0), z + 5.0), 0xc9d2dc);
    // Cockpit glass.
    let (cx, cy) = (x + fx * 7.0, y + fy * 7.0);
    draw_box(r, &box_z(&box_rot(cx, cy, heading, 14.0, 10.0, 4.5), z + 10.0), 0x151f2b);
    // Blue livery stripe along the fuselage.
    draw_box(r, &box_z(&box_rot(x, y, heading, 50.0, 12.8, 3.0), z + 5.5), 0x2f6fd0);
    // Main wings.
    let (wx, wy) = (x - fx * 6.0, y - fy * 6.0);
    let (rx, ry) = (-fy, fx);
    draw_box(r, &box_z(&box_rot(wx, wy, heading, 12.0, 64.0, 2.6), z + 8.5), 0xe8edf3);
    // Red wingtips.
    for sy in [-1.0, 1.0] {
        let (ax, ay) = (wx + rx * 29.0 * sy, wy + ry * 29.0 * sy);
        draw_box(r, &box_z(&box_rot(ax, ay, heading, 10.0, 8.0, 2.8), z + 8.4), 0xd0453a);
    }
    // Tail wings + red vertical fin.
    let (tx, ty) = (x - fx * 25.0, y - fy * 25.0);
    draw_box(r, &box_z(&box_rot(tx, ty, heading, 8.0, 26.0, 2.2), z + 7.5), 0xe8edf3);
    draw_box(r, &box_z(&box_rot(tx, ty, heading, 7.0, 3.0, 8.0), z + 12.0), 0xd0453a);
    // Spinning propeller: two blurred blade capsules whirling at the nose.
    let n0 = V::new(x + fx * 31.0, y + fy * 31.0, z + 5.0);
    for k in 0..2 {
        let a = time * 42.0 + k as f64 * std::f64::consts::FRAC_PI_2;
        let d = V::new(-fy * a.cos(), fx * a.cos(), a.sin());
        draw_cyl(r, n0.add(d.mul(12.0)), n0.sub(d.mul(12.0)), 1.5, 1.5, 6, 0.45, 0x4a5560);
    }
    // Spinner on the nose.
    draw_box(r, &box_z(&box_rot(x + fx * 32.0, y + fy * 32.0, heading, 3.0, 3.0, 3.0), z + 3.5), 0x3b4552);
}

/// Soft ground shadow (ellipse) under something at (x, y).
fn shadow(r: &R3, x: f64, y: f64, rx: f64, ry: f64) {
    shadow2(r, x, y, rx, ry, 0.28);
}

fn shadow2(r: &R3, x: f64, y: f64, rx: f64, ry: f64, alpha: f64) {
    if let Some((sx, sy, z)) = r.cam.project(V::new(x, y, 0.3)) {
        let s = r.cam.scale_at(z);
        r.ctx
            .set_fill_style_str(&format!("rgba(0,0,0,{alpha})"));
        r.ctx.begin_path();
        r.ctx.ellipse(
            sx,
            sy,
            (rx * s).max(1.0),
            (ry * s).max(1.0),
            0.0,
            0.0,
            std::f64::consts::TAU,
        );
        r.ctx.fill();
    }
}

/// Ground shadow as a world-space ellipse rotated to `heading`, so elongated
/// shadows (like an elephant's) keep their shape from any angle.
fn shadow_rot(r: &R3, x: f64, y: f64, heading: f64, rx: f64, ry: f64, alpha: f64) {
    let (fx, fy) = (heading.cos(), heading.sin());
    let (lx, ly) = (-fy, fx);
    let mut pts = Vec::with_capacity(16);
    for i in 0..16 {
        let t = i as f64 / 16.0 * std::f64::consts::TAU;
        let (ox, oy) = (t.cos() * rx, t.sin() * ry);
        pts.push(V::new(x + fx * ox + lx * oy, y + fy * ox + ly * oy, 0.3));
    }
    r.fill_poly(&pts, &format!("rgba(0,0,0,{alpha})"));
}

/// Flat world-space polygon, shaded against the sun (two-sided, never
/// culled — used for thin organic parts like ears, wings and tails).
fn fill_shaded(r: &R3, pts: &[V], color: u32) {
    if pts.len() < 3 {
        return;
    }
    let Some((sp, _)) = r.screen(pts) else { return };
    // Newell's method for a robust face normal.
    let mut n = V::new(0.0, 0.0, 0.0);
    for i in 0..pts.len() {
        n = n.add(pts[i].cross(pts[(i + 1) % pts.len()]));
    }
    let k = 0.55 + 0.45 * n.normalized().dot(SUN.normalized()).abs();
    r.trace(&sp);
    r.ctx.set_fill_style_str(&shade(color, k));
    r.ctx.fill();
}

/// Tapered cylinder (frustum) from `a` to `b`, radii `ra` -> `rb`, approx
/// with `seg` side faces. `squash` < 1 flattens the cross-section (use ~0.9
/// for organic bodies). This is the workhorse for wildlife limbs.
fn draw_cyl(r: &R3, a: V, b: V, ra: f64, rb: f64, seg: usize, squash: f64, color: u32) {
    let u = b.sub(a);
    let len = u.len();
    if len < 1e-3 {
        return;
    }
    let nrm = u.mul(1.0 / len);
    // A reference axis that isn't parallel to the cylinder axis.
    let refv = if nrm.z.abs() < 0.9 {
        V::new(0.0, 0.0, 1.0)
    } else {
        V::new(1.0, 0.0, 0.0)
    };
    let p1 = nrm.cross(refv).normalized();
    let p2 = nrm.cross(p1).normalized();
    let seg = seg.max(5);
    let ring = |center: V, rad: f64| -> Vec<V> {
        (0..seg)
            .map(|i| {
                let t = i as f64 / seg as f64 * std::f64::consts::TAU;
                let d = p1.mul(t.cos() * rad).add(p2.mul(t.sin() * rad * squash));
                center.add(d)
            })
            .collect()
    };
    let r0 = ring(a, ra);
    let r1 = ring(b, rb);
    let sun = SUN.normalized();
    for j in 0..seg {
        let j2 = (j + 1) % seg;
        let face = [r0[j], r0[j2], r1[j2], r1[j]];
        let v0 = face[0];
        let n = face[1].sub(v0).cross(face[2].sub(v0));
        if n.dot(r.cam.pos.sub(v0)) <= 0.0 {
            continue; // back face
        }
        let mut k = 0.68 + 0.50 * n.normalized().dot(sun).max(0.0);
        if n.z < -0.25 {
            k *= 0.85; // the belly is always in shade
        }
        r.fill_poly(&face, &shade(color, k.min(1.30)));
    }
}

/// A small shaded dot (eye, tuft) at a world position.
fn dot3d(r: &R3, p: V, radius: f64, color: &str) {
    if let Some((sx, sy, z)) = r.cam.project(p) {
        let rad = (radius * r.cam.scale_at(z)).max(0.6);
        r.ctx.set_fill_style_str(color);
        r.ctx.begin_path();
        r.ctx.arc(sx, sy, rad, 0.0, std::f64::consts::TAU);
        r.ctx.fill();
    }
}

/// An elephant built from shaded tapered cylinders: massive body, thick
/// neck, head with a long swaying trunk, big fan ears, four striding legs
/// with a diagonal gait, and a swishing tail.
fn draw_elephant(r: &R3, e: &crate::wildlife::Elephant, time: f64) {
    let sc = e.scale;
    let (fx, fy) = (e.heading.cos(), e.heading.sin());
    let moving = e.gait; // 0..1
    let bob = (e.phase * 2.0).sin() * 0.9 * sc * moving;
    // Local (lx forward, ly left, lz up) -> world.
    let loc = |lx: f64, ly: f64, lz: f64| {
        V::new(
            e.x + fx * (lx * sc) - fy * (ly * sc),
            e.y + fy * (lx * sc) + fx * (ly * sc),
            lz * sc + bob,
        )
    };
    shadow_rot(r, e.x, e.y, e.heading, 33.0 * sc, 17.0 * sc, 0.26);

    // Bluish-grey skin (like a real elephant in the sun).
    let skin = 0x939aa8;
    let skin_dark = 0x7f8794;
    let skin_light = 0xb3bac2;

    // Tail (drawn first: it's behind the body).
    let tail_sway = (time * 1.4 + e.seed).sin() * 2.5 * sc;
    r.line3d(loc(-26.0, 0.0, 21.0), loc(-33.0, tail_sway, 5.0), 1.4 * sc, "#6f747a");
    dot3d(r, loc(-33.0, tail_sway, 4.5), 1.4 * sc, "#6f747a");

    // Four legs, diagonal gait pairs (FL+HR and FR+HL swing together).
    let legs = [
        (20.0, -10.5, 0.0),
        (20.0, 10.5, std::f64::consts::PI),
        (-20.0, -10.5, std::f64::consts::PI),
        (-20.0, 10.5, 0.0),
    ];
    for (lx, ly, gait_off) in legs {
        let s = (e.phase + gait_off).sin();
        let lift = ((e.phase + gait_off).cos().max(0.0) * 2.6 * sc * moving).max(0.0);
        let fxo = s * 4.5 * moving * sc;
        let top = loc(lx, ly, 16.0);
        let foot = loc(lx + fxo, ly, 1.2 + lift);
        draw_cyl(r, top, foot, 5.4 * sc, 4.6 * sc, 8, 1.0, skin_dark);
        // Rounded toe, pointing forward.
        draw_cyl(
            r,
            loc(lx + fxo - 2.5, ly, 1.4 + lift),
            loc(lx + fxo + 4.5, ly, 1.4 + lift),
            4.4 * sc,
            3.6 * sc,
            8,
            1.0,
            skin_dark,
        );
    }

    // Barrel body (hips -> shoulders), then the rising neck and a big head.
    draw_cyl(r, loc(-28.0, 0.0, 19.0), loc(20.0, 0.0, 21.0), 14.5 * sc, 12.5 * sc, 10, 0.92, skin);
    draw_cyl(r, loc(17.0, 0.0, 21.0), loc(32.0, 0.0, 33.0), 10.0 * sc, 8.0 * sc, 9, 0.95, skin);
    draw_cyl(r, loc(31.0, 0.0, 33.0), loc(42.0, 0.0, 31.5), 8.5 * sc, 6.5 * sc, 9, 1.0, skin);

    // Ears: big flat fans hinged on the sides of the head (subtle flap).
    let ear_flap = (time * 1.1 + e.seed).sin() * 1.2;
    for sy in [-1.0, 1.0] {
        let pts = [
            loc(35.0, sy * 5.0, 38.0),
            loc(28.0, sy * 16.0, 40.0 + ear_flap * sc),
            loc(19.0, sy * 15.0, 26.0),
            loc(27.0, sy * 6.0, 28.0),
        ];
        fill_shaded(r, &pts, 0x787d84);
        // Pinker inner-ear skin.
        let pts_in = [
            loc(34.0, sy * 5.5, 36.5),
            loc(29.0, sy * 12.5, 37.5 + ear_flap * sc * 0.5),
            loc(22.5, sy * 12.0, 28.0),
            loc(27.5, sy * 6.5, 29.5),
        ];
        fill_shaded(r, &pts_in, 0xb8a2ab);
    }
    // Ivory tusks curling forward off the face.
    for sy in [-1.0, 1.0] {
        draw_cyl(
            r,
            loc(38.0, sy * 4.5, 29.0),
            loc(43.0, sy * 6.5, 24.5),
            1.3 * sc,
            0.7 * sc,
            6,
            1.0,
            0xf2ecdc,
        );
    }

    // Trunk: five tapering segments curling from the face down to the ground.
    let mut prev = loc(41.5, 0.0, 30.0);
    let mut prev_rad = 3.8 * sc;
    for i in 1..=5 {
        let fi = i as f64;
        let sway = (time * 1.1 + e.seed + fi * 0.6).sin() * (0.4 + fi * 0.45) * sc;
        let ysw = (time * 0.7 + e.seed).sin() * (0.15 + fi * 0.3) * sc;
        let p = loc(42.5 + fi * 2.0 + sway * 0.4, ysw, 30.0 - fi * 5.8);
        let rad = (3.8 - (fi - 1.0) * 0.55).max(1.1) * sc;
        draw_cyl(r, prev, p, prev_rad, rad, 7, 1.0, skin_light);
        prev = p;
        prev_rad = rad;
    }

    // Eyes.
    for sy in [-1.0, 1.0] {
        dot3d(r, loc(37.5, sy * 6.0, 35.0), 0.9 * sc, "#1c1c22");
    }
}

/// A bird: tapered-cylinder body, head, beak, a tail fan, and two wings
/// that sweep up and down (or hold flat while gliding).
fn draw_bird(r: &R3, b: &crate::wildlife::Bird) {
    let (fx, fy) = (b.heading.cos(), b.heading.sin());
    // Local (lx forward, ly left, lz up) -> world around the bird.
    let p = |lx: f64, ly: f64, lz: f64| {
        V::new(b.x + fx * lx - fy * ly, b.y + fy * lx + fx * ly, b.z + lz)
    };
    shadow2(r, b.x, b.y, 5.0, 3.0, 0.12);

    let w = b.len;
    let flapping = b.glide <= 0.0;
    let phi = if flapping { 0.85 * b.flap.sin() } else { -0.22 };

    // Tail fan.
    fill_shaded(
        r,
        &[
            p(-w * 0.5, 0.0, 0.0),
            p(-w * 0.5 - 4.0, -2.4, 0.9),
            p(-w * 0.5 - 4.0, 2.4, 0.9),
        ],
        b.color,
    );

    // Wings: five-point feathers from shoulder to tip, lifting with phi.
    let half = b.span * 0.5;
    for sy in [-1.0, 1.0] {
        let pts = [
            p(2.0, sy * 1.2, 1.1),
            p(-0.5, sy * 2.0, 1.1),
            p(-1.5, sy * half * 0.6, 1.1 + half * 0.6 * 0.85 * phi),
            p(-2.5, sy * half * 0.97, 1.1 + half * 0.97 * 0.85 * phi),
            p(-4.5, sy * half * 0.6, 1.1 + half * 0.6 * 0.85 * phi * 0.85),
        ];
        fill_shaded(r, &pts, b.color);
    }

    // Body, head, beak.
    draw_cyl(r, p(-w * 0.5, 0.0, 0.0), p(w * 0.25, 0.0, 0.1), 1.0, 2.2, 8, 0.8, b.color);
    draw_cyl(r, p(w * 0.22, 0.0, 0.2), p(w * 0.42, 0.0, 0.7), 1.9, 1.3, 8, 0.9, b.color);
    draw_cyl(r, p(w * 0.42, 0.0, 0.6), p(w * 0.42 + 3.2, 0.0, 0.5), 0.9, 0.05, 6, 0.9, b.beak);
}

/// The dragon, from its GLB model: transform all vertices (bank roll +
/// wingbeat + heading), back-face cull, project, sun-shade with the baked
/// vertex colors, fog by depth, and draw each color-quantized batch of
/// triangles as a single canvas path (far → near within a batch).
fn draw_dragon_mesh(r: &mut R3, d: &crate::wildlife::Dragon, m: &crate::wildlife::DragonMesh) {
    // Soft ground shadow, fading out with altitude.
    let a = (0.22 * (1.0 - (d.z - 150.0) / 900.0)).clamp(0.05, 0.22);
    shadow_rot(r, d.x, d.y, d.heading, m.half_span * 0.85, 12.0, a);

    let n = m.vpos.len();
    if n == 0 {
        return;
    }
    let (ch, sh) = (d.heading.cos(), d.heading.sin());
    let (cb, sb) = (d.bank.cos(), d.bank.sin());
    let flap = d.flap.sin() * m.half_span * 0.25;

    // Local (x forward, y left, z up) -> world, with bank roll + wingbeat.
    r.vtx.resize(n * 3, 0.0);
    for i in 0..n {
        let v = m.vpos[i];
        let lz = v[2] + flap * m.vwing[i];
        let ly = v[1] * cb - lz * sb;
        let lz2 = v[1] * sb + lz * cb;
        r.vtx[i * 3] = d.x + ch * v[0] - sh * ly;
        r.vtx[i * 3 + 1] = d.y + sh * v[0] + ch * ly;
        r.vtx[i * 3 + 2] = d.z + lz2;
    }

    let cp = r.cam.pos;
    let sun = SUN.normalized();
    let (w, h) = (r.cam.w, r.cam.h);
    r.faces.clear();
    r.faces.reserve(m.tris.len() / 2);
    for (ti, tri) in m.tris.iter().enumerate() {
        let [a, b, c] = (*tri).map(|i| i as usize * 3);
        let p0 = [r.vtx[a], r.vtx[a + 1], r.vtx[a + 2]];
        let p1 = [r.vtx[b], r.vtx[b + 1], r.vtx[b + 2]];
        let p2 = [r.vtx[c], r.vtx[c + 1], r.vtx[c + 2]];
        // Face normal (world), back-face cull.
        let nx = (p1[1] - p0[1]) * (p2[2] - p0[2]) - (p1[2] - p0[2]) * (p2[1] - p0[1]);
        let ny = (p1[2] - p0[2]) * (p2[0] - p0[0]) - (p1[0] - p0[0]) * (p2[2] - p0[2]);
        let nz = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0]);
        let nl = (nx * nx + ny * ny + nz * nz).sqrt();
        if nl < 1e-12 {
            continue;
        }
        if nx * (cp.x - p0[0]) + ny * (cp.y - p0[1]) + nz * (cp.z - p0[2]) <= 0.0 {
            continue;
        }
        // Project; skip if any vertex is behind the near plane.
        let pa = r.cam.project(V::new(p0[0], p0[1], p0[2]));
        let pb = r.cam.project(V::new(p1[0], p1[1], p1[2]));
        let pc = r.cam.project(V::new(p2[0], p2[1], p2[2]));
        let (Some(pa), Some(pb), Some(pc)) = (pa, pb, pc) else {
            continue;
        };
        let s = [[pa.0, pa.1], [pb.0, pb.1], [pc.0, pc.1]];
        let depth = (pa.2 + pb.2 + pc.2) / 3.0;
        // Screen-space cull: all vertices off one side of the viewport,
        // or the whole triangle projects sub-pixel.
        let x0 = s.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
        let x1 = s.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
        let y0 = s.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
        let y1 = s.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
        if x1 < -80.0 || x0 > w + 80.0 || y1 < -80.0 || y0 > h + 80.0 {
            continue;
        }
        if (x1 - x0) * (y1 - y0) < 0.15 {
            continue;
        }
        // Lighting: baked triangle color, sun term, belly shade, fog.
        let (nxx, nyy, nzz) = (nx / nl, ny / nl, nz / nl);
        let mut k = 0.52 + 0.55 * sun.dot(V::new(nxx, nyy, nzz)).max(0.0);
        if nzz < -0.2 {
            k *= 0.82; // the belly stays in shade
        }
        k = k.min(1.45);
        let fogf = ((depth - 400.0) / 2600.0).clamp(0.0, 0.55);
        let col = m.tric[ti];
        let fog = [(FOG_COLOR >> 16) as f64, (FOG_COLOR >> 8) as f64, FOG_COLOR as f64];
        let mixq = |c: u32, f: f64| -> u32 {
            let v = ((c as f64) * k).clamp(0.0, 255.0);
            let vf = v * (1.0 - fogf) + f * fogf;
            ((vf / 255.0 * 8.0).round() as u32).clamp(0, 7)
        };
        let key = mixq((col >> 16) & 0xff, fog[0]) << 6 | mixq((col >> 8) & 0xff, fog[1]) << 3 | mixq(col & 0xff, fog[2]);
        r.faces.push(DragonFace { depth, key, pts: [s[0][0], s[0][1], s[1][0], s[1][1], s[2][0], s[2][1]] });
    }

    // Sort by color key, then far → near, so each color batch is one path.
    r.faces.sort_by(|a, b| a.key.cmp(&b.key).then_with(|| b.depth.total_cmp(&a.depth)));
    let mut i = 0;
    while i < r.faces.len() {
        let key = r.faces[i].key;
        r.ctx.begin_path();
        while i < r.faces.len() && r.faces[i].key == key {
            let p = r.faces[i].pts;
            r.ctx.move_to(p[0], p[1]);
            r.ctx.line_to(p[2], p[3]);
            r.ctx.line_to(p[4], p[5]);
            r.ctx.close_path();
            i += 1;
        }
        let c = key_key_color(key);
        r.ctx.set_fill_style_str(&c);
        r.ctx.fill();
    }
}

/// Rebuild the quantized 3-bits-per-channel color key into a css string.
fn key_key_color(key: u32) -> String {
    let r = ((key >> 6) & 7) * 32 + 16;
    let g = ((key >> 3) & 7) * 32 + 16;
    let b = (key & 7) * 32 + 16;
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

/// Low-poly silhouette of the dragon (far away, or while the GLB is still
/// loading): swept wings, long neck, head, tail and a banked body.
fn draw_dragon_silhouette(r: &R3, d: &crate::wildlife::Dragon) {
    let span = 48.0;
    let len = 34.0;
    let bronze = 0x8a5c22;
    let bronze_dark = 0x6f4a1e;
    shadow2(r, d.x, d.y, 26.0, 12.0, 0.10);

    let (fx, fy) = (d.heading.cos(), d.heading.sin());
    let (cb, sb) = (d.bank.cos(), d.bank.sin());
    let flap = d.flap.sin();
    // Local (lx forward, ly left, lz up) -> world, with bank.
    let p = |lx: f64, ly: f64, lz: f64| {
        let ly2 = ly * cb - lz * sb;
        let lz2 = ly * sb + lz * cb;
        V::new(d.x + fx * lx - fy * ly2, d.y + fy * lx + fx * ly2, d.z + lz2)
    };
    let wy = flap * span * 0.10;

    // Tail, swept back.
    fill_shaded(
        r,
        &[
            p(-len * 0.35, 0.0, 0.0),
            p(-len * 0.72, 0.0, 2.0),
            p(-len * 0.52, 0.0, -1.5),
        ],
        bronze_dark,
    );
    // Wings: swept from the shoulders to the tips, lifting with the beat.
    for sy in [-1.0, 1.0] {
        fill_shaded(
            r,
            &[
                p(len * 0.12, sy * 2.0, 1.0),
                p(-len * 0.18, sy * 3.0, 1.5),
                p(-len * 0.30, sy * span * 0.5, 1.0 + wy),
                p(-len * 0.42, sy * span * 0.30, 0.5 + wy * 0.8),
            ],
            bronze,
        );
    }
    // Body (hips → chest), rising neck and head.
    draw_cyl(r, p(-len * 0.35, 0.0, 0.0), p(len * 0.22, 0.0, 1.5), 5.5, 4.0, 9, 0.9, bronze);
    draw_cyl(r, p(len * 0.22, 0.0, 1.5), p(len * 0.42, 0.0, 5.5), 3.6, 2.4, 8, 0.9, bronze);
    draw_cyl(r, p(len * 0.42, 0.0, 5.5), p(len * 0.52, 0.0, 5.0), 2.4, 1.2, 7, 1.0, bronze_dark);
}

enum Kind {
    Building { x: f64, y: f64, w: f64, h: f64, ht: f64, color: u32 },
    Car {
        x: f64,
        y: f64,
        heading: f64,
        len: f64,
        wid: f64,
        ht: f64,
        color: u32,
        police: bool,
    },
    Plane { x: f64, y: f64, z: f64, heading: f64 },
    /// `lift` = height of the ground the figure stands on (0 = the street;
    /// the elephant rider stands on the elephant's back).
    Ped { x: f64, y: f64, color: u32, dead: bool, lift: f64 },
    Elephant(crate::wildlife::Elephant),
    Bird(crate::wildlife::Bird),
    Dragon(crate::wildlife::Dragon),
    Tree { x: f64, y: f64, r: f64 },
    Marker { x: f64, y: f64, green: bool, pulse: f64 },
}

struct Item {
    d2: f64,
    kind: Kind,
}

fn dist2(cam: V, x: f64, y: f64, z: f64) -> f64 {
    let dx = x - cam.x;
    let dy = y - cam.y;
    let dz = z - cam.z;
    dx * dx + dy * dy + dz * dz
}

const DRAW_RANGE2: f64 = 3200.0 * 3200.0;

/// Render the full 3D frame.
pub fn render(ctx: &CanvasRenderingContext2d, s: &GameState, w: f64, h: f64, dpr: f64) {
    ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);

    let (px, py) = s.player_pos();
    // The chase cam rides along at the player's altitude (in the plane).
    let alt = s.player_alt();
    let speed = if s.in_dragon {
        s.wildlife.dragon.speed
    } else if s.on_foot {
        0.0
    } else if let Some(i) = s.riding {
        let e = &s.wildlife.elephants[i];
        e.speed * e.gait // the elephant's amble
    } else {
        s.active_vehicle().speed()
    };
    let t = (speed / 400.0).clamp(0.0, 1.0);
    let (dist, height) = if s.on_foot {
        (95.0, 50.0)
    } else if s.riding.is_some() {
        // A bit further back and higher so the whole elephant is in frame.
        (135.0, 75.0)
    } else if s.in_dragon {
        // Ride close behind and just above the dragon's back, so the beast is
        // centered in the frame rather than low beneath the camera.
        (175.0 + 95.0 * t, 46.0 + 30.0 * t)
    } else {
        (140.0 + 140.0 * t, 85.0 + 70.0 * t)
    };
    let (fx, fy) = (s.cam3d_yaw.cos(), s.cam3d_yaw.sin());
    let cam = Cam3D::new(
        V::new(px - fx * dist, py - fy * dist, height + alt),
        s.cam3d_yaw,
        w,
        h,
    )
    .with_pitch(s.cam3d_pitch);
    let mut r = R3 {
        ctx,
        cam,
        dmesh: s.dragon_mesh.as_ref(),
        vtx: Vec::new(),
        faces: Vec::new(),
    };

    // ---- Sky (horizon shifts with the user's pitch) ----
    let horizon = cam.horizon().clamp(0.0, h);
    #[allow(deprecated)] // web-sys has no non-deprecated gradient overload
    {
        let g = ctx.create_linear_gradient(0.0, 0.0, 0.0, h.max(1.0));
        let _ = g.add_color_stop(0.0, "#3f83d8");
        let _ = g.add_color_stop((horizon / h).clamp(0.05, 1.0) as f32 * 0.55, "#7fb3e8");
        let _ = g.add_color_stop((horizon / h).clamp(0.05, 1.0) as f32, "#e8f4ff");
        ctx.set_fill_style(&JsValue::from(g));
        ctx.fill_rect(0.0, 0.0, w, h);
    }
    // Sun with a soft halo, parked high in the sky.
    #[allow(deprecated)] // web-sys gradient API
    {
        let sun_x = w * 0.74;
        let sun_y = (horizon - h * 0.24).max(30.0);
        let g = ctx.create_radial_gradient(sun_x, sun_y, 4.0, sun_x, sun_y, 150.0).unwrap();
        let _ = g.add_color_stop(0.0, "rgba(255,250,220,0.9)");
        let _ = g.add_color_stop(0.25, "rgba(255,244,200,0.35)");
        let _ = g.add_color_stop(1.0, "rgba(255,244,200,0.0)");
        ctx.set_fill_style(&JsValue::from(g));
        ctx.begin_path();
        ctx.arc(sun_x, sun_y, 150.0, 0.0, std::f64::consts::TAU);
        ctx.fill();
        ctx.set_fill_style_str("rgba(255,252,238,0.95)");
        ctx.begin_path();
        ctx.arc(sun_x, sun_y, 24.0, 0.0, std::f64::consts::TAU);
        ctx.fill();
    }
    // Drifting soft clouds (screen-space, slow parallax drift).
    for i in 0..6u32 {
        let cx = ((i as f64 * 421.3 + s.time * (5.0 + i as f64 * 1.7)) % (w + 320.0)) - 160.0;
        let cy = h * (0.06 + ((i as f64 * 0.0731).fract()) * 0.30);
        if cy > horizon - 26.0 {
            continue;
        }
        let sc = 0.7 + ((i as f64 * 0.37).fract()) * 0.7;
        ctx.set_fill_style_str("rgba(255,255,255,0.55)");
        ctx.begin_path();
        ctx.ellipse(cx, cy, 68.0 * sc, 15.0 * sc, 0.0, 0.0, std::f64::consts::TAU);
        ctx.fill();
        ctx.begin_path();
        ctx.ellipse(cx + 42.0 * sc, cy - 11.0 * sc, 46.0 * sc, 13.0 * sc, 0.0, 0.0, std::f64::consts::TAU);
        ctx.fill();
        ctx.begin_path();
        ctx.ellipse(cx - 48.0 * sc, cy - 7.0 * sc, 40.0 * sc, 11.0 * sc, 0.0, 0.0, std::f64::consts::TAU);
        ctx.fill();
    }
    // Ground plane (extends past the city).
    ctx.set_fill_style_str("#3e8e52");
    ctx.fill_rect(0.0, horizon - 1.0, w, (h - horizon + 2.0).max(0.0));
    // Lawn: per-cell tone variation so the ground reads as real grass,
    // melting into the haze with distance.
    for j in 0..=N {
        for i in 0..=N {
            let x0 = i as f64 * CELL;
            let y0 = j as f64 * CELL;
            let c = match (i * 3 + j * 7) % 3 {
                0 => 0x3e8e52,
                1 => 0x3a854c,
                _ => 0x429355,
            };
            let mut v = [V::new(0.0, 0.0, 0.0); 4];
            v[0] = V::new(x0, y0, 0.0);
            v[1] = V::new(x0 + CELL, y0, 0.0);
            v[2] = V::new(x0 + CELL, y0 + CELL, 0.0);
            v[3] = V::new(x0, y0 + CELL, 0.0);
            r.fill_poly_grad(&v, c);
        }
    }

    // ---- Roads ----
    for i in 0..=N {
        let c = i as f64 * CELL;
        let mut v = [V::new(0.0, 0.0, 0.0); 4];
        v[0] = V::new(c, 0.0, 0.0);
        v[1] = V::new(c + ROAD, 0.0, 0.0);
        v[2] = V::new(c + ROAD, SIZE, 0.0);
        v[3] = V::new(c, SIZE, 0.0);
        r.fill_poly_grad(&v, 0x3b3f46);
        v[0] = V::new(0.0, c, 0.0);
        v[1] = V::new(SIZE, c, 0.0);
        v[2] = V::new(SIZE, c + ROAD, 0.0);
        v[3] = V::new(0.0, c + ROAD, 0.0);
        r.fill_poly_grad(&v, 0x3b3f46);
        // White shoulder lines on both edges.
        for off in [5.0f64, ROAD - 6.5] {
            let mut s = [V::new(0.0, 0.0, 0.0); 4];
            s[0] = V::new(c + off, 0.0, 0.04);
            s[1] = V::new(c + off + 1.6, 0.0, 0.04);
            s[2] = V::new(c + off + 1.6, SIZE, 0.04);
            s[3] = V::new(c + off, SIZE, 0.04);
            r.fill_poly(&s, "rgba(235,238,240,0.38)");
            s[0] = V::new(0.0, c + off, 0.04);
            s[1] = V::new(SIZE, c + off, 0.04);
            s[2] = V::new(SIZE, c + off + 1.6, 0.04);
            s[3] = V::new(0.0, c + off + 1.6, 0.04);
            r.fill_poly(&s, "rgba(235,238,240,0.38)");
        }
    }

    // Zebra crosswalks at every intersection approach (culled by distance).
    let cw_range2 = 1300.0 * 1300.0;
    {
        let cp0 = cam.pos;
        for j in 0..=N {
            for i in 0..=N {
                let (ci, cj) = (i as f64 * CELL, j as f64 * CELL);
                if dist2(cp0, ci + ROAD / 2.0, cj + ROAD / 2.0, 0.0) > cw_range2 {
                    continue;
                }
                let mut t = 8.0;
                while t + 5.0 < ROAD - 6.0 {
                    r.fill_poly(&[V::new(ci + t, cj - 15.0, 0.04), V::new(ci + t + 5.0, cj - 15.0, 0.04), V::new(ci + t + 5.0, cj - 4.0, 0.04), V::new(ci + t, cj - 4.0, 0.04)], "rgba(230,233,236,0.5)");
                    r.fill_poly(&[V::new(ci + t, cj + ROAD + 4.0, 0.04), V::new(ci + t + 5.0, cj + ROAD + 4.0, 0.04), V::new(ci + t + 5.0, cj + ROAD + 15.0, 0.04), V::new(ci + t, cj + ROAD + 15.0, 0.04)], "rgba(230,233,236,0.5)");
                    r.fill_poly(&[V::new(ci - 15.0, cj + t, 0.04), V::new(ci - 4.0, cj + t, 0.04), V::new(ci - 4.0, cj + t + 5.0, 0.04), V::new(ci - 15.0, cj + t + 5.0, 0.04)], "rgba(230,233,236,0.5)");
                    r.fill_poly(&[V::new(ci + ROAD + 4.0, cj + t, 0.04), V::new(ci + ROAD + 15.0, cj + t, 0.04), V::new(ci + ROAD + 15.0, cj + t + 5.0, 0.04), V::new(ci + ROAD + 4.0, cj + t + 5.0, 0.04)], "rgba(230,233,236,0.5)");
                    t += 11.0;
                }
            }
        }
    }

    let cpos = cam.pos;

    // Dashed yellow center lines on the roads (culled by distance).
    let dash_range2 = 1600.0 * 1600.0;
    for i in 0..=N {
        let c = i as f64 * CELL + ROAD / 2.0;
        let mut d = 6.0;
        while d < SIZE - 24.0 {
            if dist2(cpos, c, d + 7.0, 0.0) < dash_range2 {
                r.fill_poly(&[V::new(c - 1.5, d, 0.05), V::new(c + 1.5, d, 0.05), V::new(c + 1.5, d + 14.0, 0.05), V::new(c - 1.5, d + 14.0, 0.05)], "#f0cf4a");
            }
            if dist2(cpos, d + 7.0, c, 0.0) < dash_range2 {
                r.fill_poly(&[V::new(d, c - 1.5, 0.05), V::new(d + 14.0, c - 1.5, 0.05), V::new(d + 14.0, c + 1.5, 0.05), V::new(d, c + 1.5, 0.05)], "#f0cf4a");
            }
            d += 40.0;
        }
    }

    // ---- Blocks: sidewalk slabs & parks, with darker curb edges ----
    for j in 0..N {
        for i in 0..N {
            let bx = i as f64 * CELL + ROAD;
            let by = j as f64 * CELL + ROAD;
            let b = s.city.block(i, j);
            let mut v = [V::new(0.0, 0.0, 0.0); 4];
            v[0] = V::new(bx, by, 0.0);
            v[1] = V::new(bx + BLOCK, by, 0.0);
            v[2] = V::new(bx + BLOCK, by + BLOCK, 0.0);
            v[3] = V::new(bx, by + BLOCK, 0.0);
            match b.kind {
                crate::city::BlockKind::Buildings => r.fill_poly_grad(&v, 0xa3adb6),
                crate::city::BlockKind::Park => r.fill_poly_grad(&v, 0x43a25c),
            }
            // Curb line just inside each edge of the slab.
            let curb = if b.kind == crate::city::BlockKind::Park {
                "#3a8a4b"
            } else {
                "#8b95a0"
            };
            r.fill_poly(&[V::new(bx, by, 0.05), V::new(bx + BLOCK, by, 0.05), V::new(bx + BLOCK, by + 3.0, 0.05), V::new(bx, by + 3.0, 0.05)], curb);
            r.fill_poly(&[V::new(bx, by + BLOCK, 0.05), V::new(bx + BLOCK, by + BLOCK, 0.05), V::new(bx + BLOCK, by + BLOCK - 3.0, 0.05), V::new(bx, by + BLOCK - 3.0, 0.05)], curb);
            r.fill_poly(&[V::new(bx, by, 0.05), V::new(bx + 3.0, by, 0.05), V::new(bx + 3.0, by + BLOCK, 0.05), V::new(bx, by + BLOCK, 0.05)], curb);
            r.fill_poly(&[V::new(bx + BLOCK, by, 0.05), V::new(bx + BLOCK - 3.0, by, 0.05), V::new(bx + BLOCK - 3.0, by + BLOCK, 0.05), V::new(bx + BLOCK, by + BLOCK, 0.05)], curb);
        }
    }

    // ---- Collect 3D objects, sort far → near (painter's algorithm) ----
    let mut items: Vec<Item> = Vec::with_capacity(200);
    let push = |items: &mut Vec<Item>, d2: f64, kind: Kind| {
        if d2 < DRAW_RANGE2 {
            items.push(Item { d2, kind });
        }
    };

    for blk in &s.city.blocks {
        match blk.kind {
            crate::city::BlockKind::Buildings => {
                for lot in blk.buildings.iter().flatten() {
                    let (cx, cy) = lot.center();
                    push(
                        &mut items,
                        dist2(cpos, cx, cy, lot.height / 2.0),
                        Kind::Building { x: lot.x, y: lot.y, w: lot.w, h: lot.h, ht: lot.height, color: lot.color },
                    );
                }
            }
            crate::city::BlockKind::Park => {
                if let Some(park) = blk.park {
                    for (tx, ty, tr) in park.trees {
                        push(&mut items, dist2(cpos, tx, ty, 10.0), Kind::Tree { x: tx, y: ty, r: tr });
                    }
                }
            }
        }
    }

    for tcar in &s.traffic {
        let c = &tcar.car;
        push(
            &mut items,
            dist2(cpos, c.x, c.y, 6.0),
            Kind::Car { x: c.x, y: c.y, heading: c.heading, len: 36.0, wid: 17.0, ht: 13.0, color: c.color, police: false },
        );
    }
    for p in &s.police {
        push(
            &mut items,
            dist2(cpos, p.x, p.y, 6.0),
            Kind::Car { x: p.x, y: p.y, heading: p.heading, len: 36.0, wid: 17.0, ht: 13.0, color: 0xf1f1f1, police: true },
        );
    }
    for p in &s.peds {
        let dead = matches!(p.state, crate::ped::PedState::Dead(_));
        push(
            &mut items,
            dist2(cpos, p.x, p.y, 10.0),
            Kind::Ped { x: p.x, y: p.y, color: p.color, dead, lift: 0.0 },
        );
    }
    for e in &s.wildlife.elephants {
        push(
            &mut items,
            dist2(cpos, e.x, e.y, 16.0 * e.scale),
            Kind::Elephant(*e),
        );
    }
    for b in &s.wildlife.birds {
        push(&mut items, dist2(cpos, b.x, b.y, b.z), Kind::Bird(*b));
    }
    {
        let d = &s.wildlife.dragon;
        push(&mut items, dist2(cpos, d.x, d.y, d.z), Kind::Dragon(*d));
    }
    if s.on_foot || s.riding.is_some() {
        let (lx, ly) = s.player_pos();
        // The rider stands on the elephant's back.
        let lift = if let Some(i) = s.riding {
            33.0 * s.wildlife.elephants[i].scale
        } else {
            0.0
        };
        push(
            &mut items,
            dist2(cpos, lx, ly, 10.0 + lift),
            Kind::Ped { x: lx, y: ly, color: 0xffe0b2, dead: false, lift },
        );
    }
    // The vehicle the player is in (or the car, when on foot) plus the other
    // parked vehicle, so both the car and the plane are always visible.
    if !s.in_plane {
        let c = &s.car;
        push(
            &mut items,
            dist2(cpos, c.x, c.y, 6.0),
            Kind::Car { x: c.x, y: c.y, heading: c.heading, len: 44.0, wid: 22.0, ht: 16.0, color: c.color, police: false },
        );
    }
    {
        let p = &s.plane;
        push(
            &mut items,
            dist2(cpos, p.x, p.y, p.z + 5.0),
            Kind::Plane { x: p.x, y: p.y, z: p.z, heading: p.heading },
        );
    }

    if let Some((mx, my)) = s.mission.current_marker() {
        let green = matches!(s.mission.phase, crate::mission::MissionPhase::ToDeliver);
        let pulse = 1.0 + 0.15 * (s.time * 3.0).sin();
        push(
            &mut items,
            dist2(cpos, mx, my, 60.0),
            Kind::Marker { x: mx, y: my, green, pulse },
        );
    }

    items.sort_by(|a, b| b.d2.total_cmp(&a.d2));

    for it in &items {
        match &it.kind {
            Kind::Building { x, y, w, h, ht, color } => {
                draw_building(&r, *x, *y, *w, *h, *ht, *color, s.time);
            }
            Kind::Car { x, y, heading, len, wid, ht, color, police } => {
                draw_car(&r, *x, *y, *heading, *len, *wid, *ht, *color, *police, s.time);
            }
            Kind::Elephant(e) => {
                draw_elephant(&r, e, s.time);
            }
            Kind::Bird(b) => {
                draw_bird(&r, b);
            }
            Kind::Dragon(d) => {
                // Full GLB model up close; a low-poly silhouette when the
                // mesh is far away (too small to resolve) or not loaded yet.
                let d2 = dist2(cam.pos, d.x, d.y, d.z);
                match r.dmesh {
                    Some(m) if d2 < 1300.0 * 1300.0 => draw_dragon_mesh(&mut r, d, m),
                    _ => draw_dragon_silhouette(&r, d),
                }
            }
            Kind::Plane { x, y, z, heading } => {
                draw_plane(&r, *x, *y, *z, *heading, s.time);
            }
            Kind::Ped { x, y, color, dead, lift } => {
                if *dead {
                    ctx.set_global_alpha(0.55);
                }
                // The shadow stays on the street below, even for the rider.
                shadow(&r, *x, *y, 8.0, 8.0);
                // Legs, arms, torso, head (skin + hair cap), raised by `lift`.
                draw_box(&r, &box_rot(*x, *y, *lift, 8.0, 8.0, 11.0), 0x2b303c);
                draw_box(
                    &r,
                    &box_z(&box_rot(*x - 6.8, *y, *lift, 3.0, 6.5, 13.0), 9.0),
                    brighten(*color, 0.8),
                );
                draw_box(
                    &r,
                    &box_z(&box_rot(*x + 3.8, *y, *lift, 3.0, 6.5, 13.0), 9.0),
                    brighten(*color, 0.8),
                );
                draw_box(&r, &box_z(&box_rot(*x, *y, *lift, 9.5, 9.5, 13.0), 10.0), *color);
                // Sunlit shoulder line on the torso.
                draw_box(
                    &r,
                    &box_z(&box_rot(*x, *y, *lift, 9.9, 9.9, 1.4), 21.6),
                    brighten(*color, 1.2),
                );
                if let Some((sx, sy, z)) = cam.project(V::new(*x, *y, 27.5 + *lift)) {
                    let rad = (4.5 * cam.scale_at(z)).max(1.0);
                    ctx.set_fill_style_str("#5a4634");
                    ctx.begin_path();
                    ctx.arc(sx, sy, rad, 0.0, std::f64::consts::TAU);
                    ctx.fill();
                    // Skin face below the hair cap.
                    ctx.set_fill_style_str("#e8b87f");
                    ctx.begin_path();
                    ctx.arc(sx, sy + rad * 0.15, rad * 0.85, 0.0, std::f64::consts::TAU);
                    ctx.fill();
                }
                ctx.set_global_alpha(1.0);
            }
            Kind::Tree { x, y, r: rad } => {
                let tr = *rad;
                // Deterministic palette variation per tree.
                let th = ((*x * 131.7 + *y * 97.3) as i64).unsigned_abs() as u32;
                let (c1, c2, c3, c4) = match th % 3 {
                    0 => ("#1f4a28", "#2d6a3f", "#46b063", "#7fd492"),
                    1 => ("#24522f", "#357a46", "#54c074", "#94e0a5"),
                    _ => ("#2a5230", "#3f7d43", "#67c26b", "#a4e695"),
                };
                shadow(&r, *x, *y, tr * 1.2, tr * 1.2);
                r.line3d(V::new(*x, *y, 0.0), V::new(*x, *y, 16.0), 5.0, "#6b5238");
                // Stacked canopy blobs: dark base, bright crown, sunlit glint.
                for (z, k, col) in [
                    (15.0, 1.8, c1),
                    (19.0, 1.45, c2),
                    (22.0, 1.05, c3),
                    (24.0, 0.62, c4),
                ] {
                    if let Some((sx, sy, zc)) = cam.project(V::new(*x, *y, z)) {
                        let sc = cam.scale_at(zc);
                        let rxx = (tr * k * sc).max(1.0);
                        let ryy = rxx * 0.75;
                        ctx.set_fill_style_str(col);
                        ctx.begin_path();
                        ctx.ellipse(sx, sy, rxx, ryy, 0.0, 0.0, std::f64::consts::TAU);
                        ctx.fill();
                    }
                }
            }
            Kind::Marker { x, y, green, pulse } => {
                let color = if *green { "#4ce06a" } else { "#ffd93b" };
                let rad = 42.0 * *pulse;
                let mut ring = Vec::with_capacity(24);
                for k in 0..24 {
                    let a = k as f64 * (std::f64::consts::TAU / 24.0);
                    ring.push(V::new(x + a.cos() * rad, y + a.sin() * rad, 1.0));
                }
                let lw = if let Some((_, _, z)) = cam.project(V::new(*x, *y, 1.0)) {
                    (5.0 * cam.scale_at(z)).clamp(1.0, 30.0)
                } else {
                    0.0
                };
                // Expanding radar ring, endlessly rippling out from the base.
                let k0 = (s.time * 0.7).rem_euclid(1.0);
                let mut ring0 = Vec::with_capacity(20);
                for kk in 0..20 {
                    let a = kk as f64 * (std::f64::consts::TAU / 20.0);
                    ring0.push(V::new(x + a.cos() * rad * (0.6 + 1.2 * k0), y + a.sin() * rad * (0.6 + 1.2 * k0), 1.1));
                }
                if let Some((pts, _)) = r.screen(&ring0) {
                    r.trace(&pts);
                    ctx.set_stroke_style_str(&format!("rgba(255,255,255,{})", (0.5 * (1.0 - k0)).max(0.0)));
                    ctx.set_line_width((lw * 0.5).max(1.0));
                    ctx.stroke();
                }
                if lw > 0.0 {
                    // Translucent disc + ring.
                    if let Some((pts, _)) = r.screen(&ring) {
                        r.trace(&pts);
                        ctx.set_fill_style_str(color);
                        ctx.set_global_alpha(0.25);
                        ctx.fill();
                        ctx.set_global_alpha(1.0);
                        ctx.set_stroke_style_str(color);
                        ctx.set_line_width(lw);
                        ctx.stroke();
                    }
                    // Brighter inner ring.
                    let mut ring2 = Vec::with_capacity(24);
                    for k in 0..24 {
                        let a = k as f64 * (std::f64::consts::TAU / 24.0);
                        ring2.push(V::new(x + a.cos() * rad * 0.55, y + a.sin() * rad * 0.55, 1.2));
                    }
                    if let Some((pts, _)) = r.screen(&ring2) {
                        r.trace(&pts);
                        ctx.set_stroke_style_str("rgba(255,255,255,0.9)");
                        ctx.set_line_width((lw * 0.6).max(1.0));
                        ctx.stroke();
                    }
                    // Vertical beam.
                    ctx.set_global_alpha(0.30);
                    r.line3d(V::new(*x, *y, 0.0), V::new(*x, *y, 220.0), 18.0, color);
                    ctx.set_global_alpha(0.85);
                    r.line3d(V::new(*x, *y, 0.0), V::new(*x, *y, 220.0), 5.0, color);
                    ctx.set_global_alpha(1.0);
                }
            }
        }
    }

    // ---- Particles (world-space billboards) ----
    draw_fx(&r, &s.fx, w, h);

    // Subtle screen-space vignette to frame the world.
    draw_vignette(ctx, w, h);

    // ---- HUD + overlays (shared with 2D) ----
    draw_hud(ctx, s, w, h);
    draw_overlays(ctx, s, w, h);

    // Mode tag.
    ctx.set_font("bold 14px 'Segoe UI', system-ui, sans-serif");
    ctx.set_text_align("left");
    ctx.set_fill_style_str("rgba(0,0,0,0.45)");
    ctx.fill_rect(12.0, h - 108.0, 620.0, 26.0);
    ctx.set_fill_style_str("#9be7ff");
    if s.in_plane {
        ctx.fill_text("PLANE — DRAG: steer · LMB: throttle · RMB: brake · WHEEL: speed", 20.0, h - 90.0);
    } else {
        ctx.fill_text("3D — DRAG: LOOK · C: RESET · V: TOP-DOWN", 20.0, h - 90.0);
    }
}

/// Draw the particle pool in 3D: each particle is a billboarded dot/streak
/// sized by its distance from the camera.
fn draw_fx(r: &R3, fx: &crate::fx::Fx, w: f64, h: f64) {
    for p in &fx.particles {
        let Some((sx, sy, z)) = r.cam.project(V::new(p.x, p.y, p.z)) else {
            continue;
        };
        if sx < -60.0 || sx > w + 60.0 || sy < -60.0 || sy > h + 60.0 {
            continue;
        }
        let f = crate::fx::Fx::fade(p);
        let a = (p.alpha * f).clamp(0.0, 1.0);
        if a <= 0.01 {
            continue;
        }
        let s = r.cam.scale_at(z);
        let rad = (p.size * s).clamp(0.4, 240.0);
        match p.kind {
            crate::fx::PKind::Smoke => {
                r.ctx.set_fill_style_str(&rgba_u(p.color, a * 0.8));
                r.ctx.begin_path();
                r.ctx.arc(sx, sy, rad, 0.0, std::f64::consts::TAU);
                r.ctx.fill();
            }
            crate::fx::PKind::Spark => {
                // Streak back along the velocity.
                let back = r.cam.project(V::new(p.x - p.vx * 0.05, p.y - p.vy * 0.05, p.z - p.vz * 0.05));
                r.ctx.set_stroke_style_str(&rgba_u(p.color, a));
                r.ctx.set_line_width(rad.clamp(0.8, 4.0));
                r.ctx.begin_path();
                if let Some((bx, by, _)) = back {
                    r.ctx.move_to(bx, by);
                    r.ctx.line_to(sx, sy);
                }
                r.ctx.stroke();
            }
            crate::fx::PKind::Glitter => {
                let rr = rad * (1.0 + 0.4 * (p.life * 9.0).sin()).max(0.8);
                r.ctx.set_stroke_style_str(&rgba_u(p.color, a));
                r.ctx.set_line_width(1.3);
                r.ctx.begin_path();
                r.ctx.move_to(sx - rr, sy);
                r.ctx.line_to(sx + rr, sy);
                r.ctx.move_to(sx, sy - rr);
                r.ctx.line_to(sx, sy + rr);
                r.ctx.stroke();
                r.ctx.set_fill_style_str(&rgba_u(0xffffff, a));
                r.ctx.begin_path();
                r.ctx.arc(sx, sy, rr * 0.35, 0.0, std::f64::consts::TAU);
                r.ctx.fill();
            }
            crate::fx::PKind::Debris => {
                r.ctx.set_fill_style_str(&rgba_u(p.color, a));
                r.ctx.fill_rect(sx - rad * 0.5, sy - rad * 0.5, rad, rad);
            }
        }
    }
}

/// Subtle screen-space vignette (screen-space transform).
fn draw_vignette(ctx: &CanvasRenderingContext2d, w: f64, h: f64) {
    let rr = (w * h).sqrt();
    let g = ctx.create_radial_gradient(w / 2.0, h / 2.0, rr * 0.42, w / 2.0, h / 2.0, rr * 0.72).unwrap();
    let _ = g.add_color_stop(0.0, "rgba(0,0,10,0.0)");
    let _ = g.add_color_stop(1.0, "rgba(0,0,10,0.22)");
    ctx.set_fill_style(&JsValue::from(g));
    ctx.fill_rect(0.0, 0.0, w, h);
}
