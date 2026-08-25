# Task 3 report: anatomical rig, cable physics, and contact solver

## Scope and result

Implemented Task 3 only in `dash/perezos/rig.js`, with its contract and regression suite
in `tests/perezos/test_rig.js`. The module consumes `ComandOSPerezOS.Core` and authored
`ComandOSPerezOS.Art.PARTS` geometry. No Task 4 work was started. The unrelated
`.superpowers/sdd/task-3-report.md` remains byte-exact to base commit `48a9ba9`.

## RED / GREEN record

- Initial RED: `node --test tests/perezos/test_rig.js` failed because
  `dash/perezos/rig.js` did not exist (`MODULE_NOT_FOUND`).
- Initial GREEN: the first complete rig implementation passed the original focused
  contract, including its deterministic 30-minute synthetic simulation.
- Boundary RED/GREEN: zero `dt`, malformed support/claw state, finite but incorrect
  bone lengths, non-unit normals, and an extra initial-support field each received a
  failing regression before their production fixes.
- Review RED/GREEN, Art-derived IK: tests independently derived all four root, joint,
  and end anchors from Art world bounds/pivots and exposed hardcoded lengths/branches.
  Private frozen chain configuration now derives both lengths and bend sign from each
  authored anchor chain. Released limbs solve exactly to authored rest endpoints.
- Review RED/GREEN, contact interpolation and release safety: reach-boundary tests
  exposed disagreement between grip admission and contact solving, plus unsafe summed
  split support. Admission and solving now share the exact biased cable sample; release
  requires one other loaded support individually at `0.95` or greater, forces released
  load to zero, and rejects a pose with no loaded support.
- Review RED/GREEN, body overlap: point-only checks allowed bone traversal and body
  endpoints. Art-derived axial zones now validate joints, ends, contacts, and both bone
  segments. A root attached within its own zone may exit once, but traversal, re-entry,
  and entry into any other zone fail.
- Review RED/GREEN, atomicity and immutable configuration: failures exposed omitted
  `lastDt` state, mutable public structure, invalid `transferGoal` dereference, extra
  topology keys, equal-valued nested reference replacement, frozen/non-writable object
  rollback, and null top-level validation. Private frozen configuration plus private
  typed snapshots now make rollback total and exception-safe, restore canonical nested
  identities, reject exact-topology violations, and leave only the recovery count
  changed by a failed solve.
- Review RED/GREEN, canonical hashing and hot loop: family-by-family mutations exposed
  missing authoritative state in `poseHash`; source and identity tests exposed dynamic
  claw lookup risk. Hashing now covers all physical evolution and validation families.
  Frozen claw groups/flat references are precomputed once, and steady-state solve
  helpers contain no dynamic claw keys or explicit container construction.
- An auxiliary review initially confirmed the first review cycle, after which formal
  re-review exposed five additional Important issues and one Minor source-contract gap.
- Formal re-review RED/GREEN, descriptor safety: direct non-writable root, support, claw,
  diagnostics, rig, and late-commit fields threw or permitted partial mutation. Every
  transition now performs allocation-free same-value write/read probes before mutation.
  Supported mutable corruption rolls back; impossible poisoned-and-non-writable state
  returns `false` without throwing or adding solver mutation.
- Formal re-review RED/GREEN, deep topology: symbol/non-enumerable extras, accessors,
  equal-valued nested replacements, and method shadowing were possible. Exact anatomy
  maps are now frozen; the rig, diagnostics, anatomy records, and writable points are
  sealed; structural fields/references are non-writable; authored rest points are frozen;
  and every public typed buffer is sealed while its indexed values remain mutable.
- Formal re-review RED/GREEN, canonical references/hash: equal-valued reference and
  descriptor attacks were not fully represented. Stable reference replacement is now
  impossible, `poseHash` carries explicit identity markers for every buffer/container/
  nested-reference family, and writability/topology integrity changes alter the hash.
- Formal re-review RED/GREEN, total validation: hostile top containers and finite but
  inconsistent targets/angles exposed validation gaps. `validatePose` is total for the
  full hostile-container matrix and validates all buffer types/lengths, private snapshot
  mirrors, references, topology, IK roots/targets/angles/segments, diagnostics, and live
  physical state. Internal preflight applies the same finite IK consistency rules.
