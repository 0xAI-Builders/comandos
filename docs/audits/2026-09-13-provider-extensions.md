# Provider extensions audit

The inventory reads the selected provider, account and project. It returns
configuration evidence without claiming that a running agent loaded an extension.
The fixture suite reproduced twelve failures before the fixes. The focused suite
now passes 51 tests.

## Provider matrix

| Harness | MCP declarations read | Skill declarations read | Launch selection |
| --- | --- | --- | --- |
| Claude | Selected account `.claude.json`, current project `.mcp.json`, local MCP definitions under the exact project key, installed plugin index and plugin manifests | Selected account and ancestor `.claude/skills`, indexed plugin skills | Conditional JSON MCP selection. Local MCP definitions, active plugins and unreadable sources block isolated selection. Individual skills unsupported. |
| Codex | Selected account `config.toml`, ancestor `.codex/config.toml`, installed plugin cache and `plugins.<id>.enabled` | Account skills, shared `.agents/skills`, ancestor `.agents/skills` and legacy `.codex/skills`, `/etc/codex/skills`, plugin skills | Standalone MCP leaf overrides and standalone skill selectors. Plugin rows remain visible with selection unsupported. |
| Grok | Account TOML, repository/current-directory TOML, compatible Claude/Cursor MCP files when enabled, local plugins and installed-plugin registry | Account/project skills, compatible Claude/Cursor skills, `skills.paths`, plugin skills | Inventory only; individual launch overrides unsupported. |
| OpenCode | XDG user JSON/JSONC, explicit custom file, ancestor project JSON/JSONC, `.opencode` config directories | User and ancestor OpenCode, Claude-compatible and `.agents` directories, explicit skill paths | Inventory only; individual launch overrides unsupported. |
| Gemini CLI | User/project `settings.json`, installed extension manifests and workspace enablement rules | User/workspace `.gemini` and `.agents` skills, installed extension skills | Inventory only; individual launch overrides unsupported. |
| Antigravity CLI | `~/.gemini/config/mcp_config.json`, workspace `.agents/mcp_config.json` | `~/.gemini/config/skills`, workspace `.agents/skills`, legacy `.agent/skills` | Inventory only; individual launch overrides unsupported. |
| ACP | No independent extension root | No independent extension root | Returns `unknown`: the caller must resolve the underlying agent/account. |
| Shell | None | None | Returns `unsupported`, with empty inventories. |

The account registry remains authoritative for named Claude, Codex and Grok
accounts. Unknown harnesses, malformed aliases, missing named-account directories
and named accounts for providers without account support raise validation errors.
A custom Claude account does not fall back to its parent directory's `.claude.json`.

## Findings and fixes

`lib/capabilities.py` replaces case-insensitive OR merging with provider-aware
precedence. Distinct MCP IDs such as `docs` and `Docs` remain distinct. Project
Codex/OpenCode leaf changes inherit unspecified fields, while Claude and Grok
server definitions replace lower-priority definitions. MCP rows retain the
winning source and declarations from lower-priority configuration files.

TOML parsing supports inline tables, dotted keys, quoted names and multiline
strings. Invalid files produce `incomplete` detection and disable profile
selection that might discard unreadable settings. JSONC stripping preserves
quoted URLs and escaped strings. No MCP command or plugin code executes during
inventory reads.

Disabled declarations remain visible. Plugin MCP IDs include plugin identity so
servers with equal names in different packages do not collapse. Cache entries
without enablement evidence have `enabled: null` and `status: installed`.
Explicit plugin disablement propagates to its MCPs and skills. Gemini extension
rules use the current workspace and last matching rule. Skills found through
multiple symlinks retain one canonical identity and all discovery sources.

`lib/session_profiles.py` separates configured enablement from automatic
invocation. Claude's `disable-model-invocation` and Codex's
`agents/openai.yaml` invocation policy do not mean a skill is uninstalled or
disabled. Codex bundled-skill disablement is reflected separately. Equal skill
names at distinct Codex paths remain distinct; a path override can change one
without removing a name rule affecting the others.

Codex profile generation preserves the user skill selector array, replaces
matching canonical file selectors and appends the requested selectors. It does
not copy ignored project skill arrays into session flags. Unsupported plugin
selection cannot be bypassed by posting a profile directly to the API. Claude
strict JSON selection preserves unrelated JSON servers and writes its temporary
launch file with mode `0600`.

## Verified provider contracts

The installed CLI help was read in temporary homes for Codex, Claude, Grok and
OpenCode. It confirmed configuration overrides and the available management
commands. No authenticated provider conversation or MCP tool was started.

