//! WASM entry point: wires DOM (canvas, keyboard, rAF) to the game loop.

#![allow(static_mut_refs)] // single-threaded wasm: STATE is only written once in start()

use std::cell::RefCell;
use std::rc::Rc;

use js_sys::Array;
use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, KeyboardEvent, MouseEvent};

use crate::audio::Audio;
use crate::config::Config;
use crate::input::Input;
use crate::state::{step, Event, GameState};

/// The wasm-bindgen "start" hook: runs after `init()` in the generated JS.
#[wasm_bindgen(start)]
pub fn start() {
    let window = web_sys::window().expect("no window");
    let document = window.document().expect("no document");

    let canvas = document
        .get_element_by_id("game")
        .expect("missing <canvas id='game'>")
        .dyn_into::<HtmlCanvasElement>()
        .expect("not a canvas");

    let dpr = window.device_pixel_ratio().max(1.0);

    let state: Rc<RefCell<GameState>> = Rc::new(RefCell::new(GameState::new(20260606)));
    // SAFETY: single-threaded wasm game loop.
    unsafe { STATE = Some(state.clone()); };
    let input: Rc<RefCell<Input>> = Rc::new(RefCell::new(Input::new()));
    let audio: Rc<RefCell<Audio>> = Rc::new(RefCell::new(Audio::new()));
    let acc: Rc<RefCell<f64>> = Rc::new(RefCell::new(0.0f64));
    let last: Rc<RefCell<Option<f64>>> = Rc::new(RefCell::new(None));

    // --- Sizing (CSS in index.html stretches the canvas to the viewport) ---
    {
        let rc = canvas.clone();
        let rw = window.clone();
        let resize_once = || {
            let cw = rw.inner_width().map(|v| v.as_f64().unwrap_or(800.0)).unwrap_or(800.0);
            let ch = rw.inner_height().map(|v| v.as_f64().unwrap_or(600.0)).unwrap_or(600.0);
            rc.set_width((cw * dpr) as u32);
            rc.set_height((ch * dpr) as u32);
        };
        resize_once(); // initial size
        let cb = Closure::<dyn FnMut()>::new(move || {
            let cw = rw.inner_width().map(|v| v.as_f64().unwrap_or(800.0)).unwrap_or(800.0);
            let ch = rw.inner_height().map(|v| v.as_f64().unwrap_or(600.0)).unwrap_or(600.0);
            rc.set_width((cw * dpr) as u32);
            rc.set_height((ch * dpr) as u32);
        });
        window.set_onresize(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }

    // --- Keyboard ---
    {
        let ri = input.clone();
        let ra = audio.clone();
        let cb = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
            let k = e.key().to_string();
            let kl = k.to_ascii_lowercase();
            // Suppress the browser defaults for every currently bound key
            // (the bindings can change in the config page, so ask the live
            // config every time).
            let bound = unsafe { STATE.as_ref() }
                .map(|s| Input::prevent_keys(&s.borrow().config))
                .unwrap_or_else(|| Input::prevent_keys(&Config::default()));
            if bound.contains(&kl) {
                e.prevent_default();
                ra.borrow_mut().unlock();
            }
            ri.borrow_mut().key_down(&k);
        });
        window.set_onkeydown(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }
    {
        let ri = input.clone();
        let cb = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
            ri.borrow_mut().key_up(&e.key().to_string());
        });
        window.set_onkeyup(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }

    // --- Mouse (drag to orbit/pitch the 3D camera, or steer the plane) ---
    // No context menu: the right button is a game control.
    {
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            e.prevent_default();
        });
        let _ = window.add_event_listener_with_callback("contextmenu", cb.as_ref().unchecked_ref());
        cb.forget();
    }
    {
        let ri = input.clone();
        let ra = audio.clone();
        let cb = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
            match e.button() {
                0 => {
                    e.prevent_default();
                    ra.borrow_mut().unlock();
                    ri.borrow_mut().mouse_down();
                }
                1 => {
                    e.prevent_default();
                    ra.borrow_mut().unlock();
                    ri.borrow_mut().mouse_middle_down();
                }
                2 => {
                    e.prevent_default();
                    ra.borrow_mut().unlock();
                    ri.borrow_mut().mouse_right_down();
                }
                _ => {}
            }
        });
        window.set_onmousedown(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }
    {
        let ri = input.clone();
        let cb = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
            ri.borrow_mut()
                .mouse_move(e.movement_x() as f64, e.movement_y() as f64);
        });
        window.set_onmousemove(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }
    {
        let ri = input.clone();
        let cb = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
            match e.button() {
                0 => ri.borrow_mut().mouse_up(),
                1 => ri.borrow_mut().mouse_middle_up(),
                2 => ri.borrow_mut().mouse_right_up(),
                _ => {}
            }
        });
        window.set_onmouseup(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }

    // --- Mouse wheel (cruise throttle in the plane) ---
    {
        let ri = input.clone();
        let cb = Closure::<dyn FnMut(web_sys::WheelEvent)>::new(move |e: web_sys::WheelEvent| {
            // Normalize: one notch = 1, regardless of wheel granularity.
            // Wheel up = more throttle, wheel down = less.
            ri.borrow_mut().mouse_wheel(-e.delta_y().signum());
        });
        window.set_onwheel(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }

    // --- 2D context ---
    let ctx = canvas
        .get_context("2d")
        .expect("no 2d context")
        .expect("no 2d context")
        .dyn_into::<CanvasRenderingContext2d>()
        .unwrap();

    // --- Dragon GLB model (async fetch + parse + bake) ---
    // The game renders a low-poly silhouette until this completes.
    {
        let st = state.clone();
        load_dragon_glb(&window, st);
    }

    // --- Player config: config.ini from the page directory (or the
    //     localStorage copy saved by the config page) ---
    {
        let st = state.clone();
        load_config_ini(window.clone(), st, false);
    }

    // --- Main loop (rAF) ---
    let raf: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
    let raf2 = raf.clone();
    let w2 = window.clone();
    let loop_cb = Closure::new(move |t: f64| {
        if let Some(c) = raf2.borrow().as_ref() {
            let _ = w2.request_animation_frame(c.as_ref().unchecked_ref());
        }
        let dt = match last.borrow().clone() {
            Some(l) => (t - l).max(0.0) / 1000.0,
            None => 0.0,
        };
        *last.borrow_mut() = Some(t);

        let events: Vec<Event> = {
            step(
                &mut state.borrow_mut(),
                &mut input.borrow_mut(),
                &mut acc.borrow_mut(),
                dt,
            )
        };
        for e in events {
            let mut a = audio.borrow_mut();
            match e {
                Event::Crash | Event::PoliceHit => a.crash(),
                Event::PedHit => a.alarm(),
                Event::Mission(ev) => {
                    if ev.reward > 0 {
                        a.deliver();
                    } else {
                        a.pickup();
                    }
                }
                Event::Busted => a.busted(),
                Event::Fireball => a.fireball(),
                Event::BuildingDown => a.boom(),
                Event::ConfigSave => {
                    let ini = state.borrow().config.to_ini();
                    if let Some(doc) = w2.document() {
                        save_config_ini(&w2, &doc, &ini);
                    }
                }
                Event::ConfigLoad => {
                    load_config_ini(w2.clone(), state.clone(), true);
                }
                _ => {}
            }
        }

        let cw = w2.inner_width().map(|v| v.as_f64().unwrap_or(800.0)).unwrap_or(800.0);
        let ch = w2.inner_height().map(|v| v.as_f64().unwrap_or(600.0)).unwrap_or(600.0);
        let st = state.borrow();
        if st.view_3d {
            crate::render3d::render(&ctx, &st, cw, ch, dpr);
        } else {
            crate::render::render(&ctx, &st, cw, ch, dpr);
        }
    });

    *raf.borrow_mut() = Some(loop_cb);
    let _ = window.request_animation_frame(raf.borrow().as_ref().unwrap().as_ref().unchecked_ref());
}

