# Fase 2d — `/state` nativo y terminal rápida: plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** que el frente Rust `comandos dash` (4777) responda él mismo, byte a byte igual que el `cc-dash` Python, GET `/state` —la ruta más pesada del tablero, sondeada cada 2–3 s por `index.html`, `cc-app` y `cc-notifyd`—, POST `/workspace/sort` en modo `by` y POST `/terminal/quick` en la barra lateral, con un criterio de memoria y latencia medido, sin tocar el Python.

**Architecture:** tres capas. (1) **Lectores** en `comandos-runtime`, sin red ni tmux y con raíces inyectables (`home`, `proc_root`): `tui_state` (transcripts, pantalla, rastreador de configuración, metadatos de Grok), `pane_snapshot` (`PaneInspector`), `agent_procs` (procesos de agentes, dueños por pane, cuentas por pid) y `providers` (registro, motores, tiers, rutas seleccionables). (2) **Ensamblado** en `crates/comandos-server/src/dash/native/states/`: el bucle de `read_states` portado literalmente sobre un rasgo `CardEffects` (tmux, cuentas, observación, base de uso, ssh), las sugerencias, la proyección `app-tab-models.json` y el orden final. (3) **Ruta**: `states::Engine` con la caché de 1,2 s y un solo cómputo en vuelo, la recolección real (escaneos en `spawn_blocking`, tmux por `tokio::process`, base de uso por el carril de la 2c), el contexto de sugerencias (60 s) y la escritura de `app-tab-models.json` solo después de saber que no se declina.

**Tech Stack:** Rust 1.96 (edition 2024, `unsafe_code = "forbid"`), tokio 1.53 (`rt`, `process`, `sync`, `time`), hyper 1.11 (cliente para la subconsulta al heredado), rusqlite 0.40, serde_json con `arbitrary_precision` + `preserve_order`, `regex = "=1.13.1"` (nuevo en `comandos-runtime`), crates `comandos-core`/`comandos-store`/`comandos-runtime`; arnés `xtask parity`/`xtask poll` en un netns (`unshare -Urn`).

**Spec:** `docs/superpowers/specs/2026-10-04-comandos-rust-complete-design.md` (§4.2, §5, §7, Enmiendas 1–8). Inventario: `docs/research/2026-10-04-fase-2-inventario.md` (§1.3, §1.11, §3.3, §3.6, §9.7, §9.8). Planes previos: `docs/superpowers/plans/2026-10-04-fase-2b-dominios-nativos-i.md`, `docs/superpowers/plans/2026-10-04-fase-2c-dominios-nativos-ii.md` (sus bloques Interfaces son la fuente de los nombres de la 2c que este plan consume: `Lane<UsageBackend>`, `Native.usage`, `NativeOptions.{usage_db, repo_root}`, `files::{loads_strict, write_text_atomic, FileLock}`, `Tmux::run_blocking`, `TestHome::usage_db()`, `support::repo_root`). Procedimiento vivo: `docs/verification/cutover-dash.md`. Oráculo: `bin/cc-dash`, `lib/tui_state.py`, `lib/pane_snapshot.py`, `lib/providers.py`, `lib/accounts.py`, `lib/grok_state.py`, `lib/quick_terminal.py` de este checkout (las líneas citadas abajo son las de `bin/cc-dash` en `65528ca`).

**Precondición:** la Fase 2c está fusionada en `main` (sus módulos existen). Si al empezar algún nombre de la 2c no coincide con su bloque Interfaces, se usa el real y se anota en el commit; no se reescribe la 2c.

**Cómo se dan los ports:** el código completo va donde hay diseño nuevo (caché y vuelo único, orden declinar→escribir, recolección, contexto de sugerencias, terminal rápida, rastreador). Los ports mecánicos (expresiones de `tui_state`, el BFS de `PaneInspector`, `validate_registry`, el bucle de tarjetas) se dan por firma, regla línea a línea con la cita del Python y una prueba diferencial que corre el Python real sobre las mismas entradas y compara bytes: esa prueba es la especificación ejecutable.

---

## Rulings del controlador que fijan este plan

1. **Respuestas idénticas byte a byte** vía `comandos_core::json::response_dumps` (status, `Content-Type`, `Cache-Control: no-store`, cuerpo). `app-tab-models.json` se escribe con el mismo volcado que `write_json_file` (`files::write_json_atomic`).
2. **SQLite solo a través de workers**: `app-state` por el `BackendWorker<StateBackend>` (reclamos de la terminal rápida); la base de uso por `Native.usage` (`Lane<UsageBackend>`, 2c) para `latest_session_config`. Nunca desde el hilo del runtime.
3. **`Decline` antes de cualquier efecto.** El efecto de GET `/state` es escribir `H/app-tab-models.json` (`write_app_tab_models` 6838). Orden obligatorio: calcular todo, serializar el cuerpo y el JSON de modelos, declinar si alguna tarjeta o pieza quedó incierta, y **solo entonces** escribir. Excepciones documentadas: (a) POST `/workspace/sort` modo `by` calcula `/state` antes de su trabajo en la base (la escritura es idempotente: es una proyección del estado vivo que el Python reescribiría igual); (b) POST `/terminal/quick`: tras el reclamo (fila `launching` y carpeta reservada) ya no se declina, como el Python.
4. **tmux y procesos por `tokio::process`** con los plazos del Python: `tmux()` 5 s (5833), `pane_status_hint` 2 s (6289), `ssh -O check` 3 s (7832), lanzamiento de la terminal rápida 15 s (5609).
5. **Nada que bloquee más de milisegundos en el runtime `current_thread`**: el escaneo de los ~476 `H/state/*.json`, `/proc`, `PaneInspector`, colas de transcripts (hasta 2 MiB), `environ`, `fd` → `tokio::task::spawn_blocking`.
6. **Caché de 1,2 s con un solo cómputo en vuelo**, como `read_states_cached` (7315): los que llegan durante un cómputo esperan y reciben su resultado o su error; los errores no se cachean.
7. **Tabla nativa**: coincidencia exacta sobre la ruta cruda, por método; `--no-native` y `COMANDOS_DASH_NATIVE=0` sin cambios.
8. **Paridad**: líneas de fixture por ruta (`state-*` dejan de ser `forwarded`, `d-*` nuevas) y pruebas contra el oráculo Python por ruta y por capa.
9. **Cutover**: sección «2d» en `docs/verification/cutover-dash.md` con rutas explícitas (`NEW=…/.build/target/release/comandos`, `grep -qa`), sombra `"$NEW" dash 4782 --legacy-port 4781`, verificación, medición y reversión.
10. Comentarios en español, identificadores en inglés, sin `unsafe`, sin `unwrap`/`expect`/indexado en código que no sea de prueba, clippy `-D warnings`, rustfmt, trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, `CARGO_TARGET_DIR=<checkout>/.build/target`, `nice -n 10 cargo … -j 6`, `git add` explícito, `git add -f` para `docs/superpowers` y `*.jsonl`; cero archivos Python o bash nuevos; las pruebas nunca tocan el HOME, las bases, el tmux ni los puertos 4777–4782 reales.
11. **Tareas desplegables una a una**: lectores (1–3) → ensamblado (4) → ruta, caché y escritura (5) → rutas dependientes (6) → medición y cutover (7).

Reglas de oro heredadas: los cutovers los ejecuta el controlador; el Python no se modifica.

## Global Constraints

- Todo se compila y prueba con
  `CARGO_TARGET_DIR=/home/someguy/codebase/0xJesus/ComandOS/.build/target nice -n 10 cargo <cmd> -j 6`
  (abreviado `$C <cmd>`). Antes de cada commit: `$C fmt --all -- --check`, `$C clippy --workspace --all-targets -j 6 -- -D warnings` y las pruebas de los paquetes tocados.
- Commits con `git add <rutas>` explícitas; mensajes `feat(runtime): …` / `feat(dash): …` / `docs(verification): …` en español, con línea en blanco y `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. El plan: `git add -f docs/superpowers/plans/2026-10-04-fase-2d-state-nativo.md`; el fixture: `git add -f xtask/parity/frente.jsonl`.
- Pruebas: `support::TestHome` (HOME temporal, `TMUX_TMPDIR` propio, sin `COMANDOS_STATE_DB`/`COMANDOS_USAGE_DB`/`CLAUDE_CONFIG_DIR`/`CODEX_HOME`/`GROK_HOME`). Sin `tmux` o sin `python3` la prueba se salta con un aviso (no `#[ignore]`).
- Oráculo de capas: invocaciones `python3 -c <texto>` dentro de las pruebas Rust que importan `lib/*.py` o cargan `bin/cc-dash` con `SourceFileLoader` (como `tests/test_agent_launch.py:load_dash_module`) y sustituyen sus funciones de efecto por las del caso. No se añade ningún archivo `.py` (precedente: `crates/comandos-runtime/tests/hook_claude_parity.rs` con `bash -c`).
- Excepción no capturada del Python → `HandlerError::Failure` (500 `{"error": "Error interno del tablero"}`); `subprocess.TimeoutExpired` no capturado → `HandlerError::Timeout` (504).
- Directorios: se recorren en el orden de `std::fs::read_dir` (el mismo `getdents` que `os.scandir`/`glob`); nunca se ordenan. `glob("*")` no devuelve nombres que empiezan por `.`.

## Decisiones del plan (no fijadas por el controlador)

- **D1 — Memoria del Python que el frente no puede leer**:
  - `_TUI_STATES` (`StateTracker`), `_TUI_TRANSCRIPTS`, `_GROK_METADATA`, `_ACCOUNT_PID_CACHE`, `_ACCOUNT_EMAIL_CACHE` (1397-1399, 3895-3896): el frente tiene los suyos (como la caché de `/pane/type` en la 2c). Las cachés dan el mismo resultado por construcción (firma de archivo, pid+inicio). El rastreador no: «la evidencia que cambió gana» depende del historial de observaciones; tras arrancar el frente su primera observación equivale a un reinicio del Python. Diferencia aceptada y documentada en el cutover.
  - `MOTOR_RESULT` (3636): cada cambio pasa por `_set_motor_result` (3651), que reescribe `H/motor-results.json` bajo candado. El frente lee ese archivo en cada cómputo: ausente → `{}`; ilegible, incierto, no-objeto o con un valor que no es objeto → **declinar** (la memoria del Python es desconocida).
  - `_suggest_cache` (7069): `routes` se calcula en Rust (`providers::selectable_routes`, puro sobre el registro, cuentas, `which` y el sondeo del proxy); `guard` = GET `/usage/guard` del heredado (8502, exactamente `token_guard_with_forecast()`) y `latency` = proyección de GET `/usage/analytics?days=7` (8553, exactamente `cc_usage.experiment_analytics(USAGE_DB, 7, "")`), cada 60 s y **solo si alguna tarjeta lo necesita** (la salida no cambia: `_annotate_suggestion` no lo mira en las demás). Respuesta no-200 del heredado → `{}` (el `except` del Python ante la misma excepción); heredado inalcanzable o plazo vencido → declinar sin cachear. Dependencia temporal: la 2e porta `token_guard_report` y `experiment_analytics` y la elimina.
- **D2 — Catálogo de incertidumbre** (cualquiera → declinar todo `/state`, sin escribir): un `H/state/*.json` que `files::loads_strict` clasifica `Unsure`; `ts` no finito u `OverflowError` de `float()`; un texto que el Python formatearía con `str()` de un contenedor y llega a la salida (etiqueta de split, `previousPane`); `py::is_pane` → `None`; `cwd` de proceso no UTF-8; registro de proveedores que no valida o con un patrón que Rust no puede juzgar; `cc-notify.conf` existente pero ilegible; `motor-results.json` como en D1; `model-tiers.json` con un patrón incierto o un elemento no-objeto; columna BLOB en `usage_session_configs`; una línea de transcript cuyos campos usados contienen U+FFFD tras normalizar sustitutos; texto de pantalla con U+001F; un sujeto de regex con `\n` o no ASCII; `response_dumps` que falla.
- **D3 — Orden de errores**: el bucle de tarjetas es secuencial y en el orden del Python (por pane: pista Codex → ocupado Claude → cuenta → Grok → observación → configuración → ssh). El primer error fatal es el del Python: tmux con plazo vencido → 504; tmux que no arranca o salida no UTF-8 fuera de `reconcile_card_config` → 500; dentro de la observación, `ValueError`/`OSError` → «unconfirmed» (6997), plazo vencido → 504. La latencia la domina tmux igual que en el Python; la caché y el vuelo único reparten el coste.
- **D4 — Vuelo único con abandono**: el líder calcula dentro de su petición; si su cliente se desconecta, una guarda al soltarse publica «abandonado» y los que esperaban repiten (uno pasa a líder). El Python nunca abandona (su hilo termina); repetir solo cuesta trabajo, no cambia la salida.
- **D5 — Caché de registros por firma**: `(dev, ino, size, mtime_ns, ctime_ns)` por ruta; un archivo sin cambios no se reparsea; entradas que desaparecen se sueltan. Misma salida, menos churn (el criterio de memoria de la Tarea 7).
- **D6 — Reloj**: `NativeOptions.clock` (ms) para `now_ts`, `observedAt`, `evidenceAt`, la edad de la caché (1,2 s) y del contexto (60 s). `observedAt`/`evidenceAt` son volátiles en la paridad (el Python usa `time.time()` con microsegundos).
- **D7 — POST `/terminal/quick` solo con `place == "sidebar"`**; si no, declina antes de nada: el registro de pestaña (`quick_terminal_register` → `register_app_tab` 5439 → `write_tab_metadata` 5392) escribe `app-tabs-meta.json` bajo `TAB_METADATA_LOCK`, un candado **de proceso** del Python; un segundo escritor en otro proceso perdería actualizaciones. Ese camino va con las mutaciones de pestañas. El nativo nunca retiene el worker: reclamo y cierre son trabajos cortos; la espera (50 ms) y el lanzamiento (15 s) corren fuera.
- **D8 — POST `/workspace/close-group` sigue reenviado**: `close_app_tab` (5468) = `remember_tab` + registro + `remove_tab_metadata` (mismo candado de proceso) + `workspace_sync` + `app-tab-close.json`. No depende de `/state`; va con las mutaciones de pestañas (`/tab-close`, `/tab-new`, `/recover-tab`, `/terminal/quick` fuera de la barra).
- **D9 — `which()` y entorno**: `providers::which` replica `lib/providers.py:44` (PATH + `_USER_BIN_DIRS`) con el `PATH` del frente capturado al arrancar (`NativeOptions.search_path`); `COMANDOS_QUICK_TERMINAL_BASE`, `CODEX_HOME`, `GROK_HOME` y el directorio de trabajo también se leen del entorno del frente. El paso 0 del cutover comprueba que las dos unidades tienen el mismo `PATH`, `WorkingDirectory` y esas variables.
- **D10 — `re` de Python → `regex`**: `providers::py_regex` traduce `\Z`→`\z` y antepone `(?i)` con `re.I`; rechaza como incierto `(?` que no sea `(?:`, referencias `\1`–`\9` y todo lo que `regex` no compile. Un sujeto con `\n` (el `$` de Python también casa antes del salto final) o no ASCII (plegado de mayúsculas) → incierto. Las expresiones fijas de `tui_state` se escriben a mano con las mismas clases; `\s` de Python incluye U+001C–U+001F: una línea con U+001F es incierta (U+001C–U+001E ya parten líneas en `str.splitlines`).
- **D11 — Subconsulta al heredado** (`native/subrequest.rs`): una conexión nueva por la misma puerta de ritmo de `forward.rs`, `Host: 127.0.0.1:<puerto>` y `X-Comandos-Token`, plazo 10 s. `build` fija `NativeOptions.{legacy, legacy_token}` desde `DashConfig` también cuando las pruebas inyectan opciones.

## Mapa de rutas nativas

| Ruta | Llave | Python | Respuesta 200 | Errores | T |
|---|---|---|---|---|---|
| GET `/state` | Path | 8388 `read_states_cached` 7315 → `read_states` 7159 | lista de tarjetas (orden: vivas, con agente, `-float(ts)`; ver Tarea 4) | 500/504 del primer fallo (D3); **efecto**: escribe `H/app-tab-models.json`; incierto → declina | 4, 5 |
| POST `/workspace/sort` (`by`) | Raw | 8789 `workspace_sort` 6497 | `{…workspace_payload…, "previous": [...]}` | 400 `Orden desconocido`, `app-tabs.json no es un objeto`; 409; registro ilegible → declina | 6 |
| POST `/terminal/quick` (`place:"sidebar"`) | Raw | 9520 `quick_terminal_request` 5631 | `{tabId,paneKey,cwd,label,created}` | 400 `{error:"requestId inválido",code:"request",retryable:false}`; 409 `busy`; 500 `folder`; 502 `launch` (+`cwd`) | 6 |

`Path` = el `startswith` del Python reclamando solo la ruta exacta con consulta opcional (`/stateX` y `/state/…` se reenvían). POST `/workspace/sort` con `restore` ya es nativo desde la 2b.

## Review Focus

1. **El cliente líder se desconecta a mitad del cómputo** mientras `cc-app`, el tablero y `cc-notifyd` esperan el mismo vuelo: los demás reciben respuesta (repiten), nadie se cuelga. Prueba `state_flight_survives_leader_cancel` (Tarea 5).
2. **Un `H/state/*.json` reescrito durante el escaneo** (a medias o borrado) se salta como en el Python; uno incierto reenvía todo `/state` sin escribir `app-tab-models.json`. Prueba `state_unsure_record_declines_without_writing` (Tarea 5).
3. **tmux colgado en la captura de un pane Codex** (plazo 2 s): 504 como el Python y el runtime sigue atendiendo otras rutas mientras tanto. Prueba `state_hung_tmux_answers_504_runtime_free` (Tarea 5).
4. **Contexto de sugerencias**: heredado caído con el contexto vencido y una tarjeta que lo necesita → declina (nunca responde con contexto vacío); `/usage/guard` respondiendo 500 → `{}` cacheado 60 s, como el `except` del Python. Prueba `suggestion_context_legacy_500_is_empty_and_down_declines` (Tarea 5).
5. **Doble clic en la terminal rápida de la barra** (mismo `requestId` concurrente): una sola shell, la segunda espera el reclamo sin retener el worker (otra ruta nativa de la base responde mientras tanto). Prueba `quick_terminal_concurrent_same_request_one_shell_worker_free` (Tarea 6).

## Estructura de archivos

```
crates/comandos-core/src/text.rs                       (splitlines movido desde native/py.rs)      T1
crates/comandos-runtime/Cargo.toml                     (regex = "=1.13.1")                          T1
crates/comandos-runtime/src/lib.rs                     (Unsure, módulos nuevos)                    T1–T3
crates/comandos-runtime/src/tui_state.rs               (model_id, screen_state, TranscriptCache,
                                                        StateTracker, GrokMetadataCache)            T1
crates/comandos-runtime/src/pane_snapshot.rs           (PaneInspector)                              T2
crates/comandos-runtime/src/agent_procs.rs             (procesos, inventario, dueños, AccountCache) T2
crates/comandos-runtime/src/providers.rs               (py_regex, registro, motores, tiers, rutas)  T3
crates/comandos-runtime/src/quick_terminal.rs          (claim/finish públicos)                      T6
crates/comandos-runtime/tests/support/python.rs        (run_python)                                 T1
crates/comandos-runtime/tests/tui_state_oracle.rs, pane_snapshot_oracle.rs, providers_oracle.rs    T1–T3
crates/comandos-server/src/dash/mod.rs                 (build: legacy, legacy_token)                T5
crates/comandos-server/src/dash/forward.rs             (connect_paced pub(crate))                   T5
crates/comandos-server/src/dash/native/
  mod.rs        (StateFault, NativeRoute::{State, QuickTerminal}, campos, opciones)               T4–T6
  py.rs         (splitlines reexportado)                                                           T1
  light.rs      (read_tab_history → pub(crate))                                                    T5
  subrequest.rs (GET al heredado)                                                                  T5
  workspace.rs  (Sort con by)                                                                      T6
  quick.rs      (POST /terminal/quick)                                                             T6
  states/mod.rs      (StateFault, Engine, ROUTES, answer, states_cached)                           T4, T5
  states/records.rs  (RecordCache)                                                                 T4
  states/cards.rs    (Inputs, CardEffects, build, sort_items)                                      T4
  states/observe.rs  (ObserveEvidence, observe, reconcile)                                         T4
  states/suggest.rs  (SuggestContext, needs_context, annotate_all)                                 T4
  states/tab_models.rs                                                                             T4
  states/cache.rs    (StatesCache: TTL 1,2 s + vuelo único)                                        T5
  states/gather.rs   (RealEffects, scan, compute)                                                  T5
  states/context.rs  (contexto de sugerencias 60 s)                                                T5
crates/comandos-server/tests/support/mod.rs, support/oracle.rs  (opciones nuevas, run_python,
                                                        fake_agent, start_session)                 T4–T6
crates/comandos-server/tests/dash_native_state_cards.rs                                             T4
crates/comandos-server/tests/dash_native_state.rs                                                   T5
crates/comandos-server/tests/dash_native_quick.rs, dash_native_workspace.rs                         T6
xtask/src/poll.rs                                      (latencia p50/p95/p99 por ruta)              T7
xtask/parity/frente.jsonl                                                                           T5, T6
docs/verification/cutover-dash.md                      (sección «2d»)                               T7
```

---

### Task 1: Lectores TUI (`comandos_runtime::tui_state`)

Port de `lib/tui_state.py` completo, sin tmux ni red. Desplegable sola (nadie lo usa aún; sus pruebas comparan con el Python).

Comportamiento portado (líneas de `lib/tui_state.py`):

