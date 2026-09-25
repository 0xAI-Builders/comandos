# Codex CLI 0.155.0: per-session MCP and Skills control

Source pinned to tag `rust-v0.155.0`, commit `f0a1b8f0849d90960bc406b848f32e5a129b0457` (paths below are relative to `codex-rs/`, at github.com/openai/codex/tree/f0a1b8f/codex-rs). "Mock model" means a local Responses-API stub selected with `-c model_provider=mock2`. The stub captured each request and returned scripted `exec` tool calls. Real model calls were impossible: `~/.codex/auth.json` has a revoked refresh token (`refresh_token_invalidated`) [EMPIRICAL].

## 1. MCP per session

| Claim | Source |
|---|---|
| `enabled=false` servers are never started. The connection manager iterates only `server.enabled()` | [SOURCE codex-mcp/src/connection_manager.rs:241-290, catalog.rs:471] |
| `-c` keys are split naively on `.`, and TOML quoting is NOT supported | [SOURCE config/src/overrides.rs:22 `path.split('.')`] |
| Dashes work unquoted: `-c mcp_servers.mcp-image.enabled=false` gives `mcp-image: False` | [EMPIRICAL `codex mcp list --json -c ...`] |
| Quoted form breaks: `-c 'mcp_servers."mcp-image".enabled=false'` fails with `invalid transport in mcp_servers."mcp-image"`. The quotes become part of the name. Server names containing `.` cannot be targeted via `-c` at all | [EMPIRICAL] |
| You can add a server: `-c 'mcp_servers.tmpadd={command="echo",args=["hi"]}'` lists `tmpadd: True` | [EMPIRICAL] |
| You cannot clear servers: `-c 'mcp_servers={}'` still lists 34 servers, because the CLI layer is deep-merged. To "limit to a set", disable every other server explicitly | [EMPIRICAL; SOURCE overrides.rs `merge_toml_values`] |
| Per-tool limits: `enabled_tools` / `disabled_tools`. `-c 'mcp_servers.qcdr-search.enabled_tools=["qcdr_search","qcdr_fetch"]'` leaves `ALL_TOOLS` = `["mcp__qcdr_search__qcdr_fetch","mcp__qcdr_search__qcdr_search"]` | [SOURCE config/src/mcp_types.rs:246-251; DOC learn.chatgpt.com/docs/config-file/config-reference; EMPIRICAL mock model] |
| The built-in `codex_apps` MCP (43 tools) is separate. Remove it with `--disable apps` | [EMPIRICAL mock: A = `{codex_apps:43, qcdr_search:4}`, B = `{moneyhacktracker:3}`] |
| `-p NAME` (root-level flag, before the subcommand) layers `$CODEX_HOME/NAME.config.toml`. NAME must match `[A-Za-z0-9_-]`. The file lives inside CODEX_HOME. A missing file is silently accepted (UNVERIFIED that it is truly a no-op, but no error was printed) | [SOURCE core/src/config/mod.rs:1952; protocol/src/config_types.rs:146; EMPIRICAL temp `CODEX_HOME`: profile disabled `qcdr-search`, added `onlyinprofile`] |
| `codex mcp list -p x` is rejected. It must be `codex -p x mcp list` | [EMPIRICAL] |
| Are tool definitions in the model context? In 0.155, MCP tools are **deferred** nested tools of the code-mode `exec` tool. They are not in the prompt, and the model discovers them through `ALL_TOOLS`. Request bytes were identical (77,461) with 0, 1 or 2 servers enabled. Disabling a server removes it from `ALL_TOOLS` and callable `tools.*` | [SOURCE core/src/mcp_tool_exposure.rs:90 `ToolExposure::Deferred`; EMPIRICAL capture] |

## 2. Skills per session

