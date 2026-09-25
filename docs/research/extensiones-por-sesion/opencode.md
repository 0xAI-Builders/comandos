# OpenCode 1.17.18: per-session MCP and skill control

Source links point to tag v1.17.18: `S/` = `https://github.com/anomalyco/opencode/blob/v1.17.18/packages/opencode/src/`.
Test setup: cwd `/tmp/claude-1000/ext-research-opencode`, `--pure` (skips the ComandOS `comandos.js` plugin), two dummy stdio MCP servers (`alpha`, `beta`), and a local OpenAI-compatible capture server on 127.0.0.1:7393. The capture server was declared as provider `capture/m` in `OPENCODE_CONFIG_CONTENT`. It records every LLM request body, so the test made no real model calls and cost nothing.

## 1. Per-session config injection
- Load order, later wins: remote well-known, then global (`config.json`, `opencode.json`, `opencode.jsonc`), then `OPENCODE_CONFIG`, then project `opencode.json(c)`, then `.opencode` dirs and `OPENCODE_CONFIG_DIR`, then `OPENCODE_CONFIG_CONTENT`, then account/org config, then managed config. Nothing replaces the global files; every layer is merged on top. [SOURCE S/config/config.ts L380-522] [DOC https://opencode.ai/docs/config/]
- Merge is a deep merge. Objects merge key by key. Arrays are replaced, except `instructions` (concatenated and deduplicated) and `plugin` (concatenated, with `plugin_origins`). [SOURCE S/config/config.ts L41-51, L336-354]
  - [EMPIRICAL] `OPENCODE_CONFIG_CONTENT='{"mcp":{"teams":{"enabled":false}}}' opencode debug config` output: `teams` enabled=false, the other 12 servers unchanged and `teams.command` kept.
  - [EMPIRICAL] Injecting `obscura.command:["/bin/true"]` produced `"command":["/bin/true"]` (the array was replaced).
  - [EMPIRICAL] A `plugin` array was appended after `comandos.js`. `instructions` from `OPENCODE_CONFIG` plus `OPENCODE_CONFIG_CONTENT` gave `["b.md","a.md"]`.
  - [EMPIRICAL] `OPENCODE_CONFIG` with radek=false plus `OPENCODE_CONFIG_CONTENT` with radek=true gave `{"r":true}`, so CONTENT beats CONFIG.
- `OPENCODE_PERMISSION` (a JSON env var) is deep-merged into `permission` last. [SOURCE S/config/config.ts L545] [EMPIRICAL `debug agent build` → `{"permission":"beta_*","action":"deny","pattern":"*"}`]
- Side effects:
  - OpenCode rewrites any loaded config file that lacks `$schema`, so a file passed via `OPENCODE_CONFIG` gets modified. [SOURCE S/config/config.ts L231-235] [EMPIRICAL: extra.json gained `"$schema"`]
  - `OPENCODE_CONFIG_DIR` writes a `.gitignore` into that dir and runs an npm install of `@opencode-ai/plugin` there. Plugin init waits for that install. In my test, `debug config` hung until killed at 400 s, twice (rc=124). [SOURCE L443-457, S/plugin/index.ts L180] [EMPIRICAL]
  - `OPENCODE_CONFIG_CONTENT` writes nothing to disk. It is the safe channel.

## 2. Skills per session
- Discovery order:
  1. Built-in `customize-opencode`.
  2. `~/.claude/skills/**` and `~/.agents/skills/**`, plus project `.claude` and `.agents` dirs walking up to the worktree.
  3. Config dirs: `~/.config/opencode`, `.opencode` dirs, `~/.opencode`, and `OPENCODE_CONFIG_DIR`, using `{skill,skills}/**/SKILL.md`.
  4. `skills.paths` and `skills.urls` from config.
  [SOURCE S/skill/index.ts L175-235] [DOC https://opencode.ai/docs/skills/]
- Env flags: `OPENCODE_DISABLE_CLAUDE_CODE_SKILLS=1` drops `~/.claude`. `OPENCODE_DISABLE_EXTERNAL_SKILLS=1` drops both `.claude` and `.agents`.
  - [EMPIRICAL `opencode debug skill`] 82 skills by default, 57 with the first flag, 8 with the second (built-in plus `~/.config/opencode/skills` and `~/.opencode/skills`).
  - [EMPIRICAL] Adding `{"skills":{"paths":[".../myskills"]}}` in `OPENCODE_CONFIG_CONTENT` added `zz-test`.
- Hiding skills: `"permission":{"skill":{"devhost":"deny"}}`. `Skill.available(agent)` filters out denied skills before the `<available_skills>` system block is built, so a denied skill costs no tokens. It is also rejected if the model calls it anyway. [SOURCE S/skill/index.ts L286-291, S/session/system.ts L98-109, S/tool/skill.ts L27-32]
  - [EMPIRICAL, captured request] Baseline: 81 `<skill>` entries, devhost absent, system text 55,521 bytes.
  - With `{"*":"deny","fireflies":"allow"}`: only fireflies listed, 12,991 bytes.
  - With `"skill":"deny"`: the `skill` tool itself is removed from `tools`, 12,488 bytes.
  - Calling denied devhost returned the tool error "The user has specified a rule which prevents you from using this specific tool call...".
- `opencode debug skill` and `GET /skill` list all discovered skills. They do not apply permissions. [EMPIRICAL: /skill returned 82 under the deny-all set]

## 3. MCP tool definitions
- A server with `enabled:false` is never spawned or connected. [SOURCE S/mcp/index.ts L374, L514] [EMPIRICAL: `opencode mcp list` shows "disabled"; the beta log recorded no START during runs with beta disabled]
- Tool names are `sanitize(server)+"_"+sanitize(tool)`, where sanitize maps non-`[A-Za-z0-9_-]` characters to `_`. Example: `alpha_echo`. [SOURCE S/mcp/catalog.ts L117-119]
- `"tools":{"alpha_*":false}` becomes the permission `alpha_*: deny`. Tools whose rule is `pattern "*"` plus deny are removed from the request, and so are that server's `<mcp_instructions>`. [SOURCE S/config/config.ts L553-564, S/permission/index.ts L204-219, S/session/llm/request.ts L208-213]
  - [EMPIRICAL] The tools list dropped `alpha_echo` and `alpha_ping`, and the alpha instructions went from 1 to 0.
  - [EMPIRICAL] The server was **still spawned**: the alpha START count increased.
- Per-agent `agent.<name>.tools` and `agent.<name>.permission` work the same way. [DOC https://opencode.ai/docs/mcp-servers/] (Not tested per agent.)

## 4. Resume with a different set
[EMPIRICAL] I created session `ses_f2a212d84fferYYYMNZG0kZ2E1` with set A (alpha and beta on, devhost denied) and resumed it three times with a different set each time:

| Resume command | Set injected | Captured request |
|---|---|---|
| `run -s <id>` | B: beta disabled, skills deny-all except fireflies | beta tools and instructions gone, only fireflies listed, history kept (roles `system,user,assistant,tool,assistant,user`) |
| `run -s <id>` | C: set B plus `tools alpha_*:false` | alpha tools gone |
| `run -c` | D: set A with `skill:"deny"` | continued the same session, no `skill` tool |

- **The config is resolved per process, not stored in the session.** The `session.permission` column only holds run-mode rules (question and plan deny). [EMPIRICAL `opencode db`]
- `--fork` is available with `-s` or `-c`. (Not tested.)

## 5. Introspecting a running process
- `/proc/<pid>/environ` shows the injected `OPENCODE_CONFIG_CONTENT`. [EMPIRICAL, own `opencode serve`]
- If the process is started with `--port`, its HTTP API answers:
  - `GET /mcp` gives per-server status (`connected`, `disabled`, `failed`).
  - `GET /config` gives the resolved config, **including bearer headers**.
  - `GET /skill` gives the unfiltered skill list.
  - `GET /experimental/tool/ids` lists built-in tool IDs.
  [EMPIRICAL]
- `opencode debug config` and `debug agent <name>` recompute from the env you pass; they do not attach to the running process. `debug agent` does not list MCP tools. [EMPIRICAL]
- Log: a single shared file, `~/.local/share/opencode/log/opencode.log`. Lines are tagged `run=<id>`, not pid. It records "duplicate skill name" warnings and `evaluated permission=skill pattern=<name>` lines. It carries little MCP detail at INFO level. [EMPIRICAL]

## 6. Usage logging
- The database is `~/.local/share/opencode/opencode.db` (SQLite, tables `session`, `message`, `part`), queried with `opencode db "<sql>" --format json`.
- Each tool call is a `part` with `type='tool'`, `tool`, `callID`, and `state.{status,input,output,error,time,metadata}`. [EMPIRICAL]
  - MCP calls: `tool="alpha_echo"`.
  - Skill calls: `tool="skill"`, `input.name`, `metadata.dir` (the resolved skill directory), title "Loaded skill: X".
  - Denied calls: `status="error"`.
- `opencode export <id>` gives `{info, messages[].parts[]}` with the same parts. [EMPIRICAL]

## Recommended launch recipe
```
env OPENCODE_CONFIG_CONTENT='{"mcp":{"<unwanted>":{"enabled":false},...},
      "permission":{"skill":{"*":"deny","<keep1>":"allow","<keep2>":"allow"}}}' \
    [OPENCODE_DISABLE_EXTERNAL_SKILLS=1] \
    opencode [--port <p>] [-s <session_id> | -c] [--fork] <dir>
```
- Generate the disable map from the keys of `opencode debug config` (write it to a file first). Every server that is not wanted gets `enabled:false`, which avoids spawning it and sending its schemas.
- To add a server for one session, define it in the same JSON.
- To resume with a new set, launch again with the same `-s <id>` and new JSON.
- ComandOS reads `/proc/<pid>/environ` (or `GET /mcp` if launched with `--port`) and the db `part` table.
- Avoid `OPENCODE_CONFIG_DIR` and `OPENCODE_CONFIG` files.

## Open risks
- Duplicate skill names across `~/.agents`, `~/.claude`, `~/.config/opencode` and `~/.opencode` are loaded concurrently and the last one wins. The `fireflies` skill resolved to `~/.claude/skills/fireflies`. Which copy loads is not deterministic.
- The `tools`/permission route hides tools but still spawns the server (cost, side effects). Only `enabled:false` truly avoids it.
- Tool-name to server mapping is ambiguous for names with `_` (for example `meta_social_technologies_*`). Match on the longest known server-name prefix.
- `debug config` and `GET /config` expose the plaintext bearer tokens that sit in the user's config. Redact them.
- Piping large `opencode debug skill` output straight to `jq` truncated it (parse error). Redirect to a file first.
- `-c` picks the most recent root session from the server's session list. Its scope for non-git dirs (project "global") is UNVERIFIED; use `-s`.
- A leftover test session, `ses_f2a212d84fferYYYMNZG0kZ2E1`, remains in the user's `opencode.db` (not deleted).

Checksums after testing: `~/.config/opencode/opencode.json` and `~/.config/opencode/opencode.jsonc` are IDENTICAL to config-checksums-before.txt.