- **`model_id`** (16-26): `re.findall(MODEL, label, re.I)`; si alguna coincidencia tiene `-`, la última en minúsculas; si no, alias de pantalla `(?:Claude\s+)?(Opus|Sonnet|Haiku|Fable)\s+(\d+(?:[.\-]\d+)*)` → `claude-<familia>-<versión con . → ->` + `[1m]` si el texto dice `1M context` o `[1m]`; si no, `Grok\s+(\d+(?:\.\d+)*)` → `grok-<v>`; si no, la última coincidencia o `""`.
- **`screen_state`** (29-75): `ANSI` fuera, `str.splitlines()` (`comandos_core::text::splitlines`); codex: solo las últimas 8 líneas, `fullmatch` del pie `<modelo> <esfuerzo> ·`; opencode: línea `┃ <agente> · <resto>` seguida de una que empieza por `╹▀`; claude: respuestas `●` invalidan lo anterior, `❯ /model|/effort` cierra la respuesta, confirmaciones `⎿ Set model to|Kept model as|Current model:` y `⎿ Set effort level to|Effort (level) set to` (sin `but`/`not applied`), y en las últimas 5 líneas el estado `· claude · <modelo>` solo si no hubo otra cosa. Claves en el orden de inserción del Python (`model`, `effort`, `kind`).
- **`TranscriptCache.read`** (82-148): firma `(st_dev, st_ino, st_size, st_mtime_ns)` de `stat(path)` (sigue enlaces, como `/proc/<pid>/fd/<n>`); acierto → copia; si no, lee los últimos `max_bytes` (descarta la primera línea si no empieza en 0), `bytes.splitlines()` (`\n`, `\r`, `\r\n`), `json.loads(bytes)` por línea (BOM UTF-8 admitido; no UTF-8 → se salta), filtros `isSidechain`/`parent_session_id`/`sessionId|session_id` distinto, extracción por arnés, validaciones de `model` (`[A-Za-z0-9][A-Za-z0-9._:@/\-\[\]]{0,159}`, sensible a mayúsculas) y `effort` (`EFFORT` sin `re.I`), `result.pop('effort')` al cambiar de modelo, `revision = uuid or timestamp or f'{signature}:{index}'` (la tupla con el `repr` del Python: `(dev, ino, size, mtime_ns)`), y la conservación del resultado previo si el archivo creció sin estado nuevo. LRU de 128.
- **Sustitutos sueltos**: el `json` del Python los acepta; aquí la línea se parsea con los escapes `\uD800`–`\uDFFF` sueltos cambiados por `\uFFFD`; si un campo **usado** (`type`, `message.model`, `message.content`, `effort`, `perTurnEffort`, `payload.*`, `sessionId`, `session_id`, `uuid`, `timestamp`) contiene U+FFFD en una línea retocada, el resultado es `Err(Unsure)`. Anidamiento que el parser rechaza → `Err(Unsure)`.
- **`StateTracker.observe`** (151-187): código completo abajo (es la pieza con estado).
- **`GrokMetadataCache.read`** (190-236) + `grok_state._summary_public` (`lib/grok_state.py:100`): `active_sessions.json` → `pid → session_id` (último gana); sesión `[A-Za-z0-9_-]+`; `sessions/**/<sid>/summary.json` recursivo con exactamente una coincidencia (caché de rutas); resumen proyectado; resultado `{…resumen, sessionId, home}`. `json.load` de texto: no UTF-8 → `{}`.

**Files:**
- Create: `crates/comandos-core/src/text.rs`; Modify: `crates/comandos-core/src/lib.rs` (`pub mod text;`)
- Modify: `crates/comandos-server/src/dash/native/py.rs` (borrar el cuerpo de `splitlines`, `pub use comandos_core::text::splitlines;`)
- Modify: `crates/comandos-runtime/Cargo.toml`, `crates/comandos-runtime/src/lib.rs`
- Create: `crates/comandos-runtime/src/tui_state.rs`
- Create: `crates/comandos-runtime/tests/support/python.rs`, `crates/comandos-runtime/tests/tui_state_oracle.rs`

**Interfaces:**
- Consumes: `comandos_core::{json::{workspace_loads, python_eq, truthy}, text::splitlines}`.
- Produces: `comandos_core::text::splitlines(&str) -> Vec<&str>`; `comandos_runtime::Unsure` (`#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct Unsure;`); `comandos_runtime::tui_state::{Obs (= serde_json::Map<String, Value>), model_id(&str) -> String, screen_state(&str, &str) -> Result<Obs, Unsure>, TranscriptCache::{new(usize, u64), read(&mut self, &str, &str, &Path) -> Result<Obs, Unsure>}, StateTracker::{new(usize), observe(&mut self, &str, &Obs, &Obs, &Obs, f64) -> Obs}, GrokMetadataCache::{new(usize), read(&mut self, i64, &Path) -> Result<Obs, Unsure>}, grok_summary_public(&Value) -> Option<Obs>}`; prueba `support::python::run_python(script, args, home) -> Option<String>`.

- [ ] **Step 1: Mover `splitlines` al core**

Crear `crates/comandos-core/src/text.rs` con la función `splitlines` exacta de `crates/comandos-server/src/dash/native/py.rs:15-46` (mismo cuerpo, doc «`str.splitlines()` de Python: `\n`, `\r`, `\r\n`, U+000B, U+000C, U+001C–U+001E, U+0085, U+2028, U+2029»), añadir `pub mod text;` en `crates/comandos-core/src/lib.rs` y en `py.rs` sustituir la función por `pub use comandos_core::text::splitlines;`.

Run: `$C test -p comandos-server --test dash_native_kit` → PASS (sin cambio de comportamiento).

- [ ] **Step 2: Ayudante del oráculo**

Crear `crates/comandos-runtime/tests/support/python.rs`:

```rust
//! `python3 -c <texto> <repo> <args…>` sobre un HOME temporal; `None` sin python3.
#![allow(dead_code)]
use std::{ffi::OsStr, path::Path, process::Command};

pub fn repo() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn run_python(script: &str, args: &[&OsStr], home: &Path) -> Option<String> {
    let out = Command::new("python3")
        .arg("-c")
        .arg(script)
        .arg(repo())
        .args(args)
        .current_dir(repo())
        .env("HOME", home)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env_remove("TMUX")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .env_remove("GROK_HOME")
        .env_remove("COMANDOS_STATE_DB")
        .env_remove("COMANDOS_USAGE_DB")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output();
    match out {
        Err(_) => {
            eprintln!("python3 no está instalado: se salta la comparación con el oráculo");
            None
        }
        Ok(o) => {
            assert!(o.status.success(), "oráculo: {}", String::from_utf8_lossy(&o.stderr));
            Some(String::from_utf8(o.stdout).unwrap())
        }
    }
}
```

- [ ] **Step 3: Prueba diferencial que falla**

Crear `crates/comandos-runtime/tests/tui_state_oracle.rs`. Un único guion de casos (JSON) lo ejecutan el Python y el Rust, cada uno en su directorio de trabajo; se comparan los volcados.

```rust
//! `lib/tui_state.py` contra `comandos_runtime::tui_state` sobre los mismos casos.
mod support;
use comandos_core::json::response_dumps;
use comandos_runtime::tui_state::{
    GrokMetadataCache, StateTracker, TranscriptCache, model_id, screen_state,
};
use serde_json::{Value, json};
use std::{collections::HashMap, fs, path::Path};
use support::python::run_python;

const ORACLE: &str = r#"
import json, os, sys
repo, cases, work = sys.argv[1:4]
sys.path.insert(0, os.path.join(repo, "lib"))
import tui_state
caches, trackers, groks, out = {}, {}, {}, []
for c in json.load(open(cases)):
    op = c["op"]
    if op == "write":
        path = os.path.join(work, c["name"])
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "ab" if c.get("append") else "wb") as fh:
            fh.write(c["text"].encode())
        os.utime(path, ns=(c["mtime"], c["mtime"]))
        continue
    if op == "model_id":
        out.append(tui_state.model_id(c["label"]))
    elif op == "screen":
        out.append(tui_state.screen_state(c["harness"], c["text"]))
    elif op == "transcript":
        cache = caches.setdefault(c["cache"], tui_state.TranscriptCache(max_bytes=c.get("maxBytes", 2097152)))
        out.append(cache.read(c["harness"], c["sid"], os.path.join(work, c["name"])))
    elif op == "observe":
        tracker = trackers.setdefault(c["tracker"], tui_state.StateTracker())
        out.append(tracker.observe(c["identity"], c["launch"], c["conversation"], c["visible"], now=c["now"]))
    elif op == "grok":
        cache = groks.setdefault(c["cache"], tui_state.GrokMetadataCache())
        out.append(cache.read(c["pid"], os.path.join(work, c["home"])))
print(json.dumps(out))
"#;

fn obs(v: &Value) -> serde_json::Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

fn run_rust(cases: &[Value], work: &Path) -> String {
    let mut caches: HashMap<String, TranscriptCache> = HashMap::new();
    let mut trackers: HashMap<String, StateTracker> = HashMap::new();
    let mut groks: HashMap<String, GrokMetadataCache> = HashMap::new();
    let mut out = Vec::new();
    for c in cases {
        let s = |k: &str| c[k].as_str().unwrap().to_owned();
        match c["op"].as_str().unwrap() {
            "write" => {
                let path = work.join(s("name"));
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                let mut bytes = if c["append"].as_bool() == Some(true) {
                    fs::read(&path).unwrap_or_default()
                } else {
                    Vec::new()
                };
                bytes.extend_from_slice(s("text").as_bytes());
                fs::write(&path, bytes).unwrap();
                let mtime = std::time::UNIX_EPOCH
                    + std::time::Duration::from_nanos(c["mtime"].as_u64().unwrap());
                fs::File::options().write(true).open(&path).unwrap().set_modified(mtime).unwrap();
            }
            "model_id" => out.push(json!(model_id(&s("label")))),
            "screen" => out.push(Value::Object(screen_state(&s("harness"), &s("text")).unwrap())),
            "transcript" => {
                let cache = caches.entry(s("cache")).or_insert_with(|| {
                    TranscriptCache::new(128, c["maxBytes"].as_u64().unwrap_or(2_097_152))
                });
                out.push(Value::Object(
                    cache.read(&s("harness"), &s("sid"), &work.join(s("name"))).unwrap(),
                ));
            }
            "observe" => {
                let tracker = trackers.entry(s("tracker")).or_insert_with(|| StateTracker::new(128));
                out.push(Value::Object(tracker.observe(
                    &s("identity"),
                    &obs(&c["launch"]),
                    &obs(&c["conversation"]),
                    &obs(&c["visible"]),
                    c["now"].as_f64().unwrap(),
                )));
            }
            "grok" => {
                let cache = groks.entry(s("cache")).or_insert_with(|| GrokMetadataCache::new(128));
                out.push(Value::Object(
                    cache.read(c["pid"].as_i64().unwrap(), &work.join(s("home"))).unwrap(),
                ));
            }
            other => panic!("op {other}"),
        }
    }
    response_dumps(&Value::Array(out)).unwrap()
}

fn cases() -> Vec<Value> {
    let claude = [
        r#"{"type":"assistant","uuid":"u1","sessionId":"s1","message":{"model":"claude-opus-5"}}"#,
        r#"{"type":"assistant","uuid":"u2","sessionId":"s1","effort":"high","message":{"model":"claude-opus-5"}}"#,
        r#"{"type":"assistant","uuid":"u3","sessionId":"otra","message":{"model":"gpt-5"}}"#,
        r#"{"type":"assistant","uuid":"u4","isSidechain":true,"message":{"model":"haiku"}}"#,
        r#"{"type":"user","uuid":"u5","sessionId":"s1","message":{"content":"<local-command-stdout>Set model to Fable 5 (claude-fable-5[1m]) with max effort</local-command-stdout>"}}"#,
        r#"{"type":"assistant","uuid":"u6","sessionId":"s1","message":{"model":"<synthetic>"}}"#,
        r#"{"type":"assistant","uuid":"u7","sessionId":"s1","message":{"model":"bad model!"}}"#,
        "no es json",
        "\u{feff}{\"type\":\"assistant\",\"uuid\":\"u8\",\"sessionId\":\"s1\",\"message\":{\"model\":\"claude-sonnet-5\"},\"effort\":\"low\"}",
    ]
    .join("\n");
    let codex = [
        r#"{"type":"session_meta","payload":{"id":"c1"}}"#,
        r#"{"type":"turn_context","uuid":"t1","payload":{"model":"gpt-5.6-sol","effort":"xhigh"}}"#,
        r#"{"type":"turn_context","timestamp":"2026-10-04T12:00:00Z","payload":{"model":"gpt-5.6-luna","reasoning_effort":"low"}}"#,
    ]
    .join("\r\n");
    json!([
        {"op":"model_id","label":"Opus 4.6 (1M context)"},
        {"op":"model_id","label":"sonnet (claude-sonnet-5-20260901)"},
        {"op":"model_id","label":"Grok 4.5 Heavy"},
        {"op":"model_id","label":"nada"},
        {"op":"screen","harness":"codex","text":"x\n  gpt-5.6-sol xhigh · 40% left\n"},
        {"op":"screen","harness":"codex","text":"1. gpt-5 high · opción\n\n\n\n\n\n\n\n\n"},
        {"op":"screen","harness":"opencode","text":"┃  build · Claude Sonnet 5 · high\n╹▀▀▀\n"},
        {"op":"screen","harness":"claude","text":"❯ /model\n  ⎿  Set model to Opus 5 and high effort\n● hecho\n"},
        {"op":"screen","harness":"claude","text":"  ⎿  Set effort level to max\n\x1b[2m·\x1b[0m claude · claude-fable-5[1m]\n"},
        {"op":"screen","harness":"claude","text":"── · claude · claude-opus-5 · main\n"},
        {"op":"screen","harness":"gemini","text":"lo que sea"},
        {"op":"write","name":"claude/s1.jsonl","text":claude,"mtime":1_791_115_200_000_000_000u64},
        {"op":"transcript","cache":"a","harness":"claude","sid":"s1","name":"claude/s1.jsonl"},
        {"op":"transcript","cache":"a","harness":"claude","sid":"s1","name":"claude/s1.jsonl"},
        {"op":"write","name":"codex/c1.jsonl","text":codex,"mtime":1_791_115_201_000_000_000u64},
        {"op":"transcript","cache":"b","harness":"codex","sid":"c1","name":"codex/c1.jsonl"},
        {"op":"transcript","cache":"c","harness":"codex","sid":"c1","name":"codex/c1.jsonl","maxBytes":120},
        {"op":"write","name":"codex/c1.jsonl","text":"\n{\"type\":\"event\"}","append":true,"mtime":1_791_115_202_000_000_000u64},
        {"op":"transcript","cache":"c","harness":"codex","sid":"c1","name":"codex/c1.jsonl","maxBytes":30},
        {"op":"transcript","cache":"d","harness":"claude","sid":"s1","name":"falta.jsonl"},
        {"op":"observe","tracker":"t","identity":"p1","launch":{"model":"","effort":""},"conversation":{},"visible":{},"now":1.0},
        {"op":"observe","tracker":"t","identity":"p1","launch":{},"conversation":{"model":"claude-opus-5","revision":"u1"},"visible":{"model":"claude-opus-5","kind":"custom-status"},"now":2.0},
        {"op":"observe","tracker":"t","identity":"p1","launch":{},"conversation":{"model":"claude-opus-5","revision":"u1"},"visible":{"model":"claude-fable-5","effort":"max","kind":"confirmation"},"now":3.0},
        {"op":"observe","tracker":"t","identity":"p1","launch":{},"conversation":{"model":"claude-opus-5","revision":"u1"},"visible":{},"now":4.0},
        {"op":"observe","tracker":"t","identity":"p1","launch":{},"conversation":{"model":"claude-sonnet-5","revision":"u9"},"visible":{},"now":5.0},
        {"op":"observe","tracker":"t","identity":"p2","launch":{"model":"gpt-5","effort":"low"},"conversation":{},"visible":{"model":"gpt-5","effort":"high","kind":"status"},"now":6.0},
        {"op":"write","name":"grok/active_sessions.json","text":"[{\"pid\": 77, \"session_id\": \"g1\"}, {\"pid\": 78, \"session_id\": \"../x\"}]","mtime":1_791_115_203_000_000_000u64},
        {"op":"write","name":"grok/sessions/a/g1/summary.json","text":"{\"info\":{\"cwd\":\"/w\"},\"current_model_id\":\"grok-4.5\",\"reasoning_effort\":\"low\",\"generated_title\":\"T\",\"last_active_at\":\"2026\"}","mtime":1_791_115_203_000_000_000u64},
        {"op":"grok","cache":"g","pid":77,"home":"grok"},
        {"op":"grok","cache":"g","pid":78,"home":"grok"},
        {"op":"grok","cache":"g","pid":79,"home":"grok"}
    ])
    .as_array()
    .unwrap()
    .clone()
}

#[test]
fn tui_state_matches_python_oracle() {
    let root = std::env::temp_dir().join(format!("cmd-tui-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let (py_work, rs_work) = (root.join("py"), root.join("rs"));
    fs::create_dir_all(&py_work).unwrap();
    fs::create_dir_all(&rs_work).unwrap();
    let cases = cases();
    let file = root.join("cases.json");
    fs::write(&file, serde_json::to_string(&cases).unwrap()).unwrap();
    let Some(expected) = run_python(ORACLE, &[file.as_os_str(), py_work.as_os_str()], &root) else {
        return;
    };
    let got = run_rust(&cases, &rs_work);
    assert_eq!(got, expected.trim_end());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn transcript_surrogate_in_used_field_is_unsure() {
    let dir = std::env::temp_dir().join(format!("cmd-tui-sur-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.jsonl");
    fs::write(&path, "{\"type\":\"assistant\",\"uuid\":\"u\\ud800\",\"message\":{\"model\":\"claude-opus-5\"}}\n").unwrap();
    assert!(TranscriptCache::new(128, 2_097_152).read("claude", "s", &path).is_err());
    fs::write(&path, "{\"type\":\"assistant\",\"uuid\":\"u1\",\"message\":{\"model\":\"claude-opus-5\",\"content\":\"\\udc00\"}}\n").unwrap();
    let got = TranscriptCache::new(128, 2_097_152).read("claude", "s", &path).unwrap();
    assert_eq!(got["model"], json!("claude-opus-5"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn screen_with_unit_separator_is_unsure() {
    assert!(screen_state("codex", "gpt-5 high\u{1f}· x").is_err());
}
```

Notas: el `uuid` de cada fila evita la `revision` de respaldo (que lleva el inodo, distinto en cada directorio); la segunda prueba usa `content` dentro de un `assistant` (campo no usado en esa rama) para la vía que no es incierta.

- [ ] **Step 4: Ver el fallo**

Run: `$C test -p comandos-runtime --test tui_state_oracle`
Expected: FAIL de compilación, `unresolved import comandos_runtime::tui_state`.

- [ ] **Step 5: Dependencia, `Unsure` y el rastreador**

En `crates/comandos-runtime/Cargo.toml`, bajo `[dependencies]`: `regex = "=1.13.1"`. En `crates/comandos-runtime/src/lib.rs`: `pub mod tui_state;` y

```rust
/// El Python leería algo que este port no reproduce con certeza: quien lo
/// recibe declina (el frente reenvía al heredado).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsure;
```

En `crates/comandos-runtime/src/tui_state.rs`, el LRU acotado (128 entradas: búsqueda lineal, sin dependencia nueva) y el rastreador:

```rust
//! `lib/tui_state.py`: configuración aplicada de una TUI (pantalla, transcript,
//! metadatos de Grok) y el rastreador «la evidencia que cambió gana».
use crate::Unsure;
use comandos_core::json::{python_eq, truthy};
use serde_json::{Map, Value};
use std::path::Path;

pub type Obs = Map<String, Value>;

/// `OrderedDict` con `move_to_end` y `popitem(last=False)`.
struct Lru<V> {
    max: usize,
    items: Vec<(String, V)>,
}

impl<V> Lru<V> {
    fn new(max: usize) -> Self {
        Self { max, items: Vec::new() }
    }
    fn take(&mut self, key: &str) -> Option<V> {
        let index = self.items.iter().position(|(k, _)| k == key)?;
        Some(self.items.remove(index).1)
    }
    fn get(&self, key: &str) -> Option<&V> {
        self.items.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    /// Inserta al final (o mueve al final) y expulsa los más viejos.
    fn put(&mut self, key: String, value: V) {
        let _ = self.take(&key);
        self.items.push((key, value));
        while self.items.len() > self.max {
            self.items.remove(0);
        }
    }
}

type Fingerprint = [Option<Value>; 4];

fn fingerprint(evidence: &Obs) -> Fingerprint {
    ["model", "effort", "revision", "kind"].map(|k| evidence.get(k).cloned())
}

fn same(a: &Fingerprint, b: &Fingerprint) -> bool {
    a.iter().zip(b).all(|(x, y)| match (x, y) {
        (None, None) => true,
        (Some(x), Some(y)) => python_eq(x, y),
        // `get` devuelve None también para una clave con null.
        (None, Some(Value::Null)) | (Some(Value::Null), None) => true,
        _ => false,
    })
}

struct Entry {
    value: Obs,
    conversation: Option<Fingerprint>,
    pane: Option<Fingerprint>,
    pane_kind: Option<Value>,
}

/// `StateTracker` (151): una entrada por identidad (proceso + conversación).
pub struct StateTracker {
    entries: Lru<Entry>,
}

impl StateTracker {
    pub fn new(max_entries: usize) -> Self {
        Self { entries: Lru::new(max_entries) }
    }

    pub fn observe(
        &mut self,
        identity: &str,
        launch: &Obs,
        conversation: &Obs,
        visible: &Obs,
        now: f64,
    ) -> Obs {
        let text = |o: &Obs, k: &str| o.get(k).filter(|v| truthy(v)).cloned();
        let mut entry = self.entries.take(identity).unwrap_or_else(|| {
            let model = text(launch, "model");
            let mut value = Obs::new();
            let source = if model.is_some() { "process" } else { "unconfirmed" };
            value.insert("model".into(), model.unwrap_or_else(|| Value::String(String::new())));
            value.insert(
                "effort".into(),
                text(launch, "effort").unwrap_or_else(|| Value::String(String::new())),
            );
            value.insert("source".into(), Value::String(source.into()));
            Entry { value, conversation: None, pane: None, pane_kind: None }
        });
        for (name, evidence) in [("conversation", conversation), ("pane", visible)] {
            let print = fingerprint(evidence);
            if evidence.is_empty() {
                if name == "pane" && entry.pane_kind == Some(Value::String("confirmation".into())) {
                    // `entry.pop(name)`: suelta la huella, `paneKind` se queda.
                    entry.pane = None;
                }
                continue;
            }
            let kind = evidence.get("kind");
            let custom = kind == Some(&Value::String("custom-status".into()));
            if name == "pane" && custom {
                let source = entry.value.get("source").and_then(Value::as_str).unwrap_or("");
                if !conversation.is_empty() || matches!(source, "conversation" | "pane") {
                    continue;
                }
            }
            let slot = if name == "pane" { &mut entry.pane } else { &mut entry.conversation };
            if slot.as_ref().is_some_and(|old| same(old, &print)) {
                continue;
            }
            *slot = Some(print);
            if name == "pane" {
                entry.pane_kind = kind.cloned();
            }
            if let Some(model) = evidence.get("model").filter(|m| truthy(m)) {
                let current = entry.value.get("model").cloned().unwrap_or(Value::Null);
                if !python_eq(model, &current) {
                    entry.value.insert("effort".into(), Value::String(String::new()));
                }
            }
            for field in ["model", "effort"] {
                if let Some(v) = evidence.get(field) {
                    entry.value.insert(field.into(), v.clone());
                }
            }
            let source = if custom { "status-script" } else { name };
            entry.value.insert("source".into(), Value::String(source.into()));
            entry.value.insert("evidenceAt".into(), crate::hooks::py::float_value(now));
        }
        let out = entry.value.clone();
        self.entries.put(identity.to_owned(), entry);
        out
    }
}
```

