# Cutover de hooks a Rust — 4 de octubre de 2026

Desde las 04:52 CST los tres hooks de Claude Code apuntan al binario Rust
(`~/.local/share/comandos/bin/comandos`, commit `153c06e` de `migration/rust-full`):

| Enlace | Antes | Ahora | Reversión |
|---|---|---|---|
| `~/.claude/hooks/cc-notify.sh` | `hooks/cc-notify.sh` (bash) | `comandos hook claude` | `comandos install --rollback cc-notify.sh` |
| `~/.claude/hooks/cc-usage-tool.sh` | `hooks/cc-usage-tool.sh` (bash) | `comandos hook claude-usage` | `comandos install --rollback cc-usage-tool.sh` |
| `~/.claude/hooks/cc-status.sh` | `hooks/cc-status.sh` (bash) | `comandos hook claude-status` | `comandos install --rollback cc-status.sh` |

Los hooks se lanzan por turno: no hubo proceso que interrumpir. Grok ya entraba por
`~/.claude/hooks/cc-notify.sh` (`~/.grok/hooks/comandos.json`), así que quedó migrado en el mismo paso.

Los adaptadores de los demás harnesses se referencian por ruta del checkout principal, no por enlace;
se sustituyeron por envoltorios `exec` de dos líneas (los originales siguen en git):

| Archivo (`adapters/`) | Quién lo llama | Ahora ejecuta |
|---|---|---|
| `codex-notify.sh` | `~/.codex/config.toml` `notify` | `comandos hook codex` |
| `codex-hooks.sh` | `~/.codex/hooks.json` | `comandos hook codex-hooks` |
| `gemini-hooks.sh` | `~/.gemini/settings.json` (`CC_AGENT=gemini …`) | `comandos hook gemini` |
| `agy-hooks.sh` | `~/.gemini/antigravity-cli/settings.json`, `~/.gemini/config/hooks.json` | `comandos hook agy` |
| `agy-statusline.py` | `~/.gemini/antigravity-cli/settings.json` | `comandos hook agy-status` |
| `opencode-comandos.js` | `~/.config/opencode/plugin/comandos.js` → este archivo | shim JS que llama `comandos hook opencode` |

`adapters/grok-hooks.py` se conserva: ya nadie lo invoca (el Rust normaliza Grok dentro de `hook claude`).

## Paridad antes del cambio (sin tocar nada real)

- Pruebas de paridad del repo contra los oráculos bash/Python: 149 pruebas de `comandos-runtime`
  (14 escenarios de `cc-notify.sh`, 10 de Grok, adaptadores de codex/gemini/agy/opencode/status/usage).
- Sombra del controlador con **payloads reales** en HOMEs temporales, sonido/popup/tmux/curl falsos y
  notifyd falso:
  - `cc-notify.sh` con 3 transcripts reales (9, 83 y 199 MB) × 5 eventos (UserPromptSubmit, Stop,
    Notification, Stop, SessionEnd): 15/15 idénticos en `state/*.json`, `events.jsonl`, 9 avisos al
    notifyd, 33 llamadas a sonido/popup/tmux y la base de uso (única diferencia: la duración de la
    interacción, que mide el reloj del arnés).
  - `cc-usage-tool.sh` con 40 llamadas reales (31 Bash, 9 Read): 41 filas idénticas.
  - `cc-status.sh` sobre el `~/.claude/hooks/state` real (476 archivos): salida y caché idénticas bajo
    `en_US.UTF-8`, `es_MX.UTF-8` y `C`.
- Envoltorios: los seis probados en HOME temporal, rc 0, estado y eventos correctos.

## Después del enlace

- Barra de tmux por el enlace: idéntica.
- Esta misma sesión (pane `%26`) registró sus llamadas de herramienta en `comandos-usage.sqlite` vía Rust
  en el primer minuto; su `state` se actualiza por turno (`working`/`done`, `last` conservado).
- Sesión de prueba `claude --debug -p`: `working` → `done` con el texto exacto de la respuesta en
  `events.jsonl`; `SessionEnd` limpió el `state`; 0 errores de hook en el debug.
- Reglas de oro: 22 sesiones tmux, 316 proxies Python intactos, `cc-dash`/`cc-app` sin tocar.

## Latencia (misma máquina, bases reales)

| Hook | bash/Python | Rust |
|---|---|---|
| `Stop` con transcript de 199 MB | 0.52 s | 0.17 s |
| `PreToolUse`/`PostToolUse` (por llamada) | 115 ms (máx 266) | 4 ms (máx 9) |
| `cc-status.sh` sin caché (476 estados) | 0.38 s | < 10 ms |

## Broker (`comandos-broker.service`)

Activado a las 05:13 y desactivado a las 05:20 por el controlador: el broker contestaba a todos los
clientes la versión de protocolo de su `initialize` interno (`2025-06-18`) y Claude Code 2.1.289 sondea
con `2025-11-25`, lo que lo hacía reconectar «pinned legacy». Los `serve` vuelven solos al proxy directo
cuando no hay broker. Reactivado a las 05:31, tras verificar la Task 12b (negociación por cliente);
ver `cutover-broker.md`.
En los 7 minutos activos: 14 upstreams compartidos arrancaron con el PATH del cliente (npx, uvx, ssh),
cgroup 622 MiB, daemon 3 MiB.
