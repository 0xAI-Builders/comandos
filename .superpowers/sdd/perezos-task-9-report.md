# PerezOS Task 9 Remediation Report

## Status

GREEN after the strict visual/static/buffer remediation. This report covers
Task 9 only; no Task 10 release work was performed.

## Review findings resolved

1. The authored rest pose is now an unambiguously suspended three-quarter
   sloth. Its skull-to-pelvis line is `36x94` pixels at `69.044°`; head,
   shoulder, and pelvis advance diagonally; `front-left` and `rear-right` grip
   two separated points on the same cable; and the released rear-left limb
   folds through `(143,119) -> (102,143) -> (129,163)` into a visible C below
   the rump without reaching the floor.
2. Near-side wrist, palm, ankle, and free-hind artwork was re-authored as ragged
   indexed fur clusters rather than stacked solid rectangles. Continuous
   retained renderer bridges join the clusters along the IK bones. The final
   rendered raster measures `11,052 / 11,081 = 99.7383%` of opaque pixels in
   one eight-connected component.
3. The two contact masks now derive their origin from the actual loaded cable
   points. The obsolete fixed mask positions that produced isolated dark
   rectangles near the panel edge were removed. The authored sample has a
   bottom bound of `169` and zero pixels in the `y >= 176` floor band.
4. Static/reduced-motion rendering uses one immutable authored Rig snapshot for
   cable, supports, limbs, and claws. Across `idle`, `working`, `waiting`,
   `done`, and `dead`, only the physically loaded front-left and rear-right
   supports select `@loaded` artwork and exactly two contact masks. Status text
   can no longer relabel all twelve claws.
5. `stableBufferReplacements` now audits every retained typed hot-loop buffer,
   not a representative subset: Rig `14`, Motion `7`, Renderer `5`, and Engine
   timing/trace `12`, total `38`. Engine identity reads use the authoritative
   nested fields directly: the `values` and `scratch` buffers of all three
   timing rings, plus `timestamp`, `combined`, `update`, `render`, `active`, and
   `quality` on the performance trace. Trace `cursor`, `count`, and
   `totalSamples` are scalars; `sequenceStart`/`sequenceEnd` are derived scalar
   diagnostics, not buffers. The counter means identity replacement only.
   Retained browser heap growth remains a separate CDP measurement.
6. The visual harness derives pelvis/skull/ribcage geometry from the Art
   manifest rather than obsolete hard-coded coordinates. It now checks torso
   angle, two cable contacts, rear-hook geometry and margin, regional alpha,
   rendered connectivity, and the absence of floor pixels.
7. Primary narrative families are now an explicit frozen contract derived from
   the authored template registries: `19` idle, `3` working, `2` waiting, `3`
   done, `2` dead, and `3` interaction families (`32` total). The five long-run
   statuses require `29`; the final run covered all `29`, plus two declared
   safe fallbacks. All `31` primitive families were also covered.
8. The long-run rejects at `21,700 ms`, `245,933.333 ms`, and `55,400 ms`
   were traced to the old rectangular pelvis collision box filling transparent
   chamfers in the pixel art. The pelvis now authors five frozen collision
   rectangles matching its opaque row spans. Point, segment, endpoint, and
   contact rejection remain active against their union; no channel, cable,
   contact, stretch, or collision budget was relaxed.

## TDD evidence

### RED

- The stronger posture regression initially measured an upright skull/pelvis
  axis and a rear limb that did not form the required suspended C.
- A renderer regression proved the old contact masks did not cover either
  physical cable point and left a rectangle in the floor band.
- The Static working regression initially selected loaded variants from global
  status instead of the two loaded supports.
- The buffer regression initially found only the previously sampled
  Rig/Motion/Renderer identities rather than all retained typed buffers.
- Replacing each of the twelve authoritative Engine arrays individually first
  left `stableBufferReplacements` unchanged because the audit read mutable
  top-level aliases. The GREEN implementation snapshots and compares only the
  nested owners, outside allocation-producing hot-path constructs.
- `NARRATIVE_FAMILIES` was initially absent, and the coverage helper could only
  report observed cardinality. The GREEN contract asserts exact per-state
  names and proves that removing `doze` is reported as a primary-family loss.
- Atlas-only connectivity found several intentionally separated joint tufts.
  This was not hidden with an aesthetic assertion: atlas mass is bounded to one
  dominant body plus small bridge-owned tufts, while the browser measures the
  assembled final raster directly.
- The final Chrome run exposed `startLagMs=-0.5` because an rAF timestamp is
  captured at frame start and can precede a `performance.now()` baseline taken
  before that frame is committed. A focused test failed first. Coverage now
  intersects trace timestamps with the measured browser window, so it never
  overstates coverage or reports a negative boundary lag.

### GREEN

