# ComandOS 1.0 — prototype and human review

> Current direction: automatic restoration of the latest workspace on open. Manual recovery and its preview were removed from the active design. See "Confirmed: automatic restoration of the latest state" below.

This branch is a disposable design source, not production. Launch:

```
node /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/prototype-v1-server.cjs
```

Open http://localhost:4797/prototypes/prototype-v1-grill.html

The baseline embeds the current dashboard from the same branch against fixture APIs. Native window chrome and terminal output are recreated; it is not a screenshot of the live application. Five alternatives explore navigation structure. Data, mutations, logs, model changes and terminal actions are simulated. Some secondary controls expose their intended action as text rather than executing a complete workflow.

## Persistent review constraints

Every design round must compare desktop and remote, including narrow/touch and short landscape layouts. Both have the same product capabilities and selected session configuration. Differences are window chrome, connection status, keyboard/gestures and spatial arrangement. Remote focus must not steal desktop focus.

User validation is required before implementing a design verdict. Review the whole shell first, then session navigation and terminal, AI controls, extensions, chat, analytics/reparto, utilities and settings. Exercise normal, empty, loading, error, disconnected, pending and recovery states in relevant rounds. Prototype acceptance is not proof that production behavior works. Each implemented block needs real verification plus the user's human-in-the-loop verdict.

## Current performance evidence

Two read-only local GET /pane-extensions requests on September 26, 2026 returned 200 in 1.077 and 1.712 seconds, approximately 73.5 KB, 41 MCPs and 122 skills. No mutation was performed. These are two observations, not a latency distribution.

Code candidates: repeated inventory construction within a request, repeated pane/process inspection, POST followed by mandatory full-state GET for every selection. Attribution requires profiling. Existing usage cache must be considered before changing it. Do not mask latency with a cosmetic spinner or present simulated mockup latency as a production improvement.

## Unintegrated correctness fix

Branch fix/extension-selection-responsive contains a failing-then-passing regression for global selection ignoring search/type filters, an explicit global toolbar and separate filtered actions. It has not been activated. Preserve that fix when implementing the agreed extension design; a category is deterministic, and selected is distinct from used. Unknown or externally managed entries must remain explained and uneditable.

## Round 1

Open decision: overall organization, variants A–E. Baseline is a reference, not an alternative redesign. No user verdict yet. Provider logs are a proposal, explicitly labeled. Group and token examples in the prototype are illustrative, not measured production metadata.

## Product grilling — September 27, 2026

User decisions, replacing earlier unconfirmed assumptions:

- Product foundation: a powerful Linux terminal that serves as an interface for humans to operate AI.
- Ease of operation is the primary experience requirement.
- ComandOS has no mandate to autonomously operate the user's agents. Insights may be generated automatically; this does not authorize configuration changes or execution.
- Core success scenario: open ComandOS and easily continue prompting across N projects while preserving mental context when switching.
- The desktop-to-phone journey proposed earlier is not the primary acceptance scenario. Desktop/remote capability parity remains a requirement from prior decisions.

Working objective, pending wording approval: a Linux terminal for operating multiple AI projects, making it easy to resume and switch work while preserving technical state and mental context, under human control.

Next unresolved decisions: opening destination, minimal context needed when returning to a project, and the relationship between a project and its multiple agent sessions/panes. No A–E layout has been selected. Previous task-first/orchestration recommendations are not accepted decisions.

### Confirmed: startup continuity and recovery

On opening ComandOS, the user wants the exact previous workspace restored. Backup and recovery are among the product's most valuable capabilities and must be a primary 1.0 acceptance area. Exact restoration is a requirement to define and test, not a claim that every process can survive a machine crash. Scope still needs to cover selected pane/tab, geometry, conversations, scroll position, unsent drafts, recovery history and unsupported restoration cases.

Undecided: what recap helps a human resume work, whether AI-generated summaries add value beyond harness-native history/memory, and whether projects or individual sessions should structure navigation. Do not record either navigation alternative as accepted.

### September 28: dynamic workspace and all four re-entry needs

User confirmed that switching work causes all four difficulties: locating the right terminal, recalling intent, understanding progress while away, and identifying the next human decision. No ranking among the four was given.

