# Fase 2c — Dominios nativos II: plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** que el frente Rust `comandos dash` (4777, release `2bae7f9cd7d6` con la 2b en producción) responda él mismo, byte a byte igual que el `cc-dash` Python, las rutas de tres dominios más — F (snippets, registro de uso, Pomodoro, catálogos), G (terminal: paneles, historial, tecleo) y H (estado de operaciones de sesión) — sin tocar el Python, que sigue detrás como oráculo y destino de todo lo demás.

**Architecture:** la infraestructura de la 2b (clase `Native`, `Decline`, `BackendWorker<StateBackend>`, kit `py`/`tmux`/`files`/`query`, arnés `xtask parity` con `--state-db`, `support::oracle`) se reutiliza sin cambios de contrato. Esta fase añade módulos de dominio en `crates/comandos-server/src/dash/native/` y dos piezas que los dominios necesitan: **carriles de base** (`native/lanes.rs`: un `BackendWorker` por archivo SQLite extra — la base de uso `comandos-usage.sqlite` y el journal `session-operations.sqlite3` —, cada uno con su puerta de esquema y su apagado propio, que solo reenvía las rutas que lo usan) y un **puente síncrono a tmux** (`Tmux::run_blocking`) para las librerías de `comandos-runtime`, que llaman a tmux con callbacks síncronos: corren en un hilo de bloqueo y cada llamada la conduce el runtime del frente con el mismo `tokio::process` y el mismo plazo de 5 s. Lo que el Rust no reproduce con certeza (estado en memoria del Python, inspección de procesos, red) se declina antes de cualquier efecto o se deja reenviado, con la razón escrita en este plan.

**Tech Stack:** Rust 1.96 (edition 2024, `unsafe_code = "forbid"`; `std::fs::File::try_lock` para `flock`), tokio 1.53 (`rt`, `process`; `Handle::block_on` desde `spawn_blocking`), hyper 1.11, rusqlite 0.40 (bundled), serde_json con `arbitrary_precision` + `preserve_order`, crates `comandos-core`/`comandos-store`/`comandos-runtime`; arnés `xtask parity`/`xtask poll` en un netns (`unshare -Urn`).

**Spec:** `docs/superpowers/specs/2026-10-04-comandos-rust-complete-design.md` (§4.2, §5, §7, Enmiendas 1–8). Inventario: `docs/research/2026-10-04-fase-2-inventario.md` (§1.3, §1.4, §1.5, §1.6, §1.7, §1.11, §3.3, §7, §9). Plan previo: `docs/superpowers/plans/2026-10-04-fase-2b-dominios-nativos-i.md`. Procedimiento vivo: `docs/verification/cutover-dash.md` (secciones 2a y 2b). Oráculo: `bin/cc-dash` de este checkout (las líneas citadas abajo son las suyas; las del inventario difieren en ~100).

---

## Rulings del controlador que fijan este plan

1. **Respuestas idénticas byte a byte** vía `comandos_core::json::response_dumps` (status, `Content-Type`, `Cache-Control: no-store`, cuerpo); los textos de error son los del Python, literalmente.
2. **SQLite solo a través de workers**: `app-state` por el `BackendWorker<StateBackend>` existente. La base de uso (`comandos-usage.sqlite`) y el journal de operaciones van por un **segundo y un tercer worker** (`Lane<UsageBackend>`, `Lane<JournalBackend>`) con la misma disciplina: puerta de esquema antes de abrir y antes de cada trabajo (migraciones de `comandos_store::usage` contra `USAGE_SCHEMA_VERSION = 11` de `bin/cc_usage.py:177`), y apagado fail-closed **por base**, no global: una base de uso más nueva reenvía solo las rutas que la usan. Nunca se abren desde el hilo del runtime.
3. **`Decline` antes de cualquier efecto**; declinar reenvía la petición original al Python.
4. **tmux y procesos por `tokio::process`** con los plazos del Python (`native/tmux.rs`; `tmux()` de `bin/cc-dash:5833`, 5 s). Las librerías síncronas de `comandos-runtime` usan el mismo ejecutor a través de `Tmux::run_blocking`.
5. **Tabla nativa**: coincidencia exacta sobre la ruta cruda, por método; `--no-native` y `COMANDOS_DASH_NATIVE=0` sin cambios.
6. **Paridad**: líneas de fixture por ruta (`f-*`, `g-*`, `h-*`, punteros `volatile`); el arnés corre en el netns con `--state-db` y gana `--usage-db` (copia de solo lectura por la API de backup de SQLite, como `--state-db`); el oráculo Python de las pruebas (`support::oracle`) cubre cada ruta y cada caso de error.
7. **Documento de cutover**: sección nueva «2c: dominios nativos II» en `docs/verification/cutover-dash.md` con los mismos comandos de ruta explícita que la 2b (`NEW=…/.build/target/release/comandos`, `grep -qa`, stage → humo del hook → restart; reversión por env/drop-in o `--rollback-release`), la sombra `"$NEW" dash 4782 --legacy-port 4781` y verificación por dominio.
8. Comentarios en español, identificadores en inglés, sin `unsafe`, sin `unwrap`/`expect`/indexado en código que no sea de prueba, clippy `-D warnings`, rustfmt, trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, `CARGO_TARGET_DIR=<checkout>/.build/target`, `nice -n 10 cargo … -j 6`, nunca `git add -A`, `git add -f` para `docs/superpowers` y `*.jsonl`; cero Python o bash nuevos; las pruebas nunca tocan el HOME, las bases, el tmux ni los puertos 4777–4782 reales.
9. **Tareas desplegables una a una**, en este orden: F (solo lectura y archivos, menor riesgo), luego G (más peso: `/terminal-panes` cada 2 s por iframe), luego H, luego documentación y cutover. Cada tarea de dominio termina con sus líneas de fixture y sus pruebas en verde.

Reglas de oro heredadas (valen para todo el plan): las pruebas nunca tocan el servidor tmux del usuario, `~/.claude/hooks`, los puertos 4777–4782, `~/.local/share/comandos` ni el systemd de usuario; los cutovers los ejecuta el controlador; el Python no se modifica.

## Global Constraints

- Todo se compila y prueba con
  `CARGO_TARGET_DIR=/home/someguy/codebase/0xJesus/ComandOS/.build/target nice -n 10 cargo <cmd> -j 6`
  (abreviado `$C <cmd>`). Antes de cada commit: `$C fmt --all -- --check`, `$C clippy --workspace --all-targets -j 6 -- -D warnings` y las pruebas de los paquetes tocados.
- Commits con `git add <rutas>` explícitas; mensajes `feat(dash): …` / `fix(runtime): …` / `docs(verification): …` en español, terminados con una línea en blanco y `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. El plan se versiona con `git add -f docs/superpowers/plans/2026-10-04-fase-2c-dominios-nativos-ii.md`; el fixture con `git add -f xtask/parity/frente.jsonl`.
- Las pruebas usan `support::TestHome` (HOME temporal, `TMUX_TMPDIR` propio, sin `COMANDOS_STATE_DB`/`COMANDOS_USAGE_DB`); el oráculo Python comparte ese HOME. Sin `tmux` o sin `python3` la prueba se salta con un aviso (no `#[ignore]`), como en la 2b.
- El runtime del frente es `current_thread`: nada que bloquee más de unos milisegundos corre en él. SQLite → worker o carril; escrituras con `fsync`, lecturas de árboles de archivos y librerías síncronas → `tokio::task::spawn_blocking`; procesos → `tokio::process` (también desde `spawn_blocking`, con `Tmux::run_blocking`). Leer un JSON pequeño de `~/.claude/hooks` en línea sigue permitido.
- `Decline` solo antes de cualquier efecto. Excepción documentada (Tarea 6): `recover_abandoned` del journal, que es idempotente y es lo primero que hace también el Python.
- Excepción no capturada del Python → `HandlerError::Failure` (500 `{"error": "Error interno del tablero"}`); `subprocess.TimeoutExpired` no capturado → `HandlerError::Timeout` (504).

## Decisiones del plan (no fijadas por el controlador)

- **La base de uso de `cc-dash` NO honra `COMANDOS_USAGE_DB`**: `USAGE_DB = cc_usage.usage_db_path(HOOKS)` (`bin/cc-dash:174`) pasa `hooks_dir`, y `usage_db_path` (`bin/cc_usage.py:30`) solo mira el entorno cuando `hooks_dir is None`. El carril de uso abre siempre `~/.claude/hooks/comandos-usage.sqlite`, igual que `sovereignty_report` (4407).
- **Carriles** (`native/lanes.rs`): `Lane<B: LaneBackend>` copia la semántica de `Native::with_state` (abrir una vez en `spawn_blocking`, puerta antes de cada trabajo, el trabajo que entra en pánico responde 500 y retira el carril, lo que no empezó declina) pero el apagado es del carril: una línea en stderr que nombra la base y sus rutas; `app-state`, el resto de rutas nativas y el otro carril siguen vivos. La puerta de la base de uso es `PRAGMA user_version > 11` (más nueva: se rechaza sin tocar); `< 11` migra con `comandos_store::usage::ensure_schema` (lo mismo que haría el `init_db` del Python). La del journal comprueba las 9 columnas de `session_operations` en orden (`lib/session_operations.py:20`; el Python no versiona ese esquema).
- **Puente síncrono a tmux**: `terminal_panes::execute`, `terminal_history::capture` y `pane_typing::type_literal` reciben callbacks síncronos. Corren en `spawn_blocking`; cada callback llama a `Tmux::run_blocking(&handle, args)` = `handle.block_on(self.run(args))`. Es válido en un runtime `current_thread` porque su hilo principal está en `Runtime::block_on` (el servidor o `#[tokio::test]`) y es él quien mueve el reactor, los timers y el reaper de procesos. El ejecutor, el entorno y el plazo son los de la 2b.
- **Candado de archivos** (`files::FileLock`): `file_lock` del Python (5168) es `flock(LOCK_EX)` sobre `<archivo>.lock`; `std::fs::File::try_lock` es el mismo `flock` en Linux. Se pide **sin esperar**: si lo tiene el Python, cc-app u otra petición, se declina antes de leer nada y el Python (que sí espera) atiende. Nunca se pierde una escritura ajena.
- **`response_dumps_unicode`** en `comandos-core`: `json.dumps(x, ensure_ascii=False)` con orden de inserción y separadores por defecto (las líneas de `ui-events.jsonl`). `dumps(…, false, false)` no sirve: ordena claves.
- **Raíz del repositorio** (`REPO_ROOT` del Python, 1370): `DashConfig.repo_root` = `COMANDOS_DASH_REPO` si está; si no, el destino canónico de `<dash_dir>/index.html` dos niveles arriba (`install.sh` y el arnés enlazan `dash/*` al checkout desde el que corre el Python heredado). Sin raíz, `/model-tiers` declina.
- **Biblioteca `terminal_panes` corregida**: el inventario Rust no es el del Python — orden de claves `id, active, title, index, left…, path, identity` frente a `id, active, title, path, left, top, width, height, index, identity` (`lib/terminal_panes.py:39-41`) y una validación de identidad que el Python solo hace antes de mutar (`:138-142`). La Tarea 4 la alinea; las demás librerías (`terminal_history`, `pane_typing`, `session_operations`, `pomodoro`, `focus`) se usan tal cual y el oráculo lo comprueba.
- **`/pomodoro` (GET) escribe una vez**: `pomodoro_store()` del Python hace `focus_progress.ensure_policy` (un `INSERT … ON CONFLICT DO NOTHING`, 6617) por hilo; el nativo lo hace una vez por proceso (`StateBackend.pomodoro_policy`). También `read_focus_settings` → `init_db` puede crear la base de uso. Ambos son idempotentes.
- **`/pane/type`**: la caché de 256 respuestas por `requestId` y los candados por pane pasan a ser del frente (`native.typing`). Para que un reintento nunca teclee dos veces: (a) la caché Rust se consulta antes que la resolución de sesión (el Python la consulta después; con la misma petición el resultado es el mismo y no hay doble tecleo), y (b) un `requestId` que el frente declinó queda marcado y se vuelve a declinar siempre, para que la caché del Python responda el reintento.
- **Rutas que siguen reenviadas en 2c**, con la razón comprobada en el Python:
  - POST `/pomodoro` (6798): despierta `_POMODORO_WAKE`, un `threading.Event` del proceso Python (6820), y su scheduler; no hay señal por base.
  - GET `/analytics/week` (6734): `usage_provider_limits()` (1309) devuelve `_limits_cache`, memoria del Python que llena la red; un GET con la caché vieja lanza un hilo de refresco. Pasa a la 2e con el resto de `cc_usage`.
  - GET `/providers` (1722) y `/optimization/plans` (1463): necesitan `lib/providers.py` (`load_registry`, `validate_registry`, `public_state`, `evaluate_capability_matrix`, `validate_selection`), sin port; `public_state` usa `which()` con el `PATH` de la unidad heredada y `provider_runtime_facts` sondea el puerto del proxy. Se cargan una vez al abrir el tablero.
  - GET `/accounts` (8390): todos los llamadores vivos (`dash/term.html:2230`, `bin/cc-app:5431`) omiten `usage=0`, así que la respuesta depende siempre de `usage_provider_limits`; el nativo solo declinaría.
  - GET `/opencode/models` (4829): lanza `opencode` con caché en un hilo.
  - GET `/session-profiles` y `/extension-usage` (8475, 8487): `comandos_store::usage` no tiene lectores de `session_profiles` ni de uso de extensiones, y `session_profile_store.inventory` lee TOML y frontmatter (`lib/session_profiles.py:97-372`).
  - POST `/terminal/quick` (9520, `quick_terminal_request` 5631): `open_quick_terminal` es síncrono y retiene la conexión de `app-state` mientras arranca tmux (plazo 15 s) y espera a otro solicitante; dentro del worker único pararía todas las rutas de la base. Además registra la pestaña (`register_app_tab` + `workspace_sync`, sin port) y elige `systemd-run` con `which()`. Va a la 2d con las mutaciones de pestañas.
  - Dentro de rutas nativas: POST `/terminal-panes` con `action:"close"` declina (`save_closed_pane_snapshot` 5802 usa `tmux_snapshot.capture_session` + `pane_snapshot.PaneInspector`, inspección de procesos de la 2d); GET `/model/status` sin registro en el journal declina (`MOTOR_RESULT`, memoria del Python, 3636) y con estado `awaiting_confirmation` también (`refresh_session_confirmation` 3418 observa el proceso y escribe); POST `/pane/type` declina si algún `H/state/*.json` nombra la sesión (`resolve_project_session` 6300 sigue con `agent_procs`).
- **GET con efectos laterales** (marcados también en el mapa): GET `/pomodoro` (política una vez + `init_db` de la base de uso); GET `/model/status` (`recover_abandoned` marca operaciones de dueños muertos como `failed`/`recovery_required`; crea el journal si falta). GET `/snippets`, `/model-tiers` y `/sovereignty` no escriben (`/sovereignty` puede abrir —y migrar si es vieja— la base de uso a través del carril, como hace cualquier ruta del Python que llama a `init_db`).

## Mapa de rutas nativas

| Ruta | Llave | Python | Respuesta 200 (claves en orden) | Errores | T |
|---|---|---|---|---|---|
| GET `/snippets` | Raw | 8740 `read_snippets` 5264 | `[{id,name,body,tags,updated_at}…]` | `int(updated_at)` imposible → 500 | 1 |
| POST `/snippets` | Raw | 9287 | `{"item": {id,name,body,tags,updated_at}}` | 400 `name vacio`, `name demasiado largo (max 80)`, `body vacio`, `body demasiado largo (max 20000)`, `tags debe ser lista`, `demasiados tags (max 10)`, `tag vacio`, `tag demasiado largo (max 32)` | 1 |
| POST `/snippets/update` | Raw | 9301 | `{"item": …}` | 400 `id invalido` y los anteriores; 404 `snippet no encontrado` | 1 |
| POST `/snippets/delete` | Raw | 9320 | `{"ok": true}` | 400 `id invalido`; 404 `snippet no encontrado` | 1 |
| POST `/ui-log` | Raw | 8942 `ui_log_append` 64 | `{"ok": true, "n": n}` | conversión imposible → 500; **anexa a `H/ui-events.jsonl` y rota a 2 MB** | 1 |
| GET `/pomodoro` | Raw | 8448 `pomodoro_payload` 6646 | `{revision,serverNowMs,block,queue,settings,progress,sound}` | — ; **efecto**: política y `init_db` | 2 |
| GET `/model-tiers` | Path | 8413 `load_model_tiers` 3832 | objeto de `config/model-tiers.json` (no-objeto → `{}`) | ausente/ilegible → declina | 3 |
| GET `/sovereignty` | Raw | 8450 `sovereignty_report` 4393 | `{stores,browser,outbound,generatedAt}` | — | 3 |
| POST `/terminal-history` | Raw | 8889 `lib/terminal_history.py` | `{ok,pane,panes,text,lines,truncated,source,readOnly}` | 400 `str(ValueError)`; 503 `Historial temporalmente no disponible` | 4 |
| POST `/terminal-panes` | Raw | 8898 `lib/terminal_panes.py` | `list`: `{ok,panes,remoteFocus}`; `select`/`split`/`resize`: `{ok,panes}` (+`clientKeys`/`opened`) | 400 `str(ValueError)`; 503 `No se pudo completar la acción del panel. Comprueba Paneles antes de reintentar`; `close` → declina; **efecto**: `select`/`split`/`resize` mueven tmux | 4 |
| POST `/pane/type` | Raw | 9667 | `{ok,typed,durationMs,requestId}` | 400 `Nombre de sesion invalido`, `{"error":"Falta un pane exacto (%N)","code":"invalid"}`, `{"error":…,"code":"invalid"}`; 404 `No hay sesion tmux '<s>'. Levantala primero.`, `{"error":"El pane <p> ya no existe","code":"pane_gone"}`; 409 `typing_in_progress`; 502 `{error,code,typed,requestId}`; **efecto**: `tmux send-keys -l` | 5 |
| GET `/model/status` | Path | 8452 `session_operation_status` 6335 | `{operationId,state,ts,…resultado…,sourceHarness,sourceConversationId,handoffPath,operationKey}` | 400 `operationKey inválido`, `operationId inválido`, `La operación no pertenece a este panel`; **efecto**: `recover_abandoned` | 6 |

`Raw` = `self.path == p` (sin consulta); `Path` = el `startswith` del Python reclamando solo la ruta exacta con consulta opcional (`native::Key`, sin cambios).

## Review Focus

Cinco condiciones que las pruebas de cada librería no cubren y que más probablemente muerdan; cada una tiene su prueba en la tarea que la posee:

1. **El Python, cc-app o una segunda petición tienen el `flock` de `snippets.json`/`ui-events.jsonl`**: el frente no espera ni escribe; declina y el Python (que espera) atiende; el archivo nunca pierde una entrada. Prueba `snippets_lock_contended_declines` (Tarea 1).
2. **Un Python más nuevo migra la base de uso a `user_version = 12`**: solo GET `/pomodoro` y `/sovereignty` se reenvían (una línea en stderr); `/snippets`, `/notices` y el resto siguen nativos; la base nunca se toca. Prueba `usage_newer_schema_disables_only_usage_lane` (Tarea 2).
3. **Reintento de `/pane/type` con el mismo `requestId`**, también cuando la primera vez se declinó: nunca se teclea dos veces. Pruebas `pane_type_retry_same_request_id_types_once` y `declined_request_id_stays_declined` (Tarea 5).
4. **tmux colgado mientras varios iframes sondean `/terminal-panes` cada 2 s**: responde el 503 del Python (no 504) y el runtime sigue atendiendo otras rutas mientras tanto. Prueba `terminal_hung_tmux_answers_503_and_frees_runtime` (Tarea 4).
5. **El journal tiene una operación esperando confirmación o ninguna para ese panel**: se reenvía (nunca se responde con datos que el Python tiene en memoria o que tendría que observar del proceso). Prueba `model_status_declines_pending_confirmation_and_motor_result` (Tarea 6).

## Estructura de archivos

```
crates/comandos-core/src/json/python.rs, src/json.rs        (response_dumps_unicode)          T1
crates/comandos-core/tests/response_dumps_unicode.rs                                            T1
crates/comandos-store/src/usage.rs                          (SCHEMA_VERSION pub, schema_version,
                                                             ensure_schema, focus_settings_rows) T2
crates/comandos-store/tests/usage_focus_settings.rs                                              T2
crates/comandos-runtime/src/terminal_panes.rs               (inventario alineado con el Python)  T4
crates/comandos-runtime/tests/terminal_panes_controls.rs                                         T4
crates/comandos-server/src/dash/mod.rs                      (DashConfig.repo_root, repo_root())  T3
crates/comandos-server/src/dash/native/
  mod.rs        (variantes, tablas, campos usage/typing/journal, opciones nuevas)              T1–T6
  files.rs      (loads_strict, write_text_atomic, FileLock)                                     T1
  light.rs      (load y read_reply → pub(crate))                                                T1
  snippets.rs   (F: 4 rutas)                                                                    T1
  ui_log.rs     (F: POST /ui-log)                                                               T1
  state.rs      (Refusal::lane_message, StateBackend.pomodoro_policy)                           T2
  lanes.rs      (Lane, LaneBackend, UsageBackend; JournalBackend en T6)                         T2, T6
  pomodoro.rs   (F: GET /pomodoro)                                                              T2
  catalogs.rs   (F: GET /model-tiers, GET /sovereignty)                                         T3
  tmux.rs       (Tmux::run_blocking)                                                            T4
  terminal.rs   (G: /terminal-history, /terminal-panes)                                         T4
  typing.rs     (G: /pane/type, TypingState)                                                    T5
  operations.rs (H: GET /model/status)                                                          T6
crates/comandos-server/tests/support/mod.rs                 (usage_db, journal_db, repo, repo_root)
crates/comandos-server/tests/dash_native_snippets.rs, dash_native_pomodoro.rs,
  dash_native_catalogs.rs, dash_native_terminal.rs, dash_native_typing.rs,
  dash_native_operations.rs
xtask/src/parity.rs, xtask/src/poll.rs, xtask/parity/README.md (--usage-db)                     T2
xtask/parity/frente.jsonl                                                                       T1–T6
docs/verification/cutover-dash.md                           (sección «2c: dominios nativos II»)  T7
```

---

### Task 1: Dominio F1 — snippets y registro de uso (5 rutas, archivos con `flock`)

Desplegable sola: GET `/snippets`, POST `/snippets`, `/snippets/update`, `/snippets/delete` y POST `/ui-log` (el tablero lo llama cada 5 s si hay eventos, `dash/index.html:4448`) pasan a Rust. Las escrituras usan el mismo candado `flock` de `<archivo>.lock` que el Python, sin esperar: si está tomado, se declina.

Comportamiento portado:

- **`read_snippets`** (5264): `load_json_file(…, [])` (cualquier fallo o `null` → `[]`); no-lista → `[]`; por cada objeto: `id = str(it.get("id",""))` que case `^[a-f0-9]{16}\Z`, `name`/`body` con `str()`, `tags = it.get("tags") or []`, se descarta si `snippet_validate(name, body, tags si es lista o [])` da error; fila `{id, name, body, tags:[str(t)…], updated_at:int(it.get("updated_at") or 0)}`. `str()` de flotantes/listas/objetos y `tags` verdaderas que no son lista (el Python itera la cadena o las claves) → `Decline`; `int()` imposible → 500.
- **`snippet_validate`** (5196): el orden exacto de mensajes del mapa; `strip()` de Python (`py::strip`), longitudes en caracteres.
- **POST `/snippets`** (9287): `name = str(data.get("name",""))`, `body` igual, `tags = data.get("tags") or []`; validar → 400; bajo candado: leer, insertar al principio `{id: token_hex(8), name: name.strip(), body, tags: [t.strip()…], updated_at: int(time.time())}`, escribir con `write_json_file` (ensure_ascii, orden de inserción). Respuesta `{"item": …}`.
- **POST `/snippets/update`** (9301): `id` validado primero (400 `id invalido`), luego campos; bajo candado se reemplazan TODAS las entradas con ese id (el Python recorre la lista entera) y se escribe solo si hubo alguna; 404 `snippet no encontrado`.
- **POST `/snippets/delete`** (9320): `id` validado; se escribe solo si cambió la longitud; `{"ok": true}` o 404.
- **POST `/ui-log`** (8942, `ui_log_append` 64): `events = data.get("events") or []`; `events[:200]` sobre un objeto, número o booleano → `TypeError` → 500; sobre una cadena → caracteres, ninguno es un dict → `n: 0` sin tocar el archivo. Por cada dict: `{"ts": float(e.get("ts") or now), "k": str(e.get("k") or "")[:24], "n": …[:80], "c": …[:80], "d": int(e.get("d") or 0), "s": …[:80]}` (todas las conversiones antes del candado, como el Python); líneas `json.dumps(e, ensure_ascii=False) + "\n"` anexadas en modo texto; si tras anexar pasa de 2 000 000 bytes: líneas no vacías en modo texto (saltos universales), las últimas 20 000, las de `ts >= now - 30 días`, reescritura atómica; una línea que no es objeto o un `ts` no comparable abortan la rotación en silencio (el `except Exception: pass` externo). La rotación se decide antes de anexar: si una línea vieja es «incierta» (anidamiento o sustitutos que el parser Rust no lee como CPython) se declina sin escribir.

