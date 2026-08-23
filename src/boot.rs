//! WASM entry point: wires DOM (canvas, keyboard, rAF) to the game loop.

use std::cell::RefCell;
use std::rc::Rc;

use js_sys::Array;
use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, KeyboardEvent};

use crate::audio::Audio;
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
            if Input::KEYS.contains(&kl.as_str()) {
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

    // --- 2D context ---
    let ctx = canvas
        .get_context("2d")
        .expect("no 2d context")
        .expect("no 2d context")
        .dyn_into::<CanvasRenderingContext2d>()
        .unwrap();

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

/// Debug/test: `1` if the 3D view is active, `0` for top-down.
#[wasm_bindgen]
#[allow(static_mut_refs)] // single-threaded wasm: STATE is only written once in start()
pub fn debug_view_mode() -> f32 {
    // SAFETY: single-threaded wasm game loop.
    unsafe { STATE.as_ref().map(|s| if s.borrow().view_3d { 1.0 } else { 0.0 }) }
        .unwrap_or(0.0)
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