/// Live game state (set in `start`), for the debug/test exports below.
// SAFETY: single-threaded wasm game loop; only ever written in `start`.
pub static mut STATE: Option<Rc<RefCell<GameState>>> = None;

/// Debug/test: player speed in px/s (0 if not started).
#[wasm_bindgen]
#[allow(static_mut_refs)] // single-threaded wasm: STATE is only written once in start()
pub fn debug_player_speed() -> f32 {
    // SAFETY: single-threaded wasm game loop.
    unsafe { STATE.as_ref().map(|s| s.borrow().player_speed() as f32) }
        .unwrap_or(0.0)
}

/// Debug/test: teleport the player car to `(x, y)` facing `heading`.
/// Clears velocity and recenters the cameras; used by the browser test to
/// reach a known straight road.
#[wasm_bindgen]
#[allow(static_mut_refs)] // single-threaded wasm: STATE is only written once in start()
pub fn debug_teleport(x: f64, y: f64, heading: f64) {
    // SAFETY: single-threaded wasm game loop.
    let state = unsafe { STATE.as_ref().cloned() };
    if let Some(s) = state {
        let mut s = s.borrow_mut();
        s.on_foot = false;
        s.in_plane = false;
        s.car.x = x;
        s.car.y = y;
        s.car.heading = heading;
        s.car.vx = 0.0;
        s.car.vy = 0.0;
        s.cam_x = x;
        s.cam_y = y;
        s.cam3d_yaw = heading;
    }
}

