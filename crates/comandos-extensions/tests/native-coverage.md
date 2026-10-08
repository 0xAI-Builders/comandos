# Native extension contract coverage

The native runner in `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/extension-o2-native/crates/comandos-extensions/tests/native_o2.rs` exercises the production binary with private HOME directories, owned child processes, local HTTP servers and real TLS. Default execution requires no Python interpreter. The retained Python scenarios remain available.

## Execution and boundaries

The runner has 17 groups, including 309 protocol model cases. It executes its own fixture mode through a harness-free test executable, preserving clean JSON framing. The protocol cases contain inputs and expected assertions, not prerecorded native outputs. Token counts use the checked-in public encoding; TLS uses a checked-in test CA and server certificate. No real credential, user database or existing session is used.

Five groups require Linux: metadata blocked slots, metadata six workers, metadata proxy rotation, metadata blocked output and check process signals. They inspect `/proc` or resize an owned pipe. Other platforms report these groups as skipped. This change has been exercised on Linux; macOS verification remains pending.

The retained resource forwarding fixture lacks required MCP result fields. Native coverage explicitly rejects `resources/list` without `resources` with `-32603`, and separately checks params/extras forwarding using valid resource, prompt and completion results. Production validation remains enabled.

The source `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/extension-o2-native/crates/comandos-extensions/src/transport.rs` sets `MAX_RESPONSE` to 256 MiB. The retained Python oversized-upstream scenario expects rejection at 8 MiB. Native coverage checks acceptance and preservation of the 8 MiB field at the current production threshold. HTTP rejection above 256 MiB is not covered here. Downstream rejection at 8 MiB is covered.

Manual RSS measurement scripts and developer oracle record/check commands remain outside this scenario port. Python retirement is not asserted.

## Scenario mapping

Each source function maps to a native function below. Parameterized inputs are exercised in the mapped native function. The protocol model function covers all 309 captured input/assertion cases.

Source: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/extension-o2-native/crates/comandos-extensions/tests/test_metadata.py`

| Retained function | Native function |
|---|---|
| `test_only_complete_filtered_tool_list_records_size` | `metadata_complete_filtered_list` |
| `test_cursor_budget_exhaustion_preserves_frames_and_only_records_fresh_list` | `metadata_cursor_budget` |
| `test_shared_slots_block_before_allocating_tables_and_release_on_kill` | `metadata_blocked_slots` |
| `test_many_count_processes_never_hold_more_than_two_slots` | `metadata_many_workers` |
| `test_multiple_proxies_measure_once_per_configuration_rotation` | `metadata_proxy_rotation` |
| `test_slots_remain_owned_until_blocked_output_helper_exits` | `metadata_blocked_output` |

Source: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/extension-o2-native/crates/comandos-extensions/tests/test_check.py`

| Retained function | Native function |
|---|---|
| `test_disabled_browser_and_validation_never_spawn` | `check_scheduling_and_admission` |
| `test_tool_counts_ignore_filters_and_cap_pagination` | `check_native_contracts` |
| `test_minimal_google_reads_and_mailbox_identity` | `check_native_contracts` |
| `test_mailbox_wrong_and_correct_fields` | `check_native_contracts` |
| `test_failures_are_safe_and_meaningful` | `check_native_contracts` |
| `test_http_status_and_secret_body_suppression` | `http_check_status_and_sessions` |
| `test_completion_stream_request_order_duplicates_and_four_concurrent` | `check_scheduling_and_admission` |
| `test_catalog_version_rejected_without_spawning` | `check_scheduling_and_admission` |
| `test_many_immediate_results_use_backpressure` | `check_scheduling_and_admission` |
| `test_invalid_protocol_shapes_fail` | `check_native_contracts` |
| `test_profile_json_matches_python_duplicates_and_nonfinite` | `check_native_contracts` |
| `test_native_http_session_and_sse_completion_cleanup` | `http_check_status_and_sessions` |
| `test_cli_signal_reaps_children` | `check_process_contracts` |
| `test_stdout_saturation_signal_exits_without_hanging_or_panicking` | `check_process_contracts` |
| `test_python_falsy_disabled_values_do_not_spawn` | `check_native_contracts` |
| `test_normal_completion_reaps_child` | `check_process_contracts` |
| `test_check_auth_failure_and_static_header_precedence` | `http_auth_precedence_and_rotation` |
| `test_cleanup_uses_latest_rotated_shared_credential` | `http_auth_precedence_and_rotation` |
| `test_sdk_protocol_model_boundaries` | `check_protocol_cases` |

Source: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/extension-o2-native/crates/comandos-extensions/tests/test_serve.py`

| Retained function | Native function |
|---|---|
| `test_direct_exec_preserves_pid_arguments_environment_and_exit` | `serve_subprocess_and_limits` |
| `test_failures_do_not_expose_configuration` | `serve_subprocess_and_limits` |
| `test_proxy_protocol_filter_pagination_and_results` | `serve_protocol_frames` |
| `test_empty_enabled_tools_denies_everything` | `serve_protocol_frames` |
| `test_tools_list_preserves_entire_error_envelope` | `serve_protocol_frames` |
| `test_concurrent_requests_keep_original_ids` | `serve_protocol_frames` |
| `test_filtered_child_stops_on_eof_and_signal` | `serve_subprocess_and_limits` |
| `test_upstream_crash_exits_without_downstream_eof` | `serve_subprocess_and_limits` |
| `test_oversized_downstream_frame_is_bounded_and_closes` | `serve_subprocess_and_limits` |
| `test_cancellation_reaches_filtered_upstream` | `serve_subprocess_and_limits` |
| `test_static_header_precedence_ignores_shared_auth` | `http_serve_boundaries` |
| `test_shared_401_retries_once_only_with_new_token` | `http_serve_boundaries` |
| `test_redirect_never_forwards_credentials` | `http_serve_boundaries` |
| `test_legacy_sse_foreign_endpoint_rejected` | `http_serve_boundaries` |
| `test_invalid_catalog_error_is_sanitized` | `serve_startup_boundaries` |
| `test_unsupported_commands_fail_explicitly` | `serve_startup_boundaries` |
| `test_request_limit_is_explicit_and_cancellation_frees_slot` | `serve_subprocess_and_limits` |
| `test_oversized_upstream_body_returns_sanitized_error` | `http_serve_boundaries (8 MiB acceptance; rejection above 256 MiB remains pending)` |
| `test_stream_response_answers_upstream_ping_without_claiming_sampling` | `http_serve_boundaries` |
| `test_stream_sse_accepts_cr_line_endings_and_multiline_data` | `http_serve_boundaries` |
| `test_serve_accepts_unrelated_nonfinite_catalog_value` | `serve_startup_boundaries` |
| `test_direct_exec_nonfinite_environment_matches_python` | `serve_startup_boundaries` |

Source: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/extension-o2-native/crates/comandos-extensions/tests/test_https_oauth.py`

| Retained function | Native function |
|---|---|
| `test_tls_discovery_refresh_post_and_401_retry` | `tls_oauth_refresh_and_redirect` |
| `test_tls_token_redirect_is_not_followed` | `tls_oauth_refresh_and_redirect` |

Source: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/extension-o2-native/crates/comandos-extensions/tests/test_json_values.py`

| Retained function | Native function |
|---|---|
| `test_arbitrary_integers_and_opaque_keys_survive_proxy` | `serve_protocol_frames` |