User explicitly rejected the assumption of stable spatial memory: sessions are reordered and splits are opened and closed frequently. Restoration must preserve the latest user-chosen arrangement. Context must follow session identity across moves and view changes; do not use position as the primary identity or automatically reorder the workspace on activity.

Still unresolved: meaning of closing a split (hide/detach versus stop process); presentation and timing of a recap; project grouping; treatment of background sessions. The user has not yet approved AI-generated summaries or a particular navigation layout.

### Confirmed: closing a split ends its session

User explicitly chose termination on closing a split. Do not implement hide/detach as the default meaning of close. Scope termination to the selected pane's process; preserve other panes. Historical recovery and what exactly remains after an intentional close still need definition.

User considers flexible placement/mixed projects existing behavior. Preserve that workflow. Current next design question is how to identify the work in each pane easily; introducing a new project navigation hierarchy is not requested.

Round 2 prototype: same HTML route with ?round=panes&variant=A. Five alternatives for pane identity: persistent task header, contextual reveal, selected-pane inspector, work overview, inline context note. Names and summaries are fictitious. No decision yet on automatic title/summary generation. Simulated pane closing removes that pane; simulated reorder keeps identity/context attached. Compare desktop and remote modes.

### New requirement: split the workspace by dragging whole tabs

User wants drag-and-drop of an existing tab to show multiple whole tabs simultaneously, in addition to splitting terminal panes. A tab must retain its own inner pane layout and underlying conversations. This is a requested capability for the release plan, not permission to replace live session infrastructure during grilling.

Existing code has reorderable native notebook tabs and a separate native mosaic view. Proposed design must distinguish workspace regions containing tabs from terminal panes running processes. Preserve identities, inner pane layouts, focus, and region layout across restoration. Desktop and remote must expose equivalent operations, with a usable touch alternative.

Open decisions: whether dragging moves or duplicates a view; destination/merge gestures; removing a workspace region versus terminating a terminal pane; limits and behavior on small viewports. Earlier decision that closing a terminal split ends its session does not settle the meaning of removing a region containing multiple tabs.

Deterministic identification proposal (not yet accepted): manual title takes precedence; otherwise use a bounded literal excerpt of the first user prompt; fall back to directory/process when no prompt exists. Show last user prompt, last response, and event-derived status as distinct source-backed fields. A generated harness title or AI recap is not inherently deterministic and must be identified as such if used. A native title explicitly assigned by the user can be treated as manual metadata when provenance is known.

### Confirmed: moving tabs, with a visible combined-tab indicator

User accepted move-only drag behavior (no duplicate view of the same tab). User suggested a visual identifier when a tab contains two whole tabs. Round 3, ?round=tabs&variant=A, explores a combined tab with an orientation glyph and a count explicitly labeled tabs, distinct from its inner pane count. Alternatives compare a layout miniature, member labels, expandable membership and a second member row. Indicator design and the exact combined-tab presentation await the user's verdict.

The mock supports moving an individual tab into a group via drag/drop or explicit touch buttons, separating it back into the strip, and resizing two side-by-side members. Reordering views is simulated without touching real processes. Narrow view stacks whole tabs while retaining group membership. General nested layouts, actual restore, and group termination are not implemented or validated by this prototype.

### Accepted feedback: count followed by tabs; drag/drop as direct manipulation

User liked the number followed by "tabs" and the drag/drop interaction. User explicitly rejected the "Colocar / Servidor / Izquierda / Derecha / Arriba / Abajo" controls. Those controls were a proposed touch alternative in the prototype, not existing production UI. Remove them from the proposed product flow.

The prototype now uses mouse drag/drop and touch/pen hold-and-drag. Direction targets appear only during a drag. Reset belongs to a clearly marked mockup-control area. Real touch-device ergonomics remain for human validation; browser event simulation does not prove physical-device behavior.

### Round 4: nested distribution through direct dragging

User requested more flexible drag/drop and a mock closer to the current ComandOS interface. This supersedes the flat group distribution in round 3. Keep the accepted count followed by "tabs" and omit directional placement controls.