```text
torso dx=36 dy=94 angle=69.044223°
loaded supports=front-left,rear-right; contactError<=7.11e-15
free hind root=(143,119) joint=(102,143) end=(129,163) bend=-0.919630
rendered largest component=11052/11081 (99.7383%)
upper grip regions=533/1924 curled-hind=1662 face=3208 floorBand=0
```

The five fixed-seed/fixed-clock status renders repeat exactly and remain
causally distinct:

```text
idle=dc0f88bc working=6aebdd6a waiting=b5a96ed7
done=441aee81 dead=2bee3f97
```

## Current visual evidence

- Full idle: `/tmp/perezos-task9-full-idle.png`, `6,240` bytes,
  SHA-256 `e002d3928af77b9766878af4d46d41b625bd4e64439d7604862845270484e156`
- Full action: `/tmp/perezos-task9-full-action.png`, `6,523` bytes,
  SHA-256 `38ece485961c8d59013b5a342689560d1eb58d4d94d3f6e13c95742942804379`

Both 1400x900 captures were regenerated by the final headless Chrome run and
inspected from disk. The harness defaults to no costume so props do not conceal
the morphology; prop rendering remains covered independently.

## Browser performance and memory

The final Chrome run measured:

```text
visualFailures=[] accessibilityFailures=[]
averageMs=0.638796 p95Ms=0.900000 decodedBytes=8829470
stableBufferReplacements=0
stableBufferAudit={rig:14,motion:7,renderer:5,engine:12,total:38}
heap baseline=3912500 afterIdle=4030272 afterAction=4109148
heap growth=196648 budget=2097152 bounded=true
idle samples=598 coverageMs=30048.8 allFull=true transitions=0/0
action samples=902 raw-overlap coverageMs=29981.7 allFull=true allActive=true
action cadenceHz=30.051164 pointerSamples=272 transitions=0/0
```

The run's pre-fix report contained one lifecycle string solely for the
`-0.5 ms` raw start lag described above. Every substantive lifecycle condition
was green. Per the instruction to perform one final long browser/capture run,
the 69-second run was not repeated; the corrected window-intersection function
is covered by a focused RED/GREEN regression and makes the same recorded trace
pass without changing any budget.
- The current Art/IK geometry exposed three deterministic collision REDs:
  seed 1 recoil at `21,700 ms`, seed 7 transition/celebrate at
  `245,933.333 ms`, and seed 1 settle at `55,400 ms`. Failed-state
  instrumentation showed the front-right lower segment touching only
  transparent atlas padding. Exact focused replays are now GREEN against the
  authored opaque collision runs.

## Verification

```text
node --test <all tests/perezos/test_*.js except test_long_run.js>
  211 passed; 0 failed; 12.945s

node --test --test-name-pattern='through recoil sag|through settle sag|seed 7 idle recovery' \
  tests/perezos/test_long_run.js
  3 passed; 0 failed; 4.075s

node --test tests/perezos/test_long_run.js
  7 passed; 0 failed; 327.749s
  5,184,000 frames; idle signatures=26,480; all signatures=69,516
  primary narrative families=29/29; primitive families=31/31
  maxCableStretch=0.003769670581622058
  maxContactError=6.550880341146756e-14
  maxCableEnergy=22284.286831462567
  nonFinite=invalidContacts=deadlineMisses=cooldownViolations=stuckOwners=0

node --check <all changed PerezOS JavaScript>
bash tests/test_js_parses.sh
  all parsed

pytest -q tests/perezos/test_integration.py tests/test_dashboard_layout.py \
  tests/test_perezos_e2e.py::test_perezos_browser_harness_imports_without_launching
  21 passed; 0 failed; 0.16s

git diff --check
  clean
```

The light Node set includes the current 30-minute deterministic cable swing.
The final five-minute `test_long_run.js` evidence is from the current tree and
is stored verbatim at `/tmp/perezos-task9-review-long-run-final.tap`. The two
earlier current-turn full attempts are retained as failing diagnostic evidence,
not represented as GREEN: recoil at `21,700 ms` and settle at `55,400 ms`.

## Terminology and residual risk

- `stableBufferReplacements=0` is an exact identity-stability result for the
  named 38 retained typed buffers, not a zero-allocation claim. Every one of
  the twelve Engine arrays has an individual replacement-counter regression.
- Narrative totals distinguish the explicit primary template contract from
  safe runtime fallbacks: the final run had no missing primary family and two
  additional safe fallback families.
- Heap evidence is independently garbage-collected and bounded; current growth
  is `196,648` bytes against a `2,097,152` byte budget.
- Browser timing remains machine-dependent. The unchanged gates are average
  `<1 ms`, p95 `<2 ms`, decoded memory `<16 MiB`, complete 30-second trace
  coverage, and no Full-quality transition.
- Pixel hashes prove deterministic causality, not taste. Morphology is guarded
  by independent geometry, contact, floor-margin, region, and final-raster
  connectivity checks; the two screenshots remain the human visual evidence.
