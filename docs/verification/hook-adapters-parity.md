# Paridad de los adaptadores de hooks (Tarea 11)

Estado al 2026-10-04. Cubre `comandos hook codex|codex-hooks|gemini|agy|agy-status|grok|opencode|claude-usage|claude-status`, que reemplazan `adapters/codex-notify.sh`, `adapters/codex-hooks.sh`, `adapters/gemini-hooks.sh`, `adapters/agy-hooks.sh`, `adapters/agy-statusline.py`, `adapters/grok-hooks.py`, `adapters/opencode-comandos.js`, `hooks/cc-usage-tool.sh` y `hooks/cc-status.sh`. El hook `claude` (`hooks/cc-notify.sh`) está en `hook-claude-parity.md`.

## Método

Cada prueba corre el oráculo y el Rust con el mismo payload, `HOME` temporales independientes y entorno vacío (`env_clear`) más lo imprescindible. Nunca se tocan `~/.claude`, `~/.codex`, `~/.gemini`, el cc-notifyd real (4778), cc-dash (4777) ni el tmux del usuario: tmux, sonido, `curl` y `osascript` son binarios falsos en el `PATH`, y `COMANDOS_NOTIFYD_URL` apunta a un notifyd falso.

Los adaptadores que delegaban en `cc-notify.sh` (`exec` o `&`) se comparan por la cadena completa: el oráculo tiene detrás el `cc-notify.sh` real (envuelto con `trap wait` para esperar sus trabajos en segundo plano) y el Rust llama al pipeline de `hook claude` en proceso. En el `HOME` del Rust, `cc-notify.sh` es un stub que deja rastro si alguien lo ejecuta, así que una llamada de más se vería en la comparación.

| Prueba | Oráculo | Efectos comparados |
|---|---|---|
| `tests/hook_adapters_parity.rs` | codex-notify, codex-hooks, gemini, agy (+ `cc-notify.sh` real) | estado, `events.jsonl`, evento N1 (SQLite), base de uso, POST a cc-notifyd, llamadas a binarios falsos, `native-processes/`, stdout, código |
| `tests/hook_opencode_parity.rs` | plugin original (copia literal en `tests/fixtures/hooks/oracle/`) en el mismo proceso de node que el shim nuevo | lo mismo que arriba más las peticiones a `/event` |
| `tests/hook_python_parity.rs` | `python3 grok-hooks.py` (normalizar y `--accept`), `python3 agy-statusline.py` | stdout, código, archivo `.grok` (contenido y modo), `agy-quota.json` y todo `~/.claude/hooks` |
| `tests/hook_claude_extras_parity.rs` | `cc-usage-tool.sh` (+ `cc_usage.py` real), `cc-status.sh` | filas de la base de uso tras cada evento, llamadas a tmux; stdout byte a byte, caché y su modo, archivos sobrantes |

Fixtures en `crates/comandos-runtime/tests/fixtures/hooks/adapters/`. Son formas reales de cada harness, anonimizadas (identificadores, rutas y textos sustituidos). La de agy-status sigue la forma del `agy-quota.json` que existe en la máquina; las de codex y grok, las formas saneadas de `tests/fixtures/hook_payloads.json` ampliadas con los campos que los harness envían.

## Inventario de efectos por adaptador

### codex (`codex-notify.sh`)

- Lee el JSON del único argumento. Solo `type == "agent-turn-complete"`.
- `cwd` = `.cwd // ."workspace-path"`, si no `$PWD`.
- Una consulta a tmux (`#S`) para la clave de estado; si el estado es un `done` de codex de hace 15 s o menos, sale (dedupe con el hook `Stop`).
- Entrega `--agent codex --event done --cwd --full --hook-event Stop --session-id --turn-id` al pipeline de `hook claude`.

### codex-hooks (`codex-hooks.sh`)

- Sale si stdin está vacío o `~/.claude/hooks/cc-notify.sh` no es ejecutable (puerta conservada).
- Clave de estado calculada antes del `case` (una consulta a tmux, también para eventos ignorados).
- `UserPromptSubmit` → `working`; `Stop` → `done` con dedupe; `PermissionRequest` → `waiting` con herramienta, comando, motivo e id de petición, con las mismas listas de rutas `?` y la misma composición de `--msg`/`--full`.

### gemini (`gemini-hooks.sh`)

- `CC_AGENT` (o `gemini`) como agente; `BeforeAgent`/`AfterAgent`/`Notification`/`SessionEnd` → `working`/`done`/`waiting`/`end` con `--msg` y `--full`. No imprime nada.

### agy (`agy-hooks.sh`)

- `COMANDOS_SILENT_AGENT=1` → solo `{}`.
- Registro `native-processes/<pid>.json` del antecesor cuyo `argv[0]` es `agy` (hasta diez niveles desde el propio proceso del hook, como el `python3` del bash desde su padre): mismos campos, mismo JSON con espacios de `json.dump`, directorio 0700 y archivo 0600 vía temporal + `rename`.
- `cwd` = `.workspacePaths[0]`, si no `$PWD`; entrega `--agent agy --event E --cwd`; responde `{}`.