Open `https://nodo-01.tail63a117.ts.net:8444/prototypes/prototype-v1-grill.html?round=dock&variant=A`. Existing `round=tabs` links now open this iteration too. Five initial arrangements share the same interaction: side-by-side tabs, a main region with stacked auxiliaries, four tabs, wide stacked tabs, and a single tab from which to build a layout.

- Drag a whole tab from the top strip to an edge. Drag a member's header to move only that member. Inner terminal panes travel with their tab.
- Preview the destination before release. Inner edges divide that region; the outer workspace edge divides the whole workspace. Release outside a target or press Escape to cancel.
- Drag a member back to the top strip to separate it. Top-level groups can be reordered without duplicating their tabs.
- Drag the shared separator to resize. Keyboard arrows on a focused separator also work; there is no size slider or positioning form.
- Narrow widths stack tabs in tree order. The stored horizontal orientation returns when width permits. Minimum pane space produces scrolling rather than scaling terminal text down indefinitely.
- The sidebar embeds the current dashboard assets against fixture APIs and stays mounted during layout changes. Terminal output is simulated. On a narrow screen the sidebar opens from the menu and has a return control.

Verification uses Chrome on the remote Mac mini. All five initial arrangements were checked at 1920×1080, 1024×768, 390×844, 844×390 and 320×640 without page-level horizontal overflow. Browser PointerEvent checks covered nested docking, member movement, detaching, divider resizing, cancellation, preserved unique tab identities and touch-style dragging. Screenshots were inspected on desktop and phone widths. Physical touch-device ergonomics still require the user's verdict.

This is disposable UI code on the prototype branch. It does not implement production terminal reparenting, backup/restore of the nested tree, multi-client tmux focus isolation or group termination. Refreshing resets the example. No live sessions are modified. The following verdict records acceptance of the interaction. The example arrangements are not a requirement to use a fixed template.


### Accepted verdict: flexible tab docking

On September 28, 2026, the user approved round 4: "ME ENCANTA!!! Justo asi". Use prototype commit `f634552` as the visual and interaction reference for implementation.

Accepted direction: current ComandOS shell, direct tab dragging to nested edge targets with destination preview, resize from shared separators, detach by dragging back to the strip, count followed by "tabs", and adaptation to narrow screens while keeping the tab order. Preserve each tab's inner panes and conversations. Distribution must not require directional placement controls or a sizing form. The five starting arrangements demonstrate free placement; they are not five mandatory templates or a newly imposed default on restored workspaces.

This accepts the design for the 1.0 plan. It does not establish that production persistence, recovery or remote/native terminal behavior have been implemented or verified. Preserve the prototype as the agreed reference while continuing the release grilling.

Subsequent decisions and remaining questions are recorded below. Exact backup/restore scope and deterministic context identification also remain open.


### Confirmed: group-close confirmation and mobile arrangement

The user answered the group-close question with "Deberia preguntar porfavor". Closing a composite tab must ask for confirmation before terminating its member sessions, showing the affected tabs and sessions. Cancel leaves the group and its sessions intact. Separating a tab by dragging it back to the strip does not terminate it.

The user also confirmed "Si tambien deberia poder hacerse desde el celular porfavor". Mobile must support reorganizing tabs, grouping, separating and resizing through the accepted direct manipulation. Do not treat mobile as a view-only client.

The initial answer confirmed mobile editing. The follow-up below resolves synchronization and supersedes the earlier recommendation of separate per-device arrangements.


### Confirmed: one shared arrangement across devices

The user answered "Si" to the concrete question: when Lola is placed next to ComandOS from the phone, should that same arrangement appear on the computer, adapted to each screen's size?

The workspace arrangement is shared. Manual grouping, separation, reordering and split-layout edits from either device must be reflected on the other. Each screen adapts the shared arrangement to its available space. Automatic responsive stacking must preserve the shared tab identities, membership and order; it must not overwrite the user's arrangement merely because the phone is narrower. The previous suggestion of independently remembered device arrangements is superseded.

This is an accepted product requirement, not a claim that live synchronization exists in the prototype. The startup/recovery requirement applies to the latest shared arrangement. The follow-up below resolves active tab/pane focus separately from the shared arrangement.