(`serde_json::Map::insert` sobre una clave existente conserva su posición, como `dict`; `hooks::py::float_value` da el `float` de Python.)

- [ ] **Step 6: Implementar el resto del módulo**

En el mismo archivo, con las reglas de la sección «Comportamiento portado»:

- Expresiones (`std::sync::LazyLock<regex::Regex>`): `EFFORT = r"(none|minimal|low|medium|high|xhigh|max|ultra|ultracode|auto)"`, `MODEL = r"(?:gpt-[a-z0-9.\-]+|claude-[a-z0-9.\-]+|grok-[a-z0-9.\-]+|(?:opus|sonnet|haiku|fable)(?:-[a-z0-9.]+)?)(?:\[1m\])?"`, `ANSI = r"\x1b\[[0-?]*[ -/]*[@-~]"`; `re.I` → `(?i)`; `fullmatch` → `^(?:…)$` sobre la línea (sin `\n`); `re.match` → `^`. Antes de evaluar cualquier línea con `\s`: si contiene U+001F → `Err(Unsure)`.
- `model_id(label)`, `screen_state(harness, text)` como arriba.
- `TranscriptCache { lru: Lru<(Signature, Obs)>, max_bytes: u64 }`, `type Signature = (u64, u64, u64, i128)` desde `std::os::unix::fs::MetadataExt` (`dev`, `ino`, `size`, `mtime * 1e9 + mtime_nsec`); clave `format!("{harness}\u{1f}{session_id}\u{1f}{}", path.display())`; lectura con `File::seek` + `take(max_bytes)`; un `OSError` en cualquier paso → `Ok(Obs::new())`.
- Normalización de sustitutos para una línea: recorrer el texto; dentro de cadenas JSON, un escape `\uD800`–`\uDBFF` seguido de `\uDC00`–`\uDFFF` se deja; uno suelto (alto sin bajo, o bajo solo) se cambia por `\uFFFD` y la línea queda marcada «retocada». Parsear con `workspace_loads`; error de parseo con más de 128 `[`/`{` → `Err(Unsure)`, otro error → saltar la línea (`ValueError`).
- `GrokMetadataCache { entries: Lru<(Signature, Obs)>, paths: Lru<PathBuf> }` con `grok_summary_public(data) -> Option<Obs>` (`None` si `data` no es objeto: `AttributeError` → `{}`; `info` no-objeto verdadero → `None` también) que da `{"cwd","model","effort","title","contextWindow":0,"lastActiveAt"}` con `str()` de `hooks::py::str_of` y `or ""` por `truthy`. El recorrido `sessions/**/<sid>/summary.json` es recursivo sobre `read_dir` (sin seguir enlaces a directorios, como `glob(recursive=True)`), saltando nombres que empiezan por `.`.

- [ ] **Step 7: Verde**

Run: `$C test -p comandos-runtime --test tui_state_oracle` → 3 PASS.
Run: `$C clippy -p comandos-runtime --all-targets -j 6 -- -D warnings` → limpio.

- [ ] **Step 8: Commit**

```bash
git add crates/comandos-core/src/text.rs crates/comandos-core/src/lib.rs \
  crates/comandos-server/src/dash/native/py.rs crates/comandos-runtime/Cargo.toml Cargo.lock \
  crates/comandos-runtime/src/lib.rs crates/comandos-runtime/src/tui_state.rs \
  crates/comandos-runtime/tests/support/python.rs crates/comandos-runtime/tests/tui_state_oracle.rs
git commit -m "feat(runtime): tui_state — transcripts, pantalla, rastreador y metadatos de Grok portados de lib/tui_state.py

Prueba diferencial contra el Python sobre los mismos casos; sustitutos sueltos e
U+001F como incertidumbre explícita.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Procesos y paneles (`pane_snapshot`, `agent_procs`)

Port de `lib/pane_snapshot.py:PaneInspector` y de las funciones de procesos de `bin/cc-dash` que usa `read_states`, todas con `proc_root` inyectable para probarlas sobre un árbol `/proc` falso. Desplegable sola.

Comportamiento portado:

- **`PaneInspector::new`** (`lib/pane_snapshot.py:20-46`): `children[ppid]` desde `<proc>/<pid>/stat` (campo tras `)`, índice 1) en orden de `read_dir`; `claude[pid]` desde `~/.claude/sessions/*.json` y `~/.claude-accounts/*/sessions/*.json` (`{agent, resume_id, claude_config_dir}`; último gana); `grok[pid]` desde `active_sessions.json` de `~/.grok` y `~/.grok-accounts/*`; `acp` = `~/.claude/hooks/acp-panes.json` (`{}` si falla). La clave de pid es la de un `dict` de Python: entero, flotante integral o booleano (`True == 1`) cuentan como el entero; cadenas nunca casan.
- **`flags`** (48-79), **`native_metadata`** (81-92), **`inspect`** = `__call__` (94-187): shells → `{"agent": ""}`; `cc-acp` con descendientes; BFS por `children` desde `pane.pid`, primer ejecutable conocido (`claude`, `grok*`, `opencode`/`agy` con `--session|-s|--conversation`, fds de `antigravity-cli`, `codex` con rollouts raíz por `session_meta` no delegado y `mtime_ns`, más de una raíz → `{"agent":"codex"}`, `resume <uuid>`, `CODEX_HOME` de `environ`).
- **`agent_procs`** (6110): `<proc>/[0-9]*/cmdline` en orden de `read_dir`; `argv[:3]` de `split(b"\0")` y **luego** se filtran vacíos; basename con `decode("utf-8","replace")`; primer alias que casa; `readlink(<pid>/cwd)` (fallo → se salta); `cwd` no UTF-8 → `Err(Unsure)`. Solo Linux (sin rama `ps`/`lsof`).
- **`parent_pid`** (6162): campo 1 tras `)`; fallo → 0 (la rama `ps` del Python también da 0 para un pid que ya no existe).
- **`proc_cmdline`** (526), **`read_environ`** (3866, `dict`: último gana), **`process_start`** (3916: campo 19 tras `)` o `""`).
- **`parse_pane_inventory`** (6176): `split('|', 7)`, relleno a 8, `pane_pid` todo dígitos ASCII, `SESSION_RE`/`PANE_RE`, `float(activity or 0)` con `py::float` (error → 0), `paneActive = pane_active == "1" and window_active == "1"`. Formato `PANE_FORMAT = "#{session_name}|#{pane_id}|#{pane_pid}|#{pane_current_command}|#{pane_current_path}|#{session_activity}|#{pane_active}|#{window_active}"`.
- **`process_owners`** (6202), **`agent_pane_maps`** (6221: shells `zsh bash sh fish dash ksh ssh`; candidatos por pane en orden de procesos; `min((depth, -pid))`; envoltorio `node|nodejs|python|python3|bun` → hijo directo nativo del mismo agente con menor pid), **`external_agents`** (el bucle de 7286-7298: procesos sin dueño cuyo ancestro llega a ≤1 sin pasar por otro agente).
- **`AccountCache::account_for_pid`** (3924) + `account_email_for_dir` (3898): `AGENT_ACCOUNT_ENV` (`claude`→`CLAUDE_CONFIG_DIR`/`~/.claude`, `codex`→`CODEX_HOME`/`~/.codex`, `grok`→`GROK_HOME`/`~/.grok`); caché `(pid, inicio, agente)` que se vacía al pasar de 4096; alias `main` si `realpath` coincide, si no el basename; correo de `.claude.json` (o el del padre), `auth.json` (`tokens.id_token` JWT en base64url → `email`) o el primer registro objeto de Grok (`email`/`user_id`/`principal_id`); caché por firma `(dev, ino, mtime_ns, size)`. Resultado `{"account", "accountEmail"}`; agente sin especificación o pid 0 → `{}`.

**Files:**
- Create: `crates/comandos-runtime/src/pane_snapshot.rs`, `crates/comandos-runtime/src/agent_procs.rs`
- Modify: `crates/comandos-runtime/src/lib.rs` (`pub mod pane_snapshot; pub mod agent_procs;`)
- Create: `crates/comandos-runtime/tests/pane_snapshot_oracle.rs`

**Interfaces:**
- Consumes: `tui_state::Obs`, `Unsure`, `hooks::py::{eq, str_of, truthy}`.
- Produces: `pane_snapshot::{PaneRef<'a> { id: &'a str, pid: i64, command: &'a str }, PaneInspector::{new(&Path, &Path) -> Self, inspect(&self, &PaneRef<'_>) -> Obs, native_metadata(&self, i64, &str) -> Obs, flags(&self, i64) -> Vec<String>}}`; `agent_procs::{AgentProc { pid: i64, cwd: String, agent: String }, agent_procs(&Path, &HashMap<String, String>) -> Result<Vec<AgentProc>, Unsure>, parent_pid(&Path, i64) -> i64, proc_cmdline(&Path, i64) -> Vec<String>, read_environ(&Path, i64) -> HashMap<Vec<u8>, Vec<u8>>, process_start(&Path, i64) -> String, PaneRow { session, pane, pane_pid: i64, command, cwd, activity: f64, pane_active: bool }, PANE_FORMAT, parse_pane_inventory(&str) -> Vec<PaneRow>, process_owners(&[AgentProc], &[PaneRow], &mut dyn FnMut(i64) -> i64) -> HashMap<i64, (usize, usize)>, AgentInfo { session, pane, cwd, agent, pid: i64 }, AgentMaps { by_session: Vec<(String, AgentInfo)>, by_cwd: Vec<(String, Vec<AgentInfo>)> }, agent_pane_maps(&[AgentProc], &[PaneRow], &HashMap<i64, (usize, usize)>, &mut dyn FnMut(i64) -> Vec<String>, &mut dyn FnMut(i64) -> i64) -> AgentMaps, external_agents(&[AgentProc], &HashMap<i64, (usize, usize)>, &mut dyn FnMut(i64) -> i64) -> HashSet<(String, String)>, AccountCache::{default, account_for_pid(&mut self, &Path, &Path, i64, &str) -> Obs}}`. `AgentInfo::to_obs(&self) -> Obs` da `{session, pane, cwd, agent, pid}` en ese orden.

- [ ] **Step 1: Prueba diferencial que falla**

Crear `crates/comandos-runtime/tests/pane_snapshot_oracle.rs`. Un árbol `/proc` falso y un HOME con sesiones; el mismo árbol lo leen el Python y el Rust (solo lectura).

```rust
//! `PaneInspector` y `agent_pane_maps` contra el Python sobre un /proc falso.
mod support;
use comandos_core::json::response_dumps;
use comandos_runtime::{
    agent_procs::{AgentProc, PaneRow, agent_pane_maps, process_owners},
    pane_snapshot::{PaneInspector, PaneRef},
};
use serde_json::{Value, json};
use std::{collections::HashMap, fs, os::unix::fs::symlink, path::Path};
use support::python::run_python;

const INSPECTOR: &str = r#"
import json, os, sys
repo, home, proc, panes = sys.argv[1:5]
sys.path.insert(0, os.path.join(repo, "lib"))
import pane_snapshot
inspector = pane_snapshot.PaneInspector(home=home, proc_root=proc)
print(json.dumps([inspector(p) for p in json.load(open(panes))]))
"#;

