# PerezOS Task 5 Implementation Report

## Scope

Implemented Task 5 only:

- `dash/perezos/motion.js`
- `tests/perezos/test_motion.js`

The runtime exposes exactly `createMotion`, `enqueue`, `requestInterrupt`,
`stepMotion`, and `isIdle`. It consumes the immutable Behavior
`performance.phases` schema and the existing sealed Rig API without adding
fields to Rig.

## Implementation

- Preallocated `Int16Array` channel owners and `Float64Array` authored target,
  phase-start, base-target, prior-velocity, and acceleration state for all 120
  channels.
- Fixed eight-performance queue with a sealed eight-slot absolute-deadline
  buffer, plus a fixed sixteen-record completion ring.
- Sequential smoothstep phase blending with declared-channel validation and a
  single owner slot per channel.
- Monotonic timing, absolute phase-relative `safeEnd` handling, priority
  arbitration, waiting/dead deadlines, and deterministic brace/settle bridges
  for hazardous cuts.
- A fixed-bound segment/event loop carries frame remainder through transitions,
  safe cuts, performance completion, and multiple phase boundaries. Deadline
  events consume wall time to the exact deadline even when contact transfer
  freezes authored phase time.
- Dead-pose scheduling reserves the pending dead performance duration and the
  bridge duration so the completed safe pose lands within eight seconds. The
  original absolute terminal deadline survives queue/pending/current transfer;
  if a required contact is genuinely unreachable, Motion preserves valid
  supports and completes a deterministic neutral fallback at that exact time.
- Contact semantics are deliberately narrow: `close-grip` loads the selected
  front cable support; `open-grip` and `release` wait for Rig load transfer
  before release; `touch` and `comfort-cable` do not invent load-bearing
  contacts. All contact commands precede authored angle/target blending.
- Nineteen deterministic bounded secondary springs cover twelve fur channels,
  jaw, abdomen, four wrists (parent follow suppressed while loaded), and cable
  load. Parent acceleration is derived from preallocated prior-velocity state;
  no Rig topology change, RNG, free noise, or steady-step allocation is used.
- Fixed-capacity completion records and callback values are immutable scalar
  snapshots, including causal interrupted/completed status.
- Completion transactions append the immutable record, install the successor,
  and only then dispatch the contained callback. Nested stepping therefore sees
  post-transition state and cannot record the same terminal performance twice.
- Reentrant stepping is a bounded scalar transaction: nested calls advance the
  logical schedule but defer physics. Fixed scheduled-through and active-window
  scalars first consume a future nested start gap only where an enclosing step
  already covers it; uncovered wall gaps are skipped. The outer scheduler then
  subtracts processed overlap and consumes its remaining timeline, and the
  outermost call performs one Rig solve over the union of accepted intervals.
  Invalid or over-depth nested calls cannot steal that solve.
- A failed Rig solve rebases authored starts, causal velocity/acceleration
  buffers, and secondary springs to Rig's restored state without rewinding the
  logical phase or transition clock.

## TDD Evidence

### RED 1: missing module

Command:

`node --test tests/perezos/test_motion.js`

Expected result observed before runtime implementation:

- `MODULE_NOT_FOUND: ../../dash/perezos/motion.js`
- 0 passed, 1 failed
- Node duration: 66.002 ms
- `/usr/bin/time`: 0.10 s wall, exit 1

### GREEN 1: complete initial contract

Command:

`node --test tests/perezos/test_motion.js`

Result: 20/20 passed, Node duration 1676.986 ms, 1.71 s wall.

### RED/GREEN refinements

- Added a non-interruptible absolute-`safeEnd` regression and expanded the hot
  source audit through contact processing. Both failed for the expected old
  behavior; precomputed front-limb identities and safe-boundary scheduling made
  them green.
- Added monotonic rewind coverage. It failed because a rewound interrupt request
  shortened the absolute deadline; the effective monotonic time fix made it
  green.
- Added completed-dead-pose deadline coverage. It failed because dead began by
  eight seconds but completed after it; duration reservation made it green.
- Auxiliary review added five focused regressions. All five were RED against
  the previous scheduler: a safe boundary crossed in one frame was lost, two
  deadline paths recorded an early cut, multi-phase work discarded remainder,
  and a callback-created higher-priority interrupt was overwritten. Exact-event
  segment stepping and callback-safe pending-state ordering made all five GREEN
  (5/5, Node duration 145.676 ms).
- Re-review found one Minor snapshot edge: a pending cut at the final phase end
  reported both `interrupted` and every phase completed. The focused regression
  failed with that exact mismatch (1 failed, Node duration 66.492 ms); final
  phase completion now wins at the exact boundary and starts pending work there.
- Formal review added two Important findings and one Minor. Guarded nested-step
  callbacks were RED because they observed the old terminal performance; a
  genuine unreachable right-front contact produced no dead record by the
  absolute deadline; and poisoned Rig target/velocity rollback left Motion
  authored or causal state desynchronized. The combined regression run failed
  all 5 selected tests as expected (Node duration 188.520 ms). Post-transition
  callback dispatch, fixed absolute terminal deadlines with safe fallback, and
  allocation-free Rig-recovery rebasing made 7/7 focused cases GREEN (Node
  duration 219.590 ms).
- Follow-up recovery tests were RED 3/3 (Node duration 95.028 ms) because the
  first rebase restarted phase/brace clocks and the queue deadline buffer was
  not externally identity-checkable. Preserving elapsed clocks and exposing the
  sealed fixed buffer made those tests GREEN 3/3 (Node duration 79.752 ms).
