# T4 runtime — inventory tranche

2026-10-05. Functional first tranche after T5 b97b94e. Does not claim T4 launch/application completion.

Added extension_launch::internal_inventory and inventory with the agreed registry/harness/account/cwd/capabilities::Paths arguments, returning Result<(Context,Value),LaunchError> and Result<Value,LaunchError>. Ports the five supported adapters, native/shared MCP merging, configured disables, skill permissions, shared/plugin IDs and Claude plugin groups, shadowing/toggleability/errors/limitations. Sanitization retains explicit field order and removes raw configuration/path fields.

Reuses the existing comandos-extensions native metadata port: plugin/repository/configuration origins, skill sizes and shared MCP recorded sizes. Added tokenizer::offline_counts_at with explicit cache path, hash verification, existing bounds and no download/environment mutation. Inventory uses the existing async metadata constructor only with a synchronous Ready counter and one poll; there is no block_on, nested Tokio runtime or subprocess. An unexpected Pending declines. Tokenizer tables drop when the synchronous count ends. The caller supplies Paths environment snapshot including optional TIKTOKEN_CACHE_DIR; absent cache returns unknown sizes as Python does. The private cache is per request, avoiding global stale state.

The full launch config reader retains the Python 4 MB bound and full-object TOML behavior (separate from capabilities public projection). Nonblocking regular-file checks prevent configuration FIFO hangs. No shared configuration writes, agents, tmux, services, credentials or browser activity.

RED: missing inventory API prevented oracle compilation. GREEN: extension_inventory_oracle 3/3, including serialized internal and sanitized inventories for codex/claude/grok/opencode/agy, shared/plugin origins, cached MCP tokens, dotted MCP names, policy disables, incomplete configuration and unreadable catalog. Offline counter matches committed Python token fixture and rejects corrupt cache. Existing metadata suite 20/20 and profiles oracle 7/7 pass. The metadata suite initially lacked its public encoding fixture in this worktree; copied the existing verified public fixture from the main checkout into the ignored build directory, then all tests passed. No network fetch. Runtime all-target clippy -D warnings, fmt check and diff check pass.

Remaining authorized second tranche: prepare_launch, extension-only snapshot/apply and preserving an active extension plan during normal configuration changes, with private tmux application regressions. Server routes remain owned by root.

## Review follow-up — retained metadata cache

Parent review identified repeated rereading/recounting caused by constructing a fresh metadata cache per GET. Replaced it with a concurrency-safe bounded map of eight HOME/cache-path entries; each underlying metadata cache retains its existing 1024-file/1024-count bounds and failed-count 30-second TTL. A Mutex serializes construction/counting and retains no tokenizer tables. Monotonic timestamps preserve retry behavior. Regression RED against the prior committed code: after removing the encoding, the second GET lost its previous token count (1 -> null). GREEN retains the count, then invalidates it when the skill file changes. Inventory suite now 4/4, clippy/fmt/diff pass.
