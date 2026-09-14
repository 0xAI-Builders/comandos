# Session and remote-controls audit

Baseline: edcd1e42017ee43b8f4497c333c5ff57428815e2.

## Required outcomes

- Audit every supported harness/motor route and configuration dimension: model,
  effort, accounts, profiles, same-session changes and cross-harness changes.
  Invalid or unsupported combinations must explain the limitation before any
  source process is stopped. Failures must retain exact recovery information.
- Local and remote cards, operator chat and configuration controls must use the
  selected session/pane, including `local`, without inheriting a neighboring
  project. Remote users must reach engine/account/model controls and Pomodoro.
- Operator chat must answer status and analytics questions using current tools,
  distinguish observed metrics from estimates, and give grounded recommendations.
- Skills and MCP discovery must follow provider/account/project configuration,
  preserve disabled state and provenance, and avoid claiming runtime activation
  from files alone. Existing profiles and tool-usage controls remain accessible.

## Implementation and verification

1. Reproduce identity and remote-control failures using real shipped HTML in an
   isolated Mac browser; cover local versus Signara, pane switching, desktop
   focus changes, touch/desktop widths, hidden chat and single-pane sessions.
2. Audit and fix session coordinator/adapters; generate a route/configuration
   matrix and fault tests for identity, stale requests, duplicate requests,
   timeout, failed launch, rollback and interrupted persistence.
3. Audit and fix extension inventory and profile launch arguments against actual
   provider configuration formats with temporary account/project fixtures.
4. Audit operator catalog, prompt, dispatch and HTTP context propagation; cover
   status/usage/recommendation questions without unintended mutations.
5. Review all diffs, run the complete non-browser suite and relevant Mac browser
   suites, and distinguish mocked, real-tmux and real-provider evidence.
6. Back up the active installation, deploy verified files atomically, restart
   only the necessary dashboard service, and check active pane identities and
   live remote read-only behavior. Preserve a rollback path.

## Constraints

- Heavy browser execution only on macmini, with its sandbox enabled.
- Never mutate existing user sessions or provider configurations for test cases.
- No credentials, transcripts or private configuration contents in audit artifacts.
- No extra permanent polling service or unbounded work. Use on-demand/cached reads.
- Mark unsupported capabilities and untested external combinations explicitly.
- Test behavior at real seams; do not replace meaningful assertions to hide bugs.

## Work ownership

- Root: dashboard HTML/workspace UI, Local identity and remote controls, integration.
- Session audit: session_operations.py, session adapter/configure block in cc-dash,
  session-operation tests and route-matrix report.
- Discovery audit: capabilities.py, session_profiles.py, mcp_descriptions.py,
  extension inventory/profile tests and report. Coordinate API changes with root.
- Operator audit: operator_chat/tools/catalog/dispatch modules, operator tests,
  operator API block in cc-dash and report. Coordinate dashboard changes with root.
