# ComandOS 1.0 — prototype and human review

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

This is disposable UI code on the prototype branch. It does not implement production terminal reparenting, backup/restore of the nested tree, multi-client tmux focus isolation or group termination. Refreshing resets the example. No live sessions are modified. The adaptive layout rule and five arrangements remain proposals pending human review.