**Files:**
- Modify: `crates/comandos-core/src/json/python.rs`, `crates/comandos-core/src/json.rs`
- Create: `crates/comandos-core/tests/response_dumps_unicode.rs`
- Modify: `crates/comandos-server/src/dash/native/files.rs`, `crates/comandos-server/src/dash/native/light.rs`, `crates/comandos-server/src/dash/native/mod.rs`
- Create: `crates/comandos-server/src/dash/native/snippets.rs`, `crates/comandos-server/src/dash/native/ui_log.rs`
- Create: `crates/comandos-server/tests/dash_native_snippets.rs`
- Modify: `xtask/parity/frente.jsonl`

**Interfaces:**
- Consumes: `native::{Answer, Entry, Fault, Key, Native, NativeRoute, Verb, reply}`, `light::{data, error}`, `py::{strip, str_scalar, int_of, Conversion, float, NumError, take_chars}`, `files::{Strict, read_json_strict, write_json_atomic}`.
- Produces: `comandos_core::json::response_dumps_unicode(&Value) -> Result<String, String>`; `files::{loads_strict(&str) -> Strict, write_text_atomic(&Path, &str) -> io::Result<()>, FileLock::try_acquire(&Path) -> io::Result<Option<FileLock>>}`; `light::{load, read_reply}` pasan a `pub(crate)`; `snippets::{SnippetsRoute, ROUTES, answer}`; `ui_log::{ROUTES, answer}`; `NativeRoute::{Snippets(SnippetsRoute), UiLog}`.

- [ ] **Step 1: Prueba que falla del volcado Unicode (core)**

Crear `crates/comandos-core/tests/response_dumps_unicode.rs`:

```rust
//! `json.dumps(x, ensure_ascii=False)`: orden de inserción, separadores por defecto.
use comandos_core::json::{response_dumps_unicode, workspace_loads};

#[test]
fn keeps_insertion_order_and_utf8() {
    let value = workspace_loads(r#"{"ts": 1.0, "k": "clic ñ", "d": 3, "z": [true, null], "nan": NaN}"#)
        .unwrap();
    assert_eq!(
        response_dumps_unicode(&value).unwrap(),
        r#"{"ts": 1.0, "k": "clic ñ", "d": 3, "z": [true, null], "nan": NaN}"#
    );
}

#[test]
fn escapes_controls_like_python() {
    let value = workspace_loads(r#"{"s": "a\u0001\n\"b"}"#).unwrap();
    assert_eq!(response_dumps_unicode(&value).unwrap(), r#"{"s": "a\u0001\n\"b"}"#);
}
```

- [ ] **Step 2: Ver el fallo**

Run: `$C test -p comandos-core --test response_dumps_unicode`
Expected: FAIL de compilación, `no response_dumps_unicode in json`.

- [ ] **Step 3: Implementar**

En `crates/comandos-core/src/json/python.rs`, debajo de `response_dumps`:

```rust
/// `json.dumps(value, ensure_ascii=False)` del Python: orden de inserción,
/// separadores `", "`/`": "` y UTF-8 sin escapar (líneas de registros JSONL).
pub fn response_dumps_unicode(value: &Value) -> Result<String, String> {
    validate_workspace_depth(value, 0)?;
    encode(value, false, false, false, Policy::Workspace)
}
```

y en `crates/comandos-core/src/json.rs` cambiar la línea de reexportación por:

```rust
pub use python::{
    dumps, response_dumps, response_dumps_unicode, workspace_dumps, workspace_dumps_with_options,
};
```

Run: `$C test -p comandos-core --test response_dumps_unicode` → PASS.

- [ ] **Step 4: Kit de archivos (`files.rs`) y visibilidad en `light.rs`**

En `crates/comandos-server/src/dash/native/files.rs`, sustituir el cuerpo de `read_json_strict` a partir de la conversión a texto y añadir `loads_strict`, `write_text_atomic` y `FileLock`:

```rust
pub fn read_json_strict(path: &Path) -> Strict {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Strict::Missing,
        Err(_) => return Strict::Unsure,
    };
    match std::str::from_utf8(&bytes) {
        Ok(text) => loads_strict(text),
        Err(_) => Strict::Unsure,
    }
}

/// `json.loads(texto)` sobre un `str` ya decodificado, con la misma
/// clasificación que `read_json_strict` (BOM → ilegible; sustitutos sueltos o
/// anidamiento profundo → incierto).
pub fn loads_strict(text: &str) -> Strict {
    if text.starts_with('\u{feff}') {
        return Strict::Unreadable;
    }
    match workspace_loads(text) {
        Ok(value) => Strict::Value(value),
        Err(_) if has_surrogate_escape(text) || deep(text) => Strict::Unsure,
        Err(_) => Strict::Unreadable,
    }
}
```

Partir `write_json_atomic` en dos (mismo cuerpo, ahora sobre texto):

```rust
pub fn write_json_atomic(path: &Path, value: &Value) -> io::Result<()> {
    let text = response_dumps(value).map_err(io::Error::other)?;
    write_text_atomic(path, &text)
}

/// `write_file_atomic` (5063) con texto ya formado.
/// Bloquea: llamar dentro de `spawn_blocking`.
pub fn write_text_atomic(path: &Path, text: &str) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let mode = match fs::metadata(path) {
        Ok(meta) => meta.permissions().mode() & 0o7777,
        Err(e) if e.kind() == io::ErrorKind::NotFound => 0o600,
        Err(e) => return Err(e),
    };
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("ruta sin nombre"))?
        .to_string_lossy()
        .into_owned();
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|e| io::Error::other(e.to_string()))?;
    let suffix: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let tmp = dir.join(format!(".{name}.{suffix}.tmp"));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.flush()?;
        file.sync_all()?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// `file_lock` (5168): `flock` exclusivo sobre `<ruta>.lock` (creado 0600),
/// el mismo que toman el Python y cc-app. Se pide sin esperar: `Ok(None)` si
/// otro lo tiene; quien llama declina antes de leer o escribir nada. Se suelta
/// al soltar el valor (cerrar el descriptor suelta el `flock`).
pub struct FileLock {
    _file: fs::File,
}

impl FileLock {
    pub fn try_acquire(path: &Path) -> io::Result<Option<FileLock>> {
        let mut name = path.as_os_str().to_owned();
        name.push(".lock");
        let lock = std::path::PathBuf::from(name);
        if let Some(dir) = lock.parent().filter(|d| !d.as_os_str().is_empty()) {
            fs::create_dir_all(dir)?;
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&lock)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(FileLock { _file: file })),
            Err(fs::TryLockError::WouldBlock) => Ok(None),
            Err(fs::TryLockError::Error(e)) => Err(e),
        }
    }
}
```

En `crates/comandos-server/src/dash/native/light.rs` cambiar `fn load(` por `pub(crate) fn load(` y `fn read_reply(` por `pub(crate) fn read_reply(` (sin otro cambio).

- [ ] **Step 5: Pruebas que fallan del dominio**

Crear `crates/comandos-server/tests/dash_native_snippets.rs`:

```rust
//! Dominio F1: snippets y registro de uso respondidos por Rust, con el mismo
//! `flock` que el Python. El heredado es un puerto muerto salvo en las
//! pruebas de declinar (heredado falso que responde `{"legacy": true}`).
mod support;

use serde_json::Value;
use std::{fs, sync::Arc};
use support::{FakeLegacy, TestHome, Wire, dead_port, front, get, oracle::oracle, request_body};

const MESSY: &str = r#"[
  {"id": "0123456789abcdef", "name": " Saludo ", "body": "echo hola", "tags": ["a", " b "], "updated_at": 7.9},
  {"id": "0123456789ABCDEF", "name": "mayúsculas", "body": "x"},
  {"id": "fedcba9876543210", "name": null, "body": true, "tags": null},
  {"id": "1111111111111111", "name": "", "body": "x"},
  {"id": "2222222222222222", "name": "ok", "body": "x", "tags": ["", "y"]},
  "no-objeto",
  {"id": "3333333333333333", "name": "ñandú", "body": "y", "tags": []}
]"#;

fn seen(wire: &Wire) -> (u16, Option<String>, String) {
    (wire.status, wire.header("content-type").map(str::to_owned), wire.text())
}

#[tokio::test]
async fn snippets_list_normalizes_like_python() {
    let home = TestHome::new("snip-list");
    home.write("snippets.json", MESSY);
    let front = front(&home, dead_port(), home.options()).await;
    let wire = get(front.port, "/snippets").await;
    assert_eq!(wire.status, 200);
    assert_eq!(
        wire.text(),
        r#"[{"id": "0123456789abcdef", "name": " Saludo ", "body": "echo hola", "tags": ["a", " b "], "updated_at": 7}, {"id": "fedcba9876543210", "name": "None", "body": "True", "tags": [], "updated_at": 0}, {"id": "3333333333333333", "name": "ñandú", "body": "y", "tags": [], "updated_at": 0}]"#
    );
    if let Some(py) = oracle(&home).await {
        assert_eq!(seen(&get(py.port, "/snippets").await), seen(&wire));
    }
    front.stop().await;
}

#[tokio::test]
async fn snippets_crud_writes_python_bytes() {
    let home = TestHome::new("snip-crud");
    let front = front(&home, dead_port(), home.options()).await;
    let created = request_body(
        front.port,
        "POST",
        "/snippets",
        "",
        r#"{"name": "  Deploy  ", "body": "make deploy", "tags": [" ops ", "prod"]}"#,
    )
    .await;
    assert_eq!(created.status, 200, "{}", created.text());
    let item: Value = serde_json::from_str(&created.text()).unwrap();
    let id = item["item"]["id"].as_str().unwrap().to_owned();
    assert_eq!(id.len(), 16);
    assert_eq!(
        created.text(),
        format!(
            r#"{{"item": {{"id": "{id}", "name": "Deploy", "body": "make deploy", "tags": ["ops", "prod"], "updated_at": 1791115200}}}}"#
        )
    );
    assert_eq!(
        fs::read_to_string(home.hooks().join("snippets.json")).unwrap(),
        format!(
            r#"[{{"id": "{id}", "name": "Deploy", "body": "make deploy", "tags": ["ops", "prod"], "updated_at": 1791115200}}]"#
        )
    );
    let update = format!(r#"{{"id": "{id}", "name": "Deploy 2", "body": "make d2", "tags": []}}"#);
    let updated = request_body(front.port, "POST", "/snippets/update", "", &update).await;
    assert_eq!(
        updated.text(),
        format!(
            r#"{{"item": {{"id": "{id}", "name": "Deploy 2", "body": "make d2", "tags": [], "updated_at": 1791115200}}}}"#
        )
    );
    // Lo escrito por Rust lo lee igual el Python.
    if let Some(py) = oracle(&home).await {
        assert_eq!(seen(&get(py.port, "/snippets").await), seen(&get(front.port, "/snippets").await));
    }
    let deleted =
        request_body(front.port, "POST", "/snippets/delete", "", &format!(r#"{{"id": "{id}"}}"#)).await;
    assert_eq!((deleted.status, deleted.text().as_str()), (200, r#"{"ok": true}"#));
    let again =
        request_body(front.port, "POST", "/snippets/delete", "", &format!(r#"{{"id": "{id}"}}"#)).await;
    assert_eq!((again.status, again.text().as_str()), (404, r#"{"error": "snippet no encontrado"}"#));
    front.stop().await;
}

#[tokio::test]
async fn snippets_errors_match_python_oracle() {
    let home = TestHome::new("snip-err");
    home.write("snippets.json", MESSY);
    let Some(py) = oracle(&home).await else { return };
    let front = front(&home, dead_port(), home.options()).await;
    let long_name = "n".repeat(81);
    let cases = [
        ("/snippets", r#"{"name": "", "body": "x"}"#.to_owned()),
        ("/snippets", r#"{"name": "a", "body": "   "}"#.to_owned()),
        ("/snippets", format!(r#"{{"name": "{long_name}", "body": "x"}}"#)),
        ("/snippets", r#"{"name": "a", "body": "x", "tags": "uno"}"#.to_owned()),
        ("/snippets", r#"{"name": "a", "body": "x", "tags": ["1","2","3","4","5","6","7","8","9","10","11"]}"#.to_owned()),
        ("/snippets", r#"{"name": "a", "body": "x", "tags": [" "]}"#.to_owned()),
        ("/snippets", r#"{"name": "a", "body": "x", "tags": [3]}"#.to_owned()),
        ("/snippets", format!(r#"{{"name": "a", "body": "x", "tags": ["{}"]}}"#, "t".repeat(33))),
        ("/snippets/update", r#"{"id": "zz"}"#.to_owned()),
        ("/snippets/update", r#"{"id": "0123456789abcdef", "name": "", "body": "x"}"#.to_owned()),
        ("/snippets/update", r#"{"id": "9999999999999999", "name": "a", "body": "x"}"#.to_owned()),
        ("/snippets/delete", r#"{"id": 5}"#.to_owned()),
        ("/snippets/delete", r#"{"id": "9999999999999999"}"#.to_owned()),
    ];
    for (path, body) in &cases {
        assert_eq!(
            seen(&request_body(py.port, "POST", path, "", body).await),
            seen(&request_body(front.port, "POST", path, "", body).await),
            "{path} {body}"
        );
    }
    front.stop().await;
}

#[tokio::test]
async fn snippets_lock_contended_declines() {
    let home = TestHome::new("snip-lock");
    home.write("snippets.json", "[]");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let holder = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(home.hooks().join("snippets.json.lock"))
        .unwrap();
    holder.lock().unwrap();
    let wire = request_body(front.port, "POST", "/snippets", "", r#"{"name": "a", "body": "b"}"#).await;
    assert_eq!(wire.text(), r#"{"legacy": true}"#, "declina al heredado");
    let wire = request_body(front.port, "POST", "/ui-log", "", r#"{"events": [{"k": "x"}]}"#).await;
    // ui-log usa su propio candado: no está tomado, se atiende en Rust.
    assert_eq!(wire.text(), r#"{"ok": true, "n": 1}"#);
    assert_eq!(fs::read_to_string(home.hooks().join("snippets.json")).unwrap(), "[]");
    holder.unlock().unwrap();
    let wire = request_body(front.port, "POST", "/snippets", "", r#"{"name": "a", "body": "b"}"#).await;
    assert_eq!(wire.status, 200, "libre: nativo");
    assert_eq!(legacy.requests(), vec!["POST /snippets HTTP/1.1".to_owned()]);
    front.stop().await;
}

#[tokio::test]
async fn ui_log_appends_python_lines() {
    let home = TestHome::new("uilog");
    let front = front(&home, dead_port(), home.options()).await;
    let body = r#"{"events": [{"ts": 5, "k": "clicañ-muy-largo-de-mas-de-24", "n": 7, "c": true, "d": "12", "s": null}, "x", {"ts": "1.5e3"}]}"#;
    let wire = request_body(front.port, "POST", "/ui-log", "", body).await;
    assert_eq!(wire.text(), r#"{"ok": true, "n": 2}"#);
    assert_eq!(
        fs::read_to_string(home.hooks().join("ui-events.jsonl")).unwrap(),
        "{\"ts\": 5.0, \"k\": \"clicañ-muy-largo-de-mas-\", \"n\": \"7\", \"c\": \"True\", \"d\": 12, \"s\": \"\"}\n\
         {\"ts\": 1500.0, \"k\": \"\", \"n\": \"\", \"c\": \"\", \"d\": 0, \"s\": \"\"}\n"
    );
    for (body, expected) in [
        (r#"{}"#, r#"{"ok": true, "n": 0}"#),
        (r#"{"events": "abc"}"#, r#"{"ok": true, "n": 0}"#),
    ] {
        assert_eq!(request_body(front.port, "POST", "/ui-log", "", body).await.text(), expected);
    }
    front.stop().await;
}

#[tokio::test]
async fn ui_log_errors_and_rotation_match_python_oracle() {
    // Dos HOME con el mismo archivo de partida: cada servidor escribe el suyo.
    let rust_home = TestHome::new("uilog-rs");
    let python_home = TestHome::new("uilog-py");
    let mut old = String::new();
    let line = format!("{{\"ts\": 1.0, \"k\": \"viejo\", \"n\": \"{}\"}}\n", "x".repeat(60));
    while old.len() < 2_000_100 {
        old.push_str(&line);
    }
    old.push_str("no es json\n\n");
    old.push_str("{\"ts\": 2000000000.0, \"k\": \"nuevo\"}\n");
    for home in [&rust_home, &python_home] {
        home.write("ui-events.jsonl", &old);
    }
    let Some(py) = oracle(&python_home).await else { return };
    let mut opts = rust_home.options();
    opts.clock = Arc::new(comandos_server::dash::native::wall_clock_ms);
    let front = front(&rust_home, dead_port(), opts).await;
    let body = r#"{"events": [{"ts": 2000000001, "k": "a", "d": 1}]}"#;
    assert_eq!(
        seen(&request_body(py.port, "POST", "/ui-log", "", body).await),
        seen(&request_body(front.port, "POST", "/ui-log", "", body).await)
    );
    let rotated = fs::read_to_string(rust_home.hooks().join("ui-events.jsonl")).unwrap();
    assert_eq!(
        rotated,
        fs::read_to_string(python_home.hooks().join("ui-events.jsonl")).unwrap()
    );
    assert_eq!(
        rotated,
        "{\"ts\": 2000000000.0, \"k\": \"nuevo\"}\n{\"ts\": 2000000001.0, \"k\": \"a\", \"n\": \"\", \"c\": \"\", \"d\": 1, \"s\": \"\"}\n"
    );
    for body in [
        r#"{"events": {"a": 1}}"#,
        r#"{"events": 5}"#,
        r#"{"events": [{"d": "x"}]}"#,
        r#"{"events": [{"ts": "nope"}]}"#,
        r#"{"events": [{"ts": [1]}]}"#,
    ] {
        assert_eq!(
            seen(&request_body(py.port, "POST", "/ui-log", "", body).await),
            seen(&request_body(front.port, "POST", "/ui-log", "", body).await),
            "{body}"
        );
    }
    front.stop().await;
}

#[tokio::test]
async fn exotic_inputs_decline_to_legacy() {
    let home = TestHome::new("snip-exotic");
    home.write("snippets.json", r#"[{"id": "0123456789abcdef", "name": 1.5, "body": "x"}]"#);
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    for (method, path, body) in [
        ("GET", "/snippets", ""),
        ("POST", "/snippets", r#"{"name": 2.5, "body": "x"}"#),
        ("POST", "/ui-log", r#"{"events": [{"k": 1.5}]}"#),
        ("POST", "/ui-log", r#"{"events": [{"d": 123456789012345678901234567890}]}"#),
    ] {
        let wire = request_body(front.port, method, path, "", body).await;
        assert_eq!(wire.text(), r#"{"legacy": true}"#, "{method} {path} {body}");
    }
    assert!(!home.hooks().join("ui-events.jsonl").exists(), "declinar no escribe");
    front.stop().await;
}
```

Run: `$C test -p comandos-server --test dash_native_snippets -j 6`
Expected: FAIL (502 del heredado muerto: las rutas aún se reenvían).

- [ ] **Step 6: Implementar `snippets.rs`**

Crear `crates/comandos-server/src/dash/native/snippets.rs`:

