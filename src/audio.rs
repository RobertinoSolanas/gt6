#![allow(unused_must_use)]
//
#![allow(unused_must_use)]
//! Tiny WebAudio synth for feedback blips (no audio assets).
//! Created lazily on the first user gesture (required by browsers).

use wasm_bindgen::JsCast;
use web_sys::{AudioContext, AudioNode};

pub struct Audio {
    ctx: Option<AudioContext>,
}

impl Audio {
    pub fn new() -> Self {
        Audio { ctx: None }
    }

    /// Must be called from a user gesture (keydown) at least once.
    pub fn unlock(&mut self) {
        if self.ctx.is_some() {
            if let Some(ctx) = &self.ctx {
                let _ = ctx.resume();
            }
            return;
        }
        let opts = web_sys::AudioContextOptions::new();
        match AudioContext::new_with_context_options(&opts) {
            Ok(ctx) => {
                let _ = ctx.resume();
                self.ctx = Some(ctx);
            }
            Err(_) => {} // audio unsupported — game still fine
        }
    }

    pub fn tone(&mut self, freq: f64, dur: f64, vol: f64, when: f64) {
        let Some(ctx) = &self.ctx else { return };
        let t = ctx.current_time() + when;
        let osc = match ctx.create_oscillator() {
            Ok(o) => o,
            Err(_) => return,
        };
        let gain = match ctx.create_gain() {
            Ok(g) => g,
            Err(_) => return,
        };
        osc.set_type(web_sys::OscillatorType::Sine);
        osc.frequency().set_value_at_time(freq as f32, t);
        gain.gain().set_value_at_time(0.0001, t);
        gain.gain().set_target_at_time(vol as f32, t, 0.01);
        gain.gain().set_target_at_time(0.0001, t + dur * 0.6, 0.05);
        let _ = osc.connect_with_audio_node(&gain);
        // web-sys has no typed connect(->AudioDestinationNode); the destination
        // is just the same JS object wearing an AudioNode label.
        let dest: AudioNode = ctx.destination().unchecked_into();
        let _ = gain.connect_with_audio_node(&dest);
        let _ = osc.start_with_when(t);
        let _ = osc.stop_with_when(t + dur);
    }

    pub fn pickup(&mut self) {
        self.tone(660.0, 0.12, 0.2, 0.0);
        self.tone(880.0, 0.15, 0.2, 0.1);
    }

    pub fn deliver(&mut self) {
        self.tone(523.25, 0.15, 0.22, 0.0);
        self.tone(659.25, 0.15, 0.22, 0.12);
        self.tone(783.99, 0.3, 0.22, 0.24);
    }

    pub fn crash(&mut self) {
        self.tone(90.0, 0.25, 0.35, 0.0);
    }

    pub fn alarm(&mut self) {
        self.tone(1200.0, 0.08, 0.12, 0.0);
        self.tone(900.0, 0.08, 0.12, 0.1);
    }

    pub fn busted(&mut self) {
        self.tone(392.0, 0.3, 0.25, 0.0);
        self.tone(311.13, 0.5, 0.25, 0.25);
    }

    pub fn fireball(&mut self) {
        // A hot whoosh: descending growl.
        self.tone(300.0, 0.18, 0.2, 0.0);
        self.tone(180.0, 0.22, 0.18, 0.1);
    }

    pub fn boom(&mut self) {
        // Deep building-down blast.
        self.tone(70.0, 0.5, 0.4, 0.0);
        self.tone(48.0, 0.7, 0.35, 0.12);
    }
}
