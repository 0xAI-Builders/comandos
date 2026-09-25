# Grok Build CLI 1.0.41: controlling MCP servers and skills for one session

Tests ran in `/tmp/claude-1000/ext-research-grok/cwd` with no leader socket. Real `~/.grok/config.toml` sha256 `006c294b…3cee` matched the before checksum at the end.

Model calls fail with **402 "Grok Build usage balance exhausted"**, so no model turn completed. Grok still resolves and connects MCP servers, writes `events.jsonl`, `chat_history.jsonl` and `tool_definitions.json`, and loads history before the API call. The claims below come from those artifacts.

## 1. GROK_CONFIG overlay, --deny and --tools

- The overlay cannot exclude MCP servers or skills. Its allowlist covers `models`, `features`, `shell_environment_policy` filters and a "narrowed `toolset`" [DOC 05-configuration.md#Injecting config]. Only three `toolset` keys are marked "Overlay-allowlisted": `bash.login_shell_capture` and `web_search.allowed_domains`/`excluded_domains` [DOC 26-config-reference.md#toolset]. `toolset` has no key for tool inclusion.
  [EMPIRICAL: `GROK_CONFIG='{"disabled_mcp_servers":["gmail"],"skills":{"disabled":["caveman"]},"mcp_servers":{"telegram":{"enabled":false}}}' grok inspect` → "Config Sources … env_overlay: $GROK_CONFIG", but gmail, telegram and caveman are not tagged `[disabled]`, and `grok mcp list` shows gmail enabled. The keys are dropped without a warning.]
- `--deny` only gates calls; it does not stop a server from loading. The syntax is `MCPTool(server__*)` or `MCPTool(server__tool)`, and the Claude spelling `mcp__server`, `mcp__server__tool` or `mcp__*` is also accepted [DOC 22-permissions-and-safety.md#MCP Rules]. The flag works in both TUI and headless mode.
  [EMPIRICAL: `grok --deny 'mcp__gmail' -p …` → events.jsonl `mcp_server_connected gmail tool_count 15`.] UNVERIFIED: whether `search_tool` hides denied tools.
- MCP tools never enter the function list. The model reaches them through the meta-tools `search_tool`/`use_tool` [DOC 07#Tool Discovery; 17#Storage Layout]. `--tools`/`--disallowed-tools` apply to built-in tools, **headless only** [DOC 14#Command-Line Options].
  [EMPIRICAL: `--disallowed-tools search_tool,use_tool -p …` → tool_definitions.json 24 tools, without search_tool/use_tool. The servers still started.]

## 2. Shadow GROK_HOME

- It works for MCP servers and skills.
  [EMPIRICAL: shadow with every entry symlinked except a copied `config.toml` that sets `disabled_mcp_servers=[…]` and `[skills] disabled=["caveman","gmail"]`, `ignore=["~/.grok/skills/humanizer"]`.
  `GROK_HOME=<shadow> grok mcp list` → 11 servers marked "(disabled)". The real `grok mcp list` shows 0 marked "(disabled)".
  `grok inspect` → "Skills (116)", `caveman user [disabled]`, `gmail user [disabled]`, humanizer hidden; "Config Sources └ User: <shadow>/config.toml".]
- `grok inspect` does **not** tag servers disabled by `disabled_mcp_servers`. Use `grok mcp list` or events.jsonl instead [EMPIRICAL].
- `disabled_mcp_servers` also covers Claude-compat server names [EMPIRICAL: events `mcp_config_resolved … "disabled":["radek","x-suite",…]`].
- **Grok writes by atomic rename, which replaces symlinks in the shadow with regular files.** After one `-p` run, `auth.json`, `settings_cache.json` and `models_cache.json` were regular files [EMPIRICAL: `find <shadow> -maxdepth 1 ! -type l`].
  - **auth.json matters most.** The run refreshed the expired OAuth token and rotated the refresh token. The new pair went only to the shadow, and the real `auth.json` kept the old refresh token [EMPIRICAL: different sha256, `create_time 23:33:01Z` in the shadow copy vs `17:15:39Z` in the real one]. The refreshed file is preserved at `<scratch>/grok-auth-refreshed-2026-09-24T2333Z.json`. If a later launch with the real home fails to authenticate, run `cp -p` from that file or `grok login`.
- Other writes: a symlinked `sessions/` receives the session dirs, `sessions/session_search.sqlite` and `<cwd>/prompt_history.jsonl`; `logs/unified.jsonl` is appended; the TUI creates `active_sessions.json`, `worktrees.db` and a few small state files [EMPIRICAL].
- Leader: `-p` and TUI runs created no `leader.sock` (`[cli] use_leader` is off by default) [DOC README:304; EMPIRICAL]. Pass `--leader-socket <private>` anyway.
- Native hooks in `$GROK_HOME/hooks` fire, for example `comandos.json` → `cc-notify.sh` on StopFailure. **One desktop or Telegram notification probably fired for run 1.**

## 3. Compat sources

The environment variables `GROK_{CLAUDE,CURSOR}_{MCPS,SKILLS,HOOKS,RULES,AGENTS}_ENABLED=false` are resolved as env > config.toml > default [DOC 05#Harness compatibility; 26#compat].
[EMPIRICAL: with them set, `grok inspect` shows "mcps OFF (env)", the 8 `~/.claude.json` servers `[claude] [disabled]` and the Claude skills plus the Claude-plugin skills (superpowers and others) `[disabled]`.]

Claude-sourced items can be removed selectively by name with `disabled_mcp_servers` or `skills.disabled`. A `.mcp.json` in the project root also loads [DOC 07#Compatibility].

## 4. Resume with a different set

[EMPIRICAL: session `01a0d5c3…` was created under shadow A (gmail, qcdr-search). `GROK_HOME=shadowB grok -r 01a0d5c3… -p …`: same session ID, `turn_number 1`, `conversation_message_count 5`, and B's set was applied.
events: `mcp_config_resolved servers [moneyhacktracker, qcdr-search]`.
chat_history appended a reminder: "MCP server connected: moneyhacktracker … MCP server disconnected: gmail".]

- **Skills are not re-announced.** The skills reminder (chat_history message 2, a `<system-reminder>` listing 106 skills that excludes disabled and ignored ones) stays as rendered at creation. B's changes (tdd disabled; caveman, gmail and humanizer re-enabled) produced no delta.
- UNVERIFIED: whether B's `skills.disabled` blocks invoking a skill that is still listed.
- For a correct skill list, start a new session, or use `--fork-session`. UNVERIFIED: whether a fork re-renders the list.

## 5. Introspection and usage logging

- For each session, `~/.grok/sessions/<urlenc-cwd>/<id>/events.jsonl` records:
  - `mcp_config_resolved {servers, disabled}`
  - `mcp_server_starting` and `mcp_server_connected {server_name, tool_count, tools[]}`
  - `mcp_server_failed`, `mcp_init_completed`
  - `mcp_tool_call_started` and `mcp_tool_call_completed {server_name, tool_name, duration_ms, success}` [EMPIRICAL]
- In `updates.jsonl`, an MCP call appears as `tool_call` with `title:"use_tool"` and `rawInput.tool_name:"server__tool"`. Its `tool_call_update` is retitled to `chrome-bg__evaluate_script`, which is the source of the ComandOS names [EMPIRICAL: title tally].
- Skill use has no dedicated tool. When the model loads a skill, it shows as `read_file` of `…/SKILL.md` [EMPIRICAL]. UNVERIFIED: how a `/skill` invocation typed by the user is recorded.
- The skills and connected-MCP lists that were shown to the model are in `chat_history.jsonl` system-reminders. `grok inspect --json` is static (config only); `/session-info` shows the session.

## 6. Hot toggle

[EMPIRICAL: TUI driven in a pty under an isolated shadow, `/mcps` → Space on gmail → the shadow `config.toml` gained `disabled_mcp_servers = ["gmail"]` and `[mcp_servers.gmail] enabled = false`, and `[skills]` was reformatted.]

- `/mcps` persists to `$GROK_HOME/config.toml`. Against the real home that makes a toggle global.
- Under a shadow with a copied config, a toggle stays in that session. Keeping the shadow across resumes keeps the set.
- `grok mcp enable/disable` persists the same way [DOC 07#CLI Management].

## Recommended launch recipe

**A (preferred): real GROK_HOME with the config bind-mounted in a private user and mount namespace.**
Auth, sessions, caches and locks all stay real, so the token-rotation problem does not occur.
[EMPIRICAL: `grok-ns.py cfg.toml sh -c 'grep CapEff /proc/self/status; grok mcp list'` → `CapEff: 0000…`, only qcdr-search enabled. The real `mcp list` was unchanged.
Config writes inside the namespace fail: "failed to write …/config.toml: Resource busy (os error 16)". That means `/mcps` and `grok mcp disable` cannot persist.]

The requirement `kernel.apparmor_restrict_unprivileged_userns = 0` holds here.

```sh
python3 ~/…/grok-ns.py /run/comandos/<sid>/grok-config.toml \
  env GROK_CLAUDE_MCPS_ENABLED=false GROK_CURSOR_MCPS_ENABLED=false \
      GROK_CLAUDE_SKILLS_ENABLED=false GROK_CURSOR_SKILLS_ENABLED=false \
  ~/.grok/bin/grok --leader-socket /run/comandos/<sid>/leader.sock [-r <session-id>]
```

- `grok-config.toml` is the real `config.toml` plus a top-level `disabled_mcp_servers=[…]` and `[skills] disabled=[…]`/`ignore=[…]`. Regenerate it at each launch from the current real config.
- The helper is at `<scratch>/grok-ns.py`. It calls `unshare(CLONE_NEWUSER|CLONE_NEWNS)`, maps only the user's uid, makes `/` private, bind-mounts the file over `$GROK_HOME/config.toml` and execs the command with no capabilities.

**B (fallback): shadow GROK_HOME.**

- **Copy:** `config.toml` (modified).
- **Private, never symlinked:** `hooks/` (or symlink it on purpose), `active_sessions.json` and `.lock`, `leader*.sock`, `settings_cache.json`, `models_cache.json`.
- **Symlink:** `sessions/`, `memory/`, `memory-v2/`, `skills/`, `bundled/`, `installed-plugins/`, `marketplace-cache/`, `docs/`, `bin/`, `downloads/`, `vendor/`, `grove/`, `logs/`, `AGENTS.md`, `trusted_folders.toml`, `mcp_credentials.json` and `.lock`, `worktrees.db`, `agent_id`, `version.json`.
- **auth.json:** symlink it together with `auth.json.lock`, and after the process exits, if the shadow `auth.json` has become a regular file with a newer `create_time`, copy it back to `~/.grok/auth.json` atomically under `flock ~/.grok/auth.json.lock`. The shared lock does not prevent the two stores from diverging.
- UNVERIFIED: whether `mcp_credentials.json` is also rewritten by rename (MCP OAuth refresh).

## Open risks

- **The real `~/.grok/auth.json` may now hold a used refresh token** (see item 2). A fix is ready if needed.
- Rotating refresh tokens make approach B unsafe with several concurrent shadows unless the auth sync described above is added.
- Namespace side effects (approach A): `sudo` and other setuid tools do not work inside. Files owned by other uids appear as `nobody`. Nested sandboxing with bwrap was not tested.
- Skills listing is frozen at session creation, so resume shows the old list (item 4).
- A grok.com managed connector ("Automations (managed)") appears in `/mcps`. It is controlled server-side or through `GROK_MANAGED_MCP_GATEWAY_TOOLS_ENABLED`. UNVERIFIED.
- Test leftovers in the real home: `~/.grok/sessions/%2Ftmp%2Fclaude-1000%2Fext-research-grok%2Fcwd/` (session 01a0d5c3…), a session_search.sqlite index row and `logs/unified.jsonl` lines. They were left in place to avoid deleting inside the real home.