```rust
//! F. Snippets (`bin/cc-dash`: `snippet_validate` 5196, `read_snippets` 5264,
//! GET 8740, POST 9287–9332). `H/snippets.json` se reescribe bajo el `flock`
//! de `<archivo>.lock` que comparte con el Python; si está tomado, se declina.
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb,
    files::{self, FileLock},
    light::{data, error, load, read_reply},
    py, reply,
};
use crate::{HandlerError, Request};
use comandos_core::json::truthy;
use http::StatusCode;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnippetsRoute {
    List,
    Create,
    Update,
    Delete,
}

const fn entry(verb: Verb, path: &'static str, route: SnippetsRoute) -> Entry {
    Entry {
        verb,
        key: Key::Raw(path),
        route: NativeRoute::Snippets(route),
    }
}

pub const ROUTES: &[Entry] = &[
    entry(Verb::Get, "/snippets", SnippetsRoute::List),
    entry(Verb::Post, "/snippets", SnippetsRoute::Create),
    entry(Verb::Post, "/snippets/update", SnippetsRoute::Update),
    entry(Verb::Post, "/snippets/delete", SnippetsRoute::Delete),
];

pub async fn answer(native: &Native, route: SnippetsRoute, request: &Request) -> Answer {
    let path = native.options().hooks.join("snippets.json");
    let now = (native.options().clock)().div_euclid(1000);
    match route {
        SnippetsRoute::List => read_reply(&Value::Array(read_snippets(&path)?)),
        SnippetsRoute::Create => create(path, data(request)?, now).await,
        SnippetsRoute::Update => update(path, data(request)?, now).await,
        SnippetsRoute::Delete => delete(path, data(request)?).await,
    }
}

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

/// `SNIPPET_ID_RE = ^[a-f0-9]{16}\Z`.
fn snippet_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `str(x.get(k, ""))`: escalares seguros; flotantes y contenedores declinan.
fn text_of(value: Option<&Value>) -> Result<String, Fault> {
    match value {
        None => Ok(String::new()),
        Some(v) => py::str_scalar(v).ok_or(Fault::Decline),
    }
}

/// `snippet_validate` (5196), con `tags` tal cual lo recibe.
fn validate(name: &str, body: &str, tags: &Value) -> Option<&'static str> {
    if py::strip(name).is_empty() {
        return Some("name vacio");
    }
    if name.chars().count() > 80 {
        return Some("name demasiado largo (max 80)");
    }
    if py::strip(body).is_empty() {
        return Some("body vacio");
    }
    if body.chars().count() > 20000 {
        return Some("body demasiado largo (max 20000)");
    }
    let Value::Array(tags) = tags else {
        return Some("tags debe ser lista");
    };
    if tags.len() > 10 {
        return Some("demasiados tags (max 10)");
    }
    for tag in tags {
        match tag {
            Value::String(t) if !py::strip(t).is_empty() => {
                if t.chars().count() > 32 {
                    return Some("tag demasiado largo (max 32)");
                }
            }
            _ => return Some("tag vacio"),
        }
    }
    None
}

/// `data.get("tags") or []`.
fn tags_of(value: Option<&Value>) -> Value {
    match value {
        Some(v) if truthy(v) => v.clone(),
        _ => Value::Array(Vec::new()),
    }
}

/// `read_snippets` (5264).
fn read_snippets(path: &Path) -> Result<Vec<Value>, Fault> {
    let Some(Value::Array(raw)) = load(path)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for item in raw {
        let Value::Object(it) = item else { continue };
        let id = text_of(it.get("id"))?;
        if !snippet_id(&id) {
            continue;
        }
        let name = text_of(it.get("name"))?;
        let body = text_of(it.get("body"))?;
        let tags = match it.get("tags") {
            Some(v) if truthy(v) => match v {
                Value::Array(_) => v.clone(),
                // El Python validaría con [] e iteraría la cadena o las claves.
                _ => return Err(Fault::Decline),
            },
            _ => Value::Array(Vec::new()),
        };
        if validate(&name, &body, &tags).is_some() {
            continue;
        }
        let updated = match it.get("updated_at") {
            Some(v) if truthy(v) => py::int_of(v).map_err(|c| match c {
                py::Conversion::Exotic => Fault::Decline,
                _ => failure(),
            })?,
            _ => 0,
        };
        let mut row = Map::new();
        row.insert("id".into(), Value::String(id));
        row.insert("name".into(), Value::String(name));
        row.insert("body".into(), Value::String(body));
        row.insert("tags".into(), tags);
        row.insert("updated_at".into(), json!(updated));
        out.push(Value::Object(row));
    }
    Ok(out)
}

/// `secrets.token_hex(8)`.
fn token_hex8() -> Result<String, Fault> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|_| failure())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// `{"id","name":strip,"body","tags":[strip…],"updated_at"}`.
fn item(id: String, name: &str, body: String, tags: &Value, now: i64) -> Value {
    let tags: Vec<Value> = tags
        .as_array()
        .map(|t| {
            t.iter()
                .filter_map(Value::as_str)
                .map(|s| Value::String(py::strip(s).to_owned()))
                .collect()
        })
        .unwrap_or_default();
    let mut row = Map::new();
    row.insert("id".into(), Value::String(id));
    row.insert("name".into(), Value::String(py::strip(name).to_owned()));
    row.insert("body".into(), Value::String(body));
    row.insert("tags".into(), Value::Array(tags));
    row.insert("updated_at".into(), json!(now));
    Value::Object(row)
}

/// `with file_lock(SNIPPETS_FILE): items = read_snippets(); …; write_snippets(items)`.
/// `mutate` dice si hay que escribir. Candado tomado → declinar sin leer nada.
async fn locked<T: Send + 'static>(
    path: PathBuf,
    mutate: impl FnOnce(&mut Vec<Value>) -> (bool, T) + Send + 'static,
) -> Result<T, Fault> {
    tokio::task::spawn_blocking(move || {
        let Some(_lock) = FileLock::try_acquire(&path).map_err(|_| failure())? else {
            return Err(Fault::Decline);
        };
        let mut items = read_snippets(&path)?;
        let (write, out) = mutate(&mut items);
        if write {
            files::write_json_atomic(&path, &Value::Array(items)).map_err(|_| failure())?;
        }
        Ok(out)
    })
    .await
    .map_err(|_| failure())?
}

async fn create(path: PathBuf, data: &Map<String, Value>, now: i64) -> Answer {
    let name = text_of(data.get("name"))?;
    let body = text_of(data.get("body"))?;
    let tags = tags_of(data.get("tags"));
    if let Some(message) = validate(&name, &body, &tags) {
        return error(StatusCode::BAD_REQUEST, message);
    }
    let new = item(token_hex8()?, &name, body, &tags, now);
    let stored = new.clone();
    locked(path, move |items| {
        items.insert(0, stored);
        (true, ())
    })
    .await?;
    reply(StatusCode::OK, &json!({"item": new}))
}

async fn update(path: PathBuf, data: &Map<String, Value>, now: i64) -> Answer {
    let id = text_of(data.get("id"))?;
    if !snippet_id(&id) {
        return error(StatusCode::BAD_REQUEST, "id invalido");
    }
    let name = text_of(data.get("name"))?;
    let body = text_of(data.get("body"))?;
    let tags = tags_of(data.get("tags"));
    if let Some(message) = validate(&name, &body, &tags) {
        return error(StatusCode::BAD_REQUEST, message);
    }
    let updated = locked(path, move |items| {
        let mut updated = None;
        for it in items.iter_mut() {
            if it.get("id").and_then(Value::as_str) == Some(id.as_str()) {
                let new = item(id.clone(), &name, body.clone(), &tags, now);
                *it = new.clone();
                updated = Some(new);
            }
        }
        (updated.is_some(), updated)
    })
    .await?;
    match updated {
        None => error(StatusCode::NOT_FOUND, "snippet no encontrado"),
        Some(new) => reply(StatusCode::OK, &json!({"item": new})),
    }
}

async fn delete(path: PathBuf, data: &Map<String, Value>) -> Answer {
    let id = text_of(data.get("id"))?;
    if !snippet_id(&id) {
        return error(StatusCode::BAD_REQUEST, "id invalido");
    }
    let removed = locked(path, move |items| {
        let before = items.len();
        items.retain(|it| it.get("id").and_then(Value::as_str) != Some(id.as_str()));
        let removed = items.len() != before;
        (removed, removed)
    })
    .await?;
    if removed {
        reply(StatusCode::OK, &json!({"ok": true}))
    } else {
        error(StatusCode::NOT_FOUND, "snippet no encontrado")
    }
}
```

- [ ] **Step 7: Implementar `ui_log.rs`**

Crear `crates/comandos-server/src/dash/native/ui_log.rs`:

```rust
//! F. `POST /ui-log` (8942, `ui_log_append` 64): anexa eventos de uso a
//! `~/.claude/hooks/ui-events.jsonl` bajo el `flock` del Python y rota a 30
//! días al pasar de 2 MB. Las conversiones van antes del candado (como en el
//! Python); candado tomado o rotación incierta → se declina sin escribir.
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb,
    files::{self, FileLock, Strict},
    light::data,
    py, reply,
};
use crate::{HandlerError, Request};
use comandos_core::json::{response_dumps_unicode, truthy};
use http::StatusCode;
use serde_json::{Map, Value, json};
use std::{fs, io::Write, path::Path};

pub const ROUTES: &[Entry] = &[Entry {
    verb: Verb::Post,
    key: Key::Raw("/ui-log"),
    route: NativeRoute::UiLog,
}];

const ROTATE_BYTES: u64 = 2_000_000;
const KEEP_LINES: usize = 20_000;
const KEEP_SECONDS: f64 = 30.0 * 86_400.0;

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

pub async fn answer(native: &Native, request: &Request) -> Answer {
    let data = data(request)?;
    let now = (native.options().clock)() as f64 / 1000.0;
    let keep = records(data.get("events"), now)?;
    if keep.is_empty() {
        return reply(StatusCode::OK, &json!({"ok": true, "n": 0}));
    }
    let mut text = String::new();
    for record in &keep {
        text.push_str(&response_dumps_unicode(record).map_err(|_| Fault::Decline)?);
        text.push('\n');
    }
    let n = keep.len();
    let path = native.options().hooks.join("ui-events.jsonl");
    let done = tokio::task::spawn_blocking(move || append(&path, &text, now))
        .await
        .map_err(|_| failure())?;
    match done {
        Appended::Done => reply(StatusCode::OK, &json!({"ok": true, "n": n})),
        Appended::Contended | Appended::Unsure => Err(Fault::Decline),
        Appended::Failed => Err(failure()),
    }
}

/// `keep` del Python: dicts de `events[:200]`, campos en su orden.
fn records(events: Option<&Value>, now: f64) -> Result<Vec<Value>, Fault> {
    let items = match events {
        Some(v) if !truthy(v) => return Ok(Vec::new()),
        None => return Ok(Vec::new()),
        Some(Value::Array(items)) => items,
        // `str[:200]` itera caracteres; ninguno es un dict.
        Some(Value::String(_)) => return Ok(Vec::new()),
        // Objeto, número o booleano: `x[:200]` → TypeError no capturado.
        Some(_) => return Err(failure()),
    };
    let mut keep = Vec::new();
    for item in items.iter().take(200) {
        let Value::Object(e) = item else { continue };
        let mut record = Map::new();
        record.insert("ts".into(), ts(e.get("ts"), now)?);
        record.insert("k".into(), Value::String(text(e.get("k"), 24)?));
        record.insert("n".into(), Value::String(text(e.get("n"), 80)?));
        record.insert("c".into(), Value::String(text(e.get("c"), 80)?));
        record.insert("d".into(), json!(int(e.get("d"))?));
        record.insert("s".into(), Value::String(text(e.get("s"), 80)?));
        keep.push(Value::Object(record));
    }
    Ok(keep)
}

/// `float(e.get("ts") or now)`. No finito → declina (el `repr` de inf/nan
/// en el archivo no se arriesga); entero enorme → OverflowError → 500.
fn ts(value: Option<&Value>, now: f64) -> Result<Value, Fault> {
    let x = match value {
        None => now,
        Some(v) if !truthy(v) => now,
        Some(Value::Bool(_)) => 1.0,
        Some(Value::Number(n)) => {
            let raw = n.as_str();
            let x: f64 = raw.parse().map_err(|_| Fault::Decline)?;
            if !x.is_finite() && !raw.contains(['.', 'e', 'E', 'N', 'I']) {
                return Err(failure());
            }
            x
        }
        Some(Value::String(s)) => match py::float(s) {
            Ok(x) => x,
            Err(py::NumError::Invalid) => return Err(failure()),
            Err(py::NumError::Exotic) => return Err(Fault::Decline),
        },
        Some(_) => return Err(failure()),
    };
    if !x.is_finite() {
        return Err(Fault::Decline);
    }
    serde_json::Number::from_f64(x)
        .map(Value::Number)
        .ok_or(Fault::Decline)
}

/// `str(e.get(k) or "")[:n]`.
fn text(value: Option<&Value>, n: usize) -> Result<String, Fault> {
    match value {
        Some(v) if truthy(v) => py::str_scalar(v)
            .map(|s| py::take_chars(&s, n))
            .ok_or(Fault::Decline),
        _ => Ok(String::new()),
    }
}

/// `int(e.get("d") or 0)`.
fn int(value: Option<&Value>) -> Result<i64, Fault> {
    match value {
        Some(v) if truthy(v) => py::int_of(v).map_err(|c| match c {
            py::Conversion::Exotic => Fault::Decline,
            _ => failure(),
        }),
        _ => Ok(0),
    }
}

enum Appended {
    Done,
    Contended,
    Unsure,
    Failed,
}

enum Rotation {
    Skip,
    Write(String),
    Unsure,
}

/// Bajo el candado. La rotación se decide ANTES de anexar sobre lo viejo + lo
/// nuevo (nadie más escribe con el candado tomado): así una línea incierta
/// declina sin haber escrito nada.
fn append(path: &Path, text: &str, now: f64) -> Appended {
    let lock = match FileLock::try_acquire(path) {
        Ok(Some(lock)) => lock,
        Ok(None) => return Appended::Contended,
        Err(_) => return Appended::Failed,
    };
    let before = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let rotation = if before + text.len() as u64 > ROTATE_BYTES {
        rotated(path, text, now)
    } else {
        Rotation::Skip
    };
    if matches!(rotation, Rotation::Unsure) {
        drop(lock);
        return Appended::Unsure;
    }
    let appended = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(text.as_bytes()));
    if appended.is_err() {
        return Appended::Failed;
    }
    if let Rotation::Write(fresh) = rotation {
        // `except Exception: pass` del Python.
        let _ = files::write_text_atomic(path, &fresh);
    }
    drop(lock);
    Appended::Done
}

/// Lo que haría la rotación del Python con el archivo ya anexado.
fn rotated(path: &Path, appended: &str, now: f64) -> Rotation {
    let old = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(_) => return Rotation::Skip,
    };
    // UnicodeDecodeError dentro del `try` externo: no se rota.
    let Ok(old) = String::from_utf8(old) else {
        return Rotation::Skip;
    };
    let all = format!("{old}{appended}")
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let lines: Vec<&str> = all
        .split_inclusive('\n')
        .filter(|l| !py::strip(l).is_empty())
        .collect();
    let start = lines.len().saturating_sub(KEEP_LINES);
    let cutoff = now - KEEP_SECONDS;
    let mut fresh = String::new();
    for line in lines.iter().skip(start) {
        let keep = match files::loads_strict(line) {
            Strict::Value(Value::Object(o)) => match o.get("ts") {
                None => 0.0 >= cutoff,
                Some(Value::Bool(b)) => f64::from(u8::from(*b)) >= cutoff,
                Some(Value::Number(n)) => n.as_str().parse::<f64>().is_ok_and(|x| x >= cutoff),
                // `str >= float`, `None >= float`…: TypeError → se aborta.
                Some(_) => return Rotation::Skip,
            },
            // `.get` de una lista o un escalar: AttributeError → se aborta.
            Strict::Value(_) => return Rotation::Skip,
            Strict::Missing | Strict::Unreadable => false,
            Strict::Unsure => return Rotation::Unsure,
        };
        if keep {
            fresh.push_str(line);
        }
    }
    Rotation::Write(fresh)
}
```

- [ ] **Step 8: Montar las rutas en `mod.rs`**

En `crates/comandos-server/src/dash/native/mod.rs`: añadir `pub mod snippets;` y `pub mod ui_log;` a la lista de módulos; las variantes

```rust
    Snippets(snippets::SnippetsRoute),
    UiLog,
```

a `NativeRoute`; `snippets::ROUTES, ui_log::ROUTES,` a `TABLES` (antes de `retired::ROUTES`); y en `Native::answer`:

```rust
            NativeRoute::Snippets(route) => snippets::answer(self, route, request).await,
            NativeRoute::UiLog => ui_log::answer(self, request).await,
```

- [ ] **Step 9: Ver pasar**

Run: `$C test -p comandos-server --test dash_native_snippets -j 6` → PASS. Run también `$C test -p comandos-server -j 6` (las pruebas de la 2b siguen verdes: `read_json_strict` conserva su clasificación).

- [ ] **Step 10: Líneas de fixture**

Añadir al final de `xtask/parity/frente.jsonl`:

```
# --- 2c · F1: snippets y registro de uso (nativas; los POST escriben, el orden importa)
{"name":"f-snippets","method":"GET","path":"/snippets","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"f-snippets-vacio","method":"POST","path":"/snippets","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"name":"","body":"x"},"volatile":[],"expect":"same"}
{"name":"f-snippets-tags","method":"POST","path":"/snippets","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"name":"a","body":"x","tags":"uno"},"volatile":[],"expect":"same"}
{"name":"f-snippets-nuevo","method":"POST","path":"/snippets","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"name":" Paridad ","body":"echo paridad","tags":[" xtask "]},"volatile":["/item/id","/item/updated_at"],"expect":"same"}
{"name":"f-snippets-tras-nuevo","method":"GET","path":"/snippets","headers":{"Host":"127.0.0.1"},"body":null,"volatile":["/0/id","/0/updated_at"],"expect":"same"}
{"name":"f-snippets-update-id","method":"POST","path":"/snippets/update","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"id":"zz"},"volatile":[],"expect":"same"}
{"name":"f-snippets-update-404","method":"POST","path":"/snippets/update","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"id":"0000000000000000","name":"a","body":"b"},"volatile":[],"expect":"same"}
{"name":"f-snippets-delete-404","method":"POST","path":"/snippets/delete","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"id":"0000000000000000"},"volatile":[],"expect":"same"}
{"name":"f-ui-log","method":"POST","path":"/ui-log","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"events":[{"ts":1791115200.5,"k":"click","n":"btn","c":"x","d":3,"s":"main"},"no"]},"volatile":[],"expect":"same"}
{"name":"f-ui-log-vacio","method":"POST","path":"/ui-log","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"f-ui-log-roto","method":"POST","path":"/ui-log","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"events":[{"d":"x"}]},"volatile":[],"expect":"same"}
```

- [ ] **Step 11: Paridad**

Run (tras `$C build -p comandos-cli -p xtask -j 6`): `.build/target/debug/xtask parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks --state-db ~/.local/state/comandos/app-state.sqlite3 --comandos .build/target/debug/comandos`
Expected: 0 DIFF; en «reenviadas al heredado» ya no aparecen GET `/snippets`, POST `/snippets`, `/snippets/update`, `/snippets/delete`, `/ui-log`. Con `--no-native`: 0 DIFF salvo los `e-*` de la 2b (DIFF por diseño, ver `cutover-dash.md` 2b).

- [ ] **Step 12: fmt, clippy, commit**

```bash
$C fmt --all -- --check
$C clippy --workspace --all-targets -j 6 -- -D warnings
git add crates/comandos-core/src/json/python.rs crates/comandos-core/src/json.rs \
  crates/comandos-core/tests/response_dumps_unicode.rs \
  crates/comandos-server/src/dash/native/files.rs crates/comandos-server/src/dash/native/light.rs \
  crates/comandos-server/src/dash/native/mod.rs crates/comandos-server/src/dash/native/snippets.rs \
  crates/comandos-server/src/dash/native/ui_log.rs crates/comandos-server/tests/dash_native_snippets.rs
git add -f xtask/parity/frente.jsonl
git commit -m "feat(dash): dominio F1 nativo — snippets y registro de uso

GET/POST /snippets, /snippets/update, /snippets/delete y POST /ui-log, rama a
rama de bin/cc-dash, con el mismo flock de <archivo>.lock que el Python: si
está tomado, se declina sin leer ni escribir. La rotación de ui-events.jsonl
se decide antes de anexar para poder declinar sin efectos.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Dominio F2 — GET `/pomodoro` y el carril de la base de uso

Desplegable sola: GET `/pomodoro` (15 s por `pomodoro.js`, 1 s al vencer; también `cc-app`) pasa a Rust. Necesita tres fuentes: `app-state` (bloque, progreso, preferencias de avisos), `H/focus-queue.jsonl` y la base de uso (`focus_settings`). La base de uso entra con su propio carril.

Comportamiento portado (`pomodoro_payload` 6646): `state = pomodoro_store().snapshot()` (`{revision, serverNowMs, block}`), luego `queue` = últimas 50 líneas JSON válidas de `H/focus-queue.jsonl` (`focus_queue` 135: fallo de apertura → `[]`; línea que no parsea → se salta; bytes no UTF-8 → la iteración lanza → 500 → aquí declina), `settings` = `read_focus_settings(USAGE_DB)` (`bin/cc_usage.py:2321`: `init_db` y `select key,value from focus_settings`, valor que no parsea → se omite), `progress` = `ledger_progress(conn, POLICY_V1, serverNowMs)` y `sound` = `{"enabled", "device", "desktopDevice"}` (`pomodoro_sound_route` 6655: `prefs["modes"].get("focus") == "sound" and not prefs.get("muted")`, `sound_device(clients(conn, now), now)`, cualquier excepción → `false`/`null`; `DESKTOP_DEVICE` 999). Efecto lateral: la primera vez, `ensure_policy` (idempotente).

**Files:**
- Modify: `crates/comandos-store/src/usage.rs`; Create: `crates/comandos-store/tests/usage_focus_settings.rs`
- Create: `crates/comandos-server/src/dash/native/lanes.rs`, `crates/comandos-server/src/dash/native/pomodoro.rs`
- Modify: `crates/comandos-server/src/dash/native/state.rs`, `crates/comandos-server/src/dash/native/mod.rs`
- Modify: `crates/comandos-server/tests/support/mod.rs`; Create: `crates/comandos-server/tests/dash_native_pomodoro.rs`
- Modify: `xtask/src/parity.rs`, `xtask/src/poll.rs`, `xtask/parity/README.md`, `xtask/parity/frente.jsonl`

**Interfaces:**
- Produces: `comandos_store::usage::{SCHEMA_VERSION (pub), schema_version(&Connection) -> Result<i64>, ensure_schema(&Connection) -> Result<()>, focus_settings_rows(&Connection) -> Result<Vec<(String, Option<String>)>>}`; `lanes::{LaneBackend, Lane<B>, UsageBackend}` con `Lane::{new(PathBuf), enabled, refusals, with(job) -> Result<T, Fault>, shutdown}`; `Refusal::lane_message(&Path, &str)`; `NativeOptions.{usage_db, desktop_device}`, `native::desktop_device()`; `Native.usage`; `NativeRoute::Pomodoro`; `StateBackend.pomodoro_policy`; `support::TestHome::usage_db()`; `xtask parity|poll --usage-db`.
- Consumes: `comandos_store::{pomodoro::PomodoroStore, focus::{ensure_policy, ledger_progress}, notifications::{load_prefs, clients}}`, `comandos_core::{focus::policy_v1, notifications::sound_device}`.

- [ ] **Step 1: Prueba que falla del store**

Crear `crates/comandos-store/tests/usage_focus_settings.rs`:

```rust
//! `read_focus_settings` de `bin/cc_usage.py`: `init_db` + `select key,value`.
use comandos_store::usage::{
    SCHEMA_VERSION, ensure_schema, focus_settings_rows, open_usage_db_at, schema_version,
};