- The final auxiliary review found two Important edge cases in that revision:
  ordinarily queued dead work carried a terminal deadline but was never
  promoted, and a callback's nested step solved the Rig before the outer step
  solved it again. The three selected regressions were RED 3/3 (Node duration
  205.068 ms). Fixed-capacity queued-dead promotion and a scalar reentrant-step
  transaction guard made the four focused callback/dead cases GREEN 4/4 (Node
  duration 190.287 ms), while the original queued terminal deadline remained
  authoritative.
- Corrected formal re-review found that the serial guard discarded the outer
  schedule remainder when a callback's nested step ended before the outer
  frame. Natural-completion and interruption regressions both failed at the
  successor's 10 ms pose instead of the full 100 ms remainder (RED 0/2, Node
  duration 66.470 ms). A fixed-depth scalar transaction now reconciles elapsed
  overlap, resumes the remainder, and defers the sole physical solve to the
  outermost call. Natural, interrupted, and invalid-nested control comparisons
  are GREEN 3/3 (Node duration 75.227 ms), including exact 100 ms records,
  equal non-reentrant Rig pose hashes, and one solve.
- Auxiliary re-review then found that a nested `[150,160]` step inside outer
  `[0,200]` still skipped the covered `100..150` gap during reconciliation. The
  natural and interruption variants were RED 0/2 (Node duration 76.920 ms),
  each advancing its successor only 50 ms. Ordered covered-gap consumption made
  those cases plus invalid nesting, uncovered-gap handling, and interrupt
  retention GREEN 5/5 (Node duration 86.448 ms). A separate nested
  `[250,260]` regression proves that the uncovered `200..250` wall gap is not
  replayed; its pose matches a non-reentrant 210 ms interval union exactly.

Latest post-review focused command:

`node --test tests/perezos/test_motion.js`

Result: 36/36 passed, Node duration 1894.851 ms, 1.92 s wall.

## Verification Before Auxiliary Review

- `node --test tests/perezos/test_rig.js tests/perezos/test_behaviors.js tests/perezos/test_motion.js`
  - 102/102 passed; Node duration 11447.731 ms; 11.47 s wall.
- `node --test tests/perezos/*.js`
  - 127/127 passed; Node duration 11559.093 ms; 11.59 s wall.
- `pytest -q tests/test_perezos.py`
  - 1/1 passed in 11.84 s; 12.54 s wall.
- `node --check dash/perezos/motion.js`
  - passed.
- `node --check tests/perezos/test_motion.js`
  - passed.
- `bash tests/test_js_parses.sh`
  - `OK`; 0.18 s wall.
- `pytest -q`
  - 385/385 passed in 124.93 s; 125.78 s wall.

## Post-fix Verification

- `node --test tests/perezos/test_motion.js`
  - 36/36 passed; Node duration 1894.851 ms; 1.92 s wall.
- `node --test tests/perezos/test_rig.js tests/perezos/test_behaviors.js tests/perezos/test_motion.js`
  - 118/118 passed; Node duration 11784.797 ms; 11.81 s wall.
- `node --test tests/perezos/*.js`
  - 139/139 passed; Node duration 11472.186 ms; 11.50 s wall.
- `pytest -q tests/test_perezos.py`
  - 1/1 passed in 11.42 s; 12.26 s wall.
- `node --check dash/perezos/motion.js`
  - passed; 0.02 s wall.
- `node --check tests/perezos/test_motion.js`
  - passed; 0.02 s wall.
- `bash tests/test_js_parses.sh`
  - `OK`; 0.20 s wall.
- `pytest -q`
  - 385/385 passed in 104.88 s; 105.70 s wall.

## Files Changed

- `dash/perezos/motion.js`
- `tests/perezos/test_motion.js`

## Self-review

- Confirmed there is one prior-velocity assignment per channel per step.
- Removed per-contact template-string allocation and extended the source audit
  so contact and advance helpers are covered.
- Confirmed forced interruption is checked before contact processing, so an
  unreachable transfer cannot defeat an urgent deadline.
- Confirmed Behavior phase `safeEnd` is consumed as an absolute millisecond
  offset, not as its original primitive ratio.
- Confirmed the original `.superpowers/sdd/task-5-report.md` remains byte-exact
  at SHA-256 `59e238c4d8f6f13fa1d280eeebdae95fffd1bfc348c1512b7aceebd349dd531a`.

## Auxiliary Review

The first read-only review found three Important scheduler timing issues and one
Minor reentrancy issue: lost crossed-safe boundaries, early deadline records,
dropped remainder at transition/phase boundaries, and callback-created pending
interrupt overwrite. Re-review confirmed all four fixed and found one remaining
Minor final-boundary record contradiction. That issue also received a failing
regression and is fixed. The same reviewer then returned a clean final verdict:
no Critical, Important, or Minor findings; focused verification 27/27.

Formal review after that commit found the callback transaction, unreachable
dead contact, and Rig-recovery issues described in the TDD evidence. All formal
findings are fixed. The next same-reviewer pass found queued-dead promotion and
nested-step double-physics issues; both received RED regressions and fixes as
described above. The same reviewer then re-reviewed the complete amended diff,
ran the focused Motion suite at 34/34, and returned a clean final verdict: no
Critical, Important, or Minor findings.

Corrected formal re-review then identified the dropped outer-remainder issue
described above. The same auxiliary reviewer subsequently identified the
future-start covered-gap issue; it also received RED regressions and the
ordered-window fix described above. The full verification matrix is green;
the same reviewer reran focused Motion at 36/36 and returned a clean final
verdict with no Critical, Important, or Minor findings.
