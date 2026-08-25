# PerezOS Living Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the monolithic ANSI axolotl with a beautiful, anatomically layered, behaviorally generative PerezOS canvas mascot that is physically integrated into the selected session's Control Center and stays inside strict CPU, memory, payload, visibility, accessibility, and reduced-motion budgets.

**Architecture:** Load a no-build classic-script subsystem under the single `window.ComandOSPerezOS` namespace before the dashboard's existing inline script. Deterministic behavior produces phase targets, motion blends them into a constrained 120-channel rig, the solver maintains twelve claw/cable contacts and secondary springs, and one cached Canvas 2D renderer paints integer-snapped indexed pixel art; a lifecycle controller owns visibility and adaptive quality without recreating the mascot on state changes.

**Tech Stack:** Browser-native JavaScript, Canvas 2D, typed arrays, `requestAnimationFrame`, `IntersectionObserver`, `ResizeObserver`, CSS, Python `pytest`, Node.js built-in `node:test`, and the repository's Playwright/Chromium harness.

## Global Constraints

- Keep one visible `<canvas>` and zero per-body-part DOM elements.
- Use no WebGL, third-party animation dependency, build pipeline, network-loaded art, sound, speech, telemetry, server-side behavior generation, or new dashboard API request.
- Keep art at 224 by 192 logical pixels and scale with nearest-neighbor integer transforms only; narrow mode uses an authored crop no larger than 180 by 148 logical pixels at 1x.
- Keep the sloth's warm brown/cream identity across themes; only rim light, deep-shadow bias, cable node, and small props inherit theme colors.
- Model approximately 40–50 drawable pieces and 90–120 control channels, including four limbs and twelve independently closing claws.
- Generate at least 10,000 valid idle performance signatures from behavior composition without storing equivalent frame sequences.
- Begin `waiting` acknowledgement within five seconds and reach the safe `dead` pose within eight seconds.
- Full quality budgets after warmup: average engine update plus render below 1.0 ms per produced frame, p95 below 2.0 ms, decoded engine/atlas memory below 16 MiB, and compressed art plus engine payload below 750 KiB.
- Perform no engine work when the document is hidden, the stage is outside the viewport, reduced by the local mascot preference, or destroyed; never replay missed work on resume.
- `prefers-reduced-motion: reduce` selects an event-driven Static level with no continuous eye tracking, cable swing, breathing loop, or fur motion.
- Preserve selected-session name, status text, controls, local-only operation, current role/costume input contracts, and both desktop and narrow layouts.
- Use TDD for every behavior-bearing unit and commit after each independently testable task.

## File Map

### New runtime files

- `dash/perezos/core.js` — deterministic math, hashing, random streams, springs, fixed-size timing statistics, and diagnostics.
- `dash/perezos/art.js` — indexed palettes, 48-piece art manifest, authored pixel-cluster commands, cameras, masks, props, and in-memory atlas rasterization.
- `dash/perezos/rig.js` — 120-channel anatomical state, hierarchy, joint limits, cable constraints, inverse kinematics, load transfer, last-valid-pose recovery, and pose hashing.
- `dash/perezos/behaviors.js` — drives, deterministic personality, short-term memory, primitive registry, state priorities, cooldowns, and performance grammar.
- `dash/perezos/motion.js` — phase scheduling, channel ownership, safe interruption, anatomical target blending, and delayed fur/soft-tissue springs.
- `dash/perezos/renderer.js` — Canvas 2D atlas caches, draw order, integer camera transforms, palette/theme cache, dynamic masks, and renderer diagnostics.
- `dash/perezos/engine.js` — public `createPerezOS()` controller, scheduler, observers, adaptive quality governor, interactions, visibility, and destruction.
- `dash/perezos/perezos.css` — integrated cable stage, responsive composition, keyboard focus, disabled/hidden styling, and fixed anchor occlusion.

### New test files

- `tests/perezos/test_core.js` — deterministic core primitives.
- `tests/perezos/test_art.js` — manifest completeness, authored pieces, cameras, masks, and palette invariants.
- `tests/perezos/test_rig.js` — anatomy, contacts, constraints, cable energy, load transfer, and recovery.
- `tests/perezos/test_behaviors.js` — priority, memory, cooldown, personality, composition, and 10,000-signature coverage.
- `tests/perezos/test_motion.js` — phase progression, channel arbitration, safe interruption, and secondary motion.
- `tests/perezos/test_renderer.js` — integer scaling, stable draw order, dirty rendering, theme continuity, and quality detail policy with a fake canvas.
- `tests/perezos/test_engine.js` — public API, scheduling, visibility, observers, quality hysteresis, reduced motion, context changes, and teardown.
- `tests/perezos/test_long_run.js` — six-hour deterministic simulation and invariant coverage.
- `tests/test_perezos.py` — pytest wrapper for all Node unit/simulation suites and static integration assertions.
- `tests/e2e_perezos.js` — Playwright visual, browser lifecycle, accessibility, interaction, and performance harness.
- `tests/test_perezos_e2e.py` — optional-browser pytest wrapper with structured skip behavior.

### Modified files

- `dash/index.html` — load the engine/CSS, replace axolotl markup and lifecycle, migrate preference, provide context, and remove all old mascot code.
- `dash/sw.js` — advance the shell cache name so the split engine/CSS is not paired with an old cached dashboard.
- `tests/test_js_parses.sh` — syntax-check every PerezOS runtime file.
- `tests/test_dashboard_layout.py` — assert the integrated responsive stage and removal of the isolated halo.
- `bin/cc-dash` — rename axolotl costume comments to PerezOS diagnostic props without changing the response contract.
- `bin/cc-app` — rename the active-tab mascot comment.
- `DESIGN.md` — document PerezOS composition, canvas constraints, motion accessibility, and performance rules.

---

### Task 1: Deterministic Core and JavaScript Test Harness

**Files:**
- Create: `dash/perezos/core.js`
- Create: `tests/perezos/test_core.js`
- Create: `tests/test_perezos.py`
- Modify: `tests/test_js_parses.sh:5-14`

**Interfaces:**
- Consumes: Browser or Node `globalThis`, `performance.now()` only when a caller does not inject a clock.
- Produces: `ComandOSPerezOS.Core` containing `clamp(number, min, max)`, `lerp(a, b, t)`, `smoothstep(t)`, `hashSeed(string)`, `createRng(seed)`, `createSpring(value, stiffness, damping)`, `stepSpring(spring, target, dt)`, `createRingStats(size)`, and `createDiagnostics(clock)`.

- [ ] **Step 1: Write the failing deterministic-core tests**

Create `tests/perezos/test_core.js` with Node's built-in test runner and an isolated browser-like global:

