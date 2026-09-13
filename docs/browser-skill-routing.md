# Browser skill routing audit

Audited on 2026-09-11 after moving `chrome-bg` to the Mac mini. The search covered
166 distinct installed `SKILL.md` files across Codex, shared agents, Claude,
Grok, the Relotto Claude account, and the installed OpenAI plugin caches. Browser
references were classified and matching personal skill helpers were inspected.
This is an inventory of known routes, not a restriction on arbitrary programs.

| Route | Actual execution and status |
|---|---|
| `chrome-bg`, migrated Chrome DevTools aliases and supported cached npx commands | SSH to the Mac broker. No local fallback in the configured launcher. |
| `background-browser-control` | Updated to remote MCP, forwarded app ports and explicit artifact transfer. Legacy start/check scripts only read Mac service health. Local tab-marking helper retired. |
| `0xai-design-research` | Shared by Codex, Claude, Grok and agents through symlinks. Updated to the Mac runtime, temporary profiles, remote file paths and forwarded HTML previews. Its source verifier defaults to `cc-browser-remote`. |
| `busqueda-web` browser fallback | References `chrome-bg`, so the configured browser route is remote. Search APIs run independently. |
| Mobbin and other API-only skills | No Chrome required; they do not move to the Mac merely because the browser did. |
| `diagnose` / `diagnosing-bugs`, project Playwright/Puppeteer test scripts | Browser suites are separate runtimes. General instructions now require remote execution, but this migration does not rewrite or remotely provision arbitrary test suites. |
| `scribehow-playwright` | Still uses its separate headed Chromium, persistent profile and Scribe extension. Not provisioned on the Mac broker. Existing scripts are not automatically intercepted. |
| `mrp-docs`, Chrome extension connector and in-app browser plugin | Depend on their own browser session/extension. Not redirected by the MCP migration. |
| Screenwright | Separate MCP/runtime; not covered by `chrome-bg` migration. |

## Changes applied

- Updated the two personal browser skills and the design skill's supporting
  reference and launcher. The symlinked design skill has one source file.
- Replaced the old local-CDP start/check workflow with a read-only SSH health
  check. The retired marking helper exits with an actionable error.
- Disabled `codex-background-browser.service`, previously enabled at login.
  It was inactive with MainPID 0; no personal browser was stopped. The unit file
  remains installed, so a manual start is still technically possible.
- Added the user's remote-browser preference to `~/.codex/AGENTS.md`,
  `~/.claude/CLAUDE.md`, `~/.claude-accounts/relotto/CLAUDE.md` and
  `~/.grok/AGENTS.md`. Existing instruction text is preserved. These guide future
  instruction loads; already-running agents may retain their previous context.
- Left provider-managed plugin caches and unrelated skill runtimes unchanged.

## Verification and limits

Both updated skills pass the skill validator. Shell and JavaScript helpers pass
syntax checks. Behavioral checks confirm both health-check entrypoints invoke
SSH to `macmini`, propagate exit 255 on remote failure, and the retired marker
exits without contacting local CDP. The real legacy-start health check returned
Mac broker status with zero workers. Eight real SSH MCP clients each discovered
29 tools without needing a browser worker. A live run of the design skill's
source verifier opened Httpster on the Mac and read its title and 100 links
successfully; its MCP connection then closed.

The configuration and known launchers enforce the remote route for `chrome-bg`.
Instruction files do not enforce an OS-wide browser ban. Direct Chromium launches,
independent MCPs, explicit personal-browser connectors, future skill/plugin
updates and already-loaded instructions are outside that guarantee. Do not report
that every skill runs on the Mac until the separate workflows are migrated and
verified there.

Backups of modified instructions, helpers and the original local service unit:
`/home/someguy/secure-backups/comandos-browser-skills-20260911-195712`. The manifest records original paths and modes. Restoring those files
would reinstate the old behavior; re-enabling the local service is a separate
operation.