#[test]
fn focus_settings_rows_creates_schema_and_reads_table_order() {
    let dir = std::env::temp_dir().join(format!("cmd-usage-focus-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let conn = open_usage_db_at(&dir.join("u.sqlite")).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), 0);
    assert!(focus_settings_rows(&conn).unwrap().is_empty());
    assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
    conn.execute_batch(
        "insert into focus_settings(key,value) values('style','\"garden\"'),('cycles','4'),('roto','{')",
    )
    .unwrap();
    assert_eq!(
        focus_settings_rows(&conn).unwrap(),
        vec![
            ("style".to_owned(), Some("\"garden\"".to_owned())),
            ("cycles".to_owned(), Some("4".to_owned())),
            ("roto".to_owned(), Some("{".to_owned())),
        ]
    );
    ensure_schema(&conn).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
```

Run: `$C test -p comandos-store --test usage_focus_settings` → FAIL (`schema_version`, `ensure_schema`, `focus_settings_rows` no existen).

- [ ] **Step 2: Implementar en `usage.rs`**

En `crates/comandos-store/src/usage.rs`: `const SCHEMA_VERSION` pasa a `pub const SCHEMA_VERSION`, y debajo de `user_version` añadir:

```rust
/// `pragma user_version` (la puerta del carril de uso del frente).
pub fn schema_version(conn: &Connection) -> Result<i64> {
    user_version(conn)
}

/// `init_db` de Python: tablas base y migración hasta `SCHEMA_VERSION`.
pub fn ensure_schema(conn: &Connection) -> Result<()> {
    init_db(conn)
}

/// `read_focus_settings` = `set_focus_settings(db, {})`: `init_db` y las filas
/// en el orden de la tabla (misma consulta, sin ORDER BY). Un valor que no es
/// texto se entrega como `None` para que quien llama decida.
pub fn focus_settings_rows(conn: &Connection) -> Result<Vec<(String, Option<String>)>> {
    init_db(conn)?;
    let mut stmt = conn.prepare("select key,value from focus_settings")?;
    let rows = stmt
        .query_map([], |r| {
            let value = match r.get_ref(1)? {
                ValueRef::Text(t) => std::str::from_utf8(t).ok().map(str::to_owned),
                _ => None,
            };
            Ok((r.get::<_, String>(0)?, value))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
```

Run: `$C test -p comandos-store --test usage_focus_settings` → PASS; `$C test -p comandos-store` sigue verde.

- [ ] **Step 3: Carriles (`lanes.rs`) y mensaje de carril (`state.rs`)**

En `crates/comandos-server/src/dash/native/state.rs` añadir a `impl Refusal`:

```rust
    /// La línea de un carril: nombra la base y las rutas que dejan de ser nativas.
    pub fn lane_message(&self, path: &Path, routes: &str) -> String {
        let detail = match self {
            Refusal::Newer { found, known } => {
                format!("tiene esquema {found} y este binario conoce hasta {known}")
            }
            Refusal::Unopened(error) => format!("no se pudo abrir: {error}"),
            Refusal::Retired => "su worker se retiró tras un fallo".to_owned(),
        };
        format!(
            "comandos dash: {}: {detail}; {routes} se reenvían al heredado",
            path.display()
        )
    }
```

y en `StateBackend` el campo `pub(crate) pomodoro_policy: bool` (en `open`, `Self { conn, events: None, pomodoro_policy: false }`).

Crear `crates/comandos-server/src/dash/native/lanes.rs`:

```rust
//! Carriles de base adicionales: un `BackendWorker` por archivo SQLite que no
//! es `app-state` (la base de uso; el journal de operaciones en la Tarea 6).
//! Misma semántica que `Native::with_state`, pero el apagado es del carril:
//! una línea en stderr y solo las rutas que lo usan pasan a reenviarse.
use super::{Fault, WORKER_CAPACITY, state::Refusal};
use crate::blocking::{BackendCaller, BackendWorker};
use comandos_store::usage;
use rusqlite::Connection;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio::sync::OnceCell;

pub trait LaneBackend: Sized + Send + 'static {
    /// Rutas que dependen del carril, para la línea de stderr.
    const ROUTES: &'static str;
    /// Abre y migra; la puerta va ANTES de tocar una base más nueva.
    fn open(path: &Path) -> Result<Self, Refusal>;
    /// Se evalúa antes de cada trabajo.
    fn admit(&self) -> Result<(), Refusal>;
}

pub struct Lane<B: LaneBackend> {
    path: PathBuf,
    enabled: AtomicBool,
    refusals: AtomicUsize,
    caller: OnceCell<Option<BackendCaller<B>>>,
    worker: Mutex<Option<BackendWorker<B>>>,
}

impl<B: LaneBackend> Lane<B> {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            enabled: AtomicBool::new(true),
            refusals: AtomicUsize::new(0),
            caller: OnceCell::new(),
            worker: Mutex::new(None),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Cuántas veces se escribió la línea de apagado (0 o 1).
    pub fn refusals(&self) -> usize {
        self.refusals.load(Ordering::Acquire)
    }

    fn disable(&self, refusal: &Refusal) {
        if self.enabled.swap(false, Ordering::AcqRel) {
            self.refusals.fetch_add(1, Ordering::AcqRel);
            eprintln!("{}", refusal.lane_message(&self.path, B::ROUTES));
        }
    }

    async fn caller(&self) -> Option<&BackendCaller<B>> {
        let caller = self
            .caller
            .get_or_init(|| async {
                let path = self.path.clone();
                match tokio::task::spawn_blocking(move || B::open(&path)).await {
                    Ok(Ok(backend)) => match BackendWorker::start(WORKER_CAPACITY, backend) {
                        Ok(worker) => {
                            let caller = worker.caller();
                            *self.worker.lock().unwrap_or_else(|p| p.into_inner()) = Some(worker);
                            Some(caller)
                        }
                        Err(error) => {
                            self.disable(&Refusal::Unopened(error.to_string()));
                            None
                        }
                    },
                    Ok(Err(refusal)) => {
                        self.disable(&refusal);
                        None
                    }
                    Err(join) => {
                        self.disable(&Refusal::Unopened(join.to_string()));
                        None
                    }
                }
            })
            .await;
        caller.as_ref().filter(|_| self.enabled())
    }

    /// Un trabajo sobre la base del carril; `Err(Fault::Decline)` si el carril
    /// está apagado, si la puerta rechaza ahora o si el worker se retiró sin
    /// empezar el trabajo. Solo el trabajo que entró en pánico responde 500.
    pub async fn with<T, F>(&self, job: F) -> Result<T, Fault>
    where
        F: FnOnce(&mut B) -> T + Send + 'static,
        T: Send + 'static,
    {
        if !self.enabled() {
            return Err(Fault::Decline);
        }
        let Some(caller) = self.caller().await else {
            return Err(Fault::Decline);
        };
        if caller.stopped() {
            self.disable(&Refusal::Retired);
            return Err(Fault::Decline);
        }
        let started = Arc::new(AtomicBool::new(false));
        let mark = started.clone();
        let called = caller
            .call(move |backend: &mut B| {
                mark.store(true, Ordering::Release);
                backend.admit().map(|()| job(backend))
            })
            .await;
        match called {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(refusal)) => {
                self.disable(&refusal);
                Err(Fault::Decline)
            }
            Err(error) if started.load(Ordering::Acquire) => {
                self.disable(&Refusal::Retired);
                Err(Fault::Error(error))
            }
            Err(_) => {
                if caller.stopped() {
                    self.disable(&Refusal::Retired);
                }
                Err(Fault::Decline)
            }
        }
    }

    pub async fn shutdown(&self) {
        self.enabled.store(false, Ordering::Release);
        let worker = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(worker) = worker {
            let _ = worker.shutdown().await;
        }
    }
}

/// `~/.claude/hooks/comandos-usage.sqlite` (`USAGE_DB` de `bin/cc-dash:174`,
/// que no honra `COMANDOS_USAGE_DB`). Puerta: `user_version` <= 11.
pub struct UsageBackend {
    pub conn: Connection,
}

impl LaneBackend for UsageBackend {
    const ROUTES: &'static str = "GET /pomodoro y GET /sovereignty";

    fn open(path: &Path) -> Result<Self, Refusal> {
        let conn = usage::open_usage_db_at(path).map_err(|e| Refusal::Unopened(e.to_string()))?;
        let backend = Self { conn };
        backend.admit()?;
        usage::ensure_schema(&backend.conn).map_err(|e| Refusal::Unopened(e.to_string()))?;
        Ok(backend)
    }

    fn admit(&self) -> Result<(), Refusal> {
        let found =
            usage::schema_version(&self.conn).map_err(|e| Refusal::Unopened(e.to_string()))?;
        if found > usage::SCHEMA_VERSION {
            return Err(Refusal::Newer {
                found,
                known: usage::SCHEMA_VERSION,
            });
        }
        Ok(())
    }
}
```

- [ ] **Step 4: Opciones, estado y ruta en `mod.rs`**

En `crates/comandos-server/src/dash/native/mod.rs`:

- `pub mod lanes;` y `pub mod pomodoro;`.
- `NativeRoute::Pomodoro`; `pomodoro::ROUTES` en `TABLES`; en `Native::answer`: `NativeRoute::Pomodoro => pomodoro::answer(self).await,`.
- En `NativeOptions` los campos y su valor en `for_home`:

```rust
    /// `~/.claude/hooks/comandos-usage.sqlite`.
    pub usage_db: PathBuf,
    /// `DESKTOP_DEVICE` del Python (999).
    pub desktop_device: String,
```

```rust
            usage_db: home.join(".claude/hooks/comandos-usage.sqlite"),
            desktop_device: desktop_device(),
```

- La función (junto a `wall_clock_ms`):

```rust
/// `"desktop-" + re.sub(r"[^A-Za-z0-9_.-]", "-", os.uname().nodename or "local")[:60]`.
/// `/proc/sys/kernel/hostname` es el `nodename` de `uname`.
pub fn desktop_device() -> String {
    let node = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim_end_matches('\n').to_owned())
        .unwrap_or_default();
    let node = if node.is_empty() { "local".to_owned() } else { node };
    let clean: String = node
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
                c
            } else {
                '-'
            }
        })
        .take(60)
        .collect();
    format!("desktop-{clean}")
}
```

- En `Native`: campo `pub(crate) usage: lanes::Lane<lanes::UsageBackend>`, en `new` `usage: lanes::Lane::new(opts.usage_db.clone()),` (antes de mover `opts`), y en `shutdown`, tras parar el worker de la base: `self.usage.shutdown().await;`.

- [ ] **Step 5: Pruebas que fallan del dominio**

En `crates/comandos-server/tests/support/mod.rs`, dentro de `impl TestHome`:

```rust
    pub fn usage_db(&self) -> PathBuf {
        self.hooks().join("comandos-usage.sqlite")
    }
```

Crear `crates/comandos-server/tests/dash_native_pomodoro.rs`:

```rust
//! Dominio F2: GET /pomodoro con app-state, la cola de foco y la base de uso.
mod support;

use comandos_server::dash::native::wall_clock_ms;
use std::sync::Arc;
use support::{FakeLegacy, TestHome, dead_port, front, get, oracle::oracle, request_body};

/// `serverNowMs` difiere entre dos procesos: se iguala a 0 antes de comparar bytes.
fn masked(text: &str) -> String {
    let key = "\"serverNowMs\": ";
    let Some(at) = text.find(key) else { return text.to_owned() };
    let start = at + key.len();
    let end = start + text[start..].bytes().take_while(u8::is_ascii_digit).count();
    format!("{}0{}", &text[..start], &text[end..])
}

fn seed_usage(home: &TestHome, rows: &[(&str, &str)]) {
    let conn = comandos_store::usage::open_usage_db_at(&home.usage_db()).unwrap();
    comandos_store::usage::ensure_schema(&conn).unwrap();
    for (key, value) in rows {
        conn.execute(
            "insert into focus_settings(key,value) values(?,?)",
            rusqlite::params![key, value],
        )
        .unwrap();
    }
}

