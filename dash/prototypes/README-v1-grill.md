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