/// Debug/test: player altitude (the plane's `z`, 0 on foot / in the car).
#[wasm_bindgen]
#[allow(static_mut_refs)] // single-threaded wasm: STATE is only written once in start()
pub fn debug_player_alt() -> f32 {
    // SAFETY: single-threaded wasm game loop.
    unsafe { STATE.as_ref().map(|s| s.borrow().player_alt() as f32) }
        .unwrap_or(0.0)
}

/// Debug/test: the plane's mouse cruise throttle (0..1).
#[wasm_bindgen]
#[allow(static_mut_refs)] // single-threaded wasm: STATE is only written once in start()
pub fn debug_mouse_throttle() -> f32 {
    // SAFETY: single-threaded wasm game loop.
    unsafe { STATE.as_ref().map(|s| s.borrow().mouse_throttle as f32) }
        .unwrap_or(0.0)
}

/// Debug/test: clear wanted heat, dismiss the police, and end any BUSTED
/// screen (used by the browser tests to keep the streets quiet).
#[wasm_bindgen]
#[allow(static_mut_refs)] // single-threaded wasm: STATE is only written once in start()
pub fn debug_clear_heat() {
    // SAFETY: single-threaded wasm game loop.
    let state = unsafe { STATE.as_ref().cloned() };
    if let Some(s) = state {
        let mut s = s.borrow_mut();
        s.heat = 0.0;
        s.police.clear();
        s.busted_until = 0.0;
    }
}

/// Debug/test: `1` while the M auto-land autopilot is active.
#[wasm_bindgen]
#[allow(static_mut_refs)] // single-threaded wasm: STATE is only written once in start()
pub fn debug_landing() -> f32 {
    // SAFETY: single-threaded wasm game loop.
    unsafe { STATE.as_ref().map(|s| if s.borrow().landing { 1.0 } else { 0.0 }) }
        .unwrap_or(0.0)
}

/// Debug/test: switch to the 3D view and aim the camera at the dragon
/// (used by the browser test to photograph the GLB model).
#[wasm_bindgen]
#[allow(static_mut_refs)] // single-threaded wasm: STATE is only written once in start()
pub fn debug_dragon_focus() {
    // SAFETY: single-threaded wasm game loop.
    let state = unsafe { STATE.as_ref().cloned() };
    if let Some(s) = state {
        let mut s = s.borrow_mut();
        let (px, py) = s.player_pos();
        let (dx, dy, dz) = (s.wildlife.dragon.x, s.wildlife.dragon.y, s.wildlife.dragon.z);
        let dist = (dx - px).hypot(dy - py).max(1.0);
        // The camera keeps easing back to `player heading + cam3d_orbit`, so
        // park the orbit offset where the dragon sits.
        let heading = if s.on_foot {
            s.foot_heading
        } else {
            s.active_vehicle().heading
        };
        let mut diff = ((dy - py).atan2(dx - px) - heading).rem_euclid(std::f64::consts::TAU);
        if diff > std::f64::consts::PI {
            diff -= std::f64::consts::TAU;
        }
        s.view_3d = true;
        s.cam3d_orbit = diff;
        s.cam3d_yaw = heading + diff;
        s.cam3d_pitch = ((dz - 60.0) / dist).clamp(-0.4, 0.6);
    }
}

/// Debug/test: `1` if the 3D view is active, `0` for top-down.
#[wasm_bindgen]
#[allow(static_mut_refs)] // single-threaded wasm: STATE is only written once in start()
pub fn debug_view_mode() -> f32 {
    // SAFETY: single-threaded wasm game loop.
    unsafe { STATE.as_ref().map(|s| if s.borrow().view_3d { 1.0 } else { 0.0 }) }
        .unwrap_or(0.0)
}

