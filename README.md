# GTA VI — Web Edition

A GTA-inspired top-down open-world game, **100% Rust compiled to WebAssembly**.
Procedurally generated city, drivable cars and on-foot mode, pedestrians, AI
traffic, a police/wanted system, live wildlife (a herd of elephants that
wanders the streets and birds flapping and gliding through the sky), and a
timed package-delivery mission loop — rendered on a `<canvas>`, with sound
via the Web Audio API. There's a car and an **airplane** (fly over the whole
city), and a **dragon** you can take to the sky (press `D` and fly it with
the keyboard and mouse). Two view modes:
classic **top-down** and a **3D chase-cam** mode (press `V`). No native
code, no C, no SDL.

## Architecture

The crate (`src/`) is split into two halves:

- **Pure, unit-testable game logic** (target-agnostic, no wasm deps):
  - `city.rs` — procedural city generation
  - `car.rs` — car physics
  - `ped.rs` — pedestrian NPCs
  - `traffic.rs` — AI traffic
  - `police.rs` — police / wanted-level logic
  - `mission.rs` — timed fetch-and-deliver missions (yellow = pickup, green = delivery)
  - `wildlife.rs` — elephants (herd wander, startle-and-freeze, diagonal
    walk gait), birds (flap/glide flight, meandering sky paths) and the
    dragon (high-altitude banked meanders, plus a player-controlled flight
    mode, owns a private RNG stream so it never disturbs the deterministic
    world), plus the baked `DragonMesh` (a GLB model converted to plain
    render-ready arrays)
  - `glb.rs` — minimal binary-glTF (GLB) loader built on the Khronos
    `gltf` crate: parses geometry, applies node transforms, decodes
    embedded JPEG/PNG textures and bakes base colors into vertices
  - `state.rs` — game state machine, HUD, score
  - `fx.rs` — particle system (tire smoke, crash sparks, debris, dust,
    mission glitter) — pure data + update, drawn by both renderers
  - `input.rs` — keyboard input mapping
  - `cam3d.rs` — 3D camera & perspective projection math (chase cam,
    near-plane clipping, angle lerp)
- **wasm-only glue** (`#[cfg(target_arch = "wasm32")]`):
  - `boot.rs` — WASM entry point, wires up the game loop + canvas
  - `render.rs` — top-down canvas rendering: gradient car bodies, glass
    sheens, asphalt speckle, varied rooftops (helipads, water towers,
    antennas, parking lots), flashing siren glow, animated mission ring,
    particle FX, vignette (+ shared HUD/overlays)
  - `render3d.rs` — 3D mode: software perspective renderer (extruded
    building boxes, sun-shaded faces, painter's-algorithm depth sorting,
    near-plane clipping) drawn on the same canvas — plus distance fog, 
    vertical-gradient face shading, lawn cell texture, zebra crosswalks,
    spinning propeller, billboarded particle FX and a vignette
  - `audio.rs` — Web Audio oscillator bleeps

A small deterministic RNG (`xorshift64*`, in `lib.rs`) makes the generated
city and the tests reproducible.

The HUD is shared by both renderers: money, wanted stars, mission line,
minimap and the always-on **SPECIALS** panel (top-right) that lists every
special action currently available with its key (board/exit **E**, summon
airplane **F**, summon dragon **G**, auto-land **M**, view **V**/camera
**C**, pause **P**, recenter **R**).

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

Every special usage has its own dedicated key that never overlaps the
movement keys (WASD / arrows / Shift / Space), and the **SPECIALS panel in
the top-right corner always shows which special actions are available right
now and how to use them** — it updates with your situation (on foot, in the
car, in the plane, on the elephant, on the dragon).

- **E** — board / exit: enter the nearest rideable thing in reach (car,
  airplane, elephant) or get off the one you're on (car, airplane, elephant,
  dragon). Vehicles must be (nearly) stopped first.
- **F** — summon the airplane to you and take the controls (works from
  anywhere).
- **G** — summon the dragon to you and take the reins (works from anywhere;
  snaps you into the 3D view).
- **M** — auto-land (airplane only): autopilot flies to the nearest clear
  intersection; press again to cancel.
- **F1** — toggle **auto mode** for whatever you're currently in (car,
  airplane, elephant, dragon). In auto mode the object travels on its own
  according to its *native nature* — the car cruises the street grid like
  traffic, the plane holds a lazy level loop at its current altitude, the
  elephant wanders the streets, and the dragon meanders high above the city.
  Grab **any** control key (or the mouse, in the plane/dragon) to take over
  at any moment; release it and the object hands itself back to auto mode.
  Press **F1** again to leave auto mode entirely (full manual).
- **V** — toggle top-down / 3D chase-cam view
- **C** (3D mode) — reset the camera back to the chase position
- **P** — pause
- **R** — recenter camera on player (top-down)
- **Mouse drag** — 3D mode: orbit/tilt the camera; in the plane/dragon:
  steer it (horizontal = yaw, vertical = pitch)

