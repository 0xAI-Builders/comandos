# Native TUI commands and card observations

Cards read applied runtime state after native commands. Typing a command, opening a picker, highlighting an option, or sending Enter is not confirmation. The same observer feeds the desktop and remote dashboard through `GET /state`.

## Observation contract

Implementation: [`tui_state.py`](../lib/tui_state.py), `observe_pane` and `reconcile_card_config` in [`cc-dash`](../bin/cc-dash). Regression fixtures: [`test_tui_state.py`](../tests/test_tui_state.py).

| Harness | Verified executable | Evidence used for cards | Limits |
|---|---|---|---|
| Codex | `codex-cli 0.154.0` | Root rollout `turn_context.model`, `effort`/`reasoning_effort`; dedicated current footer | Disabled/custom footer can defer a native change until the next recorded turn. Delegated rollouts never supply the root card. |
| Claude Code | `2.1.268` | Root assistant model and effort; persisted `local-command-stdout`; native model/effort confirmations; supported custom footer | Unknown custom model labels remain unknown. A transcript older than the bounded search window can require another turn. |
| Grok Build | `1.0.25 (f7e67d6988e2)` | PID in `active_sessions.json`, exact session `summary.json`: `current_model_id`, `reasoning_effort` | Update latency depends on the CLI publishing its summary. |
| OpenCode | `1.17.18` | Native composer footer with its border, agent, model and optional variant | Known model labels only; arbitrary providers/renamed models and hidden/custom status need an adapter. No exact native conversation bridge yet. |
| ACP | Repository `cc-acp` | Pane-owned `acp-panes.json` published after configuration succeeds | Available models/modes depend on the selected agent's advertised ACP capabilities. |
| Antigravity CLI (agy) | `1.1.25` | Process launch flags, or ACP state when hosted through `cc-acp` | A disposable network-isolated TUI stops at the authentication picker before `/model` can run. Binary confirmation strings exist, but their rendered boundary is unverified; launch arguments stay unconfirmed. This is a native Go TUI. |

Observations are scoped to tmux server/pane identity, process PID/start time, harness and conversation ID. A changed conversation or process gets a fresh observation. New transcript evidence supersedes an unchanged old footer; a new native confirmation supersedes older transcript state. A disappearing confirmation retains the accepted value and rearms detection when it appears again. Changing models clears effort until that model's effort is observed. `auto` is reported as `auto`, without inventing its effective numeric budget.

`observedConfig.source` is `conversation`, `pane`, `process`, or `unconfirmed`. `confirmed` is false for launch arguments alone. `observedAt` records the sample; `evidenceAt` records when changed evidence was accepted. `limitations` identifies unavailable runtime evidence and unobserved effort. Account aliases identify the process configuration directory; `accountSource` states that provenance. Public identity files are stat-checked so native login/logout does not remain cached for the process lifetime. The observer does not claim which credential an already running vendor client has cached internally.

The transcript cache reads at most 2 MiB on change and performs only `stat` on unchanged files. It retains at most 128 entries. Grok resolves each exact summary path once and invalidates parsed metadata on file changes. Screen capture is limited to 45 history lines plus the visible pane. No model requests, command injection, full-history polling, or service restarts are required.

## Command map

Every command below goes through the same state observer. The usage column identifies commands that can require a card change. Commands for display, tools, files or integrations cause no speculative model/account change. Skill, plugin, MCP and workflow commands are dynamic namespaces; their names cannot be exhaustively listed ahead of installation. Their effects are still observed through runtime evidence.

### Codex

The complete built-in enum is pinned to the [installed 0.154.0 source](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/slash_command.rs). Platform, task and side-conversation restrictions apply.