### Confirmed: independent active tab and pane on each device

The user answered "Si asi es" to keeping ComandOS selected on the computer when Lola is selected from the phone. Share the workspace arrangement, while each device retains its own active tab and pane. Selecting a different tab or pane on one device must not redirect keyboard input on the other. This is an accepted behavior requirement; the prototype and current tmux integration do not establish that it is implemented.

### Recovery preparation: current code evidence

Read-only code review on September 28, 2026 identified these limits for the next design round. No running sessions, backup contents or live recovery operations were inspected or changed.

- Linux periodically captures per-session layouts and resume metadata, with a previous valid generation and seven days of per-minute history. The normal reader tries the current generation and its previous copy. Sources: `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app:2019` and `/home/someguy/codebase/0xJesus/ComandOS/lib/tmux_snapshot.py:205`.
- Recovering a closed tab through `/recover-tab` can create a new session from its directory and agent if the old session is gone. This route does not read a layout snapshot or pass an exact conversation ID. Reopening the tab must not be described as guaranteed exact recovery. Source: `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-dash:9117`.
- The remote pane-close path saves recovery data before termination. The native Linux context-menu action calls `kill-pane` directly. Aligning their recovery behavior is an implementation gap to address, not an already completed fix. Sources: `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-dash:5326`, `/home/someguy/codebase/0xJesus/ComandOS/lib/terminal_panes.py:65` and `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app:1239`.

The next recovery decision is whether the historical recovery flow should support rescuing selected panes/tabs while leaving the rest of the current workspace in place, in addition to restoring an entire prior arrangement. Exact restoration of a prior workspace remains the user's primary requirement.


### Accepted: preview before recovery

The user answered "Me encanta lo de la vista previa". Recovery must show a preview before applying changes. This explicitly approves the preview; it does not yet settle selective recovery of individual panes/tabs versus recovery of the entire workspace.

Round 5 explores the preview inside the accepted shell at `https://nodo-01.tail63a117.ts.net:8444/prototypes/prototype-v1-grill.html?round=recovery&variant=A`:

- A: choose a time and compare Actual / Copia in the same workspace area.
- B: compare the current and saved arrangements side by side, stacked on narrow screens.
- C: select a snapshot by its layout thumbnail.
- D: inspect a tab/pane list with conversation details.
- E: start from the list of changes, with the layout alongside it.

All use invented copies at 08:45, 08:50 and 09:05. These are not the user's actual backups. The preview distinguishes conversations already open, conversations that can resume, and missing conversation data. Opening and cancelling a preview leave the example workspace unchanged. Applying an available copy updates only in-memory example data. Loading, empty, error and offline states are available in the mockup picker.

The incomplete example blocks recovery and explains the missing conversation. Replacing a previously recovered example with an older copy can remove a pane; the preview lists it and asks for explicit confirmation before the simulated close. Both policies are proposals for review. Partial recovery, treatment of current panes absent from a copy and historical retention still require product decisions.

No real backup data is fetched, no process is restarted, and no production recovery endpoint is called. Conversation excerpts are fixtures. This prototype is for choosing a preview interaction, not evidence of exact production recovery.


Round 5 verification: the five variants rendered in Chrome on the remote Mac mini at 1440×1000, 390×844, 844×390 and 320×640 without document or preview-content horizontal overflow; the action footer remained inside the preview. Browser interaction checks passed for cancel/reopen, incomplete-copy blocking, conversation details, Escape, simulated recovery, detecting panes absent from an older copy, confirmation/cancellation of a simulated close, loading/empty/error/offline states, and returning from the current-interface reference. No JavaScript errors were reported by the fixture error collector during those checks. Real recovery remains untested and unimplemented by this mockup.

Desktop screenshot inspection passed. The final mobile screenshot request timed out in the remote browser service after all layout and interaction checks had returned successfully; a final mobile image was not inspected. Physical phone validation remains part of the user's review.


### Confirmed: automatic restoration of the latest state

The user superseded the manual recovery exploration: "Mejor olvidemos recuperacion simplemente deberia recuperarse completmante el etado ultimo cada vez que abrimos commandOs porfavor". Opening ComandOS must automatically restore the latest complete workspace, without choosing a backup, opening a recovery preview or confirming the normal startup flow.