In a car:

- **W / ↑** — accelerate
- **S / ↓** — brake / reverse
- **A / D, ← / →** — steer
- **Space** — handbrake
- **Shift** — boost
- **E** — exit vehicle (must be stopped)

In the airplane (keyboard and mouse both work; keyboard wins when held):

- **W / ↑** — throttle
- **S / ↓** — brake / slow
- **A / D, ← / →** — yaw
- **Shift** — climb
- **Space** — dive / descend
- **Mouse drag** — steer: horizontal = yaw, vertical = pitch
  (drag up = climb, drag down = dive). While in the plane the drag
  controls the plane instead of orbiting the 3D camera.
- **Left mouse (hold)** — full throttle
- **Right mouse (hold)** — full brake
- **Mouse wheel** — set cruise throttle (shown as THR % in the HUD)
- **M** — auto-land: the autopilot flies the plane to the nearest clear
  road intersection (a safe space), levels out, sets it down and brakes to a
  stop. The HUD shows `AUTO-LAND → distance` while it happens. Press **M**
  again any time to cancel and take back the controls.
- **E** — exit (must be slow; drops you to the street below)

Riding the dragon (keyboard and mouse both work; keyboard wins when held):

- **W / ↑** — speed up (throttle) · **S / ↓** — slow down / brake
- **A / D, ← / →, or mouse drag** — turn (the dragon banks into its turns)
- **Shift** — climb · **Space** — dive
- **Left mouse (hold)** — full throttle · **Right mouse (hold)** — full brake
- **Mouse wheel** — set cruise throttle (shown as THR % in the HUD)
- **Left mouse** — also breathe **fireballs** (LMB; holding streams them)
- **E** — exit: you drop to the street below, on foot, and the dragon
  resumes circling the city on its own from the altitude you left it at.

On foot:

- **WASD / arrows** — walk
- **Shift** — run
- **Right mouse (hold)** — walk forward in the direction you're facing
- **E** — board the nearest rideable thing in reach: your car, the
  airplane, or an **elephant** (it wanders the streets on its own and
  carries you along on its back; **E** again jumps you off back to the
  pavement as a pedestrian)

**Auto mode (F1)** applies to every rideable object. While it's on, the
object you're in follows its own nature the moment you stop touching the
controls, and snaps back to obeying you the instant you press a key — so
you can cruise the streets hands-free, loiter at altitude in the plane,
steer an elephant for a block and then let it wander, or ride the dragon
while it loops the sky. The **SPECIALS** panel shows the current F1 state.

### 3D mode

`V` switches to a third-person perspective view: the camera chases the
player (pulling in/out with speed), the city is extruded into buildings of
varied height (with occasional towers), and cars, peds, trees and the
mission marker and wildlife are 3D objects with shadows. It's a software rasterizer on
the 2D canvas — pure Rust, no WebGL. Every surface gets a vertical sheen
gradient and the whole scene melts into a soft atmospheric haze in the
distance; the roads carry shoulder lines and zebra crosswalks at every
intersection, and the grass has a per-cell mowed texture. The HUD,
minimap, BUSTED and PAUSED overlays are shared with top-down.

The wildlife is fully 3D here: elephants are built from shaded, tapered
3D cylinders (barrel body, neck, head, four striding legs, big fan ears,
a swaying trunk, swishing tail — with a calf in the herd), and the birds
have flapping (or gliding) swept wings, tapered bodies, heads and beaks,
casting small shadows on the street below.

While in 3D mode you can **drag with the mouse** to look around: horizontal
drag orbits the camera around the player, vertical drag changes the pitch
(the horizon moves with it). The offset is relative to the chase cam, so
the camera keeps following the player as you drive. Press **C** to snap
back to the default chase view.

### Airplane — fly over the city

