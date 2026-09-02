# Harness ACP: ComandOS como cliente de cualquier agente

ComandOS habla **Agent Client Protocol** (JSON-RPC 2.0 por stdio) con el agente
del vendor corriendo como subproceso. Cada agente usa **su propia suscripción**
(login OAuth del CLI): nunca API keys, nunca reenvío de tokens por gateway.
Así, en un solo pane puedes cambiar de motor, modelo, esfuerzo o cuenta sin
depender de la TUI del vendor.

| Motor (route `acp:<motor>`) | Proceso ACP | Cuenta | Modelo | Esfuerzo |
|---|---|---|---|---|
| claude | `claude-agent-acp` (adapter oficial de Zed sobre Claude Code; `CLAUDE_CODE_EXECUTABLE` = tu `claude`) | `CLAUDE_CONFIG_DIR` (main / `~/.claude-accounts/<alias>`) | `session/set_model` en vivo | `MAX_THINKING_TOKENS` (low…max) |
| codex | `codex-acp` (adapter oficial) | `CODEX_HOME` | `-c model="…"` | `-c model_reasoning_effort="…"` |
| grok | `grok agent … stdio` (nativo) | `GROK_HOME` | `-m` / `session/set_model` | `--reasoning-effort` |
| opencode | `opencode acp` (nativo) | única | `OPENCODE_CONFIG_CONTENT={"model":…}` | — |
| agy | `agy -p '' --input-format stream-json` (Antigravity no habla ACP: transporte stream-json con la misma interfaz) | única | `--model` (incluye Claude Sonnet/Opus y GPT-OSS con tu suscripción Antigravity) | `--effort` |

## Piezas

- `lib/acp.py` — cliente: `open_session(spec, cwd, model=, effort=, danger=, extra_env=)` →
  `initialize` / `session/new` / `session/load` / `session/prompt` (streaming de
  `session/update`), `session/request_permission` (handler o auto-allow),
  `fs/read_text_file` / `fs/write_text_file`, `set_model`, `set_mode`, `cancel`.
- `bin/cc-acp` — el pane: `cc-acp --agent codex --model gpt-5.5 --effort low --account main [--danger] [--resume <sid>]`.
  Comandos: `/model` `/effort` `/agent` `/account` `/models` `/mode` `/danger` `/status` `/exit`.
  `/agent` y `/account` relanzan el proceso; si el agente soporta `session/load`
  (Claude) se retoma la MISMA conversación, si no se inyecta un handoff visible.
  Publica `@ccmodel` en el pane tmux y `~/.claude/hooks/acp-panes.json` (lo usa
  cc-app para resucitar el pane tras un apagón) y reporta working/done al
  dashboard vía `cc-notify.sh`.
- `config/providers.json` — `acpAgents` (comando, placeholders `{model_args}`
  `{effort_args}` `{danger_args}`, env `which:<bin>`, `accountsProvider`),
  harness `acp`, motores `opencode`/`agy`, rutas `acp:*`, `opencode:opencode`, `agy:agy`.
  `matrixHarnesses` puede crecer; el núcleo claude/codex/grok sigue exigiendo
  cobertura explícita y las celdas sin ruta fuera del núcleo se sintetizan como
  `not_routed` (visibles, nunca seleccionables).
- `bin/cc-dash` — `/session-new` con `routeId: "acp:<motor>"` + `motorAccount`;
  `/harness/switch` con `toHarness: "acp", motor: "<motor>"` cambia un pane vivo
  a ACP con handoff. Las rutas ACP no dependen del gateway (`cc-model-proxy`).

## Pruebas

- Unitarias (sin red): `tests/test_acp_client.py` contra `tests/fake_acp_agent.py`.
- **E2E reales** contra los 5 agentes con tus suscripciones (claude main y
  relotto, codex, grok, opencode, agy): `COMANDOS_E2E=1 pytest tests/test_acp_client.py -k e2e`.

## Instalación

```bash
bun add -g @zed-industries/claude-agent-acp @zed-industries/codex-acp   # adapters oficiales
# grok, opencode y agy ya traen su modo ACP/stream-json
ln -s "$PWD/bin/cc-acp" ~/.local/bin/cc-acp
```

## Por qué ACP y no gateway para "Claude en otra TUI"

Desde abril de 2026 Anthropic bloquea harnesses de terceros sobre límites de
Max: `codex:claude` y `grok:claude` por gateway están excluidas a propósito. Por
ACP el agente **es** Claude Code (headless), así que tu suscripción aplica
legítimamente; y Antigravity ofrece Claude Sonnet/Opus con la suya. Toda la
matriz "cualquier cerebro × cualquier cuenta × cualquier modelo" vive en el
harness `acp`.
