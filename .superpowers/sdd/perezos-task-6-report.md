# PerezOS Task 6 Implementation Report

## Scope

Implemented and remediated Task 6 only:

- `dash/perezos/art.js`
- `dash/perezos/rig.js`
- `dash/perezos/renderer.js`
- `tests/perezos/test_art.js`
- `tests/perezos/test_rig.js`
- `tests/perezos/test_renderer.js`

No Task 7 controller, HTML, CSS, or dashboard integration was added.

## Implementation

- Exact frozen eight-name `ComandOSPerezOS.Renderer` API with Full, Balanced,
  Economy, and Static policies.
- One lazily retained Art atlas page per visited theme. Repeated draws and theme
  reuse do not rebuild pages; theme changes rebuild only palette-dependent atlas
  and fine-detail cache pages.
- Real Canvas 2D nearest-neighbor rendering using integer source rectangles,
  integer destinations, integer pivots, and no canvas rotation.
- Exact deterministic occlusion groups: rear cable/shadow, rear limbs, axial
  deep fur, torso, front limbs, face, claws/contact masks, medium/fine fur,
  props, front cable, and state rim light.
- Solved Rig values, hierarchical body offsets, limb roots/joints/endpoints,
  claw points, support points, and all nine cable nodes drive visible geometry.
  Authored `loaded`, `searching`, and `turned` atlas cells are selected from
  load, contact, context, and joint-angle bands instead of large runtime
  rotation.
- Props retain their authored parent relationship, including palm-attached
  props following solved endpoints.
- Full adds retained fine detail plus bounded authored dither; Balanced uses the
  retained merged detail page; Economy keeps anatomy, face, contacts,
  silhouette, medium fur, props, cable, and rim while omitting fine/dynamic
  detail; Static ignores live pose changes and uses a safe authored pose.
- Authored compact camera selection occurs in logical pixels before DPR is
  converted to a positive integer backing-store scale.
- Dirty tracking compares retained typed visual-state snapshots and primitive
  visual context fields. A clean render returns `false` before any Canvas 2D
  property assignment or call.
- The steady render path uses retained typed scratch/snapshot buffers and has
  no explicit array, object, map, set, or typed-array allocation.
- Diagnostics account separately for atlas bytes, retained cache bytes, typed
  arrays, revisions, detail policy, group counts, renders, and clean skips.
  Two retained theme pages remain below the 16 MiB decoded-memory ceiling.
- Hostile inputs return `false` without Canvas mutation; destruction releases
  retained pages and buffers and is idempotent.

## Post-review remediation

The first formal visual review rejected the implementation after inspecting a
real Chrome capture. Although its unit suite was green, the rendered animal was
a collection of detached rectangular masses and several scanned channels had no
visible effect. The browser also exposed a separate construction failure:
Chrome throws when `Object.seal` is applied to a populated typed array.

The remediation was driven by focused RED tests:

- the authored safe silhouette measured 7,270 pixels across 16 disconnected
  non-claw masses;
- pelvis translation did not propagate to the abdomen;
- fractional DPR 1.25 produced a 320-pixel backing width instead of a stable
  256-pixel logical raster;
- a Chrome-compatible `Object.seal` probe threw during `Rig.createRig`.

The resulting GREEN implementation:

- rebuilds the sloth atlas around one connected organic body silhouette with
  asymmetric face patches, fur tufts, belly volume, thicker long limbs, and
  differentiated near/far anatomy;
- draws integer-pixel limb bridges through solved root, joint, and claw points,
  so live IK cannot visually separate the anatomy;
- applies cumulative pelvis/spine/neck transforms and makes chest, belly,
  brow, cheek, fur, independent claw, prop-secondary, and lighting channels
  visibly affect output;
- attaches three retained detail cells to head, back, and belly parents and
  uses authored Full-quality dither instead of a world-fixed overlay;
- uses solved limb angles for atlas alternates and places the static cable at
  the authored right-claw contact;
- floors fractional DPR before allocating the nearest-neighbour backing store
  and atomically restores viewport dimensions after hostile setters;