/// Debug/test: dragon snapshot as a flat f64 array:
/// `[x, y, z, heading, flap, bank, tris]` (`tris` is the triangle count of
/// the loaded GLB mesh — 0 while it is still loading or if the fetch failed).
#[wasm_bindgen]
#[allow(static_mut_refs)] // single-threaded wasm: STATE is only written once in start()
pub fn debug_dragon() -> Array {
    let a = Array::new();
    // SAFETY: single-threaded wasm game loop.
    let state = unsafe { STATE.as_ref().cloned() };
    if let Some(s) = state {
        let s = s.borrow();
        let d = s.wildlife.dragon;
        a.push(&wasm_bindgen::JsValue::from_f64(d.x));
        a.push(&wasm_bindgen::JsValue::from_f64(d.y));
        a.push(&wasm_bindgen::JsValue::from_f64(d.z));
        a.push(&wasm_bindgen::JsValue::from_f64(d.heading));
        a.push(&wasm_bindgen::JsValue::from_f64(d.flap));
        a.push(&wasm_bindgen::JsValue::from_f64(d.bank));
        a.push(&wasm_bindgen::JsValue::from_f64(
            s.dragon_mesh.as_ref().map(|m| m.tri_count() as f64).unwrap_or(0.0),
        ));
        a.push(&wasm_bindgen::JsValue::from_f64(if s.in_dragon { 1.0 } else { 0.0 }));
        a.push(&wasm_bindgen::JsValue::from_f64(d.speed));
    }
    a
}

/// The dragon GLB: local asset (downloaded from the internet once, checked
/// in so the game also works offline) and its original home on the internet
/// (KhronosGroup glTF sample models, CC-BY 4.0) as the fallback.
const DRAGON_GLB_LOCAL: &str = "assets/dragon.glb";
const DRAGON_GLB_REMOTE: &str = "https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Models/master/2.0/DragonAttenuation/glTF-Binary/DragonAttenuation.glb";

/// Small console helpers (the `console` web-sys API takes JsValues).
fn log_err(msg: &str) {
    web_sys::console::error_1(&JsValue::from_str(msg));
}

