//! GTA VI — Web Edition. A top-down open-world game in pure Rust (WASM).
//!
//! Pure, unit-testable modules (city, car physics, entities, missions, state)
//! are target-agnostic; the wasm-only modules (render, audio, boot) are gated.

pub mod car;
pub mod city;
pub mod input;
pub mod mission;
pub mod ped;
pub mod police;
pub mod state;
pub mod traffic;

#[cfg(target_arch = "wasm32")]
pub mod audio;
#[cfg(target_arch = "wasm32")]
pub mod boot;
#[cfg(target_arch = "wasm32")]
pub mod render;

/// Tiny deterministic RNG (xorshift64*) so the city & tests are reproducible.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut s = seed;
        if s == 0 { s = 0x9E3779B97F4A7C15; }
        Self(s)
    }
    /// Next u64.
    pub fn next_u64(&mut self) -> u64 {
        let mut s = self.0;
        s ^= s >> 12;
        s ^= s << 25;
        s ^= s >> 27;
        self.0 = s;
        s.wrapping_mul(0x2545F4914F6CDD1D)
    }
    /// Float in [0, 1).
    pub fn f(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
    /// Float in [lo, hi).
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.f() * (hi - lo)
    }
    /// Integer in [0, n).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}