- replaces the unsupported typed-array sealing operation with
  `Object.preventExtensions`, preserving writable physics elements while
  preventing topology changes;
- removes the unmeasured `hotLoopAllocations: 0` diagnostic rather than
  presenting a synthetic performance claim.

## TDD Evidence

### Mandatory RED: missing renderer module

Command:

`node --test tests/perezos/test_renderer.js`

Observed before creating production code:

- 0 passed, 1 failed.
- Exact assertion: `ComandOSPerezOS.Renderer is undefined`.
- Node duration: 57.559 ms.

### Initial GREEN

Command:

`node --test tests/perezos/test_renderer.js`

Result after the first implementation: 19/19 passed, Node duration 209.883 ms.

### RED/GREEN refinements

- Added solved body/limb/face/cable/contact regressions before implementation;
  the first GREEN proved those paths consume actual Rig geometry.
- A refactor review identified temporary arrays reachable through `scanPose`,
  per-frame `restAnchor` objects, unselected contact masks, static palm-prop
  attachment, and DPR-before-camera selection. Six focused assertions failed
  for those exact reasons (17/23 passing, Node duration 255.619 ms).
- Scalar snapshot scanning, precomputed rest anchors, loaded-support/state mask
  selection, solved parent anchors, and logical-first compact-camera selection
  made the expanded renderer suite GREEN at 23/23 (Node duration 263.703 ms).

## Verification

### Initial implementation

- `node --check dash/perezos/renderer.js`
  - passed.
- `node --test tests/perezos/test_renderer.js tests/perezos/test_art.js tests/perezos/test_rig.js`
  - 95/95 passed; Node duration 11488.011 ms.
- `node --test tests/perezos/test_*.js`
  - 162/162 passed; Node duration 11607.017 ms.
- `bash tests/test_js_parses.sh`
  - passed with `OK`; elapsed 0.21 s.
- `pytest -q tests/test_perezos.py`
  - 1/1 passed; pytest duration 11.39 s.
- `pytest -q`
  - 385/385 passed; pytest duration 105.54 s.

### Post-review remediation

- `node --test tests/perezos/test_art.js tests/perezos/test_rig.js tests/perezos/test_renderer.js`
  - 102/102 passed; Node duration 11399.098 ms.
- `node --test tests/perezos/test_*.js`
  - 169/169 passed; Node duration 11458.654 ms.
- `bash tests/test_js_parses.sh`
  - passed with `OK`.
- `pytest -q`
  - 385/385 passed; pytest duration 105.54 s.
- Chrome 146 headless, without compatibility shims:
  - document reached `data-ready="true"` with no construction failure;
  - 55 drawn layers, 4.17 MiB decoded memory;
  - 0.530 ms/frame mean over 1,000 forced Full-quality redraws;
  - capture: `/tmp/perezos-renderer-unshimmed.png`.

## Files Changed

- `.superpowers/sdd/perezos-task-6-report.md`
- `dash/perezos/art.js`
- `dash/perezos/renderer.js`
- `dash/perezos/rig.js`
- `tests/perezos/test_art.js`
- `tests/perezos/test_renderer.js`
- `tests/perezos/test_rig.js`

## Self-review

- Confirmed all public Canvas operations used by the renderer are standard
  Canvas 2D methods/properties and the fake implements the same signatures.
- Confirmed every visible source/destination/fill/transform coordinate is an
  integer and no whole-part rotation is used.
- Confirmed palette changes do not write to Rig or Motion state and destination
  continuity is byte-for-byte equal in the fake draw log.
- Confirmed clean renders do not touch the Canvas context.
- Confirmed all renderer-owned decoded buffers/pages are included in
  `decodedBytes` and are released on destroy.
- Confirmed the original `.superpowers/sdd/task-6-report.md` remains byte-exact
  at SHA-256 `945ed1945b6f5aad887e2700b11131402b361d90d45325dc033c21efd99599dc`.

## Auxiliary Review

The parent SDD coordinator reserved the required fresh read-only formal reviewer
for the post-commit task gate. No implementer-side review was substituted for
that independent review.