fn log_info(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

/// Fetch the dragon GLB, parse it with the `gltf` crate, bake the mesh into
/// the game state. Local asset first; on failure it falls back to loading
/// the free model straight from the internet.
fn load_dragon_glb(window: &web_sys::Window, state: Rc<RefCell<GameState>>) {
    try_fetch_dragon(window.clone(), state, DRAGON_GLB_LOCAL, true);
}

/// Wrap an at-most-once JS callback into the `FnMut` closure the promise
/// APIs want (promise `then`/`catch` callbacks fire at most once).
fn call_once(f: impl FnOnce(JsValue) + 'static) -> Closure<dyn FnMut(JsValue)> {
    let once: RefCell<Option<Box<dyn FnOnce(JsValue)>>> = RefCell::new(Some(Box::new(f)));
    Closure::new(move |v: JsValue| {
        if let Some(cb) = once.take() {
            cb(v);
        }
    })
}

fn try_fetch_dragon(window: web_sys::Window, state: Rc<RefCell<GameState>>, url: &'static str, from_local: bool) {
    let promise = window.fetch_with_str(url);
    let st = state.clone();
    let on_response = call_once(move |resp: JsValue| {
        let resp: Result<web_sys::Response, JsValue> = resp.dyn_into();
        let buf_promise = match resp.and_then(|r| r.array_buffer()) {
            Ok(p) => p,
            Err(_) => {
                log_err("dragon.glb: not a valid response body");
                return;
            }
        };
        let on_buffer = Closure::<dyn FnMut(JsValue)>::new(move |buf: JsValue| {
            let ab: js_sys::ArrayBuffer = buf.unchecked_into();
            let bytes: Vec<u8> = js_sys::Uint8Array::new(&ab).to_vec();
            match crate::glb::load_glb(&bytes) {
                Ok(model) => match crate::wildlife::DragonMesh::from_gltf(&model) {
                    Some(mesh) => {
                        log_info(&format!("dragon.glb loaded: {} tris", mesh.tri_count()));
                        st.borrow_mut().dragon_mesh = Some(mesh);
                    }
                    None => log_err("dragon.glb: no dragon mesh found"),
                },
                Err(e) => log_err(&format!("dragon.glb parse failed: {e}")),
            }
        });
        let _ = buf_promise.then(&on_buffer);
        on_buffer.forget();
    });
    let on_reject = call_once(move |_e: JsValue| {
        if from_local {
            log_err("local dragon.glb failed, trying the internet copy");
            try_fetch_dragon(window, state, DRAGON_GLB_REMOTE, false);
        }
    });
    let _ = promise.then(&on_response);
    let _ = promise.catch(&on_reject);
    on_response.forget();
    on_reject.forget();
}

/// Debug/test: `[fireballs in flight, burning buildings, citizens
/// fighting a fire]` (the dragon's breath damage counters).
#[wasm_bindgen]
#[allow(static_mut_refs)] // single-threaded wasm: STATE is only written once in start()
pub fn debug_dragonfire() -> Array {
    let a = Array::new();
    // SAFETY: single-threaded wasm game loop.
    let state = unsafe { STATE.as_ref().cloned() };
    if let Some(s) = state {
        let s = s.borrow();
        a.push(&JsValue::from_f64(s.fireballs.len() as f64));
        a.push(&JsValue::from_f64(s.building_fires.iter().filter(|f| f.burn > 0.0).count() as f64));
        a.push(&JsValue::from_f64(s.peds.iter().filter(|p| p.firefight.is_some()).count() as f64));
    }
    a
}

/// Debug/test: wildlife snapshot as a flat f64 array:
/// `[n_el, (x, y, scale) x n_el, n_birds, (x, y, z) x n_birds]`.
#[wasm_bindgen]
#[allow(static_mut_refs)] // single-threaded wasm: STATE is only written once in start()
pub fn debug_wildlife() -> Array {
    let a = Array::new();
    // SAFETY: single-threaded wasm game loop.
    let state = unsafe { STATE.as_ref().cloned() };
    if let Some(s) = state {
        let s = s.borrow();
        a.push(&wasm_bindgen::JsValue::from_f64(s.wildlife.elephants.len() as f64));
        for e in &s.wildlife.elephants {
            a.push(&wasm_bindgen::JsValue::from_f64(e.x));
            a.push(&wasm_bindgen::JsValue::from_f64(e.y));
            a.push(&wasm_bindgen::JsValue::from_f64(e.scale));
        }
        a.push(&wasm_bindgen::JsValue::from_f64(s.wildlife.birds.len() as f64));
        for b in &s.wildlife.birds {
            a.push(&wasm_bindgen::JsValue::from_f64(b.x));
            a.push(&wasm_bindgen::JsValue::from_f64(b.y));
            a.push(&wasm_bindgen::JsValue::from_f64(b.z));
        }
    }
    a
}

/// The `config.ini` the player config is saved to / loaded from (it is also
/// kept in `localStorage` so the browser can't lose it between downloads).
const CONFIG_FILE: &str = "config.ini";
const CONFIG_LS_KEY: &str = "gt6-config";

/// Fetch `config.ini` from the page directory and apply it to the game
/// state. When it is missing, the `localStorage` copy is used instead (so a
/// config saved from the config page keeps working). `announce` shows a HUD
/// message (used by the in-page "L: load" action; silent at boot).
fn load_config_ini(window: web_sys::Window, state: Rc<RefCell<GameState>>, announce: bool) {
    let promise = window.fetch_with_str(CONFIG_FILE);
    let st = state.clone();
    let w1 = window.clone();
    let on_response = call_once(move |resp: JsValue| {
        let resp: Result<web_sys::Response, JsValue> = resp.dyn_into();
        // A 404 (no config.ini next to index.html) still resolves — only a
        // real 200 response is config.
        let text_promise = match resp {
            Ok(r) if r.ok() => r.text().expect("response text"),
            Ok(_) | Err(_) => {
                config_ini_missing(&w1, st.clone(), announce);
                return;
            }
        };
        let on_text = Closure::<dyn FnMut(JsValue)>::new(move |t: JsValue| {
            let text = t.as_string().unwrap_or_default();
            let ok = st.borrow_mut().apply_config_text(&text);
            if ok {
                log_info(&format!("{} loaded", CONFIG_FILE));
                if announce {
                    st.borrow_mut().set_msg("CONFIG LOADED FROM config.ini", 2.5);
                }
            } else {
                log_err(&format!("{} could not be parsed", CONFIG_FILE));
            }
        });
        let _ = text_promise.then(&on_text);
        on_text.forget();
    });
    let st2 = state.clone();
    let on_reject = call_once(move |_e: JsValue| config_ini_missing(&window, st2, announce));
    let _ = promise.then(&on_response);
    let _ = promise.catch(&on_reject);
    on_response.forget();
    on_reject.forget();
}

/// No `config.ini` on the server: fall back to the localStorage copy.
fn config_ini_missing(window: &web_sys::Window, state: Rc<RefCell<GameState>>, announce: bool) {
    if let Ok(Some(ls)) = window.local_storage() {
        if let Ok(Some(saved)) = ls.get_item(CONFIG_LS_KEY) {
            state.borrow_mut().apply_config_text(&saved);
            if announce {
                state.borrow_mut().set_msg("CONFIG LOADED (BROWSER COPY)", 2.5);
            }
            return;
        }
    }
    if announce {
        state
            .borrow_mut()
            .set_msg("config.ini NOT FOUND — DEFAULTS IN USE", 2.5);
    }
}

/// Save the config as `config.ini`: a browser download (the file the game
/// loads on boot) plus a `localStorage` backup.
fn save_config_ini(window: &web_sys::Window, document: &web_sys::Document, ini: &str) {
    if let Ok(Some(ls)) = window.local_storage() {
        let _ = ls.set_item(CONFIG_LS_KEY, ini);
    }
    let seq = js_sys::Array::new();
    seq.push(&JsValue::from_str(ini));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("text/plain;charset=utf-8");
    if let Ok(blob) = web_sys::Blob::new_with_str_sequence_and_options(&seq, &opts) {
        if let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) {
            if let Ok(el) = document.create_element("a") {
                if let Ok(a) = el.dyn_into::<web_sys::HtmlAnchorElement>() {
                    a.set_href(&url);
                    a.set_download(CONFIG_FILE);
                    if let Some(body) = document.body() {
                        let _ = body.append_child(&a);
                        a.click();
                        let _ = a.remove();
                    }
                }
            }
            let _ = web_sys::Url::revoke_object_url(&url);
        }
    }
    log_info(&format!("{} saved (download started)", CONFIG_FILE));
}

