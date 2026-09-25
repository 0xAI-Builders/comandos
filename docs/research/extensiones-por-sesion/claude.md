# Claude Code: per-session MCP and skill control (v2.1.282)

Method: I pointed each `claude -p` run at a local mock Messages API (`--settings '{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:18799"}}'`). The mock logged the real request body (the `tools[]` array and the skill-listing reminder) and cost $0. Throwaway fixtures: a stdio MCP server, `.mcp.json`, `.claude/skills/projskill`, and a `--plugin-dir` plugin (`testplug`, with MCP `plugmcp` and skill `plugskill`). Two real haiku calls confirmed the results. Artifacts: `/tmp/claude-1000/ext-research-claude/{cap,fx,run.sh,an.py}`.
**Caveat:** the live config dir is `CLAUDE_CONFIG_DIR=~/.claude-accounts/relotto`, not `~/.claude`. Its settings.json contains `env.ANTHROPIC_BASE_URL`, which overrides the process environment. It sets the same `enabledPlugins` as `~/.claude`.

## 1. `--mcp-config` + `--strict-mcp-config`
| Source | Default run | Strict run |
|---|---|---|
| user scope (`.claude.json` mcpServers) | loaded | **excluded** |
| local scope (`projects[cwd].mcpServers`) | loaded | **excluded** |
| project `.mcp.json` (auto-loaded in `-p`, no prompt) | loaded | **excluded** |
| plugin MCP (`mcp__plugin_testplug_plugmcp__*`) | loaded | **excluded** |
| claude.ai connectors (`claude.ai Claude Docs`) | loaded | **excluded** from tools (the list is still fetched; see debug log) |
| `--mcp-config` servers (source `dynamic`) | added | only these |
- [EMPIRICAL: `run.sh C_strict '{}' hi --plugin-dir fx/plug --strict-mcp-config --mcp-config fx/cli.json` gave init `mcp_servers:[climcp:dynamic]` and 29 tools on the wire (default run: 192).]
- [EMPIRICAL: isolated `CLAUDE_CONFIG_DIR=/tmp/.../cfg` holding user `usermcp` and local `localmcp`. Default run: `usermcp:user, projmcp:project, localmcp:local`. Strict run: `climcp:dynamic` only.]
- Plugins themselves still load under strict mode, so their skills and hooks stay. [EMPIRICAL]
- [DOC https://code.claude.com/docs/en/cli-reference] "Only use MCP servers from --mcp-config, ignoring all other MCP configurations". [CHANGELOG 2.1.246: no `.mcp.json` approval prompt in strict mode]
- Gotcha: `--mcp-config` is variadic; put the prompt before it. [EMPIRICAL]

## 2. `--settings '{"enabledPlugins":{...:false}}'`
- Yes. It deep-merges per key: `superpowers@claude-plugins-official:false` removed only superpowers. Its 15 skills, its SessionStart-hook context and its entry in init `plugins` all disappeared, and the other plugins stayed. [EMPIRICAL: E_settings; REAL2 model self-report lists no `superpowers:*`] [DOC https://code.claude.com/docs/en/settings: `--settings` sits above user/project/local and merges key by key]
- A disabled plugin's MCP servers go away with it. The docs say plugin servers start only when the plugin is enabled [DOC https://code.claude.com/docs/en/mcp#plugin-provided-mcp-servers]. Not flipped on an installed MCP plugin: **UNVERIFIED**.

## 3. `skillOverrides` in `--settings`
- Documented [DOC https://code.claude.com/docs/en/skills#override-skill-visibility-from-settings]. Working since [CHANGELOG 2.1.129].
- [EMPIRICAL E_settings / F_deny]
  - `off`: removed from the listing and from init `skills`/`slash_commands`. If the model calls it anyway, it gets `Skill caveman is disabled for model invocation in skillOverrides settings`.
  - `user-invocable-only`: removed from the listing, still in `slash_commands`.
  - `name-only`: the name is listed with no description.
- Works for `~/.claude-accounts/relotto/skills` (caveman, devhost) and for project `.claude/skills` (projskill). **Does not work for plugin skills**: `frontend-design:frontend-design`, `testplug:plugskill` and `unreal-…:unreal-mcp` stayed listed. [DOC: "Plugin skills are not affected by skillOverrides"]
- For plugin skills, disable the whole plugin (item 2). A `permissions.deny` rule of `Skill(plugin:skill)` blocks the call ("Skill execution blocked by permission rules"), but the skill **stays in the listing**. [EMPIRICAL G_denyinvoke] `--disable-slash-commands` disables all skills. [DOC cli-reference]

## 4. Token cost: removed vs. blocked
- The wire request contains only what the session loaded. With strict mode or a skill set to `off`, the definitions are not sent. [EMPIRICAL: tools_bytes 231,100 → 97,831]
- `permissions.deny: ["mcp__projmcp"]` also drops that server's tools from `tools[]`, but the server process still connects. [EMPIRICAL F_deny: `projmcp:connected`, 0 tools]
- `Skill()` deny does not save tokens: the skill stays listed.
- In this setup tool search is **off** because `ANTHROPIC_BASE_URL` is a custom proxy, so every MCP schema is sent inline. [DOC https://code.claude.com/docs/en/mcp#configure-tool-search]
- [EMPIRICAL real `claude -p /context --model haiku` (cost 0)] MCP tools total 38.9k tokens: whatsapp 12.3k, chrome-bg 7.3k, screenwright 7.1k, x-suite 4.0k, telegram 2.5k, Claude Docs 1.5k.
- Real "hi" prompt: 77.4k input tokens with the default config, 36.5k with strict + superpowers off. [EMPIRICAL A_default vs REAL2]
- The skill listing is capped at 1% of the context window. Debug log: "78 skills, 31486 chars > 8000 budget". [DOC skills.md `skillListingBudgetFraction`]
- Ways to measure:
  - `/context` works in `-p`, costs no generation, and breaks usage down by MCP tool/server and by skill. [EMPIRICAL]
  - `/usage` attribution by skill, plugin and MCP server. [DOC https://code.claude.com/docs/en/costs]
  - `/skill-doctor`. [DOC skills]
  - `OTEL_LOG_TOOL_DETAILS=1` metrics. [CHANGELOG]

## 5. Resume with a different set
- [EMPIRICAL R1→R2: started with `--strict-mcp-config --mcp-config cli.json`, resumed with `--resume <id> --strict-mcp-config --mcp-config cli2.json --settings '{superpowers:false, caveman:off}'`] The resumed request's `tools[]` held only `cli2mcp`. The old `mcp__climcp__*` tool_use/result stayed in history, and the session id was unchanged.
- **Asymmetry:**
  - Newly enabled skills are appended as a `skill_listing` delta (`isInitial:false`). [EMPIRICAL R4→R5]
  - Removed skills are **not retracted**. The original first-turn listing (and the superpowers hook text) stays in history, so the model still "sees" them. Calling one fails, and they keep costing tokens.
  - A plain `--resume` without flags returns to the global defaults.
- [DOC https://code.claude.com/docs/en/sessions: "--mcp-config, --settings, --plugin-dir … pass them again when you resume"]

## 6. Introspection of a running or resumed session
- **argv:** `/proc/<pid>/cmdline` shows the flags, but the transcript never records them. ComandOS must keep its own launch manifest.
- **stream-json `system/init`:** `mcp_servers[{name,status,source}]`, `tools`, `skills`, `slash_commands`, `plugins[{name,path}]`, `plugin_errors`, `mcp_server_errors`. This is the best signal, but only at launch. [DOC https://code.claude.com/docs/en/headless] [EMPIRICAL]
- **Transcript `.jsonl`:**
  - `attachment.type=="skill_listing"` carries `{names[], skillCount, isInitial}`. The union of all such entries is what the model was told.
  - `mcp_instructions_delta` is written on resume.
  - `prompt_snapshot.tools` looked like a cumulative union (on resume it listed climcp and cli2mcp, but the wire had only cli2mcp), so it is **not reliable**. [EMPIRICAL]
  - The transcript has no MCP-server list.
- **Debug log** (`--debug-file f`): lines like `MCP server "X": Successfully connected`, "Loaded N skills from plugin …", "Found 8 plugins (7 enabled, 1 disabled)", "Skill listing over budget". [EMPIRICAL]
- **`<config>/sessions/<pid>.json`:** pid → sessionId, cwd, tmux pane, status; no MCP data. [EMPIRICAL]

## 7. Usage fields in `projects/**/<session>.jsonl`
- **MCP call:** `type:"assistant"`, `message.content[].type=="tool_use"`, `name:"mcp__<server>__<tool>"`. Plugin servers are `mcp__plugin_<plugin>_<server>__…` and connectors are `mcp__claude_ai_<Name>__…`. The result is a `type:"user"` entry with `tool_result` + `toolUseResult`.
- **Skill call:** `tool_use name:"Skill", input:{"skill":"projskill"}`. The result entry has `toolUseResult:{success:true, commandName}`, followed by an `isMeta:true` user entry (`sourceToolUseID`) holding the skill body. A user-typed skill appears as `<command-name>/x</command-name>` in a user message.
- **Attribution:** the assistant entry that consumes a result carries `attributionMcpServer`, `attributionMcpTool`, `attributionSkill` and `attributionPlugin`, plus `message.usage`. `attributionSkill` stays set for the rest of the skill's turn, so count invocations from the `Skill` tool_use, not from attribution. [EMPIRICAL: 40 real transcripts: 61 Skill calls vs 2048 entries with `attributionSkill`]

## Recommended launch recipe
```bash
# new session; the prompt must come BEFORE the variadic --mcp-config
claude [-p "<prompt>"] --session-id <uuid> \
  --strict-mcp-config --mcp-config /run/comandos/<uuid>/mcp.json \
  --settings '{"enabledPlugins":{"superpowers@claude-plugins-official":false,"unreal-engine-skills-for-claude-code@claude-plugins-official":false},
               "skillOverrides":{"caveman":"off","devhost":"name-only"},
               "disableClaudeAiConnectors":true}'
# resume the same conversation with a new set: pass ALL flags again
claude --resume <uuid> --strict-mcp-config --mcp-config /run/comandos/<uuid>/mcp2.json --settings '<new json>'
```
- To disable a single plugin skill, deny `Skill(<plugin>:<skill>)`. It stays listed.
- A server can be blocked by name with `permissions.deny:["mcp__<server>"]`.
- Store the manifest (argv, mcp.json, settings) per session id, because Claude Code does not persist it.

## Open risks
- Removed skills stay in the resumed history (stale listing, wasted tokens, failed calls). Use `--fork-session` or a fresh session if that matters.
- Plugin skills cannot be toggled individually. `enabledPlugins` is all-or-nothing.
- Tool search is off behind the local proxy, so MCP schemas cost their full size. `ENABLE_TOOL_SEARCH=true` could change this (UNVERIFIED with this proxy).
- UNVERIFIED: that disabling an installed MCP-bearing plugin via `--settings` stops its server, and behavior in interactive (non-`-p`) sessions (tested only `-p`).
- Global settings `env` overrides the process env (for example `ANTHROPIC_BASE_URL`). Any override must go through `--settings`.

## Config integrity
`~/.claude/settings.json` and `~/.claude.json` sha256 are **unchanged**. `~/.claude-accounts/relotto/settings.json` is unchanged. `relotto/.claude.json` changed (889a78…→0b89fb…); it has no entry for the test dir, so the change most likely comes from the other live sessions. The test transcripts are in `relotto/projects/-tmp-claude-1000-ext-research-claude-work/` (12 files, not deleted). Real model calls: 2 (plus one `/context` with no generation).
