# Operator context and analytics audit

The operator changes preserve the selected session and pane in both chat transports,
provide scoped status and usage tools, and reject incomplete provider responses
before executing their tools. The verification below uses isolated data and
scripted provider responses. It does not establish real-provider answer quality
or deployment status.

Baseline evidence: `edcd1e42017ee43b8f4497c333c5ff57428815e2`.
The changes under review are in `fix/session-audit-20260913`.

## Reproduced defects and coverage

| Finding | Result under test | Regression coverage |
|---|---|---|
| The nonstream handler used desktop focus instead of the request context. | Both handlers validate the supplied session/pane before saving messages or starting inference. A pane from another session produces HTTP 400 before streaming headers. | `test_nonstream_handler_uses_request_context_not_desktop`, `test_invalid_selected_pane_is_rejected_before_inference_or_persistence`, `test_http_invalid_context_returns_400_before_sse_headers` |
| Local was omitted from operator tabs and could fuzzy-match `local-neighbor`. | Local appears once in the tab inventory. An exact selected-session name or current-session alias resolves before fuzzy matching. | `test_selected_local_cannot_resolve_to_another_tab`, `test_operator_tab_inventory_includes_local_once` |
| The selected pane leaked into tools naming another tab. | An explicit different target does not inherit that pane. | `test_explicit_other_tab_never_inherits_selected_pane` |
| Empty tool arguments could fetch another provider's default configuration. | Configuration reads resolve the selected live row and its folder, harness and account. Conflicting folders fail. Profile reads permit explicit destination harness/account overrides. | `test_session_brain_resolves_identity_instead_of_mixing_cwd_and_pane`, `test_profile_inventory_uses_selected_provider_and_account` |
| Whole-dashboard JSON was truncated after 1,800 characters, potentially omitting the selected session. | Scoped tools filter before serialization. Larger replies remain valid JSON and identify omitted content. | `test_session_status_filters_exact_live_pane_before_summarizing`, `test_large_analytics_result_is_valid_json_with_explicit_truncation` |
| An explicit request for global extension usage inherited the selected pane again. | Scope is explicit: selected pane, whole selected session, or all sessions. | `test_extension_usage_explicit_all_scope_does_not_reapply_defaults`, `test_explicit_session_scope_includes_sibling_panes` |
| Provider errors could finish as `Listo.`; malformed tool JSON became an empty argument object. | Error events, incomplete streams, token-limit stops and invalid argument objects produce failure. Tools from that response do not execute. | `test_provider_error_cannot_finish_as_success_or_execute_queued_tools`, `test_malformed_tool_json_does_not_become_empty_default_target_command`, `test_incomplete_provider_response_emits_error`, `test_token_limit_is_reported_as_incomplete` |
| Fallback between provider families retained incompatible tool-message formats. | Tool-use and tool-result history is translated without changing the original history. | `test_payload_converts_tool_history_when_provider_family_changes`, `test_provider_message_conversion_does_not_mutate_previous_history` |
| Pending mutation replies hid the operation reference from the model. | Replies expose `operationKey` and `operationId` so the model can query status. | `test_pending_operation_exposes_reference_to_the_model` |

These cases live in [test_operator_analytics.py](../../tests/test_operator_analytics.py).
The request handlers, `_op_tab`, provider payload builder and agent loop live in
[cc-dash](../../bin/cc-dash). Tool defaults and result filtering live in
[operator_dispatch.py](../../lib/operator_dispatch.py).

## Evidence and recommendation behavior

[operator_catalog.py](../../lib/operator_catalog.py) defines the tools and their
scope. `session_status` returns exact matching live rows by default;
`historical=true` also permits historical rows. `scope=session` includes sibling
panes. `session_usage` retains the counters' source confidence and time window.
It does not sum rows whose usage is shared by folder.

An absent usage row has `attribution=unavailable`. It is not a zero-token result.
Configured extensions are distinct from observed extension calls. Their token
and cost attribution can remain null. Global provider totals and experimental
comparisons are not selected-pane metrics.

