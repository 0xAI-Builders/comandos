# Session configuration and recovery audit

Configuration changes now distinguish an accepted request, a running destination
that still needs confirmation, and a confirmed configuration. Recovery retains
the exact source conversation and its accounts and explicit permission flags.
The tests cover real coordinator/adapter code, private tmux processes and scripted
ACP transports. They do not establish that every installed provider/model works
with the user's subscriptions.

Baseline evidence: `edcd1e42017ee43b8f4497c333c5ff57428815e2`.

## Executable route matrix

[test_session_route_matrix.py](../../tests/test_session_route_matrix.py) derives
30 cells from the six `matrixHarnesses` and five motors in
[providers.json](../../config/providers.json). Each cell receives model, effort,
account and combined changes, plus invalid model, effort and account requests.
The five recoverable native routes also exercise all 74 registered model/effort
combinations. Catalog coverage does not prove provider availability.

| Destination route | Mid-session adapter behavior | Account behavior |
| --- | --- | --- |
| `claude:claude` | Exact Claude resume, including model and effort changes | Same-account resume or exact transcript and sidecar copy to another authenticated account |
| `claude:codex`, `claude:grok` | Exact Claude resume through the registered gateway | Claude account selectable; gateway motor account must remain `main` |
| `codex:codex`, `grok:grok` | Exact native resume | Same account or exact history copied to another authenticated account |
| `codex:grok`, `grok:codex` | Rejected before closing the source | Registry marks bridges unverified |
| `acp:claude` | Existing ACP session only, with observed effort from `configOptions` and exact resume support | Same motor account; account transfer lacks an exporter and is rejected |
| `acp:codex`, `acp:grok`, `acp:opencode`, `acp:agy` | Rejected by the restart coordinator before closing the source | Registry does not offer exact ACP resume for these agents |
| `opencode:opencode`, `agy:agy` | Rejected by the restart coordinator before closing the source | Exact resume/observation adapter is unavailable |
| Other cells | Explicit protocol exclusion or no registered route | No terminal input |

All eight registered source types are covered: Claude, Codex, Grok, ACP,
OpenCode, Antigravity, Gemini and shell. The first three, recoverable ACP and
shell can enter the coordinator. OpenCode, Antigravity and Gemini sources are
rejected because their exact recovery is unavailable here. Gemini and shell
have no destination route in the configuration matrix.

`session_change_support()` in [cc-dash](../../bin/cc-dash) describes this policy
separately from new-session launch availability. `acp_effort_unobserved` is
conditional on current protocol evidence. `exact_resume_unavailable` and
`route_not_live_verified` are explicit limitations. In-session `profileId`
requests are rejected rather than ignored; profile creation/launch coverage is
documented in [the extension audit](2026-09-13-provider-extensions.md).

## Recovery and continuity

[session_operations.py](../../lib/session_operations.py) commits the snapshot
before terminal mutation. The source's observed conversation, model, effort,
accounts and process generation must remain stable while waiting. The API can
also pin `expectedIdentity` and `expectedConversationId` when the draft opens.
Duplicate request IDs return the existing operation; a changed payload or a
concurrent operation on the same pane is rejected.

Successful verification requires the destination's operation marker and process
identity, exact conversation when resuming, matching observed model, effort,
motor and accounts, and a fresh terminal prompt. The launch command clears only
the visible terminal screen after the shell echoes its command. Verification
does not read scrollback, so an old prompt cannot confirm a new process.

An owned destination that remains open without confirmation enters durable
`awaiting_confirmation`. This is pending, with `confirmed: false` and
`recoveryAllowed: true`. Status polling performs one observation and an atomic
conditional state update; it sends no terminal input. The pane lock and source
snapshot remain available across dashboard restarts. Explicit recovery pins the
destination PID/start/operation marker and terminates that owned startup process
without pressing Enter on a trust or login dialog.
Unchanged pending polls do not rewrite SQLite or advance the operation timestamp.
Only the successful conditional confirmation records the observed runtime
configuration for analytics.

Failed launches that exit back to shell restore the exact source and return its
observed configuration. A persistence error while entering recovery does not
prevent the recovery attempt. If recovery or its final persistence cannot be
confirmed, the snapshot remains available and success is not claimed.

Cross-native-harness changes resume a saved conversation for that destination
when one exists. Otherwise they start a new provider conversation. Results expose
`continuity: resumed|new-conversation`, `handoffRequired` and, where needed,
`handoffPath`. The handoff file preserves visible context and repository status.
It is not automatically submitted as an inference prompt. The UI must present
the continuation prompt to the user; a saved handoff is not transferred internal
provider memory.

