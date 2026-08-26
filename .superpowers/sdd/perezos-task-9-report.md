# PerezOS Task 9 Remediation Report

## Status

GREEN. This report covers Task 9 only. No Task 10 release or documentation work
was performed.

## Formal-review findings resolved

1. `body-lean-x` remains bounded to `[-2, 2]`, while `brace`, `shift-weight`,
   `pull`, `recoil`, and `recover` now author different nonlinear curves strictly
   inside that envelope. Focused tests prove that intensity/distance and side
   still change the requested target instead of collapsing at a clamp endpoint.
2. The long-run watchdog tracks continuous owner identity, phase, transition
   state, and first-owned time per channel. A phase owner is bounded by its
   declared `durationMs + 320 ms` interruption allowance and a transition owner
   by `160 ms`, each with one frame of tolerance. Failure diagnostics retain
   seed, timestamp, pose hash, and channel.
3. Long-run reporting now separates `idleSignatures` from `allSignatures` and
   behavior families from primitive families.
4. Performance uses a 2,048-entry preallocated trace, separate from the
   governor's retained 240-frame ring. Every scheduled update/render attempt is
   recorded with timestamp, combined/update/render cost, active/quiet state,
   and quality. Window slicing proves both temporal edges and every intervening
   sequence entry were retained.
5. The former allocation label is now the accurate
   `stableBufferReplacements`. Zero means the preallocated renderer, engine,
   motion, rig, timing, and trace buffers retained identity. Independent source
   audit verifies that trace writes allocate no arrays. Browser memory is
   reported separately as stabilized, bounded heap growth after CDP garbage
   collection; it is not described as zero heap allocations.
6. Each of `idle`, `working`, `waiting`, `done`, and `dead` is reconstructed from
   a fresh Rig/Director/Motion/Renderer using the same seed and fixed clock,
   repeated exactly, and required to have a causally distinct pixel hash.
7. The complete real dashboard exercises click, Enter, Space, the neighboring
   More control, the real `sw-mascot` preference/localStorage/body class, and a
   1400-to-600-pixel responsive resize. Canvas/controller/renderer identity and
   truthful switch accessibility are asserted. The harness remains only for
   deterministic internals and lifecycle seams.
8. Pause assertions capture the baseline and immediate result in the same
   JavaScript task. Preference and document visibility require zero immediate
   and sustained deltas. IntersectionObserver acknowledgement is measured as a
   distinct phase, after which sustained work must remain zero.
9. Exit 77 is restricted to a positively identified missing Playwright module,
   missing executable text, or executable `ENOENT`. Launch crashes and invalid
   flags fail normally.
10. The rest silhouette is authored as a suspended three-quarter sloth: loaded
    `front-left` and `rear-right` claws touch two upper cable points, the free
    rear-left limb bends away from the bottom edge, the pelvis remains below the
    supports, near/far contacts are diagonally asymmetric, and face regions are
    retained. Renderer ground shadow/strip artwork was removed. Tests use
    anchors, contact errors, regional alpha occupancy, silhouette bounds, and a
    zero-pixel floor band rather than subjective golden approval.
11. Static/reduced-motion rendering snapshots the same authored Rig cable,
    limbs, supports, and claws as one immutable safe pose. Its two loaded palms
    remain on the upper cable, its free rear limb remains suspended and
    asymmetric, and exactly two contact masks are emitted even if the live Rig
    subsequently changes.

## TDD evidence

### RED

- The first body-lean regression found `brace` still requested the exact `2`
  limit at high effort; all five primitives were re-authored inside the range.
- The posture regression found the free rear foot at `y=184` and a wide renderer
  floor shadow. It now ends at approximately `(53,168)` with normalized bend
  `-0.3953`; the renderer floor band is empty.
- The owner-age test initially failed because continuous age tracking did not
  exist. The regression now proves the exact declared phase allowance boundary
  and that a new phase resets identity age.
- Complete-trace tests initially lacked timestamps and active state and found
  only four buffers. The trace now has six preallocated buffers and an
  allocation-free scalar push path.
- A real Chrome action probe exposed `112` samples over five seconds
  (`22.2 Hz`) despite every sample being active. The cause was exact: two
  `16.6 ms` rAF intervals total `33.2 ms`, just below the `33.333 ms` Full action
  interval; resetting the accumulator discarded the remainder and waited for a
  third tick. A jitter regression failed at `116` rather than about `174`
  samples. The scheduler now subtracts one fixed interval while retaining the
  remainder.
- The first complete browser run also proved that repeated activation was not a
  sustained action load (`437` produced samples). The final load uses bounded
  10 Hz pointer sampling; an accepted pointer has a real 150 ms active tail, so
  safe input remains action cadence continuously without resetting Motion.
- The Static regression initially exposed a split source of truth: limbs and
  supports came from the safe Rig snapshot while the cable came from an older
  constant. The focused test failed because both loaded palms missed that old
  cable. Static now freezes the cable with the same Rig snapshot; the contact,
  suspension, asymmetry, and exact two-mask assertions pass.