| Commands | Card usage |
|---|---|
| `/model`, `/plan` | Observe selected model/effort after application. |
| `/new`, `/clear`, `/resume`, `/fork`, `/worktree`, `/app`, `/archive`, `/delete`, `/exit`, `/quit` | Re-identify the live process and root conversation. |
| `/agents`, `/subagents`, `/side`, `/btw` | Preserve root ownership; delegated conversations are isolated. |
| `/logout` | Refresh public account identity when its source changes. |
| `/cd`, `/pwd` (alias `/cwd`), `/rename`, `/title`, `/statusline` | Directory/name/display can change; observe configuration independently. |
| `/ide`, `/permissions`, `/keymap`, `/vim`, `/setup-default-sandbox`, `/sandbox-add-read-dir`, `/experimental`, `/approve`, `/memories`, `/skills`, `/import`, `/hooks`, `/review`, `/init`, `/compact`, `/recap`, `/goal`, `/copy`, `/export`, `/raw`, `/diff`, `/mention`, `/status`, `/usage`, `/debug-config`, `/theme`, `/pets` (alias `/pet`), `/mcp`, `/apps`, `/plugins`, `/feedback`, `/ps`, `/stop` (alias `/clean`), `/personality` | Inspect runtime state; command text alone changes no card configuration. |
| `/rollout`, `/test-approval`, `/debug-m-drop`, `/debug-m-update` | Debug/internal commands; availability depends on build. |

### Claude Code

