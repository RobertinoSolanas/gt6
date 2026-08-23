#![allow(unused_must_use)]
//
#![allow(unused_must_use)]
//! Canvas 2D renderer: world (roads, blocks, entities) + HUD (money, stars,
//! speed, minimap, messages).

use web_sys::CanvasRenderingContext2d;

use crate::city::{BLOCK, CELL, N, ROAD, SIZE};
use crate::state::GameState;

const FONT: &str = "bold 16px 'Segoe UI', system-ui, sans-serif";
const FONT_BIG: &str = "bold 42px 'Segoe UI', system-ui, sans-serif";

fn rgb(c: u32) -> String {
    format!("#{:06x}", c & 0xffffff)
}

/// Render the full frame.
pub fn render(ctx: &CanvasRenderingContext2d, s: &GameState, w: f64, h: f64, dpr: f64) {
    // ---- Ground ----
    ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
    ctx.set_fill_style_str("#41704b");
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

    draw_roads(ctx, s);
    draw_blocks(ctx, s);
    draw_mission_marker(ctx, s);
    for t in &s.traffic {
        draw_car(ctx, &t.car, false);
    }
    for p in &s.police {
        draw_car(ctx, p, true);
    }
    // Player: car (if not in it) or person.
    if s.on_foot {
        draw_car(ctx, &s.car, false);
    }
    draw_peds(ctx, s);
    if s.on_foot {
        draw_person(ctx, s.foot_x, s.foot_y, s.foot_heading, 0xffe0b2);
    } else {
        draw_car(ctx, &s.car, false);
    }

    // ---- HUD (screen space) ----
    ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
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
        ctx.set_fill_style_str("#ffffff");
        ctx.set_font(FONT_BIG);
        ctx.set_text_align("center");
        ctx.fill_text("PAUSED — P TO RESUME", w / 2.0, h / 2.0);
    }
}

fn draw_roads(ctx: &CanvasRenderingContext2d, _s: &GameState) {
    // Asphalt.
    ctx.set_fill_style_str("#3b3f46");
    for i in 0..=N {
        let c = i as f64 * CELL;
        ctx.fill_rect(c, 0.0, ROAD, SIZE);
        ctx.fill_rect(0.0, c, SIZE, ROAD);
    }
    // Center lane dashes.
    ctx.set_stroke_style_str("rgba(240,220,90,0.55)");
    ctx.set_line_width(3.0);
    let dashes = js_sys::Array::of2(&JsValue::from_f64(18.0), &JsValue::from_f64(26.0));
    ctx.set_line_dash(&dashes);
    for i in 0..=N {
        let c = i as f64 * CELL + ROAD / 2.0;
        ctx.begin_path();
        ctx.move_to(c, 0.0);
        ctx.line_to(c, SIZE);
        ctx.stroke();
        ctx.begin_path();
        ctx.move_to(0.0, c);
        ctx.line_to(SIZE, c);
        ctx.stroke();
    }
    let none = js_sys::Array::new();
    ctx.set_line_dash(&none);
}

