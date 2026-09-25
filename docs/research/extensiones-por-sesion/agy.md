# agy 1.2.9 (Antigravity CLI): per-session MCP and Skills control

Legend: [DOC] = builtin docs shipped in `~/.gemini/antigravity-cli/builtin/skills/agy-customizations/docs/*.md` or antigravity.google pages; [CHANGELOG] = `agy changelog`; [SOURCE] = `strings` on the binary; [EMPIRICAL] = command I ran. The test root was /tmp/claude-1000/ext-research-agy.

## 1. Where agy reads config from, and relocation
- MCP: only `$HOME/.gemini/config/mcp_config.json` (global) and `plugins/<p>/mcp_config.json` [DOC mcp_servers.md]. No workspace-level mcp_config is documented.
- Skills: builtin skills are mounted by name. Only `agy-customizations` and `antigravity-guide` show up in the prompt [EMPIRICAL]. Global discovery for the CLI is `~/.gemini/config/skills/` [EMPIRICAL: probe skills placed there were listed]; strace also shows `~/.gemini/antigravity-cli/skills`, `~/.gemini/skills` and `~/.gemini/plugins` being probed. Other sources: workspace `.agents|.agent|_agents|_agent/skills` (walked up to the repo root), and `skills.json` / `plugins.json` manifests in `~/.gemini/config/` or `.agents/` [DOC SKILL.md, json_configs.md].
- Paths `agy -p` probes at startup [EMPIRICAL: strace]: `~/.gemini/config/{mcp_config.json, hooks.json, config.json, skills/, skills.json, skills.txt, plugins/, plugins.json, agents/, agents.json, rules/, rules.json, global_workflows, projects/}` and `~/.gemini/antigravity-cli/{settings.json, keybindings.json, mcp_oauth_tokens.json, plugins/, plugin_data, secplugins/config.json, skills/}`.
- Relocation: there is no flag. All paths come from `$HOME`. [EMPIRICAL] Setting `ANTIGRAVITY_APP_DATA_DIR`, `JETSKI_APP_DATA_DIR`, `XDG_CONFIG_HOME` or `GEMINI_CLI_HOME` to a temp dir still made `agy mcp list` open `~/.gemini/config/mcp_config.json` (strace). With `HOME=<shadow>` it opened the shadow copy. The `*_APP_DATA_DIR` vars do exist in the binary [SOURCE]; whether they move `antigravity-cli/` is UNVERIFIED and does not affect config/.
- The only other env vars in the binary are UI vars (`AGY_CLI_*`), `AGY_PROXY_URL`, `AGY_GATEWAY_URL`, `AGY_ADC_AUTH` and `AGY_CA_CERT` [SOURCE].

## 2. Per-session MCP subset
- No flag exists (`agy help`). `agy mcp enable/disable` writes the global config.
- A HOME shadow works [EMPIRICAL]. `HOME=$S agy mcp list` showed only `telegram` while the real `agy mcp list` still showed all 6 servers. A per-server `"disabled": true` in the shadow copy shows `gmail http disabled`.
- It also applies in a model session. `HOME=$S agy -p ... --output-format json` answered `MCP: telegram`. After swapping the shadow file, the resumed session answered `MCP: gmail, whatsapp`.
- The first run printed `{"conversation_id":"6efccada-…","status":"SUCCESS",…,"usage":{…}}`.
- Where agy writes during a session [EMPIRICAL: strace]:
  - `antigravity-cli/conversations/<id>.db` (SQLite WAL) and `conversation_summaries.db`. SQLite follows symlinks: the -wal files were opened at the real `/home/...` path.
  - `brain/<id>/…` (artifacts and transcripts), `annotations/<id>.pbtxt`, `presence/<id>.lock`, `cache/last_conversations.json` (cwd to id map), `log/cli-*.log`, `mcp/<server>/*.json` (tool schema cache).
  - Files replaced by tmp+rename, so a symlink to them becomes a local copy: `jetbox_summaries_proto.pb` and `cli.log` (re-pointed to `log/…`).
  - `builtin/` is unlinked and re-extracted when its checksum is stale. That only removes the shadow symlink; the real builtin was untouched.
- Auth kept working with `~/.gemini/oauth_creds.json` and `google_accounts.json` symlinked.

## 3. Skills per session
- Discovery is progressive: only name and description go into the prompt [DOC].
- Per-skill disable works through `~/.gemini/config/skills.json` [DOC json_configs.md] [EMPIRICAL]. In the shadow, `{"exclude":["zz-beta-probe"],"entries":[{"path":"/…/extra-skills"}]}` removed the globally discovered beta skill and added gamma. The resumed session listed `agy-customizations, antigravity-guide, zz-alpha-probe, zz-gamma-probe`.
- `include_only` and `inherits` are also documented. Excluding builtin skills: UNVERIFIED.
- The simplest control in a shadow is to put only the chosen skills (as symlinks) in `$S/.gemini/config/skills/`.
- Workspace `.agents/skills` loaded only when the dir was passed with `--add-dir` (zz-delta appeared). With bare cwd, hooks showed `workspacePaths: []`.
- Plugins are toggled per directory name in `~/.gemini/config/config.json` → `plugins.<dir>.enabled` [DOC plugins.md]. In the shadow, copy that file instead of symlinking it. The user has none installed (`agy plugin list` → "No imported plugins.").
- A custom agent (`--agent`, `~/.gemini/config/agents/<n>.md` with frontmatter `mcpServers`, `skills`, `tools`) is another route [DOC antigravity.google/docs/subagents]. UNVERIFIED whether it replaces or adds to the global MCP set.

