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