[operator_tools.py](../../lib/operator_tools.py) supplies these distinctions to
the prompt. It directs recommendations to current read tools, sample sizes,
confidence and the analytics disclaimer. Suggested optimization profiles are
not evidence of measured savings. API price estimates are not paid costs.

Recognized status, consumption and recommendation questions place the dispatcher
in read-only mode for that request. A model-generated mutation then returns an
error without reaching its API or local handler. Explicit action requests remain
actionable. This is a bounded language heuristic, not a complete semantic parser;
the prompt also requires explicit authorization before applying recommendations.

The shipped handlers do not invoke the legacy `parse_intent`/`apply_intent` path.
The analysis classifier changes tool permissions; it does not replace a question
with a UI-opening action or a canned recommendation.

The scripted conversation test sends `session_status` and `extension_usage`
through the real dispatcher. Its next provider round receives the exact Local
state and null extension-token attribution. The final response is scripted, so
this verifies evidence delivery rather than a model's interpretation.

## Example requests and expected scope

| Request with Local / `%1` selected | Evidence path | Expected interpretation |
|---|---|---|
| “Estado y uso de skills” | `session_status`, `extension_usage` | Local / `%1`; missing extension events do not prove no usage. |
| “Estado de todos los paneles de esta sesión” | `session_status` with `scope=session` | All matching live Local panes. |
| “Uso de MCPs de todas las sesiones” | `extension_usage` with `scope=all` | Global observed calls, with no injected Local filter. |
| “¿Qué recomiendas para cambiar de modelo?” | Selected status/usage plus relevant global analytics | Advice tied to evidence and its limits; no automatic model change. |
| “Aplica el perfil ahorro” | Explicit action path | A mutation may be requested; a pending receipt is not confirmation of completion. |

Examples describe the tool contract and tested request classification. They do
not claim a real model always selects the ideal tool sequence.

## Verification

On 2026-09-13 the following command passed 93 tests against the worktree changes:

```sh
pytest -q tests/test_operator_analytics.py tests/test_operator_dispatch.py \
  tests/test_operator_tools.py tests/test_operator_agent_loop.py \
  tests/test_operator_stream.py tests/test_operator_recovery.py \
  tests/test_operator_chat.py tests/test_operator_catalog.py
```

Python 3.11 compilation passed for the changed operator modules and `bin/cc-dash`.
`git diff --check` also passed. New regression cases were run failing before their
corresponding fixes. Existing receipt tests verify that a mutation has a pending
receipt before dispatch and that closing the batch stops subsequent mutations.

The HTTP tests compile the shipped `do_POST` handler and use in-memory requests.
The agent-loop tests use scripted provider events and temporary chat stores.
No inference request, user session mutation, provider configuration change or
browser execution was used for this audit.

## Limits and integration

- `/state` must include Local and accurate `alive`/pane identities. The root
  session/UI work owns that integration.
- `/session-brain` must validate the requested session/pane again when serving
  configuration. The root API work owns that validation.
- `/usage/state` contains some counters grouped by provider and folder. Filtering
  by pane does not convert those counters into exact per-pane attribution.
- `usage_analytics`, provider comparisons and dedication remain global APIs.
  No session-scoped experiment or dedication query was added.
- Scoped operator configuration/profile reads require a unique live pane. They
  return an explicit error when a session has several panes and none is selected.
- Real-provider behavior, physical remote interaction and the active installation
  require separate integration verification. This report covers the worktree.

## Provider fallback follow-up

The integration probe reported a real Haiku quota rejection. The same read-only
probe succeeded through Codex Spark: it called `session_status` against fixture
data and returned a grounded answer without actions. The catalogue schemas passed
local JSON Schema validation; no schema change was needed.

Provider failures now remain in the saved reply, including an explicit exhausted
quota message. An error received as the first SSE event permits fallback before
any output or tool execution. A successful fallback is named in the streamed and
saved response and is tried first in later tool rounds; the user's model
preference stays unchanged. Status and analysis requests offer only the catalogue's
read-only tools; explicit action requests retain the full catalogue.

Regression tests cover quota plus failed fallbacks, an initial SSE rejection,
Anthropic/OpenAI tool filtering, action requests, and a successful fallback that
executes a read-only tool and answers in the next round.