## 4. Resume with a different set
[EMPIRICAL] `HOME=$S agy --conversation 6efccada-3f30-4cb7-86b6-2747940e3a06 --add-dir $R/ws -p "…"` found the conversation, kept its history, and applied the new MCP and skills set from 4 model calls. The conversation stayed visible in the real store: `conversation_summaries.db` row `6efccada…|17||antigravity-cli`. The history is shared because `conversations/`, `brain/` and the summaries DB are symlinked.

## 5. Introspection and usage logging
- Per conversation, `brain/<id>/.system_generated/logs/transcript_full.jsonl` has one JSON line per step with `step_index, source, type, status, created_at, content, tool_calls, error`.
- An MCP call appears as `{"name":"call_mcp_tool","args":{"ServerName":"whatsapp","ToolName":"get_status","Arguments":{}}}`, followed by a `GENERIC` result step [EMPIRICAL].
- Skill use is not a dedicated step. It shows up as `view_file` on `…/<skill>/SKILL.md` [EMPIRICAL].
- Slash-invoked skills in the TUI may be recorded differently: UNVERIFIED.
- The binary DB is `conversations/<id>.db`: table `steps(step_type, step_payload protobuf…)`, and the payload strings contain the tool name and JSON args. The summaries DB has `step_count`, `workspace_uris` and `agent_name`.
- Token usage: only `--output-format json` returns `usage` per print-mode run. No usage data was found in the transcript.
- Hooks: stdin is camelCase with `conversationId, transcriptPath, artifactDirectoryPath, workspacePaths, modelName`, plus `toolCall{name,args}` and `stepIdx` on Pre/PostToolUse and `terminationReason` on Stop [EMPIRICAL].
- ComandOS `adapters/agy-hooks.sh` uses only PreInvocation ("working") and Stop ("done"). It keys on `workspacePaths[0]`, which is empty when no workspace is registered, and then it silently no-ops.
- Gotcha [EMPIRICAL]: a PreToolUse hook that returns `{}` denies the tool ("tool call denied by pre-tool hook"). It must return `{"decision":"ask"}` or `"allow"`.

## Recommended launch recipe
```
S=$RUNDIR/agy-home-<session>
# 1. every top-level entry of $HOME except .gemini -> symlink (HOME leaks to child shells: git, ssh, npm)
# 2. $S/.gemini/: symlink oauth_creds.json google_accounts.json installation_id state.json projects.json
#    settings.json trustedFolders.json trusted_hooks.json history tmp
# 3. $S/.gemini/config/: REAL dir. symlink hooks.json projects/ (and agents/, rules/ if present)
#    COPY+filter mcp_config.json (drop servers or set "disabled":true); COPY config.json (plugins map)
#    skills/ = real dir of symlinks to the chosen skills, and/or skills.json {"exclude":[…]}
# 4. $S/.gemini/antigravity-cli/: REAL dir. symlink conversations/ brain/ annotations/ cache/ log/ mcp/
#    presence/ implicit/ knowledge/ scratch/ crashes/ updater/ bin/ conversation_summaries.db
#    history.jsonl installation_id jetski_state.pbtxt settings.json keybindings.json mcp_oauth_tokens.json(if exists);
#    do NOT link builtin/ (agy extracts it); hooks.json -> ../config/hooks.json (shadow-relative)
cd <project> && HOME=$S agy [--conversation <id>] --add-dir <project> [-p …]
```
Do not symlink `*.db-wal` or `*.db-shm`: SQLite resolves the real path by itself.

## Open risks
- Child processes inherit `HOME=$S`. New top-level dotfiles they create land in the shadow, not the real home.
- Atomic-rename writes turn symlinks into local copies and cause silent divergence: `jetbox_summaries_proto.pb`, and possibly `settings.json` (trust or permission changes made in a shadow session) and `history.jsonl`. UNVERIFIED for interactive mode.
- Undocumented and brittle: the HOME-based path resolution could change in a future release. An auto-update (`last_check.timestamp`, `updater/`) runs from any session.
- Resumed sessions got a system notice that "all subagents and background tasks have been stopped due to server restart".
- The MCP set changes mid-conversation, so earlier tool calls can reference servers that no longer exist.
- Print runs took 60-110 s once MCP servers were attached (the first run with only telegram took 5 s). The cause is not isolated.
- Test residue left in the real store (it's agy runtime data, not config): conversation `6efccada-3f30-4cb7-86b6-2747940e3a06` (db, brain/, summary row, `cache/last_conversations.json` entry for the temp ws). Its transcript holds the WhatsApp `get_status` output. The user can delete it via `/resume`.

## Integrity
`sha256sum ~/.gemini/config/mcp_config.json` = 52acc065…3cee, identical to config-checksums-before.txt. The mtimes of `hooks.json`, `config.json` and `antigravity-cli/settings.json` are unchanged, and the real `agy mcp list` still shows 6 servers. The live `agy` (pid 403437) and remote-control were not touched.