```js
"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
global.window = global;
require("../../dash/perezos/core.js");
const C = global.ComandOSPerezOS.Core;

test("seeded random streams are reproducible and forkable", () => {
  const a = C.createRng(C.hashSeed("session-a"));
  const b = C.createRng(C.hashSeed("session-a"));
  assert.deepEqual(Array.from({length: 64}, () => a.next()),
                   Array.from({length: 64}, () => b.next()));
  assert.notDeepEqual([a.fork("eyes").next(), a.fork("fur").next()],
                      [b.fork("cable").next(), b.fork("pose").next()]);
});

test("bounded spring converges without non-finite values", () => {
  const spring = C.createSpring(0, 90, 18);
  for (let i = 0; i < 600; i += 1) C.stepSpring(spring, 1, 1 / 120);
  assert.ok(Number.isFinite(spring.value));
  assert.ok(Number.isFinite(spring.velocity));
  assert.ok(Math.abs(spring.value - 1) < 0.001);
});

test("ring stats are fixed-size and report average and p95", () => {
  const stats = C.createRingStats(4);
  [1, 2, 30, 4, 5].forEach(value => stats.push(value));
  assert.equal(stats.count, 4);
  assert.equal(stats.average(), 10.25);
  assert.equal(stats.percentile(0.95), 30);
});
```

- [ ] **Step 2: Run the core test and confirm the red state**

Run: `node --test tests/perezos/test_core.js`

Expected: FAIL because `dash/perezos/core.js` does not exist.

- [ ] **Step 3: Implement the deterministic core without steady-state allocation**

Create `dash/perezos/core.js` as a classic-script IIFE. Use FNV-1a plus a nonzero xorshift32 state; return persistent child streams from `fork(label)` so repeated calls do not restart a sequence. Implement ring percentile by copying only when diagnostics are requested, never during the animation loop:

```js
(function(root){
  "use strict";
  const NS = root.ComandOSPerezOS = root.ComandOSPerezOS || {};
  const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
  const lerp = (a, b, t) => a + (b - a) * t;
  const smoothstep = t => { t = clamp(t, 0, 1); return t * t * (3 - 2 * t); };
  function hashSeed(value){
    let h = 0x811c9dc5;
    for(const ch of String(value)){ h ^= ch.charCodeAt(0); h = Math.imul(h, 0x01000193); }
    h >>>= 0;
    return h || 0x9e3779b9;
  }
  function createRng(seed){
    let state = (seed >>> 0) || 0x9e3779b9;
    const children = new Map();
    const api = {
      next(){ state ^= state << 13; state ^= state >>> 17; state ^= state << 5;
              return (state >>> 0) / 4294967296; },
      range(lo, hi){ return lerp(lo, hi, api.next()); },
      int(lo, hi){ return lo + Math.floor(api.next() * (hi - lo + 1)); },
      pick(items){ return items[api.int(0, items.length - 1)]; },
      fork(label){
        if(!children.has(label)) children.set(label, createRng(hashSeed(`${state}:${label}`)));
        return children.get(label);
      },
      get state(){ return state >>> 0; },
    };
    return api;
  }
  function createSpring(value, stiffness, damping){
    return {value, velocity:0, stiffness, damping};
  }
  function stepSpring(spring, target, dt){
    dt = clamp(dt, 0, 1 / 20);
    spring.velocity += ((target - spring.value) * spring.stiffness -
                        spring.velocity * spring.damping) * dt;
    spring.value += spring.velocity * dt;
    if(!Number.isFinite(spring.value) || !Number.isFinite(spring.velocity)){
      spring.value = target; spring.velocity = 0;
    }
    return spring.value;
  }
  NS.Core = Object.freeze({clamp, lerp, smoothstep, hashSeed, createRng,
    createSpring, stepSpring, createRingStats, createDiagnostics});
})(typeof window !== "undefined" ? window : globalThis);
```

Add `createRingStats()` and `createDiagnostics()` in the same file with preallocated `Float64Array` storage. Diagnostics exposes `begin(kind)`, `end(kind, start)`, counters, and a frozen snapshot with update/render average and p95.

- [ ] **Step 4: Wire Node suites into pytest and syntax checking**

Create `tests/test_perezos.py` with a sorted test-file runner:

```python
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]


def test_perezos_node_suites():
    if not shutil.which("node"):
        pytest.skip("node is not installed")
    files = sorted((ROOT / "tests" / "perezos").glob("test_*.js"))
    assert files, "PerezOS Node suites are missing"
    subprocess.run(["node", "--test", *map(str, files)], cwd=ROOT, check=True)
```

Extend `tests/test_js_parses.sh` with:

```bash
for f in dash/perezos/*.js; do
  node --check "$f"
done
```

- [ ] **Step 5: Run focused verification**

Run: `pytest -q tests/test_perezos.py && bash tests/test_js_parses.sh`

Expected: PASS; the Node output reports all core subtests passing and the shell test ends with `OK`.

- [ ] **Step 6: Commit the deterministic foundation**

```bash
git add dash/perezos/core.js tests/perezos/test_core.js tests/test_perezos.py tests/test_js_parses.sh
git commit -m "feat: add deterministic PerezOS animation core"
```

---

### Task 2: Authored Indexed Pixel-Art Manifest and Atlas

**Files:**
- Create: `dash/perezos/art.js`
- Create: `tests/perezos/test_art.js`

**Interfaces:**
- Consumes: `ComandOSPerezOS.Core` and a `canvasFactory(width, height)` supplied by the renderer or tests.
- Produces: `ComandOSPerezOS.Art.WORLD`, `CAMERAS`, `PALETTE`, `PARTS`, `PROPS`, `MASKS`, `buildAtlas(canvasFactory, theme)`, `compactCamera(stageWidth, stageHeight)`, and `validateManifest()`.

- [ ] **Step 1: Write manifest and rasterization tests**

Test exact anatomical coverage rather than only checking that a file exists:

```js
"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
global.window = global;
require("../../dash/perezos/core.js");
require("../../dash/perezos/art.js");
const A = global.ComandOSPerezOS.Art;

test("manifest provides the full authored PerezOS body", () => {
  assert.deepEqual(A.validateManifest(), []);
  assert.deepEqual(A.WORLD, {width:224, height:192});
  assert.ok(A.PARTS.length >= 40 && A.PARTS.length <= 50);
  assert.equal(A.PARTS.filter(part =>
    /^claw-(?:front-left|front-right|rear-left|rear-right)-[123]$/.test(part.id)).length, 12);
  for(const id of ["skull", "jaw", "muzzle", "nose", "neck-upper", "neck-mid",
      "neck-lower", "ribcage", "abdomen", "pelvis", "eye-left", "eye-right"]){
    assert.ok(A.PARTS.some(part => part.id === id), `missing ${id}`);
  }
  assert.ok(Object.keys(A.MASKS).includes("contact-belly"));
  assert.ok(Object.keys(A.PROPS).sort().join(",").includes("bufanda"));
});

test("every piece contains authored indexed clusters inside its bounds", () => {
  for(const part of A.PARTS){
    assert.ok(part.commands.length >= 2, `${part.id} has no authored clusters`);
    assert.ok(part.commands.every(command => ["px", "run", "rect", "poly"].includes(command[0])));
    assert.ok(part.pivot.length === 2 && part.bounds.length === 4);
  }
});
```