The restoration requirement covers open tabs and their order, tab groups and nested splits, proportions, each tab's inner panes, session and conversation identity, working context and configuration, reading position and unfinished input. The shared arrangement adapts to the current screen; active tab and pane remain local to each device. These are acceptance requirements, not claims about current implementation coverage.

Reuse sessions that are still alive. Reconnect to the corresponding conversation when a process must be resumed. A closed pane that is absent from the latest saved workspace stays closed. Saving and restoration must preserve the last valid complete state; a partially initialized or empty startup must not replace it. If part of the state cannot be resumed, do not silently substitute a new or unrelated conversation and call it a complete restoration.

Backups remain internal support for continuity. This instruction removes the manual historical-recovery UI from the active release design; it does not authorize deleting existing backup data. The accepted drag/drop distribution, group-close confirmation and independent device focus remain unchanged.

The approved docking prototype is active again. Existing links with `round=recovery` now open `round=dock&variant=A`. The discarded preview can be inspected in prototype commit `290e081`; it is retained only as historical design evidence. No production startup, tmux process, session or backup was changed by this design update. Production verification must cover normal reopen, remote reconnect and recovery after an interrupted application run before claiming complete automatic restoration.


### Requested next block: tab states and quick terminals

The user wants to keep tab favorites and add organizational states such as Resuelto and Congelado. Automatic work-in-progress indication is desirable only when work can be detected deterministically. The meaning of Congelado is not settled: marking work for later versus actually pausing/interruption is the first grilling question. No default answer has been accepted.

The user requests removing the visible Mosaico button now that nested tab grouping is the accepted distribution. This does not by itself authorize deleting the underlying implementation or changing its shortcut.

Nueva sesión must support a fast path that does not always ask for a folder. The user proposed inheriting the current working directory or using a dedicated quick-terminal directory. `/home/someguy/codebase/0xJesus/Terminal` is a candidate under the existing project root; it did not exist when checked and was not created. The default location and whether a quick terminal starts a shell or an AI conversation remain decisions, not implementation assumptions.

Working model proposed for review: keep favorite marking independent from a manual organizational state, and expose observed runtime activity separately. A completed agent turn is evidence of turn completion, not evidence that the user's task is Resuelto. Do not silently infer resolution from an agent response or overwrite a manual state based only on process liveness.

### Current facts for the tab-state and quick-terminal grilling

Read-only code inspection found:

- Favorites are shared preferences per session. Native and remote clients use individual `/prefs-set` updates and order LOCAL, favorites, then other tabs. Sources: `/home/someguy/codebase/0xJesus/ComandOS/lib/session_tabs.py:4`, `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app:1718`, `/home/someguy/codebase/0xJesus/ComandOS/dash/index.html:6814`.
- Current activity combines hook events and terminal-text heuristics. It can default to idle without a hook, and an alive process is not proof of an active turn. Hook adapters provide useful events, but their availability and correlation must be verified per harness before calling the indicator deterministic. Sources: `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-dash:6846`, `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-dash:6914`, `/home/someguy/codebase/0xJesus/ComandOS/hooks/cc-notify.sh:284`.
- Existing `/pause` sends SIGSTOP/SIGCONT to a selected agent PID for the session. The UI calls that action "Agente congelado". It is a real process action, not a persisted organizational label, and does not establish a whole-tab pause contract across multiple panes. Sources: `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-dash:8473`, `/home/someguy/codebase/0xJesus/ComandOS/dash/index.html:3530`.
- The Linux primary plus button opens the folder/agent/account assistant. Ctrl+T and the secondary menu already create a shell using a session-derived cwd, with a personal-directory fallback. Sources: `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app:3923`, `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app:2756`.
- The remote tab-strip plus calls `/tab-new`, which creates a shell in the personal directory and does not inherit the selected pane's cwd. `/session-new` accepts an explicit existing absolute directory and `agent: shell`. Sources: `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-dash:8982`, `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-dash:8795`.

These are code observations, not results of exercising live sessions. No production buttons, process state, preferences or creation behavior were changed during this grilling step.
