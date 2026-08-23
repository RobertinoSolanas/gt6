# GTA VI — Web Edition

A GTA-inspired top-down open-world game, **100% Rust compiled to WebAssembly**.
Procedurally generated city, drivable cars and on-foot mode, pedestrians, AI
traffic, a police/wanted system, and a timed package-delivery mission loop —
rendered on a `<canvas>`, with sound via the Web Audio API. No native code,
no C, no SDL.

## Architecture

The crate (`src/`) is split into two halves:

- **Pure, unit-testable game logic** (target-agnostic, no wasm deps):
  - `city.rs` — procedural city generation
  - `car.rs` — car physics
  - `ped.rs` — pedestrian NPCs
  - `traffic.rs` — AI traffic
  - `police.rs` — police / wanted-level logic
  - `mission.rs` — timed fetch-and-deliver missions (yellow = pickup, green = delivery)
  - `state.rs` — game state machine, HUD, score
  - `input.rs` — keyboard input mapping
- **wasm-only glue** (`#[cfg(target_arch = "wasm32")]`):
  - `boot.rs` — WASM entry point, wires up the game loop + canvas
  - `render.rs` — canvas rendering
  - `audio.rs` — Web Audio oscillator bleeps

A small deterministic RNG (`xorshift64*`, in `lib.rs`) makes the generated
city and the tests reproducible.

The web shell is minimal: `web/index.html` loads the wasm-bindgen output in
`web/pkg/` (`gt6.js` + `gt6_bg.wasm`) and calls `init()`.

## Prerequisites

- [Rust](https://rustup.rs) with the `wasm32-unknown-unknown` target:
  `rustup target add wasm32-unknown-unknown`
- [wasm-pack](https://rustwasm.github.io/wasm-pack/)
- Chrome (for the browser test) and Node.js with `puppeteer-core` (optional)

## Build & Run

```sh
# 1. Build the wasm package into web/pkg (default wasm-pack mode)
wasm-pack build --target web --out-dir web/pkg

# 2. Serve the web/ directory
cd web
python3 -m http.server 8090

# 3. Open http://localhost:8090/
```

### Controls

In a car:

- **W / ↑** — accelerate
- **S / ↓** — brake / reverse
- **A / D, ← / →** — steer
- **Space** — handbrake
- **Shift** — boost
- **E** — exit vehicle (must be stopped)

On foot:

- **WASD / arrows** — walk
- **Shift** — run
- **E** — enter your vehicle (when close)

General:

- **P** — pause
- **R** — recenter camera on player

### Gameplay

- Complete deliveries: pick up the package at the **yellow** marker, drop it
  off at the **green** marker before the 75-second timer runs out. Reward
  scales with time remaining.
- Crimes (hitting peds or traffic) raise your wanted heat; police spawn and
  chase. Getting caught triggers a **BUSTED** screen and costs you money.
- Start with $100; mission payouts add to it.

## Tests

### Unit tests (native, fast)

```sh
cargo test
```

Runs the pure-logic test suites (`car`, `city`, `input`, `mission`, `police`,
`state`) natively — no browser needed.

### Browser smoke test

With the site served at `http://localhost:8090/`:

```sh
node tools/browser-test.js
```

Drives the game headlessly via Puppeteer (accelerates, steers, brakes,
exits/re-enters the car and walks on foot, checks the handbrake, takes
screenshots to `/tmp/gt6test/`, and fails on any page/console errors).

## Project layout

```
Cargo.toml            crate config (rlib for tests + cdylib for wasm)
src/                  game code (see Architecture)
web/index.html        one-page host
web/pkg/              wasm-bindgen output (generated, do not edit)
tools/browser-test.js headless-browser smoke test
```

## License

MIT