- [ ] **Step 2: Run the art tests and confirm the red state**

Run: `node --test tests/perezos/test_art.js`

Expected: FAIL because `ComandOSPerezOS.Art` is undefined.

- [ ] **Step 3: Author the complete indexed manifest**

Create 48 named pieces in `dash/perezos/art.js`. Use data commands rather than runtime ellipses so silhouettes and dithering remain hand-authored. The command contract is exact:

```js
const commandKinds = Object.freeze({
  px:   ["px",   "palette-index", "x", "y"],
  run:  ["run",  "palette-index", "x", "y", "length"],
  rect: ["rect", "palette-index", "x", "y", "width", "height"],
  poly: ["poly", "palette-index", "x1", "y1", "x2", "y2", "x3", "y3"],
});
```

Define the body ids as this stable set so rig, art, and tests share names:

```js
const BODY_IDS = Object.freeze([
  "pelvis", "abdomen", "ribcage", "neck-lower", "neck-mid", "neck-upper",
  "skull", "face-mask", "muzzle", "jaw", "nose", "eye-left", "eye-right",
  "lid-left-upper", "lid-left-lower", "lid-right-upper", "lid-right-lower",
  "arm-fl-upper", "arm-fl-fore", "wrist-fl", "palm-fl",
  "arm-fr-upper", "arm-fr-fore", "wrist-fr", "palm-fr",
  "leg-rl-upper", "leg-rl-lower", "ankle-rl", "palm-rl",
  "leg-rr-upper", "leg-rr-lower", "ankle-rr", "palm-rr",
  "claw-front-left-1", "claw-front-left-2", "claw-front-left-3",
  "claw-front-right-1", "claw-front-right-2", "claw-front-right-3",
  "claw-rear-left-1", "claw-rear-left-2", "claw-rear-left-3",
  "claw-rear-right-1", "claw-rear-right-2", "claw-rear-right-3",
  "fur-back", "fur-belly", "fur-head",
]);
```

Give every piece explicit parent, pivot, bounds, z group, palette-indexed clusters, and alternate `loaded`, `searching`, or `turned` command sets where required. Use at least 12 palette indices with warm-brown identity ramps and separate state-light indices. Define restrained authored prop layers for every existing input contract: `corona`, `casco`, `visor`, `fuego`, `hamster`, `gordo`, `huevo`, and `bufanda`.

- [ ] **Step 4: Implement deterministic in-memory atlas construction**

Implement `buildAtlas(canvasFactory, theme)` to allocate one power-of-two canvas, rasterize each command into a non-antialiased cell, and return stable rectangles without allocating during frame rendering:

```js
function buildAtlas(canvasFactory, theme){
  const canvas = canvasFactory(1024, 1024);
  const ctx = canvas.getContext("2d", {alpha:true});
  ctx.imageSmoothingEnabled = false;
  ctx.clearRect(0, 0, 1024, 1024);
  const palette = themedPalette(theme);
  const rects = Object.create(null);
  let x = 0, y = 0, rowHeight = 0;
  for(const part of PARTS){
    const width = part.bounds[2], height = part.bounds[3];
    if(x + width > 1024){ x = 0; y += rowHeight + 2; rowHeight = 0; }
    if(y + height > 1024) throw new Error("PerezOS atlas exceeds 1024x1024");
    drawCommands(ctx, part.commands, x, y, palette);
    rects[part.id] = Object.freeze({x, y, width, height,
      pivotX:part.pivot[0], pivotY:part.pivot[1]});
    x += width + 2; rowHeight = Math.max(rowHeight, height);
  }
  return Object.freeze({canvas, rects:Object.freeze(rects), palette});
}
```

`drawCommands()` must use only `fillRect`, integer coordinates, and an integer scanline polygon fill. It must never enable smoothing, shadows, filters, text, or fractional alpha.

- [ ] **Step 5: Validate art and payload structure**

Run: `node --test tests/perezos/test_art.js && wc -c dash/perezos/art.js`

Expected: PASS; manifest has 48 pieces and twelve claws. Record the raw source size for the later gzip budget test.

- [ ] **Step 6: Commit the complete neutral art system**

```bash
git add dash/perezos/art.js tests/perezos/test_art.js
git commit -m "feat: author layered PerezOS pixel art"
```

---

### Task 3: Anatomical Rig, Cable Physics, and Contact Solver

**Files:**
- Create: `dash/perezos/rig.js`
- Create: `tests/perezos/test_rig.js`

**Interfaces:**
- Consumes: `Core`, `Art.PARTS`, a 120-value target packet, and fixed `dt` in seconds.
- Produces: `Rig.CHANNELS`, `createRig(seed)`, `setChannelTarget(rig, name, value)`, `requestGrip(rig, limb, mode, cableT)`, `solveRig(rig, dt)`, `poseHash(rig)`, and `validatePose(rig)`.

- [ ] **Step 1: Write failing anatomy and physical-invariant tests**

```js
"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
global.window = global;
require("../../dash/perezos/core.js");
require("../../dash/perezos/art.js");
require("../../dash/perezos/rig.js");
const R = global.ComandOSPerezOS.Rig;

test("rig exposes 120 finite channels and twelve claw grips", () => {
  const rig = R.createRig(42);
  assert.equal(R.CHANNELS.length, 120);
  assert.equal(rig.values.length, 120);
  assert.equal(Object.keys(rig.claws).length, 12);
  assert.deepEqual(R.validatePose(rig), []);
});

test("loaded grip cannot release without safe transferred support", () => {
  const rig = R.createRig(42);
  assert.equal(R.requestGrip(rig, "front-left", "release", 0.4), false);
  R.requestGrip(rig, "rear-right", "loaded", 0.72);
  for(let i = 0; i < 240; i += 1) R.solveRig(rig, 1 / 120);
  assert.equal(R.requestGrip(rig, "front-left", "release", 0.4), true);
  assert.ok(Math.abs(Object.values(rig.supports).reduce((sum, c) => sum + c.load, 0) - 1) < 1e-5);
});

test("solver recovers from invalid targets to its last valid pose", () => {
  const rig = R.createRig(7);
  const before = R.poseHash(rig);
  R.setChannelTarget(rig, "spine-mid-angle", Number.POSITIVE_INFINITY);
  R.solveRig(rig, 1 / 30);
  assert.deepEqual(R.validatePose(rig), []);
  assert.equal(rig.diagnostics.recoveries, 1);
  assert.equal(R.poseHash(rig), before);
});
```