- The key is `skills.config`, an array of `{path=..., enabled=bool}` or `{name=..., enabled=bool}`. Using both selectors, or neither, is ignored. Rules apply in order, so later rules win [SOURCE config/src/skills_config.rs].
- **Only the User and SessionFlags (`-c`) layers are honored**, and User includes `-p` profiles. Project `.codex/config.toml` `skills.config` is ignored [SOURCE skills_config.rs `skill_config_rules_from_stack`].
- A `-c` inline array works: `-c 'skills.config=[{name="caveman",enabled=false},{path="~/.agents/skills/tdd/SKILL.md",enabled=false}]'` shrank the catalog from 85 to 83 entries [EMPIRICAL `codex debug prompt-input`].
- `path` must be the **SKILL.md file**. A folder path had no effect, even though the docs say "Path to a skill folder" [EMPIRICAL; DOC config-reference]. The `name` selector is missing from the docs but works, including namespaced names like `humanizer:humanizer`.
- There is no allow-list or wildcard. Other switches:
  - `skills.include_instructions=false` drops the whole catalog.
  - `skills.bundled.enabled=false` disables system skills (85 to 80 entries).
  - `-c 'plugins.visualize@openai-bundled.enabled=false'` removes a whole plugin (its skills, and its MCP servers per the catalog code). Plugin IDs containing `.` are unreachable.
  - `skills.max_context_tokens` sets the catalog budget [SOURCE; EMPIRICAL].
- Discovery roots [SOURCE ext/skills/src/host_roots.rs; DOC learn.chatgpt.com/docs/build-skills]: `$CODEX_HOME/skills` (deprecated), `~/.agents/skills`, `$CODEX_HOME/skills/.system`, `/etc/codex/skills`, project `.codex/skills`, `.agents/skills` from project root to cwd, plugin roots in `$CODEX_HOME/plugins/cache/...`, and extra roots set only through the app-server `skills/extraRoots/set` call.

## 3. Resume with a different set

`codex exec resume <ID> --disable apps -c mcp_servers.qcdr-search.enabled=false -c 'skills.config=[...]' ...` works. Results [EMPIRICAL mock, thread `01a0d5cb-4285-7422-99a7-7ab05b745dd9`]:

- `ALL_TOOLS` switched from `{codex_apps:43, qcdr_search:4}` to `{moneyhacktracker:3}`.
- A new `world_state` diff (`full=false`, key `host_skills`) was appended with the new catalog: `caveman` back, `humanizer:humanizer` gone.
- **The old catalog stays in the history**, so the model sees both until compaction.

For the TUI, `codex resume` merges resume-scoped `-c` flags "with highest precedence" [SOURCE cli/src/main.rs:2869-2896]. The TUI path itself was not tested empirically.

## 4. Introspection

- **argv**: `/proc/<pid>/cmdline` shows the `-c` and `-p` flags.
- **Without a model**:
  - `codex [-p X] mcp list --json -c ...` gives name, enabled and transport per server.
  - `codex [-p X] debug prompt-input -c ...` renders the exact skills catalog and instructions. It does not render tools.
- **Rollout** `~/.codex/sessions/YYYY/MM/DD/rollout-*-<id>.jsonl`:
  - `session_meta` records cwd, originator, cli_version and model_provider, but no MCP list.
  - `world_state.state.host_skills.body` is the exact skill catalog, re-emitted as a diff on resume.
  - `turn_context.disabled_plugin_ids` lists disabled plugins.
  - MCP servers loaded are **not** recorded [EMPIRICAL].
- **Logs**: `~/.codex/log/` is empty. Logs live in `~/.codex/logs_2.sqlite`, table `logs` (thread_id, target, body). There are unstructured `rmcp` lines, not a server list [EMPIRICAL].

## 5. Usage logging in rollouts

- **MCP call**: `{"type":"event_msg","payload":{"type":"item_completed","thread_id","turn_id","item":{"type":"McpToolCall","server":"whatsapp","tool":"list_chats","arguments":{...},"status":"completed","duration":{...}}}}`. The server name is the config name (dash kept). In the model's JS it is normalized to `mcp__qcdr_search__...`. The calls run inside `custom_tool_call` name `exec`, so do not look for `function_call server__tool` [EMPIRICAL: 7,003 `McpToolCall` items and 20,449 `exec` calls across 20 days of rollouts; no `mcp_tool_call_begin` events persisted].
- **Skill use (implicit)**: `item_completed` → `CommandExecution` with `parsed_cmd[].type=="read"` and `path` ending in `/SKILL.md` (for example `cat ~/.agents/skills/using-superpowers/SKILL.md`). Codex's own detector uses the same heuristic [SOURCE ext/skills/src/invocation.rs; EMPIRICAL].
- **Skill use (explicit `$skill`)**: a user `response_item` message containing `<skill><name>..</name><path>..</path>` [SOURCE ext/skills/src/fragments.rs].
- **Namespaced skills tools** (`skills.list`/`skills.read`) would appear as `function_call` with namespace `skills`. None were seen here (UNVERIFIED when they are active).