- Formal re-review RED/GREEN, transfer/source contract: a goal naming a released support
  was accepted, and the allocation-source slice omitted `solveRig`. A transfer goal is
  now `-1` or a currently loaded support index; the source contract explicitly covers
  steady helpers, rollback, grip transitions, and the complete `solveRig` body.
- Self-review RED/GREEN additionally made the private recovery count monotonic and able
  to restore a corrupted public counter without affecting canonical pose hashing.
- Final formal review RED/GREEN, authored branch: a coherent reflected limb preserved
  lengths, end/contact points, angles, and topology while passing validation. Shared
  normalized cross-product validation now requires both end and target branches to
  match `-config.bend`, with finite, scale, and epsilon guards; the reflected pose fails
  public validation and atomically recovers in `solveRig`. Independent geometry checks
  assert the authored convention for all four chains.
- Final formal review RED/GREEN, allocation range: the source contract now begins at
  `cablePoint` and explicitly covers `cablePoint`, `solveLimb`, `solveAllLimbs`,
  `updateClaws`, rollback, `requestGrip`, and `solveRig`.
- Final GREEN: rig passes 56/56 tests (38 top-level plus 18 nested subtests); Core, Art,
  and Rig pass 77/77 together.

## Interface and exact counts

`ComandOSPerezOS.Rig` exports:

- `CHANNELS`, `CHANNEL_GROUPS`, and `LIMITS`
- `createRig(seed)`
- `setChannelTarget(rig, name, value)`
- `requestGrip(rig, limb, mode, cableT)`
- `solveRig(rig, dt)`
- `poseHash(rig)`
- `validatePose(rig)`
- `channelIndex(name)`

The 120 unique, meaningfully named channels are divided exactly as follows:

| Group | Count |
| --- | ---: |
| axial | 16 |
| face | 16 |
| limbs | 32 |
| claws | 24 |
| fur | 12 |
| cable | 8 |
| light | 6 |
| props | 6 |
| **Total** | **120** |

Every channel has a finite frozen `{min, max, default}` record. Live values, targets,
velocities, last-valid mirrors, cable state/previous state, and rollback snapshots are
preallocated, sealed `Float64Array` buffers. Four two-link limbs and twelve claws have
exact validated topology. Frozen maps and non-writable stable references prevent
replacement; sealed records prevent extra fields and accessor substitution. Public
structural mirrors are checked against module-private frozen configuration and cannot
redefine what the solver considers valid.

Art-derived chain configuration independently asserted by the tests:

| Limb | Root | Joint | Rest end | Upper | Lower | Bend |
| --- | --- | --- | --- | ---: | ---: | ---: |
| front-left | `(70, 71)` | `(64, 97)` | `(54, 135)` | `26.68332812825267` | `39.293765408777` | `-1` |
| front-right | `(144, 71)` | `(154, 97)` | `(166, 135)` | `27.85677655436824` | `39.84971769034255` | `-1` |
| rear-left | `(84, 136)` | `(81, 158)` | `(68, 184)` | `22.20360331117452` | `29.068883707497267` | `-1` |
| rear-right | `(133, 136)` | `(141, 158)` | `(156, 184)` | `23.40939982143925` | `30.016662039607265` | `1` |

## Physical and safety invariants

- Initial support is the safe loaded diagonal: front-left load `0.58` at cable position
  `0.34`, point `(76, 35)`, and rear-right load `0.42` at `0.72`, point
  `(150, 86.72222222222223)`. Loads sum exactly to one.
- A loaded grip releases only after another individual loaded grip reaches at least
  `0.95`; split `0.5 + 0.5` support is insufficient. Released supports always have
  zero load, and every valid pose retains at least one loaded support.
- Both grip admission and solving use the same cable interpolation with
  `cable-contact-bias`. Accepted boundary grips solve without recovery or contact error.
- Released limbs return to their immutable Art-derived rest endpoints. Loaded initial
  limbs are contact-solved to cable supports without altering authored chain geometry.
- Cable state uses nine Verlet nodes, pinned endpoints, immutable authored rest
  geometry, and eight distance-constraint iterations per solve.