- [ ] **Step 2: Run rig tests and confirm the red state**

Run: `node --test tests/perezos/test_rig.js`

Expected: FAIL because `dash/perezos/rig.js` is missing.

- [ ] **Step 3: Define exact channel groups and joint limits**

Create `CHANNELS` by concatenating fixed groups for axial body, face, each limb, each claw, fur, cable, light, and props. Pad only with named authored channels such as `fur-head-07`; do not use anonymous numeric reserve slots. Freeze a `LIMITS` record for every channel and assert `CHANNELS.length === 120` during load.

The initial support set must be front-left and rear-right with loads summing to one. Each support stores:

```js
{limb:"front-left", mode:"loaded", cableT:0.34, load:0.58,
 point:{x:76, y:35}, normal:{x:0, y:-1}}
```

- [ ] **Step 4: Implement bounded cable and inverse-kinematic solving**

Use nine preallocated cable nodes. Pin endpoints, integrate internal nodes with Verlet state, apply gravity/body loads, then satisfy segment length eight times. Solve two-link limbs analytically and choose the elbow/knee branch declared by the art manifest. Clamp each target before solving.

```js
function solveTwoLink(root, target, upper, lower, bend){
  const dx = target.x - root.x, dy = target.y - root.y;
  const distance = Core.clamp(Math.hypot(dx, dy), Math.abs(upper-lower)+0.001, upper+lower-0.001);
  const base = Math.atan2(dy, dx);
  const shoulder = Math.acos(Core.clamp((upper*upper + distance*distance - lower*lower) /
    (2*upper*distance), -1, 1));
  const elbow = Math.acos(Core.clamp((upper*upper + lower*lower - distance*distance) /
    (2*upper*lower), -1, 1));
  return {upper:base + bend*shoulder, lower:bend*(Math.PI-elbow)};
}
```

Before committing a solved pose, validate finiteness, channel bounds, cable stretch, body overlap zones, support-load sum, and each loaded claw's maximum contact error. On failure copy `lastValidValues`, `lastValidCable`, and `lastValidSupports` back into live arrays and increment `recoveries`.

- [ ] **Step 5: Add long-energy and contact-continuity assertions**

Extend `test_rig.js` with a 30-minute synthetic cable swing using fixed `dt=1/60`; assert maximum segment stretch below 3%, finite total energy, and loaded claw error below one logical pixel throughout.

- [ ] **Step 6: Run rig and core suites**

Run: `node --test tests/perezos/test_core.js tests/perezos/test_art.js tests/perezos/test_rig.js`

Expected: PASS with no recovery except the explicitly corrupted-target test.

- [ ] **Step 7: Commit the physical body model**

```bash
git add dash/perezos/rig.js tests/perezos/test_rig.js
git commit -m "feat: add PerezOS anatomical contact solver"
```

---

### Task 4: Behavior Drives, Memory, and Generative Grammar

**Files:**
- Create: `dash/perezos/behaviors.js`
- Create: `tests/perezos/test_behaviors.js`

**Interfaces:**
- Consumes: deterministic seed, visible-time clock, normalized `PerezOSContext`, interaction events, and completion notices from Motion.
- Produces: `Behaviors.PRIMITIVES`, `createDirector(seed)`, `updateContext(director, context, nowMs)`, `notify(director, event, nowMs)`, `nextPerformance(director, nowMs)`, `completePerformance(director, performance, nowMs)`, and `performanceSignature(performance)`.

- [ ] **Step 1: Write failing priority, personality, and diversity tests**

Test same-seed equality, different-seed personality differences, no immediate family repeat, rare-action cooldown, `waiting`/`dead` preemption deadlines, and 10,000 distinct signatures across 512 session seeds.

```js
test("grammar reaches ten thousand valid idle signatures", () => {
  const signatures = new Set();
  for(let seed = 1; seed <= 512 && signatures.size < 10000; seed += 1){
    const d = B.createDirector(seed);
    B.updateContext(d, {sessionId:`s-${seed}`, status:"idle", role:"daily",
      costume:"", contextPressure:"low", theme:"noche", expanded:false}, 0);
    for(let i = 0; i < 96; i += 1){
      const p = B.nextPerformance(d, i * 61000);
      assert.ok(p.phases.every(phase => phase.durationMs > 0 && phase.safeEnd !== undefined));
      signatures.add(B.performanceSignature(p));
      B.completePerformance(d, p, i * 61000 + p.durationMs);
    }
  }
  assert.ok(signatures.size >= 10000, `only ${signatures.size} signatures`);
});
```

- [ ] **Step 2: Run the behavior test and confirm the red state**

Run: `node --test tests/perezos/test_behaviors.js`

Expected: FAIL because `ComandOSPerezOS.Behaviors` is undefined.

- [ ] **Step 3: Implement drives, stable personality, and short-term memory**

Use fixed records, bounded numeric drives, and ring arrays of the last eight families/sides/targets. Define personality from forked random streams without consuming the action stream:

```js
function createPersonality(rng){
  const p = rng.fork("personality");
  return Object.freeze({
    preferredSide:p.next() < 0.5 ? "left" : "right",
    blinkMs:Math.round(p.range(2800, 6100)),
    curiosity:p.range(0.25, 0.85),
    sleepBias:p.range(0.2, 0.9),
    gripCaution:p.range(0.55, 0.95),
  });
}
```

Drives are `sleepiness`, `curiosity`, `attention`, `gripConfidence`, `fatigue`, `comfort`, `boredom`, `satisfaction`, `alertness`, and `habituation`, each clamped to `[0,1]`.

- [ ] **Step 4: Implement the complete primitive registry and phrase grammar**

Register these exact primitive ids: `perceive`, `orient-gaze`, `refocus`, `blink`, `turn-head`, `breathe`, `brace`, `shift-weight`, `reach`, `search`, `open-grip`, `release`, `swing`, `touch`, `close-grip`, `pull`, `settle`, `stretch`, `scratch`, `groom`, `yawn`, `doze`, `wake`, `inspect`, `point`, `recoil`, `celebrate`, `comfort-cable`, `slip`, `recover`, and `neutral`. The `working` template advances carefully and inspects command-packet props; `waiting` points one free claw toward the notice; `done` relaxes and settles; `dead` checks the signal and curls safely; `idle` chooses from the full natural grammar.