## 6. Token cost

- **Skills catalog**: 22,104 chars with 85 entries; the whole prompt preamble drops from 34,900 to 12,796 chars with `include_instructions=false` [EMPIRICAL prompt-input]. That is about 5.5k tokens at 4 chars per token (UNVERIFIED tokenization). The catalog is **budget-capped** at 2% of the context window: 258,400 × 2% ≈ 5.2k tokens. Disabling 2 skills saved only 6 chars, because descriptions re-expand to fill the budget [DOC build-skills; SOURCE skills_config.rs]. Only `include_instructions=false` or a lower `max_context_tokens` actually saves tokens.
- **MCP**: roughly 0 upfront tokens per server, because tools are deferred. The fixed `additional_tools` block is 25,490 chars, of which the `exec` description is 13,982 [EMPIRICAL capture]. Cost appears only when the model enumerates `ALL_TOOLS` or calls a tool.
- **Exact measurement**:
  - Per turn: `event_msg token_count.info.last_token_usage.input_tokens` in the rollout; compare A/B sessions.
  - Without a model: point `model_provider` at a capture stub and diff request bytes.

## Recommended launch recipe

```
# launch (embedded app-server is guaranteed because -c is non-empty; see risks)
codex -C <dir> --disable apps \
  -c mcp_servers.<each-unwanted>.enabled=false \      # enumerate from `codex mcp list --json`
  -c 'mcp_servers.<kept>.enabled_tools=["t1","t2"]' \ # optional
  -c 'skills.config=[{path="/abs/.../SKILL.md",enabled=false},{name="plugin:skill",enabled=false}]' \
  -c 'plugins.<id>@<marketplace>.enabled=false'        # optional, per plugin
# resume same thread with a new set
codex resume <SESSION_ID> --disable apps -c mcp_servers.<x>.enabled=false -c 'skills.config=[...]'
```

For large sets, an alternative is a profile file `$CODEX_HOME/comandos-<set>.config.toml` passed as `codex -p comandos-<set> ...` or `codex -p comandos-<set> resume <ID>`. This writes a new file into `~/.codex`, but does not modify `config.toml`.

## Open risks

- **Shared daemon**: a TUI launched with no `-c` and no `-p` may attach to a shared local app-server daemon if one is running, and then ignores per-launch intent. Any `-c` forces the embedded server [SOURCE tui/src/lib.rs:988 `can_reuse_implicit_local_daemon`].
- **Deny-list only**: newly added global servers or skills leak into sessions unless ComandOS re-enumerates the set at every launch.
- **Unreachable names**: server or plugin names containing `.` cannot be addressed via `-c`, only via a profile file.
- **Stale catalog on resume**: the old skills catalog stays in the history (extra tokens, and it can mislead the model).
- **Disabled skills stay readable**: a disabled skill's SKILL.md is still readable by path. Whether explicit `$name` for a disabled skill is rejected is UNVERIFIED.
- **Untested paths**: TUI resume is verified in source only. Live-model behaviour is unverified because Codex auth is revoked here; the user must re-login. Under-development features (`mcp_2026_07_28`, `deferred_tool_world_state`) may change these shapes.

Integrity: `sha256sum ~/.codex/config.toml` gives `bc56bba3...cee` both before and after (unchanged). The test sessions written by Codex to `~/.codex/sessions/2026/09/24/` are `01a0d5c9-5760...`, `01a0d5ca-9f0a...`, `01a0d5cb-4285...` and 3 other mock sessions.