### agy-status (`agy-statusline.py`)

- Guarda `plan_tier` y las cubetas válidas de `quota` en `~/.claude/hooks/agy-quota.json` (temporal `<ruta>.<pid>.tmp` 0600 + `rename`), salvo que lo guardado sea igual y tenga menos de 60 s. `repr(float)`, `str()` y `json.dump(ensure_ascii=True)` de Python reproducidos. No imprime nada y siempre sale con 0.

### grok (`grok-hooks.py`)

- Normaliza (lista blanca, alias de eventos, `subagentType`, tipos de aviso, tachado de credenciales con la semántica de `re`) e imprime JSON compacto sin `ensure_ascii`.
- `--accept RUTA`: abre o crea el archivo (0600), `flock`, decide con `should_accept_event` y guarda solo `event`/`sessionId`/`promptId`; sale con 0 o 3. Un valor no *hashable* donde Python haría `in` sobre un conjunto sale con 1, como el `TypeError` del script.

### opencode (`opencode-comandos.js`)

- El plugin queda reducido a un shim que filtra los cinco tipos de evento útiles, consulta la sesión con el SDK y pasa `{directory, event, session: {id, parentID}}` a `comandos hook opencode`.
- El Rust registra `native-processes/<pid>.json` del proceso de OpenCode (el padre), solo para la sesión raíz existente, con la misma fusión de campos que el objeto `current` del plugin.
- El POST a `/event` de cc-dash, que lanzaba `cc-notify.sh --agent opencode --event E --cwd DIR`, pasa a ser la entrega en proceso con la misma validación (`cwd` absoluto, 300 caracteres).

### claude-usage (`cc-usage-tool.sh`)

- `TMUX_PANE` `%N` exacto y sesión de tmux válida; mismo evento (`phase`, herramienta, skill validada, `tool_use_id`, `at_ms`) registrado con `comandos_store::usage::tool_event`. Con varios documentos en stdin no registra nada (como `json.load`).

### claude-status (`cc-status.sh`)

- Caché de 20 s en `${XDG_RUNTIME_DIR:-/tmp}/cc-status.cache`; recorrido de los `*.json` en orden de glob; `proyecto|estado|ts` como el `jq -Rrjs`; aritmética de bash sobre `ts` (un error aborta el bucle, como en bash); `short` con hasta dos nombres y contador; iconos Nerd Font; `printf '%b'`; temporal + `mv` y `cat` de la caché.

## Desviaciones deliberadas

1. Entrega en proceso en lugar de `exec`/segundo plano de `cc-notify.sh`. gemini y agy ya no regresan antes de que termine el pipeline síncrono (estado, evento N1 y consultas a tmux); la notificación y el uso siguen en el proceso de entrega desacoplado de `hook claude`. Si `cc-notify.sh` faltara, codex-notify salía con 127 y gemini/agy no hacían nada; el Rust no depende de ese archivo (solo codex-hooks conserva su puerta `[ -x ]`).
2. claude-usage no exige `~/.local/bin/cc_usage.py`: la base de uso se escribe con `comandos_store`.
3. opencode entrega con el entorno del propio OpenCode (no el de cc-dash): bajo tmux la clave de estado es la del pane, como en el resto de agentes, y la entrega no depende de que cc-dash esté vivo. La prueba corre sin `TMUX_PANE`, donde ambos coinciden.
4. claude-status ordena los archivos por bytes (locale C/UTF-8). En un locale con colación propia (`es_MX.UTF-8`) el glob de bash puede ordenar distinto los nombres con mayúsculas o puntuación. `set -- $1` de `short` no expande comodines. La aritmética sobre `ts` acepta un operando (con signos, bases de bash y desborde de 64 bits) o un identificador; un `ts` con operadores se trata como error.
5. Python acepta `NaN`/`Infinity`, sustitutos sueltos y anidamientos de más de 128 niveles en JSON; `serde_json` no, y el Rust trata esas entradas como inválidas (sin salida).
6. Las pruebas de los oráculos Python usan `LANG=en_US.UTF-8`: con `C.UTF-8` Python lee stdin con `surrogateescape` y aceptaría bytes inválidos que con el locale del usuario rechaza.

## Resultado

`cargo test -p comandos-runtime -p comandos-cli -j 6`: todo en verde. Con el despacho de los adaptadores retirado de `hooks/mod.rs`, las cuatro pruebas fallan (estado, stdout, filas de uso y salida de grok/agy-status distintos). Mutaciones comprobadas (cada una rompe su prueba): ventana de dedupe, `busy` de agy, evento de opencode, `parentId`, umbral de 60 s de agy-status, `break` del bucle de cc-status, `printf '%b'`, umbral de 8 h, orden del glob, fase `failed`, documentos múltiples en claude-usage y `True`/`true` de grok.

## Pendiente

- `comandos hook claude` aún no procesa los payloads de Grok (`hookEventName`): `input.rs` los descarta. `grok::normalize` y la lógica de `--accept` quedan listas para integrarlas allí.
- `tests/test_native_extension_metadata.py` probaba el plugin de OpenCode completo; con el shim necesita un `comandos` en el `PATH`.