Every primitive record declares `channels`, `duration`, `interruptible`, `safeEnd`, `precondition`, and `targets(params)`. Compose performances from state templates but vary side, gaze target, distance, grip, intensity, phase durations, pauses, head lead, and fur follow-through. Reject candidates conflicting with supports, cooldowns, or recent family memory.

Use priorities `dead=100`, `waiting=90`, `interaction=50`, `done=40`, `working=30`, `idle=10`. Attach `deadlineMs` of 8000 for dead and 5000 for waiting so Motion can choose the next safe cut.

- [ ] **Step 5: Run behavior coverage and determinism tests**

Run: `node --test tests/perezos/test_behaviors.js`

Expected: PASS, including at least 10,000 signatures and exact same-seed sequences.

- [ ] **Step 6: Commit the living behavior director**

```bash
git add dash/perezos/behaviors.js tests/perezos/test_behaviors.js
git commit -m "feat: generate PerezOS living behaviors"
```

---

### Task 5: Motion Phases, Channel Arbitration, and Secondary Fur

**Files:**
- Create: `dash/perezos/motion.js`
- Create: `tests/perezos/test_motion.js`

**Interfaces:**
- Consumes: a Rig instance, Behavior performance phrases, `dt`, and current time.
- Produces: `createMotion(rig)`, `enqueue(motion, performance, nowMs)`, `requestInterrupt(motion, performance, nowMs)`, `stepMotion(motion, dt, nowMs)`, `isIdle(motion)`, and completion records.

- [ ] **Step 1: Write failing phase and interruption tests**

Cover phase ordering, smooth target blend, no channel double ownership, `waiting` cut within five seconds, `dead` within eight, support transfer before release, completion callback, and bounded fur delay.

```js
test("urgent interruption waits for a safe phase boundary without missing deadline", () => {
  const {rig, motion} = fixture();
  M.enqueue(motion, longScratchPerformance(), 0);
  M.requestInterrupt(motion, waitingPerformance(), 1000);
  let started = null;
  for(let ms = 1000; ms <= 6000; ms += 16){
    M.stepMotion(motion, 0.016, ms);
    if(motion.current?.family === "waiting"){ started = ms; break; }
  }
  assert.ok(started !== null && started <= 6000);
  assert.deepEqual(R.validatePose(rig), []);
});
```

- [ ] **Step 2: Run motion tests and confirm the red state**

Run: `node --test tests/perezos/test_motion.js`

Expected: FAIL because `dash/perezos/motion.js` does not exist.

- [ ] **Step 3: Implement phase scheduling and channel ownership**

Store a fixed owner index and target for all 120 channels in typed arrays. A phase claims only declared channels; higher-priority behavior can request interruption but swaps only at `safeEnd` or deadline, inserting `brace` and `settle` phases when necessary. Blend targets with `Core.smoothstep(elapsed/duration)` and pass contact commands to Rig before limb-angle targets.

- [ ] **Step 4: Implement causal secondary motion**

Create one bounded spring per medium/fine fur channel plus jaw, abdomen, free wrists, and cable load. Drive these from parent acceleration and authored stiffness/damping; do not consume random values or add free noise:

```js
function updateSecondary(motion, dt){
  for(const link of motion.secondaryLinks){
    const parentAcceleration = motion.rig.acceleration[link.parentIndex];
    const target = motion.rig.values[link.parentIndex] + parentAcceleration * link.follow;
    motion.rig.targets[link.childIndex] = Core.stepSpring(link.spring, target, dt);
  }
}
```

- [ ] **Step 5: Run the motion/rig/behavior suite**

Run: `node --test tests/perezos/test_rig.js tests/perezos/test_behaviors.js tests/perezos/test_motion.js`

Expected: PASS; all interruption deadlines and pose invariants hold.

- [ ] **Step 6: Commit motion synthesis**

```bash
git add dash/perezos/motion.js tests/perezos/test_motion.js
git commit -m "feat: synthesize PerezOS anatomical motion"
```

---

### Task 6: Pixel Renderer and Adaptive Detail Policy

**Files:**
- Create: `dash/perezos/renderer.js`
- Create: `tests/perezos/test_renderer.js`

**Interfaces:**
- Consumes: Canvas, Art atlas, solved Rig pose, normalized theme, camera size, and quality name.
- Produces: `Renderer.QUALITY`, `createRenderer(canvas, options)`, `setViewport(renderer, width, height, dpr)`, `setTheme(renderer, theme)`, `render(renderer, rig, context, quality)`, `markDirty(renderer, reason)`, `destroyRenderer(renderer)`, and `rendererDiagnostics(renderer)`.

- [ ] **Step 1: Write failing fake-canvas renderer tests**

Build a fake context that records `drawImage`, `fillRect`, transform, smoothing, and clear calls. Assert one atlas build, integer transforms, deterministic z order, no draw when clean, correct Full/Balanced/Economy piece sets, compact camera under 180 by 148, and pose continuity through theme change.

```js
test("renderer uses only integer nearest-neighbor draws", () => {
  const fake = fakeCanvas(512, 416);
  const renderer = D.createRenderer(fake.canvas, {canvasFactory:fake.factory});
  D.setViewport(renderer, 256, 208, 2);
  D.render(renderer, fixtureRig(), fixtureContext(), "full");
  assert.equal(fake.context.imageSmoothingEnabled, false);
  for(const call of fake.draws){
    assert.ok(call.destination.every(Number.isInteger), JSON.stringify(call));
  }
});
```

- [ ] **Step 2: Run renderer tests and confirm the red state**

Run: `node --test tests/perezos/test_renderer.js`

Expected: FAIL because `ComandOSPerezOS.Renderer` is undefined.

- [ ] **Step 3: Implement cached atlas drawing and exact occlusion order**

Render in these groups: rear cable/shadow, rear limbs, axial deep fur, torso, front limbs, face, claws/contact masks, medium/fine fur, props, front cable, state rim light. Snap every pivot and destination to integers. Select authored alternate sprites from joint-angle and load bands rather than rotating a single sprite through large angles. `rendererDiagnostics()` must report atlas, typed-array, and retained-cache bytes as `decodedBytes` so the 16 MiB contract is directly testable.

Full draws all groups. Balanced merges fine fur into parent caches. Economy omits fine fur and dynamic dither but retains face, contacts, silhouette, and props. Static renders a safe authored pose only when context or viewport changes.

- [ ] **Step 4: Add dirty tracking and palette cache continuity**

`render()` returns `false` without touching the context when no pose, palette, prop, camera, or contact-mask revision changed. `setTheme()` rebuilds only palette-dependent cached atlas pages and does not mutate Rig or Motion.