/// Debug/test: the current config as `config.ini` text.
#[wasm_bindgen]
#[allow(static_mut_refs)] // single-threaded wasm: STATE is only written once in start()
pub fn debug_config_ini() -> String {
    // SAFETY: single-threaded wasm game loop.
    unsafe { STATE.as_ref().map(|s| s.borrow().config.to_ini()) }
        .unwrap_or_default()
}

/// Debug/test: `1` while the config page is open, `0` otherwise.
#[wasm_bindgen]
#[allow(static_mut_refs)] // single-threaded wasm: STATE is only written once in start()
pub fn debug_config_open() -> f32 {
    // SAFETY: single-threaded wasm game loop.
    unsafe { STATE.as_ref().map(|s| if s.borrow().config_open { 1.0 } else { 0.0 }) }
        .unwrap_or(0.0)
}

/// Debug/test: apply `config.ini` text to the live config (browser tests).
#[wasm_bindgen]
#[allow(static_mut_refs)] // single-threaded wasm: STATE is only written once in start()
pub fn debug_config_apply(text: &str) -> bool {
    let state = unsafe { STATE.as_ref().cloned() };
    state.map(|s| s.borrow_mut().apply_config_text(text)).unwrap_or(false)
}

/// Debug/test: `[player_x, player_y, on_foot, money, stars]`.
#[wasm_bindgen]
#[allow(static_mut_refs)] // single-threaded wasm: STATE is only written once in start()
pub fn debug_player_info() -> Array {
    let a = Array::new();
    // SAFETY: single-threaded wasm game loop.
    let state = unsafe { STATE.as_ref().cloned() };
    if let Some(s) = state {
        let s = s.borrow();
        let (x, y) = s.player_pos();
        a.push(&wasm_bindgen::JsValue::from_f64(x));
        a.push(&wasm_bindgen::JsValue::from_f64(y));
        a.push(&wasm_bindgen::JsValue::from_bool(s.on_foot));
        a.push(&wasm_bindgen::JsValue::from_f64(s.money as f64));
        a.push(&wasm_bindgen::JsValue::from_f64(s.stars() as f64));
    }
    a
}
