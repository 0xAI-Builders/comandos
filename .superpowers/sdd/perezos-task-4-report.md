# PerezOS Task 4 Report: Behavior Drives, Memory, and Generative Grammar

## Scope

Implemented Task 4 only in:

- `dash/perezos/behaviors.js`
- `tests/perezos/test_behaviors.js`

No Art or Rig files were changed, and Task 5 was not started. The pre-existing
`.superpowers/sdd/task-4-report.md` remained unchanged at SHA-256
`568b9a4f7e8f178138585bf73542716c3d571a3d72f63851a9aa371e1b02fd7b`.

## RED

The first harness attempt used `require.resolve` on the intentionally absent module and failed
at module resolution. I corrected the test harness before treating it as the task RED.

The authoritative initial RED was:

```text
node --test --test-name-pattern='behavior namespace is installed' tests/perezos/test_behaviors.js
not ok 1 - behavior namespace is installed
AssertionError: ComandOSPerezOS.Behaviors must be defined
```

Additional focused RED cycles caught real contract gaps before their fixes:

- `point` emitted claw-spread targets without declaring those channels.
- `packet-refocus` did not include a careful weight-shift advance.
- A waiting fallback could emit `point` while both front supports were loaded.
- A waiting `point` pose curled all three digits instead of extending one.
- An interaction created at the saturated visible-time cap expired at that same numeric instant.
- A gaze-only interaction could look away from its loaded-side event because another candidate
  in the set required a free side.
- A saturated absolute expiry kept an interaction alive at exactly 12,000 ms of age.
- Candidate filtering consulted only the most recently selected family instead of the completed
  last-eight family ring.
- Phrase-level duration scaling could push `actionMs` outside primitive metadata bounds.
- Rare cooldowns armed only on completion, so preemption could bypass the reservation.
- Equal-time interaction records used slot order instead of newest insertion order after wrap.
- Repeated-target habituation included the exact 12,000 ms boundary while event expiry excluded it.
- Recent-family relaxation ran before full primitive preconditions, allowing an unstable-support
  interaction to relax into an invalid `recoil` fallback.

## GREEN and Refactor

The finished implementation provides:

- exactly 31 primitive IDs, each with frozen `channels`, `duration`, `interruptible`,
  `safeEnd`, `precondition`, and `targets` metadata;
- ten clamped drives: `sleepiness`, `curiosity`, `attention`, `gripConfidence`,
  `fatigue`, `comfort`, `boredom`, `satisfaction`, `alertness`, and `habituation`;
- a stable frozen personality from the `personality` RNG fork and composition from a separate
  `actions` fork, without consuming the parent stream;
- fixed eight-entry family, side, and target rings plus a fixed eight-slot interaction buffer;
- normalized, deeply frozen context snapshots and a monotonic visible-time clock that ignores
  non-finite/negative time, refuses to rewind, caps huge time at `Number.MAX_SAFE_INTEGER`, and
  bounds drive integration steps;
- support-aware template rejection and alternating safe fallbacks, four fixed rare-action
  cooldowns reserved at composition time, completed last-eight family rejection with
  deterministic oldest-first relaxation, and unconditional immediate-repeat rejection;
- a single candidate pipeline ordered as coarse eligibility, complete primitive-precondition
  validation, recent-family rejection/relaxation, validated fallback, and final invariant check;
- deterministic interaction decay/habituation, completion notices, idempotent completion,
  cursor-ordered equal-time interaction selection, and priority preemption;
- frozen performances and phases with positive durations, consistent offsets/totals, safe cuts,
  metadata-bounded `actionMs`, meaningful channel targets, and no references to mutable director
  state;
- semantic signatures built from composition choices, phase timing, and sorted channel targets;
  raw seed, session ID, creation time, and object identity are excluded.

The fixed priorities and relative safe-cut deadlines are:

| State | Priority | `deadlineMs` |
| --- | ---: | ---: |
| dead | 100 | 8000 |
| waiting | 90 | 5000 |
| interaction | 50 | none |
| done | 40 | none |
| working | 30 | none |
| idle | 10 | none |

Working variations all advance with `shift-weight` and inspect the `command-packet`; waiting
points with one extended digit on a free front claw when support permits; done relaxes and settles;
dead checks the signal and curls safely; idle draws from 19 natural families plus support-safe
fallbacks.

## Diversity and Bounds

An exhaustive measurement over the requested sample produced:

```text
512 seeds x 96 performances = 49,152 performances
49,152 distinct semantic signatures
0 collisions
elapsed 1.88s; max RSS 128,744 KB
```

The test threshold of 10,000 signatures completes during the focused test in roughly 0.38s.
Sustained-update coverage verifies stable director keys, at most eight pending interactions,
exactly bounded last-eight memories, four fixed cooldown keys, and finite `[0,1]` drives.
An additional 1,920-performance / 7,419-phase probe across five states and three support
configurations found no immediate family repeats, support-precondition violations, or action
duration violations.

## Verification

| Command | Result | Timing |
| --- | --- | ---: |
| `node --test tests/perezos/test_behaviors.js` | 26/26 passed | 0.62s Node / 0.65s elapsed |
| `node --test tests/perezos/*.js` | 103/103 passed | 12.07s elapsed |
| `pytest -q tests/test_perezos.py` | 1/1 passed | 11.79s pytest / 12.95s elapsed |
| `pytest -q` | 385/385 passed | 118.24s pytest / 119.18s elapsed |
| `node --check` on implementation and test | passed | <0.1s |
| `git diff --check` | passed | <0.1s |

The final registry probe reported 31 primitives. State probes reported the exact priority and
deadline table above and positive durations for every sampled template.

## Self-review and Concerns

- Public records are frozen snapshots; mutable director state is private in a `WeakMap`, sealed,
  and uses only fixed-capacity records. Update/notification work does not grow arrays or maps.
- Performance composition intentionally allocates frozen phase records and targets; this is not
  the future per-frame Motion loop and is permitted by the brief.
- If every front support is marked loaded, waiting uses a safe gaze fallback instead of violating
  a support precondition to point. With any free front claw, every waiting template points.
- The requested independent review initially found one Important template/side-selection bug and
  one Minor cap-expiry boundary bug. Both were reproduced in RED and fixed; the final assessment
  was Ready with zero remaining Critical, Important, or Minor findings.
- The formal-fix auxiliary review then found one Important precondition-ordering defect. Its exact
  unstable-support reproduction was added in RED and fixed. The follow-up assessment was Ready
  with zero Critical, Important, Minor, or required recommendation findings.
- The behavior director is not wired into the later Motion/runtime layers here; that belongs to
  subsequent tasks and was deliberately left untouched.