- [ ] **Step 5: Run renderer and syntax suites**

Run: `node --test tests/perezos/test_art.js tests/perezos/test_renderer.js && bash tests/test_js_parses.sh`

Expected: PASS with no fractional destination coordinate and no redundant clean render.

- [ ] **Step 6: Commit the pixel renderer**

```bash
git add dash/perezos/renderer.js tests/perezos/test_renderer.js
git commit -m "feat: render PerezOS with adaptive pixel detail"
```

---

### Task 7: Public Controller, Scheduler, Observers, and Quality Governor

**Files:**
- Create: `dash/perezos/engine.js`
- Create: `tests/perezos/test_engine.js`

**Interfaces:**
- Consumes: Core, Art, Rig, Behaviors, Motion, Renderer, one canvas, and injectable browser environment.
- Produces: `window.ComandOSPerezOS.createPerezOS(canvas, options)` returning `setContext(context)`, `setVisible(visible)`, `setReducedMotion(reduced)`, `setViewport(width,height,dpr)`, `notifyInteraction(kind,x,y)`, `destroy()`, and `getDiagnostics()`.

- [ ] **Step 1: Write failing public lifecycle tests with a fake browser environment**

The fake environment records animation callbacks, observers, listeners, clock, and hot-loop buffer identities. Assert API shape, no duplicate listeners, same controller across context changes, 10 Hz pointer sampling, habituation, hidden/offscreen/preference pause, bounded resume, Static reduced motion, quality hysteresis, decode fallback, zero steady-state allocation counters, stable buffer identities, and idempotent destroy.

```js
test("hidden controller performs zero work and does not replay backlog", () => {
  const env = fakeEnvironment();
  const controller = E.createPerezOS(env.canvas, {env});
  controller.setContext(context("idle"));
  env.frames.advance(1000);
  const before = controller.getDiagnostics();
  env.document.hidden = true; env.document.emit("visibilitychange");
  env.frames.advance(600000);
  const hidden = controller.getDiagnostics();
  assert.equal(hidden.updates, before.updates);
  assert.equal(hidden.renders, before.renders);
  env.document.hidden = false; env.document.emit("visibilitychange");
  env.frames.advance(100);
  assert.ok(controller.getDiagnostics().updates - hidden.updates <= 4);
});
```

- [ ] **Step 2: Run engine tests and confirm the red state**

Run: `node --test tests/perezos/test_engine.js`

Expected: FAIL because `createPerezOS` is not exported.

- [ ] **Step 3: Implement the public controller and visible-time scheduler**

Validate context into this exact immutable shape:

```js
{
  sessionId:"session-name", status:"idle", role:"daily", costume:"",
  contextPressure:"low", theme:"noche", expanded:false, timestamp:0,
  colors:{brand:"#8B7CFF", panel:"#121722", line:"#222A3A"}
}
```

Use `requestAnimationFrame` only as wakeup. Advance behavior/physics at the active quality cadence, clamp one resumed step to 100 ms, and skip render when Renderer reports clean. `setContext()` emits differences into Behavior and never recreates rig, atlas, canvas, or observers unless `sessionId` changes; session changes reset director/motion/rig but retain atlas caches.

- [ ] **Step 4: Implement quality timing and hysteresis**

Use 240-sample fixed ring stats. Downgrade after 120 produced frames with average over 1.0 ms or p95 over 2.0 ms. Upgrade only after 600 frames with average below 0.65 ms and p95 below 1.25 ms. Never upgrade above the viewport/device ceiling or above Static under reduced motion.

Quality changes preserve pose, contacts, behavior phase, and random streams. Diagnostics returns quality transitions, solver recoveries, updates, renders, skipped-clean renders, observer/listener counts, and timing snapshots.

- [ ] **Step 5: Implement all stop conditions and teardown**

Running is `userVisible && !document.hidden && intersecting && !destroyed`. A false transition cancels the scheduled frame immediately. Register one visibility listener, one media-query listener, one intersection observer, and one resize observer. `destroy()` cancels all work, disconnects observers, removes listeners, destroys renderer, clears event queues, and is safe on repeated calls.

- [ ] **Step 6: Run all engine-level tests**

Run: `node --test tests/perezos/test_core.js tests/perezos/test_art.js tests/perezos/test_rig.js tests/perezos/test_behaviors.js tests/perezos/test_motion.js tests/perezos/test_renderer.js tests/perezos/test_engine.js`

Expected: PASS; hidden and destroyed update/render deltas remain zero.

- [ ] **Step 7: Commit the complete isolated engine**

```bash
git add dash/perezos/engine.js tests/perezos/test_engine.js
git commit -m "feat: orchestrate PerezOS lifecycle and quality"
```

---

### Task 8: Control Center Integration and Axolotl Removal

**Files:**
- Create: `dash/perezos/perezos.css`
- Create: `tests/perezos/test_integration.py`
- Modify: `dash/index.html:315-374,1045-1047,1108,1399,1785-2000,2060-2093,2135-2190`
- Modify: `dash/sw.js:3`
- Modify: `tests/test_dashboard_layout.py`
- Modify: `bin/cc-dash:720-721`
- Modify: `bin/cc-app:2538-2539`

**Interfaces:**
- Consumes: `ComandOSPerezOS.createPerezOS`, current `renderCentro(list)`, existing session item/status/model/role/costume/context fields, existing `toast()`, theme variables, and localStorage.
- Produces: persistent `CENTRO_VIEW.mascot` lifecycle, `perezosContext(it, role, costume)`, `mascotVisible()`, `applyMascotPreference()`, semantic `.perezos-stage` button/canvas, and updated localized copy.

- [ ] **Step 1: Write failing static integration tests**

Create `tests/perezos/test_integration.py` to assert scripts load in dependency order before the inline dashboard script, the CSS link exists, canvas/button semantics exist, preference migration reads `cc-axo` only in the migration function, and every old axolotl artifact is absent after integration.

```python
from pathlib import Path

HTML = Path("dash/index.html").read_text()


def test_perezos_runtime_loads_in_dependency_order():
    names = ["core", "art", "rig", "behaviors", "motion", "renderer", "engine"]
    positions = [HTML.index(f'/perezos/{name}.js') for name in names]
    assert positions == sorted(positions)
    assert positions[-1] < HTML.index("<script>")


def test_old_axolotl_runtime_is_fully_removed():
    forbidden = ["AXO_PIX", "AXO_MOVES", "axoAscii", "axoDraw", "axoIdleLoop",
                 "axo-ascii", "axobreathe", "Mascota ajolote", "ComandOS axolotl"]
    assert not [token for token in forbidden if token in HTML]


def test_engine_has_no_network_or_per_part_dom_api():
    source = "\n".join(path.read_text() for path in sorted(Path("dash/perezos").glob("*.js")))
    for token in ["fetch(", "XMLHttpRequest", "WebSocket", "appendChild(", "insertBefore("]:
        assert token not in source
```

