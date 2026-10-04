# `comandos hook claude`: paridad con `hooks/cc-notify.sh`

Fecha: 2026-10-04. Prueba: `crates/comandos-runtime/tests/hook_claude_parity.rs`.

## Método

Cada escenario corre el oráculo (`hooks/cc-notify.sh`, sin modificar) y el Rust (`comandos-hook claude`, mismo código que `comandos hook claude` y que el symlink `cc-notify.sh`) con el mismo payload, dos `HOME` temporales independientes y el mismo entorno mínimo (`env_clear`). El `PATH` empieza por binarios falsos que genera la prueba (`tmux`, `pw-play`, `paplay`, `piper`, `spd-say`, `osascript`, `curl`), así que ninguno de los dos lados toca el tmux, el audio ni el cc-notifyd reales. El bash se lanza como `bash -c 'trap wait EXIT; . "$0" "$@"' hooks/cc-notify.sh` para que la prueba espere sus trabajos en segundo plano; el Rust entrega en un proceso desacoplado y la prueba espera a que iguale lo que hizo el bash, más un margen para detectar efectos de más.

Se compara byte a byte, normalizando solo las marcas de tiempo del intervalo de la prueba (segundos y milisegundos) y los identificadores aleatorios hexadecimales:

- archivos de `~/.claude/hooks/state/` (nombre, modo y contenido);
- `~/.claude/hooks/events.jsonl` (contenido y modo);
- cuerpo del POST a cc-notifyd (el `-d` que recibe el `curl` falso del bash contra el cuerpo que recibe el cc-notifyd falso en hyper del Rust) y la URL del bash;
- registro de los binarios externos (argumentos y, para piper, la frase por stdin);
- base N1 `~/.local/state/comandos/app-state.sqlite3` y base de uso `~/.claude/hooks/comandos-usage.sqlite`: esquema, `user_version` y todas las filas;
- código de salida.

## Efectos observables del bash

1. `COMANDOS_SILENT_AGENT=1`: sale sin hacer nada.
2. `mkdir -p ~/.claude/hooks/state`.
3. Lee `cc-notify.conf` (por `source`) sobre los valores por defecto.
4. `tmux display-message` para sesión y `pane_pid`, solo con `TMUX_PANE=%N`.
5. Estado `state/<clave>.json`: temporal en el mismo directorio, `chmod 644`, `mv`; JSON indentado de jq.
6. Línea en `events.jsonl` bajo `flock` de `events.jsonl.lock` (espera 3 s); recorte a 500 líneas pasadas las 2000 (el archivo recortado queda en 0600 por `mktemp`).
7. Evento N1 en SQLite (`lib/event_intake.py record --claim-sound desktop-<host>`) y la decisión de sonido (`play`, vacío o `legacy`).
8. Contabilidad de uso: `cc_usage.py capture-hook` (solo con números `COMANDOS_USAGE_*`) y `cc_usage.py lifecycle`.
9. `SessionEnd`: borra el estado y `.<clave>.grok`.
10. POST `http://127.0.0.1:4778/notify` (curl `-m 2`); si falla, `osascript` (macOS).
11. Voz (piper con su `.wav` temporal y `pw-play`, `say` o `spd-say`) o chime (`pw-play`, `paplay` o `afplay`).
12. Código de salida 0 en todos los caminos.

Todos se reproducen salvo lo indicado en “Desviaciones”.

## Escenarios y resultado

| Escenario | Qué cubre | Resultado |
|---|---|---|
| `prompt` | `UserPromptSubmit` en pane de tmux; conserva la respuesta previa en `last` | igual |
| `stop` | `Stop` con transcript (markdown, `tool_result`, corte de 180 bytes en mitad de una `ñ`), uso con seis números | igual |
| `notification_permission` | permiso: opciones `Si␟Si, siempre␟No`, preview del turno, chime | igual |
| `notification_idle` | `idle_prompt` (N1 lo ignora), anti-spam por `waiting` reciente | igual |
| `notification_ask` | `AskUserQuestion`: pregunta, etiquetas y lista numerada | igual |
| `session_end` | borra estado y `.grok`, `lifecycle end` | igual |
| `stop_piper` | voz con piper (registro N1 caído → `legacy`), inglés | igual |
| `stop_spd` | `spd-say`, `CC_LANG=auto` con `LANG=es_MX.UTF-8`, volumen 7 | igual |
| `stop_chime` | chime con `VOLUME=250` recortado a 100 | igual |
| `adapter_codex` | modo adaptador `--agent codex --event waiting …` con todos los IDs | igual |
| `curl_fails` | POST fallido → `osascript` con el cuerpo truncado a 200 bytes | igual |
| `precompact_quiet` | evento desconocido (rama `done`) con `NOTIFY_ON_DONE=0` | igual |
| `silent_agent` | `COMANDOS_SILENT_AGENT=1` | igual |

Además, `truncated_payload_writes_nothing`: un JSON truncado no deja estado, timeline, bases ni llamadas externas.

## Tiempos (evento `Stop` con transcript, sin tmux)

20 corridas por lado tras una de calentamiento, bucle con `date +%s%N` (no hay `hyperfine`), máquina de 28 núcleos con carga media 4-6, binario `release`:

| | mín. | mediana | media | máx. |
|---|---|---|---|---|
| bash (`hooks/cc-notify.sh`) | 308,7 ms | 360,6 ms | 362,8 ms | 425,0 ms |
| Rust (`comandos hook claude`) | 11,8 ms | 17,0 ms | 36,0 ms | 81,9 ms |

El tiempo del bash no incluye `cc_usage.py`, que deja en segundo plano; el del Rust sí incluye la contabilidad de uso (hilo unido antes de salir). En los dos casos el POST y el sonido quedan fuera: corren desacoplados.

## Desviaciones deliberadas

- JSON inválido, truncado o que no es objeto: el bash lo toma como un `Stop` sin datos y escribe estado del `$PWD`; el Rust no deja rastro (sale 0).
- Payload de Grok (`hookEventName`): el bash llama a `adapters/grok-hooks.py`; el Rust aún no (Tarea 11) y sale 0 con un aviso en stderr.
- Bandera del modo adaptador sin valor: el bash se cuelga (`shift 2` falla); el Rust la toma como vacía.
- Contabilidad de uso: el bash solo la hace si existe `~/.local/bin/cc_usage.py`; el Rust siempre (en producción el archivo existe). `capture-hook` y `lifecycle` corren en orden; en el bash son dos procesos en paralelo que pueden cruzarse.
- `VOLUME` con ceros a la izquierda: el bash usa aritmética octal en `paplay`/`spd-say`; el Rust decimal (no probado).
- `sqlite3 .dump` no está en `/usr/bin` de esta máquina: el volcado se hace con rusqlite (esquema, `user_version` y filas).
