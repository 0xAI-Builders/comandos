# PerezOS Task 9 Implementer Report

## Status

DONE

## Scope delivered

- Added a deterministic six-visible-hour simulation over the exact eight seeds
  and five statuses at 30 Hz (5,184,000 checked frames). Every frame validates
  120 finite/bounded channels, ownership, normalized supports, loaded contact
  error, cable stretch/energy, and solver recovery; minute boundaries also run
  the full topology validator.
- Added exact waiting/dead response deadlines, rare-family cooldown checks,
  idle ownership release, behavior/primitive coverage, and a deterministic
  signature sweep. Invariant failures name seed, simulated timestamp, pose
  hash, and the affected channel/contact.
- Added an import-safe Playwright module that reuses `loadPlaywright()` from
  `tests/e2e_mobile_remote.js`, finds an existing npm-cached Playwright only as
  a loader fallback, binds an isolated server to `127.0.0.1:0`, and serves both
  the real complete dashboard and a focused seven-script PerezOS harness.
- Exercised Chrome at 1400x900 in headless/background mode. Scenarios cover
  Full idle, working, waiting, done, dead, activation, deterministic slip
  recovery, day-theme accents, narrow camera, canvas/controller/renderer
  identity, click/Enter/Space, neighboring-control isolation, pointer sampling
  and habituation, reduced motion, user hide, offscreen intersection, document
  visibility, no resume replay, resize continuity, session reset, and destroy.
- Added per-canvas FNV pixel hashes, transparent bounds, 8..32-color palette,
  and objective composition checks. The real-dashboard capture requires at
  least 8% nontransparent occupancy, at least 42% canvas width and 50% canvas
  height, and a centroid no farther than 22%/25% from canvas center. This is
  motivated by `/tmp/perezos-control-center-real.png` without creating a
  subjective golden-image dependency.
- Added real Full-mode performance measurement: 10-second warmup, 30-second
  idle sample, and 30-second repeated-action sample. The report exposes the
  separate update/render rings as well as combined engine timings.
- Added pytest wrappers for import safety, browser results, exact performance
  thresholds, and screenshot artifacts; the existing PerezOS wrapper now
  includes the non-skippable long simulation with a bounded timeout.

## TDD evidence

### Long-run RED

The first invariant run failed rather than recovering silently:

```text
seed=7 timestampMs=24133.333... poseHash=04a260f5 channel=shift-weight
seed=7 timestampMs=1184166.666... poseHash=7397ac3 channel=recover
```

Focused instrumentation showed right-limb endpoints entering an unauthorized
axial overlap zone when `body-lean-x` approached 2.96. The systemic authored
limit allowed `[-10, 10]`, although the valid renderer/solver grammar only
needed `[-2, 2]`. The fix narrows that Rig channel to `[-2, 2]`; two focused
regressions preserve both failures, and no invariant assertion was loosened.

### Long-run GREEN

```text
node --test tests/perezos/test_long_run.js
3 passed
```

Observed report:

```text
nonFinite=0 invalidContacts=0 deadlineMisses=0 cooldownViolations=0
stuckOwners=0 recoveries=0
maxCableStretch=0.003769670581622058
maxContactError=6.355287432313019e-14
maxCableEnergy=22284.286831462567
idleSignatures=69516 missingPrimitiveFamilies=[]
statusFrames={idle:1036800,working:1036800,waiting:1036800,
              done:1036800,dead:1036800}
```

### Browser/performance RED

The initial real Chrome sample measured approximately 1.21--1.23 ms average,
1.7 ms p95, and triggered Economy. Update/render instrumentation localized the
cost to rendering (about 0.63 ms/frame). A renderer regression then recorded
5,115 individual `fillRect` calls for one Full frame (RED).

The renderer now constructs thick limb bridges as preallocated integer row
runs and submits them in Canvas paths. The row buffers are retained, included
in decoded-memory accounting, audited for identity, and cleared on destroy.
The engine's preallocated timing rings separately measure update and produced
render work. The regression budget is below 1,500 individual `fillRect` calls;
the focused engine/renderer suite is 58/58 GREEN.

The visibility lifecycle initially sampled the cancellation transition itself.
The browser regression now waits for the state acknowledgement, establishes a
post-transition baseline, then requires exactly zero updates and renders for
1.1 seconds. Resume additionally bounds work to fresh elapsed visible time and
`maxStepMs <= 100`; it does not permit backlog replay.

## Browser result and artifacts

Latest standalone headless Chrome result:

```text
visualFailures=[] lifecycleFailures=[] accessibilityFailures=[] browser.errors=[]
Full maximum: average=0.8329166715 ms p95=1.3000000715 ms
idle update avg/p95=0.2600/0.5000 ms render=0.5692/0.9000 ms
action update avg/p95=0.2458/0.4000 ms render=0.5713/0.8000 ms
decodedBytes=8735822 steadyAllocations=0 quality=full
```

`steadyAllocations` is the measured change in the controller's typed hot-loop
buffer identity counter, not a fabricated constant. The source audit and
renderer stress test cover the intended allocation-free steady hot path; it is
not presented as a browser-heap-profiler measurement.

- Full idle: `/tmp/perezos-task9-full-idle.png` (6,223 bytes)
- Full action: `/tmp/perezos-task9-full-action.png` (6,411 bytes)
- Real dashboard sample: 27.08% occupancy, centroid (124.51, 102.82) in a
  256x208 canvas, bounds 151x200, 19 rendered colors.
- Seeded slip-recovery sample: hash `70d0c34e`, repeat-exact hash/bounds,
  33.68% occupancy, bounds 144x192, 21 rendered colors.

## Fresh verification

```text
pytest -q tests/test_perezos.py tests/test_perezos_e2e.py
3 passed in 391.73s

node --test tests/perezos/test_engine.js tests/perezos/test_renderer.js
58 passed

pytest -q tests/test_dashboard_assets.py tests/test_dashboard_layout.py \
  tests/test_dashboard_security.py
28 passed in 0.59s

bash tests/test_js_parses.sh
OK

git diff --check
clean
```

The pytest browser test may skip only when Playwright/Chromium is unavailable;
on this machine it ran and passed. The six-hour simulation cannot skip.

## Files changed

- `tests/perezos/test_long_run.js` (new)
- `tests/e2e_perezos.js` (new)
- `tests/test_perezos_e2e.py` (new)
- `tests/test_perezos.py`
- `tests/perezos/test_engine.js`
- `tests/perezos/test_renderer.js`
- `dash/perezos/rig.js`
- `dash/perezos/engine.js`
- `dash/perezos/renderer.js`

## Self-review findings

- Re-read the Task 9 checklist and inspected the complete production/test diff.
  All requested scenarios and exact thresholds have executable assertions;
  there is no Task 10 work.
- Renamed the renderer regression during review so it accurately claims a
  reduction in individual `fillRect` submissions, not all Canvas method calls.
- Confirmed there are no task debug prints, TODO/FIXME markers, sleeps in the
  simulation, subjective golden hashes, or unrelated tracked changes.
- Confirmed the report's zero-allocation wording is bounded to the real
  diagnostic being measured and does not claim a browser heap result.

## Residual risks

- Browser timings depend on machine and Chrome build; the test intentionally
  enforces the approved absolute Full-mode budgets on every available run.
- Pixel hashes document deterministic frames but are not golden snapshots;
  objective bounds, palette, transparency, occupancy, and repeated seeded
  recovery provide less-fragile visual regressions.
- No Task 10 documentation or release work was performed.