Add a lifecycle assertion that `renderCentro()` calls `setContext()` without assigning `box.innerHTML` when only status/theme/costume changes for the same selected session.

- [ ] **Step 2: Run static integration tests and confirm the red state**

Run: `pytest -q tests/perezos/test_integration.py tests/test_dashboard_layout.py`

Expected: FAIL because the old axolotl and halo are still present.

- [ ] **Step 3: Add the integrated responsive stage CSS**

Load `/perezos/perezos.css` from the head. Define a 256 by 208 desktop stage and 180 by 148 narrow stage, transparent canvas, fixed cable anchor attached to the Control Center border, behind/front occlusion pseudo-elements, no halo, 2 px brand focus ring, and `image-rendering:pixelated`. Use `.no-mascot` to hide only the stage. Do not add CSS animation keyframes; Motion and Static policies own all continuous movement.

- [ ] **Step 4: Load classic scripts and migrate the preference**

Insert ordered, non-deferred local scripts immediately before the existing inline script. Replace axolotl preference code with:

```js
function migrateMascotPreference(){
  if(localStorage.getItem("cc-mascot") === null){
    localStorage.setItem("cc-mascot", localStorage.getItem("cc-axo") === "0" ? "0" : "1");
  }
}
function mascotVisible(){ return localStorage.getItem("cc-mascot") !== "0"; }
function applyMascotPreference(){
  document.body.classList.toggle("no-mascot", !mascotVisible());
  const button = $("#sw-mascot");
  if(button){ button.classList.toggle("on", mascotVisible());
              button.setAttribute("aria-checked", String(mascotVisible())); }
  CENTRO_VIEW.mascot?.setVisible(mascotVisible());
}
```

Rename the switch to `sw-mascot`, label it `Mascota PerezOS`, and keep `cc-axo` read-only for one-time migration.

- [ ] **Step 5: Refactor `renderCentro()` to keep one mascot controller alive**

Add a stable `CENTRO_VIEW = {sessionId:"", item:null, mascot:null, roleSig:""}`. Build the Control Center shell only when the selected session identity changes. Event handlers read `CENTRO_VIEW.item` rather than closing over a stale item. On same-session state/model/role/costume/theme changes, update text/buttons and call `CENTRO_VIEW.mascot.setContext(perezosContext(CENTRO_VIEW.item, role, costume))` without replacing the stage or canvas.

Mount semantic markup:

```html
<button type="button" class="perezos-stage"
  aria-label="PerezOS, mascota de la sesión seleccionada">
  <canvas class="perezos-canvas" width="224" height="192" aria-hidden="true"></canvas>
</button>
```

Activation calls `notifyInteraction("activate", 0, 0)` and shows one localized phrase. Pointer movement is converted to stage-local logical coordinates and passed to `notifyInteraction("pointer", x, y)`; Engine enforces 10 Hz.

When no selected session exists, destroy the controller and clear `CENTRO_VIEW`. Theme changes call `setContext()` with fresh colors instead of resetting the pose.

- [ ] **Step 6: Remove both axolotl implementations and update names**

Delete the unused procedural canvas builder, ANSI pixel map, palette conversion, `<pre>` renderer, random timers, movement arrays, CSS keyframes, halo styles, and axolotl phrases. Rename only comments in `bin/cc-dash` and `bin/cc-app`; keep backend `costume` values and JSON response fields unchanged.

Bump `SHELL` in `dash/sw.js` from `comandos-shell-v2` to `comandos-shell-v3`.

- [ ] **Step 7: Run integration, layout, parser, and existing dashboard tests**

Run: `pytest -q tests/perezos/test_integration.py tests/test_dashboard_layout.py tests/test_dashboard_cards.py tests/test_terminal_prefs.py && bash tests/test_js_parses.sh`

Expected: PASS; no old axolotl identifier is found and existing Control Center/session tests remain green.

- [ ] **Step 8: Commit product integration**

```bash
git add dash/index.html dash/sw.js dash/perezos/perezos.css tests/perezos/test_integration.py tests/test_dashboard_layout.py bin/cc-dash bin/cc-app
git commit -m "feat: integrate PerezOS into the control center"
```

---

### Task 9: Six-Hour Simulation, Browser Visuals, Accessibility, and Performance

**Files:**
- Create: `tests/perezos/test_long_run.js`
- Create: `tests/e2e_perezos.js`
- Create: `tests/test_perezos_e2e.py`
- Modify: `tests/test_perezos.py`

**Interfaces:**
- Consumes: all runtime units, injected deterministic clock, fake canvas for headless simulation, Playwright loader from `tests/e2e_mobile_remote.js`, and controller diagnostics.
- Produces: repeatable invariant report, pixel hash report, lifecycle report, accessibility report, and 30-second timing samples for Full idle/action scenarios.

- [ ] **Step 1: Write the six-hour simulation before changing runtime code**

Advance multiple seeds/statuses at fixed 30 Hz without sleeping. At every step assert finite channels, joint bounds, support-load sum, contact error, bounded cable energy, no stuck channel owner, cooldown integrity, and deadline response. At the end require 10,000 unique idle signatures and behavior-family coverage for every required primitive.

```js
test("six visible hours remain finite, supported, diverse, and bounded", () => {
  const report = simulate({seeds:[1,7,19,41,97,193,389,769],
    statuses:["idle","working","waiting","done","dead"], seconds:21600, hz:30});
  assert.equal(report.nonFinite, 0);
  assert.equal(report.invalidContacts, 0);
  assert.equal(report.deadlineMisses, 0);
  assert.equal(report.cooldownViolations, 0);
  assert.ok(report.maxCableStretch <= 0.03);
  assert.ok(report.idleSignatures >= 10000);
  assert.deepEqual(report.missingPrimitiveFamilies, []);
});
```

- [ ] **Step 2: Run the long simulation and fix only proven invariant failures**

Run: `node --test tests/perezos/test_long_run.js`

Expected initial result: either PASS or a precise invariant failure naming seed, simulated timestamp, pose hash, and channel/contact. Adjust limits, grammar preconditions, or recovery based on that evidence; do not loosen assertions to hide failures.

- [ ] **Step 3: Build the focused Playwright harness**

`tests/e2e_perezos.js` must import safely, load Playwright through `loadPlaywright()`, serve the `dash/` directory from a loopback Node HTTP server, and open a minimal harness document that loads all seven scripts plus CSS. Export helpers so `tests/test_perezos_e2e.py` can test import safety without launching a browser.