### GREEN

The real five-second Chrome focal measured:

```text
samples=150 coverageMs=4983.2 cadenceHz=29.900
allActive=true allFull=true updates=150 renders=150 pointers=45
```

The final 30-second action window measured:

```text
samples=900 coverageMs=29965.5 cadenceHz=30.001168
allActive=true allFull=true pointerSamples=272
qualityTransitions=0 governorTransitions=0
```

## Six-visible-hour result

The simulation covers eight deterministic seeds, five statuses, and 21,600
visible seconds at 30 Hz: 5,184,000 externally checked frames. It does not sleep
or recover by weakening assertions.

```text
statusFrames: idle=1036800 working=1036800 waiting=1036800
              done=1036800 dead=1036800
nonFinite=0 invalidContacts=0 deadlineMisses=0
cooldownViolations=0 stuckOwners=0 maxOwnerAgeMs=2500
maxCableStretch=0.003769670581622058
maxContactError=6.355287432313019e-14
maxCableEnergy=22284.286831462567
idleSignatures=26480 allSignatures=69516
behaviorFamilies=31 primitiveFamilies=31 missingPrimitiveFamilies=[]
```

`node tests/perezos/test_long_run.js` completed 4/4 in 285.10 seconds;
the six-hour case itself took 281.39 seconds.

## Deterministic visual evidence

Fixed seed `task9-fixed-status` and fixed clock `12000` repeat exactly:

```text
idle=afda9ae2 working=f7a909b2 waiting=e4b042ad
done=c7fc7d94 dead=a5c8f830
```

The authored rest sample has 31.88% occupancy, bounds `146x176`, centroid
approximately `(106.66,89.10)`, 21 rendered colors, and zero nontransparent
pixels in the bottom floor band. Regional alpha counts are 874/1291 for the two
upper grip zones, 983 for the curled hind zone, and 3546 for the face zone.

- Full idle: `/tmp/perezos-task9-full-idle.png`, 6,249 bytes,
  SHA-256 `ede20a7f271b9123ce79732d8228722e018cebf8724a15e4445b4f2979725e21`
- Full action: `/tmp/perezos-task9-full-action.png`, 6,447 bytes,
  SHA-256 `cfff0df35133f644e02b123b525a0331b6f650cfbdba194b36fa534358ce07b1`

Both latest screenshots were inspected from disk after the pytest wrapper.
Chrome remained headless/background throughout.

## Browser performance and memory

Standalone final evidence, followed by a passing pytest browser wrapper:

```text
visualFailures=[] lifecycleFailures=[] accessibilityFailures=[]
averageMs=0.6135678417 p95Ms=0.8999999762
decodedBytes=8829470 stableBufferReplacements=0
heap baseline=3946184 afterIdle=4029240 afterAction=4120956
heap growth=174772 budget=2097152 bounded=true
idle samples=597 coverageMs=30015.4 allFull=true transitions=0/0
action samples=900 coverageMs=29965.5 allFull=true allActive=true
action cadenceHz=30.001168 transitions=0/0
```

The timing values include every scheduled end-to-end attempt and its update and
render-check components. The governor still uses its original produced-frame
ring of 240 samples and hysteresis behavior.

## Fresh verification

```text
node tests/perezos/test_long_run.js
  4 passed; 5,184,000 frames; 285.10s

node --test <all tests/perezos/test_*.js except test_long_run.js>
  205 tests passed; 0 failed; 11.62s

node --test tests/perezos/test_renderer.js
  35 tests passed; 0 failed; 0.52s (post-review Static regression)

pytest -q tests/test_perezos_e2e.py
  2 passed in 92.29s

pytest -q tests/test_dashboard_assets.py tests/test_dashboard_layout.py \
  tests/test_dashboard_security.py
  28 passed in 0.51s

bash tests/test_js_parses.sh
  OK

git diff --check
  clean
```

The long-run was not repeated inside `tests/test_perezos.py`; its exact Node
suite and every other PerezOS Node suite were run separately, preserving the
same non-skippable coverage without spending another simulated six-hour pass.

## Terminology and residual risk

- `behaviorFamilies` means composed narrative families such as
  `careful-advance`; `primitiveFamilies` means phase primitives such as
  `shift-weight`; signatures are reported as idle-only and all-status totals.
- `stableBufferReplacements=0` is an identity-stability counter, not a heap
  allocation count. Heap evidence is independently stabilized and bounded.
- Browser timing remains machine-dependent by design; the absolute approved
  gates remain average `<1 ms`, p95 `<2 ms`, decoded memory `<16 MiB`, and no
  Full-quality transition in either complete window.
- Pixel hashes prove deterministic causality, not aesthetic equivalence.
  Identity is protected by objective posture, contacts, regions, occupancy,
  palette, bounds, and face/silhouette checks.