A small airplane is parked at the airfield (intersection 5,5 — east of the
spawn point). Drive or walk up to it and press **E** to board — or just press
**F** anywhere and the plane teleports in next to you and hands you the
controls. Fly with **W**/**A**/**D**/**Shift**/**Space**, or with the mouse:
drag to steer (horizontal = yaw, drag up = climb), hold **left click** for
full throttle, hold **right click** to brake, and use the **wheel** to set
cruise throttle. Climb up to ~1,200 px of altitude and fly straight over
buildings, traffic and elephants (nothing below the wings can hurt you, and
you'll leave the police far behind); the HUD shows altitude and throttle.
Press **V** for the 3D chase cam: the camera follows you up and the city
spreads out beneath you. To land by hand, dive back down, cut the throttle
and press **E** once you're nearly stopped — or just press **M** and the
autopilot lands the plane on the nearest clear intersection for you.

### The dragon — a real 3D model (GLB)

A bronze dragon circles the city at high altitude. It isn't hand-modeled:
it's a genuine GLB (binary glTF) asset — Khronos' *DragonAttenuation*
sample model (~6.5 MB, 91,216 dragon triangles, embedded JPEG textures) —
downloaded to `web/assets/dragon.glb`. At boot the game fetches it (local
file first, with a raw-GitHub-URL fallback), parses it with the `gltf`
crate, and bakes it into a flat `DragonMesh` (positions, normals, wing-flap
weights and per-triangle colors, all pre-scaled and re-oriented). The 3D
renderer then transforms and depth-sorts the whole mesh every frame behind
the buildings, with the same backface culling, sun lighting and distance
fog as everything else; far away it falls back to a cheap silhouette so the
software rasterizer never drowns in distant triangles. In the top-down view
it's a small flapping shadow-caster. Press **G** anywhere to summon it to
you and take its reins (W/S speed, A/D or drag to turn, Shift/Space to
climb and dive, LMB to breathe fireballs) — a smooth, banked,
wing-flapping pursuit over the rooftops — then press **E** to drop back to
the street and let it resume its rounds. Its flight loop runs on a private
RNG so the rest of the deterministic world is unaffected.

### Visual FX & particles

Both view modes share one deterministic particle pool (pure Rust, unit-
tested in `fx.rs`): handbrake drifts leave tire smoke off the rear wheels,
boost leaves a flash of exhaust, wall/traffic/elephant crashes throw up
sparks, debris and smoke, running over a pedestrian leaves a red mist,
walking elephants kick up a trail of street dust, the plane sheds a thin
contrail from its wingtips (and puffs dust and sparks on a hard landing),
and mission pickups/deliveries shower gold/green glitter. The airplane's
propeller visibly spins (blur disc top-down, whirling blades in 3D), police
light bars cast a pulsing red/blue glow, car paint has sun-glint streaks,
and the mission marker carries a rotating dashed ring (top-down) or an
expanding radar ring (3D). A subtle vignette frames both views.
- Crimes (hitting peds or traffic) raise your wanted heat; police spawn and
  chase. Getting caught triggers a **BUSTED** screen and costs you money.
- Watch out for the **elephants** on the streets: they freeze when a fast car
  comes near, and they're solid — a fast hit raises your wanted level. Or just
  walk up to one and press `E` to **board it** — the elephant amblers down the
  streets on its own (with the whole herd, at a very relaxed pace), and `E`
  again drops you back to the pavement as a pedestrian. Pigeons, gulls and
  hawks circle overhead; press `V` and look up.
- Start with $100; mission payouts add to it.

### Car handling

- **Parking steering**: steering has a minimum authority at any moving
  speed, so the car can manoeuvre at crawl speeds (a fully stopped car
  still can't spin its wheels).
- **Firm brake**: full brake keeps brake strength down through zero —
  no low-speed dead zone that leaks into the reverse ramp. Holding ↓ from
  a stop then builds up a normal reverse.
- **Drift FX**: breaking traction with the handbrake (or a hard swerve at
  speed) kicks up tire smoke from the rear wheels in both view modes.

## Tests

### Unit tests (native, fast)

```sh
cargo test
```

Runs the pure-logic test suites (`car`, `city`, `fx`, `input`, `mission`,
`police`, `state`) natively — no browser needed.

### Browser smoke test

With the site served at `http://localhost:8090/`:

```sh
node tools/browser-test.js
```

Drives the game headlessly via Puppeteer (accelerates, steers, brakes,
exits/re-enters the car and walks on foot, checks the handbrake, toggles
the 3D view and drives in it, then proves pure arrow-key driving on a
teleported open park (straight, steer right, steer back left, hard brake),
takes screenshots to `/tmp/gt6test/`, and fails on any page/console
errors).

`node tools/plane-test.js` also flies the airplane: summons it with **F**,
flies it with the mouse (climb, yaw, wheel throttle), screenshots the 3D
flyover (`/tmp/gt6_plane_fly.png`), then auto-lands with **M** at the nearest
safe space.

`node tools/dragon-test.js` checks the GLB pipeline: the dragon model loads
with its triangle count, the dragon is actually flying, and a focused 3D
screenshot lands in `/tmp/gt6_dragon_3d.png`.

`node tools/dragon-fly-test.js` exercises the dragon-control mode: mounts it
with **D**, climbs and accelerates with the keyboard, banks a turn with a
mouse drag, screenshots the 3D chase cam (`/tmp/gt6_dragon_fly.png`), then
releases it back to the street.

## Project layout

```
Cargo.toml            crate config (rlib for tests + cdylib for wasm)
src/                  game code (see Architecture)
web/index.html        one-page host
web/assets/dragon.glb the dragon 3D model (GLB, Khronos sample)
web/pkg/              wasm-bindgen output (generated, do not edit)
tools/browser-test.js headless-browser smoke test
tools/dragon-test.js  headless GLB/dragon smoke test
tools/dragon-fly-test.js  headless dragon-control smoke test
```

## License

MIT