Run these browser scenarios at 1400 by 900:

1. Deterministic idle, working, waiting, done, dead, activation, slip recovery, theme change, and narrow camera; hash `getImageData()` and assert nontransparent bounds/unique palette counts.
2. Same canvas node and controller diagnostics across state/theme changes.
3. Click, Enter, Space, pointer habituation, and no event on a neighboring control.
4. `prefers-reduced-motion`, show/hide, offscreen intersection, page hidden, resize, and session change.
5. Ten-second warmup plus 30 seconds each of Full idle and Full action; assert average below 1.0 ms, p95 below 2.0 ms, decoded engine/atlas memory below 16 MiB, and steady-state allocation count equal to zero from controller diagnostics.
6. Destroy; assert zero callbacks, observers, listeners, updates, or renders afterward.

- [ ] **Step 4: Add the structured pytest browser wrapper**

Create `tests/test_perezos_e2e.py`:

```python
import json
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]


def test_perezos_browser_contract():
    if not shutil.which("node"):
        pytest.skip("node is not installed")
    run = subprocess.run(["node", "tests/e2e_perezos.js"], cwd=ROOT,
                         text=True, capture_output=True)
    if run.returncode == 77:
        pytest.skip(run.stderr.strip() or "Playwright is unavailable")
    assert run.returncode == 0, run.stdout + run.stderr
    report = json.loads(run.stdout)
    assert report["visualFailures"] == []
    assert report["lifecycleFailures"] == []
    assert report["accessibilityFailures"] == []
    assert report["performance"]["averageMs"] < 1.0
    assert report["performance"]["p95Ms"] < 2.0
    assert report["performance"]["decodedBytes"] < 16 * 1024 * 1024
    assert report["performance"]["steadyAllocations"] == 0
```

- [ ] **Step 5: Run simulation and browser verification**

Run: `pytest -q tests/test_perezos.py tests/test_perezos_e2e.py`

Expected: PASS, or the browser test is explicitly SKIPPED only when Playwright/Chromium is unavailable; the six-hour simulation may not skip.

- [ ] **Step 6: Commit regression and performance coverage**

```bash
git add tests/perezos/test_long_run.js tests/e2e_perezos.js tests/test_perezos.py tests/test_perezos_e2e.py
git commit -m "test: verify PerezOS life and performance"
```

---

### Task 10: Documentation, Payload Budget, and Full Regression Verification

**Files:**
- Modify: `DESIGN.md`
- Modify: `docs/superpowers/specs/2026-08-25-perezos-living-engine-design.md` only if implementation discovered a factual interface correction; requirements and budgets may not be weakened.

**Interfaces:**
- Consumes: final runtime diagnostics, committed file sizes, all repository tests, and the approved design specification.
- Produces: canonical PerezOS design-system guidance and final evidence that code, art, accessibility, lifecycle, and budgets match the approved design.

- [ ] **Step 1: Add a failing documentation/payload assertion**

Extend `tests/perezos/test_integration.py` with:

```python
import gzip


def test_perezos_payload_and_design_documentation():
    files = sorted(Path("dash/perezos").glob("*.js")) + [Path("dash/perezos/perezos.css")]
    compressed = sum(len(gzip.compress(path.read_bytes(), compresslevel=9)) for path in files)
    assert compressed < 750 * 1024
    design = Path("DESIGN.md").read_text()
    for phrase in ["PerezOS", "un solo canvas", "prefers-reduced-motion",
                   "píxeles enteros", "cero trabajo cuando está oculto"]:
        assert phrase in design
```

- [ ] **Step 2: Run the focused assertion and confirm documentation is red**

Run: `pytest -q tests/perezos/test_integration.py::test_perezos_payload_and_design_documentation`

Expected: FAIL because `DESIGN.md` does not yet define PerezOS.

- [ ] **Step 3: Document the shipped mascot contract**

Add a `PerezOS` section to `DESIGN.md` stating: the cable/panel composition, warm-brown theme identity, single-canvas rule, integer pixel scaling, status behaviors, no per-part DOM, reduced-motion Static behavior, zero work while hidden/offscreen/disabled, adaptive quality priorities, keyboard semantics, and prohibition against whole-sprite novelty transforms.

- [ ] **Step 4: Run placeholder, legacy-name, diff, and budget scans**

Run:

```bash
rg -n "T[B]D|T[O]DO|implement[[:space:]]later|fill[[:space:]]in|FIXME|XXX" dash/perezos tests/perezos tests/e2e_perezos.js DESIGN.md
rg -n -i "ajolote|axolotl|axo-ascii|AXO_PIX|AXO_MOVES|axoIdleLoop" dash/index.html dash/perezos bin/cc-app bin/cc-dash DESIGN.md
git diff --check
pytest -q tests/perezos/test_integration.py
```

Expected: the placeholder and legacy-name scans return no matches, `git diff --check` is silent, and integration tests PASS. The historical design specification may still use “axolotl” only when describing what is being replaced.

- [ ] **Step 5: Run the complete regression suite**

Run:

```bash
pytest -q
bash tests/test_js_parses.sh
bash tests/test_codex_adapters.sh
bash tests/test_ccx_agent.sh
bash tests/test_platform.sh
bash tests/test_doctor.sh
bash tests/test_install_hooks.sh
```

Expected: all available tests PASS; environment-dependent tests may report their existing explicit SKIP behavior, and no new failure is accepted.

- [ ] **Step 6: Inspect the mascot in every required mode**

Run the local dashboard through its normal project command, inspect desktop 1400×900, narrow 390×844, all five themes, all five session states, reduced motion, disabled preference, pointer/keyboard activation, and a 30-second Performance panel recording. Compare observations with controller diagnostics and the acceptance criteria in the design spec.

Expected: PerezOS remains attached to its cable/panel, never covers controls, shows no fractional pixel blur or pose snap, and the dashboard remains responsive.

- [ ] **Step 7: Commit documentation and any evidence-driven final corrections**

```bash
git add DESIGN.md docs/superpowers/specs/2026-08-25-perezos-living-engine-design.md dash/perezos dash/index.html dash/sw.js tests/perezos tests/test_perezos.py tests/test_perezos_e2e.py tests/e2e_perezos.js
git commit -m "docs: define PerezOS mascot contract"
```

- [ ] **Step 8: Record final evidence for handoff**

Capture the final commit list, `pytest -q` summary, shell-suite summaries, Playwright report, compressed payload bytes, decoded memory estimate, average/p95 render timings, quality level, and `git status --short`. Do not claim completion unless the actual outputs satisfy the approved budgets and every acceptance criterion.