#[tokio::test]
async fn pomodoro_is_native_with_python_shape() {
    let home = TestHome::new("pomo-shape");
    seed_usage(&home, &[("focusMinutes", "50"), ("style", "\"garden\""), ("roto", "{")]);
    home.write("focus-queue.jsonl", "{\"a\": 1}\nroto\n\n{\"b\": [2]}\n");
    let front = front(&home, dead_port(), home.options()).await;
    let wire = get(front.port, "/pomodoro").await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    let body = wire.text();
    assert!(body.starts_with(r#"{"revision": "#), "{body}");
    assert!(body.contains(r#""serverNowMs": 1791115200000, "block": null, "queue": [{"a": 1}, {"b": [2]}], "settings": {"focusMinutes": 50, "style": "garden"}, "progress": {"#), "{body}");
    let device = comandos_server::dash::native::desktop_device();
    assert!(body.ends_with(&format!(r#""sound": {{"enabled": false, "device": null, "desktopDevice": "{device}"}}}}"#)), "{body}");
    front.stop().await;
}

#[tokio::test]
async fn pomodoro_matches_python_oracle() {
    let home = TestHome::new("pomo-oracle");
    seed_usage(&home, &[("cycles", "4"), ("autoBreak", "true")]);
    home.write("focus-queue.jsonl", "{\"title\": \"ñ\"}\n");
    let Some(py) = oracle(&home).await else { return };
    let mut opts = home.options();
    opts.clock = Arc::new(wall_clock_ms);
    let front = front(&home, dead_port(), opts).await;
    let a = get(py.port, "/pomodoro").await;
    let b = get(front.port, "/pomodoro").await;
    assert_eq!((a.status, masked(&a.text())), (b.status, masked(&b.text())));
    // Con el foco en «sonido», `sound.enabled` cambia en los dos.
    let prefs = r#"{"modes": {"focus": "sound"}}"#;
    assert_eq!(request_body(front.port, "POST", "/notices/prefs", "", prefs).await.status, 200);
    let a = get(py.port, "/pomodoro").await;
    let b = get(front.port, "/pomodoro").await;
    assert_eq!(masked(&a.text()), masked(&b.text()));
    assert!(b.text().contains(r#""sound": {"enabled": true"#), "{}", b.text());
    front.stop().await;
}

#[tokio::test]
async fn usage_newer_schema_disables_only_usage_lane() {
    let home = TestHome::new("pomo-newer");
    seed_usage(&home, &[]);
    rusqlite::Connection::open(home.usage_db())
        .unwrap()
        .execute_batch("pragma user_version=12")
        .unwrap();
    home.write("snippets.json", "[]");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    for _ in 0..2 {
        assert_eq!(get(front.port, "/pomodoro").await.text(), r#"{"legacy": true}"#);
    }
    assert_eq!(get(front.port, "/snippets").await.text(), "[]", "las demás siguen nativas");
    assert_eq!(get(front.port, "/notices/prefs").await.status, 200);
    let version: i64 = rusqlite::Connection::open(home.usage_db())
        .unwrap()
        .query_row("pragma user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 12, "nunca se baja la versión");
    assert_eq!(legacy.requests(), vec!["GET /pomodoro HTTP/1.1".to_owned(); 2]);
    front.stop().await;
}

#[tokio::test]
async fn undecodable_queue_declines() {
    let home = TestHome::new("pomo-queue");
    std::fs::write(home.hooks().join("focus-queue.jsonl"), b"{\"a\": 1}\n\xff\n").unwrap();
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    assert_eq!(get(front.port, "/pomodoro").await.text(), r#"{"legacy": true}"#);
    front.stop().await;
}
```

Run: `$C test -p comandos-server --test dash_native_pomodoro -j 6` → FAIL (`/pomodoro` no es nativa).

- [ ] **Step 6: Implementar `pomodoro.rs`**

Crear `crates/comandos-server/src/dash/native/pomodoro.rs`:

```rust
//! F. GET /pomodoro (8448, `pomodoro_payload` 6646): bloque y progreso de
//! app-state, cola de `H/focus-queue.jsonl`, ajustes de la base de uso y la
//! ruta de sonido de los avisos. POST /pomodoro sigue en el Python: despierta
//! su scheduler en memoria (`_POMODORO_WAKE`, 6820).
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb,
    files::{self, Strict},
    light::read_reply,
};
use crate::HandlerError;
use comandos_core::{focus::policy_v1, json::truthy, notifications::sound_device};
use comandos_store::{focus, notifications as nd, pomodoro::PomodoroStore, usage};
use serde_json::{Map, Value, json};
use std::path::Path;

pub const ROUTES: &[Entry] = &[Entry {
    verb: Verb::Get,
    key: Key::Raw("/pomodoro"),
    route: NativeRoute::Pomodoro,
}];

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

pub async fn answer(native: &Native) -> Answer {
    let clock = native.options().clock.clone();
    let (state, progress, sound) = native
        .with_state(move |b| -> Result<(Value, Value, (bool, Value)), Fault> {
            let policy = policy_v1();
            let now = clock();
            if !b.pomodoro_policy {
                focus::ensure_policy(&b.conn, &policy, now).map_err(|_| failure())?;
                b.pomodoro_policy = true;
            }
            let new_id = String::new;
            let state = PomodoroStore::new(&b.conn, &*clock, &new_id)
                .snapshot()
                .map_err(|_| failure())?;
            let server_now = state["serverNowMs"].as_i64().ok_or_else(failure)?;
            let progress =
                focus::ledger_progress(&b.conn, &policy, server_now).map_err(|_| failure())?;
            let sound_now = clock();
            // `except Exception: enabled, device = False, None`.
            let sound = match (nd::load_prefs(&b.conn), nd::clients(&b.conn, sound_now)) {
                (Ok(prefs), Ok(clients)) => {
                    let enabled = prefs["modes"].get("focus") == Some(&json!("sound"))
                        && !truthy(&prefs["muted"]);
                    (enabled, sound_device(&clients, sound_now))
                }
                _ => (false, Value::Null),
            };
            Ok((state, progress, sound))
        })
        .await??;
    let queue = focus_queue(&native.options().hooks.join("focus-queue.jsonl"))?;
    let rows = native
        .usage
        .with(|u| usage::focus_settings_rows(&u.conn))
        .await?
        .map_err(|_| failure())?;
    let mut settings = Map::new();
    for (key, value) in rows {
        // `json.loads(value)` con `except Exception: pass`.
        match value.as_deref().map(files::loads_strict) {
            Some(Strict::Value(v)) => {
                settings.insert(key, v);
            }
            Some(Strict::Missing | Strict::Unreadable) => {}
            Some(Strict::Unsure) | None => return Err(Fault::Decline),
        }
    }
    let Value::Object(mut out) = state else {
        return Err(failure());
    };
    out.insert("queue".into(), Value::Array(queue));
    out.insert("settings".into(), Value::Object(settings));
    out.insert("progress".into(), progress);
    let (enabled, device) = sound;
    out.insert(
        "sound".into(),
        json!({
            "enabled": enabled,
            "device": if enabled { device } else { Value::Null },
            "desktopDevice": native.options().desktop_device,
        }),
    );
    read_reply(&Value::Object(out))
}

/// `focus_queue` (135): líneas JSON válidas, las últimas 50. Modo texto: bytes
/// no UTF-8 lanzan fuera del `try` interno (500 en el Python) → se declina.
fn focus_queue(path: &Path) -> Result<Vec<Value>, Fault> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(Vec::new()),
    };
    let text = String::from_utf8(bytes).map_err(|_| Fault::Decline)?;
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = Vec::new();
    for line in text.split_inclusive('\n') {
        match files::loads_strict(line) {
            Strict::Value(v) => out.push(v),
            Strict::Missing | Strict::Unreadable => {}
            Strict::Unsure => return Err(Fault::Decline),
        }
    }
    let start = out.len().saturating_sub(50);
    Ok(out.split_off(start))
}
```

(`with_state(...).await??`: el primer `?` es el `Fault` del worker, el segundo el del trabajo.)

- [ ] **Step 7: Ver pasar**

Run: `$C test -p comandos-server --test dash_native_pomodoro -j 6` → PASS; `$C test -p comandos-server -j 6` verde.

- [ ] **Step 8: Arnés — `--usage-db`**

En `xtask/src/parity.rs`:

- `StackOptions` gana `pub usage_db: Option<&'a Path>,` (comentario: «Base de uso copiada (backup de SQLite) a `~/.claude/hooks/comandos-usage.sqlite` de los dos HOME»).
- Junto a `copy_state_db`:

```rust
/// Copia la base de uso con la API de backup sobre la que trajo `cp -a`
/// (que puede estar a medias si la real tenía WAL): nunca escribe en `src`.
fn copy_usage_db(src: &Path, home: &Path) -> Result<(), String> {
    let dest = home.join(".claude/hooks/comandos-usage.sqlite");
    for suffix in ["", "-wal", "-shm"] {
        let mut name = dest.as_os_str().to_owned();
        name.push(suffix);
        let _ = fs::remove_file(PathBuf::from(name));
    }
    let source = rusqlite::Connection::open_with_flags(
        src,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("--usage-db {}: {e}", src.display()))?;
    source
        .backup(rusqlite::MAIN_DB, &dest, None)
        .map_err(|e| format!("backup de {}: {e}", src.display()))
}
```

- Tras el bloque `if let Some(db) = o.state_db { … }`:

```rust
        if let Some(db) = o.usage_db {
            copy_usage_db(db, &home1)?;
            copy_usage_db(db, &home2)?;
        }
```

- `Args` gana `usage_db: Option<PathBuf>`; en `parse_args`, `"--usage-db" => usage_db = Some(it.next().map(PathBuf::from).ok_or("--usage-db sin ruta")?),`; en `run`, `usage_db: a.usage_db.as_deref(),` al construir `StackOptions`.
- En `xtask/src/poll.rs`, el mismo argumento en su `parse_args` y `usage_db: …as_deref()` donde construye `StackOptions` (el compilador señala el sitio).
- En `xtask/parity/README.md`, bajo el uso: `--usage-db <ruta>`: copia de solo lectura (backup) de la base de uso a los dos HOME; usar con `~/.claude/hooks/comandos-usage.sqlite` para que las rutas que la leen comparen sobre datos consistentes.

Run: `$C build -p xtask -j 6` → compila.

- [ ] **Step 9: Fixture y paridad**

```
# --- 2c · F2: Pomodoro (nativa; lee app-state, focus-queue y la base de uso)
{"name":"f-pomodoro","method":"GET","path":"/pomodoro","headers":{"Host":"127.0.0.1"},"body":null,"volatile":["/serverNowMs"],"expect":"same"}
{"name":"f-pomodoro-consulta","method":"GET","path":"/pomodoro?x=1","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same","forwarded":true}
```

Run: `.build/target/debug/xtask parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks --state-db ~/.local/state/comandos/app-state.sqlite3 --usage-db ~/.claude/hooks/comandos-usage.sqlite --comandos .build/target/debug/comandos`
Expected: 0 DIFF; GET `/pomodoro` ya no está en «reenviadas»; `/pomodoro?x=1` sí (no es la ruta exacta). Si `f-pomodoro` diera DIFF solo en `/progress` cerca de medianoche, repetir: el progreso depende del día local de cada proceso.

- [ ] **Step 10: fmt, clippy, commit**

```bash
$C fmt --all -- --check
$C clippy --workspace --all-targets -j 6 -- -D warnings
git add crates/comandos-store/src/usage.rs crates/comandos-store/tests/usage_focus_settings.rs \
  crates/comandos-server/src/dash/native/lanes.rs crates/comandos-server/src/dash/native/pomodoro.rs \
  crates/comandos-server/src/dash/native/state.rs crates/comandos-server/src/dash/native/mod.rs \
  crates/comandos-server/tests/support/mod.rs crates/comandos-server/tests/dash_native_pomodoro.rs \
  xtask/src/parity.rs xtask/src/poll.rs xtask/parity/README.md
git add -f xtask/parity/frente.jsonl
git commit -m "feat(dash): dominio F2 nativo — GET /pomodoro y carril de la base de uso

La base de uso (~/.claude/hooks/comandos-usage.sqlite, la que usa cc-dash sin
importar COMANDOS_USAGE_DB) entra con su propio worker, su puerta de
user_version y su apagado: una base más nueva reenvía solo /pomodoro y
/sovereignty. El arnés gana --usage-db (copia de solo lectura por backup).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Dominio F3 — catálogos de solo lectura: GET `/model-tiers` y GET `/sovereignty`

Desplegable sola. `/model-tiers` (`index.html:2198`, una vez al cargar) sirve `config/model-tiers.json` del repositorio; `/sovereignty` (el panel «Soberanía») inventaría archivos, bases y canales. Ninguna escribe.

- **GET `/model-tiers`** (8413, `load_model_tiers` 3832): el Python cachea por `mtime` y, si el archivo falta o no parsea, devuelve lo último que cargó (o `{}`). El nativo: objeto → tal cual; JSON válido no objeto → `{}`; ausente, ilegible o incierto → `Decline` (solo el Python sabe qué tiene en caché).
- **GET `/sovereignty`** (8450, `sovereignty_report` 4393): `stores` en este orden — la base de uso (`finfo` + `tables`: `select name from sqlite_master where type='table' and name not like 'sqlite_%'` y `count(*)` de cada una; error → `[]`), `{"label":"Estado por sesión (hooks)","path":"~/.claude/hooks/state/","kind":"json","count":len(glob(STATE/*.json))}`, `events.jsonl` (`finfo` + `count` de líneas en binario), los 10 archivos de `H` en su orden, transcripts (`~/.claude/projects/*/*.jsonl`), rollouts de Codex (solo si hay), servidores SSH (líneas `Host` sin `*`); `browser` fijo; `outbound` con `webterm-enabled` y `NATIVE_NOTIFY` de `cc-notify.conf`; `generatedAt = int(time.time())`. `finfo` = `{label, path (con $HOME → ~), kind, bytes, mtime: int, mode: oct(), private: mode & 0o077 == 0}`. `glob` de Python: `*` no casa nombres que empiezan por `.`; los componentes intermedios deben ser directorios (siguiendo symlinks). `~/.ssh/config` o `cc-notify.conf` no UTF-8 → `UnicodeDecodeError` fuera de su `except OSError` → 500 → aquí `Decline`.

**Files:**
- Modify: `crates/comandos-server/src/dash/mod.rs` (`DashConfig.repo_root`, `repo_root()`, `REPO_ENV`, `from_env`, `build`)
- Create: `crates/comandos-server/src/dash/native/catalogs.rs`; Modify: `crates/comandos-server/src/dash/native/mod.rs`
- Modify: `crates/comandos-server/tests/support/mod.rs`; Create: `crates/comandos-server/tests/dash_native_catalogs.rs`
- Modify: `xtask/parity/frente.jsonl`

**Interfaces:**
- Produces: `dash::{repo_root(&Path, Option<&str>) -> Option<PathBuf>, REPO_ENV}`, `DashConfig.repo_root: Option<PathBuf>`, `NativeOptions.repo_root: Option<PathBuf>`; `catalogs::{CatalogRoute, ROUTES, answer}`, `NativeRoute::Catalog(CatalogRoute)`; `support::{repo, TestHome::options con repo_root}`.
- Consumes: carril de uso (Tarea 2), `files::read_json_strict`, `light::read_reply`.

- [ ] **Step 1: Raíz del repositorio en el frente**

En `crates/comandos-server/src/dash/mod.rs`:

```rust
/// Raíz del checkout del Python heredado (`REPO_ROOT`, `bin/cc-dash:1370`).
pub const REPO_ENV: &str = "COMANDOS_DASH_REPO";

/// `COMANDOS_DASH_REPO` si está; si no, el destino canónico de
/// `<dash_dir>/index.html` dos niveles arriba (`install.sh` y el arnés
/// enlazan `dash/*` al checkout desde el que corre el Python).
pub fn repo_root(dash_dir: &Path, override_dir: Option<&str>) -> Option<PathBuf> {
    if let Some(raw) = override_dir.filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(raw));
    }
    let index = std::fs::canonicalize(dash_dir.join("index.html")).ok()?;
    index.parent()?.parent().map(Path::to_path_buf)
}
```

`DashConfig` gana `pub repo_root: Option<PathBuf>,` (también en su `Debug`); `parse_args` lo deja en `None`; `from_env`, tras resolver `cfg.dash_dir`: `cfg.repo_root = repo_root(&cfg.dash_dir, std::env::var(REPO_ENV).ok().as_deref());`; en `build`, el cierre que crea las opciones:

```rust
        Arc::new(native::Native::new(opts.unwrap_or_else(|| {
            let mut o = native::NativeOptions::for_home(&cfg.home, cfg.state_db.clone());
            o.repo_root = cfg.repo_root.clone();
            o
        })))
```

En `native/mod.rs`, `NativeOptions` gana `pub repo_root: Option<PathBuf>,` (`None` en `for_home`).

En `tests/support/mod.rs`:

```rust
/// El checkout de las pruebas: el oráculo corre `bin/cc-dash` desde aquí.
pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
```

y en `TestHome::options`, antes de devolver: `opts.repo_root = std::fs::canonicalize(repo()).ok();`.

- [ ] **Step 2: Pruebas que fallan**

Crear `crates/comandos-server/tests/dash_native_catalogs.rs`:

```rust
//! Dominio F3: /model-tiers del repositorio y /sovereignty (inventario local).
mod support;

use comandos_server::dash::{native::wall_clock_ms, repo_root};
use std::{fs, sync::Arc};
use support::{FakeLegacy, TestHome, dead_port, front, get, oracle::oracle, repo};

fn masked(text: &str, key: &str) -> String {
    let key = format!("\"{key}\": ");
    let Some(at) = text.find(&key) else { return text.to_owned() };
    let start = at + key.len();
    let end = start + text[start..].bytes().take_while(u8::is_ascii_digit).count();
    format!("{}0{}", &text[..start], &text[end..])
}

#[test]
fn repo_root_follows_dash_index_symlink() {
    let home = TestHome::new("repo-root");
    let dash = home.root.join("dash");
    fs::create_dir_all(&dash).unwrap();
    std::os::unix::fs::symlink(repo().join("dash/index.html"), dash.join("index.html")).unwrap();
    assert_eq!(repo_root(&dash, None), fs::canonicalize(repo()).ok());
    assert_eq!(repo_root(&dash, Some("/otro")), Some("/otro".into()));
    assert_eq!(repo_root(&home.root.join("nada"), None), None);
}

#[tokio::test]
async fn model_tiers_serves_repo_file_like_python() {
    let home = TestHome::new("tiers");
    let front = front(&home, dead_port(), home.options()).await;
    let wire = get(front.port, "/model-tiers").await;
    assert_eq!(wire.status, 200);
    assert!(wire.text().starts_with('{'));
    if let Some(py) = oracle(&home).await {
        for target in ["/model-tiers", "/model-tiers?x=1"] {
            let a = get(py.port, target).await;
            let b = get(front.port, target).await;
            assert_eq!((a.status, a.text()), (b.status, b.text()), "{target}");
        }
    }
    front.stop().await;
}

#[tokio::test]
async fn model_tiers_declines_without_readable_file() {
    let home = TestHome::new("tiers-decline");
    let fake = home.root.join("repo");
    fs::create_dir_all(fake.join("config")).unwrap();
    let legacy = FakeLegacy::start().await;
    let mut opts = home.options();
    opts.repo_root = Some(fake.clone());
    let front = front(&home, legacy.port, opts).await;
    assert_eq!(get(front.port, "/model-tiers").await.text(), r#"{"legacy": true}"#, "ausente");
    fs::write(fake.join("config/model-tiers.json"), "{roto").unwrap();
    assert_eq!(get(front.port, "/model-tiers").await.text(), r#"{"legacy": true}"#, "ilegible");
    fs::write(fake.join("config/model-tiers.json"), "[1]").unwrap();
    assert_eq!(get(front.port, "/model-tiers").await.text(), "{}", "no objeto → {}");
    front.stop().await;
}

#[tokio::test]
async fn sovereignty_matches_python_oracle() {
    let home = TestHome::new("sovereignty");
    home.write("prefs.json", "{}");
    home.write("snippets.json", "[]");
    home.write("events.jsonl", "a\nb\nc");
    home.write("cc-notify.conf", "# x\nNATIVE_NOTIFY=\"1\"\nVOLUME=40\n");
    home.write("webterm-enabled", "");
    home.write("state/uno.json", "{}");
    home.write("state/.oculto.json", "{}");
    fs::create_dir_all(home.root.join(".claude/projects/p")).unwrap();
    fs::write(home.root.join(".claude/projects/p/t.jsonl"), "").unwrap();
    fs::write(home.root.join(".claude/projects/p/.h.jsonl"), "").unwrap();
    fs::create_dir_all(home.root.join(".codex/sessions/2026/10/04")).unwrap();
    fs::write(home.root.join(".codex/sessions/2026/10/04/rollout-1.jsonl"), "").unwrap();
    fs::create_dir_all(home.root.join(".ssh")).unwrap();
    fs::write(home.root.join(".ssh/config"), "Host a\n  host b\nHost *\nhost c\n").unwrap();
    let conn = comandos_store::usage::open_usage_db_at(&home.usage_db()).unwrap();
    comandos_store::usage::ensure_schema(&conn).unwrap();
    conn.execute("insert into focus_settings(key,value) values('a','1')", [])
        .unwrap();
    drop(conn);
    let Some(py) = oracle(&home).await else { return };
    let mut opts = home.options();
    opts.clock = Arc::new(wall_clock_ms);
    let front = front(&home, dead_port(), opts).await;
    let a = get(py.port, "/sovereignty").await;
    let b = get(front.port, "/sovereignty").await;
    assert_eq!(b.status, 200, "{}", b.text());
    assert_eq!(masked(&a.text(), "generatedAt"), masked(&b.text(), "generatedAt"));
    assert!(b.text().contains(r#""label": "Servidores SSH", "path": "~/.ssh/config", "kind": "conf", "count": 3}"#), "{}", b.text());
    front.stop().await;
}

#[tokio::test]
async fn sovereignty_declines_on_undecodable_conf() {
    let home = TestHome::new("sovereignty-bytes");
    fs::write(home.hooks().join("cc-notify.conf"), b"NATIVE_NOTIFY=1\n\xff\n").unwrap();
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    assert_eq!(get(front.port, "/sovereignty").await.text(), r#"{"legacy": true}"#);
    front.stop().await;
}
```

(`TestHome::write` escribe bajo `hooks()`; para `state/uno.json` crear antes el directorio: `TestHome::new` ya crea `.claude/hooks/state`.)

Run: `$C test -p comandos-server --test dash_native_catalogs -j 6` → FAIL.

- [ ] **Step 3: Implementar `catalogs.rs`**

Crear `crates/comandos-server/src/dash/native/catalogs.rs`:

```rust
//! F. Catálogos de solo lectura: GET /model-tiers (8413, `load_model_tiers`
//! 3832) y GET /sovereignty (8450, `sovereignty_report` 4393).
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb,
    files::{self, Strict},
    light::read_reply,
    py, reply,
};
use crate::HandlerError;
use http::StatusCode;
use serde_json::{Map, Value, json};
use std::{
    fs,
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogRoute {
    ModelTiers,
    Sovereignty,
}

pub const ROUTES: &[Entry] = &[
    Entry {
        verb: Verb::Get,
        key: Key::Path("/model-tiers"),
        route: NativeRoute::Catalog(CatalogRoute::ModelTiers),
    },
    Entry {
        verb: Verb::Get,
        key: Key::Raw("/sovereignty"),
        route: NativeRoute::Catalog(CatalogRoute::Sovereignty),
    },
];

pub async fn answer(native: &Native, route: CatalogRoute) -> Answer {
    match route {
        CatalogRoute::ModelTiers => model_tiers(native),
        CatalogRoute::Sovereignty => sovereignty(native).await,
    }
}

fn model_tiers(native: &Native) -> Answer {
    let Some(root) = native.options().repo_root.as_ref() else {
        return Err(Fault::Decline);
    };
    match files::read_json_strict(&root.join("config/model-tiers.json")) {
        Strict::Value(Value::Object(map)) => read_reply(&Value::Object(map)),
        Strict::Value(_) => reply(StatusCode::OK, &json!({})),
        // Ausente o ilegible: el Python responde su caché en memoria.
        Strict::Missing | Strict::Unreadable | Strict::Unsure => Err(Fault::Decline),
    }
}

/// `os.path.expanduser("~")` del proceso: el HOME del que cuelga `hooks`.
fn home_of(hooks: &Path) -> Result<(PathBuf, String), Fault> {
    let home = hooks
        .parent()
        .and_then(Path::parent)
        .ok_or(Fault::Decline)?
        .to_path_buf();
    let text = home.to_str().ok_or(Fault::Decline)?.to_owned();
    Ok((home, text))
}

async fn sovereignty(native: &Native) -> Answer {
    let hooks = native.options().hooks.clone();
    let db = hooks.join("comandos-usage.sqlite");
    // Las tablas, por el carril (solo si el archivo existe: `finfo` va antes).
    let tables = if fs::metadata(&db).is_ok() {
        Some(
            native
                .usage
                .with(|u| -> rusqlite::Result<Vec<(String, i64)>> {
                    let names = u
                        .conn
                        .prepare("select name from sqlite_master where type='table' and name not like 'sqlite_%'")?
                        .query_map([], |r| r.get::<_, String>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    names
                        .into_iter()
                        .map(|t| {
                            let sql = format!("select count(*) from \"{}\"", t.replace('"', "\"\""));
                            u.conn.query_row(&sql, [], |r| r.get(0)).map(|n| (t, n))
                        })
                        .collect()
                })
                .await?
                .unwrap_or_default(),
        )
    } else {
        None
    };
    let now = (native.options().clock)().div_euclid(1000);
    let report = tokio::task::spawn_blocking(move || report(&hooks, tables, now))
        .await
        .map_err(|_| Fault::Error(HandlerError::Failure))??;
    read_reply(&report)
}

fn finfo(path: &Path, label: &str, kind: &str, home: &str) -> Option<Map<String, Value>> {
    let meta = fs::metadata(path).ok()?;
    let mode = meta.permissions().mode() & 0o7777;
    let mut m = Map::new();
    m.insert("label".into(), json!(label));
    m.insert("path".into(), json!(path.to_str()?.replace(home, "~")));
    m.insert("kind".into(), json!(kind));
    m.insert("bytes".into(), json!(meta.len()));
    m.insert(
        "mtime".into(),
        json!(std::os::unix::fs::MetadataExt::mtime(&meta)),
    );
    m.insert("mode".into(), json!(format!("0o{mode:o}")));
    m.insert("private".into(), json!(mode & 0o077 == 0));
    Some(m)
}

/// `len(glob.glob(base/p1/p2/…))`; cada componente es `(prefijo, sufijo)` de
/// un patrón con un solo `*`. `*` no casa nombres que empiezan por `.`; los
/// intermedios han de ser directorios (siguiendo symlinks).
fn glob_count(base: &Path, parts: &[(&str, &str)]) -> usize {
    let Some(((prefix, suffix), rest)) = parts.split_first() else {
        return 1;
    };
    let Ok(entries) = fs::read_dir(base) else {
        return 0;
    };
    let mut n = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let bytes = name.as_bytes();
        let fits = bytes.first() != Some(&b'.')
            && bytes.len() >= prefix.len() + suffix.len()
            && bytes.starts_with(prefix.as_bytes())
            && bytes.ends_with(suffix.as_bytes());
        if !fits {
            continue;
        }
        if rest.is_empty() {
            n += 1;
        } else if entry.path().is_dir() {
            n += glob_count(&entry.path(), rest);
        }
    }
    n
}

/// `sum(1 for _ in open(path, "rb"))`.
fn line_count(path: &Path) -> Option<usize> {
    let bytes = fs::read(path).ok()?;
    let n = bytes.iter().filter(|b| **b == b'\n').count();
    Some(n + usize::from(bytes.last().is_some_and(|b| *b != b'\n')))
}

/// Texto que el Python lee con `open()` en modo texto: ausente/OSError → `None`;
/// no UTF-8 → `UnicodeDecodeError` no capturado → se declina.
fn text_file(path: &Path) -> Result<Option<String>, Fault> {
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(|t| Some(t.replace("\r\n", "\n").replace('\r', "\n")))
            .map_err(|_| Fault::Decline),
        Err(_) => Ok(None),
    }
}

const FILES: [(&str, &str, &str); 10] = [
    ("prefs.json", "Preferencias del tablero", "json"),
    ("snippets.json", "Snippets", "json"),
    ("app-tabs.json", "Pestañas de la app", "json"),
    ("focus.json", "Pomodoro en curso", "json"),
    ("focus-queue.jsonl", "Avisos encolados en foco", "jsonl"),
    ("pane-models.txt", "Modelo por pane (tmux)", "txt"),
    ("dash-token", "Token de acceso remoto", "secret"),
    ("telegram.env", "Credenciales de Telegram retiradas (sin uso)", "secret"),
    ("providers.env", "Credenciales de proveedores", "secret"),
    ("cc-notify.conf", "Configuración de avisos", "conf"),
];

fn report(hooks: &Path, tables: Option<Vec<(String, i64)>>, now: i64) -> Result<Value, Fault> {
    let (home, home_text) = home_of(hooks)?;
    let mut stores = Vec::new();
    if let Some(mut fi) = finfo(
        &hooks.join("comandos-usage.sqlite"),
        "Uso y costos (turnos medidos)",
        "sqlite",
        &home_text,
    ) {
        let rows: Vec<Value> = tables
            .unwrap_or_default()
            .into_iter()
            .map(|(name, rows)| json!({"name": name, "rows": rows}))
            .collect();
        fi.insert("tables".into(), Value::Array(rows));
        stores.push(Value::Object(fi));
    }
    stores.push(json!({
        "label": "Estado por sesión (hooks)",
        "path": "~/.claude/hooks/state/",
        "kind": "json",
        "count": glob_count(&hooks.join("state"), &[("", ".json")]),
    }));
    let events = hooks.join("events.jsonl");
    if let Some(mut fi) = finfo(&events, "Historial de eventos", "jsonl", &home_text) {
        if let Some(n) = line_count(&events) {
            fi.insert("count".into(), json!(n));
        }
        stores.push(Value::Object(fi));
    }
    for (name, label, kind) in FILES {
        if let Some(fi) = finfo(&hooks.join(name), label, kind, &home_text) {
            stores.push(Value::Object(fi));
        }
    }
    stores.push(json!({
        "label": "Transcripts de Claude Code",
        "path": "~/.claude/projects/*/",
        "kind": "jsonl",
        "count": glob_count(&home.join(".claude/projects"), &[("", ""), ("", ".jsonl")]),
    }));
    let codex = glob_count(
        &home.join(".codex/sessions"),
        &[("", ""), ("", ""), ("", ""), ("rollout-", ".jsonl")],
    );
    if codex > 0 {
        stores.push(json!({"label": "Rollouts de Codex", "path": "~/.codex/sessions/", "kind": "jsonl", "count": codex}));
    }
    let ssh = text_file(&home.join(".ssh/config"))?.map_or(0, |t| {
        t.split_inclusive('\n')
            .filter(|l| py::strip(l).to_lowercase().starts_with("host ") && !l.contains('*'))
            .count()
    });
    stores.push(json!({"label": "Servidores SSH", "path": "~/.ssh/config", "kind": "conf", "count": ssh}));
    let mut native_notify = String::from("0");
    if let Some(conf) = text_file(&hooks.join("cc-notify.conf"))? {
        for line in conf.split_inclusive('\n') {
            if line.contains('=') && !py::strip(line).starts_with('#') {
                if let Some((k, v)) = py::strip(line).split_once('=') {
                    if k == "NATIVE_NOTIFY" {
                        native_notify = py::strip(v).trim_matches(|c| c == '"' || c == '\'').to_owned();
                    }
                }
            }
        }
    }
    Ok(json!({
        "stores": stores,
        "browser": [
            {"key": "cc-pomos", "label": "Registro de pomodoros"},
            {"key": "cc-dash-labels", "label": "Nombres de pestañas"},
            {"key": "cc-ssh-open", "label": "Servidores desplegados"},
        ],
        "outbound": [
            {"label": "Acceso remoto (tailnet)", "on": hooks.join("webterm-enabled").exists(), "how": "tailscale serve — solo tu tailnet, nunca Funnel"},
            {"label": "Proveedores de IA (los agentes)", "on": true, "how": "Claude/Codex/etc. hablan con su API: eso es el agente, no ComandOS"},
            {"label": "Notificaciones nativas del escritorio", "on": native_notify == "1", "how": "GNOME, en esta máquina"},
        ],
        "generatedAt": now,
    }))
}
```

Notas para el implementador: `json!` con `preserve_order` conserva el orden escrito (es el del Python); `os.path.exists(webterm-enabled)` sigue symlinks como `Path::exists`; `conf[k] = v` deja el último `NATIVE_NOTIFY`, igual que el bucle de arriba. En `mod.rs`: `pub mod catalogs;`, `NativeRoute::Catalog(catalogs::CatalogRoute)`, `catalogs::ROUTES` en `TABLES` y `NativeRoute::Catalog(route) => catalogs::answer(self, route).await,`.

- [ ] **Step 4: Ver pasar**

Run: `$C test -p comandos-server --test dash_native_catalogs -j 6` → PASS; `$C test -p comandos-server -j 6` verde.

- [ ] **Step 5: Fixture y paridad**

```
# --- 2c · F3: catálogos de solo lectura (nativas)
{"name":"f-model-tiers","method":"GET","path":"/model-tiers","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"f-sovereignty","method":"GET","path":"/sovereignty","headers":{"Host":"127.0.0.1"},"body":null,"volatile":["/generatedAt","/stores/*/mtime"],"expect":"same"}
```

(`mtime` es volátil: los POST anteriores del fixture reescriben `prefs.json`/`snippets.json` en segundos distintos en cada copia.)

Run: la orden de paridad de la Tarea 2 (con `--usage-db`). Expected: 0 DIFF; `/model-tiers` y `/sovereignty` fuera de «reenviadas».

- [ ] **Step 6: fmt, clippy, commit**

```bash
$C fmt --all -- --check
$C clippy --workspace --all-targets -j 6 -- -D warnings
git add crates/comandos-server/src/dash/mod.rs crates/comandos-server/src/dash/native/mod.rs \
  crates/comandos-server/src/dash/native/catalogs.rs crates/comandos-server/tests/support/mod.rs \
  crates/comandos-server/tests/dash_native_catalogs.rs
git add -f xtask/parity/frente.jsonl
git commit -m "feat(dash): dominio F3 nativo — /model-tiers y /sovereignty

/model-tiers sirve config/model-tiers.json del checkout del heredado (raíz por
COMANDOS_DASH_REPO o por el enlace de dash/index.html) y declina si el archivo
falta o no parsea (el Python respondería su caché). /sovereignty reproduce el
inventario con la semántica de glob de Python y las tablas por el carril de uso.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Dominio G1 — POST `/terminal-panes` y POST `/terminal-history`

Desplegable sola. `/terminal-panes` con `action:"list"` es la ruta más frecuente que queda (`dash/term.html:2306-2334`, cada 2 s **por iframe**; el `xtask poll` la mide). Usa las librerías `comandos_runtime::{terminal_panes, terminal_history}` a través del puente síncrono.

Correspondencia (8889, 8898):

- **`/terminal-history`**: `terminal_history.capture(tmux, data)`; `(ValueError, TypeError)` → 400 `str(exc)`; cualquier otra excepción (incluido `TimeoutExpired` y `FileNotFoundError` de tmux) → 503 `Historial temporalmente no disponible`. Salida de tmux no UTF-8 → `UnicodeDecodeError` (un `ValueError` con texto del códec) → `Decline` (la ruta no escribe). `HistoryError::MalformedPanes` (filas que el Rust rechaza y el Python convierte con `int()`) → `Decline`.
- **`/terminal-panes`**: `terminal_panes.execute(tmux, _pane_identity, save_closed_pane_snapshot, data)`; 400 `str(exc)`; otra excepción → 503 `No se pudo completar la acción del panel. Comprueba Paneles antes de reintentar`. `identify` reproduce `_pane_identity` (6887): `display-message -p -t <pane>` con los 8 campos, `strip().split('\t')`, mensajes `se necesita el panel exacto` / `el panel ya no existe` / `el panel no pertenece a esa sesión`, y `server_start` = campo 20 tras el último `)` de `/proc/<pid>/stat` (`OSError` → `""`; falta el campo → `IndexError` → 503). `action:"close"` → `Decline` antes de nada. No UTF-8 → `Decline` si todavía no se ejecutó un comando que muta (`resize-pane`, `if-shell`, `set-option`); después, 503 (diferencia aceptada: exigiría un pane nuevo con ruta no UTF-8 justo tras un `split`).

**Files:**
- Modify: `crates/comandos-runtime/src/terminal_panes.rs`, `crates/comandos-runtime/tests/terminal_panes_controls.rs`
- Modify: `crates/comandos-server/src/dash/native/tmux.rs`, `crates/comandos-server/src/dash/native/mod.rs`
- Create: `crates/comandos-server/src/dash/native/terminal.rs`
- Create: `crates/comandos-server/tests/dash_native_terminal.rs`
- Modify: `xtask/parity/frente.jsonl`

**Interfaces:**
- Produces: `Tmux::run_blocking(&self, &tokio::runtime::Handle, &[&str]) -> Result<Output, TmuxError>`; `terminal::{TerminalRoute, ROUTES, answer, Bridge, home_text}`; `NativeRoute::Terminal(TerminalRoute)`.
- Consumes: `comandos_runtime::{terminal_panes::{execute, PaneError}, terminal_history::{capture, HistoryError}, pane_typing::TmuxResult}`.

- [ ] **Step 1: Prueba que falla de la librería (orden de claves del inventario)**

Añadir a `crates/comandos-runtime/tests/terminal_panes_controls.rs` (usa el `Harness` existente del archivo):

```rust
#[test]
fn list_keys_follow_python_order() {
    let h = Harness::new();
    let out = panes::execute(
        |a| h.tmux(a),
        |s, p| Ok(h.identity_for(s, p)),
        |_, _| Err(PaneError("no".into())),
        &json!({"session":"fixture"}),
        "/home/fixture",
    )
    .unwrap();
    let text = comandos_core::json::response_dumps(&out).unwrap();
    assert!(
        text.starts_with(r#"{"ok": true, "panes": [{"id": "%1", "active": false, "title": "zsh", "path": "~/p", "left": 0, "top": 0, "width": 59, "height": 40, "index": 0, "identity": ""#),
        "{text}"
    );
}

#[test]
fn list_does_not_validate_identity_digits() {
    // El Python solo valida la identidad antes de mutar (lib/terminal_panes.py:138-142).
    let h = Harness::new();
    let out = panes::execute(
        |a| h.tmux(a),
        |_, p| Ok(json!({"pid":"x","session_id":"$1","pane_id":p,"pane_pid":"1"})),
        |_, _| Err(PaneError("no".into())),
        &json!({"session":"fixture"}),
        "/home/fixture",
    );
    assert!(out.is_ok(), "{out:?}");
}
```

Si el arnés del archivo no tiene `identity_for(session, pane)`, añadirlo junto a `identity`: `fn identity_for(&self, _session: &str, pane: &str) -> Value { self.identity(pane) }`.

Run: `$C test -p comandos-runtime --test terminal_panes_controls` → FAIL (orden `index` antes de `left`; la segunda, «No se pudo verificar la identidad del panel»).

- [ ] **Step 2: Alinear `inventory` con el Python**

En `crates/comandos-runtime/src/terminal_panes.rs`, sustituir el cuerpo del bucle de `inventory` (desde `let index = …` hasta `panes.push(…)`) por:

```rust
        let index = head[3]
            .parse::<u64>()
            .map_err(|_| error("No se pudo leer la lista de paneles"))?;
        // Orden de claves del Python: id, active, title, path, geometría, index, identity.
        let mut public = Map::new();
        public.insert("id".into(), json!(head[0]));
        public.insert("active".into(), json!(head[1] == "1"));
        public.insert(
            "title".into(),
            json!(head[2].chars().take(100).collect::<String>()),
        );
        let mut parts = head[4].rsplitn(5, '\t').collect::<Vec<_>>();
        parts.reverse();
        let geometry = parts.len() == 5 && parts.iter().skip(1).all(|p| digits(p));
        let path = if geometry { parts[0] } else { head[4] };
        public.insert(
            "path".into(),
            json!(friendly_path(path, home).chars().take(300).collect::<String>()),
        );
        if geometry {
            for (key, raw) in ["left", "top", "width", "height"].iter().zip(parts.iter().skip(1)) {
                let value = raw
                    .parse::<u64>()
                    .map_err(|_| error("No se pudo leer la lista de paneles"))?;
                public.insert((*key).into(), json!(value));
            }
        }
        public.insert("index".into(), json!(index));
        // `identify` es `_pane_identity` del llamador: él valida pane y sesión.
        let raw = identify(session, head[0])?;
        public.insert("identity".into(), json!(version(&raw)?));
        panes.push(Pane {
            public: Value::Object(public),
            raw,
        });
```

(La validación de dígitos de la identidad sigue en `execute`, antes de construir la condición de `if-shell`, como en el Python.) Corregir en el archivo de pruebas el caso existente que esperaba «No se pudo verificar la identidad del panel» o «El panel no pertenece a esta sesión» desde `list`: cambiar su `action` a `select` con el `pane`/`identity` del inventario (ese es el camino donde el Python valida) o, si comprobaba la sesión, mover la comprobación al callback `identify` del caso.

Run: `$C test -p comandos-runtime --test terminal_panes_controls` → PASS.

- [ ] **Step 3: Puente síncrono a tmux**

En `crates/comandos-server/src/dash/native/tmux.rs`, en `impl Tmux`:

```rust
    /// `run` desde un hilo de bloqueo (librerías síncronas de `comandos-runtime`).
    /// El reactor, los timers y el reaper los mueve el hilo del runtime, que
    /// está en `Runtime::block_on`; aquí solo se espera el resultado.
    pub fn run_blocking(
        &self,
        handle: &tokio::runtime::Handle,
        args: &[&str],
    ) -> Result<Output, TmuxError> {
        handle.block_on(self.run(args))
    }
```

- [ ] **Step 4: Pruebas que fallan del dominio**

Crear `crates/comandos-server/tests/dash_native_terminal.rs`:

```rust
//! Dominio G1: paneles e historial de tmux por el frente, sobre un servidor
//! tmux privado compartido con el oráculo (mismas identidades de pane).
mod support;

use comandos_server::dash::native::tmux::{Program, Tmux};
use std::{
    ffi::OsString,
    time::{Duration, Instant},
};
use support::{FakeLegacy, TestHome, Wire, dead_port, front, get, oracle::oracle, request_body, tmux_available};

fn tmux(home: &TestHome, args: &[&str]) -> String {
    let out = std::process::Command::new("tmux")
        .args(["-f", "/dev/null"])
        .args(args)
        .env_remove("TMUX")
        .env("TMUX_TMPDIR", home.tmux_dir())
        .output()
        .unwrap();
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

fn seen(wire: &Wire) -> (u16, Option<String>, String) {
    (wire.status, wire.header("content-type").map(str::to_owned), wire.text())
}

/// Sesión `s1` con dos paneles (%0 y %1) que muestran texto conocido.
fn two_panes(home: &TestHome) {
    tmux(home, &["new-session", "-d", "-s", "s1", "-x", "120", "-y", "40", "sh -c 'printf \"uno\\ndos ñ\\n\"; exec cat'"]);
    tmux(home, &["split-window", "-h", "-t", "=s1:", "cat"]);
}

#[tokio::test]
async fn terminal_routes_match_python_oracle() {
    if !tmux_available() {
        eprintln!("tmux no está instalado: se salta");
        return;
    }
    let home = TestHome::new("term-oracle");
    two_panes(&home);
    let Some(py) = oracle(&home).await else { return };
    let front = front(&home, dead_port(), home.options()).await;
    let list = r#"{"session": "s1"}"#;
    let listed = request_body(front.port, "POST", "/terminal-panes", "", list).await;
    assert_eq!(listed.status, 200, "{}", listed.text());
    let identity = serde_json::from_str::<serde_json::Value>(&listed.text()).unwrap()["panes"][0]["identity"]
        .as_str()
        .unwrap()
        .to_owned();
    let cases = [
        ("/terminal-panes", list.to_owned()),
        ("/terminal-panes", r#"{"session": "a b"}"#.to_owned()),
        ("/terminal-panes", r#"{"session": 5}"#.to_owned()),
        ("/terminal-panes", r#"{"session": "nadie"}"#.to_owned()),
        ("/terminal-panes", r#"{"session": "s1", "action": "zz"}"#.to_owned()),
        ("/terminal-panes", r#"{"session": "s1", "action": "resize", "axis": "x", "size": 1}"#.to_owned()),
        ("/terminal-panes", r#"{"session": "s1", "action": "split", "direction": "up"}"#.to_owned()),
        ("/terminal-panes", r#"{"session": "s1", "action": "select", "pane": "%0", "identity": "otra"}"#.to_owned()),
        ("/terminal-panes", format!(r#"{{"session": "s1", "action": "select", "pane": "%0", "identity": "{identity}", "scope": "client"}}"#)),
        ("/terminal-history", r#"{"session": "s1", "pane": "%0", "lines": 50}"#.to_owned()),
        ("/terminal-history", r#"{"session": "s1", "lines": 0}"#.to_owned()),
        ("/terminal-history", r#"{"session": "s1", "lines": true}"#.to_owned()),
        ("/terminal-history", r#"{"session": "s1", "pane": "%9"}"#.to_owned()),
        ("/terminal-history", r#"{"session": "s1", "col": 0, "row": 0}"#.to_owned()),
        ("/terminal-history", r#"{"session": "s1", "col": 999, "row": 0}"#.to_owned()),
        ("/terminal-history", r#"{"session": "s1", "col": -1, "row": 0}"#.to_owned()),
        ("/terminal-history", r#"{"session": "nadie"}"#.to_owned()),
    ];
    for (path, body) in &cases {
        assert_eq!(
            seen(&request_body(py.port, "POST", path, "", body).await),
            seen(&request_body(front.port, "POST", path, "", body).await),
            "{path} {body}"
        );
    }
    front.stop().await;
}

#[tokio::test]
async fn terminal_panes_mutations_are_native() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("term-mut");
    two_panes(&home);
    let front = front(&home, dead_port(), home.options()).await;
    let listed = request_body(front.port, "POST", "/terminal-panes", "", r#"{"session": "s1"}"#).await;
    let value: serde_json::Value = serde_json::from_str(&listed.text()).unwrap();
    let identity = value["panes"][0]["identity"].as_str().unwrap().to_owned();
    let split = format!(r#"{{"session": "s1", "action": "split", "pane": "%0", "identity": "{identity}", "direction": "down"}}"#);
    let wire = request_body(front.port, "POST", "/terminal-panes", "", &split).await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    assert!(wire.text().ends_with(r#", "opened": "%2"}"#), "{}", wire.text());
    let resize = r#"{"session": "s1", "action": "resize", "pane": "%1", "axis": "x", "size": 30}"#;
    assert_eq!(request_body(front.port, "POST", "/terminal-panes", "", resize).await.status, 200);
    assert_eq!(tmux(&home, &["display-message", "-p", "-t", "%1", "#{pane_width}"]).trim(), "30");
    front.stop().await;
}

#[tokio::test]
async fn terminal_panes_close_declines() {
    let home = TestHome::new("term-close");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let body = r#"{"session": "s1", "action": "close", "pane": "%0", "identity": "x"}"#;
    assert_eq!(request_body(front.port, "POST", "/terminal-panes", "", body).await.text(), r#"{"legacy": true}"#);
    assert_eq!(legacy.requests(), vec!["POST /terminal-panes HTTP/1.1".to_owned()]);
    front.stop().await;
}

#[tokio::test]
async fn terminal_hung_tmux_answers_503_and_frees_runtime() {
    let home = TestHome::new("term-hung");
    let mut opts = home.options();
    // `sh -c 'sleep 10' sh <args de tmux>`: cuelga sin mirar los argumentos.
    opts.tmux = Tmux {
        program: Program {
            path: "sh".into(),
            prefix: vec![OsString::from("-c"), OsString::from("sleep 10"), OsString::from("sh")],
            env: vec![],
            env_remove: vec![],
        },
        timeout: Duration::from_secs(2),
    };
    let front = front(&home, dead_port(), opts).await;
    let port = front.port;
    let started = Instant::now();
    let hung = tokio::spawn(async move {
        request_body(port, "POST", "/terminal-panes", "", r#"{"session": "s1"}"#).await
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let quick = Instant::now();
    assert_eq!(get(port, "/prefs").await.status, 200);
    assert!(quick.elapsed() < Duration::from_secs(1), "el runtime sigue libre");
    let wire = hung.await.unwrap();
    assert_eq!(
        (wire.status, wire.text().as_str()),
        (503, r#"{"error": "No se pudo completar la acción del panel. Comprueba Paneles antes de reintentar"}"#)
    );
    assert!(started.elapsed() >= Duration::from_secs(2));
    let wire = request_body(port, "POST", "/terminal-history", "", r#"{"session": "s1"}"#).await;
    assert_eq!((wire.status, wire.text().as_str()), (503, r#"{"error": "Historial temporalmente no disponible"}"#));
    front.stop().await;
}
```

Run: `$C test -p comandos-server --test dash_native_terminal -j 6` → FAIL.

- [ ] **Step 5: Implementar `terminal.rs`**

Crear `crates/comandos-server/src/dash/native/terminal.rs`:

```rust
//! G. Terminal: POST /terminal-history (8889, `lib/terminal_history.py`) y
//! POST /terminal-panes (8898, `lib/terminal_panes.py`) sobre las librerías
//! de `comandos-runtime`, que llaman a tmux de forma síncrona: corren en un
//! hilo de bloqueo y cada llamada la conduce el runtime (`run_blocking`).
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb,
    light::{data, error},
    py, reply,
    tmux::{Tmux, TmuxError},
};
use crate::{HandlerError, Request};
use comandos_runtime::{
    pane_typing::TmuxResult,
    terminal_history::{self, HistoryError},
    terminal_panes::{self, PaneError},
};
use http::StatusCode;
use serde_json::{Map, Value};
use std::{cell::RefCell, path::Path};
use tokio::runtime::Handle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalRoute {
    History,
    Panes,
}

pub const ROUTES: &[Entry] = &[
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/terminal-history"),
        route: NativeRoute::Terminal(TerminalRoute::History),
    },
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/terminal-panes"),
        route: NativeRoute::Terminal(TerminalRoute::Panes),
    },
];

const PANES_UNAVAILABLE: &str =
    "No se pudo completar la acción del panel. Comprueba Paneles antes de reintentar";
const HISTORY_UNAVAILABLE: &str = "Historial temporalmente no disponible";
/// Errores de callback: nunca llegan a la respuesta (se mira `Bridge` antes).
const BRIDGE_FAILED: &str = "comandos: tmux no disponible";

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

/// `os.path.expanduser("~").rstrip("/")` de `friendly_path`.
pub fn home_text(native: &Native) -> Result<String, Fault> {
    let home = native
        .options()
        .hooks
        .parent()
        .and_then(Path::parent)
        .ok_or(Fault::Decline)?;
    Ok(home.to_str().ok_or(Fault::Decline)?.trim_end_matches('/').to_owned())
}

/// Lo que pasó con tmux durante la llamada a la librería.
#[derive(Default)]
pub struct Bridge {
    /// TimeoutExpired, FileNotFoundError, `IndexError` de `/proc`: 503.
    pub unavailable: bool,
    /// Salida no UTF-8: `UnicodeDecodeError` con texto del códec.
    pub undecodable: bool,
    /// Ya corrió un comando que muta (`resize-pane`, `if-shell`, `set-option`).
    pub effected: bool,
}

/// Una llamada a tmux desde la librería. `None` si falló (y queda anotado).
fn call(handle: &Handle, tmux: &Tmux, bridge: &RefCell<Bridge>, args: &[&str]) -> Option<TmuxResult> {
    if matches!(args.first(), Some(&("resize-pane" | "if-shell" | "set-option"))) {
        bridge.borrow_mut().effected = true;
    }
    match tmux.run_blocking(handle, args) {
        Ok(out) => Some(TmuxResult {
            returncode: if out.ok { 0 } else { 1 },
            stdout: out.stdout,
            stderr: out.stderr,
        }),
        Err(TmuxError::Decode) => {
            bridge.borrow_mut().undecodable = true;
            None
        }
        Err(_) => {
            bridge.borrow_mut().unavailable = true;
            None
        }
    }
}

pub async fn answer(native: &Native, route: TerminalRoute, request: &Request) -> Answer {
    let data = Value::Object(data(request)?.clone());
    let home = home_text(native)?;
    let tmux = native.options().tmux.clone();
    let handle = Handle::current();
    match route {
        TerminalRoute::History => {
            let (result, bridge) = tokio::task::spawn_blocking(move || {
                let bridge = RefCell::new(Bridge::default());
                let result = terminal_history::capture(
                    |args| {
                        call(&handle, &tmux, &bridge, args).unwrap_or(TmuxResult {
                            returncode: 1,
                            ..TmuxResult::default()
                        })
                    },
                    &data,
                    &home,
                );
                (result, bridge.into_inner())
            })
            .await
            .map_err(|_| failure())?;
            if bridge.unavailable {
                return error(StatusCode::SERVICE_UNAVAILABLE, HISTORY_UNAVAILABLE);
            }
            if bridge.undecodable {
                return Err(Fault::Decline);
            }
            match result {
                Ok(response) => reply(StatusCode::OK, &response.to_json()),
                Err(HistoryError::MalformedPanes) => Err(Fault::Decline),
                Err(e) => error(StatusCode::BAD_REQUEST, &e.to_string()),
            }
        }
        TerminalRoute::Panes => {
            // `close` guarda una copia con `tmux_snapshot` + `PaneInspector`
            // (inspección de procesos, aún en Python): se declina antes de nada.
            if data.get("action").and_then(Value::as_str) == Some("close") {
                return Err(Fault::Decline);
            }
            let (result, bridge) = tokio::task::spawn_blocking(move || {
                let bridge = RefCell::new(Bridge::default());
                let result = terminal_panes::execute(
                    |args| call(&handle, &tmux, &bridge, args).ok_or_else(|| PaneError(BRIDGE_FAILED.into())),
                    |session, pane| identify(&handle, &tmux, &bridge, session, pane),
                    |_, _| Err(PaneError(BRIDGE_FAILED.into())),
                    &data,
                    &home,
                );
                (result, bridge.into_inner())
            })
            .await
            .map_err(|_| failure())?;
            if bridge.unavailable {
                return error(StatusCode::SERVICE_UNAVAILABLE, PANES_UNAVAILABLE);
            }
            if bridge.undecodable {
                return if bridge.effected {
                    error(StatusCode::SERVICE_UNAVAILABLE, PANES_UNAVAILABLE)
                } else {
                    Err(Fault::Decline)
                };
            }
            match result {
                Ok(value) => reply(StatusCode::OK, &value),
                Err(PaneError(message)) => error(StatusCode::BAD_REQUEST, &message),
            }
        }
    }
}

const IDENTITY_FIELDS: [&str; 8] = [
    "socket_path",
    "pid",
    "session_id",
    "session_name",
    "pane_id",
    "pane_pid",
    "pane_current_command",
    "pane_current_path",
];

/// `_pane_identity` (6887).
fn identify(
    handle: &Handle,
    tmux: &Tmux,
    bridge: &RefCell<Bridge>,
    session: &str,
    pane: &str,
) -> Result<Value, PaneError> {
    if py::is_pane(pane) != Some(true) {
        return Err(PaneError("se necesita el panel exacto".into()));
    }
    let format = IDENTITY_FIELDS
        .iter()
        .map(|f| format!("#{{{f}}}"))
        .collect::<Vec<_>>()
        .join("\t");
    let out = call(handle, tmux, bridge, &["display-message", "-p", "-t", pane, &format])
        .ok_or_else(|| PaneError(BRIDGE_FAILED.into()))?;
    let parts: Vec<&str> = py::strip(&out.stdout).split('\t').collect();
    if out.returncode != 0 || parts.len() != IDENTITY_FIELDS.len() {
        return Err(PaneError("el panel ya no existe".into()));
    }
    let mut identity = Map::new();
    for (key, value) in IDENTITY_FIELDS.iter().zip(&parts) {
        identity.insert((*key).into(), Value::String((*value).to_owned()));
    }
    if parts.get(4) != Some(&pane) || parts.get(3) != Some(&session) {
        return Err(PaneError("el panel no pertenece a esa sesión".into()));
    }
    let pid = parts.get(1).copied().unwrap_or("");
    let start = match std::fs::read(format!("/proc/{pid}/stat")) {
        Err(_) => String::new(),
        Ok(bytes) => {
            let Ok(text) = String::from_utf8(bytes) else {
                bridge.borrow_mut().undecodable = true;
                return Err(PaneError(BRIDGE_FAILED.into()));
            };
            match text.rsplit_once(')').and_then(|(_, rest)| rest.split_whitespace().nth(19)) {
                Some(field) => field.to_owned(),
                None => {
                    bridge.borrow_mut().unavailable = true;
                    return Err(PaneError(BRIDGE_FAILED.into()));
                }
            }
        }
    };
    identity.insert("server_start".into(), Value::String(start));
    Ok(Value::Object(identity))
}
```

En `mod.rs`: `pub mod terminal;`, `NativeRoute::Terminal(terminal::TerminalRoute)`, `terminal::ROUTES` en `TABLES` y `NativeRoute::Terminal(route) => terminal::answer(self, route, request).await,`.

- [ ] **Step 6: Ver pasar**

Run: `$C test -p comandos-server --test dash_native_terminal -j 6` → PASS; `$C test -p comandos-runtime -p comandos-server -j 6` verde.

- [ ] **Step 7: Fixture y paridad**

```
# --- 2c · G1: paneles e historial (nativas; close se reenvía). tmux privado por copia: identidades distintas
{"name":"g-terminal-panes","method":"POST","path":"/terminal-panes","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"local"},"volatile":["/panes/*/identity"],"expect":"same"}
{"name":"g-terminal-panes-mala","method":"POST","path":"/terminal-panes","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"a b"},"volatile":[],"expect":"same"}
{"name":"g-terminal-panes-nadie","method":"POST","path":"/terminal-panes","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"no-existe"},"volatile":[],"expect":"same"}
{"name":"g-terminal-panes-accion","method":"POST","path":"/terminal-panes","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"local","action":"zz"},"volatile":[],"expect":"same"}
{"name":"g-terminal-panes-close","method":"POST","path":"/terminal-panes","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"local","action":"close","pane":"%0","identity":"x"},"volatile":[],"expect":"same","forwarded":true}
{"name":"g-terminal-history","method":"POST","path":"/terminal-history","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"local","lines":5},"volatile":["/text"],"expect":"same"}
{"name":"g-terminal-history-lineas","method":"POST","path":"/terminal-history","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"local","lines":0},"volatile":[],"expect":"same"}
{"name":"g-terminal-history-nadie","method":"POST","path":"/terminal-history","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"no-existe"},"volatile":[],"expect":"same"}
```

Run: la orden de paridad de la Tarea 2. Expected: 0 DIFF; POST `/terminal-history` fuera de «reenviadas»; POST `/terminal-panes` solo por `g-terminal-panes-close`. Run además `.build/target/debug/xtask poll --shadow --minutes 3 --hooks ~/.claude/hooks --state-db ~/.local/state/comandos/app-state.sqlite3 --usage-db ~/.claude/hooks/comandos-usage.sqlite --comandos .build/target/debug/comandos`: los no-2xx siguen siendo los de `/terminal-panes` a la sesión inexistente `poll` (ahora 400 de Rust) y POST `/terminal-panes` desaparece de las reenviadas.

- [ ] **Step 8: fmt, clippy, commit**

```bash
$C fmt --all -- --check
$C clippy --workspace --all-targets -j 6 -- -D warnings
git add crates/comandos-runtime/src/terminal_panes.rs crates/comandos-runtime/tests/terminal_panes_controls.rs \
  crates/comandos-server/src/dash/native/tmux.rs crates/comandos-server/src/dash/native/mod.rs \
  crates/comandos-server/src/dash/native/terminal.rs crates/comandos-server/tests/dash_native_terminal.rs
git add -f xtask/parity/frente.jsonl
git commit -m "feat(dash): dominio G1 nativo — /terminal-panes y /terminal-history

Las librerías síncronas de comandos-runtime corren en un hilo de bloqueo y
llaman a tmux por el mismo tokio::process (Tmux::run_blocking). El inventario
de paneles sigue ahora el orden de claves del Python y valida la identidad
solo antes de mutar. action:close se reenvía (necesita PaneInspector).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Dominio G2 — POST `/pane/type` (tecleo literal con caché y candados)

Desplegable sola. La barra de comandos (`dash/command-sidebar.js:341-350`) teclea texto en un pane exacto sin Enter. La ruta entera (resolución de sesión, candado por pane, caché de 256 respuestas) vive en el frente.

Orden del Python (9552 → 9630 → 9667), con la caché adelantada (ver Decisiones):

1. `sess = data.get("session", "")`; no-cadena → `TypeError` → 500; `SESSION_RE` → 400 `Nombre de sesion invalido`.
2. `rid = str(data.get("requestId") or "")[:64]`; un `rid` declinado antes → `Decline`; en caché → la respuesta guardada.
3. `resolve_project_session(sess)`: `tmux list-sessions` (excepción no capturada: 504/500), luego cada `H/state/*.json` (sin ocultos; directorios se saltan; JSON roto se salta; no-objeto o `project` no-cadena → el Python da 500 → `Decline`); si `re.sub("[.:]", "-", project)[:80] == sess` en alguno → `Decline` (y se marca `rid`).
4. `want = str(data.get("pane","") or "")`; si casa `PANE_RE` y `display-message -p -t want '#{pane_id}'` devuelve `want`, `pane = want`; si no, `pane = "=<sess>:"`. Dígitos no ASCII → `Decline`.
5. `has-session -t =<sess>` ≠ 0 → 404 `No hay sesion tmux '<sess>'. Levantala primero.`; `want` sin `PANE_RE` → 400 `{"error": "Falta un pane exacto (%N)", "code": "invalid"}`; `pane != want` → 404 `{"error": "El pane <want> ya no existe", "code": "pane_gone"}`.
6. `pane_typing.validate(data.get("text"))` → 400 `{"error", "code": "invalid"}`; candado → 409 `{"error": "Ya se está escribiendo en ese pane; espera a que termine.", "code": "typing_in_progress"}`.
7. `type_literal` (send-keys `-l --` carácter a carácter, `;` como `\;`, pausa `min(0.022, 1.2/(n-1))`): 200 `{"ok": true, "typed", "durationMs": int(ms), "requestId"}` o 502 `{"error", "code", "typed", "requestId"}`; un `TimeoutExpired` en medio → 504 sin caché (el Python no llega a guardar). Con `rid`, la respuesta entra en la caché (FIFO de 256).

**Files:**
- Create: `crates/comandos-server/src/dash/native/typing.rs`; Modify: `crates/comandos-server/src/dash/native/mod.rs`
- Create: `crates/comandos-server/tests/dash_native_typing.rs`
- Modify: `xtask/parity/frente.jsonl`

**Interfaces:**
- Produces: `typing::{TypingState, ROUTES, answer, project_session_matches}`; `Native.typing`; `NativeRoute::PaneType`.
- Consumes: `comandos_runtime::pane_typing::{validate, type_literal, TypingOptions, PaneTypingLocks, TmuxResult}`, `light::{load, tmux_sessions}`, `Tmux::run_blocking`.

- [ ] **Step 1: Pruebas que fallan**

Crear `crates/comandos-server/tests/dash_native_typing.rs`:

```rust
//! Dominio G2: POST /pane/type por el frente, con su caché por requestId.
mod support;

use std::time::Duration;
use support::{FakeLegacy, TestHome, Wire, dead_port, front, oracle::oracle, request_body, tmux_available};

fn tmux(home: &TestHome, args: &[&str]) -> String {
    let out = std::process::Command::new("tmux")
        .args(["-f", "/dev/null"])
        .args(args)
        .env_remove("TMUX")
        .env("TMUX_TMPDIR", home.tmux_dir())
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap()
}

fn cat_session(home: &TestHome) {
    tmux(home, &["new-session", "-d", "-s", "s1", "cat"]);
}

fn seen(wire: &Wire) -> (u16, String) {
    (wire.status, wire.text())
}

async fn typed_text(home: &TestHome) -> String {
    tokio::time::sleep(Duration::from_millis(200)).await;
    tmux(home, &["capture-pane", "-p", "-t", "%0"]).trim().to_owned()
}

#[tokio::test]
async fn pane_type_types_literal_text() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("type-ok");
    cat_session(&home);
    let front = front(&home, dead_port(), home.options()).await;
    let body = r#"{"session": "s1", "pane": "%0", "text": "a;b ñ", "requestId": "r1"}"#;
    let wire = request_body(front.port, "POST", "/pane/type", "", body).await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    assert!(wire.text().starts_with(r#"{"ok": true, "typed": 5, "durationMs": "#), "{}", wire.text());
    assert!(wire.text().ends_with(r#", "requestId": "r1"}"#));
    assert_eq!(typed_text(&home).await, "a;b ñ");
    front.stop().await;
}

#[tokio::test]
async fn pane_type_retry_same_request_id_types_once() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("type-retry");
    cat_session(&home);
    let front = front(&home, dead_port(), home.options()).await;
    let body = r#"{"session": "s1", "pane": "%0", "text": "xyz", "requestId": "rep"}"#;
    let first = request_body(front.port, "POST", "/pane/type", "", body).await;
    let second = request_body(front.port, "POST", "/pane/type", "", body).await;
    assert_eq!(seen(&first), seen(&second), "misma respuesta, mismo durationMs");
    assert_eq!(typed_text(&home).await, "xyz", "una sola vez");
    front.stop().await;
}

#[tokio::test]
async fn pane_type_errors_match_python_oracle() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("type-oracle");
    cat_session(&home);
    let Some(py) = oracle(&home).await else { return };
    let front = front(&home, dead_port(), home.options()).await;
    let long = "x".repeat(2001);
    let cases = [
        r#"{"session": "a b", "text": "x"}"#.to_owned(),
        r#"{"session": 5, "text": "x"}"#.to_owned(),
        r#"{"session": "nadie", "pane": "%0", "text": "x"}"#.to_owned(),
        r#"{"session": "s1", "text": "x"}"#.to_owned(),
        r#"{"session": "s1", "pane": "%99", "text": "x"}"#.to_owned(),
        r#"{"session": "s1", "pane": "%0", "text": "   "}"#.to_owned(),
        r#"{"session": "s1", "pane": "%0", "text": 7}"#.to_owned(),
        r#"{"session": "s1", "pane": "%0", "text": "a\nb"}"#.to_owned(),
        format!(r#"{{"session": "s1", "pane": "%0", "text": "{long}"}}"#),
    ];
    for body in &cases {
        assert_eq!(
            seen(&request_body(py.port, "POST", "/pane/type", "", body).await),
            seen(&request_body(front.port, "POST", "/pane/type", "", body).await),
            "{body}"
        );
    }
    front.stop().await;
}

#[tokio::test]
async fn declined_request_id_stays_declined() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("type-decline");
    cat_session(&home);
    home.write("state/proyecto.json", r#"{"project": "s1", "session": "s1"}"#);
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let body = r#"{"session": "s1", "pane": "%0", "text": "x", "requestId": "dup"}"#;
    assert_eq!(request_body(front.port, "POST", "/pane/type", "", body).await.text(), r#"{"legacy": true}"#);
    std::fs::remove_file(home.hooks().join("state/proyecto.json")).unwrap();
    // Sin el estado ya sería nativa, pero ese requestId lo atendió el Python.
    assert_eq!(request_body(front.port, "POST", "/pane/type", "", body).await.text(), r#"{"legacy": true}"#);
    assert_eq!(typed_text(&home).await, "", "Rust nunca tecleó");
    front.stop().await;
}

#[tokio::test]
async fn pane_type_busy_pane_is_409() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("type-busy");
    cat_session(&home);
    let front = front(&home, dead_port(), home.options()).await;
    let port = front.port;
    let long = format!(r#"{{"session": "s1", "pane": "%0", "text": "{}"}}"#, "y".repeat(400));
    let first = tokio::spawn(async move { request_body(port, "POST", "/pane/type", "", &long).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let busy = request_body(port, "POST", "/pane/type", "", r#"{"session": "s1", "pane": "%0", "text": "z"}"#).await;
    assert_eq!(
        seen(&busy),
        (409, r#"{"error": "Ya se está escribiendo en ese pane; espera a que termine.", "code": "typing_in_progress"}"#.to_owned())
    );
    assert_eq!(first.await.unwrap().status, 200);
    front.stop().await;
}
```

Run: `$C test -p comandos-server --test dash_native_typing -j 6` → FAIL.

- [ ] **Step 2: Implementar `typing.rs`**

Crear `crates/comandos-server/src/dash/native/typing.rs`:

```rust
//! G. POST /pane/type (9667): teclea texto literal en un pane exacto, letra a
//! letra y sin Enter. La caché de 256 respuestas por `requestId` y los
//! candados por pane son del frente; un `requestId` que se declinó se vuelve
//! a declinar siempre, para que un reintento lo responda la caché del Python.
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb,
    light::{data, error, load, tmux_sessions},
    py, reply,
};
use crate::{HandlerError, Request};
use comandos_core::json::truthy;
use comandos_runtime::pane_typing::{self, PaneTypingLocks, TmuxResult, TypingOptions};
use http::StatusCode;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::runtime::Handle;

pub const ROUTES: &[Entry] = &[Entry {
    verb: Verb::Post,
    key: Key::Raw("/pane/type"),
    route: NativeRoute::PaneType,
}];

const CACHE: usize = 256;

#[derive(Default)]
pub struct TypingState {
    locks: PaneTypingLocks,
    /// `_PANE_TYPING_RESULTS`: requestId → (status, cuerpo), FIFO.
    results: Mutex<VecDeque<(String, u16, Value)>>,
    declined: Mutex<VecDeque<String>>,
}

impl TypingState {
    fn cached(&self, rid: &str) -> Option<(u16, Value)> {
        let results = self.results.lock().unwrap_or_else(|p| p.into_inner());
        results
            .iter()
            .find(|(k, _, _)| k == rid)
            .map(|(_, status, body)| (*status, body.clone()))
    }

    /// `results[rid] = res` + `popitem(last=False)` mientras pase de 256.
    fn remember(&self, rid: &str, status: u16, body: &Value) {
        let mut results = self.results.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(slot) = results.iter_mut().find(|(k, _, _)| k == rid) {
            slot.1 = status;
            slot.2 = body.clone();
        } else {
            results.push_back((rid.to_owned(), status, body.clone()));
        }
        while results.len() > CACHE {
            results.pop_front();
        }
    }

    fn was_declined(&self, rid: &str) -> bool {
        let declined = self.declined.lock().unwrap_or_else(|p| p.into_inner());
        declined.iter().any(|k| k == rid)
    }

    fn decline(&self, rid: &str) -> Fault {
        if !rid.is_empty() {
            let mut declined = self.declined.lock().unwrap_or_else(|p| p.into_inner());
            if !declined.iter().any(|k| k == rid) {
                declined.push_back(rid.to_owned());
            }
            while declined.len() > CACHE {
                declined.pop_front();
            }
        }
        Fault::Decline
    }
}

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

/// `str(x or "")`.
fn text_or_empty(value: Option<&Value>) -> Result<String, Fault> {
    match value {
        Some(v) if truthy(v) => py::str_scalar(v).ok_or(Fault::Decline),
        _ => Ok(String::new()),
    }
}

/// `resolve_project_session` (6300) hasta saber si ALGÚN estado nombra esta
/// sesión: si sí, el Python sigue con procesos (`agent_procs`) → se declina.
pub fn project_session_matches(state: &Path, sess: &str) -> Result<bool, Fault> {
    let entries = match std::fs::read_dir(state) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(Fault::Decline),
    };
    for entry in entries {
        let entry = entry.map_err(|_| Fault::Decline)?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || !name.ends_with(".json") {
            continue;
        }
        let path: PathBuf = entry.path();
        // `open()` de un directorio: IsADirectoryError → `continue`.
        if path.is_dir() {
            continue;
        }
        let Some(doc) = load(&path)? else { continue };
        // `.get` de un no-dict o `re.sub` de un no-str: excepción → 500 en el Python.
        let Value::Object(doc) = doc else {
            return Err(Fault::Decline);
        };
        let project = match doc.get("project") {
            None => "",
            Some(Value::String(p)) => p.as_str(),
            Some(_) => return Err(Fault::Decline),
        };
        let derived: String = project
            .chars()
            .map(|c| if c == '.' || c == ':' { '-' } else { c })
            .take(80)
            .collect();
        if derived == sess {
            return Ok(true);
        }
    }
    Ok(false)
}

pub async fn answer(native: &Native, request: &Request) -> Answer {
    let data = data(request)?;
    let tmux = &native.options().tmux;
    let state = &native.typing;
    let sess = match data.get("session") {
        None => "",
        Some(Value::String(s)) => s.as_str(),
        Some(_) => return Err(failure()),
    };
    if !py::is_session(sess) {
        return error(StatusCode::BAD_REQUEST, "Nombre de sesion invalido");
    }
    let rid = py::take_chars(&text_or_empty(data.get("requestId"))?, 64);
    if !rid.is_empty() {
        if state.was_declined(&rid) {
            return Err(Fault::Decline);
        }
        if let Some((status, body)) = state.cached(&rid) {
            let status = StatusCode::from_u16(status).map_err(|_| failure())?;
            return reply(status, &body);
        }
    }
    tmux_sessions(tmux).await?;
    let dir = native.options().hooks.join("state");
    let owned = sess.to_owned();
    let matches = tokio::task::spawn_blocking(move || project_session_matches(&dir, &owned))
        .await
        .map_err(|_| failure())?;
    match matches {
        Ok(false) => {}
        Ok(true) | Err(Fault::Decline) => return Err(state.decline(&rid)),
        Err(fault) => return Err(fault),
    }
    let want = text_or_empty(data.get("pane"))?;
    let mut pane = format!("={sess}:");
    match py::is_pane(&want) {
        None => return Err(state.decline(&rid)),
        Some(true) => {
            let out = tmux
                .run(&["display-message", "-p", "-t", &want, "#{pane_id}"])
                .await
                .map_err(|e| Fault::Error(e.uncaught()))?;
            if py::strip(&out.stdout) == want {
                pane = want.clone();
            }
        }
        Some(false) => {}
    }
    let target = format!("={sess}");
    let has = tmux
        .run(&["has-session", "-t", &target])
        .await
        .map_err(|e| Fault::Error(e.uncaught()))?;
    if !has.ok {
        return error(
            StatusCode::NOT_FOUND,
            &format!("No hay sesion tmux '{sess}'. Levantala primero."),
        );
    }
    if py::is_pane(&want) != Some(true) {
        return reply(
            StatusCode::BAD_REQUEST,
            &json!({"error": "Falta un pane exacto (%N)", "code": "invalid"}),
        );
    }
    if pane != want {
        return reply(
            StatusCode::NOT_FOUND,
            &json!({"error": format!("El pane {want} ya no existe"), "code": "pane_gone"}),
        );
    }
    let text = match data.get("text") {
        Some(Value::String(t)) if !py::strip(t).is_empty() => t.clone(),
        _ => {
            return reply(
                StatusCode::BAD_REQUEST,
                &json!({"error": "Texto vacío", "code": "invalid"}),
            );
        }
    };
    if let Err(e) = pane_typing::validate(&text) {
        if e.message == "Texto vacío" {
            // `strip()` de Python y `is_whitespace` de Rust no coinciden aquí.
            return Err(state.decline(&rid));
        }
        return reply(
            StatusCode::BAD_REQUEST,
            &json!({"error": e.message, "code": "invalid"}),
        );
    }
    if !state.locks.acquire(&pane) {
        return reply(
            StatusCode::CONFLICT,
            &json!({"error": "Ya se está escribiendo en ese pane; espera a que termine.", "code": "typing_in_progress"}),
        );
    }
    let started = Instant::now();
    let handle = Handle::current();
    let runner = tmux.clone();
    let (target_pane, literal) = (pane.clone(), text.clone());
    let typed = tokio::task::spawn_blocking(move || {
        let mut uncaught = None;
        let result = pane_typing::type_literal(
            |args| match runner.run_blocking(&handle, args) {
                Ok(out) => TmuxResult {
                    returncode: if out.ok { 0 } else { 1 },
                    stdout: out.stdout,
                    stderr: out.stderr,
                },
                Err(e) => {
                    uncaught.get_or_insert(e.uncaught());
                    TmuxResult {
                        returncode: 1,
                        ..TmuxResult::default()
                    }
                }
            },
            &target_pane,
            &literal,
            |seconds| std::thread::sleep(Duration::from_secs_f64(seconds)),
            TypingOptions::default(),
        );
        (result, uncaught)
    })
    .await;
    state.locks.release(&pane);
    let (result, uncaught) = typed.map_err(|_| failure())?;
    if let Some(error) = uncaught {
        // TimeoutExpired/FileNotFoundError: la excepción sale sin cachear.
        return Err(Fault::Error(error));
    }
    let (status, body) = match result {
        Ok(out) => (
            200u16,
            json!({
                "ok": true,
                "typed": out.typed,
                "durationMs": started.elapsed().as_millis() as u64,
                "requestId": rid,
            }),
        ),
        Err(e) => (
            502u16,
            json!({"error": e.message, "code": e.code, "typed": e.typed, "requestId": rid}),
        ),
    };
    if !rid.is_empty() {
        state.remember(&rid, status, &body);
    }
    reply(StatusCode::from_u16(status).map_err(|_| failure())?, &body)
}
```

(`started.elapsed().as_millis() as u64`: el `as` trunca un `u128` que nunca pasa de unos segundos; si clippy lo marca, usar `u64::try_from(…).unwrap_or(u64::MAX)`.)

En `mod.rs`: `pub mod typing;`, `NativeRoute::PaneType`, `typing::ROUTES` en `TABLES`, `NativeRoute::PaneType => typing::answer(self, request).await,` y el campo `pub(crate) typing: typing::TypingState` (`typing: typing::TypingState::default(),` en `new`).

- [ ] **Step 3: Ver pasar**

Run: `$C test -p comandos-server --test dash_native_typing -j 6` → PASS; `$C test -p comandos-server -j 6` verde.

- [ ] **Step 4: Fixture y paridad**

```
# --- 2c · G2: tecleo en pane (nativa; la última teclea en el tmux privado de cada copia)
{"name":"g-pane-type-mala","method":"POST","path":"/pane/type","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"a b","text":"x"},"volatile":[],"expect":"same"}
{"name":"g-pane-type-nadie","method":"POST","path":"/pane/type","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"no-existe","pane":"%0","text":"x"},"volatile":[],"expect":"same"}
{"name":"g-pane-type-sin-pane","method":"POST","path":"/pane/type","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"local","text":"x"},"volatile":[],"expect":"same"}
{"name":"g-pane-type-vacio","method":"POST","path":"/pane/type","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"local","pane":"%0","text":"  "},"volatile":[],"expect":"same"}
{"name":"g-pane-type","method":"POST","path":"/pane/type","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"local","pane":"%0","text":"ab","requestId":"paridad-1"},"volatile":["/durationMs"],"expect":"same"}
```

Run: la orden de paridad de la Tarea 2. Expected: 0 DIFF; POST `/pane/type` fuera de «reenviadas». Si la copia de `~/.claude/hooks/state` tiene un estado cuyo proyecto se llame `local`, `g-pane-type` aparecerá reenviada (correcto: declina) — anotarlo y seguir.

- [ ] **Step 5: fmt, clippy, commit**

```bash
$C fmt --all -- --check
$C clippy --workspace --all-targets -j 6 -- -D warnings
git add crates/comandos-server/src/dash/native/typing.rs crates/comandos-server/src/dash/native/mod.rs \
  crates/comandos-server/tests/dash_native_typing.rs
git add -f xtask/parity/frente.jsonl
git commit -m "feat(dash): dominio G2 nativo — POST /pane/type

Tecleo literal por tmux send-keys -l desde el frente, con candado por pane y
caché FIFO de 256 respuestas por requestId. Si algún estado de hooks nombra la
sesión (la resolución sigue con procesos) se declina, y ese requestId se
declina siempre después para que nunca se teclee dos veces.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Dominio H — GET `/model/status` (journal de operaciones de sesión)

Desplegable sola. El tablero (`index.html:2587`, mientras haya cambios pendientes), `term.html:2278` y `cc-app:5279` sondean el progreso de un cambio de cuenta o modelo. Lee el journal `H/session-operations.sqlite3` por un tercer carril.

Correspondencia (8452, `session_operation_status` 6335): consulta `parse_qs` (primera ocurrencia no vacía) de `operationKey` y `operationId`; `session, _, pane = operationKey.partition('|')`; `SESSION_RE.fullmatch(session)` y, si hay `pane`, `PANE_RE.fullmatch(pane)` → si no, 400 `operationKey inválido`; `operationId` no vacío fuera de `[A-Za-z0-9_-]{1,128}` → 400 `operationId inválido`. Luego `recover_abandoned()` (dueño muerto → `failed` si estaba en `validating|waiting|snapshot`, si no `recovery_required`; `os.kill(pid, 0)` ≡ existe `/proc/<pid>`), `record = get(id)` o `latest_for_target(session, pane)`; con `operationId` y sin registro o con otro panel → 400 `La operación no pertenece a este panel`. Sin `operationId` y sin registro → `Decline` (`MOTOR_RESULT` en memoria). `awaiting_confirmation` → `Decline`. Resultado: `operationId`, `state`, `ts` (= `updated`, flotante); `durable = result or {}`; terminal (`confirmed|failed|rolled_back|recovery_required`) → `update(durable)`, `detail = durable.get('error') or 'configuración confirmada'`, `update(durable.get('observed') or {})`; si no, `update(durable)`, `pop('ok')` (conservando el orden: `shift_remove`), `stage` y `stageCode` = estado; después `sourceHarness`, `sourceConversationId`, `handoffPath` del `snapshot.origin` (`or ''`); al final `operationKey`. Formas que el Python no puede `.get` (no-dict verdadero) → `Decline`. **Efecto lateral**: `recover_abandoned` escribe y el journal se crea si falta; es idempotente y es lo primero que hace también el Python, así que declinar después es equivalente a declinar antes.

**Files:**
- Modify: `crates/comandos-server/src/dash/native/lanes.rs` (`JournalBackend`), `crates/comandos-server/src/dash/native/mod.rs`
- Create: `crates/comandos-server/src/dash/native/operations.rs`
- Modify: `crates/comandos-server/tests/support/mod.rs`; Create: `crates/comandos-server/tests/dash_native_operations.rs`
- Modify: `xtask/parity/frente.jsonl`

**Interfaces:**
- Produces: `lanes::JournalBackend`, `NativeOptions.journal_db`, `Native.journal`, `operations::{ROUTES, answer}`, `NativeRoute::ModelStatus`, `support::TestHome::journal_db()`.
- Consumes: `comandos_runtime::session_operations::{open_journal, OperationStore}`, `query::Query`, `Lane` (Tarea 2).

- [ ] **Step 1: Pruebas que fallan**

En `tests/support/mod.rs`, dentro de `impl TestHome`:

```rust
    pub fn journal_db(&self) -> PathBuf {
        self.hooks().join("session-operations.sqlite3")
    }
```

Crear `crates/comandos-server/tests/dash_native_operations.rs`:

```rust
//! Dominio H: GET /model/status sobre el journal de operaciones.
mod support;

use comandos_runtime::session_operations::open_journal;
use rusqlite::params;
use support::{FakeLegacy, TestHome, dead_port, front, get, oracle::oracle};

/// Un pid que no existe: por encima de cualquier `pid_max`.
const DEAD: i64 = 2_147_483_647;

fn seed(home: &TestHome, rows: &[(&str, &str, &str, &str, i64, Option<&str>, Option<&str>, f64)]) {
    let conn = open_journal(&home.journal_db()).unwrap();
    for (id, pane_key, request, state, owner, snapshot, result, updated) in rows {
        conn.execute(
            "INSERT INTO session_operations VALUES (?,?,?,?,?,?,?,?,?)",
            params![id, pane_key, "f", request, state, owner, snapshot, result, updated],
        )
        .unwrap();
    }
}

fn rows(alive: i64) -> Vec<(&'static str, &'static str, &'static str, &'static str, i64, Option<&'static str>, Option<&'static str>, f64)> {
    vec![
        ("op-ok", "s1|%1", r#"{"session": "s1", "pane": "%1"}"#, "confirmed", alive,
         Some(r#"{"origin": {"agent": "claude", "observed": {"conversationId": "c-1"}, "handoffPath": "/tmp/h"}}"#),
         Some(r#"{"ok": true, "model": "m", "observed": {"harness": "codex", "model": "gpt"}}"#), 1791115200.25),
        ("op-run", "s2|%2", r#"{"session": "s2", "pane": "%2"}"#, "applying", alive, None,
         Some(r#"{"ok": true, "pending": true}"#), 1791115201.5),
        ("op-dead", "s3", r#"{"session": "s3", "pane": ""}"#, "validating", DEAD, None, None, 1791115202.0),
    ]
}

#[tokio::test]
async fn model_status_matches_python_oracle() {
    let home = TestHome::new("ops-oracle");
    let me = i64::from(std::process::id());
    seed(&home, &rows(me));
    let Some(py) = oracle(&home).await else { return };
    let front = front(&home, dead_port(), home.options()).await;
    for target in [
        "/model/status?operationKey=s1%7C%251",
        "/model/status?operationKey=s1%7C%251&operationId=op-ok",
        "/model/status?operationKey=s2%7C%252",
        "/model/status?operationKey=s3",
        "/model/status?operationKey=s3&operationId=op-dead",
        "/model/status?operationKey=a%20b",
        "/model/status?operationKey=s1%7Cx",
        "/model/status?operationKey=s1&operationId=%24%24",
        "/model/status?operationKey=s1&operationId=op-ok",
        "/model/status?operationKey=s1&operationId=nadie",
    ] {
        // Rust primero en la mitad de los casos: recover_abandoned es idempotente.
        let (b, a) = (get(front.port, target).await, get(py.port, target).await);
        assert_eq!((a.status, a.text()), (b.status, b.text()), "{target}");
    }
    let b = get(front.port, "/model/status?operationKey=s1%7C%251").await;
    assert_eq!(
        b.text(),
        r#"{"operationId": "op-ok", "state": "confirmed", "ts": 1791115200.25, "ok": true, "model": "gpt", "observed": {"harness": "codex", "model": "gpt"}, "detail": "configuración confirmada", "harness": "codex", "sourceHarness": "claude", "sourceConversationId": "c-1", "handoffPath": "/tmp/h", "operationKey": "s1|%1"}"#
    );
    front.stop().await;
}

#[tokio::test]
async fn model_status_recovers_dead_owner_natively() {
    let home = TestHome::new("ops-recover");
    seed(&home, &rows(i64::from(std::process::id())));
    let front = front(&home, dead_port(), home.options()).await;
    let wire = get(front.port, "/model/status?operationKey=s3").await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    assert!(wire.text().contains(r#""state": "failed""#), "{}", wire.text());
    assert!(wire.text().contains(r#""detail": "operación interrumpida; revisar recuperación""#), "{}", wire.text());
    front.stop().await;
}

#[tokio::test]
async fn model_status_declines_pending_confirmation_and_motor_result() {
    let home = TestHome::new("ops-decline");
    seed(&home, &[("op-wait", "s4|%4", r#"{"session": "s4", "pane": "%4"}"#, "awaiting_confirmation",
        i64::from(std::process::id()), Some(r#"{"origin": {}}"#), Some("{}"), 1.0)]);
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    for target in ["/model/status?operationKey=s4%7C%254", "/model/status?operationKey=sin-registro"] {
        assert_eq!(get(front.port, target).await.text(), r#"{"legacy": true}"#, "{target}");
    }
    assert_eq!(legacy.requests().len(), 2);
    front.stop().await;
}
```

(El caso `op-ok` comprueba el orden: `update(durable)` añade `ok`, `model`, `observed`; `detail`; `update(observed)` reescribe `model` en su sitio y añade `harness`.)

Run: `$C test -p comandos-server --test dash_native_operations -j 6` → FAIL.

- [ ] **Step 2: Carril del journal**

En `crates/comandos-server/src/dash/native/lanes.rs`:

```rust
/// `~/.claude/hooks/session-operations.sqlite3` (2132). El Python no versiona
/// su esquema: la puerta exige las 9 columnas de `session_operations` en orden.
pub struct JournalBackend {
    pub conn: Connection,
}

const JOURNAL_COLUMNS: [&str; 9] = [
    "id", "pane_key", "fingerprint", "request", "state", "owner", "snapshot", "result", "updated",
];

impl LaneBackend for JournalBackend {
    const ROUTES: &'static str = "GET /model/status";

    fn open(path: &Path) -> Result<Self, Refusal> {
        let conn = comandos_runtime::session_operations::open_journal(path)
            .map_err(|e| Refusal::Unopened(e.to_string()))?;
        let backend = Self { conn };
        backend.admit()?;
        Ok(backend)
    }

    fn admit(&self) -> Result<(), Refusal> {
        let names = self
            .conn
            .prepare("PRAGMA table_info(session_operations)")
            .and_then(|mut stmt| {
                stmt.query_map([], |r| r.get::<_, String>(1))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(|e| Refusal::Unopened(e.to_string()))?;
        if names.iter().map(String::as_str).eq(JOURNAL_COLUMNS) {
            Ok(())
        } else {
            Err(Refusal::Unopened(format!(
                "columnas de session_operations desconocidas: {}",
                names.join(",")
            )))
        }
    }
}
```

En `mod.rs`: `NativeOptions.journal_db: PathBuf` (`home.join(".claude/hooks/session-operations.sqlite3")` en `for_home`); `Native.journal: lanes::Lane<lanes::JournalBackend>` (`lanes::Lane::new(opts.journal_db.clone())` en `new`; `self.journal.shutdown().await;` en `shutdown`); `pub mod operations;`, `NativeRoute::ModelStatus`, `operations::ROUTES` en `TABLES`, `NativeRoute::ModelStatus => operations::answer(self, request).await,`.

- [ ] **Step 3: Implementar `operations.rs`**

Crear `crates/comandos-server/src/dash/native/operations.rs`:

```rust
//! H. GET /model/status (8452, `session_operation_status` 6335) sobre el
//! journal. Sin registro (el `MOTOR_RESULT` vive en la memoria del Python) o
//! con `awaiting_confirmation` (`refresh_session_confirmation` observa el
//! proceso y escribe) se declina; `recover_abandoned` va antes, como en el
//! Python, y es idempotente.
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb, light::error, py, query::Query, reply,
};
use crate::HandlerError;
use comandos_core::json::truthy;
use comandos_runtime::session_operations::OperationStore;
use http::StatusCode;
use rusqlite::Connection;
use serde_json::{Map, Value};
use std::path::Path;

pub const ROUTES: &[Entry] = &[Entry {
    verb: Verb::Get,
    key: Key::Path("/model/status"),
    route: NativeRoute::ModelStatus,
}];

const TERMINAL: [&str; 4] = ["confirmed", "failed", "rolled_back", "recovery_required"];

enum Status {
    Ok(Map<String, Value>),
    BadRequest(&'static str),
    Decline,
    Failed,
}

pub async fn answer(native: &Native, request: &crate::Request) -> Answer {
    let query = Query::parse(&request.target)?;
    let operation = query.first("operationKey").unwrap_or("").to_owned();
    let operation_id = query.first("operationId").unwrap_or("").to_owned();
    let (session, pane) = operation.split_once('|').unwrap_or((operation.as_str(), ""));
    if !py::is_session(session) {
        return error(StatusCode::BAD_REQUEST, "operationKey inválido");
    }
    if !pane.is_empty() && !py::is_pane(pane).ok_or(Fault::Decline)? {
        return error(StatusCode::BAD_REQUEST, "operationKey inválido");
    }
    let id_ok = (1..=128).contains(&operation_id.len())
        && operation_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !operation_id.is_empty() && !id_ok {
        return error(StatusCode::BAD_REQUEST, "operationId inválido");
    }
    let (session, pane) = (session.to_owned(), pane.to_owned());
    let clock = native.options().clock.clone();
    let status = native
        .journal
        .with(move |j| {
            let now = move || clock() as f64 / 1000.0;
            status_in(&j.conn, &session, &pane, &operation_id, &now)
        })
        .await?;
    match status {
        Status::Ok(mut result) => {
            result.insert("operationKey".into(), Value::String(operation));
            reply(StatusCode::OK, &Value::Object(result))
        }
        Status::BadRequest(message) => error(StatusCode::BAD_REQUEST, message),
        Status::Decline => Err(Fault::Decline),
        Status::Failed => Err(Fault::Error(HandlerError::Failure)),
    }
}

/// `os.kill(pid, 0)`: `ProcessLookupError` → muerto; `PermissionError` → vivo.
fn alive(pid: i64) -> comandos_runtime::session_operations::Result<bool> {
    Ok(pid <= 0 || Path::new(&format!("/proc/{pid}")).exists())
}

/// `x or {}` donde x debe ser un dict; un no-dict verdadero haría fallar el `.get`.
fn object_or_empty(value: Option<&Value>) -> Option<Map<String, Value>> {
    match value {
        Some(Value::Object(map)) => Some(map.clone()),
        Some(v) if truthy(v) => None,
        _ => Some(Map::new()),
    }
}

/// `x or ''`.
fn or_empty(value: Option<&Value>) -> Value {
    match value {
        Some(v) if truthy(v) => v.clone(),
        _ => Value::String(String::new()),
    }
}

fn status_in(
    conn: &Connection,
    session: &str,
    pane: &str,
    operation_id: &str,
    now: &dyn Fn() -> f64,
) -> Status {
    let owner = || 0i64;
    let store = OperationStore {
        connection: conn,
        owner: &owner,
        clock: now,
    };
    if store.recover_abandoned(alive).is_err() {
        return Status::Failed;
    }
    let record = if operation_id.is_empty() {
        store.latest_for_target(session, pane)
    } else {
        store.get(operation_id)
    };
    let Ok(record) = record else {
        return Status::Decline;
    };
    if !operation_id.is_empty() {
        let Some(found) = &record else {
            return Status::BadRequest("La operación no pertenece a este panel");
        };
        let Value::Object(req) = &found["request"] else {
            return Status::Decline;
        };
        let same_session = req.get("session") == Some(&Value::String(session.to_owned()));
        let same_pane = match req.get("pane") {
            Some(v) if truthy(v) => v == &Value::String(pane.to_owned()),
            _ => pane.is_empty(),
        };
        if !same_session || !same_pane {
            return Status::BadRequest("La operación no pertenece a este panel");
        }
    }
    let Some(record) = record else {
        return Status::Decline;
    };
    let Some(state) = record["state"].as_str() else {
        return Status::Decline;
    };
    if state == "awaiting_confirmation" {
        return Status::Decline;
    }
    let mut result = Map::new();
    result.insert("operationId".into(), record["id"].clone());
    result.insert("state".into(), Value::String(state.to_owned()));
    result.insert("ts".into(), record["updated"].clone());
    let Some(durable) = object_or_empty(record.get("result")) else {
        return Status::Decline;
    };
    for (k, v) in &durable {
        result.insert(k.clone(), v.clone());
    }
    if TERMINAL.contains(&state) {
        let detail = match durable.get("error") {
            Some(v) if truthy(v) => v.clone(),
            _ => Value::String("configuración confirmada".into()),
        };
        result.insert("detail".into(), detail);
        let Some(observed) = object_or_empty(durable.get("observed")) else {
            return Status::Decline;
        };
        for (k, v) in observed {
            result.insert(k, v);
        }
    } else {
        result.shift_remove("ok");
        result.insert("stage".into(), Value::String(state.to_owned()));
        result.insert("stageCode".into(), Value::String(state.to_owned()));
    }
    let Some(snapshot) = object_or_empty(record.get("snapshot")) else {
        return Status::Decline;
    };
    let Some(origin) = object_or_empty(snapshot.get("origin")) else {
        return Status::Decline;
    };
    let Some(observed) = object_or_empty(origin.get("observed")) else {
        return Status::Decline;
    };
    result.insert("sourceHarness".into(), or_empty(origin.get("agent")));
    result.insert(
        "sourceConversationId".into(),
        or_empty(observed.get("conversationId")),
    );
    result.insert("handoffPath".into(), or_empty(origin.get("handoffPath")));
    Status::Ok(result)
}
```

(Si la versión de `serde_json` del workspace no expone `Map::shift_remove`, usar `Map::remove` solo después de comprobar en su documentación que con `preserve_order` conserva el orden; si no lo conserva, reconstruir el mapa sin `ok` con un `filter`. La prueba `model_status_matches_python_oracle` con `op-run` lo fija.)

- [ ] **Step 4: Ver pasar**

Run: `$C test -p comandos-server --test dash_native_operations -j 6` → PASS; `$C test -p comandos-server -j 6` verde.

- [ ] **Step 5: Fixture y paridad**

```
# --- 2c · H: estado de operaciones (nativa; sin registro se reenvía)
{"name":"h-model-status-clave","method":"GET","path":"/model/status?operationKey=a%20b","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"h-model-status-id","method":"GET","path":"/model/status?operationKey=local&operationId=%24%24","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"h-model-status-ajena","method":"GET","path":"/model/status?operationKey=local&operationId=no-existe","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"h-model-status-sin-registro","method":"GET","path":"/model/status?operationKey=sin-registro-paridad","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same","forwarded":true}
```

Run: la orden de paridad de la Tarea 2. Expected: 0 DIFF; GET `/model/status` reenviada solo por `h-model-status-sin-registro`.

- [ ] **Step 6: fmt, clippy, commit**

```bash
$C fmt --all -- --check
$C clippy --workspace --all-targets -j 6 -- -D warnings
git add crates/comandos-server/src/dash/native/lanes.rs crates/comandos-server/src/dash/native/mod.rs \
  crates/comandos-server/src/dash/native/operations.rs crates/comandos-server/tests/support/mod.rs \
  crates/comandos-server/tests/dash_native_operations.rs
git add -f xtask/parity/frente.jsonl
git commit -m "feat(dash): dominio H nativo — GET /model/status

El journal session-operations.sqlite3 entra por su propio carril (puerta: las
9 columnas de session_operations). Sin registro (MOTOR_RESULT vive en el
Python) o esperando confirmación (observa el proceso) se declina;
recover_abandoned corre antes, igual que en el Python, y es idempotente.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Documento de cutover «2c: dominios nativos II» y corrida completa

**Files:**
- Modify: `docs/verification/cutover-dash.md` (sección nueva al final, tras «2b»)

- [ ] **Step 1: Corrida completa antes de escribir**

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
  --comandos .build/target/release/comandos
```

Expected: 0 fallos, 0 DIFF; anotar número de pruebas, OK/DIFF/SKIP, rutas reenviadas distintas y Pss del frente minuto 1 y 10 (plano, ± 1 MiB) para la subsección «Medido antes del cutover».

- [ ] **Step 2: Añadir la sección**

Al final de `docs/verification/cutover-dash.md`:

````markdown
## 2c: dominios nativos II

Procedimiento para el controlador, como el de la 2b. El frente ya está en 4777 (release
`2bae7f9cd7d6`) y el Python heredado en 4781; la 2c solo cambia el binario del frente. Ni el
Python ni las unidades ni tmux se tocan.

### Qué cambia

El frente responde 12 rutas más. Detalle, líneas del Python y razones de lo que sigue reenviado
en `docs/superpowers/plans/2026-10-04-fase-2c-dominios-nativos-ii.md` («Mapa de rutas nativas»).

| Dominio | Rutas nativas |
|---|---|
| F · snippets, uso, Pomodoro, catálogos (8) | GET `/snippets`, `/pomodoro`, `/model-tiers`, `/sovereignty`; POST `/snippets`, `/snippets/update`, `/snippets/delete`, `/ui-log` |
| G · terminal (3) | POST `/terminal-panes` (salvo `action:"close"`), `/terminal-history`, `/pane/type` |
| H · operaciones (1) | GET `/model/status` |

Siguen en el Python: POST `/pomodoro` (despierta su scheduler en memoria), GET `/analytics/week`
(límites de proveedor en memoria, red), `/providers`, `/optimization/plans`, `/accounts`,
`/opencode/models`, `/session-profiles`, `/extension-usage`, POST `/terminal/quick`, `/state`,
`/usage/*` y todo lo no listado. Dentro de las nativas se reenvían: `/terminal-panes` con
`close`, `/model/status` sin registro en el journal o esperando confirmación, `/pane/type` cuando
un estado de `~/.claude/hooks/state` nombra la sesión, y cualquier escritura con el `flock` de
`snippets.json`/`ui-events.jsonl` tomado.

Bases nuevas, cada una con su hilo y su apagado propio (no global):

- `~/.claude/hooks/comandos-usage.sqlite` (la base de uso que usa `cc-dash`, que **no** honra
  `COMANDOS_USAGE_DB`): si un Python más nuevo la migra por encima de `user_version` 11, una
  línea `comandos dash: …comandos-usage.sqlite: tiene esquema N…; GET /pomodoro y GET /sovereignty
  se reenvían al heredado` y solo esas dos se reenvían hasta reiniciar.
- `~/.claude/hooks/session-operations.sqlite3`: si `session_operations` cambia de columnas, una
  línea análoga y solo GET `/model/status` se reenvía.

`/model-tiers` lee `config/model-tiers.json` del checkout: `COMANDOS_DASH_REPO` o el destino de
`~/.claude/hooks/dash/index.html` dos niveles arriba. Comprobar en el paso 0 que es el mismo
checkout que ejecuta `cc-dash-legacy.service`.

### Diferencias y comportamientos aceptados (2c)

- GET `/pomodoro` registra una vez por proceso la política de foco (idempotente) y puede crear la
  base de uso si faltara, como ya hacía el Python. GET `/model/status` marca como fallidas las
  operaciones de dueños muertos (`recover_abandoned`), como el Python.
- `/pane/type`: la caché de 256 respuestas por `requestId` es del frente; un `requestId` que el
  frente declinó se declina siempre, así un reintento nunca teclea dos veces.
- `/terminal-panes` y el Python ya no comparten candado de proceso: cada uno serializa lo suyo; la
  condición `if-shell` de tmux sigue garantizando que una acción no se aplique a un pane cambiado.
- Una escritura de snippets o de `ui-log` con el candado tomado se reenvía (el Python sí espera).

### 0. Previos

```sh
cd ~/codebase/0xJesus/ComandOS && git log -1 --oneline        # main con la Fase 2c fusionada
CARGO_TARGET_DIR=$PWD/.build/target nice -n 10 cargo build --release -p comandos-cli -j 6
CARGO_TARGET_DIR=$PWD/.build/target nice -n 10 cargo build -p xtask -j 6
NEW=$HOME/codebase/0xJesus/ComandOS/.build/target/release/comandos
XT=$HOME/codebase/0xJesus/ComandOS/.build/target/debug/xtask
grep -qa COMANDOS_DASH_REPO "$NEW" || echo "BINARIO SIN 2c: no seguir"
systemctl --user is-active cc-dash.service cc-dash-legacy.service    # active active
~/.local/share/comandos/bin/comandos install --releases              # anotar la actual ('*': 2bae7f9cd7d6)
readlink -f ~/.claude/hooks/dash/index.html                          # …/codebase/0xJesus/ComandOS/dash/index.html
ss -ltn 'sport = :4782'                                               # libre
```

### 1. Sombra en 4782 con nativo, contra el heredado 4781

Igual que en la 2b: la sombra usa las bases reales (sus rutas nativas escriben lo mismo que el
tablero: snippets, `ui-events.jsonl`, `recover_abandoned`); nunca `xtask poll` contra 4777/4781/4782.

```sh
COMANDOS_DASH_TRACE_FORWARD=1 "$NEW" dash 4782 --legacy-port 4781 2> /tmp/sombra-2c.log
```

Desde otra terminal, en `~/codebase/0xJesus/ComandOS`:

```sh
"$XT" parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 \
  --usage-db ~/.claude/hooks/comandos-usage.sqlite --comandos "$NEW"        # 0 DIFF
"$XT" poll --shadow --minutes 10 --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 \
  --usage-db ~/.claude/hooks/comandos-usage.sqlite --comandos "$NEW"        # Pss plano
for r in /snippets /model-tiers /pomodoro; do
  cmp -s <(curl -s 127.0.0.1:4782$r | sed 's/"serverNowMs": [0-9]*//') \
         <(curl -s 127.0.0.1:4781$r | sed 's/"serverNowMs": [0-9]*//') && echo "igual $r" || echo "DISTINTO $r"
done
S=$(tmux list-sessions -F '#{session_name}' | head -1)
for p in /terminal-panes /terminal-history; do
  B="{\"session\":\"$S\"}"
  cmp -s <(curl -s -H 'Content-Type: application/json' -d "$B" 127.0.0.1:4782$p) \
         <(curl -s -H 'Content-Type: application/json' -d "$B" 127.0.0.1:4781$p) && echo "igual $p" || echo "DISTINTO $p"
done                                    # el historial puede cambiar entre las dos lecturas: repetir
```

Navegación manual en `http://127.0.0.1:4782` con `chrome-bg` (vía `cc-browser-expose start 4782`):
crear, editar y borrar un snippet; abrir el Pomodoro; abrir «Soberanía»; en una pestaña remota
partir un pane, redimensionarlo y seleccionar otro; teclear un comando desde la barra de comandos
(sin Enter). Luego:

```sh
grep -c 'se reenvían al heredado\|rutas nativas desactivadas' /tmp/sombra-2c.log   # 0
grep 'reenvío' /tmp/sombra-2c.log | sed 's/.*reenvío //' | sort | uniq -c | sort -rn
```

No deben aparecer las rutas de «Qué cambia» salvo los casos reenviados descritos arriba. Ctrl+C.

### 2. Cutover

```sh
"$NEW" install --stage
grep -qa COMANDOS_DASH_REPO "$(readlink -f ~/.local/share/comandos/bin/comandos)" || echo "STAGE MALO: ~/.local/share/comandos/bin/comandos install --rollback-release"
~/.local/share/comandos/bin/comandos hook claude-status >/dev/null && echo "hooks OK con la release nueva"
systemctl --user restart cc-dash.service
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:4777/        # 200
journalctl --user -u cc-dash.service --since -2min --no-pager | grep -c 'se reenvían al heredado\|rutas nativas desactivadas'   # 0
```

Sin operaciones de sesión en vuelo (`/model/status` pendiente en el tablero) en el momento del
reinicio. El tablero queda sin respuesta unos 3 s; cc-app reintenta solo.

### 3. Verificación por dominio

```sh
systemctl --user set-environment COMANDOS_DASH_TRACE_FORWARD=1 && systemctl --user restart cc-dash.service
sleep 600
journalctl --user -u cc-dash.service --since -11min --no-pager | grep 'reenvío' \
  | sed 's/.*reenvío //' | sort | uniq -c | sort -rn
systemctl --user unset-environment COMANDOS_DASH_TRACE_FORWARD && systemctl --user restart cc-dash.service
```

- **F**: no aparecen `/snippets*`, `/ui-log`, GET `/pomodoro`, `/model-tiers`, `/sovereignty`. Un snippet creado en el tablero aparece en cc-app y viceversa; POST `/pomodoro` (iniciar/pausar) sí aparece.
- **G**: POST `/terminal-panes` solo aparece al cerrar un pane; `/terminal-history` y `/pane/type` no aparecen. Con dos iframes remotos abiertos, `/terminal-panes` cada 2 s no se acumula en la traza.
- **H**: GET `/model/status` solo aparece en un cambio de cuenta que espera confirmación o sin operación registrada; el resto del cambio se sigue desde Rust.
- `/state`, `/usage/state` y `/analytics/week` siguen apareciendo: es lo esperado.
- Memoria: `grep Pss /proc/$(systemctl --user show -p MainPID --value cc-dash.service)/smaps_rollup` al minuto 1 y al 10: plano (± 1 MiB).

### 4. Reversión

Igual que en la 2b. A/B sin cambiar binario:

```sh
systemctl --user set-environment COMANDOS_DASH_NATIVE=0 && systemctl --user restart cc-dash.service
# persistente: drop-in ~/.config/systemd/user/cc-dash.service.d/no-native.conf con
# [Service]\nEnvironment=COMANDOS_DASH_NATIVE=0, daemon-reload y restart (deshacer: rm + daemon-reload + restart)
```

Volver a la release anterior (`2bae7f9cd7d6`, la 2b):

```sh
~/.local/share/comandos/bin/comandos install --releases
~/.local/share/comandos/bin/comandos install --rollback-release
systemctl --user restart cc-dash.service
grep -qa COMANDOS_DASH_REPO "$(readlink -f ~/.local/share/comandos/bin/comandos)" && echo "SIGUE LA 2c"
```

Las escrituras nativas (snippets, `ui-events.jsonl`, journal, política de foco) están en los mismos
archivos y bases que usa el Python, con el mismo formato: revertir no necesita limpiar nada.

### Medido antes del cutover

(Rellenar con los números del paso 1 del plan 2c: pruebas, OK/DIFF/SKIP, rutas reenviadas, Pss.)
````

- [ ] **Step 3: Verificar el documento**

Run: `grep -n '^## 2c' docs/verification/cutover-dash.md` (una línea) y revisar que cada orden usa `"$NEW"`/`"$XT"` con ruta explícita y que la subsección «Medido antes del cutover» tiene los números reales del Step 1.

- [ ] **Step 4: Commit**

```bash
git add docs/verification/cutover-dash.md
git add -f docs/superpowers/plans/2026-10-04-fase-2c-dominios-nativos-ii.md
git commit -m "docs(verification): cutover 2c — dominios nativos II

Sombra con --usage-db, verificación por dominio (F, G, H), carriles de base con
apagado propio y reversión por env/drop-in o --rollback-release.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Self-review

- **Cobertura de los rulings**: 1 → cada tarea compara bytes con el oráculo (`*_matches_python_oracle`); 2 → `lanes.rs` (Tarea 2, journal en la 6) con puerta y apagado por base, prueba `usage_newer_schema_disables_only_usage_lane`; 3 → `Decline` antes de efectos en todas las tareas, con la excepción documentada de `recover_abandoned`; 4 → `Tmux::run_blocking` sobre el mismo ejecutor; 5 → llaves `Raw`/`Path` por ruta, sin tocar `--no-native`; 6 → líneas `f-*`, `g-*`, `h-*` y `--usage-db`; 7 → Tarea 7; 8 → Global Constraints; 9 → orden F (1–3), G (4–5), H (6), documentación (7).
- **Alcance pedido que queda reenviado**: POST `/pomodoro`, `/analytics/week`, `/providers`, `/optimization/plans`, `/accounts`, `/opencode/models`, `/session-profiles`, `/extension-usage`, `/terminal/quick` y `close` de `/terminal-panes`, cada uno con su razón comprobada en el Python (Decisiones).
- **Placeholders**: ninguno salvo la subsección «Medido antes del cutover», que se rellena con los números del Step 1 de la Tarea 7 (son mediciones, no diseño).
- **Tipos**: `Lane::with` / `Native::with_state` devuelven `Result<T, Fault>`; `NativeRoute::{Snippets, UiLog, Pomodoro, Catalog, Terminal, PaneType, ModelStatus}` coinciden entre `mod.rs` y cada módulo; `NativeOptions.{usage_db, desktop_device, repo_root, journal_db}` se crean en `for_home` (repo_root en `build`).
- **Review Focus**: las cinco líneas tienen su prueba en la tarea dueña (1, 2, 5, 4, 6).