const MAPS: &str = r#"
import importlib.machinery, importlib.util, json, os, sys
repo, case = sys.argv[1:3]
sys.path.insert(0, os.path.join(repo, "bin"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
c = json.load(open(case))
dash.parent_pid = lambda pid: c["parents"].get(str(pid), 0)
dash._proc_cmdline = lambda pid: c["cmdlines"].get(str(pid), [])
by_session, by_cwd = dash.agent_pane_maps([tuple(p) for p in c["procs"]], c["panes"])
print(json.dumps({"bySession": by_session, "byCwd": by_cwd}))
"#;

fn process(proc: &Path, pid: i64, ppid: i64, argv: &[&str], environ: &[&str]) {
    let dir = proc.join(pid.to_string());
    fs::create_dir_all(dir.join("fd")).unwrap();
    fs::write(dir.join("stat"), format!("{pid} (x y) S {ppid} 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 4242 0")).unwrap();
    fs::write(dir.join("cmdline"), argv.join("\0") + "\0").unwrap();
    fs::write(dir.join("environ"), environ.join("\0") + "\0").unwrap();
}

#[test]
fn inspector_matches_python_oracle() {
    let root = std::env::temp_dir().join(format!("cmd-insp-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let (home, proc) = (root.join("home"), root.join("proc"));
    fs::create_dir_all(home.join(".claude/sessions")).unwrap();
    fs::create_dir_all(home.join(".claude-accounts/work/sessions")).unwrap();
    fs::create_dir_all(home.join(".claude/hooks/native-processes")).unwrap();
    fs::create_dir_all(home.join(".codex/sessions")).unwrap();
    fs::write(home.join(".claude/sessions/a.json"), r#"{"pid": 101, "sessionId": "s-main"}"#).unwrap();
    fs::write(home.join(".claude-accounts/work/sessions/b.json"), r#"{"pid": 102.0, "sessionId": "s-work"}"#).unwrap();
    fs::write(home.join(".claude/hooks/acp-panes.json"), r#"{"%9": {"pid": 401, "agent": "codex", "model": "gpt-5", "sessionId": "x"}}"#).unwrap();
    // claude con sesión, claude de otra cuenta, shell, codex con rollout raíz y uno delegado, cc-acp.
    process(&proc, 100, 1, &["zsh"], &[]);
    process(&proc, 101, 100, &["/usr/bin/claude", "--model", "opus", "--dangerously-skip-permissions"], &[]);
    process(&proc, 102, 1, &["claude", "-m", "--effort"], &[]);
    process(&proc, 200, 1, &["node", "/x/codex.js"], &[]);
    process(&proc, 201, 200, &["codex", "--yolo", "-c", "model_reasoning_effort=high"], &["CODEX_HOME=/c"]);
    let root_rollout = home.join(".codex/sessions/rollout-2026-0f0f0f0f-0000-4000-8000-000000000001.jsonl");
    fs::write(&root_rollout, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"0f0f0f0f-0000-4000-8000-000000000001\",\"source\":\"cli\"}}\n").unwrap();
    let sub = home.join(".codex/sessions/rollout-2026-0f0f0f0f-0000-4000-8000-000000000002.jsonl");
    fs::write(&sub, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"0f0f0f0f-0000-4000-8000-000000000002\",\"source\":{\"subagent\":{}}}}\n").unwrap();
    symlink(&root_rollout, proc.join("201/fd/7")).unwrap();
    symlink(&sub, proc.join("201/fd/8")).unwrap();
    process(&proc, 400, 1, &["cc-acp"], &[]);
    process(&proc, 401, 400, &["codex-acp"], &[]);
    let panes = json!([
        {"id":"%1","pid":100,"command":"node"},
        {"id":"%2","pid":102,"command":"claude"},
        {"id":"%3","pid":100,"command":"zsh"},
        {"id":"%4","pid":200,"command":"node"},
        {"id":"%9","pid":400,"command":"cc-acp"},
        {"id":"%8","pid":999,"command":"vim"}
    ]);
    let file = root.join("panes.json");
    fs::write(&file, panes.to_string()).unwrap();
    let Some(expected) = run_python(INSPECTOR, &[home.as_os_str(), proc.as_os_str(), file.as_os_str()], &home) else {
        return;
    };
    let inspector = PaneInspector::new(&home, &proc);
    let got: Vec<Value> = panes
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            Value::Object(inspector.inspect(&PaneRef {
                id: p["id"].as_str().unwrap(),
                pid: p["pid"].as_i64().unwrap(),
                command: p["command"].as_str().unwrap(),
            }))
        })
        .collect();
    assert_eq!(response_dumps(&Value::Array(got)).unwrap(), expected.trim_end());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn agent_pane_maps_match_python_oracle() {
    let root = std::env::temp_dir().join(format!("cmd-maps-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    // Copia del caso de tests/test_live_pane_inventory.py, con un envoltorio node → codex.
    let case = json!({
        "panes": [
            {"session":"mixed","pane":"%1","pane_pid":100,"command":"node","cwd":"/same","activity":50.0,"paneActive":true},
            {"session":"mixed","pane":"%2","pane_pid":200,"command":"claude","cwd":"/same","activity":50.0,"paneActive":false},
            {"session":"other","pane":"%5","pane_pid":500,"command":"node","cwd":"/same","activity":40.0,"paneActive":false},
            {"session":"terminal","pane":"%6","pane_pid":600,"command":"zsh","cwd":"/shell","activity":30.0,"paneActive":false}
        ],
        "procs": [[101,"/same","codex"],[102,"/same","codex"],[201,"/same","claude"],[501,"/same","codex"],[601,"/shell","claude"]],
        "parents": {"101":100,"102":101,"201":200,"501":500,"601":600},
        "cmdlines": {"101":["node","/x/codex.js"],"102":["codex"],"201":["claude"],"501":["codex"]}
    });
    let file = root.join("case.json");
    fs::write(&file, case.to_string()).unwrap();
    let Some(expected) = run_python(MAPS, &[file.as_os_str()], &root) else {
        return;
    };
    let panes: Vec<PaneRow> = case["panes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| PaneRow {
            session: p["session"].as_str().unwrap().into(),
            pane: p["pane"].as_str().unwrap().into(),
            pane_pid: p["pane_pid"].as_i64().unwrap(),
            command: p["command"].as_str().unwrap().into(),
            cwd: p["cwd"].as_str().unwrap().into(),
            activity: p["activity"].as_f64().unwrap(),
            pane_active: p["paneActive"].as_bool().unwrap(),
        })
        .collect();
    let procs: Vec<AgentProc> = case["procs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| AgentProc {
            pid: p[0].as_i64().unwrap(),
            cwd: p[1].as_str().unwrap().into(),
            agent: p[2].as_str().unwrap().into(),
        })
        .collect();
    let parents: HashMap<i64, i64> = case["parents"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.parse().unwrap(), v.as_i64().unwrap()))
        .collect();
    let cmdlines = case["cmdlines"].clone();
    let mut parent = |pid: i64| parents.get(&pid).copied().unwrap_or(0);
    let owners = process_owners(&procs, &panes, &mut parent);
    let mut cmdline = |pid: i64| -> Vec<String> {
        serde_json::from_value(cmdlines[pid.to_string()].clone()).unwrap_or_default()
    };
    let maps = agent_pane_maps(&procs, &panes, &owners, &mut cmdline, &mut parent);
    let object = |rows: Vec<(String, Value)>| Value::Object(rows.into_iter().collect());
    let got = json!({
        "bySession": object(maps.by_session.iter().map(|(k, i)| (k.clone(), Value::Object(i.to_obs()))).collect()),
        "byCwd": object(maps.by_cwd.iter().map(|(k, rows)| (k.clone(), Value::Array(rows.iter().map(|i| Value::Object(i.to_obs())).collect()))).collect()),
    });
    assert_eq!(response_dumps(&got).unwrap(), expected.trim_end());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn inventory_parses_like_python() {
    use comandos_runtime::agent_procs::parse_pane_inventory;
    let rows = parse_pane_inventory("s|%1|12|zsh|/a|1791115200|1|1\nbad|%x|1|a|b|c|d|e\ns|%2|7|claude|/a|nan?|0|1\ns|%3|8\n");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].activity, 1_791_115_200.0);
    assert!(rows[0].pane_active);
    assert_eq!(rows[1].activity, 0.0);
    assert_eq!((rows[2].command.as_str(), rows[2].cwd.as_str()), ("", ""));
}
```

- [ ] **Step 2: Ver el fallo**

Run: `$C test -p comandos-runtime --test pane_snapshot_oracle`
Expected: FAIL de compilación, `unresolved import comandos_runtime::agent_procs`.

- [ ] **Step 3: Implementar**

`crates/comandos-runtime/src/agent_procs.rs` y `pane_snapshot.rs` con las reglas de arriba. Puntos que la prueba no fuerza y deben respetarse:

- Nada de `ps`/`lsof`: el frente solo corre en Linux.
- `read_dir` sin ordenar; nombres de `/proc` que no son todo dígitos se saltan.
- `AgentInfo` conserva `cwd = proc.cwd or row.cwd or ''`.
- `AccountCache` usa `std::fs::canonicalize` como `realpath` (si falla, el texto tal cual, como `realpath` de un camino inexistente: se compara el resultado de `canonicalize` del padre más el nombre; si tampoco existe, el texto normalizado con `Path::components`).
- Decodificación JWT: `base64::engine::general_purpose::URL_SAFE` con el relleno `=` que añade el Python; cualquier fallo → `""`.

- [ ] **Step 4: Verde**

Run: `$C test -p comandos-runtime --test pane_snapshot_oracle` → 3 PASS. Run: `$C test -p comandos-runtime` → PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/comandos-runtime/src/lib.rs crates/comandos-runtime/src/pane_snapshot.rs \
  crates/comandos-runtime/src/agent_procs.rs crates/comandos-runtime/tests/pane_snapshot_oracle.rs
git commit -m "feat(runtime): PaneInspector y procesos de agentes con /proc inyectable

agent_procs, dueños por pane, mapas por sesión y cwd, agentes externos y cuentas
por pid; prueba diferencial contra lib/pane_snapshot.py y bin/cc-dash.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Registro de proveedores, tiers y rutas seleccionables (`providers`)

Port de lo que `read_states` y `write_app_tab_models` necesitan de `lib/providers.py` y `bin/cc-dash`. Desplegable sola.

Comportamiento portado:

- **`py_regex(pattern, ignore_case)`**: D10. **`py_search(&Regex, subject) -> Result<bool, Unsure>`**: `\n` o no ASCII en el sujeto → `Unsure`.
- **`validate_registry`** (`lib/providers.py:70-138`): las mismas comprobaciones y mensajes (`ProviderRegistryError`); `re.compile` de `modelMatch` vía `py_regex` (incierto → `Err(Unsure)`). `load_registry(providers_json, catalog)` = leer con `files`-equivalente (texto UTF-8 estricto; fallo → `Err(Unsure)`), `hydrate_registry` (`model_catalog`, ya portado), validar; inválido → `Err(Unsure)` (D2). `RegistryCache` por `(mtime_ns de providers.json, catalog_signature)` como `load_provider_registry` (1415).
- **`engine_for_model`** (157): `motors` o `claudeEngines`, primer `re.search(modelMatch or r"$^", model, re.I)`.
- **`read_conf`** (4937): `strip`, comentario, `partition("=")`, comillas simétricas; ausente → vacío; otro error u UTF-8 inválido → `Unsure`. **`agent_set`** (6083): `AGENTS` o `DEFAULT_AGENTS = "claude codex grok acp opencode gemini agy aider"` partido por espacios + harnesses del registro salvo `shell`. **`process_aliases`** (6092): `{a: a}` + `processNames` no genéricos (`node python python3 bash zsh fish sh`) por basename, en orden del registro (último gana).
- **`which`** (`lib/providers.py:44`): `PATH` (entradas vacías = directorio actual, como `shutil.which`) y luego `~/.local/bin ~/.bun/bin ~/.cargo/bin ~/.npm-global/bin ~/.opencode/bin ~/bin /usr/local/bin`; archivo regular ejecutable.
- **`runtime_facts`** (1610) y **`selectable_routes`** = ids de `evaluate_capability_matrix` (`lib/providers.py:219`) con `selectable`: rutas del registro con sus cinco razones en orden; exclusiones y celdas sin ruta nunca son seleccionables. `public_state` (287) solo aporta `available` (binario nulo o `which`) y `authenticated` (con `authFile`: `defaultHome` expandido no vacío y archivo presente; sin `authFile`: `None`).
- **`model_tier`** (3848) / **`tier_symbol`** (3861): `patterns` en orden, `re.search(match, model, re.I)` (error de regex del Python → se salta; patrón incierto → `Unsure`; elemento no-objeto → `Unsure`), si no `defaultTier`; símbolo `tiers[tier].symbol` crudo o `""`.
- **`proxy_port(repo_root)`** (`load_proxy_cfg` 1747 → `int(port or 18765)`; archivo ilegible → 18765; `int()` imposible → `Unsure`).

**Files:**
- Create: `crates/comandos-runtime/src/providers.rs`; Modify: `crates/comandos-runtime/src/lib.rs`
- Create: `crates/comandos-runtime/tests/providers_oracle.rs`

**Interfaces:**
- Consumes: `model_catalog::{Paths, catalog_signature, hydrate_registry}`, `Unsure`, `hooks::py::{eq, truthy}`.
- Produces: `providers::{py_regex(&str, bool) -> Result<regex::Regex, Unsure>, py_search(&regex::Regex, &str) -> Result<bool, Unsure>, validate_registry(&Value) -> Result<Result<(), String>, Unsure>, RegistryCache::{default, load(&mut self, &Path, &model_catalog::Paths) -> Result<Value, Unsure>}, engine_for_model(&Value, &str) -> Result<String, Unsure>, harness_has_accounts(&Value, &str) -> bool, read_conf(&Path) -> Result<Vec<(String, String)>, Unsure>, agent_set(Option<&str>, &Value) -> BTreeSet<String>, process_aliases(&BTreeSet<String>, &Value) -> HashMap<String, String>, which(&str, Option<&OsStr>, &Path) -> Option<PathBuf>, runtime_facts(&Value, &Value, &dyn Fn(&str) -> bool, &Path, bool, bool) -> Value, selectable_routes(&Value, &Value) -> BTreeSet<String>, model_tier(&Value, &str) -> Result<String, Unsure>, tier_symbol(&Value, &str) -> Value, proxy_port(&Path) -> Result<u16, Unsure>}`. Argumentos de `runtime_facts`: `(registry, discovered = accounts::public_accounts(...), available(binario), home, gateway_installed, gateway_alive)`.

- [ ] **Step 1: Prueba que falla**

Crear `crates/comandos-runtime/tests/providers_oracle.rs`:

```rust
//! Registro real de config/providers.json contra lib/providers.py y bin/cc-dash.
mod support;
use comandos_core::json::response_dumps;
use comandos_runtime::providers::{
    engine_for_model, model_tier, py_regex, py_search, runtime_facts, selectable_routes,
    validate_registry,
};
use serde_json::{Value, json};
use std::fs;
use support::python::{repo, run_python};

const ORACLE: &str = r#"
import importlib.machinery, importlib.util, json, os, sys
repo, case = sys.argv[1:3]
sys.path.insert(0, os.path.join(repo, "bin"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
c = json.load(open(case))
reg = dash.provider_registry.load_registry(os.path.join(repo, "config/providers.json"))
dash.load_provider_registry = lambda: reg
dash.provider_registry.which = lambda b: "/bin/" + b if b in c["available"] else None
dash.account_registry.public_accounts = lambda r: c["discovered"]
dash.proxy_alive = lambda: c["alive"]
dash.shutil.which = lambda n: None
facts = dash.provider_runtime_facts()
routes = sorted(x["id"] for x in dash.provider_registry.evaluate_capability_matrix(reg, facts) if x.get("selectable"))
engines = [dash.provider_registry.engine_for_model(reg, m) for m in c["models"]]
tiers = json.load(open(os.path.join(repo, "config/model-tiers.json")))
dash.load_model_tiers = lambda: tiers
print(json.dumps({"facts": facts, "routes": routes, "engines": engines,
                  "tiers": [dash.model_tier(m) for m in c["models"]]}))
"#;

#[test]
fn providers_match_python_oracle() {
    let home = std::env::temp_dir().join(format!("cmd-prov-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).unwrap();
    let case = json!({
        "available": ["claude", "codex"],
        "discovered": {"claude": [{"alias": "main", "selectable": true}], "codex": [{"alias": "main", "selectable": false}], "grok": []},
        "alive": false,
        "models": ["claude-fable-5[1m]", "gpt-5.6-sol", "grok-4.5", "opencode/x", "gemini-3", "fable", "nada"]
    });
    let file = home.join("case.json");
    fs::write(&file, case.to_string()).unwrap();
    let Some(expected) = run_python(ORACLE, &[file.as_os_str()], &home) else {
        return;
    };
    let registry: Value =
        serde_json::from_str(&fs::read_to_string(repo().join("config/providers.json")).unwrap()).unwrap();
    assert_eq!(validate_registry(&registry), Ok(Ok(())));
    let tiers: Value =
        serde_json::from_str(&fs::read_to_string(repo().join("config/model-tiers.json")).unwrap()).unwrap();
    let available = |b: &str| matches!(b, "claude" | "codex");
    let facts = runtime_facts(&registry, &case["discovered"], &available, &home, false, false);
    let models: Vec<&str> = case["models"].as_array().unwrap().iter().map(|m| m.as_str().unwrap()).collect();
    let got = json!({
        "facts": facts,
        "routes": selectable_routes(&registry, &facts),
        "engines": models.iter().map(|m| engine_for_model(&registry, m).unwrap()).collect::<Vec<_>>(),
        "tiers": models.iter().map(|m| model_tier(&tiers, m).unwrap()).collect::<Vec<_>>(),
    });
    assert_eq!(response_dumps(&got).unwrap(), expected.trim_end());
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn python_only_regex_is_unsure() {
    assert!(py_regex(r"(?<=a)b", true).is_err());
    assert!(py_regex(r"(a)\1", false).is_err());
    let re = py_regex(r"^opus$", true).unwrap();
    assert_eq!(py_search(&re, "OPUS"), Ok(true));
    assert!(py_search(&re, "opus\n").is_err());
    assert!(py_search(&re, "ópus").is_err());
    assert_eq!(py_search(&py_regex(r"x\Z", false).unwrap(), "ax"), Ok(true));
}

#[test]
fn invalid_registry_reports_python_message() {
    assert_eq!(
        validate_registry(&json!({"version": 3})),
        Ok(Err("providers.json version must be 1 or 2".into()))
    );
}
```

- [ ] **Step 2: Ver el fallo**

Run: `$C test -p comandos-runtime --test providers_oracle`
Expected: FAIL de compilación (`providers` no existe).

- [ ] **Step 3: Implementar** `crates/comandos-runtime/src/providers.rs` con las reglas de arriba; `pub mod providers;` en `lib.rs`. Los mensajes de `validate_registry` usan `repr` de Python para `{ident!r}` (`hooks::py::repr` sobre `Value::String`), y `sorted(...)` de celdas en «matrix coverage mismatch» se forma ordenando pares `(harness, motor)` como tuplas de cadenas.

- [ ] **Step 4: Verde**

Run: `$C test -p comandos-runtime --test providers_oracle` → 3 PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/comandos-runtime/src/lib.rs crates/comandos-runtime/src/providers.rs \
  crates/comandos-runtime/tests/providers_oracle.rs
git commit -m "feat(runtime): registro de proveedores, tiers y rutas seleccionables

validate_registry, engine_for_model, agent_set/aliases, which con los bin de usuario,
hechos de ejecución y rutas seleccionables; re de Python traducido con incertidumbre
explícita.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Ensamblado de tarjetas (`native/states/{records,cards,observe,suggest,tab_models}`)

El bucle de `read_states` y lo que lo rodea, portado sobre un rasgo de efectos. Desplegable sola: ninguna ruta lo usa aún; la prueba diferencial carga `bin/cc-dash` con sus funciones de efecto sustituidas por las del caso y compara la lista y `app-tab-models.json` byte a byte.

Comportamiento portado (líneas de `bin/cc-dash`):

- **`RecordCache::scan`** (7175-7184, D5): nombres `*.json` que no empiezan por `.`; `fs::metadata` (sigue enlaces; no-archivo → se salta); leer; UTF-8 inválido → se salta; `loads_strict`: `Unreadable` → se salta, `Unsure` → `Err(StateFault::Decline)`; no-objeto → se salta; `float(record.get('ts') or 0)` con `py::float` / número / booleano: error → se salta, no finito u overflow → `Decline`. Directorio ausente → vacío.
- **Anotación** (7185-7216): `explicit = str(record.get('session') or '')` (contenedor verdadero → texto no vacío e inválido); `session = explicit` si `py::is_session`, si no `session_name(record.get('project', ''))` (`re.sub(r"[.:]", "-", x)[:80]`; `project` presente no-cadena → `Failure`, el `TypeError`); `pane = str(record.get('pane') or '')`; `reported = record.get('agent') or 'claude'` (valor crudo, comparado con `python_eq`); pane exacto → `agents[(session, pane)]` con agente igual; si no, candidatos de la sesión con ese agente y, sin candidatos ni sesión explícita, por `cwd` (`record.get('cwd') or ''`: contenedor → `Failure`, `unhashable`); un solo candidato. Prioridad `(exacto, timestamp)` mayor gana. Sin coincidencia: `session not in live or status == 'waiting'` → desacoplado.
- **Tarjetas vivas** (7218-7258), en el orden de `panes`, con el rasgo `CardEffects` para cada efecto: oculta (`hub`, `control`; `local` nunca) sin agente → fuera; `base = '⌂ local' | labels[s] | record.project | basename(cwd.rstrip('/')) | session` (por `truthy`); split → `f'{base} ⫽{pane[1:]}'` (`str()` de escalar con `hooks::py::str_of`; contenedor → `Decline`); `status` (`dead` → `idle`); `pane_status_hint` (solo codex: `capture-pane -p -t <pane> -S -12`, plazo 2 s, `\bWorking\s*\(`); sin anotación y claude → `claude_pane_busy` (`capture-pane -p -t <pane> -S -14`, 5 s, las tres reglas de 3575); el diccionario con sus 16 claves en orden; con `info.pid`: `effects.account` → `update`, grok → `grokTitle`, `effects.observe` + `effects.session_config` → `reconcile` (6997-7015); sin pid: las 7 claves por defecto; `ssh-*` → `ssh_state` (`list-panes -s -t =<s> -F #{pane_current_command}` contiene `ssh` → `ssh`; si no, `SSH_HOST_RE` y `effects.ssh_check(host)` → `mux`; si no `""`) → `sshConnected`, `sshMux`.
- **Históricas** (7260-7318): `external_agents` (de la Tarea 2, calculado en el escaneo); `recent_done`, zombi (`ZOMBIE_WAITING_GRACE_SECS = 86400`, `DONE_GRACE_SECS = 600`), `dead`; clave `(session, str(record.get('pane') or ''))` (contenedor → `Decline`); se conserva la de `ts` mayor (`>=` gana la anterior); 15 claves en orden.
- **Observación** (6917-6996, sin efectos: la evidencia llega hecha): claves y orden del resultado; `harnessAccount` (`account` o `unknown`, `main` si el arnés no tiene cuentas); `launch` desde `cmdline` (`--model|-m`, `--effort`, `-c|--config` con `model_reasoning_effort=…`, valores que empiezan por `-` se saltan); acp sustituye `launch` y añade `motor`, `motorAccount`, `harnessAccount='main'`, `effortSource`, `requestedModel`, `requestedEffort`; `tracker.observe` con la clave `identity\x1fagent\x1fpid\x1fstart\x1fconversationId`; `motor` (`opencode|agy|gemini` → el arnés; si no `engine_for_model(model)` o el arnés); `motorAccount`; claude con motor codex/grok y `ANTHROPIC_BASE_URL` local → `main`; `confirmed`, `accountSource`, `limitations`.
- **`reconcile`** (6997): con `Observed::Unconfirmed` → `{"confirmed": false, "model": "", "effort": "", "source": "unconfirmed"}`; `lastConfirmedConfig = config or None`; las 11 actualizaciones en orden; `routeId = f'{agent or ""}:{motor}'`.
- **Sugerencias** (7058-7156): `needs_context(items, motor)` = alguna viva con agente `claude|codex|grok`, sin cambio pendiente, y `status == working` o modelo que casa `fable|opus|-sol` (`re.I`). `annotate_all`: el bucle de `_annotate_suggestion` sobre los ítems en su orden de construcción; una excepción del Python (operación sobre un tipo inesperado, `float()`/`int()` imposibles, `.get` sobre no-dict) **detiene** la anotación de los restantes sin error (el `try/except Exception: pass` de 7319); `round` del Python = `f64::round_ties_even`; textos idénticos (copiados de las f-strings).
- **Orden** (7322): estable por `(not alive, 0 if agent else 1, -float(ts or 0))`.
- **`tab_models`** (6838-6886): agrupado por la primera etiqueta verdadera de cada sesión (el `setdefault` solo corre con etiqueta), `short` (`\[.*$`, `claude-` fuera, `-(?:202[0-9]{5}|5)$`), `detectando…`, `motor.capitalize()` (no ASCII → `Decline`), detalle por pane con `MOTOR_RESULT[s|pane]` (cambio en curso < 300 s), `tierSym`, `labels[:2]`, `panes[:6]`, `mixed`.

**Files:**
- Create: `crates/comandos-server/src/dash/native/states/mod.rs`, `records.rs`, `cards.rs`, `observe.rs`, `suggest.rs`, `tab_models.rs`
- Modify: `crates/comandos-server/src/dash/native/mod.rs` (`pub mod states;`, `From<StateFault> for Fault`)
- Modify: `crates/comandos-server/tests/support/oracle.rs` (`run_python`)
- Create: `crates/comandos-server/tests/dash_native_state_cards.rs`

**Interfaces:**
- Consumes: Tareas 1–3; `native::{Fault, py::{is_session, is_pane, float}, files::{loads_strict, Strict}, tmux::{Output, TmuxError}}`; `comandos_core::json::{python_eq, truthy, response_dumps}`; `hooks::py::{str_of, float_value}`.
- Produces: `states::StateFault { Decline, Failure, Timeout }` (+ `From<StateFault> for Fault`: `Decline` → `Fault::Decline`, `Failure` → `HandlerError::Failure`, `Timeout` → `HandlerError::Timeout`; `StateFault::from_tmux(&TmuxError)` = `uncaught()`); `records::{Record { value: Map<String, Value>, timestamp: f64 }, RecordCache::{default, scan(&mut self, &Path) -> Result<Vec<Record>, StateFault>}}`; `cards::{Inputs { now: f64, panes: Vec<PaneRow>, maps: AgentMaps, external: HashSet<(String, String)>, labels: HashMap<String, String>, records: Vec<Record> }, CardEffects (rasgo, abajo), build<E: CardEffects>(&Inputs, &E) -> impl Future<Output = Result<Vec<Value>, StateFault>>, sort_items(&mut [Value]) -> Result<(), StateFault>}`; `observe::{Observed { Seen(Obs), Unconfirmed }, ObserveEvidence { … }, observe(&ObserveEvidence, &mut StateTracker, &Value, f64) -> Result<Obs, StateFault>, reconcile(&mut Map<String, Value>, &Observed, Option<Obs>)}`; `suggest::{SuggestContext { guard: Value, routes: BTreeSet<String>, latency: Vec<((Value, Value), (Value, Value))> }, needs_context(&[Value], &Map<String, Value>) -> Result<bool, StateFault>, annotate_all(&mut [Value], &SuggestContext, &Map<String, Value>, f64) -> Result<(), StateFault>, latency_from(&Value) -> Vec<((Value, Value), (Value, Value))>}`; `tab_models::tab_models(&[Value], &Value, &Map<String, Value>, &Value, f64) -> Result<Value, StateFault>`; prueba `support::oracle::run_python`.

- [ ] **Step 1: Esqueleto con los tipos y el rasgo**

`crates/comandos-server/src/dash/native/states/mod.rs`:

```rust
//! GET `/state` (`read_states` 7159, `read_states_cached` 7315).
//! Tarea 4: ensamblado puro sobre `CardEffects`; Tarea 5: ruta, caché, recolección.
pub mod cards;
pub mod observe;
pub mod records;
pub mod suggest;
pub mod tab_models;

use super::{Fault, tmux::TmuxError};
use crate::HandlerError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateFault {
    /// Incierto (D2): el frente reenvía; nada se escribió.
    Decline,
    /// Excepción no capturada del Python: 500.
    Failure,
    /// `subprocess.TimeoutExpired` no capturado: 504.
    Timeout,
}

impl StateFault {
    pub fn from_tmux(error: &TmuxError) -> Self {
        match error.uncaught() {
            HandlerError::Timeout => StateFault::Timeout,
            _ => StateFault::Failure,
        }
    }
}

impl From<comandos_runtime::Unsure> for StateFault {
    fn from(_: comandos_runtime::Unsure) -> Self {
        StateFault::Decline
    }
}

impl From<StateFault> for Fault {
    fn from(fault: StateFault) -> Self {
        match fault {
            StateFault::Decline => Fault::Decline,
            StateFault::Failure => Fault::Error(HandlerError::Failure),
            StateFault::Timeout => Fault::Error(HandlerError::Timeout),
        }
    }
}
```

(Si `HandlerError` no tiene `Timeout` con ese nombre, usar la variante que `TmuxError::uncaught` ya devuelve para el plazo.) En `crates/comandos-server/src/dash/native/mod.rs`: `pub mod states;`.

En `cards.rs`, el rasgo (async en rasgo nativo: genérico, sin `dyn`):

```rust
use comandos_runtime::agent_procs::AgentInfo;
use super::{StateFault, observe::Observed};
use crate::dash::native::tmux::{Output, TmuxError};
use serde_json::{Map, Value};
use std::time::Duration;

/// Cada efecto que el bucle de `read_states` hace por pane, en su orden.
pub trait CardEffects {
    /// `tmux(*args, timeout=…)`.
    fn tmux(&self, args: &[&str], timeout: Duration)
        -> impl Future<Output = Result<Output, TmuxError>>;
    /// `account_for_pid(pid, agent)`.
    fn account(&self, pid: i64, agent: &str) -> impl Future<Output = Result<Map<String, Value>, StateFault>>;
    /// `grok_metadata_for_pid(pid)`.
    fn grok_metadata(&self, pid: i64) -> impl Future<Output = Result<Map<String, Value>, StateFault>>;
    /// `observe_pane` dentro del `try` de `reconcile_card_config`.
    fn observe(&self, session: &str, pane: &str, info: &AgentInfo)
        -> impl Future<Output = Result<Observed, StateFault>>;
    /// `cc_usage.latest_session_config(USAGE_DB, session, pane)` (`None` = `{}`).
    fn session_config(&self, session: &str, pane: &str)
        -> impl Future<Output = Result<Option<Map<String, Value>>, StateFault>>;
    /// `ssh -O check <host>` con plazo 3 s: `true` si sale 0 (cualquier excepción → `false`).
    fn ssh_check(&self, host: &str) -> impl Future<Output = bool>;
}
```

- [ ] **Step 2: Ayudante del oráculo en el servidor**

En `crates/comandos-server/tests/support/oracle.rs` añadir una función `run_python(script, args, home)` con el mismo cuerpo que `crates/comandos-runtime/tests/support/python.rs` (repo = `CARGO_MANIFEST_DIR/../..`).

- [ ] **Step 3: Prueba diferencial que falla**

Crear `crates/comandos-server/tests/dash_native_state_cards.rs`. El caso es un JSON con todas las entradas; el Python sustituye las funciones de efecto; el Rust usa `FakeEffects` sobre el mismo JSON.

```rust
//! read_states (bin/cc-dash) con sus efectos sustituidos, contra
//! states::cards + suggest + tab_models con efectos falsos idénticos.
mod support;
use comandos_core::json::response_dumps;
use comandos_runtime::agent_procs::{AgentInfo, AgentMaps, AgentProc, PaneRow, agent_pane_maps, external_agents, process_owners};
use comandos_server::dash::native::{
    states::{
        StateFault,
        cards::{self, CardEffects, Inputs},
        observe::Observed,
        records::RecordCache,
        suggest::{self, SuggestContext},
        tab_models::tab_models,
    },
    tmux::{Output, TmuxError},
};
use serde_json::{Map, Value, json};
use std::{collections::{HashMap, HashSet}, fs, path::Path, time::Duration};

const ORACLE: &str = r#"
import importlib.machinery, importlib.util, json, os, sys, types
repo, case, out_path = sys.argv[1:4]
sys.path.insert(0, os.path.join(repo, "bin"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
c = json.load(open(case))
dash.STATE = c["stateDir"]
dash.APP_TAB_MODELS_FILE = out_path
dash.time.time = lambda: c["now"]
dash.tmux_pane_inventory = lambda: c["panes"]
dash.agent_procs = lambda: [tuple(p) for p in c["procs"]]
dash.parent_pid = lambda pid: c["parents"].get(str(pid), 0)
dash._proc_cmdline = lambda pid: c["cmdlines"].get(str(pid), [])
dash.pane_snapshot.PaneInspector = lambda *a, **k: None
dash.session_labels = lambda: c["labels"]
def tmux(*args, timeout=5):
    hit = c["tmux"].get("\x1f".join(args))
    if hit is None:
        return types.SimpleNamespace(returncode=1, stdout="", stderr="no")
    return types.SimpleNamespace(returncode=hit[0], stdout=hit[1], stderr="")
dash.tmux = tmux
dash.account_for_pid = lambda pid, agent: dict(c["accounts"].get(str(pid), {}))
dash.grok_metadata_for_pid = lambda pid: dict(c["grok"].get(str(pid), {}))
def observe_pane(sess, pane, info=None, inspector=None):
    seen = c["observed"].get(pane)
    if seen is None:
        raise ValueError("el panel ya no existe")
    return dict(seen)
dash.observe_pane = observe_pane
dash.cc_usage.latest_session_config = lambda db, s, p: dict(c["configs"].get(s + "|" + p, {}))
real_run = dash.subprocess.run
def run(argv, **kw):
    if argv[:3] == ["ssh", "-O", "check"]:
        return types.SimpleNamespace(returncode=0 if c["ssh"].get(argv[3]) else 255)
    return real_run(argv, **kw)
dash.subprocess.run = run
latency = {(l[0], l[1]): (l[2], l[3]) for l in c["latency"]}
dash._suggestion_context = lambda: (c["guard"], set(c["routes"]), latency)
dash.MOTOR_RESULT.clear()
dash.MOTOR_RESULT.update(c["motor"])
dash.load_provider_registry = lambda: c["registry"]
dash.load_model_tiers = lambda: c["tiers"]
print(json.dumps(dash.read_states()))
"#;

struct FakeEffects<'a>(&'a Value);

impl CardEffects for FakeEffects<'_> {
    async fn tmux(&self, args: &[&str], _timeout: Duration) -> Result<Output, TmuxError> {
        Ok(match self.0["tmux"].get(args.join("\u{1f}")) {
            None => Output { ok: false, stdout: String::new(), stderr: "no".into() },
            Some(hit) => Output {
                ok: hit[0] == json!(0),
                stdout: hit[1].as_str().unwrap().into(),
                stderr: String::new(),
            },
        })
    }
    async fn account(&self, pid: i64, _agent: &str) -> Result<Map<String, Value>, StateFault> {
        Ok(self.0["accounts"].get(pid.to_string()).and_then(Value::as_object).cloned().unwrap_or_default())
    }
    async fn grok_metadata(&self, pid: i64) -> Result<Map<String, Value>, StateFault> {
        Ok(self.0["grok"].get(pid.to_string()).and_then(Value::as_object).cloned().unwrap_or_default())
    }
    async fn observe(&self, _s: &str, pane: &str, _i: &AgentInfo) -> Result<Observed, StateFault> {
        Ok(match self.0["observed"].get(pane).and_then(Value::as_object) {
            Some(seen) => Observed::Seen(seen.clone()),
            None => Observed::Unconfirmed,
        })
    }
    async fn session_config(&self, s: &str, p: &str) -> Result<Option<Map<String, Value>>, StateFault> {
        Ok(self.0["configs"].get(format!("{s}|{p}")).and_then(Value::as_object).cloned())
    }
    async fn ssh_check(&self, host: &str) -> bool {
        self.0["ssh"].get(host).is_some_and(|v| v == &json!(true))
    }
}

/// Lo que el escaneo real (Tarea 5) produce, construido desde el caso.
fn inputs(c: &Value, records: RecordCache) -> (Inputs, RecordCache) {
    let mut records = records;
    let panes: Vec<PaneRow> = c["panes"].as_array().unwrap().iter().map(|p| PaneRow {
        session: p["session"].as_str().unwrap().into(),
        pane: p["pane"].as_str().unwrap().into(),
        pane_pid: p["pane_pid"].as_i64().unwrap(),
        command: p["command"].as_str().unwrap().into(),
        cwd: p["cwd"].as_str().unwrap().into(),
        activity: p["activity"].as_f64().unwrap(),
        pane_active: p["paneActive"].as_bool().unwrap(),
    }).collect();
    let procs: Vec<AgentProc> = c["procs"].as_array().unwrap().iter().map(|p| AgentProc {
        pid: p[0].as_i64().unwrap(),
        cwd: p[1].as_str().unwrap().into(),
        agent: p[2].as_str().unwrap().into(),
    }).collect();
    let mut parent = |pid: i64| c["parents"].get(pid.to_string()).and_then(Value::as_i64).unwrap_or(0);
    let mut cmdline = |pid: i64| -> Vec<String> {
        serde_json::from_value(c["cmdlines"][pid.to_string()].clone()).unwrap_or_default()
    };
    let owners = process_owners(&procs, &panes, &mut parent);
    let maps: AgentMaps = agent_pane_maps(&procs, &panes, &owners, &mut cmdline, &mut parent);
    let external: HashSet<(String, String)> = external_agents(&procs, &owners, &mut parent);
    let labels: HashMap<String, String> =
        serde_json::from_value(c["labels"].clone()).unwrap();
    let scanned = records.scan(Path::new(c["stateDir"].as_str().unwrap())).unwrap();
    let inputs = Inputs { now: c["now"].as_f64().unwrap(), panes, maps, external, labels, records: scanned };
    (inputs, records)
}

async fn rust_side(c: &Value) -> (String, String) {
    let (inputs, _cache) = inputs(c, RecordCache::default());
    let effects = FakeEffects(c);
    let mut items = cards::build(&inputs, &effects).await.unwrap();
    let motor = c["motor"].as_object().cloned().unwrap();
    let ctx = SuggestContext {
        guard: c["guard"].clone(),
        routes: c["routes"].as_array().unwrap().iter().map(|r| r.as_str().unwrap().to_owned()).collect(),
        latency: c["latency"].as_array().unwrap().iter()
            .map(|l| ((l[0].clone(), l[1].clone()), (l[2].clone(), l[3].clone()))).collect(),
    };
    suggest::annotate_all(&mut items, &ctx, &motor, inputs.now).unwrap();
    cards::sort_items(&mut items).unwrap();
    let models = tab_models(&items, &c["registry"], &motor, &c["tiers"], inputs.now).unwrap();
    (response_dumps(&Value::Array(items)).unwrap(), response_dumps(&models).unwrap())
}

fn write_state(dir: &Path, name: &str, record: Value) {
    fs::write(dir.join(name), record.to_string()).unwrap();
}

fn base_case(dir: &Path) -> Value {
    let state = dir.join("state");
    fs::create_dir_all(&state).unwrap();
    let now = 1_791_115_200.0_f64;
    write_state(&state, "exact.json", json!({"session":"mixed","pane":"%3","agent":"grok","status":"waiting","detail":"exact question","ts":now - 50.0,"cwd":"/stale"}));
    write_state(&state, "duplicate.json", json!({"session":"mixed","pane":"%3","agent":"grok","status":"working","ts":now - 60.0}));
    write_state(&state, "legacy.json", json!({"session":"mixed","agent":"grok","status":"waiting","ts":now - 10.0}));
    write_state(&state, "old-done.json", json!({"project":"gone.proj","status":"done","ts":now - 300.0,"cwd":"/ext","agent":"codex"}));
    write_state(&state, "zombie.json", json!({"session":"ghost","status":"waiting","ts":now - 200_000.0,"pane":"%77"}));
    write_state(&state, "str-ts.json", json!({"session":"term","pane":"%6","agent":"claude","status":"working","ts":"1791115100"}));
    write_state(&state, "bad-ts.json", json!({"session":"x","ts":"mañana"}));
    fs::write(state.join("roto.json"), "{").unwrap();
    fs::write(state.join(".oculto.json"), "{}").unwrap();
    let registry: Value = serde_json::from_str(
        &fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/providers.json")).unwrap(),
    ).unwrap();
    let tiers: Value = serde_json::from_str(
        &fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/model-tiers.json")).unwrap(),
    ).unwrap();
    json!({
        "stateDir": state,
        "now": now,
        "panes": [
            {"session":"mixed","pane":"%1","pane_pid":100,"command":"node","cwd":"/same","activity":now - 5.0,"paneActive":true},
            {"session":"mixed","pane":"%2","pane_pid":200,"command":"claude","cwd":"/same","activity":now - 5.0,"paneActive":false},
            {"session":"mixed","pane":"%3","pane_pid":300,"command":"grok","cwd":"/same","activity":now - 5.0,"paneActive":false},
            {"session":"term","pane":"%6","pane_pid":600,"command":"claude","cwd":"/shell","activity":now - 9.0,"paneActive":true},
            {"session":"hub","pane":"%7","pane_pid":700,"command":"zsh","cwd":"/h","activity":now,"paneActive":false},
            {"session":"local","pane":"%8","pane_pid":800,"command":"zsh","cwd":"/home/u","activity":now - 1.0,"paneActive":false},
            {"session":"ssh-box","pane":"%9","pane_pid":900,"command":"zsh","cwd":"/r","activity":now - 2.0,"paneActive":false}
        ],
        "procs": [[101,"/same","codex"],[201,"/same","claude"],[301,"/same","grok"],[601,"/shell","claude"],[555,"/ext","codex"]],
        "parents": {"101":100,"201":200,"301":300,"601":600,"555":1},
        "cmdlines": {"101":["codex"],"201":["claude"],"301":["grok"],"601":["claude"]},
        "labels": {"mixed": "Project"},
        "tmux": {
            "capture-pane\u{1f}-p\u{1f}-t\u{1f}%1\u{1f}-S\u{1f}-12": [0, "  Working (12s • esc to interrupt)\n"],
            "capture-pane\u{1f}-p\u{1f}-t\u{1f}%2\u{1f}-S\u{1f}-14": [0, "● hola\n esc to interrupt\n"],
            "list-panes\u{1f}-s\u{1f}-t\u{1f}=ssh-box\u{1f}-F\u{1f}#{pane_current_command}": [0, "zsh\n"]
        },
        "accounts": {"101": {"account":"main","accountEmail":"a@b.c"}, "201": {"account":"work","accountEmail":""}, "301": {"account":"main","accountEmail":""}, "601": {"account":"main","accountEmail":""}},
        "grok": {"301": {"title": "Grok T"}},
        "observed": {
            "%1": {"harness":"codex","pid":101,"conversationId":"c1","model":"gpt-5.6-sol","effort":"xhigh","source":"conversation","observedAt":now,"identity":"i1","harnessAccount":"main","evidenceAt":now,"motor":"codex","motorAccount":"main","confirmed":true,"accountSource":"process-environment","limitations":[]},
            "%2": {"harness":"claude","pid":201,"conversationId":"s2","model":"claude-fable-5[1m]","effort":"max","source":"pane","observedAt":now,"identity":"i2","harnessAccount":"work","motor":"claude","motorAccount":"work","confirmed":true,"accountSource":"process-environment","limitations":[]}
        },
        "configs": {"mixed|%2": {"id": 7, "tmux_session": "mixed", "tmux_pane": "%2", "model": "claude-opus-5", "effective_at": 1791115000.5}},
        "ssh": {"box": true},
        "guard": {"projects": [{"project": "Project", "level": "warning", "calls10m": 61, "tokensHour": 4200000}], "forecasts": [{"scope": "Fable", "level": "critical", "downtimeHours": 30.5}]},
        "routes": ["claude:claude", "codex:codex"],
        "latency": [["claude-sonnet-5", "low", 4500, 12]],
        "motor": {"mixed|%1": {"stage": "cambiando", "ts": now - 10.0, "model": "gpt-5.6-luna"}},
        "registry": registry,
        "tiers": tiers
    })
}

#[tokio::test(flavor = "current_thread")]
async fn cards_match_python_oracle() {
    let dir = std::env::temp_dir().join(format!("cmd-cards-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let case = base_case(&dir);
    let file = dir.join("case.json");
    fs::write(&file, case.to_string()).unwrap();
    let models_py = dir.join("models-py.json");
    let Some(expected) =
        support::oracle::run_python(ORACLE, &[file.as_os_str(), models_py.as_os_str()], &dir)
    else {
        return;
    };
    let (items, models) = rust_side(&case).await;
    assert_eq!(items, expected.trim_end());
    assert_eq!(models, fs::read_to_string(&models_py).unwrap());
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "current_thread")]
async fn annotation_error_stops_remaining_like_python() {
    let dir = std::env::temp_dir().join(format!("cmd-cards-err-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut case = base_case(&dir);
    // `int(project.get('tokensHour'))` de una lista: TypeError dentro del try.
    case["guard"]["projects"][0]["tokensHour"] = json!(["x"]);
    let file = dir.join("case.json");
    fs::write(&file, case.to_string()).unwrap();
    let models_py = dir.join("models-py.json");
    let Some(expected) =
        support::oracle::run_python(ORACLE, &[file.as_os_str(), models_py.as_os_str()], &dir)
    else {
        return;
    };
    assert_eq!(rust_side(&case).await.0, expected.trim_end());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unsure_record_declines_scan() {
    let dir = std::env::temp_dir().join(format!("cmd-cards-unsure-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("a.json"), r#"{"session":"s","detail":"\ud800","ts":1}"#).unwrap();
    assert_eq!(RecordCache::default().scan(&dir).err(), Some(StateFault::Decline));
    fs::write(dir.join("a.json"), r#"{"session":"s","ts":1e400}"#).unwrap();
    assert_eq!(RecordCache::default().scan(&dir).err(), Some(StateFault::Decline));
    let _ = fs::remove_dir_all(&dir);
}
```

El caso cubre: anotación exacta frente a duplicada y heredada ambigua, `ts` como cadena, `ts` ilegible y JSON roto saltados, oculto con punto, sesión `hub` oculta y `local` visible, split con etiqueta, pista Codex `working`, Claude ocupado sin anotación, cuenta, título Grok, observación `Unconfirmed` (`%3`, `%6`), configuración con flotante, `ssh-*` con `mux`, desacoplado externo (`/ext` con codex de padre 1) y zombi, cambio de motor en curso, sugerencias de proyecto y de cuota, y el orden final.

- [ ] **Step 4: Ver el fallo**

Run: `$C test -p comandos-server --test dash_native_state_cards`
Expected: FAIL de compilación (`cards::build`, `RecordCache`… no existen).

- [ ] **Step 5: `records.rs`**

```rust
//! `glob(STATE/*.json)` + `json.load` de `read_states` (7175-7184), con
//! caché por firma (D5): un archivo sin cambios no se reparsea.
use super::StateFault;
use crate::dash::native::{files::{Strict, loads_strict}, py};
use comandos_core::json::truthy;
use serde_json::{Map, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct Record {
    pub value: Map<String, Value>,
    pub timestamp: f64,
}

type Signature = (u64, u64, u64, i64, i64, i64, i64);

#[derive(Default)]
pub struct RecordCache {
    entries: HashMap<PathBuf, (Signature, Option<Record>)>,
}

/// `float(x)` de Python sobre un valor de JSON; `None` = el `except` salta el
/// registro; `Err` = no finito u overflow (D2).
fn py_float_value(value: &Value) -> Result<Option<f64>, StateFault> {
    let f = match value {
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Number(n) => match n.as_str().parse::<f64>() {
            Ok(f) => f,
            Err(_) => return Ok(None),
        },
        Value::String(s) => match py::float(s) {
            Ok(f) => f,
            Err(_) => return Ok(None),
        },
        _ => return Ok(None),
    };
    if f.is_finite() { Ok(Some(f)) } else { Err(StateFault::Decline) }
}

fn parse_record(path: &Path) -> Result<Option<Record>, StateFault> {
    let Ok(bytes) = fs::read(path) else { return Ok(None) };
    let Ok(text) = std::str::from_utf8(&bytes) else { return Ok(None) };
    let value = match loads_strict(text) {
        Strict::Value(v) => v,
        Strict::Unsure => return Err(StateFault::Decline),
        Strict::Missing | Strict::Unreadable => return Ok(None),
    };
    let Value::Object(map) = value else { return Ok(None) };
    let timestamp = match map.get("ts").filter(|v| truthy(v)) {
        None => 0.0,
        Some(v) => match py_float_value(v)? {
            Some(f) => f,
            None => return Ok(None),
        },
    };
    Ok(Some(Record { value: map, timestamp }))
}

impl RecordCache {
    /// Bloquea: llamar dentro de `spawn_blocking`.
    pub fn scan(&mut self, dir: &Path) -> Result<Vec<Record>, StateFault> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let Ok(listing) = fs::read_dir(dir) else {
            self.entries.clear();
            return Ok(out);
        };
        for entry in listing.flatten() {
            let name = entry.file_name();
            let bytes = name.as_bytes();
            if bytes.first() == Some(&b'.') || !bytes.ends_with(b".json") {
                continue;
            }
            let path = entry.path();
            let Ok(meta) = fs::metadata(&path) else { continue };
            if !meta.is_file() {
                continue;
            }
            let signature = (
                meta.dev(), meta.ino(), meta.size(),
                meta.mtime(), meta.mtime_nsec(), meta.ctime(), meta.ctime_nsec(),
            );
            seen.insert(path.clone());
            if let Some((old, record)) = self.entries.get(&path) {
                if *old == signature {
                    out.extend(record.clone());
                    continue;
                }
            }
            let record = parse_record(&path)?;
            out.extend(record.clone());
            self.entries.insert(path, (signature, record));
        }
        self.entries.retain(|p, _| seen.contains(p));
        Ok(out)
    }
}
```

- [ ] **Step 6: `cards.rs`, `observe.rs`, `suggest.rs`, `tab_models.rs`**

Implementar con las reglas de «Comportamiento portado». Estructura obligatoria de `cards::build` (el bucle del Python, secuencial, los efectos en su orden):

```rust
pub async fn build<E: CardEffects>(inputs: &Inputs, effects: &E) -> Result<Vec<Value>, StateFault> {
    let annotations = annotate(inputs)?;          // (anotaciones por (s, p), desacoplados)
    let counts = pane_counts(&inputs.panes);
    let mut items = Vec::new();
    for row in &inputs.panes {
        if let Some(item) = live_card(inputs, &annotations, &counts, row, effects).await? {
            items.push(Value::Object(item));
        }
    }
    items.extend(historical(inputs, &annotations)?);
    Ok(items)
}
```

Dentro de `live_card`, el orden de efectos es exactamente: `pane_status_hint` → `claude_pane_busy` → `effects.account` → (grok) `effects.grok_metadata` → `effects.observe` → `effects.session_config` → `ssh_state`; un error de cualquiera se propaga al instante (D3). `effects.tmux(...)` con `Err` → `StateFault::from_tmux`; con `ok == false` → `""`.

`observe::ObserveEvidence` (lo que la Tarea 5 recoge, en el orden de 6917-6996):

```rust
pub struct ObserveEvidence {
    pub agent: String,
    pub pid: i64,
    /// `_identity_key(identity)`.
    pub identity: String,
    /// `inspector({...})` de la Tarea 2.
    pub snap: Obs,
    /// `account_for_pid(pid, agent).get('account') or 'unknown'`.
    pub account: String,
    pub harness_has_accounts: bool,
    pub cmdline: Vec<String>,
    /// codex: contexto del rollout raíz; grok/opencode/agy: metadatos filtrados;
    /// claude: transcript único (`{}` si no). Ya proyectado como `conversation`.
    pub conversation: Obs,
    /// acp: `acp_state_for_pane(pane)` (el filtro de pid/sesión se aplica en `observe`).
    pub acp: Obs,
    /// `pane_visible_config` (`{}` para los demás arneses).
    pub visible: Obs,
    pub process_start: String,
    /// `ANTHROPIC_BASE_URL` del `environ` (`""` si falta).
    pub base_url: String,
}
```

`suggest::latency_from(stats)` replica la comprensión de 7088-7090 (clave o valor con contenedor no hasheable → lista vacía, el `except` del Python).

- [ ] **Step 7: Verde**

Run: `$C test -p comandos-server --test dash_native_state_cards` → 3 PASS.
Run: `$C clippy -p comandos-server --all-targets -j 6 -- -D warnings` → limpio.

- [ ] **Step 8: Commit**

```bash
git add crates/comandos-server/src/dash/native/mod.rs crates/comandos-server/src/dash/native/states \
  crates/comandos-server/tests/support/oracle.rs crates/comandos-server/tests/dash_native_state_cards.rs
git commit -m "feat(dash): ensamblado de /state — registros, tarjetas, observación, sugerencias y app-tab-models

El bucle de read_states portado sobre CardEffects en el orden del Python; prueba
diferencial contra bin/cc-dash con sus efectos sustituidos.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: GET `/state` nativo — recolección, caché de 1,2 s, contexto y escritura

Desplegable sola: GET `/state` (y `/state?…`) pasa a Rust. Toda la recolección real, el vuelo único, el contexto de sugerencias con el heredado, y la escritura de `app-tab-models.json` solo después de saber que no se declina.

Cómputo (`gather::compute`, en este orden):

1. `tmux list-panes -a -F PANE_FORMAT` (5 s) → `parse_pane_inventory`; `ok == false` → `[]`; error → `from_tmux`.
2. `spawn_blocking(scan)`: registro (`RegistryCache`, `repo_root/config/providers.json`; sin `repo_root` → `Decline`), `read_conf` → `agent_set` → `process_aliases`, `agent_procs(proc_root)`, `process_owners`, `agent_pane_maps`, `external_agents` (padres y `cmdline` de `/proc` con cachés locales por cómputo), `PaneInspector::new(home, proc_root)`, `RecordCache::scan(H/state)`, `H/motor-results.json` (D1), `model-tiers.json` (`repo_root/config`), `light::tab_labels`, `light::read_tab_history`.
3. `light::tmux_sessions` → `session_labels` (6824: etiquetas de pestañas + historial de sesiones vivas que no tienen etiqueta propia o la tienen igual al nombre).
4. `cards::build(&inputs, &RealEffects)`.
5. Si `suggest::needs_context` → `context.get()` (D1); `annotate_all`.
6. `sort_items`; `tab_models(...)`.
7. Serializar el cuerpo (`response_dumps(&items)`) y el JSON de modelos (`response_dumps`); fallo → `Decline`.
8. `spawn_blocking(files::write_json_atomic(H/app-tab-models.json))`, error ignorado (`except Exception: pass`).

`RealEffects`:
- `tmux` = `Native.options().tmux.run` con plazo propio (2 s para la pista: un `Tmux` clonado con `timeout` cambiado).
- `account` = `spawn_blocking(AccountCache::account_for_pid(proc_root, home, pid, agent))`.
- `grok_metadata` = `spawn_blocking`: `environ` → `GROK_HOME` (`~/.grok` si falta, `canonicalize` como `realpath`) → `GrokMetadataCache::read(pid, home)`.
- `observe`: (a) `tmux display-message -p -t <pane> <campos>` (6890): plazo vencido → `Err(Timeout)`; spawn/decodificación → `Observed::Unconfirmed`; campos que no son 8 o `pane_id`/`session_name` distintos → `Unconfirmed`; (b) `spawn_blocking`: `server_start` de `/proc/<pid>/stat`, `inspector.inspect`, cuenta, `cmdline`, conversación (codex: `fd` de `/proc/<pid>/fd/*` que acaben en `<id>.jsonl` en orden de `read_dir`, `TranscriptCache::read("codex", id, fd)` hasta uno con modelo; grok: `GrokMetadataCache` si `sessionId` casa; opencode/agy: `nativeMetadata` si casa; claude: `glob(<config>/projects/*/<id>.jsonl)` exactamente una → `TranscriptCache::read("claude", …)`; acp: `H/acp-panes.json[pane]`), `process_start`, `ANTHROPIC_BASE_URL` — `Unsure` de un lector → `Err(Decline)`; (c) `tmux capture-pane -p -t <pane> -S -45` para codex/claude/opencode → `screen_state` (decodificación → `Unconfirmed`, plazo → `Err(Timeout)`); (d) `observe::observe` con el `StateTracker` del motor (`Mutex`, sin `await` dentro).
- `session_config` = `native.usage.with(|b| …)`: `SELECT * FROM usage_session_configs WHERE tmux_session=? AND (tmux_pane=? OR (?='' AND tmux_pane='')) ORDER BY effective_at DESC LIMIT 1` → objeto con las columnas en el orden del `SELECT *` (INTEGER → entero, REAL → `hooks::py::float_value`, TEXT, NULL; BLOB → `Decline`). El carril declina si está apagado; su error de SQL → `Failure`.
- `ssh_check` = `run_program(&opts.ssh, ["-O", "check", host], 3 s)` → `ok`; cualquier error → `false`.

Contexto (`context.rs`): `routes` = `selectable_routes(registry, runtime_facts(registry, accounts::public_accounts(registry, &accounts::Paths::new(home, cwd)), which, home, which("cc-model-proxy").is_some(), proxy_alive))`, con `proxy_alive` = `TcpStream::connect(127.0.0.1:proxy_port)` con plazo 300 ms (async). `guard` = `subrequest::get(legacy, token, "/usage/guard")`; `latency` = `latency_from` de `subrequest::get(…, "/usage/analytics?days=7")`. Status 200 + JSON → valor; otro status → `{}`/vacío; transporte o plazo → `Err(Decline)` sin cachear. Vigencia: `now_ms - at_ms > 60_000` → recalcular (la condición del Python, `>`).

**Files:**
- Create: `crates/comandos-server/src/dash/native/states/cache.rs`, `gather.rs`, `context.rs`; `crates/comandos-server/src/dash/native/subrequest.rs`
- Modify: `crates/comandos-server/src/dash/native/states/mod.rs` (`Engine`, `ROUTES`, `answer`), `crates/comandos-server/src/dash/native/mod.rs` (variante, tabla, campo `states`, opciones, `states_cached`), `crates/comandos-server/src/dash/native/light.rs` (`read_tab_history` → `pub(crate)`), `crates/comandos-server/src/dash/native/lanes.rs` (`UsageBackend::ROUTES` = `"GET /pomodoro, GET /sovereignty y GET /state"`), `crates/comandos-server/src/dash/forward.rs` (`pub(crate) async fn connect_paced(SocketAddr) -> io::Result<TcpStream>` extraído de `relay`), `crates/comandos-server/src/dash/mod.rs` (`build`)
- Modify: `crates/comandos-server/tests/support/mod.rs` (opciones nuevas, `fake_agent`, `start_session`)
- Create: `crates/comandos-server/tests/dash_native_state.rs`
- Modify: `xtask/parity/frente.jsonl`

**Interfaces:**
- Consumes: Tareas 1–4; `Native.usage` (2c), `NativeOptions.repo_root` (2c), `files::write_json_atomic`, `light::{tab_labels, tmux_sessions, read_tab_history}`, `tmux::{Tmux, Program, run_program}`, `comandos_runtime::accounts::{public_accounts, Paths}`.
- Produces: `NativeOptions.{proc_root: PathBuf, legacy: SocketAddr, legacy_token: Vec<u8>, search_path: Option<OsString>, cwd: PathBuf, codex_home: Option<PathBuf>, grok_home: Option<PathBuf>, ssh: tmux::Program}`; `states::{Engine, States { items: Arc<Vec<Value>>, body: bytes::Bytes }, ROUTES, answer}`; `cache::{StatesCache, TTL_MS: i64 = 1200}`; `Native::states_cached(&self) -> Result<Arc<States>, Fault>`; `NativeRoute::State`; `subrequest::{get(SocketAddr, &[u8], &str, Duration) -> Result<(u16, bytes::Bytes), SubError>, SubError}`; `support::{fake_agent(&TestHome, &str) -> PathBuf, start_session(&TestHome, &str, &str)}`.

- [ ] **Step 1: Opciones nuevas y `build`**

En `NativeOptions` (2c ya añadió `usage_db`, `desktop_device`, `repo_root`, `journal_db`):

```rust
    /// Raíz de `/proc` (las pruebas de capa usan una falsa; el frente, la real).
    pub proc_root: PathBuf,
    /// Heredado para el contexto de sugerencias (D1, D11). `build` lo fija.
    pub legacy: std::net::SocketAddr,
    pub legacy_token: Vec<u8>,
    /// `PATH` del frente al arrancar, para `providers::which` (D9).
    pub search_path: Option<std::ffi::OsString>,
    /// Directorio de trabajo del frente (rutas relativas del catálogo y cuentas).
    pub cwd: PathBuf,
    pub codex_home: Option<PathBuf>,
    pub grok_home: Option<PathBuf>,
    /// `ssh` de `ssh_state` (7832).
    pub ssh: tmux::Program,
```

En `for_home`: `proc_root: "/proc".into()`, `legacy: SocketAddr::from(([127, 0, 0, 1], 4781))`, `legacy_token: Vec::new()`, `search_path: std::env::var_os("PATH")`, `cwd: std::env::current_dir().unwrap_or_else(|_| home.to_path_buf())`, `codex_home: std::env::var_os("CODEX_HOME").filter(|v| !v.is_empty()).map(PathBuf::from)`, igual `grok_home` con `GROK_HOME`, `ssh: tmux::Program::named("ssh")`. En `dash::build`, tras resolver `opts`:

```rust
        let mut opts = opts.unwrap_or_else(|| native::NativeOptions::for_home(&cfg.home, cfg.state_db.clone()));
        // El contexto de sugerencias habla con el MISMO heredado al que se reenvía.
        opts.legacy = SocketAddr::from((Ipv4Addr::LOCALHOST, cfg.legacy_port));
        opts.legacy_token = cfg.token.clone();
```

En `TestHome::options()` (tests/support): `opts.search_path = Some(self.root.join("bin").into_os_string())` (directorio vacío creado en `TestHome::new`), `opts.ssh = Program::named("/no-existe/ssh")`, `opts.codex_home = None`, `opts.grok_home = None`, `opts.cwd = self.root.clone()`. Ayudantes:

```rust
/// Copia `/bin/sleep` como `<home>/bin/<name>`: un proceso con ese argv[0]
/// para `agent_procs` (nunca un agente real).
pub fn fake_agent(home: &TestHome, name: &str) -> PathBuf {
    let path = home.root.join("bin").join(name);
    std::fs::copy("/bin/sleep", &path).unwrap();
    path
}

/// Sesión del tmux privado con `cmd` como proceso del pane, sin el entorno
/// del desarrollador (`env -i`), con el HOME de la prueba.
pub fn start_session(home: &TestHome, name: &str, cmd: &str) {
    let status = Command::new("tmux")
        .args(["-f", "/dev/null", "new-session", "-d", "-s", name, "-c"])
        .arg(&home.root)
        .arg(format!("env -i HOME={} PATH=/usr/bin:/bin {cmd}", home.root.display()))
        .env_remove("TMUX")
        .env("TMUX_TMPDIR", home.tmux_dir())
        .status()
        .unwrap();
    assert!(status.success());
}
```

- [ ] **Step 2: Pruebas que fallan**

Crear `crates/comandos-server/tests/dash_native_state.rs`:

```rust
//! GET /state respondido por Rust: oráculo cc-dash real sobre el mismo HOME
//! y el mismo tmux privado, vuelo único, declinar sin escribir, 504.
mod support;
use serde_json::{Value, json};
use std::{fs, sync::{Arc, atomic::{AtomicI64, Ordering}}, time::Duration};
use support::{TestHome, config, dead_port, fake_agent, front, get, oracle::oracle, start_session, tmux_available};

fn masked(body: &[u8]) -> String {
    let mut v: Value = serde_json::from_slice(body).unwrap();
    for item in v.as_array_mut().unwrap() {
        if let Some(obs) = item.get_mut("observedConfig").and_then(Value::as_object_mut) {
            for k in ["observedAt", "evidenceAt"] {
                if obs.contains_key(k) {
                    obs.insert(k.into(), json!("<volátil>"));
                }
            }
        }
    }
    comandos_core::json::response_dumps(&v).unwrap()
}

/// HOME con un agente claude falso en su propio pane, su sesión registrada,
/// un transcript con modelo, un split de shells y registros de estado.
fn seed(home: &TestHome) -> Option<()> {
    if !tmux_available() {
        eprintln!("tmux no está: se salta");
        return None;
    }
    let claude = fake_agent(home, "claude");
    start_session(home, "proj", &format!("{} 600", claude.display()));
    start_session(home, "shells", "sh");
    std::process::Command::new("tmux")
        .args(["split-window", "-t", "=shells", "sh"])
        .env_remove("TMUX")
        .env("TMUX_TMPDIR", home.tmux_dir())
        .status()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let pid = std::process::Command::new("tmux")
        .args(["display-message", "-p", "-t", "=proj:", "#{pane_pid}"])
        .env_remove("TMUX")
        .env("TMUX_TMPDIR", home.tmux_dir())
        .output()
        .unwrap();
    let pid: i64 = String::from_utf8_lossy(&pid.stdout).trim().parse().unwrap();
    fs::create_dir_all(home.root.join(".claude/sessions")).unwrap();
    fs::write(home.root.join(".claude/sessions/x.json"), json!({"pid": pid, "sessionId": "abc"}).to_string()).unwrap();
    fs::create_dir_all(home.root.join(".claude/projects/p")).unwrap();
    fs::write(
        home.root.join(".claude/projects/p/abc.jsonl"),
        "{\"type\":\"assistant\",\"uuid\":\"u1\",\"sessionId\":\"abc\",\"message\":{\"model\":\"claude-sonnet-5\"}}\n",
    ).unwrap();
    let state = home.hooks().join("state");
    fs::write(state.join("a.json"), json!({"session":"proj","agent":"claude","status":"working","ts":1}).to_string()).unwrap();
    fs::write(state.join("b.json"), json!({"session":"gone","status":"waiting","detail":"¿sigo?","ts":2.5}).to_string()).unwrap();
    home.write("app-tabs.json", r#"{"proj": "Proyecto"}"#);
    Some(())
}

#[tokio::test(flavor = "current_thread")]
async fn state_matches_python_oracle_and_writes_same_models() {
    let home = TestHome::new("state-oracle");
    if seed(&home).is_none() {
        return;
    }
    let Some(py) = oracle(&home).await else { return };
    let expected = get(py.port, "/state").await;
    let models_py = fs::read(home.hooks().join("app-tab-models.json")).unwrap();
    fs::remove_file(home.hooks().join("app-tab-models.json")).unwrap();
    // El frente reenvía lo no nativo (y las subconsultas) a ese mismo Python.
    let front = front(&home, py.port, home.options()).await;
    let got = get(front.port, "/state").await;
    assert_eq!(got.status, 200);
    assert_eq!(masked(&got.body), masked(&expected.body));
    assert_eq!(fs::read(home.hooks().join("app-tab-models.json")).unwrap(), models_py);
    front.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn state_unsure_record_declines_without_writing() {
    let home = TestHome::new("state-unsure");
    if seed(&home).is_none() {
        return;
    }
    home.write("state/c.json", r#"{"session":"s","detail":"\ud800","ts":3}"#);
    let legacy = support::FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let got = get(front.port, "/state").await;
    assert_eq!(got.text(), r#"{"legacy": true}"#);
    assert!(legacy.requests().iter().any(|l| l.starts_with("GET /state")));
    assert!(!home.hooks().join("app-tab-models.json").exists());
    front.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn state_flight_survives_leader_cancel() {
    let home = TestHome::new("state-flight");
    if seed(&home).is_none() {
        return;
    }
    let clock = Arc::new(AtomicI64::new(support::NOW_MS));
    let mut opts = home.options();
    let shared = clock.clone();
    opts.clock = Arc::new(move || shared.load(Ordering::SeqCst));
    let front = front(&home, dead_port(), opts).await;
    // Líder que se desconecta enseguida y dos seguidores.
    let port = front.port;
    let leader = tokio::spawn(async move {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut s, format!(
            "GET /state HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Comandos-Token: {}\r\n\r\n", support::TOKEN
        ).as_bytes()).await.unwrap();
        drop(s);
    });
    let (a, b) = tokio::join!(get(port, "/state"), get(port, "/state"));
    leader.await.unwrap();
    assert_eq!((a.status, b.status), (200, 200));
    assert_eq!(a.body, b.body);
    // Dentro de 1,2 s: caché (mismo cuerpo aunque cambie un registro).
    home.write("state/d.json", r#"{"session":"otro","status":"waiting","ts":4}"#);
    clock.fetch_add(1_000, Ordering::SeqCst);
    assert_eq!(get(port, "/state").await.body, a.body);
    clock.fetch_add(300, Ordering::SeqCst);
    assert_ne!(get(port, "/state").await.body, a.body);
    front.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn state_hung_tmux_answers_504_runtime_free() {
    let home = TestHome::new("state-hung");
    if seed(&home).is_none() {
        return;
    }
    // Un pane «codex» (pista con plazo 2 s) y un tmux que se cuelga en capture-pane.
    let codex = fake_agent(&home, "codex");
    start_session(&home, "cx", &format!("{} 600", codex.display()));
    std::thread::sleep(Duration::from_millis(300));
    let wrapper = home.root.join("bin/tmux-colgado");
    fs::write(&wrapper, "#!/bin/sh\ncase \"$*\" in *capture-pane*) exec sleep 30;; esac\nexec tmux \"$@\"\n").unwrap();
    std::os::unix::fs::PermissionsExt::set_mode(&mut fs::metadata(&wrapper).unwrap().permissions(), 0o755);
    fs::set_permissions(&wrapper, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut opts = home.options();
    opts.tmux.program.path = wrapper;
    let front = front(&home, dead_port(), opts).await;
    let port = front.port;
    let slow = tokio::spawn(async move { get(port, "/state").await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let started = std::time::Instant::now();
    assert_eq!(get(port, "/prefs").await.status, 200);
    assert!(started.elapsed() < Duration::from_millis(500), "el runtime quedó bloqueado");
    assert_eq!(slow.await.unwrap().status, 504);
    front.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn suggestion_context_legacy_500_is_empty_and_down_declines() {
    use comandos_server::dash::native::states::context::Context;
    // Contexto de una tarjeta que lo necesita: heredado caído → Decline;
    // heredado que responde 500 → guard {} y latencia vacía, cacheado 60 s.
    let home = TestHome::new("state-ctx");
    let opts = home.options();
    let ctx = Context::default();
    let down = ctx.get(&opts, &serde_json::json!({}), support::NOW_MS).await;
    assert!(down.is_err());
    let failing = support::FixedLegacy::start(500, r#"{"error": "Error interno del tablero"}"#).await;
    let mut opts = home.options();
    opts.legacy = std::net::SocketAddr::from(([127, 0, 0, 1], failing.port));
    let got = ctx.get(&opts, &serde_json::json!({}), support::NOW_MS).await.unwrap();
    assert_eq!(got.guard, json!({}));
    assert!(got.latency.is_empty());
    drop(failing);
    // 60 s después sigue siendo el mismo (el heredado ya no existe).
    assert!(ctx.get(&opts, &serde_json::json!({}), support::NOW_MS + 60_000).await.is_ok());
    assert!(ctx.get(&opts, &serde_json::json!({}), support::NOW_MS + 60_001).await.is_err());
}
```

Añadir a `tests/support/mod.rs` un `FixedLegacy::start(status, body)` (como `FakeLegacy`, con status y cuerpo fijos; `Drop` aborta su tarea y cierra el puerto).

- [ ] **Step 3: Ver el fallo**

Run: `$C test -p comandos-server --test dash_native_state`
Expected: FAIL de compilación (`context::Context`, `fake_agent`… no existen).

- [ ] **Step 4: Caché y vuelo único (`cache.rs`)**

```rust
//! `read_states_cached` (7315): TTL de 1,2 s y un solo cómputo en vuelo.
//! Los que llegan durante un cómputo reciben su resultado o su error (D4:
//! si el líder se abandona, repiten). Los errores no se cachean.
use super::{StateFault, States};
use std::{future::Future, sync::{Arc, Mutex}};
use tokio::sync::watch;

pub const TTL_MS: i64 = 1200;

#[derive(Clone)]
enum Flight {
    Running,
    Done(Result<Arc<States>, StateFault>),
    Abandoned,
}

#[derive(Default)]
struct Inner {
    at_ms: i64,
    items: Option<Arc<States>>,
    flight: Option<watch::Receiver<Flight>>,
}

#[derive(Default)]
pub struct StatesCache {
    inner: Mutex<Inner>,
}

/// Publica «abandonado» si el líder se suelta sin terminar.
struct Guard<'a> {
    cache: &'a StatesCache,
    tx: Option<watch::Sender<Flight>>,
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            self.cache.lock().flight = None;
            let _ = tx.send(Flight::Abandoned);
        }
    }
}

impl StatesCache {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub async fn get<F, Fut>(&self, now_ms: &dyn Fn() -> i64, compute: F) -> Result<Arc<States>, StateFault>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<States, StateFault>>,
    {
        loop {
            let waiter = {
                let mut inner = self.lock();
                if let Some(items) = &inner.items {
                    if now_ms() - inner.at_ms < TTL_MS {
                        return Ok(items.clone());
                    }
                }
                match &inner.flight {
                    Some(rx) => Some(rx.clone()),
                    None => {
                        let (tx, rx) = watch::channel(Flight::Running);
                        inner.flight = Some(rx);
                        drop(inner);
                        let mut guard = Guard { cache: self, tx: Some(tx) };
                        let result = compute().await.map(Arc::new);
                        let tx = guard.tx.take();
                        {
                            let mut inner = self.lock();
                            if let Ok(items) = &result {
                                inner.at_ms = now_ms();
                                inner.items = Some(items.clone());
                            }
                            inner.flight = None;
                        }
                        if let Some(tx) = tx {
                            let _ = tx.send(Flight::Done(result.clone()));
                        }
                        return result;
                    }
                }
            };
            if let Some(mut rx) = waiter {
                let seen = rx.wait_for(|f| !matches!(f, Flight::Running)).await.map(|f| f.clone());
                match seen {
                    Ok(Flight::Done(result)) => return result,
                    // Abandonado o emisor soltado: repetir (uno pasa a líder).
                    Ok(Flight::Abandoned | Flight::Running) | Err(_) => continue,
                }
            }
        }
    }
}
```

- [ ] **Step 5: `Engine`, ruta y `states_cached` (`states/mod.rs`, `native/mod.rs`)**

```rust
/// Estado del frente para `/state`: cachés de los lectores (D1, D5), el
/// rastreador y el vuelo único.
#[derive(Default)]
pub struct Engine {
    pub(crate) cache: cache::StatesCache,
    pub(crate) context: context::Context,
    pub(crate) tracker: std::sync::Mutex<comandos_runtime::tui_state::StateTracker>,
    pub(crate) shared: Arc<std::sync::Mutex<gather::Blocking>>,
}

pub struct States {
    pub items: Arc<Vec<Value>>,
    pub body: bytes::Bytes,
}

pub const ROUTES: &[Entry] = &[Entry { verb: Verb::Get, key: Key::Path("/state"), route: NativeRoute::State }];

pub async fn answer(native: &Native) -> Answer {
    let states = native.states_cached().await?;
    Ok(Reply::bytes(http::StatusCode::OK, "application/json", states.body.clone()))
}
```

`Default` de `StateTracker` no existe: implementar `Default for Engine` a mano con `StateTracker::new(128)`. `gather::Blocking` agrupa `RecordCache`, `TranscriptCache::new(128, 2_097_152)`, `GrokMetadataCache::new(128)`, `AccountCache`, `RegistryCache` (todo lo que se usa dentro de `spawn_blocking`; un `Arc<Mutex<…>>` que el trabajo bloqueante toma sin `await`). En `native/mod.rs`: `NativeRoute::State`, `states::ROUTES` en `TABLES` (antes de `retired::ROUTES`), campo `pub(crate) states: states::Engine` (en `new`: `states: states::Engine::default()`), rama `NativeRoute::State => states::answer(self).await` y

```rust
    /// GET `/state` con la caché del Python (también lo usa `/workspace/sort` `by`).
    pub async fn states_cached(&self) -> Result<Arc<states::States>, Fault> {
        let clock = self.opts.clock.clone();
        self.states
            .cache
            .get(&*clock, || states::gather::compute(self))
            .await
            .map_err(Fault::from)
    }
```

- [ ] **Step 6: Recolección, contexto y subconsulta**

`gather.rs`: `compute(native: &Native) -> Result<States, StateFault>` con los 8 pasos del inicio de la tarea; `RealEffects<'a> { native: &'a Native, inspector: Arc<PaneInspector> }` implementando `CardEffects` como se describe. La escritura final:

```rust
    // Paso 7: nada se escribe si algo de lo anterior declinó.
    let body = comandos_core::json::response_dumps(&Value::Array(items.clone()))
        .map_err(|_| StateFault::Decline)?;
    comandos_core::json::response_dumps(&models).map_err(|_| StateFault::Decline)?;
    // Paso 8: `write_app_tab_models`; un fallo se ignora, como su `except`.
    let path = native.options().hooks.join("app-tab-models.json");
    let _ = tokio::task::spawn_blocking(move || files::write_json_atomic(&path, &models)).await;
    Ok(States { items: Arc::new(items), body: bytes::Bytes::from(body) })
```

`subrequest.rs`: `get(addr, token, target, timeout)` = `forward::connect_paced(addr)` + `http1::handshake`, `GET <target>` con `Host: 127.0.0.1:<puerto>`, `X-Comandos-Token` (si hay token) y `Connection: close`, cuerpo recogido con `BodyExt::collect`; todo dentro de `tokio::time::timeout(timeout, …)`; `SubError::{Connect, Protocol, Timeout}`.

`context.rs`:

```rust
#[derive(Default)]
pub struct Context {
    slot: tokio::sync::Mutex<Option<(i64, Arc<SuggestContext>)>>,
}

impl Context {
    /// `_suggestion_context` (7069) con la vigencia del Python (> 60 s).
    pub async fn get(&self, opts: &NativeOptions, registry: &Value, now_ms: i64)
        -> Result<Arc<SuggestContext>, StateFault>
    {
        let mut slot = self.slot.lock().await;
        if let Some((at, ctx)) = slot.as_ref() {
            if now_ms - at <= 60_000 {
                return Ok(ctx.clone());
            }
        }
        let guard = match subrequest::get(opts.legacy, &opts.legacy_token, "/usage/guard", Duration::from_secs(10)).await {
            Ok((200, body)) => serde_json::from_slice(&body).unwrap_or_else(|_| json!({})),
            Ok(_) => json!({}),
            Err(_) => return Err(StateFault::Decline),
        };
        let routes = routes(opts, registry).await;
        let latency = match subrequest::get(opts.legacy, &opts.legacy_token, "/usage/analytics?days=7", Duration::from_secs(10)).await {
            Ok((200, body)) => serde_json::from_slice(&body).map(|v| latency_from(&v)).unwrap_or_default(),
            Ok(_) => Vec::new(),
            Err(_) => return Err(StateFault::Decline),
        };
        let ctx = Arc::new(SuggestContext { guard, routes, latency });
        *slot = Some((now_ms, ctx.clone()));
        Ok(ctx)
    }
}
```

`routes(opts, registry)` = `spawn_blocking` (cuentas + `which`) seguido del sondeo async del proxy; cualquier error → conjunto vacío (el `except` de 7087). Con `registry` `{}` (la prueba) el conjunto es vacío.

`compute` llama a `context.get` solo si `suggest::needs_context(&items, &motor)?`.

- [ ] **Step 7: Fixture de paridad**

En `xtask/parity/frente.jsonl`, en las líneas `state-local` y `state-remoto-con-token`: `"volatile":["/*/observedConfig/observedAt","/*/observedConfig/evidenceAt"]` y quitar `"forwarded":true`. Añadir:

```json
{"name":"d-state-consulta","method":"GET","path":"/state?x=1","headers":{"Host":"127.0.0.1"},"body":null,"volatile":["/*/observedConfig/observedAt","/*/observedConfig/evidenceAt"],"expect":"same"}
{"name":"d-state-prefijo","method":"GET","path":"/states","headers":{"Host":"127.0.0.1"},"body":null,"volatile":["/*/observedConfig/observedAt","/*/observedConfig/evidenceAt"],"expect":"same","forwarded":true}
```

- [ ] **Step 8: Verde**

Run: `$C test -p comandos-server --test dash_native_state` → 5 PASS.
Run: `$C test -p comandos-server` → PASS (las pruebas de la 2b que esperaban `/state` reenviado se actualizan a nativo: buscar `"/state"` en `crates/comandos-server/tests/` y cambiar la expectativa de reenvío por la de respuesta nativa).
Run: `$C build --release -p comandos-cli -j 6 && $C build -p xtask -j 6 && .build/target/debug/xtask parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks --state-db ~/.local/state/comandos/app-state.sqlite3 --usage-db ~/.claude/hooks/comandos-usage.sqlite --comandos .build/target/release/comandos`
Expected: 0 DIFF; `GET /state` ya no aparece entre las reenviadas (sí `GET /states`).

- [ ] **Step 9: Commit**

```bash
git add crates/comandos-server/src/dash/mod.rs crates/comandos-server/src/dash/forward.rs \
  crates/comandos-server/src/dash/native/mod.rs crates/comandos-server/src/dash/native/light.rs \
  crates/comandos-server/src/dash/native/lanes.rs crates/comandos-server/src/dash/native/subrequest.rs \
  crates/comandos-server/src/dash/native/states crates/comandos-server/tests/support/mod.rs \
  crates/comandos-server/tests/dash_native_state.rs
git add -f xtask/parity/frente.jsonl
git commit -m "feat(dash): GET /state nativo — recolección, caché de 1,2 s con vuelo único y escritura tras decidir

Escaneos en spawn_blocking, tmux por tokio::process en el orden del Python, base de
uso por su carril, contexto de sugerencias con el heredado (60 s) y app-tab-models.json
escrito solo si nada quedó incierto.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Rutas dependientes — `/workspace/sort` modo `by` y `/terminal/quick` en la barra

Desplegable sola. `by` usa `Native::states_cached`; la terminal rápida reclama y cierra en trabajos cortos del worker y espera/lanza fuera.

Comportamiento portado:

- **`/workspace/sort` `by`** (6497): `by` = `data.get("by")`: cadena → tal cual; ausente o `null` → `"<sin orden>"` (no está en `SORTS`: 400 `Orden desconocido` de `sort_groups`); otro tipo → declina. Antes del trabajo en la base: `states = native.states_cached().await?` (excepción 3a de los rulings). En el trabajo, por intento (máx. 3): `sync` → registro de pestañas para ordenar (archivo ausente → `{}`; JSON roto u otro `OSError` → declina, el texto de `JSONDecodeError` no se reproduce; no-objeto → 400 `app-tabs.json no es un objeto`) → favoritos (`read_prefs`) → `info` por sesión (`activeAt = max(float(ts or 0))`, `need`) + `label`/`fav` por pestaña → `comandos_core::workspace::layout::sort_groups(doc, by, &info)` → `commit` con `token_hex12()`; conflicto → reintentar; `Invalid` → 400 con su texto o `Orden inválido`; tras 3 → 409.
- **`/terminal/quick`** (5631, `lib/quick_terminal.py:121`): `place != "sidebar"` → declina (D7). `requestId` inválido → 400 sin tocar la base. Bucle: `claim` (trabajo del worker; reserva de carpeta dentro del mismo trabajo) → `Ready` → 200 `created:false`; `Own` → fuera del worker: `create_dir_all(cwd)`, `tmux has-session -t =<s>` (5 s), si no existe lanzar `scope + tmux new-session -d -s <s> -c <cwd> -P -F #{pane_id}` (15 s; `scope` = `systemd-run --user --scope --collect --quiet` si `NativeOptions.scope` existe), salida que empieza por `%` → `tmux set-option -p -t <pane> @comandos-pane-key <key>`; registro: nada (barra); `finish` (`ready`) → 200 `created:true`. `Wait` → si `clock ≥ deadline (15 s)` → 409 `{"error":"La terminal se está abriendo; reintenta en unos segundos","code":"busy","retryable":true}`; si no, `tokio::time::sleep(50 ms)` y repetir. Fallo de un paso tras `Own`: mensaje = `str(exc) or clase` (`RuntimeError((stderr or "tmux falló").strip())` para el lanzamiento; `TmuxError::python_message` para plazo/arranque con el argv completo; salida no UTF-8 → `UnicodeDecodeError`, diferencia aceptada), `finish("failed", mensaje[:500])`, 502 `{"error":"No se pudo abrir la terminal: <m>","code":"launch","retryable":true,"cwd":<cwd>}`. Carpeta imposible → 500 `code:"folder"`.

**Files:**
- Modify: `crates/comandos-runtime/src/quick_terminal.rs` (hacer públicos `Claim`, `Terminal` con sus campos, `claim` y `finish`; `open_quick_terminal` sigue igual por fuera y se reescribe sobre ellos)
- Modify: `crates/comandos-server/src/dash/native/workspace.rs`, `crates/comandos-server/src/dash/native/mod.rs`
- Create: `crates/comandos-server/src/dash/native/quick.rs`
- Modify: `crates/comandos-server/tests/dash_native_workspace.rs`; Create: `crates/comandos-server/tests/dash_native_quick.rs`
- Modify: `xtask/parity/frente.jsonl`

**Interfaces:**
- Consumes: `Native::states_cached` (Tarea 5), `workspace::{sync, store, payload, message_or, token_hex12}`, `light::{read_prefs, favorites_set}`, `comandos_runtime::quick_terminal::{valid_request_id, Options, default_base}`.
- Produces: `comandos_runtime::quick_terminal::{Claim::{Ready(Terminal), Own(Terminal), Wait}, Terminal { cwd, session, pane } + label() + result(bool), claim(&Connection, &str, &Options<'_>, &dyn Fn() -> f64) -> Result<Claim>, finish(&Connection, &str, &str, Option<&str>, &str, &dyn Fn() -> f64) -> Result<()>}`; `NativeOptions.{scope: Option<tmux::Program>, quick_base: PathBuf}`; `quick::{ROUTES, answer}`; `NativeRoute::QuickTerminal`.

- [ ] **Step 1: Pruebas que fallan**

En `crates/comandos-server/tests/dash_native_workspace.rs`, añadir (siguiendo el estilo de sus pruebas de `restore`, que ya montan el oráculo y el frente sobre el mismo HOME):

```rust
#[tokio::test(flavor = "current_thread")]
async fn sort_by_matches_python_oracle() {
    let home = TestHome::new("ws-sort-by");
    home.write("app-tabs.json", r#"{"beta": "Beta", "alfa": "Alfa"}"#);
    home.write("state/w.json", r#"{"session":"beta","status":"waiting","ts":5}"#);
    let Some(py) = oracle(&home).await else { return };
    let _ = get(py.port, "/workspace").await;
    let front = front(&home, py.port, home.options()).await;
    for by in [r#"{"by":"alpha"}"#, r#"{"by":"need"}"#, r#"{"by":"nada"}"#, r#"{}"#] {
        let expected = request_body(py.port, "POST", "/workspace/sort", "", by).await;
        let got = request_body(front.port, "POST", "/workspace/sort", "", by).await;
        assert_eq!((got.status, got.text()), (expected.status, expected.text()), "{by}");
    }
    front.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn sort_by_non_string_declines() {
    let home = TestHome::new("ws-sort-by-dec");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let got = request_body(front.port, "POST", "/workspace/sort", "", r#"{"by":["alpha"]}"#).await;
    assert_eq!(got.text(), r#"{"legacy": true}"#);
    front.stop().await;
}
```

(El oráculo y el frente comparten base: cada par de peticiones se compara en la misma revisión porque `sort` vuelve al mismo orden del mismo documento; si el Python commitea primero, el Rust ve esa revisión y produce el mismo cuerpo salvo `revision`. Para que los bytes coincidan, la prueba alterna: primero el frente con `{"by":"alpha"}`, luego deshace con `restore` vía el Python, y compara el cuerpo del frente con el del Python sobre el estado restaurado; la comparación es por `status` y por el documento sin `revision`.)

Crear `crates/comandos-server/tests/dash_native_quick.rs`:

```rust
//! POST /terminal/quick en la barra, nativo: reclamo en el worker, espera y
//! lanzamiento fuera de él.
mod support;
use serde_json::Value;
use std::{fs, time::Duration};
use support::{FakeLegacy, TestHome, dead_port, front, get, request_body, tmux_available};

fn opts(home: &TestHome) -> comandos_server::dash::native::NativeOptions {
    let mut o = home.options();
    o.quick_base = home.root.join("Terminal");
    o.scope = None;
    o
}

#[tokio::test(flavor = "current_thread")]
async fn quick_terminal_sidebar_opens_once_and_replays() {
    if !tmux_available() { return; }
    let home = TestHome::new("quick-open");
    let front = front(&home, dead_port(), opts(&home)).await;
    let body = r#"{"requestId":"req-1","place":"sidebar"}"#;
    let first = request_body(front.port, "POST", "/terminal/quick", "", body).await;
    assert_eq!(first.status, 200, "{}", first.text());
    let v: Value = serde_json::from_slice(&first.body).unwrap();
    assert_eq!(v["created"], Value::Bool(true));
    assert!(v["tabId"].as_str().unwrap().starts_with("term-q"));
    assert!(fs::metadata(v["cwd"].as_str().unwrap()).unwrap().is_dir());
    let again = request_body(front.port, "POST", "/terminal/quick", "", body).await;
    let w: Value = serde_json::from_slice(&again.body).unwrap();
    assert_eq!((w["created"].clone(), w["cwd"].clone()), (Value::Bool(false), v["cwd"].clone()));
    front.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn quick_terminal_concurrent_same_request_one_shell_worker_free() {
    if !tmux_available() { return; }
    let home = TestHome::new("quick-conc");
    // tmux lento en new-session: el segundo espera el reclamo mientras /workspace responde.
    let wrapper = home.root.join("bin/tmux-lento");
    fs::write(&wrapper, "#!/bin/sh\ncase \"$*\" in *new-session*) sleep 1;; esac\nexec tmux \"$@\"\n").unwrap();
    fs::set_permissions(&wrapper, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut o = opts(&home);
    o.tmux.program.path = wrapper;
    let front = front(&home, dead_port(), o).await;
    let port = front.port;
    let body = r#"{"requestId":"req-2","place":"sidebar"}"#;
    let a = tokio::spawn(async move { request_body(port, "POST", "/terminal/quick", "", body).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let b = tokio::spawn(async move { request_body(port, "POST", "/terminal/quick", "", body).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let started = std::time::Instant::now();
    assert_eq!(get(port, "/workspace").await.status, 200);
    assert!(started.elapsed() < Duration::from_millis(400), "el worker quedó retenido");
    let (a, b) = (a.await.unwrap(), b.await.unwrap());
    let (va, vb): (Value, Value) = (serde_json::from_slice(&a.body).unwrap(), serde_json::from_slice(&b.body).unwrap());
    assert_eq!(va["tabId"], vb["tabId"]);
    let created = [&va, &vb].iter().filter(|v| v["created"] == Value::Bool(true)).count();
    assert_eq!(created, 1);
    front.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn quick_terminal_outside_sidebar_and_invalid() {
    let home = TestHome::new("quick-dec");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, opts(&home)).await;
    let declined = request_body(front.port, "POST", "/terminal/quick", "", r#"{"requestId":"r"}"#).await;
    assert_eq!(declined.text(), r#"{"legacy": true}"#);
    let bad = request_body(front.port, "POST", "/terminal/quick", "", r#"{"requestId":"-x","place":"sidebar"}"#).await;
    assert_eq!((bad.status, bad.text().as_str()), (400, r#"{"error": "requestId inválido", "code": "request", "retryable": false}"#));
    front.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn quick_terminal_launch_failure_matches_python_text() {
    let home = TestHome::new("quick-fail");
    let mut o = opts(&home);
    o.tmux.program.path = "/no-existe/tmux".into();
    let front = front(&home, dead_port(), o).await;
    let got = request_body(front.port, "POST", "/terminal/quick", "", r#"{"requestId":"r9","place":"sidebar"}"#).await;
    assert_eq!(got.status, 502);
    let v: Value = serde_json::from_slice(&got.body).unwrap();
    assert_eq!(v["error"], Value::String("No se pudo abrir la terminal: [Errno 2] No such file or directory: 'tmux'".into()));
    assert_eq!((v["code"].clone(), v["retryable"].clone()), (Value::String("launch".into()), Value::Bool(true)));
    front.stop().await;
}
```

- [ ] **Step 2: Ver el fallo**

Run: `$C test -p comandos-server --test dash_native_quick --test dash_native_workspace`
Expected: FAIL (`quick_base` no existe; `by` declina).

- [ ] **Step 3: Runtime — reclamo y cierre públicos**

En `crates/comandos-runtime/src/quick_terminal.rs`: `pub struct Terminal { pub cwd: String, pub session: String, pub pane: String }` con `pub fn label(&self)` y `pub fn result(&self, created: bool) -> Value`; `pub enum Claim`; `pub fn claim(conn, id, options, clock: &dyn Fn() -> f64)` (el cuerpo actual, con `clock()` en lugar de `(callbacks.clock)()`); `pub fn finish(...)` sin cambios de cuerpo. `open_quick_terminal` llama a `claim(conn, id, options, callbacks.clock)`. Run: `$C test -p comandos-runtime --test quick_terminal` → PASS.

- [ ] **Step 4: `quick.rs`**

```rust
//! POST `/terminal/quick` (9520, `quick_terminal_request` 5631) solo en la
//! barra lateral (D7). El worker solo ve trabajos cortos (reclamo, cierre).
use super::{Answer, Entry, Fault, Key, Native, NativeRoute, Verb, light::data, reply, tmux::{RunError, run_program}};
use crate::{HandlerError, Request};
use comandos_runtime::quick_terminal::{Claim, Options, POLL_SECONDS, WAIT_SECONDS, Terminal, claim, finish, valid_request_id};
use http::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

pub const ROUTES: &[Entry] = &[Entry { verb: Verb::Post, key: Key::Raw("/terminal/quick"), route: NativeRoute::QuickTerminal }];

fn failure() -> Fault { Fault::Error(HandlerError::Failure) }

fn seconds(native: &Native) -> f64 { (native.options().clock)() as f64 / 1000.0 }

pub async fn answer(native: &Native, request: &Request) -> Answer {
    let data = data(request)?;
    if data.get("place") != Some(&Value::String("sidebar".into())) {
        return Err(Fault::Decline);
    }
    let raw = data.get("requestId").cloned().unwrap_or(Value::Null);
    if !valid_request_id(&raw) {
        return reply(StatusCode::BAD_REQUEST, &json!({"error": "requestId inválido", "code": "request", "retryable": false}));
    }
    let id = raw.as_str().ok_or_else(failure)?.to_owned();
    let deadline = seconds(native) + WAIT_SECONDS;
    let terminal = loop {
        let (base, clock) = (native.options().quick_base.clone(), native.options().clock.clone());
        let now = chrono::DateTime::from_timestamp_millis(clock())
            .ok_or_else(failure)?
            .fixed_offset();
        let id2 = id.clone();
        let claimed = native
            .with_state(move |b| claim(&b.conn, &id2, &Options::new(&base, now), &|| clock() as f64 / 1000.0))
            .await?;
        match claimed {
            Ok(Claim::Ready(t)) => return reply(StatusCode::OK, &t.result(false)),
            Ok(Claim::Own(t)) => break t,
            Ok(Claim::Wait) => {
                if seconds(native) >= deadline {
                    return reply(StatusCode::CONFLICT, &json!({"error": "La terminal se está abriendo; reintenta en unos segundos", "code": "busy", "retryable": true}));
                }
                tokio::time::sleep(Duration::from_secs_f64(POLL_SECONDS)).await;
            }
            Err(comandos_runtime::quick_terminal::Error::Quick(q)) if q.code == "folder" => {
                return reply(StatusCode::INTERNAL_SERVER_ERROR, &json!({"error": q.message, "code": "folder", "retryable": q.retryable}));
            }
            Err(_) => return Err(failure()),
        }
    };
    let outcome = launch(native, &terminal).await;
    let (state, stored) = match &outcome {
        Ok(()) => ("ready", None),
        Err(m) => ("failed", Some(m.chars().take(500).collect::<String>())),
    };
    let (id2, cwd, clock) = (id.clone(), terminal.cwd.clone(), native.options().clock.clone());
    native
        .with_state(move |b| finish(&b.conn, &id2, state, stored.as_deref(), &cwd, &|| clock() as f64 / 1000.0))
        .await?
        .map_err(|_| failure())?;
    match outcome {
        Ok(()) => reply(StatusCode::OK, &terminal.result(true)),
        Err(message) => reply(StatusCode::BAD_GATEWAY, &json!({
            "error": format!("No se pudo abrir la terminal: {message}"),
            "code": "launch", "retryable": true, "cwd": terminal.cwd,
        })),
    }
}

/// `os.makedirs` + `exists` + `launch` (5601-5615); el texto es `str(exc) or clase`.
async fn launch(native: &Native, t: &Terminal) -> Result<(), String> {
    let cwd = t.cwd.clone();
    tokio::task::spawn_blocking(move || std::fs::create_dir_all(&cwd))
        .await
        .map_err(|_| "JoinError".to_owned())?
        .map_err(|e| python_os_error(&e))?;
    let tmux = &native.options().tmux;
    let exists = tmux
        .run(&["has-session", "-t", &format!("={}", t.session)])
        .await
        .map_err(|e| e.python_message().unwrap_or_else(|| "UnicodeDecodeError".into()))?;
    if exists.ok {
        return Ok(());
    }
    let mut program = tmux.program.clone();
    let mut args: Vec<String> = vec!["new-session".into(), "-d".into(), "-s".into(), t.session.clone(),
        "-c".into(), t.cwd.clone(), "-P".into(), "-F".into(), "#{pane_id}".into()];
    if let Some(scope) = &native.options().scope {
        // `systemd-run --user --scope --collect --quiet tmux …`.
        let mut prefixed = vec![program.path.to_string_lossy().into_owned()];
        prefixed.extend(program.prefix.iter().map(|p| p.to_string_lossy().into_owned()));
        prefixed.extend(args);
        args = prefixed;
        program = scope.clone();
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = match run_program(&program, &refs, Duration::from_secs(15)).await {
        Ok(out) => out,
        Err(RunError::Timeout) => return Err(timed_out(&program, &refs)),
        Err(RunError::Spawn(e)) => return Err(spawn_message(&program, &e)),
        Err(RunError::Decode) => return Err("UnicodeDecodeError".into()),
    };
    if !out.ok {
        let text = if out.stderr.is_empty() { "tmux falló" } else { out.stderr.as_str() };
        return Err(super::py::strip(text).to_owned());
    }
    let pane = super::py::strip(&out.stdout).to_owned();
    if pane.starts_with('%') {
        // El resultado del set-option no cuenta (el Python lo ignora); su plazo sí.
        tmux.run(&["set-option", "-p", "-t", &pane, "@comandos-pane-key", &t.pane])
            .await
            .map_err(|e| e.python_message().unwrap_or_else(|| "UnicodeDecodeError".into()))?;
    }
    Ok(())
}
```

`python_os_error(e)` = `"[Errno N] <strerror>: '<ruta>'"` (`e.raw_os_error()` + `comandos_runtime` ya tiene `io_message`; aquí se forma el texto completo de `OSError.__str__`); `timed_out(program, args)` = `"Command '[<repr de cada elemento del argv completo>]' timed out after 15 seconds"` con `py::repr_ascii`; `spawn_message(program, e)` = `"[Errno 2] No such file or directory: '<argv[0]>'"` para `NotFound` (otros → `e.to_string()` sin el sufijo `(os error N)`). Añadir `NativeOptions.scope` (`for_home`: `Program::named("systemd-run")` si `providers::which("systemd-run", PATH, home)` existe, si no `None`; `TestHome::options`: `None`) y `quick_base` (`for_home`: `quick_terminal::default_base(home, env COMANDOS_QUICK_TERMINAL_BASE)`; `TestHome::options`: `<root>/Terminal`). `NativeRoute::QuickTerminal`, `quick::ROUTES` en `TABLES`, rama en `answer`.

- [ ] **Step 5: `workspace.rs` — `by`**

En la rama `WorkspaceRoute::Sort`, cuando `restore` no es lista:

```rust
            let by = match data.get("by") {
                None | Some(Value::Null) => "<sin orden>".to_owned(),
                Some(Value::String(s)) => s.clone(),
                Some(_) => return Err(Fault::Decline),
            };
            // Excepción 3a: la escritura de app-tab-models.json es idempotente.
            let states = native.states_cached().await?;
            run(native, move |b| {
                for _ in 0..3 {
                    let current = sync(b, &hooks, now_seconds)?;
                    let previous = group_ids(&current.document)?;
                    let labels = match sort_registry(&hooks)? {
                        Ok(labels) => labels,
                        Err(message) => return Ok((StatusCode::BAD_REQUEST, json!({"error": message}))),
                    };
                    let favorites = favorites_set(&read_prefs(&hooks)?)?;
                    let info = sort_info(&states.items, &current.document, &labels, &favorites)?;
                    let wanted = match sort_groups(&current.document, &by, &info) {
                        Ok(w) => w,
                        Err(e) => return Ok((StatusCode::BAD_REQUEST, message_or(e, "Orden inválido"))),
                    };
                    let store = store(&b.conn)?;
                    match store.commit(&json!(current.revision), &wanted, &token_hex12()?, "user", now_seconds) {
                        Ok(saved) => {
                            let mut body = payload(&saved);
                            if let Some(map) = body.as_object_mut() {
                                map.insert("previous".into(), Value::Array(previous));
                            }
                            return Ok((StatusCode::OK, body));
                        }
                        Err(WsError::Conflict { .. }) => continue,
                        Err(WsError::Invalid(m)) => return Ok((StatusCode::BAD_REQUEST, message_or(m, "Orden inválido"))),
                        Err(_) => return Err(failure()),
                    }
                }
                Ok((StatusCode::CONFLICT, json!({"error": "El acomodo cambió mientras ordenaba; intenta de nuevo"})))
            })
            .await
```

`group_ids` es el cálculo de `previous` que ya existe en la rama `restore` (extraerlo a función y usarlo en ambas). `sort_registry(hooks) -> Result<Result<Vec<(String, String)>, &'static str>, Fault>` (`_tab_registry` 6466: `Missing` → vacío; `Value(Object)` → pares cadena no vacía; `Value(_)` → `Err("app-tabs.json no es un objeto")`; `Unreadable`/`Unsure` → `Fault::Decline`). `sort_info` construye `{tab: {"activeAt": f64, "need": bool, "label": …, "fav": bool}}` en el orden del Python (sesiones de las tarjetas primero, `setdefault` para pestañas); `float(row.get("ts") or 0)` imposible → `Fault::Decline` (en el Python está dentro del `try` de 6506 y responde 400 con el texto de `ValueError`/`TypeError`, que no se reproduce; en la práctica no ocurre: las tarjetas de `/state` ya llevan `ts` validado por `RecordCache`).

- [ ] **Step 6: Fixture**

```json
{"name":"d-sort-by-desconocido","method":"POST","path":"/workspace/sort","headers":{"Host":"127.0.0.1","X-Comandos-Token":"{{token}}"},"body":{"by":"nada"},"volatile":[],"expect":"same"}
{"name":"d-quick-invalido","method":"POST","path":"/terminal/quick","headers":{"Host":"127.0.0.1","X-Comandos-Token":"{{token}}"},"body":{"requestId":"-x","place":"sidebar"},"volatile":[],"expect":"same"}
{"name":"d-quick-barra","method":"POST","path":"/terminal/quick","headers":{"Host":"127.0.0.1","X-Comandos-Token":"{{token}}"},"body":{"requestId":"parity-1","place":"sidebar"},"volatile":["/cwd","/label"],"expect":"same"}
```

(En la pila aislada el `fakebin` trae un `systemd-run` vacío en el `PATH` de los dos lados: ambos «lanzan» sin abrir tmux y responden `created:true`.)

- [ ] **Step 7: Verde**

Run: `$C test -p comandos-server --test dash_native_quick --test dash_native_workspace` → PASS. `$C test --workspace -j 6` → PASS. `xtask parity` (comando del Step 8 de la Tarea 5) → 0 DIFF.

- [ ] **Step 8: Commit**

```bash
git add crates/comandos-runtime/src/quick_terminal.rs crates/comandos-server/src/dash/native/mod.rs \
  crates/comandos-server/src/dash/native/workspace.rs crates/comandos-server/src/dash/native/quick.rs \
  crates/comandos-server/tests/dash_native_workspace.rs crates/comandos-server/tests/dash_native_quick.rs \
  crates/comandos-server/tests/support/mod.rs
git add -f xtask/parity/frente.jsonl
git commit -m "feat(dash): /workspace/sort con by y /terminal/quick de la barra, nativos

by usa la caché de /state; la terminal rápida reclama y cierra en trabajos cortos
del worker y espera y lanza fuera; fuera de la barra se reenvía.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Medición (latencia por ruta) y documento de cutover «2d»

**Files:**
- Modify: `xtask/src/poll.rs` (latencia por ruta)
- Modify: `docs/verification/cutover-dash.md` (sección nueva al final)

Criterio de aceptación de la 2d (va al documento y se mide antes del cutover):

- **Memoria del frente**: `xtask poll --shadow --minutes 10` con nativo: Pss del frente desde el minuto 5 con `max − min ≤ 1 MiB` y pendiente `≤ 1024 KiB/h`.
- **Memoria del heredado en la pila aislada**: crecimiento del minuto 1 al 10 `≤ 50 %` del de la misma corrida con `--no-native` (la 2b midió 45 → 273 MiB con `/state` reenviado).
- **Latencia de GET `/state`**: p95 nativo `≤` p95 con `--no-native` en la misma máquina, y p99 nativo `≤ 1000 ms`.

- [ ] **Step 1: Prueba que falla del percentil**

En `xtask/src/poll.rs`, dentro de su `mod tests`:

```rust
    #[test]
    fn percentile_nearest_rank() {
        let mut v = vec![50, 10, 40, 20, 30];
        assert_eq!(percentile(&mut v, 50.0), 30);
        assert_eq!(percentile(&mut v, 95.0), 50);
        assert_eq!(percentile(&mut Vec::new(), 95.0), 0);
    }
```

Run: `$C test -p xtask percentile` → FAIL (`percentile` no existe).

- [ ] **Step 2: Implementar**

```rust
/// Percentil por rango más cercano (ms); 0 sin muestras.
pub fn percentile(samples: &mut [u32], p: f64) -> u32 {
    if samples.is_empty() {
        return 0;
    }
    samples.sort_unstable();
    let rank = ((p / 100.0) * samples.len() as f64).ceil() as usize;
    samples.get(rank.saturating_sub(1).min(samples.len() - 1)).copied().unwrap_or(0)
}
```

En `Stats` añadir `latency: std::sync::Mutex<std::collections::BTreeMap<String, Vec<u32>>>` (inicializado vacío). En el hilo periódico, alrededor de la llamada `http(...)`: `let t = Instant::now();` antes y, tras obtener el resultado (sea cual sea), `s1.latency.lock().unwrap_or_else(|p| p.into_inner()).entry(format!("{method} {}", path_only(&path))).or_default().push(t.elapsed().as_millis().min(u128::from(u32::MAX)) as u32);`. Al final, tras la línea de «peticiones: …»:

```rust
    println!("\nlatencia por ruta (ms): p50 p95 p99 n");
    let mut table = stats.latency.lock().unwrap_or_else(|p| p.into_inner());
    for (route, samples) in table.iter_mut() {
        let n = samples.len();
        let (p50, p95, p99) = (percentile(samples, 50.0), percentile(samples, 95.0), percentile(samples, 99.0));
        println!("  {route:<28} {p50:>5} {p95:>5} {p99:>5} {n:>5}");
    }
```

Run: `$C test -p xtask` → PASS.

- [ ] **Step 3: Corrida completa y medición**

```bash
$C fmt --all -- --check
$C clippy --workspace --all-targets -j 6 -- -D warnings
$C test --workspace -j 6
$C build --release -p comandos-cli -j 6 && $C build -p xtask -j 6
.build/target/debug/xtask parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 --usage-db ~/.claude/hooks/comandos-usage.sqlite \
  --comandos .build/target/release/comandos
.build/target/debug/xtask poll --shadow --minutes 10 --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 --usage-db ~/.claude/hooks/comandos-usage.sqlite \
  --comandos .build/target/release/comandos | tee /tmp/poll-2d-nativo.txt
.build/target/debug/xtask poll --shadow --no-native --minutes 10 --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 --usage-db ~/.claude/hooks/comandos-usage.sqlite \
  --comandos .build/target/release/comandos | tee /tmp/poll-2d-sin-nativo.txt
```

Expected: 0 fallos, 0 DIFF, `GET /state` fuera de las reenviadas en la corrida nativa, y los tres criterios de arriba. Anotar los números para la subsección «Medido antes del cutover». Si un criterio falla, no se escribe el documento: se abre `superpowers:systematic-debugging` sobre la medición (Pss por `smaps_rollup` de los dos procesos y la tabla de latencias).

- [ ] **Step 4: Añadir la sección**

Al final de `docs/verification/cutover-dash.md`:

````markdown
## 2d: `/state` nativo y terminal rápida

Procedimiento para el controlador, como el de la 2c. El frente está en 4777 y el Python heredado
en 4781; la 2d solo cambia el binario del frente. Ni el Python ni las unidades ni tmux se tocan.

### Qué cambia

| Ruta | Nativa | Sigue en el Python |
|---|---|---|
| GET `/state` (y `/state?…`) | sí | cuando algo es incierto (abajo); `/states` y `/state/…` |
| POST `/workspace/sort` | `restore` (2b) y `by` | `by` que no es cadena; `app-tabs.json` ilegible |
| POST `/terminal/quick` | `place: "sidebar"` | el resto (registra pestaña: candado de proceso del Python) |
| POST `/workspace/close-group` | no | todo (`close_app_tab`) |

GET `/state` escribe `~/.claude/hooks/app-tab-models.json` como el Python, solo cuando la respuesta
es nativa. Se reenvía entero (sin escribir) si un `~/.claude/hooks/state/*.json` no se lee con
certeza, si `motor-results.json` está ilegible, si el registro de proveedores no valida, si el
contexto de sugerencias está vencido y el heredado no responde, o por los demás casos de la
decisión D2 del plan `docs/superpowers/plans/2026-10-04-fase-2d-state-nativo.md`.

### Diferencias y comportamientos aceptados (2d)

- El rastreador de configuración (`StateTracker`) es del frente: tras reiniciar el frente, la
  primera observación de cada pane equivale a un reinicio del Python («la evidencia que cambió
  gana» empieza de cero).
- `guard` y `latency` de las sugerencias los da el heredado (GET `/usage/guard`, GET
  `/usage/analytics?days=7`) cada 60 s y solo cuando alguna tarjeta los necesita; `routes` lo
  calcula el frente. Los dos procesos refrescan su contexto en instantes distintos.
- El Python sigue escribiendo `app-tab-models.json` cuando otra de sus rutas llama a
  `read_states_cached` (cambio de modelo, ordenar con `by` reenviado): escritura atómica, gana la
  última, las dos son proyección del mismo estado vivo.
- `/terminal/quick`: una salida no UTF-8 de tmux tras reservar la carpeta responde
  `No se pudo abrir la terminal: UnicodeDecodeError` (el Python daría el texto del códec).

### 0. Previos

```sh
cd ~/codebase/0xJesus/ComandOS && git log -1 --oneline        # main con la Fase 2d fusionada
CARGO_TARGET_DIR=$PWD/.build/target nice -n 10 cargo build --release -p comandos-cli -j 6
CARGO_TARGET_DIR=$PWD/.build/target nice -n 10 cargo build -p xtask -j 6
NEW=$HOME/codebase/0xJesus/ComandOS/.build/target/release/comandos
XT=$HOME/codebase/0xJesus/ComandOS/.build/target/debug/xtask
grep -qa 'GET /pomodoro, GET /sovereignty y GET /state' "$NEW" || echo "BINARIO SIN 2d: no seguir"
systemctl --user is-active cc-dash.service cc-dash-legacy.service    # active active
~/.local/share/comandos/bin/comandos install --releases              # anotar la actual ('*')
for u in cc-dash.service cc-dash-legacy.service; do
  systemctl --user show -p Environment -p WorkingDirectory "$u"; done # mismo PATH y WorkingDirectory;
                                                                      # COMANDOS_QUICK_TERMINAL_BASE,
                                                                      # CODEX_HOME, GROK_HOME iguales o ausentes
ss -ltn 'sport = :4782'                                               # libre
```

### 1. Sombra en 4782 con nativo, contra el heredado 4781

La sombra usa los archivos reales: su GET `/state` escribe `app-tab-models.json` (lo mismo que
escribiría el Python) y su `/terminal/quick` abre una terminal de verdad (probarla solo a mano).

```sh
COMANDOS_DASH_TRACE_FORWARD=1 "$NEW" dash 4782 --legacy-port 4781 2> /tmp/sombra-2d.log
```

Desde otra terminal, en `~/codebase/0xJesus/ComandOS`:

```sh
"$XT" parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 \
  --usage-db ~/.claude/hooks/comandos-usage.sqlite --comandos "$NEW"        # 0 DIFF
T=$(cat ~/.claude/hooks/dash-token)
norm() { python3 -c 'import json,sys
v=json.load(sys.stdin)
for i in v:
    o=i.get("observedConfig") or {}
    for k in ("observedAt","evidenceAt"):
        if k in o: o[k]="<volátil>"
print(json.dumps(v))'; }
for i in 1 2 3; do
  A=$(curl -s -H "X-Comandos-Token: $T" 127.0.0.1:4782/state | norm)
  B=$(curl -s -H "X-Comandos-Token: $T" 127.0.0.1:4781/state | norm)
  [ "$A" = "$B" ] && echo "igual /state" || echo "DISTINTO /state (repetir: un turno pudo cambiar entre lecturas)"
  sleep 2
done
for r in /state /prefs; do
  curl -s -o /dev/null -w "$r %{time_total}s\n" -H "X-Comandos-Token: $T" 127.0.0.1:4782$r
done
```

`norm` es una línea de inspección en la terminal del controlador, no código del repositorio.
Navegación manual en `http://127.0.0.1:4782` con `chrome-bg` (vía `cc-browser-expose start 4782`):
el tablero muestra las mismas tarjetas que en 4777 (modelos, cuentas, sugerencias, estado
waiting/working de un turno en curso); «Ordenar» por nombre, recientes y pendientes, y Deshacer;
abrir una terminal rápida desde la barra de comandos dos veces seguidas (una sola). Luego:

```sh
grep -c 'se reenvían al heredado\|rutas nativas desactivadas' /tmp/sombra-2d.log   # 0
grep 'reenvío' /tmp/sombra-2d.log | sed 's/.*reenvío //' | sort | uniq -c | sort -rn
```

`GET /state` no debe aparecer (o solo de forma aislada, con un registro incierto en ese momento).
Ctrl+C.

### 2. Cutover

```sh
"$NEW" install --stage
grep -qa 'GET /pomodoro, GET /sovereignty y GET /state' "$(readlink -f ~/.local/share/comandos/bin/comandos)" || echo "STAGE MALO: ~/.local/share/comandos/bin/comandos install --rollback-release"
~/.local/share/comandos/bin/comandos hook claude-status >/dev/null && echo "hooks OK con la release nueva"
systemctl --user restart cc-dash.service
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:4777/        # 200
journalctl --user -u cc-dash.service --since -2min --no-pager | grep -c 'se reenvían al heredado\|rutas nativas desactivadas'   # 0
```

Sin un cambio de modelo o cuenta en vuelo en el momento del reinicio. El tablero queda sin
respuesta unos 3 s; cc-app y cc-notifyd reintentan solos.

### 3. Verificación

```sh
systemctl --user set-environment COMANDOS_DASH_TRACE_FORWARD=1 && systemctl --user restart cc-dash.service
sleep 600
journalctl --user -u cc-dash.service --since -11min --no-pager | grep 'reenvío' \
  | sed 's/.*reenvío //' | sort | uniq -c | sort -rn
systemctl --user unset-environment COMANDOS_DASH_TRACE_FORWARD && systemctl --user restart cc-dash.service
P=$(systemctl --user show -p MainPID --value cc-dash.service)
L=$(systemctl --user show -p MainPID --value cc-dash-legacy.service)
grep Pss /proc/$P/smaps_rollup; grep Pss /proc/$L/smaps_rollup     # al minuto 1 y al 10
```

- `GET /state` no aparece en la traza (o menos de 1 de cada 100 sondeos); `GET /usage/guard` y
  `GET /usage/analytics` aparecen como mucho una vez por minuto cada una.
- Pss del frente plano (± 1 MiB entre el minuto 1 y el 10); el heredado crece menos que antes del
  cutover en el mismo intervalo.
- Una pestaña nueva, un turno que pasa a «esperando» y un cambio de modelo se reflejan en el
  tablero y en cc-app en ≤ 3 s; las popups de cc-notifyd siguen llegando.

### 4. Reversión

Igual que en la 2c. A/B sin cambiar binario:

```sh
systemctl --user set-environment COMANDOS_DASH_NATIVE=0 && systemctl --user restart cc-dash.service
# persistente: drop-in ~/.config/systemd/user/cc-dash.service.d/no-native.conf con
# [Service]\nEnvironment=COMANDOS_DASH_NATIVE=0, daemon-reload y restart (deshacer: rm + daemon-reload + restart)
```

Volver a la release anterior (la 2c):

```sh
~/.local/share/comandos/bin/comandos install --releases
~/.local/share/comandos/bin/comandos install --rollback-release
systemctl --user restart cc-dash.service
grep -qa 'GET /pomodoro, GET /sovereignty y GET /state' "$(readlink -f ~/.local/share/comandos/bin/comandos)" && echo "SIGUE LA 2d"
```

`app-tab-models.json`, las filas de `quick_terminal_requests` y el orden del workspace tienen el
mismo formato que escribe el Python: revertir no necesita limpiar nada.

### Medido antes del cutover

Rama de la 2d, binario release, arnés en namespace de red privado, copias de `~/.claude/hooks`,
`app-state.sqlite3` y `comandos-usage.sqlite`.

- Suite del workspace: N pruebas, 0 fallos.
- `xtask parity`: N OK, 0 DIFF, 0 SKIP. Reenviadas: (lista).
- `xtask poll --shadow --minutes 10` con nativo: Pss del frente minuto 1 → 10 (KiB), desde el
  minuto 5 min–max y pendiente; heredado minuto 1 → 10; latencia de `GET /state` p50/p95/p99.
- Lo mismo con `--no-native`: heredado minuto 1 → 10; latencia de `GET /state` p50/p95/p99.
- Criterios: frente plano (sí/no), heredado ≤ 50 % del crecimiento sin nativo (sí/no), p95 nativo ≤
  p95 sin nativo y p99 ≤ 1000 ms (sí/no).
````

Sustituir cada `N`, «(lista)» y «(sí/no)» de «Medido antes del cutover» por los números y resultados reales del Step 3 antes de guardar (son mediciones, no diseño).

- [ ] **Step 5: Verificar el documento**

Run: `grep -n '^## 2d' docs/verification/cutover-dash.md` → una línea. Revisar que cada orden usa `"$NEW"`/`"$XT"` con ruta explícita, que la cadena de `grep -qa` existe en el binario (`grep -qa 'GET /pomodoro, GET /sovereignty y GET /state' .build/target/release/comandos && echo ok`) y que «Medido antes del cutover» no conserva ningún `N` ni «(sí/no)».

- [ ] **Step 6: Commit**

```bash
git add xtask/src/poll.rs docs/verification/cutover-dash.md
git add -f docs/superpowers/plans/2026-10-04-fase-2d-state-nativo.md
git commit -m "docs(verification): cutover 2d — /state nativo, terminal rápida y criterio de memoria y latencia

xtask poll mide p50/p95/p99 por ruta; sombra, verificación, A/B con --no-native y
reversión por env/drop-in o --rollback-release.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Self-review

- **Cobertura de los rulings**: 1 → cada tarea compara bytes con el Python (capas en 1–4, ruta en 5–6) y `app-tab-models.json` se compara byte a byte (Tareas 4 y 5); 2 → `latest_session_config` por `Native.usage`, reclamos por el worker (Tarea 6); 3 → paso 7 antes del 8 en `gather::compute`, prueba `state_unsure_record_declines_without_writing`, excepciones 3a/3b escritas; 4 → plazos 5/2/3/15 s en `RealEffects` y `quick.rs`; 5 → `spawn_blocking` para escaneos, inspector, lectores y escritura, prueba `state_hung_tmux_answers_504_runtime_free`; 6 → `cache.rs` con `state_flight_survives_leader_cancel`; 7 → `Key::Path("/state")`, `Key::Raw` para las POST; 8 → fixture `state-*`/`d-*` y oráculos por ruta; 9 → Tarea 7; 10 → Global Constraints; 11 → orden 1–3, 4, 5, 6, 7.
- **Alcance pedido**: `read_states`, `read_states_cached`, `write_app_tab_models`, lectores de `tui_state`/`pane_snapshot` (Tareas 1–5); `/terminal/quick` sin retener el worker (Tarea 6, Review Focus 5); `/workspace/sort` `by` (Tarea 6); `/workspace/close-group` queda reenviado con su razón (D8); criterio de memoria y latencia (Tarea 7). Nada de esto existía en `comandos-runtime` (comprobado: no hay `tui_state`, `pane_snapshot` ni procesos de agentes; sí `quick_terminal`, `accounts`, `model_catalog`, que se reutilizan).
- **Placeholders**: ninguno salvo «Medido antes del cutover», que el Step 4 de la Tarea 7 exige rellenar con las mediciones del Step 3.
- **Tipos**: `StateFault` (Tarea 4) es el error de `RecordCache::scan`, `cards::build`, `CardEffects`, `StatesCache::get` y `Context::get`; `Unsure` (Tarea 1) se convierte en `StateFault::Decline`; `Native::states_cached` devuelve `Result<Arc<States>, Fault>` y lo usan `states::answer` y la rama `by`; `Obs` es `Map<String, Value>` en todas las capas; `AgentInfo`, `PaneRow`, `AgentProc`, `AgentMaps` (Tarea 2) son los que consume `cards::Inputs`.
- **Review Focus**: las cinco líneas tienen su prueba en la tarea dueña (5, 5, 5, 5, 6).
