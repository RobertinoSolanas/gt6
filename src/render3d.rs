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
    let cam = Cam3D::new(V::new(px - fx * dist, py - fy * dist, height), s.cam3d_yaw, w, h);
    let r = R3 { ctx, cam };

    // ---- Sky (camera has no pitch, so the horizon is mid-screen) ----
    #[allow(deprecated)] // web-sys has no non-deprecated gradient overload
    {
        let g = ctx.create_linear_gradient(0.0, 0.0, 0.0, h);
        let _ = g.add_color_stop(0.0, "#79b6ec");
        let _ = g.add_color_stop(0.5, "#cfe7fb");
        ctx.set_fill_style(&JsValue::from(g));
        ctx.fill_rect(0.0, 0.0, w, h);
    }
    // Ground plane (extends past the city).
    ctx.set_fill_style_str("#41704b");
    ctx.fill_rect(0.0, h / 2.0 - 1.0, w, h / 2.0 + 2.0);

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

    // ---- Blocks: sidewalk slabs & parks (flat) ----
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
        }
    }

    // ---- Collect 3D objects, sort far → near (painter's algorithm) ----
    let mut items: Vec<Item> = Vec::with_capacity(200);
    let cpos = cam.pos;
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
                draw_box(&r, &box_ax(*x, *y, *w, *h, *ht), *color);
            }
            Kind::Car { x, y, heading, len, wid, ht, color, police } => {
                shadow(&r, *x, *y, *len * 0.55, *wid * 0.7);
                draw_box(&r, &box_rot(*x, *y, *heading, *len, *wid, *ht), *color);
                if *police {
                    // Flashing light bar on the roof.
                    let on = ((s.time * 8.0).floor() as u32) % 2 == 0;
                    let c = if on { 0xff3b30 } else { 0x3478f6 };
                    let bar = box_rot(*x, *y, *heading, 10.0, 5.0, 4.0)
                        .map(|v| V::new(v.x, v.y, v.z + *ht));
                    draw_box(&r, &bar, c);
                }
            }
            Kind::Ped { x, y, color, dead } => {
                if *dead {
                    ctx.set_global_alpha(0.55);
                }
                shadow(&r, *x, *y, 8.0, 8.0);
                draw_box(&r, &box_rot(*x, *y, 0.0, 9.0, 9.0, 24.0), *color);
                // Head.
                if let Some((sx, sy, z)) = cam.project(V::new(*x, *y, 29.0)) {
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
                if let Some((sx, sy, z)) = cam.project(V::new(*x, *y, 18.0)) {
                    let s = cam.scale_at(z);
                    let rx = (tr * 1.7 * s).max(1.0);
                    let ry = rx * 0.75;
                    ctx.set_fill_style_str("#2d6a3f");
                    ctx.begin_path();
                    ctx.ellipse(sx, sy + 2.0 * s, rx, ry, 0.0, 0.0, std::f64::consts::TAU);
                    ctx.fill();
                    ctx.set_fill_style_str("#4caf6d");
                    ctx.begin_path();
                    ctx.ellipse(sx, sy, rx, ry, 0.0, 0.0, std::f64::consts::TAU);
                    ctx.fill();
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
    ctx.fill_rect(12.0, h - 108.0, 132.0, 26.0);
    ctx.set_fill_style_str("#9be7ff");
    ctx.fill_text("3D — V: TOP-DOWN", 20.0, h - 90.0);
}