Three isolated Codex app-server `skills/list` probes checked real installed
behavior. A `SKILL.md` selector with `enabled=false` disabled the fixture skill.
The directory containing that file did not. A name selector disabled it. The
[Codex skill configuration implementation](https://github.com/openai/codex/blob/main/codex-rs/config/src/skills_config.rs)
also reads selector rules only from user and session-flag layers. Generic
configuration documentation describes a skill-folder path, but the specific
[skill guide](https://developers.openai.com/codex/skills) uses `SKILL.md`; the
implementation and installed binary determine this audit's behavior.

The following primary references supplied format and precedence evidence:

- [Codex configuration reference](https://developers.openai.com/codex/config-reference) and [plugin cache selection](https://github.com/openai/codex/blob/main/codex-rs/core-plugins/src/store.rs).
- [Claude MCP scopes](https://code.claude.com/docs/en/mcp) and [plugin manifests and scopes](https://code.claude.com/docs/en/plugins-reference).
- [Grok configuration](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/05-configuration.md), [plugin discovery](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-agent/src/plugins/discovery.rs) and [installed-plugin index](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-agent/src/plugins/install_registry.rs).
- [OpenCode configuration](https://opencode.ai/docs/config/) and [skills](https://opencode.ai/docs/skills/).
- [Gemini skills](https://geminicli.com/docs/cli/skills/), [settings](https://geminicli.com/docs/reference/configuration/) and [extension enablement](https://github.com/google-gemini/gemini-cli/blob/main/packages/cli/src/config/extensions/extensionEnablement.ts).
- [Antigravity MCP configuration](https://antigravity.google/docs/cli/mcp) and [skills](https://antigravity.google/docs/skills).

## Verification and limits

Run the focused tests from the repository root:

```sh
python3.11 -m pytest -q tests/test_provider_extensions.py tests/test_session_profiles.py tests/test_provider_accounts.py tests/test_mcp_descriptions.py
```

The 51 tests cover account/project isolation, MCP precedence and provenance,
inline and malformed TOML, JSONC, disabled and installed plugin state, Gemini
workspace rules, Grok installed plugins, symlink cycles, invocation policy,
unsupported selection, canonical skill selectors, duplicate skill names and
private launch-file permissions. Existing profile API and new-session launch
tests also pass.

System Python is 3.10 and has no TOML package. The reader uses `tomllib` or `tomli`
when available; otherwise it invokes installed Python 3.11 with a three-second
timeout. A 64-entry digest cache avoids repeated parsing subprocesses and retains
only extension metadata. Tests verify one parse for repeated identical reads,
absence of transport credentials from the cache and explicit incomplete state
when no parser is available.

A read-only smoke check under system Python 3.10 read all registered providers
against local configuration. The first inventory reads took 1-126 ms per provider.
Only aggregate counts and elapsed times were printed. No provider configuration,
active session or service was modified.

Every returned MCP/skill has `effectiveNow: null` and `runtimeEnabled: null`.
Configured enablement does not establish runtime connectivity, authorization,
trust, managed policy or tool availability. CLI-only flags, arbitrary process
environment overrides, remote/workspace-managed plugin activation, Claude cloud
connectors, OpenCode JavaScript plugins and remote configuration, Gemini builtin
skills, Grok Claude-plugin compatibility and Antigravity IDE plugins are not fully
resolved. The API exposes limitations instead of treating those reads as complete
runtime evidence. Malformed files and missing indexed plugin files are explicit
errors. Bounded skill scans report truncation.

Provider behavior outside the three Codex skill probes was verified through
local help, primary source code/documentation and isolated fixtures. Live launch
selection has not been exercised against authenticated provider accounts. The
root audit owns dashboard integration, browser verification and deployment.

## Native model and effort observations

`lib/tui_state.py` keeps observations scoped by process and conversation identity.
Claude transcript model changes and native command output take precedence over
custom status scripts. A custom status script is returned as a candidate with
`source: status-script` and `confirmed: false`. An unchanged status footer cannot
overwrite a newer conversation update after a picker briefly hides it. A repeated
native confirmation can still count as new evidence after it disappears.

Transcript reads preserve a previously observed effort when a subsequent turn of
the same model omits that field. Changing models clears the old effort. Malformed
message, model and effort values are ignored, and multiline local-command output
is recognized. Assistant/tool output after the assistant response marker does not
count as a native model confirmation.

The non-ACP section of `observe_pane` keeps OpenCode, Antigravity and Gemini motors
bound to their harness. A GPT model name displayed by OpenCode does not imply a
Codex route. Gemini/Antigravity native runtime state without a verified adapter
remains unconfirmed; startup arguments alone do not prove a native `/model` or
`/effort` operation succeeded.

Run native-observation regressions with:

```sh
python3.11 -m pytest -q tests/test_tui_state.py
```

The 27 tests exercise Codex native footer versus open picker, Claude native command
and transcript updates, stale footer handling, process-only candidates, Grok
PID/session summary changes and stat-only idle reads, OpenCode's composer footer,
malformed metadata and unsupported provider screens. Native `/model` and `/effort`
commands were not sent to active user sessions. Grok and transcript changes in
these tests are fixtures, so this report does not claim end-to-end verification
of every installed TUI version.
