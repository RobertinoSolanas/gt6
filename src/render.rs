#![allow(unused_must_use)]
#![allow(deprecated)] // web-sys gradient setters
//! Canvas 2D renderer: world (roads, blocks, entities) + HUD (money, stars,
//! speed, minimap, messages) + particle FX.

use web_sys::CanvasRenderingContext2d;

use crate::city::{BLOCK, CELL, N, ROAD, SIZE};
use crate::state::GameState;

const FONT: &str = "bold 16px 'Segoe UI', system-ui, sans-serif";
const FONT_BIG: &str = "bold 42px 'Segoe UI', system-ui, sans-serif";

fn rgb(c: u32) -> String {
    format!("#{:06x}", c & 0xffffff)
}

fn rgba(c: u32, a: f64) -> String {
    format!("rgba({}, {}/ 255, {}/ 255, {})", (c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff, a.clamp(0.0, 1.0))
}

/// 0xRRGGBB scaled toward black/white by `k` (k=1 is unchanged), as a u32.
fn shade_c(c: u32, k: f64) -> u32 {
    let k = k.clamp(0.0, 1.6);
    let r = (((c >> 16) & 0xff) as f64 * k).round().clamp(0.0, 255.0) as u8;
    let g = (((c >> 8) & 0xff) as f64 * k).round().clamp(0.0, 255.0) as u8;
    let b = ((c & 0xff) as f64 * k).round().clamp(0.0, 255.0) as u8;
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// 0xRRGGBB scaled toward black/white by `k` (k=1 is unchanged).
fn shade(c: u32, k: f64) -> String {
    rgb(shade_c(c, k))
}

/// Visible world rectangle (with margin) used to cull draw work.
struct View {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl View {
    fn contains(&self, x: f64, y: f64, w: f64, h: f64) -> bool {
        x + w > self.x0 && x < self.x1 && y + h > self.y0 && y < self.y1
    }
}

/// Render the full frame.
pub fn render(ctx: &CanvasRenderingContext2d, s: &GameState, w: f64, h: f64, dpr: f64) {
    // ---- Ground ----
    ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
    // Darker grass beyond the city limits.
    ctx.set_fill_style_str("#2e5a3a");
    ctx.fill_rect(0.0, 0.0, w, h);

    // ---- World space ----
    let zoom = 1.0;
    ctx.set_transform(
        dpr * zoom,
        0.0,
        0.0,
        dpr * zoom,
        dpr * (w / 2.0 - s.cam_x * zoom),
        dpr * (h / 2.0 - s.cam_y * zoom),
    );

    // Visible world rect for culling.
    let pad = 80.0;
    let view = View {
        x0: s.cam_x - w / 2.0 - pad,
        y0: s.cam_y - h / 2.0 - pad,
        x1: s.cam_x + w / 2.0 + pad,
        y1: s.cam_y + h / 2.0 + pad,
    };

    // City lawn with a subtle mowed checker pattern.
    ctx.set_fill_style_str("#3f7f4d");
    ctx.fill_rect(0.0, 0.0, SIZE, SIZE);
    ctx.set_fill_style_str("rgba(255,255,255,0.03)");
    for j in 0..N {
        for i in 0..N {
            if (i + j) % 2 == 0 {
                let cx = i as f64 * CELL;
                let cy = j as f64 * CELL;
                if view.contains(cx, cy, CELL, CELL) {
                    ctx.fill_rect(cx, cy, CELL, CELL);
                }
            }
        }
    }

    draw_roads(ctx, s, &view);
    draw_blocks(ctx, s, &view);
    draw_mission_marker(ctx, s);
    for t in &s.traffic {
        draw_car(ctx, &t.car, false, s.time);
    }
    for p in &s.police {
        draw_car(ctx, p, true, s.time);
    }
    // Player: vehicles (any not being driven) or person.
    if s.on_foot || s.in_plane {
        draw_car(ctx, &s.car, false, s.time);
    }
    if s.on_foot || !s.in_plane {
        draw_plane(ctx, &s.plane, s.time);
    }
    draw_peds(ctx, s);
    for e in &s.wildlife.elephants {
        draw_elephant(ctx, e, s.time);
    }
    if s.on_foot || s.riding.is_some() {
        // On foot — or the rider, drawn on top of the elephant's back.
        draw_person(ctx, s.foot_x, s.foot_y, s.foot_heading, 0xffe0b2, s.time);
    } else if s.in_plane {
        draw_plane(ctx, &s.plane, s.time);
    } else {
        draw_car(ctx, &s.car, false, s.time);
    }
    for b in &s.wildlife.birds {
        draw_bird(ctx, b);
    }
    draw_dragon(ctx, &s.wildlife.dragon);
    draw_fx(ctx, &s.fx, &view);

    // ---- HUD (screen space) ----
    ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
    draw_vignette(ctx, w, h);
    draw_hud(ctx, s, w, h);
    draw_hud(ctx, s, w, h);
    draw_overlays(ctx, s, w, h);
}

/// BUSTED screen + PAUSED overlay (shared by the 2D and 3D renderers).
pub fn draw_overlays(ctx: &CanvasRenderingContext2d, s: &GameState, w: f64, h: f64) {
    if s.time < s.busted_until {
        draw_busted(ctx, w, h);
    }
    if s.paused && s.time >= s.busted_until {
        ctx.set_fill_style_str("rgba(0,0,0,0.5)");
        ctx.fill_rect(0.0, 0.0, w, h);
        ctx.set_fill_style_str("rgba(0,0,0,0.55)");
        fill_round(ctx, w / 2.0 - 260.0, h / 2.0 - 44.0, 520.0, 64.0, 14.0);
        ctx.set_stroke_style_str("rgba(255,255,255,0.35)");
        ctx.set_line_width(2.0);
        ctx.begin_path();
        let _ = ctx.round_rect_with_f64(w / 2.0 - 260.0, h / 2.0 - 44.0, 520.0, 64.0, 14.0);
        let _ = ctx.stroke();
        ctx.set_fill_style_str("#ffffff");
        ctx.set_font(FONT_BIG);
        ctx.set_text_align("center");
        ctx.fill_text("PAUSED — P TO RESUME", w / 2.0, h / 2.0);
    }
}

fn draw_roads(ctx: &CanvasRenderingContext2d, s: &GameState, view: &View) {
    // Asphalt.
    ctx.set_fill_style_str("#3a3e45");
    for i in 0..=N {
        let c = i as f64 * CELL;
        if view.contains(c, 0.0, ROAD, SIZE) {
            ctx.fill_rect(c, 0.0, ROAD, SIZE);
        }
        if view.contains(0.0, c, SIZE, ROAD) {
            ctx.fill_rect(0.0, c, SIZE, ROAD);
        }
    }
    // Lighter wear band down the middle of each carriageway.
    ctx.set_fill_style_str("rgba(255,255,255,0.045)");
    for i in 0..=N {
        let c = i as f64 * CELL;
        if view.contains(c + ROAD * 0.30, 0.0, ROAD * 0.40, SIZE) {
            ctx.fill_rect(c + ROAD * 0.30, 0.0, ROAD * 0.40, SIZE);
        }
        if view.contains(0.0, c + ROAD * 0.30, SIZE, ROAD * 0.40) {
            ctx.fill_rect(0.0, c + ROAD * 0.30, SIZE, ROAD * 0.40);
        }
    }
    // White shoulder lines just inside each road edge.
    ctx.set_fill_style_str("rgba(255,255,255,0.42)");
    for i in 0..=N {
        let c = i as f64 * CELL;
        if view.contains(c, 0.0, ROAD, SIZE) {
            ctx.fill_rect(c + 4.0, 0.0, 2.0, SIZE);
            ctx.fill_rect(c + ROAD - 6.0, 0.0, 2.0, SIZE);
        }
        if view.contains(0.0, c, SIZE, ROAD) {
            ctx.fill_rect(0.0, c + 4.0, SIZE, 2.0);
            ctx.fill_rect(0.0, c + ROAD - 6.0, SIZE, 2.0);
        }
    }
    // Center lane dashes.
    ctx.set_stroke_style_str("rgba(250,222,92,0.6)");
    ctx.set_line_width(3.0);
    let dashes = js_sys::Array::of2(&JsValue::from_f64(18.0), &JsValue::from_f64(26.0));
    ctx.set_line_dash(&dashes);
    for i in 0..=N {
        let c = i as f64 * CELL + ROAD / 2.0;
        if view.contains(c, 0.0, 1.0, SIZE) {
            ctx.begin_path();
            ctx.move_to(c, 0.0);
            ctx.line_to(c, SIZE);
            ctx.stroke();
        }
        if view.contains(0.0, c, SIZE, 1.0) {
            ctx.begin_path();
            ctx.move_to(0.0, c);
            ctx.line_to(SIZE, c);
            ctx.stroke();
        }
    }
    let none = js_sys::Array::new();
    ctx.set_line_dash(&none);

    // Asphalt speckle texture: deterministic chips & patches per 24px cell
    // of road, so the surface reads as worn concrete instead of flat grey.
    let gs = 24.0;
    let x0 = view.x0.clamp(0.0, SIZE);
    let y0 = view.y0.clamp(0.0, SIZE);
    let x1 = view.x1.clamp(0.0, SIZE);
    let y1 = view.y1.clamp(0.0, SIZE);
    if x1 > x0 && y1 > y0 {
        let i0 = (x0 / gs).floor() as i64;
        let j0 = (y0 / gs).floor() as i64;
        let i1 = (x1 / gs).ceil() as i64;
        let j1 = (y1 / gs).ceil() as i64;
        for j in j0..j1 {
            for i in i0..i1 {
                let h = ((i as i64).wrapping_mul(374761393).wrapping_add(j as i64 * 668265263))
                    .wrapping_mul(1274126177) as u32;
                let cx = i as f64 * gs + (h as f64 % gs);
                let cy = j as f64 * gs + ((h >> 9) as f64 % gs);
                if !s.city.is_road(cx, cy) {
                    continue;
                }
                // One dark chip, one light chip, one faint wear patch.
                let h2 = (h >> 17) as f64;
                ctx.set_fill_style_str("rgba(0,0,0,0.14)");
                ctx.fill_rect(cx, cy, 2.4, 1.6);
                ctx.set_fill_style_str("rgba(255,255,255,0.06)");
                ctx.fill_rect(cx + (h2 % (gs - 4.0)), cy + (h2 * 7.1 % (gs - 4.0)), 2.0, 1.4);
                if h % 5 == 0 {
                    ctx.set_fill_style_str("rgba(255,255,255,0.028)");
                    ctx.fill_rect(cx + (h2 * 13.7 % (gs - 10.0)), cy + (h2 * 3.3 % (gs - 10.0)), 12.0, 7.0);
                }
            }
        }
    }

    // Zebra crosswalks on all four approaches of every intersection.
    ctx.set_fill_style_str("rgba(235,238,240,0.55)");
    for j in 0..=N {
        for i in 0..=N {
            let ci = i as f64 * CELL;
            let cj = j as f64 * CELL;
            if !view.contains(ci - 20.0, cj - 20.0, ROAD + 40.0, ROAD + 40.0) {
                continue;
            }
            // North / south approach (pedestrians cross the vertical road).
            let mut x = ci + 7.0;
            while x < ci + ROAD - 8.0 {
                ctx.fill_rect(x, cj - 15.0, 5.0, 11.0);
                ctx.fill_rect(x, cj + ROAD + 4.0, 5.0, 11.0);
                x += 11.0;
            }
            // East / west approach (pedestrians cross the horizontal road).
            let mut y = cj + 7.0;
            while y < cj + ROAD - 8.0 {
                ctx.fill_rect(ci - 15.0, y, 11.0, 5.0);
                ctx.fill_rect(ci + ROAD + 4.0, y, 11.0, 5.0);
                y += 11.0;
            }
        }
    }
}

fn draw_blocks(ctx: &CanvasRenderingContext2d, s: &GameState, view: &View) {
    for j in 0..N {
        for i in 0..N {
            let bx = i as f64 * CELL + ROAD;
            let by = j as f64 * CELL + ROAD;
            if !view.contains(bx, by, BLOCK, BLOCK) {
                continue;
            }
            let b = s.city.block(i, j);
            match b.kind {
                crate::city::BlockKind::Buildings => {
                    // Sidewalk slab: light stone with a darker curb edge.
                    ctx.set_fill_style_str("#a7b1ba");
                    ctx.fill_rect(bx, by, BLOCK, BLOCK);
                    ctx.set_fill_style_str("#98a2ab");
                    ctx.fill_rect(bx, by + BLOCK - 5.0, BLOCK, 5.0);
                    ctx.fill_rect(bx + BLOCK - 5.0, by, 5.0, BLOCK);
                    // Subtle slab paving seams.
                    ctx.set_fill_style_str("rgba(0,0,0,0.07)");
                    for q in 1..4 {
                        let t = bx + (q as f64) * BLOCK / 4.0;
                        ctx.fill_rect(t, by, 1.5, BLOCK);
                        ctx.fill_rect(bx, by + (q as f64) * BLOCK / 4.0, BLOCK, 1.5);
                    }
                    for lot in b.buildings.iter().flatten() {
                        let (cx, cy) = lot.center();
                        // Drop shadow onto the slab.
                        ctx.set_fill_style_str("rgba(0,0,0,0.20)");
                        ctx.fill_rect(lot.x + 5.0, lot.y + 6.0, lot.w, lot.h);
                        // Deterministic per-roof hash for the prop variety.
                        let hsh = ((lot.x * 131.7 + lot.y * 97.3) as i64).unsigned_abs();
                        // Roof: diagonal sun gradient for a less flat look.
                        let c0 = rgb(shade_c(lot.color, 1.10));
                        let c1 = rgb(shade_c(lot.color, 0.94));
                        let g = ctx.create_linear_gradient(lot.x, lot.y, lot.x + lot.w, lot.y + lot.h);
                        let _ = g.add_color_stop(0.0, &c0);
                        let _ = g.add_color_stop(1.0, &c1);
                        ctx.set_fill_style(&JsValue::from(g));
                        ctx.fill_rect(lot.x, lot.y, lot.w, lot.h);
                        // Sunk-in roof inset for depth.
                        ctx.set_fill_style_str("rgba(0,0,0,0.10)");
                        ctx.fill_rect(lot.x + 6.0, lot.y + 6.0, lot.w - 12.0, lot.h - 12.0);
                        // Subtle tar-paper panel seams on larger roofs.
                        if lot.w > 60.0 && lot.h > 60.0 {
                            ctx.set_fill_style_str("rgba(0,0,0,0.05)");
                            for q in 1..((lot.w / 30.0) as i32).min(6) {
                                let t = lot.x + q as f64 * 30.0;
                                ctx.fill_rect(t, lot.y + 4.0, 1.2, lot.h - 8.0);
                            }
                            for q in 1..((lot.h / 30.0) as i32).min(6) {
                                let t = lot.y + q as f64 * 30.0;
                                ctx.fill_rect(lot.x + 4.0, t, lot.w - 8.0, 1.2);
                            }
                        }
                        // Sun bevel: bright top-left edge, shaded bottom-right.
                        ctx.set_line_width(3.0);
                        ctx.set_stroke_style_str("rgba(255,255,255,0.35)");
                        ctx.begin_path();
                        ctx.move_to(lot.x, lot.y + lot.h);
                        ctx.line_to(lot.x, lot.y);
                        ctx.line_to(lot.x + lot.w, lot.y);
                        ctx.stroke();
                        ctx.set_stroke_style_str("rgba(0,0,0,0.35)");
                        ctx.begin_path();
                        ctx.move_to(lot.x + lot.w, lot.y);
                        ctx.line_to(lot.x + lot.w, lot.y + lot.h);
                        ctx.line_to(lot.x, lot.y + lot.h);
                        ctx.stroke();
                        // AC unit detail with a lit top edge.
                        ctx.set_fill_style_str("rgba(0,0,0,0.22)");
                        ctx.fill_rect(cx - 8.0, cy - 8.0, 16.0, 16.0);
                        ctx.set_fill_style_str("rgba(255,255,255,0.25)");
                        ctx.fill_rect(cx - 8.0, cy - 8.0, 16.0, 3.0);
                        // Skylight on some roofs.
                        if hsh % 3 != 0 && lot.w > 46.0 && lot.h > 46.0 {
                            ctx.set_fill_style_str("rgba(146,201,235,0.75)");
                            let sx = cx + (((hsh / 3) % 5) as f64 - 2.0) * 9.0;
                            let sy = cy + (((hsh / 7) % 5) as f64 - 2.0) * 9.0;
                            ctx.fill_rect(sx - 7.0, sy - 7.0, 14.0, 14.0);
                            ctx.set_stroke_style_str("rgba(255,255,255,0.5)");
                            ctx.set_line_width(1.5);
                            ctx.stroke_rect(sx - 7.0, sy - 7.0, 14.0, 14.0);
                        }
                        // Water tower on some roofs.
                        if hsh % 5 == 0 && lot.w > 56.0 && lot.h > 56.0 {
                            let wx = lot.x + lot.w - 18.0;
                            let wy = lot.y + 18.0;
                            ctx.set_fill_style_str("rgba(0,0,0,0.18)");
                            ctx.begin_path();
                            ctx.arc(wx + 2.5, wy + 3.0, 8.0, 0.0, std::f64::consts::TAU);
                            ctx.fill();
                            ctx.set_fill_style_str("#6f4a30");
                            ctx.begin_path();
                            ctx.arc(wx, wy, 8.0, 0.0, std::f64::consts::TAU);
                            ctx.fill();
                            ctx.set_fill_style_str("#8a5f42");
                            ctx.begin_path();
                            ctx.arc(wx, wy, 5.5, 0.0, std::f64::consts::TAU);
                            ctx.fill();
                            ctx.set_fill_style_str("rgba(255,255,255,0.25)");
                            ctx.begin_path();
                            ctx.arc(wx - 1.5, wy - 1.5, 3.0, 0.0, std::f64::consts::TAU);
                            ctx.fill();
                        }
                        // Varied rooftop props so no two roofs feel the same.
                        match hsh % 7 {
                            1 if lot.w > 64.0 && lot.h > 64.0 => {
                                // Helipad: painted H in a ring.
                                let hx = lot.x + lot.w * 0.30;
                                let hy = lot.y + lot.h * 0.32;
                                ctx.set_stroke_style_str("rgba(220,228,235,0.7)");
                                ctx.set_line_width(2.5);
                                ctx.begin_path();
                                ctx.arc(hx, hy, 16.0, 0.0, std::f64::consts::TAU);
                                ctx.stroke();
                                ctx.set_fill_style_str("rgba(220,228,235,0.8)");
                                ctx.fill_rect(hx - 8.0, hy - 7.0, 3.0, 14.0);
                                ctx.fill_rect(hx + 5.0, hy - 7.0, 3.0, 14.0);
                                ctx.fill_rect(hx - 8.0, hy - 1.5, 16.0, 3.0);
                            }
                            2 if lot.w > 50.0 => {
                                // Rooftop garden: a green patch with shrubs.
                                let gx = lot.x + 10.0;
                                let gy = lot.y + lot.h - 34.0;
                                ctx.set_fill_style_str("rgba(70,140,90,0.85)");
                                fill_round(ctx, gx, gy, lot.w - 20.0, 24.0, 4.0);
                                ctx.set_fill_style_str("#3f7d43");
                                for q in 0..(((lot.w - 36.0) / 12.0) as i32).max(2) {
                                    let tx = gx + 8.0 + q as f64 * 12.0;
                                    ctx.begin_path();
                                    ctx.arc(tx, gy + 12.0, 4.5, 0.0, std::f64::consts::TAU);
                                    ctx.fill();
                                }
                            }
                            3 | 4 if lot.w > 44.0 => {
                                // Antenna mast with a blinking red beacon.
                                let ax = lot.x + lot.w * 0.78;
                                let ay = lot.y + 12.0;
                                ctx.set_stroke_style_str("rgba(190,198,206,0.9)");
                                ctx.set_line_width(2.0);
                                ctx.begin_path();
                                ctx.move_to(ax, ay);
                                ctx.line_to(ax + 10.0, ay - 14.0);
                                ctx.stroke();
                                ctx.set_fill_style_str(
                                    if (s.time * 1.6).fract() < 0.5 {
                                        "rgba(255,80,70,0.95)"
                                    } else {
                                        "rgba(255,80,70,0.25)"
                                    },
                                );
                                ctx.begin_path();
                                ctx.arc(ax + 10.0, ay - 14.0, 2.2, 0.0, std::f64::consts::TAU);
                                ctx.fill();
                            }
                            5 => {
                                // Parking lot: paint the slab, add bay lines.
                                if lot.w > 44.0 && lot.h > 44.0 {
                                    let px0 = lot.x + 6.0;
                                    let py0 = lot.y + 6.0;
                                    let pw = (lot.w - 12.0).max(30.0);
                                    let ph = (lot.h - 12.0).max(30.0);
                                    ctx.set_fill_style_str("rgba(58,62,69,0.85)");
                                    fill_round(ctx, px0, py0, pw, ph, 3.0);
                                    ctx.set_stroke_style_str("rgba(235,238,240,0.4)");
                                    ctx.set_line_width(1.4);
                                    let mut yy = py0 + 4.0;
                                    while yy < py0 + ph - 4.0 {
                                        ctx.begin_path();
                                        ctx.move_to(px0 + 4.0, yy);
                                        ctx.line_to(px0 + pw - 4.0, yy);
                                        ctx.stroke();
                                        yy += 12.0;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                crate::city::BlockKind::Park => {
                    // Bright lawn with subtle mowing bands.
                    ctx.set_fill_style_str("#43a25c");
                    ctx.fill_rect(bx, by, BLOCK, BLOCK);
                    ctx.set_fill_style_str("rgba(255,255,255,0.05)");
                    for q in (0..4).step_by(2) {
                        ctx.fill_rect(bx, by + (q as f64) * BLOCK / 4.0, BLOCK, BLOCK / 4.0);
                    }
                    // Gravel cross-paths through the park.
                    let (pcx, pcy) = (bx + BLOCK / 2.0, by + BLOCK / 2.0);
                    ctx.set_fill_style_str("rgba(226,207,163,0.9)");
                    ctx.fill_rect(bx + 8.0, pcy - 8.0, BLOCK - 16.0, 16.0);
                    ctx.fill_rect(pcx - 8.0, by + 8.0, 16.0, BLOCK - 16.0);
                    if let Some(park) = b.park {
                        for (tx, ty, r) in park.trees {
                            // Cast shadow, trunk, two-tone canopy, sunlit glint.
                            ctx.set_fill_style_str("rgba(0,0,0,0.18)");
                            ctx.begin_path();
                            ctx.ellipse(tx + 4.0, ty + 5.0, r, r * 0.8, 0.0, 0.0, std::f64::consts::TAU);
                            ctx.fill();
                            ctx.set_fill_style_str("#5b4632");
                            ctx.fill_rect(tx - 2.0, ty - 2.0, 4.0, 4.0);
                            ctx.set_fill_style_str("#2d6a3f");
                            ctx.begin_path();
                            ctx.arc(tx, ty + 3.0, r, 0.0, std::f64::consts::TAU);
                            ctx.fill();
                            ctx.set_fill_style_str("#4caf6d");
                            ctx.begin_path();
                            ctx.arc(tx, ty, r, 0.0, std::f64::consts::TAU);
                            ctx.fill();
                            ctx.set_fill_style_str("rgba(255,255,255,0.20)");
                            ctx.begin_path();
                            ctx.arc(tx - r * 0.3, ty - r * 0.35, r * 0.45, 0.0, std::f64::consts::TAU);
                            ctx.fill();
                        }
                    }
                }
            }
        }
    }
}

fn draw_mission_marker(ctx: &CanvasRenderingContext2d, s: &GameState) {
    if let Some((mx, my)) = s.mission.current_marker() {
        let delivering = matches!(s.mission.phase, crate::mission::MissionPhase::ToDeliver);
        let pulse = 1.0 + 0.15 * (s.time * 3.0).sin();
        let (color, glow, glowc) = if delivering {
            ("#4ce06a", "rgba(76,224,106,", 0x4ce06a)
        } else {
            ("#ffd93b", "rgba(255,217,59,", 0xffd93b)
        };
        // Soft glow rings, outer to inner.
        for (rr, aa) in [(58.0, 0.10), (48.0, 0.20)] {
            ctx.set_fill_style_str(&format!("{glow}{aa})"));
            ctx.begin_path();
            ctx.arc(mx, my, rr * pulse, 0.0, std::f64::consts::TAU);
            ctx.fill();
        }
        ctx.set_stroke_style_str(color);
        ctx.set_line_width(5.0);
        ctx.begin_path();
        ctx.arc(mx, my, 42.0 * pulse, 0.0, std::f64::consts::TAU);
        ctx.stroke();
        ctx.set_fill_style_str(color);
        ctx.set_global_alpha(0.25);
        ctx.begin_path();
        ctx.arc(mx, my, 42.0 * pulse, 0.0, std::f64::consts::TAU);
        ctx.fill();
        ctx.set_global_alpha(1.0);
        // Rotating dashed outer ring (a subtle radar sweep around the base).
        ctx.set_stroke_style_str(&rgba(glowc, 0.5));
        ctx.set_line_width(2.0);
        let dashes = js_sys::Array::of2(&JsValue::from_f64(10.0), &JsValue::from_f64(14.0));
        ctx.set_line_dash(&dashes);
        let none = js_sys::Array::new();
        let off = (s.time * 30.0) % 24.0;
        ctx.set_line_dash_offset(-off);
        ctx.begin_path();
        ctx.arc(mx, my, 54.0 * pulse, 0.0, std::f64::consts::TAU);
        ctx.stroke();
        ctx.set_line_dash(&none);
        ctx.set_line_dash_offset(0.0);
        // Pulsing center dot.
        ctx.begin_path();
        ctx.arc(mx, my, 6.0 + 2.0 * (s.time * 4.0).sin(), 0.0, std::f64::consts::TAU);
        ctx.fill();
        // Bobbing arrow pointer above.
        let bob = 4.0 * (s.time * 3.0).sin();
        ctx.begin_path();
        ctx.move_to(mx, my - 66.0 + bob);
        ctx.line_to(mx - 12.0, my - 86.0 + bob);
        ctx.line_to(mx + 12.0, my - 86.0 + bob);
        ctx.close_path();
        ctx.fill();
        ctx.set_stroke_style_str("rgba(0,0,0,0.35)");
        ctx.set_line_width(2.0);
        ctx.stroke();
    }
}

/// Top-down airplane: fuselage, wings, tail, painted stripe and a spinning
/// propeller. When airborne it scales up slightly and casts a soft shadow on
/// the street below.
fn draw_plane(ctx: &CanvasRenderingContext2d, c: &crate::car::Car, time: f64) {
    if c.z > 2.0 {
        // Soft two-lobe shadow (body + wings) that thins with altitude.
        let a = (0.20 - c.z * 0.00008).max(0.06);
        ctx.set_fill_style_str(&rgba(0x000000, a));
        ctx.save();
        ctx.translate(c.x, c.y);
        ctx.rotate(c.heading);
        ctx.begin_path();
        ctx.ellipse(0.0, 0.0, 34.0 * k_of(c.z), 26.0 * k_of(c.z), 0.0, 0.0, std::f64::consts::TAU);
        ctx.fill();
        ctx.fill_rect(-8.0, -52.0 * k_of(c.z), 22.0, 104.0 * k_of(c.z));
        ctx.restore();
    }
    ctx.save();
    ctx.translate(c.x, c.y);
    ctx.rotate(c.heading);
    let k = 1.0 + c.z * 0.0006; // grows a touch with altitude
    ctx.scale(k, k);
    // Wings (white with a blue leading-edge accent and a soft sheen).
    let wg = ctx.create_linear_gradient(0.0, -52.0, 0.0, 52.0);
    let _ = wg.add_color_stop(0.0, "#eef2f6");
    let _ = wg.add_color_stop(0.5, "#dde4ea");
    let _ = wg.add_color_stop(1.0, "#eef2f6");
    ctx.set_fill_style(&JsValue::from(wg));
    ctx.fill_rect(-8.0, -52.0, 22.0, 104.0);
    ctx.set_fill_style_str("#2f6fd0");
    ctx.fill_rect(-8.0, -52.0, 6.0, 104.0);
    // Fuselage + nose with a top-to-bottom sheen gradient.
    let fg = ctx.create_linear_gradient(0.0, -9.0, 0.0, 9.0);
    let _ = fg.add_color_stop(0.0, "#ffffff");
    let _ = fg.add_color_stop(0.55, "#f4f6f9");
    let _ = fg.add_color_stop(1.0, "#c3ccd6");
    ctx.set_fill_style(&JsValue::from(fg));
    ctx.fill_rect(-32.0, -9.0, 58.0, 18.0);
    ctx.set_fill_style_str("#c9d2dc");
    ctx.fill_rect(24.0, -7.0, 12.0, 14.0);
    // Blue livery stripe down the fuselage.
    ctx.set_fill_style_str("#2f6fd0");
    ctx.fill_rect(-30.0, -2.0, 54.0, 4.0);
    // Tail wings + red vertical fin.
    ctx.set_fill_style_str("#e8edf3");
    ctx.fill_rect(-32.0, -20.0, 10.0, 40.0);
    ctx.set_fill_style_str("#d0453a");
    ctx.fill_rect(-32.0, -4.0, 10.0, 8.0);
    // Cockpit glass with a glint band.
    let cg = ctx.create_linear_gradient(0.0, -5.0, 0.0, 5.0);
    let _ = cg.add_color_stop(0.0, "#35567e");
    let _ = cg.add_color_stop(0.4, "#1e3a5f");
    let _ = cg.add_color_stop(1.0, "#14263e");
    ctx.set_fill_style(&JsValue::from(cg));
    ctx.fill_rect(2.0, -5.0, 14.0, 10.0);
    ctx.set_fill_style_str("rgba(255,255,255,0.35)");
    ctx.fill_rect(2.0, -5.0, 14.0, 3.0);
    // Spinning propeller: faint blur disc + a fast-moving blade.
    let pa = time * 46.0;
    ctx.set_fill_style_str("rgba(120,132,146,0.14)");
    ctx.begin_path();
    ctx.arc(38.0, 0.0, 14.5, 0.0, std::f64::consts::TAU);
    ctx.fill();
    ctx.set_stroke_style_str("rgba(60,70,84,0.55)");
    ctx.set_line_width(2.5);
    ctx.begin_path();
    ctx.move_to(38.0 + pa.cos() * 14.0, pa.sin() * 14.0);
    ctx.line_to(38.0 - pa.cos() * 14.0, -pa.sin() * 14.0);
    ctx.stroke();
    ctx.set_fill_style_str("#3b4552");
    ctx.begin_path();
    ctx.arc(38.0, 0.0, 2.5, 0.0, std::f64::consts::TAU);
    ctx.fill();
    ctx.restore();
}

fn k_of(z: f64) -> f64 {
    1.0 + z * 0.0006
}

fn draw_car(ctx: &CanvasRenderingContext2d, c: &crate::car::Car, police: bool, time: f64) {
    let w = if c.kind == crate::car::CarKind::Player { 44.0 } else { 36.0 };
    let h = if c.kind == crate::car::CarKind::Player { 22.0 } else { 17.0 };
    ctx.save();
    ctx.translate(c.x, c.y);
    ctx.rotate(c.heading);

    // Soft shadow (offset like the sun, plus a tight contact shadow).
    ctx.set_fill_style_str("rgba(0,0,0,0.18)");
    fill_round(ctx, -w / 2.0 + 3.5, -h / 2.0 + 4.5, w, h, 6.0);
    ctx.set_fill_style_str("rgba(0,0,0,0.22)");
    fill_round(ctx, -w / 2.0 + 1.0, -h / 2.0 + 1.5, w, h, 5.0);

    // Wheels peeking out from under the body.
    ctx.set_fill_style_str("#181b20");
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            ctx.fill_rect(sx * (w / 2.0 - 10.0) - 4.0, sy * (h / 2.0 - 1.5) - 1.5, 8.0, 3.0);
        }
    }

    // Body: a front-to-back gradient (sun hits the hood, shade at the rear),
    // with a subtle outline.
    let body = if police { 0xf1f1f1 } else { c.color };
    let bg = ctx.create_linear_gradient(-w / 2.0, 0.0, w / 2.0, 0.0);
    let c0 = rgb(shade_c(body, 0.88));
    let c1 = rgb(shade_c(body, 1.22));
    let c2 = rgb(shade_c(body, 1.02));
    let _ = bg.add_color_stop(0.0, &c0);
    let _ = bg.add_color_stop(0.45, &c1);
    let _ = bg.add_color_stop(1.0, &c2);
    ctx.set_fill_style(&JsValue::from(bg));
    fill_round(ctx, -w / 2.0, -h / 2.0, w, h, 5.0);
    ctx.set_stroke_style_str("rgba(0,0,0,0.35)");
    ctx.set_line_width(1.5);
    ctx.begin_path();
    let _ = ctx.round_rect_with_f64(-w / 2.0, -h / 2.0, w, h, 5.0);
    let _ = ctx.stroke();

    // Hood + roof highlights (sun from above).
    ctx.set_fill_style_str(&shade(body, 1.15));
    fill_round(ctx, w * 0.18, -h / 2.0 + 2.5, w * 0.27, h - 5.0, 3.0);
    ctx.set_fill_style_str(&shade(body, 1.22));
    fill_round(ctx, -w * 0.18, -h / 2.0 + 2.5, w * 0.38, h - 5.0, 3.0);

    // Windshield / rear window with a glass sheen gradient.
    let gg = ctx.create_linear_gradient(0.0, -h / 2.0, 0.0, h / 2.0);
    let _ = gg.add_color_stop(0.0, "rgba(38,62,94,0.92)");
    let _ = gg.add_color_stop(1.0, "rgba(14,26,44,0.92)");
    ctx.set_fill_style(&JsValue::from(gg));
    fill_round(ctx, w * 0.05, -h / 2.0 + 2.5, w * 0.22, h - 5.0, 3.0);
    fill_round(ctx, -w * 0.32, -h / 2.0 + 3.0, w * 0.16, h - 6.0, 3.0);
    ctx.set_fill_style_str("rgba(255,255,255,0.22)");
    ctx.fill_rect(w * 0.05 + 2.0, -h / 2.0 + 3.5, w * 0.22 - 4.0, 2.0);
    // A sun-glint streak sweeping across the paint.
    let glint = (0.06 + 0.05 * (time * 0.35 + c.x * 0.001).sin()).max(0.04);
    ctx.set_fill_style_str(&rgba(0xffffff, glint));
    ctx.fill_rect(-w * 0.15, -h / 2.0 + 1.5, w * 0.4, 2.2);

    // Police light bar (flashing, with a pulsing glow wash).
    if police {
        let phase = (time * 8.0).floor() as u32 % 2;
        let col: u32 = if phase == 0 { 0xff3b30 } else { 0x3478f6 };
        // Glow halo radiating from the bar.
        let halo = ctx.create_radial_gradient(0.0, 0.0, 4.0, 0.0, 0.0, 34.0).unwrap();
        let _ = halo.add_color_stop(0.0, &rgba(col, 0.35));
        let _ = halo.add_color_stop(1.0, &rgba(col, 0.0));
        ctx.set_fill_style(&JsValue::from(halo));
        ctx.begin_path();
        ctx.arc(0.0, 0.0, 34.0, 0.0, std::f64::consts::TAU);
        ctx.fill();
        ctx.set_fill_style_str(&rgb(col));
        ctx.fill_rect(-3.0, -h / 2.0 + 2.0, 6.0, h - 4.0);
        // Dark door stripe.
        ctx.set_fill_style_str("rgba(20,28,40,0.5)");
        ctx.fill_rect(-w * 0.1, -h / 2.0 + 1.5, w * 0.2, 2.0);
        ctx.fill_rect(-w * 0.1, h / 2.0 - 3.5, w * 0.2, 2.0);
    }
    // Headlights.
    ctx.set_fill_style_str("rgba(255,244,180,0.95)");
    ctx.fill_rect(w / 2.0 - 3.0, -h / 2.0 + 2.0, 3.0, 4.0);
    ctx.fill_rect(w / 2.0 - 3.0, h / 2.0 - 6.0, 3.0, 4.0);
    // Taillights.
    ctx.set_fill_style_str("#e2483d");
    ctx.fill_rect(-w / 2.0 + 0.5, -h / 2.0 + 2.0, 3.0, 4.0);
    ctx.fill_rect(-w / 2.0 + 0.5, h / 2.0 - 6.0, 3.0, 4.0);
    ctx.restore();
}

fn fill_round(ctx: &CanvasRenderingContext2d, x: f64, y: f64, w: f64, h: f64, r: f64) {
    ctx.begin_path();
    let _ = ctx.round_rect_with_f64(x, y, w, h, r);
    let _ = ctx.fill();
}

fn draw_peds(ctx: &CanvasRenderingContext2d, s: &GameState) {
    for p in &s.peds {
        let dead = matches!(p.state, crate::ped::PedState::Dead(_));
        if dead {
            ctx.set_global_alpha(0.55);
        }
        // Each ped gets its own walk phase from its position.
        draw_person(ctx, p.x, p.y, p.heading, p.color, s.time + p.x * 0.13 + p.y * 0.07);
        ctx.set_global_alpha(1.0);
    }
}

/// Top-down elephant: grey body, head, ears, tusks and a swaying trunk.
fn draw_elephant(ctx: &CanvasRenderingContext2d, e: &crate::wildlife::Elephant, time: f64) {
    let s = e.scale;
    ctx.save();
    ctx.translate(e.x, e.y);
    ctx.rotate(e.heading);
    // Legs (stubs poking out of the body).
    ctx.set_fill_style_str("#7c8188");
    for (lx, ly) in [(-18.0, -9.5), (-18.0, 9.5), (18.0, -9.5), (18.0, 9.5)] {
        ctx.begin_path();
        ctx.ellipse(lx * s, ly * s, 6.0 * s, 5.0 * s, 0.0, 0.0, std::f64::consts::TAU);
        ctx.fill();
    }
    // Body with a sunlit back.
    ctx.set_fill_style_str("#8f949a");
    ctx.begin_path();
    ctx.ellipse(-2.0 * s, 0.0, 24.0 * s, 13.5 * s, 0.0, 0.0, std::f64::consts::TAU);
    ctx.fill();
    ctx.set_fill_style_str("rgba(255,255,255,0.12)");
    ctx.begin_path();
    ctx.ellipse(-4.0 * s, -2.5 * s, 17.0 * s, 8.0 * s, 0.0, 0.0, std::f64::consts::TAU);
    ctx.fill();
    // Head.
    ctx.set_fill_style_str("#8f949a");
    ctx.begin_path();
    ctx.ellipse(21.0 * s, 0.0, 9.0 * s, 8.0 * s, 0.0, 0.0, std::f64::consts::TAU);
    ctx.fill();
    // Ears with pinker inner skin.
    for sy in [-1.0, 1.0] {
        ctx.set_fill_style_str("#787d84");
        ctx.begin_path();
        ctx.ellipse(17.0 * s, sy * 9.0 * s, 7.0 * s, 4.5 * s, 0.0, 0.0, std::f64::consts::TAU);
        ctx.fill();
        ctx.set_fill_style_str("rgba(214,178,186,0.55)");
        ctx.begin_path();
        ctx.ellipse(16.5 * s, sy * 8.5 * s, 4.0 * s, 2.4 * s, 0.0, 0.0, std::f64::consts::TAU);
        ctx.fill();
    }
    // Tusks.
    ctx.set_stroke_style_str("#f2ecdc");
    ctx.set_line_width(1.8 * s);
    ctx.set_line_cap("round");
    for sy in [-1.0, 1.0] {
        ctx.begin_path();
        ctx.move_to(27.0 * s, sy * 4.0 * s);
        ctx.quadratic_curve_to(31.0 * s, sy * 6.0 * s, 33.0 * s, sy * 6.5 * s);
        ctx.stroke();
    }
    // Skin wrinkle arcs across the body (faint, deterministic).
    ctx.set_stroke_style_str("rgba(0,0,0,0.08)");
    ctx.set_line_width(1.2 * s);
    for (qx, rw) in [(-14.0, 10.5), (-4.0, 11.5), (6.0, 10.5)] {
        ctx.begin_path();
        ctx.ellipse(qx * s, 0.0, rw * s, 12.0 * s, 0.0, -1.2, 1.2);
        ctx.stroke();
    }
    // Eyes.
    ctx.set_fill_style_str("#23252b");
    for sy in [-1.0, 1.0] {
        ctx.begin_path();
        ctx.arc(25.5 * s, sy * 4.5 * s, 1.1 * s, 0.0, std::f64::consts::TAU);
        ctx.fill();
    }
    // Trunk (sways gently).
    let sway = (time * 1.1 + e.seed).sin() * 3.0 * s;
    ctx.set_stroke_style_str("#a6a9ab");
    ctx.set_line_width(3.4 * s);
    ctx.begin_path();
    ctx.move_to(28.0 * s, 0.0);
    ctx.quadratic_curve_to(32.0 * s, sway * 0.6, 35.0 * s, sway);
    ctx.stroke();
    ctx.restore();
}

/// Top-down bird: a tiny body with flapping wings and a faint ground shadow.
fn draw_bird(ctx: &CanvasRenderingContext2d, b: &crate::wildlife::Bird) {
    // Faint shadow on the ground.
    ctx.set_fill_style_str("rgba(0,0,0,0.10)");
    ctx.begin_path();
    ctx.ellipse(b.x, b.y, 4.5, 2.8, 0.0, 0.0, std::f64::consts::TAU);
    ctx.fill();

    // Higher birds look smaller; wings foreshorten with the beat.
    let k = (260.0 / (120.0 + b.z)).clamp(0.45, 1.3);
    let flap = if b.glide > 0.0 { 0.15 } else { b.flap.sin().abs() };
    let half = b.span * 0.30 * k;
    let ext = (0.35 + 0.65 * flap) * half;
    ctx.save();
    ctx.translate(b.x, b.y);
    ctx.rotate(b.heading);
    // Wings with a lighter leading edge.
    ctx.set_fill_style_str(&shade(b.color, 0.85));
    for sy in [-1.0, 1.0] {
        ctx.set_global_alpha(0.92);
        ctx.begin_path();
        ctx.ellipse(-0.5 * k, sy * ext * 0.55, 2.6 * k, ext * 0.55, 0.0, 0.0, std::f64::consts::TAU);
        ctx.fill();
    }
    ctx.set_global_alpha(1.0);
    // Body + head.
    ctx.set_fill_style_str(&rgb(b.color));
    ctx.begin_path();
    ctx.ellipse(0.0, 0.0, 3.2 * k, 1.4 * k, 0.0, 0.0, std::f64::consts::TAU);
    ctx.fill();
    ctx.begin_path();
    ctx.arc(3.0 * k, 0.0, 1.1 * k, 0.0, std::f64::consts::TAU);
    ctx.fill();
    // Beak.
    ctx.set_fill_style_str(&rgb(b.beak));
    ctx.begin_path();
    ctx.arc(4.3 * k, 0.0, 0.7 * k, 0.0, std::f64::consts::TAU);
    ctx.fill();
    ctx.restore();
}

/// Top-down dragon: a big bronze body with flapping swept wings, a long
/// tail and a faint shadow on the streets far below.
fn draw_dragon(ctx: &CanvasRenderingContext2d, d: &crate::wildlife::Dragon) {
    // Faint shadow on the ground (it is high up, so it's small and soft).
    let shk = (260.0 / (150.0 + d.z)).clamp(0.3, 1.0);
    ctx.set_fill_style_str("rgba(0,0,0,0.10)");
    ctx.begin_path();
    ctx.ellipse(d.x, d.y, 24.0 * shk, 10.0 * shk, 0.0, 0.0, std::f64::consts::TAU);
    ctx.fill();

    let bronze = 0x8a5c22;
    let k = (300.0 / (150.0 + d.z)).clamp(0.35, 1.15);
    let flap = 0.5 + 0.5 * d.flap.sin(); // 0..1 wing cycle
    let ext = (0.35 + 0.65 * flap) * 22.0 * k;

    ctx.save();
    ctx.translate(d.x, d.y);
    ctx.rotate(d.heading);
    // Tail, swept back.
    ctx.set_fill_style_str(&shade(bronze, 0.78));
    ctx.begin_path();
    ctx.move_to(-10.0 * k, -2.0 * k);
    ctx.quadratic_curve_to(-20.0 * k, 0.0, -30.0 * k, 1.5 * k);
    ctx.quadratic_curve_to(-22.0 * k, 2.5 * k, -11.0 * k, 2.5 * k);
    ctx.close_path();
    ctx.fill();
    // Wings, sweeping with the beat.
    for sy in [-1.0, 1.0] {
        ctx.set_fill_style_str(&shade(bronze, 0.92));
        ctx.begin_path();
        ctx.move_to(4.0 * k, sy * 1.5 * k);
        ctx.quadratic_curve_to(-2.0 * k, sy * ext * 0.75, -8.0 * k, sy * ext);
        ctx.quadratic_curve_to(-6.0 * k, sy * ext * 0.35, -9.0 * k, sy * 1.8 * k);
        ctx.close_path();
        ctx.fill();
        // Sunlit leading edge.
        ctx.set_fill_style_str(&shade(bronze, 1.15));
        ctx.begin_path();
        ctx.move_to(4.0 * k, sy * 1.5 * k);
        ctx.quadratic_curve_to(-1.0 * k, sy * ext * 0.7, -8.0 * k, sy * ext);
        ctx.quadratic_curve_to(-4.0 * k, sy * ext * 0.6, 4.0 * k, sy * 1.5 * k);
        ctx.close_path();
        ctx.fill();
    }
    // Body + head + neck.
    ctx.set_fill_style_str(&rgb(bronze));
    ctx.begin_path();
    ctx.ellipse(-1.0 * k, 0.0, 12.0 * k, 4.5 * k, 0.0, 0.0, std::f64::consts::TAU);
    ctx.fill();
    ctx.begin_path();
    ctx.ellipse(11.0 * k, 0.0, 5.0 * k, 3.0 * k, 0.0, 0.0, std::f64::consts::TAU);
    ctx.fill();
    ctx.begin_path();
    ctx.arc(15.5 * k, 0.0, 2.2 * k, 0.0, std::f64::consts::TAU);
    ctx.fill();
    // Horn glint.
    ctx.set_fill_style_str(&shade(bronze, 1.5));
    ctx.begin_path();
    ctx.arc(15.5 * k, 0.0, 0.8 * k, 0.0, std::f64::consts::TAU);
    ctx.fill();
    ctx.restore();
}

fn draw_person(
    ctx: &CanvasRenderingContext2d,
    x: f64,
    y: f64,
    heading: f64,
    color: u32,
    phase: f64,
) {
    let (dx, dy) = (heading.cos(), heading.sin());
    // Arms swing along the heading while the figure walks.
    let swing = phase.sin() * 1.8;
    // Shadow.
    ctx.set_fill_style_str("rgba(0,0,0,0.25)");
    ctx.begin_path();
    ctx.ellipse(x + 1.5, y + 2.0, 6.5, 5.5, 0.0, 0.0, std::f64::consts::TAU);
    ctx.fill();
    // Arms (small nubs perpendicular to the heading, swinging with the walk).
    ctx.set_fill_style_str(&shade(color, 0.8));
    for sy in [-1.0, 1.0] {
        ctx.begin_path();
        ctx.arc(
            x + dx * swing * sy - dy * sy * 4.5,
            y + dy * swing * sy + dx * sy * 4.5,
            2.2,
            0.0,
            std::f64::consts::TAU,
        );
        ctx.fill();
    }
    // Body with a subtle outline.
    ctx.set_fill_style_str(&rgb(color));
    ctx.begin_path();
    ctx.arc(x, y, 6.0, 0.0, std::f64::consts::TAU);
    ctx.fill();
    ctx.set_stroke_style_str("rgba(0,0,0,0.35)");
    ctx.set_line_width(1.5);
    ctx.begin_path();
    ctx.arc(x, y, 6.0, 0.0, std::f64::consts::TAU);
    ctx.stroke();
    // Head (offset toward facing direction) with a hair ring.
    ctx.set_fill_style_str("#5a4634");
    ctx.begin_path();
    ctx.arc(x + dx * 3.0, y + dy * 3.0, 4.0, 0.0, std::f64::consts::TAU);
    ctx.fill();
    ctx.set_fill_style_str("#e8b87f");
    ctx.begin_path();
    ctx.arc(x + dx * 3.3, y + dy * 3.3, 3.2, 0.0, std::f64::consts::TAU);
    ctx.fill();
}

/// Draw the particle pool in world space (top-down).
fn draw_fx(ctx: &CanvasRenderingContext2d, fx: &crate::fx::Fx, view: &View) {
    for p in &fx.particles {
        if !view.contains(p.x - p.size, p.y - p.size, p.size * 2.0, p.size * 2.0) {
            continue;
        }
        let f = crate::fx::Fx::fade(p);
        let a = (p.alpha * f).clamp(0.0, 1.0);
        if a <= 0.005 {
            continue;
        }
        // Altitude lifts a particle slightly toward the "north-west" sun.
        let (px, py) = (p.x - p.z * 0.10, p.y - p.z * 0.14);
        match p.kind {
            crate::fx::PKind::Smoke => {
                ctx.set_fill_style_str(&rgba(p.color, a * 0.85));
                ctx.begin_path();
                ctx.arc(px, py, p.size, 0.0, std::f64::consts::TAU);
                ctx.fill();
            }
            crate::fx::PKind::Spark => {
                // A short streak along the velocity.
                let vl = (p.vx * p.vx + p.vy * p.vy).sqrt().max(1e-3);
                let ln = (4.0 * p.size / 2.5).min(14.0);
                ctx.set_stroke_style_str(&rgba(p.color, a));
                ctx.set_line_width(1.6);
                ctx.begin_path();
                ctx.move_to(px, py);
                ctx.line_to(px - p.vx / vl * ln, py - p.vy / vl * ln);
                ctx.stroke();
            }
            crate::fx::PKind::Glitter => {
                // Additive four-point sparkle.
                let r = p.size * (1.0 + 0.4 * (p.life * 9.0).sin());
                ctx.set_stroke_style_str(&rgba(p.color, a));
                ctx.set_line_width(1.4);
                ctx.begin_path();
                ctx.move_to(px - r, py);
                ctx.line_to(px + r, py);
                ctx.move_to(px, py - r);
                ctx.line_to(px, py + r);
                ctx.stroke();
                ctx.set_fill_style_str(&rgba(0xffffff, a));
                ctx.begin_path();
                ctx.arc(px, py, r * 0.35, 0.0, std::f64::consts::TAU);
                ctx.fill();
            }
            crate::fx::PKind::Debris => {
                ctx.set_fill_style_str(&rgba(p.color, a));
                ctx.fill_rect(px - p.size * 0.5, py - p.size * 0.5, p.size, p.size);
            }
        }
    }
}

/// Subtle screen-space vignette to frame the world (screen-space transform).
fn draw_vignette(ctx: &CanvasRenderingContext2d, w: f64, h: f64) {
    let g = ctx
        .create_radial_gradient(w / 2.0, h / 2.0, (w * h).sqrt() * 0.42, w / 2.0, h / 2.0, (w * h).sqrt() * 0.72)
        .unwrap();
    let _ = g.add_color_stop(0.0, "rgba(0,0,10,0.0)");
    let _ = g.add_color_stop(1.0, "rgba(0,0,10,0.22)");
    ctx.set_fill_style(&JsValue::from(g));
    ctx.fill_rect(0.0, 0.0, w, h);
}

/// Shared HUD (money, stars, speed, mission line, banner, minimap).
pub fn draw_hud(ctx: &CanvasRenderingContext2d, s: &GameState, w: f64, h: f64) {
    // Money (top-left).
    ctx.set_font(FONT);
    ctx.set_text_align("left");
    ctx.set_fill_style_str("rgba(0,0,0,0.45)");
    fill_round(ctx, 12.0, 12.0, 150.0, 34.0, 8.0);
    // Little coin badge.
    ctx.set_fill_style_str("#2e7d32");
    ctx.begin_path();
    ctx.arc(30.0, 29.0, 10.0, 0.0, std::f64::consts::TAU);
    ctx.fill();
    ctx.set_fill_style_str("#7CFC00");
    ctx.set_font("bold 13px 'Segoe UI', system-ui, sans-serif");
    ctx.set_text_align("center");
    ctx.fill_text("$", 30.0, 34.0);
    ctx.set_font(FONT);
    ctx.set_text_align("left");
    ctx.set_fill_style_str("#7CFC00");
    ctx.fill_text(&format!("${}", s.money), 46.0, 36.0);

    // Wanted stars (top-right) with a soft glow on the lit ones.
    let st = s.stars();
    for i in 0..5u32 {
        let x = w - 40.0 - (4 - i) as f64 * 30.0;
        if i < st {
            draw_star(ctx, x, 30.0, 17.0, "rgba(255,214,10,0.25)");
            draw_star(ctx, x, 30.0, 12.0, "#ffd60a");
        } else {
            draw_star(ctx, x, 30.0, 12.0, "rgba(255,255,255,0.15)");
        }
    }

    // Auto-land status (bottom-left, above the altitude box).
    if !s.on_foot && s.in_plane && s.landing {
        let (tx, ty) = s.landing_target;
        let (px, py) = s.player_pos();
        let d = ((tx - px).powi(2) + (ty - py).powi(2)).sqrt();
        let grounded = s.plane.z < 1.0;
        ctx.set_fill_style_str("rgba(0,0,0,0.45)");
        ctx.fill_rect(14.0, h - 112.0, 240.0, 22.0);
        ctx.set_fill_style_str("#8af09a");
        let line = if grounded {
            "AUTO-LAND — SETTING DOWN".to_string()
        } else {
            format!("AUTO-LAND → {}m", (d * 0.5).round() as u32)
        };
        ctx.fill_text(&line, 20.0, h - 96.0);
    }
    // Altitude + cruise throttle (bottom-left) when in the plane.
    if !s.on_foot && s.in_plane && s.plane.z > 5.0 {
        let alt = (s.plane.z * 2.0).round() as u32;
        let thr = (s.mouse_throttle * 100.0).round() as u32;
        ctx.set_fill_style_str("rgba(0,0,0,0.45)");
        ctx.fill_rect(14.0, h - 84.0, 190.0, 22.0);
        ctx.set_fill_style_str("#9ad1ff");
        ctx.fill_text(&format!("ALT {}m · THR {}%", alt, thr), 20.0, h - 68.0);
    }
    // Altitude + cruise throttle (bottom-left) when riding the dragon.
    if s.in_dragon && s.wildlife.dragon.z > 5.0 {
        let d = &s.wildlife.dragon;
        let alt = (d.z * 2.0).round() as u32;
        let thr = (s.mouse_throttle * 100.0).round() as u32;
        ctx.set_fill_style_str("rgba(0,0,0,0.45)");
        ctx.fill_rect(14.0, h - 84.0, 210.0, 22.0);
        ctx.set_fill_style_str("#ffd08a");
        ctx.fill_text(&format!("DRAGON · ALT {}m · THR {}%", alt, thr), 20.0, h - 68.0);
    }
    // Speed (bottom-left) when in a car, with a colored speed bar.
    if s.riding.is_some() {
        ctx.set_fill_style_str("rgba(255,255,255,0.7)");
        ctx.set_font(FONT);
        ctx.fill_text("ON AN ELEPHANT — Z: jump off", 16.0, h - 24.0);
    } else if !s.on_foot {
        let spd = if s.in_dragon { s.wildlife.dragon.speed } else { s.active_vehicle().speed() };
        let kmh = (spd * 0.18).round() as u32;
        ctx.set_fill_style_str("rgba(0,0,0,0.45)");
        fill_round(ctx, 12.0, h - 56.0, 120.0, 40.0, 8.0);
        ctx.set_font("bold 24px 'Segoe UI', system-ui, sans-serif");
        ctx.set_fill_style_str("#ffffff");
        ctx.fill_text(&format!("{} km/h", kmh), 24.0, h - 30.0);
        // Speed bar: green → yellow → red.
        let frac = ((kmh as f64) / 220.0).clamp(0.0, 1.0);
        let hue = 140.0 - 140.0 * frac;
        ctx.set_fill_style_str("rgba(255,255,255,0.18)");
        ctx.fill_rect(20.0, h - 24.0, 104.0, 5.0);
        ctx.set_fill_style_str(&format!("hsl({} 90% 55%)", hue as u32));
        ctx.fill_rect(20.0, h - 24.0, (104.0 * frac).max(2.0), 5.0);
    } else {
        ctx.set_fill_style_str("rgba(255,255,255,0.7)");
        ctx.set_font(FONT);
        ctx.fill_text("ON FOOT — E: enter vehicle · F: plane · D: dragon", 16.0, h - 24.0);
    }

    // Mission timer.
    if let Some((mx, my)) = s.mission.current_marker() {
        let (px, py) = s.player_pos();
        let d = ((mx - px).powi(2) + (my - py).powi(2)).sqrt();
        let d_m = (d * 0.5).round() as u32; // ~meters
        let delivering = matches!(s.mission.phase, crate::mission::MissionPhase::ToDeliver);
        let mut line = if delivering {
            format!("DELIVER  {}m  ({}s)", d_m, s.mission.time_left.ceil() as u32)
        } else {
            format!("PICKUP   {}m", d_m)
        };
        line.push_str("  ↑");
        ctx.set_font(FONT);
        ctx.set_fill_style_str("rgba(0,0,0,0.45)");
        let tw = 150.0;
        fill_round(ctx, w / 2.0 - tw / 2.0, 10.0, tw, 30.0, 8.0);
        ctx.set_fill_style_str(if delivering { "#8df0a5" } else { "#ffe27a" });
        ctx.set_text_align("center");
        ctx.fill_text(&line, w / 2.0, 31.0);
    }

    // Message banner.
    if let Some((msg, _)) = &s.msg {
        ctx.set_font("bold 22px 'Segoe UI', system-ui, sans-serif");
        ctx.set_text_align("center");
        ctx.set_fill_style_str("rgba(0,0,0,0.55)");
        let tw = msg.len() as f64 * 12.0 + 40.0;
        fill_round(ctx, w / 2.0 - tw / 2.0, 54.0, tw, 38.0, 8.0);
        ctx.set_stroke_style_str("rgba(255,214,10,0.6)");
        ctx.set_line_width(2.0);
        ctx.begin_path();
        let _ = ctx.round_rect_with_f64(w / 2.0 - tw / 2.0, 54.0, tw, 38.0, 8.0);
        let _ = ctx.stroke();
        ctx.set_fill_style_str("#ffd60a");
        ctx.fill_text(msg, w / 2.0, 80.0);
    }

    draw_minimap(ctx, s, w, h);
}

fn draw_star(ctx: &CanvasRenderingContext2d, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_fill_style_str(color);
    ctx.begin_path();
    for i in 0..10 {
        let ang = -std::f64::consts::FRAC_PI_2 + i as f64 * (std::f64::consts::PI / 5.0);
        let rad = if i % 2 == 0 { r } else { r * 0.45 };
        let x = cx + ang.cos() * rad;
        let y = cy + ang.sin() * rad;
        if i == 0 {
            ctx.move_to(x, y);
        } else {
            ctx.line_to(x, y);
        }
    }
    ctx.close_path();
    ctx.fill();
}

fn draw_minimap(ctx: &CanvasRenderingContext2d, s: &GameState, w: f64, h: f64) {
    let ms = 168.0;
    let ox = w - ms - 14.0;
    let oy = h - ms - 14.0;
    let k = ms / SIZE;

    ctx.set_fill_style_str("rgba(10,14,18,0.85)");
    fill_round(ctx, ox, oy, ms, ms, 10.0);
    ctx.save();
    // Clip to the map.
    ctx.begin_path();
    ctx.rect(ox, oy, ms, ms);
    ctx.clip();

    // Park blocks show up as green squares.
    for j in 0..N {
        for i in 0..N {
            if s.city.block(i, j).kind == crate::city::BlockKind::Park {
                let bx = i as f64 * CELL + ROAD;
                let by = j as f64 * CELL + ROAD;
                ctx.set_fill_style_str("rgba(90,180,110,0.5)");
                ctx.fill_rect(ox + bx * k, oy + by * k, BLOCK * k, BLOCK * k);
            }
        }
    }

    // Roads.
    ctx.set_stroke_style_str("rgba(255,255,255,0.35)");
    ctx.set_line_width(2.5);
    for i in 0..=N {
        let c = (i as f64 * CELL + ROAD / 2.0) * k;
        ctx.begin_path();
        ctx.move_to(ox + c, oy);
        ctx.line_to(ox + c, oy + ms);
        ctx.stroke();
        ctx.begin_path();
        ctx.move_to(ox, oy + c);
        ctx.line_to(ox + ms, oy + c);
        ctx.stroke();
    }

    let pt = |wx: f64, wy: f64| (ox + wx * k, oy + wy * k);

    // Mission marker (pulsing ring).
    if let Some((mx, my)) = s.mission.current_marker() {
        let (x, y) = pt(mx, my);
        let gold = matches!(s.mission.phase, crate::mission::MissionPhase::ToPickup);
        let col = if gold { "#ffd60a" } else { "#4caf50" };
        let rp = 4.5 + 1.5 * (s.time * 4.0).sin();
        ctx.set_stroke_style_str(col);
        ctx.set_line_width(1.5);
        ctx.begin_path();
        ctx.arc(x, y, rp + 3.0, 0.0, std::f64::consts::TAU);
        ctx.stroke();
        ctx.set_fill_style_str(col);
        ctx.begin_path();
        ctx.arc(x, y, 4.0, 0.0, std::f64::consts::TAU);
        ctx.fill();
    }
    // Traffic as faint dots.
    ctx.set_fill_style_str("rgba(255,255,255,0.30)");
    for t in &s.traffic {
        let (x, y) = pt(t.car.x, t.car.y);
        ctx.fill_rect(x - 1.0, y - 1.0, 2.0, 2.0);
    }
    // Police.
    for p in &s.police {
        let (x, y) = pt(p.x, p.y);
        ctx.set_fill_style_str("#3478f6");
        ctx.begin_path();
        ctx.arc(x, y, 3.0, 0.0, std::f64::consts::TAU);
        ctx.fill();
    }
    // Player (with a small heading tick).
    let (px, py) = s.player_pos();
    let (x, y) = pt(px, py);
    ctx.set_fill_style_str("#ffffff");
    ctx.begin_path();
    ctx.arc(x, y, 4.5, 0.0, std::f64::consts::TAU);
    ctx.fill();
    ctx.set_stroke_style_str("#ffd60a");
    ctx.set_line_width(2.0);
    let hd = if s.on_foot {
        s.foot_heading
    } else {
        s.active_vehicle().heading
    };
    ctx.begin_path();
    ctx.move_to(x, y);
    ctx.line_to(x + hd.cos() * 9.0, y + hd.sin() * 9.0);
    ctx.stroke();
    ctx.restore();

    ctx.set_stroke_style_str("rgba(255,255,255,0.5)");
    ctx.set_line_width(2.0);
    ctx.begin_path();
    let _ = ctx.round_rect_with_f64(ox, oy, ms, ms, 10.0);
    let _ = ctx.stroke();
}

fn draw_busted(ctx: &CanvasRenderingContext2d, w: f64, h: f64) {
    ctx.set_fill_style_str("rgba(0,0,40,0.75)");
    ctx.fill_rect(0.0, 0.0, w, h);
    ctx.set_font("bold 64px 'Segoe UI', system-ui, sans-serif");
    ctx.set_text_align("center");
    // Drop shadow + main text.
    ctx.set_fill_style_str("rgba(60,0,0,0.9)");
    ctx.fill_text("BUSTED", w / 2.0 + 3.0, h / 2.0 + 3.0);
    ctx.set_fill_style_str("#ff3b30");
    ctx.fill_text("BUSTED", w / 2.0, h / 2.0);
    ctx.set_stroke_style_str("rgba(255,255,255,0.7)");
    ctx.set_line_width(1.5);
    ctx.stroke_text("BUSTED", w / 2.0, h / 2.0);
    ctx.set_font("bold 18px 'Segoe UI', system-ui, sans-serif");
    ctx.set_fill_style_str("rgba(255,255,255,0.75)");
    ctx.fill_text("The cops got you — fine paid.", w / 2.0, h / 2.0 + 44.0);
}

use wasm_bindgen::JsValue;
