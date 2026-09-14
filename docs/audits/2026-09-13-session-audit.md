# Session configuration and remote parity audit

Baseline: `edcd1e42017ee43b8f4497c333c5ff57428815e2`.

The dashboard now scopes its control card, configuration and operator chat to the
selected live session and pane. Configuration operations preserve recovery data,
verify the actual destination and retain a recoverable pending state when a CLI
has not finished startup. Provider extension inventories describe their sources
and activation limits instead of substituting Claude configuration.

## Findings and resulting behavior

| Area | Reproduced failure | Result |
| --- | --- | --- |
| Local | Hidden shell panes and historical waiting rows could redirect the control card to Signara. | Local always emits live pane cards. A missing active session or exact pane cannot select another session. |
| Remote splits | The browser did not learn pane changes made inside the terminal. | Existing `/state` polling reports the active pane of the active window. Bottom-bar pane selection also notifies its owning frame. |
| Remote controls | CSS hid Pomodoro; native Claude configuration was disabled when the gateway was down. | Pomodoro is visible remotely. The explicit **Configurar IA** button opens one form for CLI, engine, model, effort and accounts. Unsupported combinations explain why they cannot apply. |
| Drafts and results | Matching model text could clear account-only or effort-only operations; status lookup could return another request. | Selection is a draft until one Apply action. Requests pin conversation/pane identity; polling binds the operation ID and never treats unchanged model text as success. |
| Startup | A prompt left by the old process could confirm a new process before its first redraw. | Launch clears the old display without erasing saved history. Verification requires fresh destination evidence; delayed startup remains recoverable. |
| Continuity | A cross-CLI launch could imply the old internal conversation transferred. | Original history remains intact. New-provider conversations expose a saved continuation note and a button to copy the instruction for the next prompt. |
| Accounts and extensions | Legacy APIs forced providers to Claude, lost disabled state, retained stale account chips or cached inventories indefinitely. | Inventories resolve verified process/account/project context, retain descriptions/provenance, and refresh with a bounded cache. Old account choices are replaced when provider/account evidence changes. |
| Runtime model | Historical usage or stale custom status output could masquerade as the active model. | Live cards use current process/conversation observations. Unconfirmed candidates are labeled; historical usage cannot replace missing runtime evidence. |
| Operator | Local was missing from tab tools, request context could fall back to desktop, malformed streams could execute default arguments or say “Done.” | Session/pane membership is validated. Status and analytics tools preserve scope and attribution; invalid/incomplete streams stop with an explicit error. |

## Detailed audits

- [Configuration routes, account copies, ACP and recovery](2026-09-13-session-routes.md).
- [Provider configuration, skills, MCPs, profiles and TUI observations](2026-09-13-provider-extensions.md).
- [Operator tools, analytics and scoped recommendations](2026-09-13-operator-analytics.md).

## Verification

Browser execution used the Mac mini's sandboxed Chrome. No local automation
browser was launched. The new `tests/e2e_session_audit.cjs` loads the shipped HTML,
CSS and JavaScript against isolated APIs. Its baseline reproduced the Local,
chat-destination and hidden-Pomodoro failures.

The corrected UI passed at 320×568, 390×844, 844×390 and 1400×900 remotely, plus a
420×900 native-bridge fixture. Checks cover configuration access, one combined
request, pinned request identity, pending-operation recovery/continuation,
clipboard copying, replacement of provider accounts, active split propagation,
MCP descriptions, horizontal overflow and JavaScript exceptions.

Existing Mac browser regressions also passed:

- Remote workspace: favorites, Home-first ordering, preserved horizontal scroll,
  single active indicator, frame preservation and the terminal erase button.
- Terminal: native IME input, duplicate-input prevention, offline draft retention,
  no replay, stable history selection, copy without terminal input, exact pane
  targeting, physical controls and desktop raw input.

The real-tmux coordinator tests use private sockets and compiled non-networking
CLI fixtures. Independent delayed-redraw reproduction changed from premature
confirmation at 1.24 seconds to confirmation after the prompt at 8.77 seconds.
A shortened initial wait produced `awaiting_confirmation`, retained the original
snapshot and confirmed on a later read after the real redraw.

A read-only inventory of the running installation returned 31 live cards,
including Local's AI and shell panes. The live Claude, Codex and Grok inventory
endpoints returned provider-specific extension lists. The old dashboard stayed
running during preparation; no user CLI was changed to perform an audit case.

An authenticated operator smoke used an isolated fixture session and a dispatcher
that rejected every mutation. Haiku returned an exhausted-usage error; the
configured Codex Spark fallback called `session_status`, reported the fixture's
actual model/effort and gave a recommendation. It issued no actions and did not
write to the user's chat history. The final fallback UX is checked separately.

The full Python run completed with 1,347 passed, six opt-in provider checks
skipped, one local-browser case deselected and three failures in obsolete source
assertions. The dedicated local-browser module was excluded because browser
execution belongs on the Mac.

Those three assertions were replaced or updated to check the actual request
binding and launch/poll behavior. The JavaScript poll regression now executes the
shipped callback, rejects stale operation IDs, applies every configuration field
on success and rollback, and verifies another pane remains untouched. The shell
launch regression verifies exact literal tmux input, one Enter and display
clearing without bracketed paste.

The subsequent affected-suite run passed all 267 tests in 139.67 seconds,
including the operator, selected-session, provider-context, usage UI and remote
control cases. Separate snippet regressions passed all 17 tests. Three test AST
loaders now extract only the requested helpers, without changing their assertions.
Changed Python files also compiled with the installed Python 3.10 runtime, and
the whitespace check passed. These were separate complete commands; this report
does not present their totals as one zero-failure full-suite run.

A final operator review added six regressions for empty or contaminated provider
answers. Such answers now return an explicit failure, preserving any real tool
result already obtained. The full operator subset passed all 108 tests after
that change.

A private recovery backup contains the baseline source, workspace layouts,
application tab state, pane IDs/PIDs and online SQLite backups of session
operations and usage. Deployment verification is recorded after installation.

## Operational limits

This is evidence from configuration fixtures, real local process boundaries,
protocol fixtures and browser interaction; it is not a guarantee about every
future provider release or authenticated inference response. Real Claude/Codex
startup probes used private temporary homes, accepted no trust prompts and sent
no inference. Six external provider E2E checks remain explicitly skipped.

A provider lacking an exact resumable adapter is unavailable for destructive
mid-session switching. New-session availability is separate. ACP effort requires
acknowledged `configOptions` evidence. Some startup flows need the user to finish
login/trust or send a first prompt before confirmation; recovery remains visible.

Installed/configured extensions are not automatically claimed active in an
already-running provider. Unsupported live toggles and launch overrides are
identified before applying. Usage without a skill/MCP attribution remains
unknown; token savings are not invented.

No permanent worker or polling service was added. Extension requests are shared
while in flight and cached for ten seconds; pane focus uses the existing state
poll. Unchanged pending confirmation polls do not rewrite SQLite. Recovery and
profile state remain in local SQLite with bounded configuration parsing caches.