Account copies reject divergent histories and symlinks in source/destination
paths. Each file replacement is atomic, so an interrupted copy cannot truncate
an existing destination transcript. Explicit sandbox, approval, configuration,
MCP and permission flags survive supported resume paths.

## ACP protocol and direct commands

The ACP client consumes `configOptions` from session creation/load and
`session/set_config_option` responses, plus `config_option_update` notifications.
It uses the published `model`, `mode` and `thought_level` categories, preserving
unknown categories without guessing their meaning. Requested values must match
acknowledged returned state. These fields are defined in the primary
[ACP session configuration specification](https://agentclientprotocol.com/protocol/v1/session-config-options).

[cc-acp](../../bin/cc-acp) retains the original process until a replacement
connection has initialized and loaded the exact conversation. An explicit resume
cannot fall back to `session/new`. Unsupported account/cross-agent transfers and
invalid model/effort choices leave the origin open. In-place `/model`, `/mode`
and published `/effort` options use protocol acknowledgements without restarting.
Providers lacking effort options can retain the supported restart path, but the
display labels the effort as requested rather than observed.

Turning `/danger` off restores the known safe provider mode. `/mode` also updates
the local permission handler's state. Failed mode changes do not claim the
requested permission state. `publish()` serializes shared state-file writers and
records `observedModel`, `observedEffort`, `requestedModel`, `requestedEffort` and
`effortSource`. Dashboard observation accepts only matching ACP PID/session data.

## Evidence and limits

The initial adapter matrix reproduced 26 failures out of 151 tests, including
ignored stale/unsupported drafts, premature confirmation and missing observed
rollback results. Additional failing regressions reproduced interrupted recovery
persistence, symlink account escape, truncated-copy exposure, lost permission
flags, process replacement during exit and unobserved source effort. Direct ACP
tests reproduced 15 failures out of 18 before their corresponding changes.

The independent review reproduced premature confirmation in real private tmux:
an account switch reported success after 1.24 seconds while the new process
delayed its prompt for eight seconds. With the screen/readiness fix it confirmed
after 8.77 seconds. A shortened verification window instead entered the durable
pending state, and a later read confirmed it after the prompt appeared.

[test_session_tmux.py](../../tests/test_session_tmux.py) uses real tmux, `/proc`,
PaneInspector, metadata readers, terminal exit/send and account files. Its Codex
executable is a compiled non-networking fixture. It covers model/effort changes,
account copying, failed-launch rollback, delayed prompt confirmation and
preservation of another pane and the window layout.

[test_acp_client.py](../../tests/test_acp_client.py) drives real JSON-RPC subprocess
transport against [fake_acp_agent.py](../../tests/fake_acp_agent.py). Configuration
option tests send no inference prompts. Direct client failure/mode tests use
controlled session objects in [test_acp_reconfiguration.py](../../tests/test_acp_reconfiguration.py).

An additional disposable startup probe launched the installed Claude 2.1.269 and
Codex 0.154.0 in private tmux with temporary homes, projects and private copies of
authentication files. No provider configuration or existing user session changed.
After seven seconds each had zero transcripts and zero session metadata files;
Claude showed onboarding and Codex showed workspace trust. No dialog was
accepted and no inference prompt was sent. This establishes that startup can
remain unconfirmed; it does not prove when a fully onboarded provider creates its
first transcript or that a particular model is accessible to the account.

The four legacy trust integration assertions required automatic workspace-trust
acceptance. Their replacements exercise command construction, coordinator dialog
handling, account snapshot copying and ACP connection without accepting trust.
The explicit legacy trust helper's six unit tests remain unchanged.

Run the focused suite from the repository root:

```sh
python3.11 -m pytest -q tests/test_session_operations.py tests/test_session_route_matrix.py tests/test_session_tmux.py tests/test_agent_launch.py tests/test_pane_snapshot.py tests/test_claude_trust.py tests/test_acp_reconfiguration.py tests/test_acp_client.py
```

The six existing provider-inference E2E cases remain opt-in and were not run.
The focused command completed with 432 passed and six skipped. The subsequent
unchanged-poll/one-time-analytics guard was checked with its three affected
coordinator regressions and the private tmux suite.
No provider inference success, live gateway routing result, deployment or browser
validation is claimed by this report. User-edited settings files after snapshot,
unrecognized launch flags and provider-specific session-store changes remain
outside exact rollback guarantees; the coordinator refuses missing observations
and preserves recovery evidence instead of guessing.
