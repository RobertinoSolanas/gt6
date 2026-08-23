//! 3D mode: a software perspective renderer drawn on the same 2D canvas.
//!
//! Chase camera behind the player (yaw smoothed in `GameState`), the city
//! extruded into boxes, painter's-algorithm depth sorting, Sutherland–
//! Hodgman near-plane clipping, and a cheap fixed-sun face shading.

#![allow(unused_must_use)]

use wasm_bindgen::JsValue;
use web_sys::CanvasRenderingContext2d;

use crate::cam3d::{clip_near, Cam3D, NEAR, V3 as V};
use crate::city::{BLOCK, CELL, N, ROAD, SIZE};
use crate::render::{draw_hud, draw_overlays};
use crate::state::GameState;

/// Fixed "sun" direction for face shading (normalized at use site).
const SUN: V = V::new(0.45, -0.55, 0.70);

type Pts = Vec<(f64, f64)>;

/// Renderer state for one frame (css-px coordinates).
struct R3<'a> {
    ctx: &'a CanvasRenderingContext2d,
    cam: Cam3D,
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

/// 0xRRGGBB scaled toward black/white by `k` (k=1 is unchanged).
fn shade(c: u32, k: f64) -> String {
    let k = k.clamp(0.0, 1.6);
    let r = (((c >> 16) & 0xff) as f64 * k).round().clamp(0.0, 255.0) as u8;
    let g = (((c >> 8) & 0xff) as f64 * k).round().clamp(0.0, 255.0) as u8;
    let b = ((c & 0xff) as f64 * k).round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", r, g, b)
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
        r.fill_poly(&verts, &shade(color, k));
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

/// Building: shaded box + windows + parapet outline + rooftop AC unit.
fn draw_building(r: &R3, x: f64, y: f64, w: f64, h: f64, ht: f64, color: u32) {
    // Camera inside (or close around) this box: skip it entirely —
    // near-plane-clipped walls would smear fullscreen slivers across the view.
    let cp = r.cam.pos;
    if cp.x > x - 24.0 && cp.x < x + w + 24.0 && cp.y > y - 24.0 && cp.y < y + h + 24.0 && cp.z <= ht + 2.0 {
        return;
    }
    let corners = box_ax(x, y, w, h, ht);
    draw_box(r, &corners, color);
    draw_windows(r, &corners, ht);
    // Parapet outline + rooftop AC unit: only when the roof itself is
    // visible from above. When the camera is lower than the roof, these
    // roofline edges project as long streaks across the sky (the wall
    // behind them is edge-on and culled).
    if cp.z > ht {
        let edge = shade(color, 0.78);
        for i in 0..4 {
            r.line3d(corners[i + 4], corners[(i + 1) % 4 + 4], 2.5, &edge);
        }
        let hh = ((x * 131.7 + y * 97.3) as i64).unsigned_abs() as u32;
        if hh % 2 == 0 && w > 26.0 && h > 26.0 {
            let ac = box_ax(x + w * 0.60, y + h * 0.25, 12.0, 9.0, 5.0);
            draw_box(r, &box_z(&ac, ht - 1.0), 0x78828d);
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
            draw_box(r, &box_z(&box_rot(rw, rw2, heading, 6.5, 3.2, 6.2), 0.9), 0x99a1ac);
        }
    }
    // Body, sitting on the wheels.
    draw_box(r, &box_z(&box_rot(x, y, heading, len, wid, ht), 4.0), color);
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
    draw_box(r, &box_z(&box_rot(cx, cy, heading, cl, wid * 0.88, 8.5), cz), 0x151f2b);
    draw_box(
        r,
        &box_z(&box_rot(cx, cy, heading, cl * 0.99, wid * 0.90, 2.0), cz + 6.5),
        color,
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
        draw_box(r, &box_z(&box_rot(tx, ty, heading, 1.4, 4.5, 3.5), 5.5), 0xd84330);
    }
    if police {
        let on = ((time * 8.0).floor() as u32) % 2 == 0;
        let c = if on { 0xff3b30 } else { 0x3478f6 };
        let bar = box_z(&box_rot(x, y, heading, 10.0, 5.0, 4.0), cz + 8.5);
        draw_box(r, &bar, c);
    }
}

/// Soft ground shadow (ellipse) under something at (x, y).
fn shadow(r: &R3, x: f64, y: f64, rx: f64, ry: f64) {
    if let Some((sx, sy, z)) = r.cam.project(V::new(x, y, 0.3)) {
        let s = r.cam.scale_at(z);
        r.ctx.set_fill_style_str("rgba(0,0,0,0.28)");
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
    Ped { x: f64, y: f64, color: u32, dead: bool },
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
    let speed = if s.on_foot { 0.0 } else { s.car.speed() };
    let t = (speed / 400.0).clamp(0.0, 1.0);
    let (dist, height) = if s.on_foot {
        (95.0, 50.0)
    } else {
        (140.0 + 140.0 * t, 85.0 + 70.0 * t)
    };
    let (fx, fy) = (s.cam3d_yaw.cos(), s.cam3d_yaw.sin());
    let cam = Cam3D::new(
        V::new(px - fx * dist, py - fy * dist, height),
        s.cam3d_yaw,
        w,
        h,
    )
    .with_pitch(s.cam3d_pitch);
    let r = R3 { ctx, cam };

    // ---- Sky (horizon shifts with the user's pitch) ----
    let horizon = cam.horizon().clamp(0.0, h);
    #[allow(deprecated)] // web-sys has no non-deprecated gradient overload
    {
        let g = ctx.create_linear_gradient(0.0, 0.0, 0.0, h.max(1.0));
        let _ = g.add_color_stop(0.0, "#79b6ec");
        let _ = g.add_color_stop((horizon / h).clamp(0.05, 1.0) as f32, "#cfe7fb");
        ctx.set_fill_style(&JsValue::from(g));
        ctx.fill_rect(0.0, 0.0, w, h);
    }
    // Ground plane (extends past the city).
    ctx.set_fill_style_str("#41704b");
    ctx.fill_rect(0.0, horizon - 1.0, w, (h - horizon + 2.0).max(0.0));

    // ---- Roads ----
    ctx.set_fill_style_str("#3b3f46");
    for i in 0..=N {
        let c = i as f64 * CELL;
        let mut v = [V::new(0.0, 0.0, 0.0); 4];
        v[0] = V::new(c, 0.0, 0.0);
        v[1] = V::new(c + ROAD, 0.0, 0.0);
        v[2] = V::new(c + ROAD, SIZE, 0.0);
        v[3] = V::new(c, SIZE, 0.0);
        r.fill_poly(&v, "#3b3f46");
        v[0] = V::new(0.0, c, 0.0);
        v[1] = V::new(SIZE, c, 0.0);
        v[2] = V::new(SIZE, c + ROAD, 0.0);
        v[3] = V::new(0.0, c + ROAD, 0.0);
        r.fill_poly(&v, "#3b3f46");
    }

    let cpos = cam.pos;

    // Dashed yellow center lines on the roads (culled by distance).
    let dash_range2 = 1600.0 * 1600.0;
    for i in 0..=N {
        let c = i as f64 * CELL + ROAD / 2.0;
        let mut d = 6.0;
        while d < SIZE - 24.0 {
            if dist2(cpos, c, d + 7.0, 0.0) < dash_range2 {
                r.fill_poly(&[V::new(c - 1.5, d, 0.05), V::new(c + 1.5, d, 0.05), V::new(c + 1.5, d + 14.0, 0.05), V::new(c - 1.5, d + 14.0, 0.05)], "#c9a227");
            }
            if dist2(cpos, d + 7.0, c, 0.0) < dash_range2 {
                r.fill_poly(&[V::new(d, c - 1.5, 0.05), V::new(d + 14.0, c - 1.5, 0.05), V::new(d + 14.0, c + 1.5, 0.05), V::new(d, c + 1.5, 0.05)], "#c9a227");
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
                crate::city::BlockKind::Buildings => r.fill_poly(&v, "#8f9aa3"),
                crate::city::BlockKind::Park => r.fill_poly(&v, "#3e8e52"),
            }
            // Curb line just inside each edge of the slab.
            let curb = if b.kind == crate::city::BlockKind::Park {
                "#357a45"
            } else {
                "#78828b"
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
            Kind::Ped { x: p.x, y: p.y, color: p.color, dead },
        );
    }
    if s.on_foot {
        let (lx, ly) = s.player_pos();
        push(
            &mut items,
            dist2(cpos, lx, ly, 10.0),
            Kind::Ped { x: lx, y: ly, color: 0xffe0b2, dead: false },
        );
    } else {
        let c = &s.car;
        push(
            &mut items,
            dist2(cpos, c.x, c.y, 6.0),
            Kind::Car { x: c.x, y: c.y, heading: c.heading, len: 44.0, wid: 22.0, ht: 16.0, color: c.color, police: false },
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
                draw_building(&r, *x, *y, *w, *h, *ht, *color);
            }
            Kind::Car { x, y, heading, len, wid, ht, color, police } => {
                draw_car(&r, *x, *y, *heading, *len, *wid, *ht, *color, *police, s.time);
            }
            Kind::Ped { x, y, color, dead } => {
                if *dead {
                    ctx.set_global_alpha(0.55);
                }
                shadow(&r, *x, *y, 8.0, 8.0);
                // Legs, torso, head.
                draw_box(&r, &box_rot(*x, *y, 0.0, 8.0, 8.0, 11.0), 0x2b303c);
                draw_box(&r, &box_z(&box_rot(*x, *y, 0.0, 9.5, 9.5, 13.0), 10.0), *color);
                if let Some((sx, sy, z)) = cam.project(V::new(*x, *y, 27.5)) {
                    let rad = (4.5 * cam.scale_at(z)).max(1.0);
                    ctx.set_fill_style_str("#e0ac69");
                    ctx.begin_path();
                    ctx.arc(sx, sy, rad, 0.0, std::f64::consts::TAU);
                    ctx.fill();
                }
                ctx.set_global_alpha(1.0);
            }
            Kind::Tree { x, y, r: rad } => {
                let tr = *rad;
                shadow(&r, *x, *y, tr * 1.2, tr * 1.2);
                r.line3d(V::new(*x, *y, 0.0), V::new(*x, *y, 16.0), 5.0, "#5b4632");
                // Three stacked canopy blobs, darker at the base.
                for (z, k, col) in [(15.0, 1.8, "#24522f"), (19.0, 1.45, "#2d6a3f"), (22.0, 1.05, "#4caf6d")] {
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
                let color = if *green { "#4caf50" } else { "#ffd60a" };
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

    // ---- HUD + overlays (shared with 2D) ----
    draw_hud(ctx, s, w, h);
    draw_overlays(ctx, s, w, h);

    // Mode tag.
    ctx.set_font("bold 14px 'Segoe UI', system-ui, sans-serif");
    ctx.set_text_align("left");
    ctx.set_fill_style_str("rgba(0,0,0,0.45)");
    ctx.fill_rect(12.0, h - 108.0, 380.0, 26.0);
    ctx.set_fill_style_str("#9be7ff");
    ctx.fill_text("3D — DRAG: LOOK · C: RESET · V: TOP-DOWN", 20.0, h - 90.0);
}