- Finite targets are clamped to frozen limits. `dt` is bounded to `[0, 1/30]`; zero
  `dt` sanitizes target packets without advancing live dynamics.
- Complete Art-derived central overlap zones validate contact/end/joint points and both
  bone segments, including deliberate own-body attachment exit handling.
- Each limb's normalized root-to-joint/root-to-end and root-to-joint/root-to-target
  cross products must have the authored `-config.bend` sign. Degenerate, non-finite,
  and epsilon-close branch geometry is invalid.
- Candidate validation covers all channel/cable/support/limb/claw values, exact object
  topology and nested identities, support normals/load/modes, bone lengths/branches,
  contact consistency, energy/stretch, transfer state, and last-valid state.
- Any supported mutable invalid preflight or candidate restores every canonical live and
  last-valid field, including values/targets/velocities, cable/current previous/rest
  mirrors, supports, limbs, claws, transfer state, diagnostics, and `lastDt`; only
  `diagnostics.recoveries` increments. A field poisoned and then made non-writable is
  mathematically unrestorable, so the transition returns `false` without throwing or
  mutating any additional solver state.
- `poseHash` includes support normals; all claw state, points, and errors; transfer goal;
  limb targets, angles, and errors; cable/rest/previous state; last-valid state;
  `lastDt`; structural/topology markers; and physical diagnostics. Recovery count is
  deliberately excluded so an atomic rollback preserves the canonical pose hash.
- Steady-state `solveRig` preserves observable buffer, anatomy, and claw-group identity.
  It creates no object or array container and constructs no dynamic claw key. Frozen
  topology makes replacement cleanup unnecessary, so rollback is also allocation-free;
  diagnostics, hashing, and public validation may allocate as allowed.

## Long-run evidence and timing

Seed `31`, fixed `dt = 1/60`, 108,000 frames (30 simulated minutes), deterministic
sinusoidal `cable-wind` and `cable-pulse` targets:

- Wall time: `4771.652562 ms`
- Maximum segment stretch: `0.002081118867990858` (`0.2081118867990858%`), below 3%
- Maximum loaded contact error: `5.859285502108464e-14 px`, below 1 px
- Peak recorded cable energy: `12563.276735698373`, finite
- Unexpected recoveries: `0`
- Final deterministic pose hash: `07d51f3c`; the test's identical 108,000-frame replay
  matched it

## Verification

- `node --test tests/perezos/test_rig.js` — PASS, 56/56 in 12.17s.
- `node --test tests/perezos/test_core.js tests/perezos/test_art.js tests/perezos/test_rig.js`
  — PASS, 77/77 in 11.74s.
- `node --test tests/perezos/test_*.js` — PASS, 77/77 in 12.03s.
- `pytest -q tests/test_perezos.py` — PASS, 1 passed in 11.98s.
- `bash tests/test_js_parses.sh` — PASS (`OK`).
- Prior final `pytest -q` — PASS, 385 passed in 106.86s. It was not rerun for this final
  validation-predicate/source-contract-only delta: the exact branch regressions, full
  PerezOS Node suites, PerezOS pytest, syntax check, and 108,000-frame steady validation
  path all passed; retaining that run was explicitly permitted for this review cycle.
- `git diff --check` and staged diff check — PASS, no whitespace errors.

## Self-review and concerns

- Geometry, branch, topology, rollback, and hash tests intentionally compute or compare
  independent state rather than relying only on the implementation's pose hash.
- The deterministic cable model is expressed in logical pixels; any future load or Art
  geometry change should retain the 108,000-frame stretch/contact/recovery regression.
- The sealed/frozen construction prevents topology and stable-reference replacement.
  An external caller can still turn a writable dynamic field non-writable. The solver
  detects that before mutation and returns `false`; if the caller first poisons that
  field, the caller must recreate the rig because JavaScript cannot restore it.
- Valid steady-state solving and supported rollback remain allocation-free by contract;
  the source regression begins at `cablePoint` and explicitly includes limb/cable
  helpers, claw updating, rollback, `requestGrip`, and the complete `solveRig` body.
- The explicit 120-channel schema has no anonymous reserve slots. Presentation channels
  are for later rendering tasks and were not consumed in Task 3.
- No outstanding correctness concern was found within Task 3 scope.