Inventory extracted from local `2.1.268` embedded `local`, `local-jsx`, and `prompt` command declarations. Some are hidden, platform-specific or gated; declaration does not establish availability for every account. The [vendor commands reference](https://code.claude.com/docs/en/commands) describes the public surface.

| Commands | Card usage |
|---|---|
| `/model`, `/effort`, `/fast`, `/config`, `/plan` | Observe applied model/effort, including capped effort and auto reset. |
| `/clear`, `/fork`, `/branch`, `/resume`, `/session`, `/desktop`, `/teleport`, `/exit` | Re-identify conversation/process; never reuse another pane's transcript. |
| `/login`, `/logout`, `/setup-bedrock`, `/setup-vertex` | Observe account identity source; backend credentials are not inferred from text. |
| `/agents`, `/list-agents`, `/subtask`, `/btw`, `/background`, `/tasks` | Root conversation remains separate from delegated work. |
| `/add-dir`, `/cd`, `/rename` | Directory/session metadata changes; configuration remains evidence-based. |
| `/advisor`, `/artifacts`, `/auto-mode-setup`, `/autocompact`, `/autofix-pr`, `/brief`, `/bug`, `/cloud-plugins`, `/color`, `/compact`, `/context`, `/copy`, `/daemon`, `/design-consent`, `/design-login`, `/design-revoke`, `/diff`, `/export`, `/extra-usage`, `/feedback`, `/focus`, `/goal`, `/heapdump`, `/help`, `/hooks`, `/ide`, `/import`, `/init`, `/insights`, `/install`, `/install-github-app`, `/install-slack-app`, `/loops`, `/mcp`, `/memory`, `/mobile`, `/passes`, `/pause-memory`, `/permissions`, `/plugin`, `/plugin-types`, `/powerup`, `/privacy-settings`, `/pro-trial-expired`, `/radio`, `/rate-limit-options`, `/recap`, `/reload-plugins`, `/reload-skills`, `/remote-control`, `/remote-env`, `/scroll-speed`, `/skill-doctor`, `/skills`, `/status`, `/stickers`, `/stop`, `/team-onboarding`, `/terminal-setup`, `/theme`, `/tui`, `/ultraplan`, `/ultrareview`, `/update`, `/upgrade`, `/usage`, `/usage-credits`, `/version`, `/voice`, `/web-setup`, `/wellbeing`, `/workflow-launch-exec`, `/workflows`, `/__remote-workflow` | No speculative card configuration mutation. Runtime effects, including model changes caused by tools/hooks, are observed normally. |
| Public bundled commands/aliases: `/adddir`, `/allowed-tools`, `/android`, `/app`, `/bashes`, `/batch`, `/bg`, `/checkpoint`, `/checkup`, `/chrome`, `/claude-api`, `/code-review`, `/continue`, `/cost`, `/dataviz`, `/debug`, `/deep-research`, `/design`, `/design-sync`, `/doctor`, `/fewer-permission-prompts`, `/ios`, `/keybindings`, `/loop`, `/new`, `/peers`, `/pr-comments`, `/proactive`, `/quit`, `/rc`, `/release-notes`, `/reset`, `/review`, `/rewind`, `/routines`, `/run`, `/run-skill-generator`, `/sandbox`, `/schedule`, `/security-review`, `/settings`, `/share`, `/simplify`, `/stats`, `/statusline`, `/tp`, `/undo`, `/verify`, `/vim`, `/workflow-authoring` | Public reference supplement, including bundled skills/workflows and aliases; runtime evidence decides card state. |
| MCP `mcp__…`, skill `/name`, plugin `/plugin:skill`; aliases including `/breaks`, `/break-reminder`, `/downtime` | Dynamic/gated names; evaluate state, not a whitelist of input strings. |

Native confirmation strings were checked in the installed executable. Claude persists local command output in its root JSONL, so confirmed changes can survive a dashboard restart even before another model response. Later assistant output makes older visible confirmation text historical.

### Grok Build

The [upstream registry](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/src/slash/commands/mod.rs) and [command guide](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/04-slash-commands.md) provide the inventory. The installed short commit is not publicly resolvable; upstream-only names below are reference coverage, not a claim that every name is enabled in 1.0.25.

| Commands | Card usage |
|---|---|
| `/model` (alias `/m`), `/effort`, `/plan`, `/auto`, `/always-approve` | Observe summary model/effort after native application. Permission mode is separate. |
| `/new`, `/resume`, `/fork`, `/rewind` (alias `/undo`), `/home`, `/delete`, `/quit`, `/exit` | Refresh PID-owned conversation state. |
| `/login`, `/logout`, `/usage`, `/privacy` | Refresh account identity source; usage remains a separate field. |
| `/config-agents`, `/personas`, `/btw`, `/tasks`, `/workflow`, `/workflows`, `/goal`, `/deep-research`, `/loop` | Preserve parent/child ownership. |
| `/tutorial`, `/settings`, `/dashboard`, `/plugins`, `/voice`, `/context`, `/compact`, `/view-plan`, `/remember`, `/recap`, `/jump`, `/expand`, `/edit-prompt`, `/queue`, `/session-info`, `/share`, `/rename`, `/history`, `/transcript`, `/export`, `/copy`, `/find`, `/skills`, `/mcps`, `/hooks`, `/marketplace`, `/theme`, `/vim-mode`, `/multiline`, `/compact-mode`, `/timestamps`, `/toggle-mouse-reporting`, `/minimal`, `/fullscreen`, `/timeline`, `/cd`, `/imagine`, `/imagine-video`, `/docs`, `/release-notes`, `/announcements`, `/feedback`, `/doctor`, `/import-claude`, `/help`, `/memory`, `/flush`, `/dream` | Inspect runtime state without inferring configuration from input. |
| `/gboom`, `/scroll-debug`, build-specific debug commands, installed skills/workflows | Hidden or dynamic commands. |

### OpenCode

Inventory combines `slashName` and session `slash` declarations in installed `1.17.18` with the [public TUI reference](https://opencode.ai/docs/tui/). The known composer footer was captured in a disposable, separate tmux server with empty configuration and no submitted model turn.

| Commands | Card usage |
|---|---|
| `/models` (alias `/mo`), `/variants`, `/agents`; `Ctrl+T` variant cycle and model/agent shortcuts | Read applied composer model/variant. Picker rows are not evidence. |
| `/new` (alias `/clear`), `/sessions` (aliases `/resume`, `/continue`), `/fork`, `/undo`, `/redo`, `/exit` (aliases `/quit`, `/q`) | Process/pane state is fresh; exact native conversation ID is not yet exposed. |
| `/connect`, `/org` (aliases `/orgs`, `/switch-org`) | Provider/organization operations; never infer a credential from a model label. |
| `/compact` (alias `/summarize`), `/copy`, `/debug`, `/details`, `/diff`, `/editor`, `/export`, `/help`, `/init`, `/mcps`, `/move`, `/rename`, `/share`, `/skills`, `/slash`, `/status`, `/themes`, `/thinking` (alias `/toggle-thinking`), `/timeline`, `/timestamps` (alias `/toggle-timestamps`), `/unshare`, `/warp`, `/workspaces` | Runtime observation continues. `/thinking` controls visibility, not reasoning effort. |

### ACP (`cc-acp`)

The complete local dispatcher is [`bin/cc-acp`](../bin/cc-acp). Agent-advertised commands are protocol metadata, not automatically supported local commands.

| Commands | Card usage |
|---|---|
| `/model <id>`, `/effort <level>`, `/agent <id>`, `/account <alias>` | Read newly published applied model, effort, motor and account. |
| `/mode <id>`, `/danger` | Mode/approval changes are separate from model configuration. |
| `/help`, `/status`, `/models` | Display only. |
| `/exit`, `/quit`, `/q` | Process/pane lifetime changes. |

### Antigravity CLI (`agy`)

The [vendor CLI reference](https://www.antigravity.google/docs/cli/reference/) describes the current public commands. Installed `1.1.25` binary strings additionally confirm `/effort`, `/codesearch` and `/plan`. The live vendor reference identifies itself as 1.2.0; later commands cannot be assumed present in 1.1.25.

| Commands | Card usage |
|---|---|
| `/model`, `/effort`, `/agents`, `/config` (alias `/settings`), `/fast`, `/planning`, `/plan` | Native live evidence needs an adapter; launch flags alone remain unconfirmed. |
| `/clear` (alias `/new`), `/fork` (alias `/branch`), `/resume` (aliases `/switch`, `/conversation`), `/rewind` (alias `/undo`), `/exit` (alias `/quit`) | Refresh process identity. |
| `/logout`, `/credits`, `/usage` (alias `/quota`) | Account/quota operations; do not infer authentication state. |
| `/add-dir`, `/artifact`, `/boost`, `/btw`, `/codesearch`, `/context`, `/copy`, `/diff`, `/feedback`, `/help`, `/hooks`, `/keybindings`, `/mcp`, `/open`, `/permissions`, `/rename`, `/skills`, `/statusline`, `/tasks`, `/teamwork-preview` (alias `/teamwork`), `/title`, `/voice` (alias `/record`) | Configuration is read through available evidence; custom status/title scripts are not parsed speculatively. |

## Verification and latency

Tests cover native confirmation versus menu highlight, prose exclusion, a newer transcript against a stale footer, repeated confirmations after disappearance, root/subagent isolation, cleared effort, login/logout, large trailing tool output, and stat-only idle reads. Unknown evidence is explicit rather than filled from global defaults.

A sequential read-only comparison on the same machine measured main warm collections of 0.4744, 0.6077 and 0.4492 seconds, and this observer at 0.5787, 0.4523 and 0.4206 seconds (medians 0.4744 and 0.4523). Cold collections were 0.6248 and 0.8366 seconds. At measurement there were 28 live panes and 26 AI processes; the observer resolved all 26 models. These measurements describe one loaded workstation, not a general performance guarantee.

The dashboard's visible poll defaults to two seconds and the state cache TTL is 1.2 seconds. End-to-end latency adds the CLI's publication delay and one state collection. These are polling bounds, not a measured maximum. The observer preserves accepted state between polls; a changed configuration with no observable vendor output remains unconfirmed until evidence appears.