fn draw_blocks(ctx: &CanvasRenderingContext2d, s: &GameState) {
    for j in 0..N {
        for i in 0..N {
            let bx = i as f64 * CELL + ROAD;
            let by = j as f64 * CELL + ROAD;
            let b = s.city.block(i, j);
            match b.kind {
                crate::city::BlockKind::Buildings => {
                    // Sidewalk slab.
                    ctx.set_fill_style_str("#8f9aa3");
                    ctx.fill_rect(bx, by, BLOCK, BLOCK);
                    for lot in b.buildings.iter().flatten() {
                        // Roof.
                        ctx.set_fill_style_str(&rgb(lot.color));
                        ctx.fill_rect(lot.x, lot.y, lot.w, lot.h);
                        // Roof shading/outline.
                        ctx.set_stroke_style_str("rgba(0,0,0,0.35)");
                        ctx.set_line_width(3.0);
                        ctx.stroke_rect(lot.x, lot.y, lot.w, lot.h);
                        // AC unit detail.
                        let (cx, cy) = lot.center();
                        ctx.set_fill_style_str("rgba(0,0,0,0.18)");
                        ctx.fill_rect(cx - 8.0, cy - 8.0, 16.0, 16.0);
                    }
                }
                crate::city::BlockKind::Park => {
                    ctx.set_fill_style_str("#3e8e52");
                    ctx.fill_rect(bx, by, BLOCK, BLOCK);
                    if let Some(park) = b.park {
                        for (tx, ty, r) in park.trees {
                            ctx.set_fill_style_str("#2d6a3f");
                            ctx.begin_path();
                            ctx.arc(tx, ty + 3.0, r, 0.0, std::f64::consts::TAU);
                            ctx.fill();
                            ctx.set_fill_style_str("#4caf6d");
                            ctx.begin_path();
                            ctx.arc(tx, ty, r, 0.0, std::f64::consts::TAU);
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
        let color = if delivering { "#4caf50" } else { "#ffd60a" };
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
        // Arrow pointer above.
        ctx.begin_path();
        ctx.move_to(mx, my - 70.0);
        ctx.line_to(mx - 12.0, my - 90.0);
        ctx.line_to(mx + 12.0, my - 90.0);
        ctx.close_path();
        ctx.fill();
    }
}

fn draw_car(ctx: &CanvasRenderingContext2d, c: &crate::car::Car, police: bool) {
    let w = if c.kind == crate::car::CarKind::Player { 44.0 } else { 36.0 };
    let h = if c.kind == crate::car::CarKind::Player { 22.0 } else { 17.0 };
    ctx.save();
    ctx.translate(c.x, c.y);
    ctx.rotate(c.heading);

    // Shadow.
    ctx.set_fill_style_str("rgba(0,0,0,0.3)");
    fill_round(ctx, -w / 2.0 + 2.0, -h / 2.0 + 3.0, w, h, 5.0);

    // Body.
    let body = if police { 0xf1f1f1 } else { c.color };
    ctx.set_fill_style_str(&rgb(body));
    fill_round(ctx, -w / 2.0, -h / 2.0, w, h, 5.0);

    // Windshield / rear window.
    ctx.set_fill_style_str("rgba(20,30,48,0.85)");
    fill_round(ctx, w * 0.05, -h / 2.0 + 2.5, w * 0.22, h - 5.0, 3.0);
    fill_round(ctx, -w * 0.32, -h / 2.0 + 3.0, w * 0.16, h - 6.0, 3.0);

    // Police light bar (flashing).
    if police {
        let phase = (s_time() * 8.0).floor() as u32 % 2;
        ctx.set_fill_style_str(if phase == 0 { "#ff3b30" } else { "#3478f6" });
        ctx.fill_rect(-3.0, -h / 2.0 + 2.0, 6.0, h - 4.0);
    }
    // Headlights.
    ctx.set_fill_style_str("rgba(255,244,180,0.9)");
    ctx.fill_rect(w / 2.0 - 3.0, -h / 2.0 + 2.0, 3.0, 4.0);
    ctx.fill_rect(w / 2.0 - 3.0, h / 2.0 - 6.0, 3.0, 4.0);
    ctx.restore();
}

fn s_time() -> f64 {
    // Cheap global clock for flashing; render passes time via state usually.
    static mut T: f64 = 0.0;
    unsafe {
        T += 1.0 / 60.0;
        T
    }
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
        draw_person(ctx, p.x, p.y, p.heading, p.color);
        ctx.set_global_alpha(1.0);
    }
}

fn draw_person(ctx: &CanvasRenderingContext2d, x: f64, y: f64, heading: f64, color: u32) {
    // Body.
    ctx.set_fill_style_str(&rgb(color));
    ctx.begin_path();
    ctx.arc(x, y, 6.0, 0.0, std::f64::consts::TAU);
    ctx.fill();
    // Head (offset toward facing direction).
    ctx.set_fill_style_str("#e0ac69");
    ctx.begin_path();
    ctx.arc(x + heading.cos() * 3.0, y + heading.sin() * 3.0, 3.5, 0.0, std::f64::consts::TAU);
    ctx.fill();
}

/// Shared HUD (money, stars, speed, mission line, banner, minimap).
pub fn draw_hud(ctx: &CanvasRenderingContext2d, s: &GameState, w: f64, h: f64) {
    // Money (top-left).
    ctx.set_font(FONT);
    ctx.set_text_align("left");
    ctx.set_fill_style_str("rgba(0,0,0,0.45)");
    fill_round(ctx, 12.0, 12.0, 150.0, 34.0, 8.0);
    ctx.set_fill_style_str("#7CFC00");
    ctx.fill_text(&format!("${}", s.money), 22.0, 36.0);

    // Wanted stars (top-right).
    let st = s.stars();
    for i in 0..5u32 {
        let x = w - 40.0 - (4 - i) as f64 * 30.0;
        draw_star(ctx, x, 30.0, 12.0, if i < st { "#ffd60a" } else { "rgba(255,255,255,0.15)" });
    }

    // Speed (bottom-left) when in a car.
    if !s.on_foot {
        let kmh = (s.car.speed() * 0.18).round() as u32;
        ctx.set_fill_style_str("rgba(0,0,0,0.45)");
        fill_round(ctx, 12.0, h - 56.0, 120.0, 40.0, 8.0);
        ctx.set_font("bold 24px 'Segoe UI', system-ui, sans-serif");
        ctx.set_fill_style_str("#ffffff");
        ctx.fill_text(&format!("{} km/h", kmh), 24.0, h - 28.0);
    } else {
        ctx.set_fill_style_str("rgba(255,255,255,0.7)");
        ctx.set_font(FONT);
        ctx.fill_text("ON FOOT — E: enter car", 16.0, h - 24.0);
    }

    // Mission timer.
    if let Some((mx, my)) = s.mission.current_marker() {
        let (px, py) = s.player_pos();
        let d = ((mx - px).powi(2) + (my - py).powi(2)).sqrt();
        let d_m = (d * 0.5).round() as u32; // ~meters
        let mut line = if matches!(s.mission.phase, crate::mission::MissionPhase::ToDeliver) {
            format!("DELIVER  {}m  ({}s)", d_m, s.mission.time_left.ceil() as u32)
        } else {
            format!("PICKUP   {}m", d_m)
        };
        line.push_str("  ↑");
        ctx.set_font(FONT);
        ctx.set_fill_style_str("rgba(0,0,0,0.45)");
        let tw = 150.0;
        fill_round(ctx, w / 2.0 - tw / 2.0, 10.0, tw, 30.0, 8.0);
        ctx.set_fill_style_str("#ffffff");
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

    // Mission marker.
    if let Some((mx, my)) = s.mission.current_marker() {
        let (x, y) = pt(mx, my);
        let gold = matches!(s.mission.phase, crate::mission::MissionPhase::ToPickup);
        ctx.set_fill_style_str(if gold { "#ffd60a" } else { "#4caf50" });
        ctx.begin_path();
        ctx.arc(x, y, 4.0, 0.0, std::f64::consts::TAU);
        ctx.fill();
    }
    // Police.
    for p in &s.police {
        let (x, y) = pt(p.x, p.y);
        ctx.set_fill_style_str("#3478f6");
        ctx.begin_path();
        ctx.arc(x, y, 3.0, 0.0, std::f64::consts::TAU);
        ctx.fill();
    }
    // Player.
    let (px, py) = s.player_pos();
    let (x, y) = pt(px, py);
    ctx.set_fill_style_str("#ffffff");
    ctx.begin_path();
    ctx.arc(x, y, 4.5, 0.0, std::f64::consts::TAU);
    ctx.fill();
    ctx.restore();

    ctx.set_stroke_style_str("rgba(255,255,255,0.5)");
    ctx.set_line_width(2.0);
    ctx.stroke_rect(ox, oy, ms, ms);
}

fn draw_busted(ctx: &CanvasRenderingContext2d, w: f64, h: f64) {
    ctx.set_fill_style_str("rgba(0,0,40,0.75)");
    ctx.fill_rect(0.0, 0.0, w, h);
    ctx.set_font("bold 64px 'Segoe UI', system-ui, sans-serif");
    ctx.set_text_align("center");
    ctx.set_fill_style_str("#ff3b30");
    ctx.fill_text("BUSTED", w / 2.0, h / 2.0);
}

use wasm_bindgen::JsValue;
