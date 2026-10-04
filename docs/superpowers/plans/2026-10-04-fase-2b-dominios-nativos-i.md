# Fase 2b — Dominios nativos I: plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** que el frente Rust `comandos dash` (4777, en producción desde la release 17aa1bea2309) responda él mismo, byte a byte igual que el `cc-dash` Python, las rutas de cinco dominios — lecturas ligeras, eventos y marcas, notificaciones, workspace y rutas retiradas — sin tocar el Python, que sigue detrás como oráculo y como destino de todo lo demás.

**Architecture:** el enrutador gana una tercera clase, `Native`, que se comprueba antes que `Forward` con coincidencia exacta de ruta y método. Cada ruta nativa es una función async en `crates/comandos-server/src/dash/native/`; lo que toca SQLite pasa por un único `BackendWorker<StateBackend>` (un hilo, una conexión a `app-state.sqlite3`, puerta de esquema antes de cada trabajo); tmux y `fc-list` van por `tokio::process` con el mismo plazo que el Python. Ante una entrada que el Rust no puede reproducir con certeza (dígitos no ASCII, enteros fuera de `i64`, formas JSON exóticas, esquema más nuevo) el manejador responde `Decline` antes de cualquier efecto y el frente reenvía la petición original al Python: la paridad nunca se adivina. `--no-native` / `COMANDOS_DASH_NATIVE=0` devuelven el comportamiento exacto de la Fase 2a para comparar A/B.

**Tech Stack:** Rust 1.96 (edition 2024, `unsafe_code = "forbid"`), tokio 1.53 (se añade la feature `process`), hyper 1.11, rusqlite 0.40 (bundled), serde_json con `arbitrary_precision` + `preserve_order`, sha2 0.10.9 y getrandom 0.3.4 (ya en el workspace), crates `comandos-core`/`comandos-store`/`comandos-runtime`; arnés `xtask parity`/`xtask poll` en un netns (`unshare -Urn`).

**Spec:** `docs/superpowers/specs/2026-10-04-comandos-rust-complete-design.md` (§4.2, §5, §7, Enmiendas 1–8; Fase 2 dividida en 2a/2b). Inventario: `docs/research/2026-10-04-fase-2-inventario.md` (§1.3, §1.7, §1.9, §1.11, §1.12, §3.3, §7, §9.7). Plan previo: `docs/superpowers/plans/2026-10-04-fase-2a-dash-cimientos.md`. Procedimiento vivo: `docs/verification/cutover-dash.md`.

---

## Rulings del controlador que fijan este plan

1. **Respuestas idénticas byte a byte**: status, `Content-Type`, `Cache-Control: no-store` y cuerpo vía `comandos_core::json::response_dumps` (separadores y `ensure_ascii` de `json.dumps`, orden de inserción). Los textos de error son los del Python, literalmente.
2. **SQLite solo a través de `comandos_server::blocking::BlockingWorker`** (generalizado aquí como `BackendWorker`): un worker por archivo de base, con la misma resolución de ruta que `lib/app_state.py`; se abre con las migraciones del store. Si al abrir (o antes de cualquier trabajo) la base tiene un esquema más nuevo que el que conocen las migraciones Rust, **todo** el conjunto nativo se desactiva y se reenvía, con una sola línea en stderr: se falla cerrado hacia el Python; nunca se cae el proceso ni se baja de versión.
3. **El long-poll nunca bloquea el worker**: un bucle async manda `revision()` cada 200 ms hasta que cambia o vence `wait` (acotado 0–25 s, por defecto 25); `handler_timeout` (120 s) lo permite.
4. **tmux por `tokio::process::Command`** con el mismo plazo que el Python (5 s), respetando `TMUX_TMPDIR`; se reutilizan ayudantes de `comandos-runtime` donde existen (no existe un ejecutor de tmux: se crea `native/tmux.rs`).
5. **Enrutador**: la clase `Native` se comprueba ANTES que `Forward`; coincidencia exacta sobre la ruta (`/notices` ≠ `/notices/`), por método; todo lo no nativo conserva el comportamiento de la Fase 2a. `--no-native` (y `COMANDOS_DASH_NATIVE=0`) reenvía todo, para A/B.
6. **Paridad**: `xtask/parity/frente.jsonl` crece con cada ruta nativa (GET y POST; punteros `volatile` para marcas de tiempo, revisiones e ids); el arnés netns compara Python A contra el frente Rust B sobre copias idénticas. No hace falta `--sequence`: el arnés ya ejecuta los casos en el orden del fixture contra ambos lados (ver Decisiones). Cada tarea añade sus líneas.
7. **Documento de cutover**: sección nueva «2b: dominios nativos» en `docs/verification/cutover-dash.md` con stage/restart/rollback y verificación por dominio (qué rutas dejan de aparecer en el tráfico del heredado; `xtask poll --shadow` 10 min; Pss plano).
8. Comentarios en español, identificadores en inglés, sin `unsafe`, clippy `-D warnings`, rustfmt, trailer `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`, `CARGO_TARGET_DIR=<checkout>/.build/target`, `nice -n 10 cargo … -j 6`, nunca `git add -A`, `git add -f` para archivos bajo `docs/superpowers`.
9. **Orden de tareas, cada una desplegable por sí sola**: infraestructura primero (clase Native + worker + puerta de esquema + `--no-native`), luego D, B, A, C, E y por último documentación/cutover. Cada tarea de dominio termina con sus líneas de fixture y sus pruebas en verde.

Reglas de oro heredadas del spec (valen para todo el plan): las pruebas nunca tocan el servidor tmux del usuario, `~/.claude/hooks`, los puertos 4777/4778/4781, `~/.local/share/comandos` ni el systemd de usuario; los cutovers los ejecuta el controlador, no el implementador; el Python no se modifica; **cero Python o bash nuevos** (ni archivos, ni scripts de prueba: los dobles de `tmux`/`fc-list` en las pruebas son binarios del sistema con argumentos, ver Tarea 2).

## Global Constraints

- Todo se compila y prueba con:
  `CARGO_TARGET_DIR=/home/someguy/codebase/0xJesus/ComandOS/.build/target nice -n 10 cargo <cmd> -j 6`
  (abreviado abajo como `$C <cmd>`). Antes de cada commit: `$C fmt --all -- --check`, `$C clippy --workspace --all-targets -j 6 -- -D warnings`, y las pruebas del paquete tocado.
- Commits con `git add <rutas>` explícitas; mensajes `feat(dash): …` / `test(dash): …` / `docs(verification): …` en español, terminados con una línea en blanco y `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`. Este plan se versiona con `git add -f docs/superpowers/plans/2026-10-04-fase-2b-dominios-nativos-i.md`.
- Ninguna prueba abre `~/.local/state/comandos/app-state.sqlite3` real: toda prueba fija `HOME` temporal, quita `COMANDOS_STATE_DB` y `COMANDOS_USAGE_DB` y fija `XDG_STATE_HOME=<home>/.local/state` en todo proceso que lance (Python oráculo incluido). La Tarea 1 corrige el hueco existente en `xtask/src/parity.rs` y en `tests/dash_forward_python.rs`.
- tmux en pruebas: siempre `TMUX_TMPDIR=<dir temporal>` y sin `TMUX`; si `tmux` no está instalado la prueba se salta con un aviso (no `#[ignore]`), como hace la 2a con `python3`.
- El runtime del frente es `current_thread`: nada que pueda bloquear más de unos milisegundos corre en él. SQLite → worker; escrituras de archivos con `fsync` → `tokio::task::spawn_blocking`; procesos → `tokio::process`. Lecturas de JSON pequeños de `~/.claude/hooks` se hacen en línea (igual que el Python, son de kilobytes).
- `Decline` solo se devuelve **antes** de cualquier efecto (escritura en base, archivo o tmux). Un manejador que ya escribió nunca declina: responde lo que respondería el Python o `HandlerError::Failure`.
- Un `500` del Python por excepción no capturada se reproduce con `HandlerError::Failure` (500 `{"error": "Error interno del tablero"}` + `Connection: close`, idéntico por construcción del transporte); un `subprocess.TimeoutExpired` no capturado con `HandlerError::Timeout` (504 `Tiempo de espera agotado`, `bin/cc-dash:8164`).

## Decisiones del plan (no fijadas por el controlador)

- **D. `POST /prefs` no existe en el Python**: la escritura de preferencias es `POST /prefs-set` (`bin/cc-dash:9100`, `update_prefs` 7768), que es lo que llaman cc-app, cc-notifyd y el tablero. Se porta `POST /prefs-set`.
- **C. Workspace son 7 rutas, no 8**: GET `/workspace`, GET `/workspace/close-group`, GET `/workspace/client`, POST `/workspace`, POST `/workspace/sort`, POST `/workspace/client`, POST `/workspace/close-group`. `comandos-runtime` no tiene ni ejecutor de tmux ni `close_app_tab`, así que **POST `/workspace/close-group` se queda reenviado** (es la única). POST `/workspace/sort` es nativo solo con `restore` (lista de cadenas); el modo `by` necesita `read_states_cached` (la ruta `/state`, no portada) → `Decline`.
- **E. Son 37 rutas, no 39**: GET y POST `/events/v2` están en la lista de §1.12 pero el dominio B las monta nativas. Las 37 restantes responden 410 con el cuerpo exacto de `OPERATOR_RETIRED` (`bin/cc-dash:5751`), por el ruling del usuario (sustituye el `{"error":"Ruta retirada"}` que anunciaba `cutover-dash.md`). `dash/sw.js` solo precachea `["/", "/manifest.webmanifest"]` y tiene `/events` en su lista de bypass «live» → `/events` también pasa a 410. `/operator` se queda reenviado (ya responde 410 en el Python). Cambio de comportamiento aceptado: POST `/harness/switch` y `/model/switch-cancel` sin `session` hoy dan 400 «Nombre de sesion invalido»; pasan a 410.
- **Coincidencia de ruta sin decodificar**: se compara el `target` crudo (sin decodificar `%XX`). Una ruta con `%` nunca es nativa y la contesta el Python. Tres llaves reproducen los tres estilos del Python: `Path` (`urlsplit(path).path == p` o `startswith(p)`: se reclama solo la ruta exacta, con consulta opcional), `ExactOrQuery` (`self.path == p or startswith(p + "?")`, solo `/tmux-mouse`) y `Raw` (`self.path == p`, todos los POST y GET `/proxy`). Si `request_target_parts` (el `urlsplit` portado) no ve la misma ruta que el corte crudo, se declina.
- **Mecanismo `Decline`**: `enum Outcome { Reply(Reply), Decline }`; el frente reenvía la petición original intacta. Se usa para dígitos no ASCII, enteros fuera de `i64`, flotantes donde haría falta el `repr` del Python, `kind`/`label`/`session` con forma de objeto, U+FFFD en la consulta (el `parse_qs(errors="replace")` del Python), errores de decodificación JSON del store, layouts de snapshot exóticos y esquema nuevo.
- **Puerta de esquema**: la versión es `MAX(version) FROM schema_migrations` en ambos lados (`PRAGMA user_version` no se usa). La puerta se reevalúa antes de **cada** trabajo (más estricta que el ruling: también detecta que el Python migre con el frente ya vivo). La primera negativa apaga todo el conjunto nativo hasta reiniciar, con una línea en stderr.
- **`BlockingWorker` se generaliza** en `BackendWorker<B>` + `BackendCaller<B>::call(FnOnce(&mut B) -> T)`; `BlockingWorker` pasa a ser un envoltorio sin cambiar su API ni sus pruebas.
- **Dependencias nuevas** en `comandos-server`: feature `process` de tokio, `sha2` y `getrandom` (ambos ya en el workspace). En `xtask`: `rusqlite` (workspace) para `--state-db`.
- **Arnés**: guarda de entorno (Tarea 1), `--state-db <ruta>` (copia de solo lectura vía backup de SQLite a los dos HOME), sesiones tmux privadas idénticas en los dos `TMUX_TMPDIR`, `--no-native` pasado al frente, campo de fixture `oracle_path` (el oráculo responde la ruta retirada con `/operator`) y resumen de rutas reenviadas al terminar (traza `COMANDOS_DASH_TRACE_FORWARD=1`).
- **GET con efectos laterales** (inventario §9.7): GET `/workspace` y GET `/workspace/close-group` hacen `workspace_sync()`, que puede **confirmar una revisión**; el nativo lo reproduce con el mismo `requestId` determinista (`sync-<rev>-<sha256[:24]>`), así que Rust y Python sincronizando a la vez no duplican revisiones (el store deduplica por `requestId`). GET `/events/v2` importa una sola vez `events.jsonl` heredado; el nativo usa el mismo `import_legacy` con la misma marca en `workspace_meta`. GET `/notices` y `/notices/watch` no escriben. Las demás GET nativas son de solo lectura.

## Mapa de rutas nativas

Columnas: método y ruta · llave · línea del Python · claves de la respuesta en orden · errores · tarea.

| Ruta | Llave | Python | Respuesta 200 | Errores | T |
|---|---|---|---|---|---|
| GET `/prefs` | Path | 8604 `read_prefs` 7754, `installed_terminal_fonts` 7745 | claves de `prefs.json` en su orden, luego las de `PREFS_DEFAULTS` que falten, luego `fonts` `[{family,label,a11y}]` | — | 3 |
| POST `/prefs-set` | Raw | 9100 `update_prefs` 7768 | `{"ok": true, "favorites": …}` | 400 `Favorito inválido`, `Máximo de 200 favoritos alcanzado`, `cannot convert float NaN to integer`; 503 `No se pudieron guardar las preferencias`; ∞ → 500 | 3 |
| GET `/tabs` | Path | 8673 | `[{"session":"local","label":"⌂ local","closable":false}?, {session,label}…]` | tmux colgado → 504 | 3 |
| GET `/tab-history` | Path | 8684 `read_tab_history` 5318 | `[{session,label,cwd,agent,ts,reason,alive}…]` (≤40) | `ts` no convertible → 500 | 3 |
| GET `/tab-models` | Path | 8415 | `{"session","panes"}` | entrada no-dict verdadera → 500 | 3 |
| GET `/active-tab` | Path | 8427 | objeto de `app-tab-active.json` (+`pane`) | no-objeto → 500; tmux → 504/500 | 3 |
| GET `/tmux-mouse` | ExactOrQuery | 8604 `get_tmux_mouse` 5883 | `{"mouse":"on"\|"off"}` | 400 `Nombre de sesion invalido`; 404 `No hay sesion tmux '<s>'`; 500 stderr/`tmux fallo`/excepción | 3 |
| POST `/tmux-mouse` | Raw | 9552–9562 `set_tmux_mouse` 5905 | `{"ok": true, "mouse": …}` | 400/404/500 como arriba; `session` no-cadena → 500 | 3 |
| GET `/work-marks` | Path | 8712 | `{"marks","panes","activity"}` | — | 4 |
| POST `/work-marks` | Raw | 8867 | `{"mark": …}` | 400 del store; 409 `{"error":"Revisión desactualizada","current":…}` | 4 |
| GET `/events/v2` | Path | 8714 | `{"events","nextAfter","latest"}` (+`turns`) | 400 `{after\|limit} inválido` | 4 |
| POST `/events/v2` | Raw | 8870 | `{"event": …}` 200 / `{"ignored": true}` 202 | 403 `Solo productores internos de este equipo`; 400 | 4 |
| GET `/notices` | Path | 8639 | `{"notices","nextAfter","pending","prefs","focusActive","badge"}` | 400 `invalid literal for int() with base 10: '<x>'` | 5 |
| GET `/notices/prefs` | Path | 8639 | preferencias de avisos | — | 5 |
| GET `/notices/watch` | Path | 8616 | `{"rev","badge","unread","pending","latest"}` | — (long-poll ≤25 s) | 5 |
| GET `/notifs/count` | Path | 8506 | `{"count": n}` (excepción → 0) | — | 5 |
| POST `/presence` | Raw | 8818 | `{"ok": true}` | 400 `deviceId inválido` | 5 |
| POST `/notices/read` | Raw | 8818 | `{"ok": true, "read": […]}` | 400 `eventIds inválido` y los del store | 5 |
| POST `/notices/sound` | Raw | 8818 | `{"play": false, "reason": …}` / `{"play": true, "cue": …}` | 400 del store | 5 |
| POST `/notices/prefs` | Raw | 8818 | preferencias fusionadas | 400 del store | 5 |
| GET `/workspace` | Path | 8614 `workspace_payload(workspace_sync())` 6533/6566 | documento + `revision` + `ready` | 500 si `pane_bindings` falla | 6 |
| GET `/workspace/close-group` | Path | 8657 | `{"groupId","members","revision"}` | 404 mensaje del core | 6 |
| GET `/workspace/client` | Path | 8666 | estado del dispositivo o `{}` | — | 6 |
| POST `/workspace` | Raw | 8792 | documento + `revision` + `ready` | 400 `expectedRevision inválido`, `La distribución no coincide con las tabs abiertas`, mensajes del core; 409 `{"error","current"}` | 6 |
| POST `/workspace/sort` (`restore`) | Raw | 8789 `workspace_sort` 6497 | documento + `revision` + `ready` + `previous` | 400 mensaje o `Orden inválido`; 409 `El acomodo cambió mientras ordenaba; intenta de nuevo` | 6 |
| POST `/workspace/client` | Raw | 8878 | estado guardado | 400 `deviceId inválido`/`Estado de cliente inválido` | 6 |
| 14 GET + 23 POST retiradas | Path / Raw | §1.12 | 410 `{"error": "El chat de CommandOS se retiró; usa la barra de comandos", "code": "retired"}` | — | 7 |

Siguen reenviadas en 2b: POST `/workspace/close-group`, `/operator*`, `/notify-popup`, `/test`, push, `/state` y todo lo demás.

## Review Focus

Cinco clases de entrada que las pruebas existentes no cubren; cada una tiene su prueba en la tarea que la posee:

1. **Rust y Python sincronizan el workspace a la vez** (el frente nativo y un cc-app que todavía habla con el heredado en 4781): no debe haber revisiones duplicadas ni conflictos visibles. Prueba `workspace_sync_interop_no_duplicate_revision` (Tarea 6): GET `/workspace` al frente nativo y al Python sobre el mismo HOME/base dan la misma `revision` y la segunda no crea revisión nueva.
2. **El esquema se vuelve más nuevo con el frente vivo** (se instala un Python que migra a la v12): la siguiente ruta nativa se reenvía, sale una sola línea en stderr, nunca se baja de versión y no se vuelve a abrir. Prueba `schema_newer_while_live_forwards_once` (Tarea 1) y `notices_forwarded_after_schema_bump` (Tarea 5).
3. **Muchos long-polls y clientes que cierran**: 50 `/notices/watch` abiertos no ahogan el worker; `/notifs/count` sigue respondiendo en < 1 s y un `/presence` despierta a todos. Prueba `watch_storm_keeps_worker_responsive` (Tarea 5).
4. **tmux ausente o colgado**: ausente → los mismos 500 con el mensaje exacto del Python (`[Errno 2] No such file or directory: 'tmux'`) o 500 genérico donde el Python no lo captura; colgado → 504 donde el Python no captura el `TimeoutExpired`, y `/notifs/count`/`/notices` siguen respondiendo (su `is_live` es `None`). Pruebas `tmux_missing_messages` y `tmux_hung_times_out` (Tarea 3) y `notices_survive_hung_tmux` (Tarea 5).
5. **Entradas exóticas declinan al Python**: dígitos árabes en `after`, enteros de 30 cifras, `kind` objeto, `label` flotante, U+FFFD en la consulta, snapshot con índice booleano. Prueba `exotic_inputs_decline_to_legacy` en cada tarea de dominio (3, 5, 6) contra un heredado falso que responde `{"legacy": true}`.

## Estructura de archivos

```
crates/comandos-server/
  Cargo.toml                      (tokio +process, sha2, getrandom)
  src/lib.rs                      (Request: Clone)
  src/blocking.rs                 (BackendWorker<B>, BackendCaller<B>; BlockingWorker como envoltorio)
  src/dash/mod.rs                 (DashConfig.native/state_db/trace_forward, build, serve_with, Native en DashState)
  src/dash/router.rs              (RouteClass::Native, classify_with)
  src/dash/native/mod.rs          (Native, NativeOptions, NativeRoute, Key/Entry, route(), Outcome/Fault)
  src/dash/native/state.rs        (StateBackend, Refusal, puerta de esquema)
  src/dash/native/py.rs           (semántica Python: strip, splitlines, int, float, repr, str de escalares)
  src/dash/native/tmux.rs         (Program, Tmux, Output, TmuxError)
  src/dash/native/files.rs        (read_json, write_json_atomic)
  src/dash/native/query.rs        (consulta: urlsplit + parse_qs, primera ocurrencia)
  src/dash/native/light.rs        (D)
  src/dash/native/events.rs       (B)
  src/dash/native/notices.rs      (A)
  src/dash/native/workspace.rs    (C)
  src/dash/native/retired.rs      (E)
  tests/support/mod.rs            (request_body, FakeLegacy, TestHome, front)
  tests/support/oracle.rs         (Python oráculo con HOME compartido)
  tests/dash_native_infra.rs, dash_native_kit.rs, dash_native_light.rs, dash_native_events.rs,
  tests/dash_native_notices.rs, dash_native_workspace.rs, dash_native_retired.rs
crates/comandos-core/src/workspace/snapshot.rs   (valid_snapshot del Python)
crates/comandos-core/tests/workspace_snapshot.rs
crates/comandos-store/src/notifications.rs       (recent → pub)
xtask/Cargo.toml, xtask/src/parity.rs, xtask/src/poll.rs, xtask/parity/frente.jsonl
docs/verification/cutover-dash.md                (sección «2b: dominios nativos»)
```

---

### Task 1: Infraestructura nativa (worker genérico, puerta de esquema, clase `Native`, `--no-native`)

Desplegable sola: tras esta tarea no hay ninguna ruta nativa (la tabla está vacía) y el frente se comporta como la 2a, pero ya abre la base al arrancar, aplica la puerta de esquema y acepta `--no-native`/`COMANDOS_DASH_NATIVE=0`/`COMANDOS_DASH_TRACE_FORWARD=1`.

**Files:**
- Modify: `crates/comandos-server/Cargo.toml`
- Modify: `crates/comandos-server/src/lib.rs` (derive `Clone` en `Request`)
- Modify: `crates/comandos-server/src/blocking.rs`
- Create: `crates/comandos-server/src/dash/native/mod.rs`, `crates/comandos-server/src/dash/native/state.rs`
- Modify: `crates/comandos-server/src/dash/mod.rs`, `crates/comandos-server/src/dash/router.rs`
- Modify: `crates/comandos-server/tests/blocking.rs`, `crates/comandos-server/tests/dash_forward_python.rs`
- Create: `crates/comandos-server/tests/dash_native_infra.rs`
- Modify: `xtask/src/parity.rs` (guarda de entorno)

**Interfaces:**
- Produces: `BackendWorker<B>::{start(capacity, backend), caller(), shutdown()}`, `BackendCaller<B>::call(FnOnce(&mut B) -> T) -> Result<T, HandlerError>`; `dash::native::{Native, NativeOptions, NativeRoute, Verb, Key, Entry, Outcome, Fault, Answer, Clock, route, wall_clock_ms, WORKER_CAPACITY}`; `Native::{new, options, enabled, refusals, ready, with_state, dispatch, shutdown}`; `dash::native::state::{StateBackend, Refusal, known_versions}`; `router::{RouteClass::Native, classify_with}`; `DashConfig.{native, state_db, trace_forward}`; `dash::{build, serve_with, trace_line, NATIVE_ENV, TRACE_ENV}`.
- Consumes: `comandos_store::state::{connect, migrate, MIGRATIONS}`, `comandos_runtime::{state_path, now_ms}`.

- [ ] **Step 1: Escribir las pruebas que fallan del worker genérico**

Añadir al final de `crates/comandos-server/tests/blocking.rs`:

```rust
use comandos_server::blocking::BackendWorker;

#[tokio::test]
async fn backend_worker_keeps_state_between_calls() {
    let worker = BackendWorker::start(4, 0u64).unwrap();
    let caller = worker.caller();
    for expected in 1..=3u64 {
        let got = caller
            .call(|n: &mut u64| {
                *n += 1;
                *n
            })
            .await
            .unwrap();
        assert_eq!(got, expected);
    }
    // Un clon del llamador habla con el mismo backend.
    let other = caller.clone();
    assert_eq!(other.call(|n: &mut u64| *n).await.unwrap(), 3);
    worker.shutdown().await.unwrap();
    assert!(caller.call(|n: &mut u64| *n).await.is_err());
}

#[tokio::test]
async fn backend_worker_retires_after_panic() {
    let worker = BackendWorker::start(4, Vec::<u8>::new()).unwrap();
    let caller = worker.caller();
    let boom = caller
        .call(|_: &mut Vec<u8>| -> u8 { panic!("estado inconsistente") })
        .await;
    assert!(matches!(boom, Err(comandos_server::HandlerError::Failure)));
    // Tras un pánico el backend no se reutiliza.
    assert!(caller.call(|v: &mut Vec<u8>| v.len()).await.is_err());
    worker.shutdown().await.unwrap();
}
```

- [ ] **Step 2: Ejecutar y ver el fallo**

Run: `$C test -p comandos-server --test blocking -j 6`
Expected: FAIL de compilación, `no BackendWorker in blocking`.

- [ ] **Step 3: Generalizar `blocking.rs`**

Reemplazar el contenido de `crates/comandos-server/src/blocking.rs` por:

```rust
//! Serial ownership of a synchronous backend, separate from the network runtime.
//! `BackendWorker<B>` owns `B` on one thread; `BackendCaller<B>` submits closures.
//! `BlockingWorker` keeps the request-shaped API of the previous phase.
use crate::{Handler, HandlerError, Reply, Request};
use std::{
    io,
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, oneshot};

/// One queued closure over the backend. `abandoned` lets the thread skip work
/// whose caller already gave up; `run` reports whether the closure panicked.
trait Task<B>: Send {
    fn abandoned(&self) -> bool;
    fn run(self: Box<Self>, backend: &mut B) -> bool;
}

struct Call<B, T, F> {
    job: F,
    reply: oneshot::Sender<Result<T, HandlerError>>,
    // fn(&mut B) keeps Call Send/Sync independent of B, which never crosses threads here.
    _backend: PhantomData<fn(&mut B)>,
}

impl<B, T, F> Task<B> for Call<B, T, F>
where
    F: FnOnce(&mut B) -> T + Send,
    T: Send,
{
    fn abandoned(&self) -> bool {
        self.reply.is_closed()
    }
    fn run(self: Box<Self>, backend: &mut B) -> bool {
        let Call { job, reply, .. } = *self;
        let result = catch_unwind(AssertUnwindSafe(|| job(backend)));
        let panicked = result.is_err();
        let _ = reply.send(result.map_err(|_| HandlerError::Failure));
        panicked
    }
}

struct Queued<B> {
    task: Box<dyn Task<B>>,
    permit: OwnedSemaphorePermit,
}

struct Shared<B> {
    // The sole sender is private. No clone can outlive this short critical
    // section, so taking it reliably wakes an idle receiver on shutdown.
    sender: Mutex<Option<mpsc::Sender<Queued<B>>>>,
    slots: Arc<Semaphore>,
    stopping: AtomicBool,
}

impl<B> Shared<B> {
    fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
        self.slots.close();
        self.sender.lock().unwrap_or_else(|p| p.into_inner()).take();
    }
}

/// Owner of one synchronous backend. Call `shutdown` before runtime teardown.
/// Drop also stops and joins, but blocks the dropping thread. Closures must
/// finish in bounded time and must not depend on that thread making progress.
/// Running work is never forcibly aborted; it may commit after its reply is lost.
pub struct BackendWorker<B> {
    shared: Arc<Shared<B>>,
    done: Option<oneshot::Receiver<()>>,
    thread: Option<JoinHandle<()>>,
}

impl<B: Send + 'static> BackendWorker<B> {
    /// Queue capacity excludes the currently executing job. Transport limits
    /// separately bound callers waiting for admission and their input bytes.
    pub fn start(capacity: usize, backend: B) -> io::Result<Self> {
        if !(1..=65536).contains(&capacity) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid worker capacity",
            ));
        }
        let (sender, receiver) = mpsc::channel::<Queued<B>>();
        let (finished, done) = oneshot::channel();
        let shared = Arc::new(Shared {
            sender: Mutex::new(Some(sender)),
            slots: Arc::new(Semaphore::new(capacity)),
            stopping: AtomicBool::new(false),
        });
        let state = shared.clone();
        let thread = thread::Builder::new()
            .name("comandos-handler".into())
            .spawn(move || {
                let mut backend = backend;
                while let Ok(Queued { task, permit }) = receiver.recv() {
                    drop(permit);
                    if state.stopping.load(Ordering::Acquire) {
                        break;
                    }
                    if task.abandoned() {
                        continue;
                    }
                    // Mutable backend state may be inconsistent after unwinding.
                    // Retire the worker rather than reusing it after a panic.
                    if task.run(&mut backend) {
                        break;
                    }
                }
                state.stop();
                drop(receiver);
                drop(backend);
                let _ = finished.send(());
            })?;
        Ok(Self {
            shared,
            done: Some(done),
            thread: Some(thread),
        })
    }

    pub fn caller(&self) -> BackendCaller<B> {
        BackendCaller {
            shared: self.shared.clone(),
        }
    }

    /// Stop admission, skip queued work and await the active operation and join.
    /// Canceling this future uses the synchronous Drop fallback.
    pub async fn shutdown(mut self) -> io::Result<()> {
        self.shared.stop();
        let completed = self.done.take().expect("worker completion receiver").await;
        let joined = self.thread.take().expect("worker thread").join();
        if completed.is_err() || joined.is_err() {
            Err(io::Error::other("blocking worker terminated unexpectedly"))
        } else {
            Ok(())
        }
    }
}

impl<B> Drop for BackendWorker<B> {
    fn drop(&mut self) {
        self.shared.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Cloneable submitter. Waiting for a queue slot is cancel-safe; once queued,
/// a dropped caller future only marks the job abandoned.
pub struct BackendCaller<B> {
    shared: Arc<Shared<B>>,
}

impl<B> Clone for BackendCaller<B> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}

impl<B: 'static> BackendCaller<B> {
    pub async fn call<T, F>(&self, job: F) -> Result<T, HandlerError>
    where
        F: FnOnce(&mut B) -> T + Send + 'static,
        T: Send + 'static,
    {
        let permit = self
            .shared
            .slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| HandlerError::Failure)?;
        let (reply, result) = oneshot::channel();
        {
            let sender = self
                .shared
                .sender
                .lock()
                .map_err(|_| HandlerError::Failure)?;
            if self.shared.stopping.load(Ordering::Acquire) {
                return Err(HandlerError::Failure);
            }
            let task: Box<dyn Task<B>> = Box::new(Call {
                job,
                reply,
                _backend: PhantomData,
            });
            sender
                .as_ref()
                .ok_or(HandlerError::Failure)?
                .send(Queued { task, permit })
                .map_err(|_| HandlerError::Failure)?;
        }
        result.await.unwrap_or(Err(HandlerError::Failure))
    }
}

type Work = Box<dyn FnMut(Request) -> Result<Reply, HandlerError> + Send>;

/// Request-shaped worker of the previous phase, now a thin `BackendWorker`.
pub struct BlockingWorker {
    inner: BackendWorker<Work>,
}

impl BlockingWorker {
    pub fn start<F>(capacity: usize, work: F) -> io::Result<Self>
    where
        F: FnMut(Request) -> Result<Reply, HandlerError> + Send + 'static,
    {
        Ok(Self {
            inner: BackendWorker::start(capacity, Box::new(work) as Work)?,
        })
    }

    pub fn handler(&self) -> Handler {
        let caller = self.inner.caller();
        Arc::new(move |request| {
            let caller = caller.clone();
            Box::pin(async move { caller.call(move |work: &mut Work| work(request)).await? })
        })
    }

    pub async fn shutdown(self) -> io::Result<()> {
        self.inner.shutdown().await
    }
}
```

En `crates/comandos-server/src/lib.rs` cambiar `pub struct Request {` por `#[derive(Clone)]\npub struct Request {` (todos sus campos son `Clone`; `Bytes` se clona sin copiar).

- [ ] **Step 4: Ver pasar el worker (y las pruebas viejas sin tocar)**

Run: `$C test -p comandos-server --test blocking -j 6`
Expected: PASS, incluidas las 7 pruebas previas de `BlockingWorker` (capacidad inválida, serie, contrapresión, cancelación, apagado, retiro tras pánico, drop que une).

- [ ] **Step 5: Escribir las pruebas que fallan de la infraestructura nativa**

Crear `crates/comandos-server/tests/dash_native_infra.rs`:

```rust
//! Infraestructura de la Fase 2b: flag `--no-native`, ruta de la base,
//! puerta de esquema y clase `Native` del enrutador.
use comandos_server::dash::{
    DEFAULT_LEGACY_PORT,
    native::{Native, NativeOptions, state::{Refusal, StateBackend}},
    parse_args,
    router::{classify, classify_with},
    trace_line,
};
use http::Method;
use std::{fs, path::PathBuf, sync::Arc};

fn root(tag: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("cmd-native-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    base
}

#[test]
fn native_is_on_by_default_and_off_with_flag() {
    let home = PathBuf::from("/tmp/no-existe-home");
    let cfg = parse_args(&[], &home, None).unwrap();
    assert!(cfg.native);
    assert!(!cfg.trace_forward);
    assert_eq!(
        cfg.state_db,
        home.join(".local/state/comandos/app-state.sqlite3")
    );
    let cfg = parse_args(&["--no-native".into()], &home, None).unwrap();
    assert!(!cfg.native);
    assert_eq!(cfg.port, 4777);
    assert_eq!(cfg.legacy_port, DEFAULT_LEGACY_PORT);
}

#[test]
fn classify_with_native_off_is_phase_2a() {
    let exists = |p: &str| p == "/index.html";
    for (method, target) in [
        (Method::GET, "/"),
        (Method::GET, "/prefs"),
        (Method::GET, "/notices?after=3"),
        (Method::POST, "/presence"),
        (Method::HEAD, "/index.html"),
        (Method::GET, "/no-existe"),
    ] {
        assert_eq!(
            classify_with(&method, target, &exists, false),
            classify(&method, target, &exists),
            "{method} {target}"
        );
    }
}

#[test]
fn trace_line_omits_query() {
    assert_eq!(
        trace_line(&Method::GET, "/notices?deviceId=secreto"),
        "comandos dash: reenvío GET /notices"
    );
}

fn bump_schema(db: &std::path::Path, version: i64) {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.execute(
        "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?, 'futuro', 0)",
        [version],
    )
    .unwrap();
}

#[test]
fn state_backend_migrates_then_refuses_newer_schema() {
    let base = root("gate");
    let db = base.join("state/app-state.sqlite3");
    let backend = StateBackend::open(&db, 1_791_115_200.0).unwrap();
    backend.admit().unwrap();
    let known = *comandos_server::dash::native::state::known_versions()
        .iter()
        .max()
        .unwrap();
    bump_schema(&db, known + 1);
    assert_eq!(
        backend.admit(),
        Err(Refusal::Newer {
            found: known + 1,
            known
        })
    );
    drop(backend);
    assert!(matches!(
        StateBackend::open(&db, 1_791_115_200.0),
        Err(Refusal::Newer { .. })
    ));
    let _ = fs::remove_dir_all(&base);
}

fn options(base: &std::path::Path) -> NativeOptions {
    let mut opts = NativeOptions::for_home(base, base.join("state/app-state.sqlite3"));
    opts.clock = Arc::new(|| 1_791_115_200_000);
    opts
}

#[tokio::test]
async fn schema_newer_while_live_forwards_once() {
    let base = root("live");
    let native = Native::new(options(&base));
    assert!(native.ready().await);
    let known = *comandos_server::dash::native::state::known_versions()
        .iter()
        .max()
        .unwrap();
    bump_schema(&base.join("state/app-state.sqlite3"), known + 1);
    // El primer trabajo descubre el esquema nuevo y apaga todo el conjunto.
    assert!(native.with_state(|_| ()).await.is_err());
    assert!(!native.enabled());
    assert!(native.with_state(|_| ()).await.is_err());
    assert!(!native.ready().await);
    assert_eq!(native.refusals(), 1, "una sola línea en stderr");
    native.shutdown().await;
    let _ = fs::remove_dir_all(&base);
}

#[tokio::test]
async fn unopenable_database_disables_native() {
    let base = root("unopen");
    // Un directorio donde va el archivo: no se puede abrir como base.
    fs::create_dir_all(base.join("state/app-state.sqlite3")).unwrap();
    let native = Native::new(options(&base));
    assert!(!native.ready().await);
    assert!(!native.enabled());
    assert_eq!(native.refusals(), 1);
    let _ = fs::remove_dir_all(&base);
}
```

(Las pruebas de integración ya ven `rusqlite`, `http` y `tokio` porque son dependencias normales del paquete.) En `[dependencies]` de `crates/comandos-server/Cargo.toml`:

```toml
tokio = { version = "=1.53.1", default-features = false, features = ["rt", "macros", "net", "sync", "time", "io-util", "signal", "process"] }
sha2.workspace = true
getrandom.workspace = true
```

- [ ] **Step 6: Ejecutar y ver el fallo**

Run: `$C test -p comandos-server --test dash_native_infra -j 6`
Expected: FAIL de compilación: `no native in dash`, `no field native on DashConfig`, `classify_with`/`trace_line` inexistentes.

- [ ] **Step 7: Implementar `native/state.rs`**

Crear `crates/comandos-server/src/dash/native/state.rs`:

```rust
//! Una conexión a `app-state.sqlite3` para todas las rutas nativas, con la
//! puerta de esquema: si el Python migró a una versión que este binario no
//! conoce, se rechaza (y el frente reenvía todo al heredado).
use comandos_store::state::{self, MIGRATIONS};
use rusqlite::Connection;
use std::{collections::BTreeSet, path::Path, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// La base tiene migraciones aplicadas que este binario no conoce.
    Newer { found: i64, known: i64 },
    /// No se pudo abrir, consultar o migrar.
    Unopened(String),
}

impl Refusal {
    pub fn message(&self, path: &Path) -> String {
        match self {
            Refusal::Newer { found, known } => format!(
                "comandos dash: {} tiene esquema {found} y este binario conoce hasta {known}; \
                 rutas nativas desactivadas, todo se reenvía al heredado",
                path.display()
            ),
            Refusal::Unopened(error) => format!(
                "comandos dash: no se pudo abrir {}: {error}; \
                 rutas nativas desactivadas, todo se reenvía al heredado",
                path.display()
            ),
        }
    }
}

pub fn known_versions() -> BTreeSet<i64> {
    MIGRATIONS.iter().map(|m| m.version).collect()
}

pub struct StateBackend {
    pub conn: Connection,
}

impl StateBackend {
    /// Como `open_state` del runtime (3 intentos ante `SQLITE_BUSY`), pero la
    /// puerta va ANTES de migrar: una base más nueva nunca se toca.
    pub fn open(path: &Path, now_seconds: f64) -> Result<Self, Refusal> {
        let mut last = String::new();
        for attempt in 0..3u64 {
            let conn = state::connect(path).map_err(|e| Refusal::Unopened(e.to_string()))?;
            let backend = Self { conn };
            backend.admit()?;
            match state::migrate(&backend.conn, MIGRATIONS, now_seconds) {
                Ok(_) => return Ok(backend),
                Err(comandos_store::Error::Sql(error)) if attempt < 2 => {
                    last = error.to_string();
                    drop(backend);
                    std::thread::sleep(Duration::from_millis(50 * (attempt + 1)));
                }
                Err(error) => return Err(Refusal::Unopened(error.to_string())),
            }
        }
        Err(Refusal::Unopened(last))
    }

    /// Se evalúa antes de cada trabajo: cuesta una consulta a una tabla de
    /// una docena de filas.
    pub fn admit(&self) -> Result<(), Refusal> {
        let known = known_versions();
        let found = self
            .conn
            .prepare_cached("SELECT version FROM schema_migrations")
            .and_then(|mut stmt| {
                stmt.query_map([], |r| r.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(|e| Refusal::Unopened(e.to_string()))?
            .into_iter()
            .filter(|v| !known.contains(v))
            .max();
        match found {
            None => Ok(()),
            Some(found) => Err(Refusal::Newer {
                found,
                known: known.last().copied().unwrap_or(0),
            }),
        }
    }
}
```

- [ ] **Step 8: Implementar `native/mod.rs` (tabla vacía)**

Crear `crates/comandos-server/src/dash/native/mod.rs`:

```rust
//! Rutas que el frente responde sin el Python (Fase 2b).
//!
//! Cada ruta reproduce byte a byte una rama de `bin/cc-dash`. Lo que toca la
//! base pasa por un único `BackendWorker<StateBackend>`; lo que toca tmux o
//! `fc-list`, por `tokio::process`. Si una entrada no se puede reproducir con
//! certeza, el manejador devuelve `Fault::Decline` ANTES de cualquier efecto
//! y el frente reenvía la petición original al heredado.
pub mod state;

use crate::{
    HandlerError, Reply, Request,
    blocking::{BackendCaller, BackendWorker},
    dash::router::path_of,
};
use http::Method;
use state::{Refusal, StateBackend};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio::sync::OnceCell;

/// Trabajos en cola del worker de la base (sin contar el que corre).
pub const WORKER_CAPACITY: usize = 64;

/// Cada dominio añade su variante en su tarea.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeRoute {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Get,
    Post,
}

/// Cómo compara el Python la ruta (sin decodificar `%XX`).
#[derive(Debug, Clone, Copy)]
pub enum Key {
    /// `urlsplit(self.path).path == p` o `self.path.startswith(p)`: se
    /// reclama solo la ruta exacta, con consulta opcional.
    Path(&'static str),
    /// `self.path == p or self.path.startswith(p + "?")`.
    ExactOrQuery(&'static str),
    /// `self.path == p`: sin consulta.
    Raw(&'static str),
}

impl Key {
    pub fn matches(self, target: &str) -> bool {
        match self {
            Key::Path(p) => path_of(target) == p,
            Key::ExactOrQuery(p) => {
                target == p || target.strip_prefix(p).is_some_and(|rest| rest.starts_with('?'))
            }
            Key::Raw(p) => target == p,
        }
    }
}

pub struct Entry {
    pub verb: Verb,
    pub key: Key,
    pub route: NativeRoute,
}

/// Una tabla por dominio; las tareas 3–7 añaden la suya.
const TABLES: &[&[Entry]] = &[];

pub fn route(method: &Method, target: &str) -> Option<NativeRoute> {
    let verb = if *method == Method::GET {
        Verb::Get
    } else if *method == Method::POST {
        Verb::Post
    } else {
        return None;
    };
    TABLES
        .iter()
        .flat_map(|table| table.iter())
        .find(|entry| entry.verb == verb && entry.key.matches(target))
        .map(|entry| entry.route)
}

pub enum Outcome {
    Reply(Reply),
    /// Nada se escribió: el frente reenvía la petición original.
    Decline,
}

pub enum Fault {
    Decline,
    Error(HandlerError),
}

impl From<HandlerError> for Fault {
    fn from(error: HandlerError) -> Self {
        Fault::Error(error)
    }
}

pub type Answer = Result<Reply, Fault>;

/// Milisegundos Unix; las pruebas lo sustituyen.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

pub fn wall_clock_ms() -> i64 {
    comandos_runtime::now_ms().map_or(0, |ms| i64::try_from(ms).unwrap_or(i64::MAX))
}

#[derive(Clone)]
pub struct NativeOptions {
    pub state_db: PathBuf,
    /// `~/.claude/hooks` (el `HOOKS` del Python).
    pub hooks: PathBuf,
    pub clock: Clock,
}

impl NativeOptions {
    pub fn for_home(home: &Path, state_db: PathBuf) -> Self {
        Self {
            state_db,
            hooks: home.join(".claude/hooks"),
            clock: Arc::new(wall_clock_ms),
        }
    }
}

pub struct Native {
    opts: NativeOptions,
    enabled: AtomicBool,
    refusals: AtomicUsize,
    state: OnceCell<Option<BackendCaller<StateBackend>>>,
    worker: Mutex<Option<BackendWorker<StateBackend>>>,
}

impl Native {
    pub fn new(opts: NativeOptions) -> Self {
        Self {
            opts,
            enabled: AtomicBool::new(true),
            refusals: AtomicUsize::new(0),
            state: OnceCell::new(),
            worker: Mutex::new(None),
        }
    }

    pub fn options(&self) -> &NativeOptions {
        &self.opts
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Cuántas veces se escribió la línea de desactivación (0 o 1).
    pub fn refusals(&self) -> usize {
        self.refusals.load(Ordering::Acquire)
    }

    fn disable(&self, refusal: &Refusal) {
        if self.enabled.swap(false, Ordering::AcqRel) {
            self.refusals.fetch_add(1, Ordering::AcqRel);
            eprintln!("{}", refusal.message(&self.opts.state_db));
        }
    }

    /// Abre la base una sola vez (en un hilo de bloqueo) y arranca el worker.
    /// Devuelve si el conjunto nativo está activo.
    pub async fn ready(&self) -> bool {
        let caller = self
            .state
            .get_or_init(|| async {
                let path = self.opts.state_db.clone();
                let now_seconds = (self.opts.clock)() as f64 / 1000.0;
                let opened =
                    tokio::task::spawn_blocking(move || StateBackend::open(&path, now_seconds))
                        .await;
                match opened {
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
        caller.is_some() && self.enabled()
    }

    /// Un trabajo sobre la base. `Err(Fault::Decline)` si el conjunto está
    /// apagado o si la puerta de esquema rechaza ahora (y lo apaga).
    pub async fn with_state<T, F>(&self, job: F) -> Result<T, Fault>
    where
        F: FnOnce(&mut StateBackend) -> T + Send + 'static,
        T: Send + 'static,
    {
        if !self.ready().await {
            return Err(Fault::Decline);
        }
        let Some(Some(caller)) = self.state.get() else {
            return Err(Fault::Decline);
        };
        match caller
            .call(move |backend: &mut StateBackend| backend.admit().map(|()| job(backend)))
            .await?
        {
            Ok(value) => Ok(value),
            Err(refusal) => {
                self.disable(&refusal);
                Err(Fault::Decline)
            }
        }
    }

    pub async fn dispatch(
        &self,
        route: NativeRoute,
        request: &Request,
    ) -> Result<Outcome, HandlerError> {
        if !self.ready().await {
            return Ok(Outcome::Decline);
        }
        match self.answer(route, request).await {
            Ok(reply) => Ok(Outcome::Reply(reply)),
            Err(Fault::Decline) => Ok(Outcome::Decline),
            Err(Fault::Error(error)) => Err(error),
        }
    }

    async fn answer(&self, route: NativeRoute, _request: &Request) -> Answer {
        match route {}
    }

    /// Para el worker (si arrancó) y espera a que termine su trabajo en curso.
    pub async fn shutdown(&self) {
        let worker = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(worker) = worker {
            let _ = worker.shutdown().await;
        }
    }
}
```

- [ ] **Step 9: Enrutador con clase `Native`**

En `crates/comandos-server/src/dash/router.rs`:

```rust
use crate::dash::native::{self, NativeRoute};
use http::Method;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteClass {
    /// GET/HEAD de un archivo regular existente: lo sirve Rust desde `dash_dir`.
    Static,
    /// Ruta de un dominio nativo (Fase 2b): la responde Rust salvo `Decline`.
    Native(NativeRoute),
    /// Todo lo demás: se reenvía al Python heredado.
    Forward,
}

/// La clase nativa se comprueba antes que todo lo demás; con `native` en
/// falso (`--no-native`) el resultado es exactamente el de la Fase 2a.
pub fn classify_with(
    method: &Method,
    target: &str,
    asset_exists: &dyn Fn(&str) -> bool,
    native: bool,
) -> RouteClass {
    if native && let Some(route) = native::route(method, target) {
        return RouteClass::Native(route);
    }
    classify(method, target, asset_exists)
}
```

(`classify`, `path_of`, `static_path` y la tabla `DYNAMIC_GET` no cambian.)

- [ ] **Step 10: Configuración, construcción y despacho en `dash/mod.rs`**

Cambios en `crates/comandos-server/src/dash/mod.rs`:

```rust
pub mod native;
// … (los `pub mod` existentes)

pub const NATIVE_ENV: &str = "COMANDOS_DASH_NATIVE";
pub const TRACE_ENV: &str = "COMANDOS_DASH_TRACE_FORWARD";

#[derive(Clone)]
pub struct DashConfig {
    pub port: u16,
    pub legacy_port: u16,
    pub home: PathBuf,
    pub dash_dir: PathBuf,
    pub token: Vec<u8>,
    /// Falso con `--no-native` o `COMANDOS_DASH_NATIVE=0`: todo se reenvía (2a).
    pub native: bool,
    /// `app-state.sqlite3`, resuelto como `lib/app_state.py`.
    pub state_db: PathBuf,
    /// `COMANDOS_DASH_TRACE_FORWARD=1`: una línea en stderr por reenvío.
    pub trace_forward: bool,
}
```

En el `Debug` manual añadir `.field("native", &self.native).field("state_db", &self.state_db).field("trace_forward", &self.trace_forward)`.

En `parse_args`: reconocer `--no-native` en el bucle (`else if word == "--no-native" { native = false; }`, con `let mut native = true;` arriba) y construir:

```rust
    Ok(DashConfig {
        port,
        legacy_port,
        home: home.to_path_buf(),
        dash_dir: default_dash_dir(home),
        token: Vec::new(),
        native,
        state_db: home.join(".local/state/comandos/app-state.sqlite3"),
        trace_forward: false,
    })
```

En `from_env`, tras resolver `dash_dir` y antes del token:

```rust
    if std::env::var(NATIVE_ENV).is_ok_and(|v| v == "0") {
        cfg.native = false;
    }
    cfg.trace_forward = std::env::var(TRACE_ENV).is_ok_and(|v| v == "1");
    let state_override = std::env::var_os("COMANDOS_STATE_DB").map(PathBuf::from);
    let xdg = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from);
    cfg.state_db = comandos_runtime::state_path(
        None,
        state_override.as_deref(),
        xdg.as_deref(),
        Some(&home),
    )
    .map_err(|e| StartError::Config(format!("app-state: {e}")))?;
```

Estado compartido, despacho y construcción:

```rust
pub struct DashState {
    pub config: DashConfig,
    pub asset_exists: AssetExists,
    pub native: Option<Arc<native::Native>>,
}

async fn handle(state: &DashState, request: Request) -> Result<Reply, HandlerError> {
    let live = state.native.as_ref().filter(|n| n.enabled());
    let class = router::classify_with(
        &request.method,
        &request.target,
        &*state.asset_exists,
        live.is_some(),
    );
    match class {
        RouteClass::Native(route) => {
            let Some(native) = live else {
                return forward_to_legacy(state, request).await;
            };
            match native.dispatch(route, &request).await? {
                native::Outcome::Reply(reply) => Ok(reply),
                native::Outcome::Decline => forward_to_legacy(state, request).await,
            }
        }
        RouteClass::Static => statics::serve(&state.config.dash_dir, &request).await,
        // HEAD solo existe para estáticos; a una ruta API es 404 (el Python
        // devolvía 404 HTML vía SimpleHTTPRequestHandler.do_HEAD).
        RouteClass::Forward if request.method == Method::HEAD => not_found(),
        RouteClass::Forward => forward_to_legacy(state, request).await,
    }
}

async fn forward_to_legacy(state: &DashState, request: Request) -> Result<Reply, HandlerError> {
    if state.config.trace_forward {
        eprintln!("{}", trace_line(&request.method, &request.target));
    }
    let legacy = SocketAddr::from((Ipv4Addr::LOCALHOST, state.config.legacy_port));
    forward::relay(legacy, request).await
}

/// Sin consulta: los `deviceId` y tokens nunca llegan a los registros.
pub fn trace_line(method: &Method, target: &str) -> String {
    format!("comandos dash: reenvío {method} {}", router::path_of(target))
}

/// Transporte + conjunto nativo. `opts` sustituye a las opciones derivadas
/// de `cfg` (las pruebas inyectan reloj, tmux y `fc-list`).
pub fn build(
    cfg: DashConfig,
    opts: Option<native::NativeOptions>,
) -> (Config, Option<Arc<native::Native>>) {
    let asset_exists = asset_exists(&cfg.dash_dir);
    let token = cfg.token.clone();
    let native = cfg.native.then(|| {
        Arc::new(native::Native::new(opts.unwrap_or_else(|| {
            native::NativeOptions::for_home(&cfg.home, cfg.state_db.clone())
        })))
    });
    let state = Arc::new(DashState {
        config: cfg,
        asset_exists: asset_exists.clone(),
        native: native.clone(),
    });
    let config = Config {
        token,
        asset_exists,
        handler: handler(state),
        limits: limits(),
    };
    (config, native)
}

/// Configuración del transporte para un `DashConfig` ya resuelto (API de la 2a).
pub fn transport_config(cfg: DashConfig) -> Config {
    build(cfg, None).0
}

pub async fn serve_with(
    listener: TcpListener,
    cfg: DashConfig,
    opts: Option<native::NativeOptions>,
    shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let (config, native) = build(cfg, opts);
    if let Some(native) = &native {
        // Abre la base antes de atender: la primera petición no paga la migración.
        native.ready().await;
    }
    let served = crate::serve(listener, config, shutdown).await;
    if let Some(native) = native {
        native.shutdown().await;
    }
    served
}

pub async fn serve_listener(
    listener: TcpListener,
    cfg: DashConfig,
    shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    serve_with(listener, cfg, None, shutdown).await
}
```

`serve_until_signal` no cambia: ya llama a `serve_listener`, que ahora abre la base antes de atender (el aviso «Centro Claude corriendo…» sigue saliendo antes, como en la 2a: quien lo lee ya puede conectar y la primera petición espera la apertura, de milisegundos).

- [ ] **Step 11: Guardas de entorno en el arnés y en la prueba con oráculo**

En `xtask/src/parity.rs`, función `spawn`, añadir a la cadena de `cmd` (justo después de `.env_remove("WAYLAND_DISPLAY")`):

```rust
        // La base de estado nunca es la real: ni override ni XDG del usuario.
        .env_remove("COMANDOS_STATE_DB")
        .env_remove("COMANDOS_USAGE_DB")
        .env_remove(NATIVE_ENV_OF_FRONT)
        .env("XDG_STATE_HOME", e.home.join(".local/state"))
```

con `const NATIVE_ENV_OF_FRONT: &str = "COMANDOS_DASH_NATIVE";` al principio del módulo (el arnés decide el modo con `--no-native`, nunca el entorno del usuario).

En `crates/comandos-server/tests/dash_forward_python.rs`, función `launch`, añadir a la cadena de `Command`:

```rust
        .env_remove("COMANDOS_STATE_DB")
        .env_remove("COMANDOS_USAGE_DB")
        .env("XDG_STATE_HOME", home.join(".local/state"))
```

y donde la prueba arma el frente (`parse_args(...)` → `transport_config(cfg)`), poner `cfg.native = false;` antes: esa prueba compara el reenvío puro de la 2a y debe seguir haciéndolo.

- [ ] **Step 12: Ver pasar todo**

Run: `$C test -p comandos-server -j 6 && $C build -p xtask -j 6`
Expected: PASS de `dash_native_infra` (6 pruebas), `blocking` (9), `dash_boot`, `dash_forward`, `dash_forward_python`, `dash_statics`, `events_routes*`, `transport`. La prueba `schema_newer_while_live_forwards_once` imprime exactamente una línea «rutas nativas desactivadas» en stderr.

- [ ] **Step 13: Paridad sin regresiones**

Run (desde el checkout, con el binario recién compilado): `$C build -p comandos-cli -p xtask -j 6 && .build/target/debug/xtask parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks --comandos .build/target/debug/comandos`
Expected: `resumen: 16 OK, 0 DIFF` (o los SKIP ya documentados): la tabla nativa está vacía.

- [ ] **Step 14: fmt, clippy, commit**

```bash
$C fmt --all -- --check
$C clippy --workspace --all-targets -j 6 -- -D warnings
git add crates/comandos-server/Cargo.toml crates/comandos-server/src/lib.rs \
  crates/comandos-server/src/blocking.rs crates/comandos-server/src/dash/mod.rs \
  crates/comandos-server/src/dash/router.rs crates/comandos-server/src/dash/native/mod.rs \
  crates/comandos-server/src/dash/native/state.rs crates/comandos-server/tests/blocking.rs \
  crates/comandos-server/tests/dash_native_infra.rs crates/comandos-server/tests/dash_forward_python.rs \
  xtask/src/parity.rs Cargo.lock
git commit -m "feat(dash): clase Native, worker genérico de backend y puerta de esquema de app-state

El frente abre app-state.sqlite3 al arrancar con un BackendWorker<StateBackend>;
si la base tiene una migración desconocida apaga todo el conjunto nativo con una
línea en stderr. --no-native / COMANDOS_DASH_NATIVE=0 devuelven la 2a exacta.
El arnés y la prueba con oráculo ya no pueden tocar la base de estado real.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Kit de paridad (semántica Python, tmux, archivos, consulta) y arnés con base y tmux reales

Desplegable sola: añade módulos sin rutas nuevas; el frente sigue igual. El arnés gana `--state-db`, sesiones tmux privadas, `--no-native`, `oracle_path` y el resumen de reenvíos, que las tareas siguientes usan.

**Files:**
- Create: `crates/comandos-server/src/dash/native/py.rs`, `tmux.rs`, `files.rs`, `query.rs`
- Modify: `crates/comandos-server/src/dash/native/mod.rs` (módulos y campos `tmux`, `fc_list` en `NativeOptions`)
- Modify: `crates/comandos-server/tests/support/mod.rs`
- Create: `crates/comandos-server/tests/dash_native_kit.rs`
- Modify: `xtask/Cargo.toml`, `xtask/src/parity.rs`, `xtask/src/poll.rs`

**Interfaces:**
- Produces: `py::{is_space, strip, splitlines, int, float, repr_ascii, str_scalar, int_of, Conversion, NumError, is_session, is_pane, take_chars, clamp_py_float}`; `tmux::{Program, Tmux, Output, RunError, TmuxError, run_program}`; `files::{read_json, read_json_strict, Strict, write_json_atomic}`; `query::Query::{parse, first}`; `NativeOptions.{tmux, fc_list}`; en pruebas `support::{TOKEN, request_body, FakeLegacy, TestHome, Front, front, dead_port, tmux_available}`; en el arnés `StackOptions`, `Stack::start_with`, `Stack::forwarded_summary`, campo de fixture `oracle_path`.
- Consumes: `comandos_core::dashboard_access::{request_target_parts, query_pairs}`, `comandos_core::json::{workspace_loads_bytes, response_dumps}`.

- [ ] **Step 1: Pruebas que fallan del kit**

Crear `crates/comandos-server/tests/dash_native_kit.rs`:

```rust
//! Semántica Python, tmux, archivos y consulta de las rutas nativas.
mod support;

use comandos_server::dash::native::{
    files::{self, Strict},
    py::{self, Conversion, NumError},
    query::Query,
    tmux::{Program, Tmux, TmuxError},
};
use serde_json::json;
use std::{ffi::OsString, fs, os::unix::fs::PermissionsExt, time::Duration};
use support::{TestHome, tmux_available};

#[test]
fn python_int_float_and_repr() {
    assert_eq!(py::int(" 42 ").ok(), Some(42));
    assert_eq!(py::int("-1_000").ok(), Some(-1000));
    assert_eq!(py::int("007").ok(), Some(7));
    assert!(matches!(py::int("1__0"), Err(NumError::Invalid)));
    assert!(matches!(py::int("x"), Err(NumError::Invalid)));
    assert!(matches!(py::int("١٢"), Err(NumError::Exotic)));
    assert!(matches!(py::int("99999999999999999999999"), Err(NumError::Exotic)));
    assert_eq!(
        py::int_error_message("x'y"),
        Some(r#"invalid literal for int() with base 10: "x'y""#.to_string())
    );
    assert_eq!(
        py::int_error_message("a\tb"),
        Some(r"invalid literal for int() with base 10: 'a\tb'".to_string())
    );
    assert_eq!(py::float(" 2.5 ").ok(), Some(2.5));
    assert_eq!(py::float("1_0e1").ok(), Some(100.0));
    assert!(py::float("nan").unwrap().is_nan());
    assert_eq!(py::float("-Infinity").ok(), Some(f64::NEG_INFINITY));
    assert!(matches!(py::float("1_"), Err(NumError::Invalid)));
    assert_eq!(py::clamp_py_float(f64::NAN, 0.0, 25.0), 25.0);
    assert_eq!(py::clamp_py_float(-3.0, 0.0, 25.0), 0.0);
}

#[test]
fn python_str_and_int_of_json_values() {
    assert_eq!(py::str_scalar(&json!("a")).as_deref(), Some("a"));
    assert_eq!(py::str_scalar(&json!(true)).as_deref(), Some("True"));
    assert_eq!(py::str_scalar(&json!(null)).as_deref(), Some("None"));
    assert_eq!(py::str_scalar(&json!(12)).as_deref(), Some("12"));
    assert_eq!(py::str_scalar(&json!(1.5)), None);
    assert_eq!(py::str_scalar(&json!([1])), None);
    assert_eq!(py::int_of(&json!(1.9)), Ok(1));
    assert_eq!(py::int_of(&json!(-1.9)), Ok(-1));
    assert_eq!(py::int_of(&json!("12")), Ok(12));
    assert_eq!(py::int_of(&json!(true)), Ok(1));
    assert_eq!(
        py::int_of(&json!("z")),
        Err(Conversion::Value(
            "invalid literal for int() with base 10: 'z'".into()
        ))
    );
    assert_eq!(py::int_of(&json!([1])), Err(Conversion::Type));
    let nan: serde_json::Value = serde_json::from_str("NaN").unwrap_or(json!(null));
    if !nan.is_null() {
        assert_eq!(
            py::int_of(&nan),
            Err(Conversion::Value("cannot convert float NaN to integer".into()))
        );
    }
}

#[test]
fn python_lines_and_names() {
    assert_eq!(py::splitlines("a\nb\x0bc\r\nd\n"), ["a", "b", "c", "d"]);
    assert_eq!(py::strip("\u{1c} x \u{3000}"), "x");
    assert!(py::is_session("work_1.a-b"));
    assert!(!py::is_session("a\n"));
    assert!(!py::is_session(&"a".repeat(81)));
    assert!(py::is_pane("%12"));
    assert!(!py::is_pane("%12345678"));
    assert_eq!(py::take_chars("ñandú", 3), "ñan");
}

#[test]
fn query_is_urlsplit_plus_parse_qs() {
    let q = Query::parse("/tab-models?session=a+b&session=c&x=").ok().unwrap();
    assert_eq!(q.first("session"), Some("a b"));
    assert_eq!(q.first("x"), None);
    assert!(Query::parse("/notices?after=%FF").is_err(), "U+FFFD declina");
    let q = Query::parse("/notices?after=1#frag").ok().unwrap();
    assert_eq!(q.first("after"), Some("1"));
}

#[test]
fn atomic_json_write_keeps_mode_and_python_bytes() {
    let home = TestHome::new("kit-files");
    let path = home.hooks().join("prefs.json");
    files::write_json_atomic(&path, &json!({"b": 1, "a": "ñ"})).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"b": 1, "a": "ñ"}"#);
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o7777, 0o600);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    files::write_json_atomic(&path, &json!({})).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o7777, 0o640);
    assert_eq!(files::read_json(&path), Some(json!({})));
    fs::write(&path, "{roto").unwrap();
    assert_eq!(files::read_json(&path), None);
    assert!(matches!(files::read_json_strict(&path), Strict::Unreadable));
    assert!(matches!(
        files::read_json_strict(&home.hooks().join("no-existe.json")),
        Strict::Missing
    ));
    // Ningún temporal huérfano.
    let leftovers: Vec<_> = fs::read_dir(home.hooks())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty());
}

#[tokio::test]
async fn tmux_runs_against_a_private_server() {
    if !tmux_available() {
        eprintln!("tmux no está instalado: se salta");
        return;
    }
    let home = TestHome::new("kit-tmux");
    let tmux = Tmux::private(&home.tmux_dir());
    let made = tmux.run(&["new-session", "-d", "-s", "kit", "cat"]).await.unwrap();
    assert!(made.ok, "{}", made.stderr);
    let listed = tmux.run(&["list-sessions", "-F", "#{session_name}"]).await.unwrap();
    assert_eq!(py::splitlines(&listed.stdout), ["kit"]);
    let missing = tmux.run(&["has-session", "-t", "=otra"]).await.unwrap();
    assert!(!missing.ok);
    tmux.run(&["kill-server"]).await.unwrap();
}

#[tokio::test]
async fn tmux_timeout_missing_and_decode_errors() {
    // `tail -f /dev/null -- …` nunca termina: simula un tmux colgado.
    let hung = Tmux {
        program: Program {
            path: "tail".into(),
            prefix: vec![OsString::from("-f"), "/dev/null".into(), "--".into()],
            env: vec![],
            env_remove: vec![],
        },
        timeout: Duration::from_millis(300),
    };
    let error = hung.run(&["has-session", "-t", "=x"]).await.unwrap_err();
    assert!(matches!(error, TmuxError::Timeout { .. }));
    assert_eq!(
        error.python_message().as_deref(),
        Some("Command '['tmux', 'has-session', '-t', '=x']' timed out after 0.3 seconds")
    );
    let missing = Tmux {
        program: Program::named("/no-existe/tmux"),
        timeout: Duration::from_secs(5),
    };
    let error = missing.run(&["list-sessions"]).await.unwrap_err();
    assert_eq!(
        error.python_message().as_deref(),
        Some("[Errno 2] No such file or directory: 'tmux'")
    );
    // printf con `%.0s` se traga el argumento extra.
    let bytes = Tmux {
        program: Program {
            path: "printf".into(),
            prefix: vec![OsString::from("a\\r\\nb\\rc\\n%.0s")],
            env: vec![],
            env_remove: vec![],
        },
        timeout: Duration::from_secs(5),
    };
    assert_eq!(bytes.run(&["x"]).await.unwrap().stdout, "a\nb\nc\n");
    let invalid = Tmux {
        program: Program {
            path: "printf".into(),
            prefix: vec![OsString::from("\\377%.0s")],
            env: vec![],
            env_remove: vec![],
        },
        timeout: Duration::from_secs(5),
    };
    let error = invalid.run(&["x"]).await.unwrap_err();
    assert!(matches!(error, TmuxError::Decode));
    assert_eq!(error.python_message(), None);
}
```

Ampliar `crates/comandos-server/tests/support/mod.rs` (se conserva lo existente):

```rust
use comandos_server::dash::{
    DashConfig,
    native::{NativeOptions, tmux::{Program, Tmux}},
    parse_args, serve_with,
};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};
use tokio::{net::TcpListener, sync::watch, task::JoinHandle};

pub const TOKEN: &str = "dash-native-test-token";
/// 2026-10-04T12:00:00Z en ms: el reloj fijo de las pruebas nativas.
pub const NOW_MS: i64 = 1_791_115_200_000;

/// Como `request`, con cuerpo JSON y su `Content-Length`.
pub async fn request_body(port: u16, method: &str, target: &str, extra: &str, body: &str) -> Wire {
    let extra = format!(
        "{extra}X-Comandos-Token: {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    let wire = format!(
        "{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{extra}Connection: close\r\n\r\n{body}"
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(wire.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    timeout(Duration::from_secs(40), stream.read_to_end(&mut out))
        .await
        .expect("la respuesta debe cerrar la conexión")
        .unwrap();
    parse(&out)
}

/// GET con el token local (las rutas nativas son API: pasan la puerta).
pub async fn get(port: u16, target: &str) -> Wire {
    request_body(port, "GET", target, "", "").await
}

pub fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Un puerto sin nadie escuchando: si el frente reenviara, respondería 502.
pub fn dead_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// HOME temporal con `~/.claude/hooks/state`, token y `TMUX_TMPDIR` propios.
pub struct TestHome {
    pub root: PathBuf,
}

impl TestHome {
    pub fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("cmd-native-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".claude/hooks/state")).unwrap();
        std::fs::create_dir_all(root.join("tmux")).unwrap();
        std::fs::write(root.join(".claude/hooks/dash-token"), TOKEN).unwrap();
        std::fs::write(root.join(".claude/hooks/app-tabs.json"), "{}").unwrap();
        Self { root }
    }
    pub fn hooks(&self) -> PathBuf {
        self.root.join(".claude/hooks")
    }
    pub fn tmux_dir(&self) -> PathBuf {
        self.root.join("tmux")
    }
    pub fn state_db(&self) -> PathBuf {
        self.root.join(".local/state/comandos/app-state.sqlite3")
    }
    pub fn write(&self, name: &str, text: &str) {
        std::fs::write(self.hooks().join(name), text).unwrap();
    }
    pub fn options(&self) -> NativeOptions {
        let mut opts = NativeOptions::for_home(&self.root, self.state_db());
        opts.clock = Arc::new(|| NOW_MS);
        opts.tmux = Tmux::private(&self.tmux_dir());
        // Sin fc-list en las pruebas salvo que una prueba lo fije.
        opts.fc_list = Program::named("/no-existe/fc-list");
        opts
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        // Nunca el servidor tmux del usuario: el socket vive en tmux_dir.
        let _ = Command::new("tmux")
            .arg("kill-server")
            .env_remove("TMUX")
            .env("TMUX_TMPDIR", self.tmux_dir())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub struct Front {
    pub port: u16,
    stop: watch::Sender<bool>,
    task: JoinHandle<std::io::Result<()>>,
}

impl Front {
    pub async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }
}

pub fn config(home: &TestHome, legacy_port: u16) -> DashConfig {
    let mut cfg = parse_args(&[], &home.root, Some(&legacy_port.to_string())).unwrap();
    cfg.token = TOKEN.as_bytes().to_vec();
    cfg.dash_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dash");
    cfg.state_db = home.state_db();
    cfg
}

pub async fn front(home: &TestHome, legacy_port: u16, opts: NativeOptions) -> Front {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (stop, shutdown) = watch::channel(false);
    let cfg = config(home, legacy_port);
    let task = tokio::spawn(serve_with(listener, cfg, Some(opts), shutdown));
    Front { port, stop, task }
}

/// Heredado falso: responde `{"legacy": true}` y anota la línea de petición.
pub struct FakeLegacy {
    pub port: u16,
    pub seen: Arc<Mutex<Vec<String>>>,
}

impl FakeLegacy {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let log = log.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    loop {
                        let Ok(n) = stream.read(&mut chunk).await else { return };
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&buf[..end]).to_string();
                            let length = head
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                                })
                                .unwrap_or(0);
                            if buf.len() >= end + 4 + length {
                                log.lock().unwrap().push(head.lines().next().unwrap_or("").into());
                                break;
                            }
                        }
                    }
                    let body = br#"{"legacy": true}"#;
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(head.as_bytes()).await;
                    let _ = stream.write_all(body).await;
                });
            }
        });
        Self { port, seen }
    }
    pub fn requests(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}
```

(Las importaciones `TcpStream`, `AsyncReadExt`, `AsyncWriteExt`, `timeout` y `Duration` ya están en el módulo.)

- [ ] **Step 2: Ejecutar y ver el fallo**

Run: `$C test -p comandos-server --test dash_native_kit -j 6`
Expected: FAIL de compilación: `no py/tmux/files/query in native`, `no field tmux/fc_list`.

- [ ] **Step 3: Implementar `py.rs`**

Crear `crates/comandos-server/src/dash/native/py.rs`:

```rust
//! Semántica de CPython que las rutas nativas reproducen. Todo lo que no se
//! puede reproducir con certeza devuelve `Exotic` y la ruta declina.
use serde_json::Value;

/// `str.isspace()` de CPython: `char::is_whitespace` más U+001C–U+001F.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// `str.splitlines()`: sin la línea vacía final.
pub fn splitlines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let boundary = matches!(
            c,
            '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}'
        );
        if boundary {
            out.push(&s[start..i]);
            let mut next = i + c.len_utf8();
            if c == '\r' && chars.peek().is_some_and(|&(_, d)| d == '\n') {
                chars.next();
                next += 1;
            }
            start = next;
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumError {
    /// `ValueError` del Python.
    Invalid,
    /// Válido o inválido según reglas Unicode o de enteros grandes: se declina.
    Exotic,
}

/// Dígitos ASCII con `_` solo entre dígitos (PEP 515).
fn digits_with_underscores(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(s.len());
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'_' {
            let left = i > 0 && bytes[i - 1].is_ascii_digit();
            let right = bytes.get(i + 1).is_some_and(u8::is_ascii_digit);
            if !(left && right) {
                return None;
            }
        } else if b.is_ascii_digit() {
            out.push(b as char);
        } else {
            return None;
        }
    }
    Some(out)
}

/// `int(text)` en base 10.
pub fn int(text: &str) -> Result<i64, NumError> {
    if !text.is_ascii() {
        return Err(NumError::Exotic);
    }
    let t = strip(text);
    let (negative, body) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let digits = digits_with_underscores(body).ok_or(NumError::Invalid)?;
    let magnitude: i128 = digits.parse().map_err(|_| NumError::Exotic)?;
    let value = if negative { -magnitude } else { magnitude };
    i64::try_from(value).map_err(|_| NumError::Exotic)
}

/// `str(ValueError)` de `int(text)`; `None` si el texto no es ASCII.
pub fn int_error_message(text: &str) -> Option<String> {
    repr_ascii(text).map(|r| format!("invalid literal for int() with base 10: {r}"))
}

/// `float(text)`.
pub fn float(text: &str) -> Result<f64, NumError> {
    if !text.is_ascii() {
        return Err(NumError::Exotic);
    }
    let t = strip(text);
    let lower = t.to_ascii_lowercase();
    let unsigned = lower.trim_start_matches(['+', '-']);
    if lower.len() - unsigned.len() > 1 {
        return Err(NumError::Invalid);
    }
    if matches!(unsigned, "inf" | "infinity" | "nan") {
        return lower.parse::<f64>().map_err(|_| NumError::Invalid);
    }
    // Quita `_` solo si separa dos dígitos; cualquier otro `_` es inválido.
    let bytes = t.as_bytes();
    let mut clean = String::with_capacity(t.len());
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'_' {
            let ok = i > 0
                && bytes[i - 1].is_ascii_digit()
                && bytes.get(i + 1).is_some_and(u8::is_ascii_digit);
            if !ok {
                return Err(NumError::Invalid);
            }
        } else if b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E') {
            clean.push(b as char);
        } else {
            return Err(NumError::Invalid);
        }
    }
    clean.parse::<f64>().map_err(|_| NumError::Invalid)
}

/// `max(lo, min(hi, x))` de Python: con NaN, `min(hi, nan)` devuelve `hi`.
pub fn clamp_py_float(x: f64, lo: f64, hi: f64) -> f64 {
    let upper = if x < hi { x } else { hi };
    if lo < upper { upper } else { lo }
}

/// `repr(str)` para texto ASCII; `None` si hay no-ASCII (se declina).
pub fn repr_ascii(s: &str) -> Option<String> {
    if !s.is_ascii() {
        return None;
    }
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c == '\u{7f}' => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    Some(out)
}

fn integer_text(raw: &str) -> bool {
    !raw.contains(['.', 'e', 'E']) && !matches!(raw, "NaN" | "Infinity" | "-Infinity")
}

/// `str(value)` para escalares cuya forma es segura; `None` → declinar
/// (flotantes: el `repr` de Python; listas y objetos: su `repr`).
pub fn str_scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Bool(true) => Some("True".into()),
        Value::Bool(false) => Some("False".into()),
        Value::Null => Some("None".into()),
        Value::Number(n) if integer_text(n.as_str()) => {
            Some(if n.as_str() == "-0" { "0".into() } else { n.as_str().to_owned() })
        }
        _ => None,
    }
}

/// Excepción que lanzaría `int(value)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conversion {
    /// `ValueError(str)`.
    Value(String),
    /// `OverflowError` (infinito).
    Overflow,
    /// `TypeError`.
    Type,
    /// No reproducible con certeza: declinar.
    Exotic,
}

/// `int(value)` sobre un valor JSON.
pub fn int_of(value: &Value) -> Result<i64, Conversion> {
    match value {
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Number(n) => {
            let raw = n.as_str();
            match raw {
                "NaN" => Err(Conversion::Value("cannot convert float NaN to integer".into())),
                "Infinity" | "-Infinity" => Err(Conversion::Overflow),
                _ if integer_text(raw) => raw.parse::<i64>().map_err(|_| Conversion::Exotic),
                _ => {
                    let x: f64 = raw.parse().map_err(|_| Conversion::Exotic)?;
                    if !x.is_finite() {
                        return Err(Conversion::Overflow);
                    }
                    let t = x.trunc();
                    if t >= -9.223_372_036_854_775_808e18 && t < 9.223_372_036_854_775_808e18 {
                        Ok(t as i64)
                    } else {
                        Err(Conversion::Exotic)
                    }
                }
            }
        }
        Value::String(s) => match int(s) {
            Ok(v) => Ok(v),
            Err(NumError::Exotic) => Err(Conversion::Exotic),
            Err(NumError::Invalid) => int_error_message(s)
                .map(Conversion::Value)
                .ok_or(Conversion::Exotic),
        },
        Value::Null | Value::Array(_) | Value::Object(_) => Err(Conversion::Type),
    }
}

/// `SESSION_RE = ^[A-Za-z0-9._-]{1,80}\Z` (`bin/cc-dash:5708`).
pub fn is_session(s: &str) -> bool {
    (1..=80).contains(&s.len())
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// `PANE_RE = ^%\d{1,7}\Z` (5709). tmux solo emite dígitos ASCII.
pub fn is_pane(s: &str) -> bool {
    s.strip_prefix('%')
        .is_some_and(|d| (1..=7).contains(&d.len()) && d.bytes().all(|b| b.is_ascii_digit()))
}

/// `s[:n]` de Python (por caracteres).
pub fn take_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
```

- [ ] **Step 4: Implementar `tmux.rs`**

Crear `crates/comandos-server/src/dash/native/tmux.rs`:

```rust
//! Procesos externos (`tmux`, `fc-list`) como `subprocess.run(..., text=True,
//! capture_output=True, timeout=…)` del Python: stdout/stderr decodificados
//! como UTF-8 estricto con saltos universales; al vencer el plazo se mata.
use super::py::repr_ascii;
use crate::HandlerError;
use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

#[derive(Debug, Clone)]
pub struct Program {
    pub path: PathBuf,
    /// Argumentos que van antes de los del llamador (dobles de prueba).
    pub prefix: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub env_remove: Vec<OsString>,
}

impl Program {
    /// Hereda el entorno del proceso, como `subprocess.run`.
    pub fn named(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            prefix: Vec::new(),
            env: Vec::new(),
            env_remove: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// `returncode == 0`.
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug)]
pub enum RunError {
    Timeout,
    Spawn(io::Error),
    Decode,
}

fn universal(bytes: Vec<u8>) -> Result<String, RunError> {
    let text = String::from_utf8(bytes).map_err(|_| RunError::Decode)?;
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

pub async fn run_program(
    program: &Program,
    args: &[&str],
    timeout: Duration,
) -> Result<Output, RunError> {
    let mut cmd = tokio::process::Command::new(&program.path);
    cmd.args(&program.prefix)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for name in &program.env_remove {
        cmd.env_remove(name);
    }
    for (name, value) in &program.env {
        cmd.env(name, value);
    }
    let child = cmd.spawn().map_err(RunError::Spawn)?;
    // Al vencer, el futuro se suelta con el hijo dentro y kill_on_drop lo mata.
    let out = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| RunError::Timeout)?
        .map_err(RunError::Spawn)?;
    Ok(Output {
        ok: out.status.success(),
        stdout: universal(out.stdout)?,
        stderr: universal(out.stderr)?,
    })
}

#[derive(Debug, Clone)]
pub struct Tmux {
    pub program: Program,
    pub timeout: Duration,
}

#[derive(Debug)]
pub enum TmuxError {
    Timeout { args: Vec<String>, after: Duration },
    Spawn(io::ErrorKind),
    Decode,
}

impl Tmux {
    /// Producción: `tmux` del PATH con el entorno del proceso (`TMUX_TMPDIR`,
    /// `TMUX`), igual que el Python. Plazo de `tmux()` (5833): 5 s.
    pub fn system() -> Self {
        Self {
            program: Program::named("tmux"),
            timeout: Duration::from_secs(5),
        }
    }

    /// Servidor privado de pruebas: nunca el del usuario.
    pub fn private(socket_dir: &Path) -> Self {
        let mut program = Program::named("tmux");
        program.env.push(("TMUX_TMPDIR".into(), socket_dir.into()));
        program.env_remove.push("TMUX".into());
        Self {
            program,
            timeout: Duration::from_secs(5),
        }
    }

    pub async fn run(&self, args: &[&str]) -> Result<Output, TmuxError> {
        run_program(&self.program, args, self.timeout)
            .await
            .map_err(|error| match error {
                RunError::Timeout => TmuxError::Timeout {
                    args: args.iter().map(|a| (*a).to_owned()).collect(),
                    after: self.timeout,
                },
                RunError::Spawn(e) => TmuxError::Spawn(e.kind()),
                RunError::Decode => TmuxError::Decode,
            })
    }
}

fn seconds(after: Duration) -> String {
    if after.subsec_nanos() == 0 {
        after.as_secs().to_string()
    } else {
        format!("{}", after.as_secs_f64())
    }
}

impl TmuxError {
    /// `str(exc)` del Python cuando la ruta captura la excepción
    /// (`get_tmux_mouse`/`set_tmux_mouse`). `None`: no hay texto seguro → declinar.
    pub fn python_message(&self) -> Option<String> {
        match self {
            TmuxError::Timeout { args, after } => {
                let mut parts = vec![repr_ascii("tmux")?];
                for arg in args {
                    parts.push(repr_ascii(arg)?);
                }
                Some(format!(
                    "Command '[{}]' timed out after {} seconds",
                    parts.join(", "),
                    seconds(*after)
                ))
            }
            TmuxError::Spawn(io::ErrorKind::NotFound) => {
                Some("[Errno 2] No such file or directory: 'tmux'".into())
            }
            TmuxError::Spawn(_) | TmuxError::Decode => None,
        }
    }

    /// Ramas donde el Python NO captura: `TimeoutExpired` → 504, el resto → 500.
    pub fn uncaught(&self) -> HandlerError {
        match self {
            TmuxError::Timeout { .. } => HandlerError::Timeout,
            _ => HandlerError::Failure,
        }
    }
}
```

- [ ] **Step 5: Implementar `files.rs` y `query.rs`**

Crear `crates/comandos-server/src/dash/native/files.rs`:

```rust
//! Archivos JSON de `~/.claude/hooks` como los lee y escribe `bin/cc-dash`.
use comandos_core::json::{response_dumps, workspace_loads_bytes};
use serde_json::Value;
use std::{
    fs, io,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};

/// `json.load(open(path))` con `except Exception`: cualquier fallo es `None`.
pub fn read_json(path: &Path) -> Option<Value> {
    fs::read(path).ok().and_then(|bytes| workspace_loads_bytes(&bytes))
}

pub enum Strict {
    /// `FileNotFoundError`.
    Missing,
    /// Otro `OSError` o `ValueError` (JSON roto, UTF-8 inválido).
    Unreadable,
    Value(Value),
}

/// `_tab_registry` (6466): ausente ≠ ilegible.
pub fn read_json_strict(path: &Path) -> Strict {
    match fs::read(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Strict::Missing,
        Err(_) => Strict::Unreadable,
        Ok(bytes) => workspace_loads_bytes(&bytes).map_or(Strict::Unreadable, Strict::Value),
    }
}

/// `write_json_file` (5232) → `write_file_atomic` (5208): temporal en el mismo
/// directorio, fsync, permisos del archivo previo (0600 si es nuevo) y rename.
/// Bloquea: llamar dentro de `spawn_blocking`.
pub fn write_json_atomic(path: &Path, value: &Value) -> io::Result<()> {
    let text = response_dumps(value).map_err(io::Error::other)?;
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
```

Crear `crates/comandos-server/src/dash/native/query.rs`:

```rust
//! `urllib.parse.parse_qs(urlsplit(self.path).query)` y `(q.get(k) or [d])[0]`.
use super::Fault;
use crate::dash::router::path_of;
use comandos_core::dashboard_access::{query_pairs, request_target_parts};

pub struct Query(Vec<(String, String)>);

impl Query {
    /// Declina si el `urlsplit` portado ve otra ruta que el corte crudo, o si
    /// la decodificación produjo U+FFFD (`errors="replace"` del Python: el
    /// texto resultante no se puede comparar con certeza).
    pub fn parse(target: &str) -> Result<Query, Fault> {
        let (path, query) = request_target_parts(target).ok_or(Fault::Decline)?;
        if path != path_of(target) {
            return Err(Fault::Decline);
        }
        let pairs = query_pairs(&query, false);
        if pairs
            .iter()
            .any(|(k, v)| k.contains('\u{fffd}') || v.contains('\u{fffd}'))
        {
            return Err(Fault::Decline);
        }
        Ok(Query(pairs))
    }

    /// Primera ocurrencia no vacía (`parse_qs` descarta las vacías).
    pub fn first(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}
```

En `native/mod.rs`: añadir `pub mod files; pub mod py; pub mod query; pub mod tmux;`, y en `NativeOptions` los campos

```rust
    /// `tmux` como lo llama el Python (entorno heredado, plazo 5 s).
    pub tmux: tmux::Tmux,
    /// `fc-list` de `_installed_font_families` (7724).
    pub fc_list: tmux::Program,
```

inicializados en `for_home` con `tmux: tmux::Tmux::system()` y `fc_list: tmux::Program::named("fc-list")`.

- [ ] **Step 6: Ver pasar el kit**

Run: `$C test -p comandos-server --test dash_native_kit -j 6`
Expected: PASS (7 pruebas; `tmux_runs_against_a_private_server` se salta con aviso si no hay tmux).

- [ ] **Step 7: Arnés — base de estado, tmux privado, `--no-native`, `oracle_path`, resumen de reenvíos**

`xtask/Cargo.toml`, en `[dependencies]`: `rusqlite.workspace = true`.

En `xtask/src/parity.rs`:

```rust
/// Opciones de la pila aislada (las de `Stack::start` más las de la 2b).
pub struct StackOptions<'a> {
    pub hooks: &'a Path,
    pub comandos: &'a Path,
    pub keep: bool,
    /// Copia de solo lectura (backup de SQLite) en los dos HOME.
    pub state_db: Option<&'a Path>,
    /// Pasa `--no-native` al frente: A/B contra la 2a.
    pub no_native: bool,
}

/// Copia la base con la API de backup: nunca escribe en `src`.
fn copy_state_db(src: &Path, home: &Path) -> Result<(), String> {
    let dest = home.join(".local/state/comandos/app-state.sqlite3");
    fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    let source = rusqlite::Connection::open_with_flags(
        src,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("--state-db {}: {e}", src.display()))?;
    source
        .backup(rusqlite::DatabaseName::Main, &dest, None)
        .map_err(|e| format!("backup de {}: {e}", src.display()))
}

/// Sesiones `local` + claves de `app-tabs.json` (≤20, nombres válidos), creadas
/// en el MISMO orden en los dos servidores privados: mismos `%pane` y `$id`.
fn tmux_sessions_for(hooks: &Path) -> Vec<String> {
    let mut names = vec!["local".to_string()];
    if let Ok(text) = fs::read_to_string(hooks.join("app-tabs.json"))
        && let Ok(Value::Object(tabs)) = serde_json::from_str::<Value>(&text)
    {
        for key in tabs.keys() {
            let valid = (1..=80).contains(&key.len())
                && key.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
            if valid && !names.contains(key) && names.len() < 21 {
                names.push(key.clone());
            }
        }
    }
    names
}

fn start_tmux(socket_dir: &Path, sessions: &[String]) -> Result<(), String> {
    for name in sessions {
        let status = Command::new("tmux")
            .args(["new-session", "-d", "-s", name, "cat"])
            .env_remove("TMUX")
            .env("TMUX_TMPDIR", socket_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("tmux: {e}"))?;
        if !status.success() {
            return Err(format!("tmux new-session {name} falló"));
        }
    }
    Ok(())
}

fn kill_tmux(socket_dir: &Path) {
    let _ = Command::new("tmux")
        .arg("kill-server")
        .env_remove("TMUX")
        .env("TMUX_TMPDIR", socket_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}
```

`Stack` gana los campos `tmux_dirs: Vec<PathBuf>` y `front_log: PathBuf` y un `impl Drop for Stack` que llama a `kill_tmux` en cada `tmux_dirs`. `Drop::drop` corre antes de soltar los campos: los servidores tmux privados mueren primero, luego los procesos (`servers`) y al final la raíz temporal (`_root`), que sigue siendo el último campo. `Stack::start(hooks, comandos, keep)` pasa a ser:

```rust
    pub fn start(hooks: &Path, comandos: &Path, keep: bool) -> Result<Stack, String> {
        Self::start_with(StackOptions { hooks, comandos, keep, state_db: None, no_native: false })
    }
```

y el cuerpo actual se mueve a `start_with(o: StackOptions)` con estos cambios, en este orden: tras `copy_hooks` a ambos HOME, `if let Some(db) = o.state_db { copy_state_db(db, &home1)?; copy_state_db(db, &home2)?; }`; tras `make_fakebin`, `let sessions = tmux_sessions_for(&hooks1); if tmux_available() { start_tmux(&tmux1, &sessions)?; start_tmux(&tmux2, &sessions)?; }` (con `fn tmux_available() -> bool` igual que en las pruebas); el frente recibe `if o.no_native { front.arg("--no-native"); }` y `front.env("COMANDOS_DASH_TRACE_FORWARD", "1")` (se fija en el `Command` antes de pasarlo a `spawn`, que no lo quita); y se guarda `front_log: d("front.log")`.

```rust
impl Stack {
    /// Rutas que el frente reenvió al heredado (de su traza), con su cuenta.
    pub fn forwarded_summary(&self) -> Vec<(String, usize)> {
        let text = fs::read_to_string(&self.front_log).unwrap_or_default();
        let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
        for line in text.lines() {
            if let Some(route) = line.strip_prefix("comandos dash: reenvío ") {
                *counts.entry(route.to_string()).or_default() += 1;
            }
        }
        counts.into_iter().collect()
    }
}
```

En `Case` añadir `oracle_path: Option<String>` (leído con `s("oracle_path")`), y en `run`:

```rust
        let go = |port, path: &str| request(port, &c.method, path, &headers, c.body.as_deref());
        let oracle = c.oracle_path.as_deref().unwrap_or(&c.path);
        let (r_py, r_rs) = match (go(stack.p_py, oracle), go(stack.p_front, &c.path)) {
```

`Args` gana `state_db: Option<PathBuf>` (`--state-db <ruta>`) y `no_native: bool` (`--no-native`); `run` llama a `Stack::start_with(StackOptions { … })` y, antes de `drop(stack)`:

```rust
    let forwarded = stack.forwarded_summary();
    println!("\nreenviadas al heredado por el frente ({} rutas distintas):", forwarded.len());
    for (route, n) in &forwarded {
        println!("  {n:>4}  {route}");
    }
```

En `xtask/src/poll.rs`, `parse` acepta `--state-db <ruta>` y `--no-native` (guardados en `Opts`) y `run` usa `crate::parity::Stack::start_with(crate::parity::StackOptions { hooks, comandos: &comandos, keep: false, state_db: o.state_db.as_deref(), no_native: o.no_native })`; al terminar imprime `stack.forwarded_summary()` igual que `parity`.

- [ ] **Step 8: Verificar el arnés**

Run: `$C build -p comandos-cli -p xtask -j 6 && .build/target/debug/xtask parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks --state-db ~/.local/state/comandos/app-state.sqlite3 --comandos .build/target/debug/comandos`
Expected: `resumen: 16 OK, 0 DIFF` y la lista «reenviadas al heredado…» con las rutas API del fixture (`/state`, `/work-marks`, `/prefs`…). La base real solo se abre en modo lectura (comprobar `ls -l --time-style=full-iso ~/.local/state/comandos/app-state.sqlite3` antes y después: misma mtime).

- [ ] **Step 9: fmt, clippy, commit**

```bash
$C fmt --all -- --check
$C clippy --workspace --all-targets -j 6 -- -D warnings
git add crates/comandos-server/src/dash/native/mod.rs crates/comandos-server/src/dash/native/py.rs \
  crates/comandos-server/src/dash/native/tmux.rs crates/comandos-server/src/dash/native/files.rs \
  crates/comandos-server/src/dash/native/query.rs crates/comandos-server/tests/support/mod.rs \
  crates/comandos-server/tests/dash_native_kit.rs xtask/Cargo.toml xtask/src/parity.rs xtask/src/poll.rs Cargo.lock
git commit -m "feat(dash): kit de paridad para rutas nativas y arnés con base y tmux privados

py (int/float/repr/str/splitlines de CPython), tmux por tokio::process con los
mensajes exactos de TimeoutExpired/FileNotFoundError, JSON atómico como
write_file_atomic y consulta como parse_qs. xtask parity/poll: --state-db
(backup de solo lectura), sesiones tmux privadas idénticas, --no-native,
oracle_path y resumen de rutas reenviadas.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Dominio D — lecturas ligeras y preferencias (8 rutas)

Desplegable sola: GET `/prefs`, `/tabs`, `/tab-history`, `/tab-models`, `/active-tab`, `/tmux-mouse` y POST `/prefs-set`, `/tmux-mouse` pasan a Rust. Son las de más tráfico del inventario §1.11 (`/active-tab` cada 1 s, `/tab-models` cada 2 s por iframe, `/prefs` cada 3–5 s). Ninguna GET de este dominio escribe.

**Files:**
- Create: `crates/comandos-server/src/dash/native/light.rs`
- Modify: `crates/comandos-server/src/dash/native/mod.rs`
- Create: `crates/comandos-server/tests/support/oracle.rs`; Modify: `crates/comandos-server/tests/support/mod.rs` (`pub mod oracle;`)
- Create: `crates/comandos-server/tests/dash_native_light.rs`
- Modify: `xtask/parity/frente.jsonl`

**Interfaces:**
- Produces: `light::{LightRoute, ROUTES, answer, read_prefs, tab_labels, favorites_set, ordered_tab_keys, tmux_sessions, HIDDEN_SESSIONS}`; `native::{reply, NativeRoute::Light}`; campos `Native.{prefs_lock, fonts}`; `support::oracle::{oracle, Oracle}`.
- Consumes: kit de la Tarea 2.

Comportamiento portado, ruta a ruta (la prueba que lo fija entre corchetes):

- **GET `/prefs`** (8604): `dict(read_prefs())` — claves de `prefs.json` si es un objeto (cualquier fallo o no-objeto → `{}`), luego `setdefault` de `PREFS_DEFAULTS` (7689: `theme`, `font_family`, `font_size`, `cursor_shape`, `cursor_blink`, `terminal_padding`, `terminal_opacity`, `ligatures`, `button_style`, `tabs_layout`) — y `fonts` = filas `{"family","label","a11y"}` de `FONT_CATALOG` (7707) cuya familia sale en `fc-list : family` (caché de 60 s, también del fallo; plazo 5 s; se lee `stdout` aunque falle) más `{"family":"Monospace","label":"Monospace del sistema","a11y":false}`. [`prefs_get_*`]
- **POST `/prefs-set`** (9100, `update_prefs` 7768): mismo orden de comprobaciones y asignaciones que el Python (detallado en el código). 400 con `str(ValueError)`; `OSError` al escribir → 503 `No se pudieron guardar las preferencias`; `int(inf)` → `OverflowError` no capturado → 500. Respuesta `{"ok": true, "favorites": prefs.get("favorites", [])}`. Un `tokio::sync::Mutex` hace de `_PREFS_LOCK`. [`prefs_set_*`]
- **GET `/tabs`** (8673): `tmux list-sessions -F #{session_name}` (rc≠0 → vacío; excepción no capturada → 504/500), fila `local` si está viva, luego `ordered_tab_keys(tab_labels(), favorites)` filtrado por vivas y no ocultas (`hub`, `local`, `control`). [`tabs_and_history_*`]
- **GET `/tab-history`** (8684, `read_tab_history` 5318): hasta 80 entradas, filas `{session,label,cwd,agent,ts,reason}` + `alive`; se saltan las de pestañas abiertas; máximo 40. [`tabs_and_history_*`]
- **GET `/tab-models`** (8415): `{"session": s, "panes": …}`; una entrada verdadera que no es objeto → `AttributeError` → 500. [`tab_models_*`]
- **GET `/active-tab`** (8427): el objeto de `app-tab-active.json` (fallo → `{}`; JSON no-objeto → 500) y, si `session` es un nombre válido y `has-session` da 0, `pane` = `display-message -p '#{pane_id}'` si casa `PANE_RE`. [`active_tab_*`]
- **GET/POST `/tmux-mouse`** (8604 / 9552, `get_tmux_mouse` 5883, `set_tmux_mouse` 5905): 400 `Nombre de sesion invalido`; 404 `No hay sesion tmux '<s>'`; 500 con `stderr.strip() or "tmux fallo"` o con `str(exc).strip()` (excepción capturada). [`tmux_mouse_*`, `tmux_missing_messages`, `tmux_hung_times_out`]

- [ ] **Step 1: Ayudante del oráculo Python**

Crear `crates/comandos-server/tests/support/oracle.rs`:

```rust
//! El `cc-dash` Python del repositorio sobre el MISMO HOME que el frente:
//! se comparan bytes y se comprueba que lo que escribe uno lo lee el otro.
//! Sin `python3` las pruebas que lo usan se saltan con un aviso.
#![allow(dead_code)]
use super::TestHome;
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tokio::{net::TcpStream, time::sleep};

pub struct Oracle {
    pub port: u16,
    child: Child,
}

impl Drop for Oracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub async fn oracle(home: &TestHome) -> Option<Oracle> {
    let python = Command::new("python3")
        .args(["-c", "import sys"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !python {
        eprintln!("python3 no está instalado: se salta la comparación con el oráculo");
        return None;
    }
    let port = super::dead_port();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let err = std::fs::File::create(home.root.join("oracle.err")).unwrap();
    let mut child = Command::new("python3")
        .arg(repo.join("bin/cc-dash"))
        .arg(port.to_string())
        .arg("--no-open")
        .env_remove("TMUX")
        .env_remove("COMANDOS_STATE_DB")
        .env_remove("COMANDOS_USAGE_DB")
        .env("HOME", &home.root)
        .env("XDG_STATE_HOME", home.root.join(".local/state"))
        .env("TMUX_TMPDIR", home.tmux_dir())
        .env("COMANDOS_DASH_DIR", repo.join("dash"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(err)
        .spawn()
        .unwrap();
    let started = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).await.is_err() {
        if let Ok(Some(status)) = child.try_wait() {
            let log = std::fs::read_to_string(home.root.join("oracle.err")).unwrap_or_default();
            panic!("cc-dash salió con {status}: {log}");
        }
        assert!(started.elapsed() < Duration::from_secs(20), "cc-dash no arrancó");
        sleep(Duration::from_millis(100)).await;
    }
    Some(Oracle { port, child })
}
```

y en `support/mod.rs` añadir `pub mod oracle;`.

- [ ] **Step 2: Pruebas que fallan del dominio D**

Crear `crates/comandos-server/tests/dash_native_light.rs`:

```rust
//! Dominio D: preferencias, pestañas y ratón de tmux respondidos por Rust.
//! El heredado es un puerto muerto: si el frente reenviara, sería 502.
mod support;

use comandos_server::dash::native::tmux::{Program, Tmux};
use std::{ffi::OsString, fs, time::Duration};
use support::{
    FakeLegacy, TestHome, dead_port, front, get, oracle::oracle, request_body, tmux_available,
};

const MONO: &str = r#"{"family": "Monospace", "label": "Monospace del sistema", "a11y": false}"#;

fn new_tmux_sessions(home: &TestHome, names: &[&str]) {
    for name in names {
        let ok = std::process::Command::new("tmux")
            .args(["new-session", "-d", "-s", name, "cat"])
            .env_remove("TMUX")
            .env("TMUX_TMPDIR", home.tmux_dir())
            .status()
            .unwrap()
            .success();
        assert!(ok, "tmux new-session {name}");
    }
}

#[tokio::test]
async fn prefs_get_fills_defaults_after_file_keys() {
    let home = TestHome::new("prefs-get");
    home.write("prefs.json", r#"{"theme": "dia", "favorites": ["a"], "zzz": 1}"#);
    let front = front(&home, dead_port(), home.options()).await;
    let wire = get(front.port, "/prefs").await;
    assert_eq!(wire.status, 200);
    assert_eq!(wire.header("content-type"), Some("application/json"));
    assert_eq!(wire.header("cache-control"), Some("no-store"));
    assert_eq!(
        wire.text(),
        format!(
            r#"{{"theme": "dia", "favorites": ["a"], "zzz": 1, "font_family": "Ubuntu Sans Mono", "font_size": 13, "cursor_shape": "block", "cursor_blink": true, "terminal_padding": 8, "terminal_opacity": 100, "ligatures": true, "button_style": "sutil", "tabs_layout": "row", "fonts": [{MONO}]}}"#
        )
    );
    front.stop().await;
}

#[tokio::test]
async fn prefs_get_lists_installed_catalog_fonts() {
    let home = TestHome::new("prefs-fonts");
    let mut opts = home.options();
    // printf con dos `%.0s` se traga ": family".
    opts.fc_list = Program {
        path: "printf".into(),
        prefix: vec![OsString::from("Hack\\nFira Code,Fira Code Retina\\n%.0s%.0s")],
        env: vec![],
        env_remove: vec![],
    };
    let front = front(&home, dead_port(), opts).await;
    let body = get(front.port, "/prefs").await.text();
    assert!(
        body.ends_with(&format!(
            r#""fonts": [{{"family": "Fira Code", "label": "Fira Code", "a11y": false}}, {{"family": "Hack", "label": "Hack", "a11y": false}}, {MONO}]}}"#
        )),
        "{body}"
    );
    front.stop().await;
}

#[tokio::test]
async fn prefs_set_applies_python_rules_and_writes_python_bytes() {
    let home = TestHome::new("prefs-set");
    home.write("prefs.json", r#"{"favorites": ["b", "local", 3, "b"], "theme": "dia"}"#);
    let front = front(&home, dead_port(), home.options()).await;
    let body = r#"{"favorite": {"session": "s1", "enabled": true}, "nfDismiss": {"k": 0}, "nfSnooze": {"x": 5, "y": true, "z": "n"}, "font_size": 99.5, "terminal_padding": -3, "theme": "neon", "notif_pos": "tr", "font_family": "  Hack  "}"#;
    let wire = request_body(front.port, "POST", "/prefs-set", "", body).await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    assert_eq!(wire.text(), r#"{"ok": true, "favorites": ["b", "s1"]}"#);
    assert_eq!(
        fs::read_to_string(home.hooks().join("prefs.json")).unwrap(),
        r#"{"favorites": ["b", "s1"], "theme": "neon", "font_family": "Hack", "font_size": 28, "cursor_shape": "block", "cursor_blink": true, "terminal_padding": 0, "terminal_opacity": 100, "ligatures": true, "button_style": "sutil", "tabs_layout": "row", "nfDismiss": {"k": 1}, "nfSnooze": {"x": 5.0, "y": 1.0}, "notif_pos": "tr"}"#
    );
    front.stop().await;
}

#[tokio::test]
async fn prefs_set_errors_are_pythons() {
    let home = TestHome::new("prefs-err");
    let front = front(&home, dead_port(), home.options()).await;
    // `ensure_ascii`: los textos con tilde viajan como `á`.
    for (body, status, text) in [
        (r#"{"favorite": {"session": "local", "enabled": true}}"#, 400, r#"{"error": "Favorito inválido"}"#),
        (r#"{"favorite": {"session": "x", "enabled": 1}}"#, 400, r#"{"error": "Favorito inválido"}"#),
        (r#"{"font_size": NaN}"#, 400, r#"{"error": "cannot convert float NaN to integer"}"#),
        (r#"{"font_size": Infinity}"#, 500, r#"{"error": "Error interno del tablero"}"#),
    ] {
        let wire = request_body(front.port, "POST", "/prefs-set", "", body).await;
        assert_eq!((wire.status, wire.text().as_str()), (status, text), "{body}");
    }
    assert!(!home.hooks().join("prefs.json").exists(), "ningún error escribe");
    let many: Vec<String> = (0..200).map(|i| format!("\"f{i}\"")).collect();
    home.write("prefs.json", &format!("{{\"favorites\": [{}]}}", many.join(", ")));
    let wire = request_body(
        front.port,
        "POST",
        "/prefs-set",
        "",
        r#"{"favorite": {"session": "otra", "enabled": true}}"#,
    )
    .await;
    assert_eq!(wire.text(), r#"{"error": "Máximo de 200 favoritos alcanzado"}"#);
    front.stop().await;
}

#[tokio::test]
async fn prefs_set_unwritable_is_503() {
    use std::os::unix::fs::PermissionsExt;
    let home = TestHome::new("prefs-503");
    let front = front(&home, dead_port(), home.options()).await;
    fs::set_permissions(home.hooks(), fs::Permissions::from_mode(0o500)).unwrap();
    let wire = request_body(front.port, "POST", "/prefs-set", "", r#"{"theme": "neon"}"#).await;
    fs::set_permissions(home.hooks(), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(wire.status, 503);
    assert_eq!(wire.text(), r#"{"error": "No se pudieron guardar las preferencias"}"#);
    front.stop().await;
}

#[tokio::test]
async fn tabs_and_history_follow_registry_favorites_and_tmux() {
    if !tmux_available() {
        eprintln!("tmux no está instalado: se salta");
        return;
    }
    let home = TestHome::new("tabs");
    new_tmux_sessions(&home, &["local", "s1", "s2", "hub"]);
    home.write("app-tabs.json", r#"{"s1": "Uno", "s2": "Dos", "hub": "H", "muerta": "M"}"#);
    home.write("prefs.json", r#"{"favorites": ["s2"]}"#);
    home.write(
        "app-tabs-history.json",
        r#"[{"session": "s1"}, {"session": "vieja", "label": 5, "cwd": "rel", "ts": 12.9}, {"session": "s2x", "cwd": "/tmp", "agent": "codex", "reason": "crash", "ts": "7"}, "basura", {"session": "a b"}]"#,
    );
    let front = front(&home, dead_port(), home.options()).await;
    assert_eq!(
        get(front.port, "/tabs").await.text(),
        r#"[{"session": "local", "label": "⌂ local", "closable": false}, {"session": "s2", "label": "Dos"}, {"session": "s1", "label": "Uno"}]"#
    );
    assert_eq!(
        get(front.port, "/tab-history").await.text(),
        r#"[{"session": "vieja", "label": "5", "cwd": "", "agent": "claude", "ts": 12, "reason": "closed", "alive": false}, {"session": "s2x", "label": "s2x", "cwd": "/tmp", "agent": "codex", "ts": 7, "reason": "crash", "alive": false}]"#
    );
    front.stop().await;
}

#[tokio::test]
async fn tab_models_and_active_tab() {
    let home = TestHome::new("models");
    home.write("app-tab-models.json", r#"{"s1": {"panes": [{"pane": "%1"}]}, "mala": [1], "vacia": {}}"#);
    let front = front(&home, dead_port(), home.options()).await;
    assert_eq!(
        get(front.port, "/tab-models?session=s1").await.text(),
        r#"{"session": "s1", "panes": [{"pane": "%1"}]}"#
    );
    assert_eq!(
        get(front.port, "/tab-models?session=vacia").await.text(),
        r#"{"session": "vacia", "panes": []}"#
    );
    assert_eq!(
        get(front.port, "/tab-models").await.text(),
        r#"{"session": "", "panes": []}"#
    );
    let bad = get(front.port, "/tab-models?session=mala").await;
    assert_eq!((bad.status, bad.text().as_str()), (500, r#"{"error": "Error interno del tablero"}"#));
    assert_eq!(get(front.port, "/active-tab").await.text(), "{}");
    home.write("app-tab-active.json", "[1]");
    assert_eq!(get(front.port, "/active-tab").await.status, 500);
    if tmux_available() {
        new_tmux_sessions(&home, &["s1"]);
        home.write("app-tab-active.json", r#"{"session": "s1", "x": 1}"#);
        let text = get(front.port, "/active-tab").await.text();
        assert!(text.starts_with(r#"{"session": "s1", "x": 1, "pane": "%"#), "{text}");
    }
    front.stop().await;
}

#[tokio::test]
async fn tmux_mouse_get_and_set() {
    if !tmux_available() {
        eprintln!("tmux no está instalado: se salta");
        return;
    }
    let home = TestHome::new("mouse");
    new_tmux_sessions(&home, &["s1"]);
    let front = front(&home, dead_port(), home.options()).await;
    assert_eq!(get(front.port, "/tmux-mouse?session=s1").await.text(), r#"{"mouse": "off"}"#);
    let set = request_body(front.port, "POST", "/tmux-mouse", "", r#"{"session": "s1", "enabled": true}"#).await;
    assert_eq!(set.text(), r#"{"ok": true, "mouse": "on"}"#);
    assert_eq!(get(front.port, "/tmux-mouse?session=s1").await.text(), r#"{"mouse": "on"}"#);
    let none = get(front.port, "/tmux-mouse?session=nope").await;
    assert_eq!((none.status, none.text().as_str()), (404, r#"{"error": "No hay sesion tmux 'nope'"}"#));
    let bad = get(front.port, "/tmux-mouse?session=a%20b").await;
    assert_eq!((bad.status, bad.text().as_str()), (400, r#"{"error": "Nombre de sesion invalido"}"#));
    let typed = request_body(front.port, "POST", "/tmux-mouse", "", r#"{"session": 5}"#).await;
    assert_eq!(typed.status, 500, "SESSION_RE.match(int) es TypeError");
    front.stop().await;
}

#[tokio::test]
async fn tmux_missing_messages() {
    let home = TestHome::new("tmux-missing");
    let mut opts = home.options();
    opts.tmux = Tmux { program: Program::named("/no-existe/tmux"), timeout: Duration::from_secs(5) };
    let front = front(&home, dead_port(), opts).await;
    let caught = get(front.port, "/tmux-mouse?session=s1").await;
    assert_eq!(
        (caught.status, caught.text().as_str()),
        (500, r#"{"error": "[Errno 2] No such file or directory: 'tmux'"}"#)
    );
    let uncaught = get(front.port, "/tabs").await;
    assert_eq!((uncaught.status, uncaught.text().as_str()), (500, r#"{"error": "Error interno del tablero"}"#));
    front.stop().await;
}

#[tokio::test]
async fn tmux_hung_times_out() {
    let home = TestHome::new("tmux-hung");
    let mut opts = home.options();
    opts.tmux = Tmux {
        program: Program {
            path: "tail".into(),
            prefix: vec!["-f".into(), "/dev/null".into(), "--".into()],
            env: vec![],
            env_remove: vec![],
        },
        timeout: Duration::from_millis(300),
    };
    let front = front(&home, dead_port(), opts).await;
    let uncaught = get(front.port, "/tabs").await;
    assert_eq!(uncaught.status, 504);
    let caught = get(front.port, "/tmux-mouse?session=s1").await;
    assert_eq!(
        caught.text(),
        r#"{"error": "Command '['tmux', 'has-session', '-t', '=s1']' timed out after 0.3 seconds"}"#
    );
    front.stop().await;
}

#[tokio::test]
async fn exotic_inputs_decline_to_legacy() {
    let home = TestHome::new("light-exotic");
    let legacy = FakeLegacy::start().await;
    home.write("app-tabs-history.json", r#"[{"session": "s", "label": 1.5}]"#);
    home.write("prefs.json", r#"{"favorites": "abc"}"#);
    home.write("app-tab-active.json", r#"{"session": 2.5}"#);
    let front = front(&home, legacy.port, home.options()).await;
    for target in ["/tab-history", "/tab-models?session=%FF", "/active-tab"] {
        assert_eq!(get(front.port, target).await.text(), r#"{"legacy": true}"#, "{target}");
    }
    let wire = request_body(
        front.port,
        "POST",
        "/prefs-set",
        "",
        r#"{"favorite": {"session": "s", "enabled": true}}"#,
    )
    .await;
    assert_eq!(wire.text(), r#"{"legacy": true}"#);
    assert_eq!(
        fs::read_to_string(home.hooks().join("prefs.json")).unwrap(),
        r#"{"favorites": "abc"}"#,
        "declinar nunca escribe"
    );
    assert_eq!(legacy.requests().len(), 4);
    front.stop().await;
}

#[tokio::test]
async fn light_routes_match_python_oracle() {
    let home = TestHome::new("light-oracle");
    if tmux_available() {
        new_tmux_sessions(&home, &["local", "s1"]);
    }
    home.write("app-tabs.json", r#"{"s1": "Uno ñ"}"#);
    home.write("prefs.json", r#"{"favorites": ["s1"], "theme": "neon"}"#);
    home.write("app-tab-models.json", r#"{"s1": {"panes": [{"pane": "%0", "model": "ñ"}]}}"#);
    home.write("app-tab-active.json", r#"{"session": "s1"}"#);
    home.write("app-tabs-history.json", r#"[{"session": "vieja", "ts": 3}]"#);
    let Some(py) = oracle(&home).await else { return };
    let mut opts = home.options();
    opts.fc_list = Program::named("fc-list");
    let front = front(&home, dead_port(), opts).await;
    for target in [
        "/prefs",
        "/tabs",
        "/tab-history",
        "/tab-models?session=s1",
        "/active-tab",
        "/tmux-mouse?session=s1",
        "/tmux-mouse?session=nope",
    ] {
        let (a, b) = (get(py.port, target).await, get(front.port, target).await);
        assert_eq!(
            (a.status, a.header("content-type"), a.text()),
            (b.status, b.header("content-type"), b.text()),
            "{target}"
        );
    }
    front.stop().await;
}
```

- [ ] **Step 3: Ejecutar y ver el fallo**

Run: `$C test -p comandos-server --test dash_native_light -j 6`
Expected: FAIL: las rutas aún se reenvían al puerto muerto (502 `Servidor heredado no disponible` en vez de 200).

- [ ] **Step 4: Implementar `light.rs`**

Crear `crates/comandos-server/src/dash/native/light.rs`:

```rust
//! D. Lecturas ligeras y preferencias, rama a rama de `bin/cc-dash`.
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb, files, py, query::Query, reply,
    tmux::{Tmux, TmuxError, run_program},
};
use crate::{HandlerError, Request};
use comandos_core::json::{response_dumps, truthy};
use http::StatusCode;
use serde_json::{Map, Value, json};
use std::{
    collections::HashSet,
    path::Path,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightRoute {
    Prefs,
    PrefsSet,
    Tabs,
    TabHistory,
    TabModels,
    ActiveTab,
    TmuxMouseGet,
    TmuxMouseSet,
}

const fn entry(verb: Verb, key: Key, route: LightRoute) -> Entry {
    Entry { verb, key, route: NativeRoute::Light(route) }
}

pub const ROUTES: &[Entry] = &[
    entry(Verb::Get, Key::Path("/prefs"), LightRoute::Prefs),
    entry(Verb::Get, Key::Path("/tabs"), LightRoute::Tabs),
    entry(Verb::Get, Key::Path("/tab-history"), LightRoute::TabHistory),
    entry(Verb::Get, Key::Path("/tab-models"), LightRoute::TabModels),
    entry(Verb::Get, Key::Path("/active-tab"), LightRoute::ActiveTab),
    entry(Verb::Get, Key::ExactOrQuery("/tmux-mouse"), LightRoute::TmuxMouseGet),
    entry(Verb::Post, Key::Raw("/prefs-set"), LightRoute::PrefsSet),
    entry(Verb::Post, Key::Raw("/tmux-mouse"), LightRoute::TmuxMouseSet),
];

pub async fn answer(native: &Native, route: LightRoute, request: &Request) -> Answer {
    let hooks = native.options().hooks.as_path();
    let tmux = &native.options().tmux;
    match route {
        LightRoute::Prefs => {
            let mut prefs = read_prefs(hooks);
            prefs.insert("fonts".into(), fonts(native).await);
            reply(StatusCode::OK, &Value::Object(prefs))
        }
        LightRoute::PrefsSet => prefs_set(native, data(request)?).await,
        LightRoute::Tabs => tabs(hooks, tmux).await,
        LightRoute::TabHistory => tab_history(hooks, tmux).await,
        LightRoute::TabModels => tab_models(hooks, &request.target),
        LightRoute::ActiveTab => active_tab(hooks, tmux).await,
        LightRoute::TmuxMouseGet => {
            let query = Query::parse(&request.target)?;
            let sess = query.first("session").unwrap_or("");
            if !py::is_session(sess) {
                return error(StatusCode::BAD_REQUEST, "Nombre de sesion invalido");
            }
            match get_tmux_mouse(tmux, sess).await? {
                Ok(on) => reply(StatusCode::OK, &json!({"mouse": if on { "on" } else { "off" }})),
                Err(message) => mouse_error(&message),
            }
        }
        LightRoute::TmuxMouseSet => {
            let data = data(request)?;
            // `SESSION_RE.match(sess)` con un no-str es TypeError → 500.
            let sess = match data.get("session") {
                None => "",
                Some(Value::String(s)) => s.as_str(),
                Some(_) => return Err(HandlerError::Failure.into()),
            };
            if !py::is_session(sess) {
                return error(StatusCode::BAD_REQUEST, "Nombre de sesion invalido");
            }
            let enabled = data.get("enabled").is_none_or(truthy);
            match set_tmux_mouse(tmux, sess, enabled).await? {
                None => reply(
                    StatusCode::OK,
                    &json!({"ok": true, "mouse": if enabled { "on" } else { "off" }}),
                ),
                Some(message) => mouse_error(&message),
            }
        }
    }
}

/// El transporte garantiza un objeto en todo POST admitido.
pub(crate) fn data(request: &Request) -> Result<&Map<String, Value>, Fault> {
    request
        .data
        .as_ref()
        .and_then(Value::as_object)
        .ok_or(Fault::Error(HandlerError::Failure))
}

pub(crate) fn error(status: StatusCode, message: &str) -> Answer {
    reply(status, &json!({"error": message}))
}

fn mouse_error(message: &str) -> Answer {
    let status = if message.starts_with("No hay sesion") {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    error(status, message)
}

// ---------------------------------------------------------------- preferencias

fn prefs_defaults() -> [(&'static str, Value); 10] {
    [
        ("theme", json!("noche")),
        ("font_family", json!("Ubuntu Sans Mono")),
        ("font_size", json!(13)),
        ("cursor_shape", json!("block")),
        ("cursor_blink", json!(true)),
        ("terminal_padding", json!(8)),
        ("terminal_opacity", json!(100)),
        ("ligatures", json!(true)),
        ("button_style", json!("sutil")),
        ("tabs_layout", json!("row")),
    ]
}

/// `read_prefs` (7754).
pub fn read_prefs(hooks: &Path) -> Map<String, Value> {
    let mut prefs = match files::read_json(&hooks.join("prefs.json")) {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    for (key, value) in prefs_defaults() {
        prefs.entry(key).or_insert(value);
    }
    prefs
}

const FONT_CATALOG: [(&str, &str, bool); 15] = [
    ("Ubuntu Sans Mono", "Ubuntu Sans Mono · la de Ubuntu (default)", false),
    ("JetBrainsMono Nerd Font Mono", "JetBrains Mono · con iconos (bundle)", false),
    ("Atkinson Hyperlegible Mono", "Atkinson Hyperlegible Mono", true),
    ("Intel One Mono", "Intel One Mono", true),
    ("JetBrains Mono", "JetBrains Mono", false),
    ("Monaspace Neon Var", "Monaspace Neon", false),
    ("Monaspace Argon Var", "Monaspace Argon", false),
    ("Geist Mono", "Geist Mono", false),
    ("Cascadia Code", "Cascadia Code", false),
    ("Fira Code", "Fira Code", false),
    ("Ubuntu Mono", "Ubuntu Mono · clásica 2011", false),
    ("Iosevka", "Iosevka", false),
    ("Hack", "Hack", false),
    ("Source Code Pro", "Source Code Pro", false),
    ("DejaVu Sans Mono", "DejaVu Sans Mono", false),
];

/// `_installed_font_families` (7724) + `installed_terminal_fonts` (7745).
async fn fonts(native: &Native) -> Value {
    let cached = native
        .fonts
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .filter(|(at, _)| at.elapsed() < Duration::from_secs(60))
        .map(|(_, fams)| fams.clone());
    let fams = match cached {
        Some(fams) => fams,
        None => {
            let mut fams = HashSet::new();
            // El Python lee `.stdout` aunque fc-list falle; una excepción deja el conjunto vacío.
            if let Ok(out) =
                run_program(&native.options().fc_list, &[":", "family"], Duration::from_secs(5)).await
            {
                for line in py::splitlines(&out.stdout) {
                    for fam in line.split(',') {
                        let fam = py::strip(fam);
                        if !fam.is_empty() {
                            fams.insert(fam.to_owned());
                        }
                    }
                }
            }
            *native.fonts.lock().unwrap_or_else(|p| p.into_inner()) =
                Some((Instant::now(), fams.clone()));
            fams
        }
    };
    let mut rows: Vec<Value> = FONT_CATALOG
        .iter()
        .filter(|(family, _, _)| fams.contains(*family))
        .map(|(family, label, a11y)| json!({"family": family, "label": label, "a11y": a11y}))
        .collect();
    rows.push(json!({"family": "Monospace", "label": "Monospace del sistema", "a11y": false}));
    Value::Array(rows)
}

enum PrefsFault {
    /// `ValueError` → 400 con su texto.
    Bad(&'static str),
    Fault(Fault),
}

impl From<Fault> for PrefsFault {
    fn from(fault: Fault) -> Self {
        PrefsFault::Fault(fault)
    }
}

/// `prefs.get("favorites", [])` iterada por el Python.
fn stored_favorites(prefs: &Map<String, Value>) -> Result<Vec<Value>, Fault> {
    match prefs.get("favorites") {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => Ok(items.clone()),
        // Iterar un str da caracteres y un dict sus claves: raro, se declina.
        Some(Value::String(_) | Value::Object(_)) => Err(Fault::Decline),
        // `for x in None/5/True`: TypeError no capturado.
        Some(_) => Err(HandlerError::Failure.into()),
    }
}

fn dedupe(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    items.into_iter().filter(|s| seen.insert(s.clone())).collect()
}

/// `float(v)` para un `int`/`float`/`bool` de JSON, como valor JSON.
fn py_float_value(value: &Value) -> Result<Option<Value>, PrefsFault> {
    let as_number = |x: f64| {
        serde_json::Number::from_f64(x)
            .map(Value::Number)
            .ok_or(PrefsFault::Fault(Fault::Decline))
    };
    match value {
        Value::Bool(b) => as_number(if *b { 1.0 } else { 0.0 }).map(Some),
        Value::Number(n) => {
            let raw = n.as_str();
            if matches!(raw, "NaN" | "Infinity" | "-Infinity") || raw.contains(['.', 'e', 'E']) {
                // Ya es float: `float(x)` es la identidad y `json.dumps` lo reescribe con su repr.
                Ok(Some(value.clone()))
            } else {
                let x: f64 = raw.parse().map_err(|_| Fault::Decline)?;
                if !x.is_finite() {
                    // `float(10**400)`: OverflowError no capturado.
                    return Err(Fault::Error(HandlerError::Failure).into());
                }
                as_number(x).map(Some)
            }
        }
        _ => Ok(None),
    }
}

/// `max(lo, min(hi, int(x)))` para `isinstance(x, (int, float))`.
fn clamped(value: Option<&Value>, lo: i64, hi: i64) -> Result<Option<i64>, PrefsFault> {
    let n = match value {
        Some(Value::Bool(b)) => i64::from(*b),
        Some(Value::Number(n)) => match n.as_str() {
            "NaN" => return Err(PrefsFault::Bad("cannot convert float NaN to integer")),
            "Infinity" | "-Infinity" => return Err(Fault::Error(HandlerError::Failure).into()),
            raw if !raw.contains(['.', 'e', 'E']) => raw.parse::<i64>().unwrap_or(if raw.starts_with('-') {
                i64::MIN
            } else {
                i64::MAX
            }),
            raw => {
                let x: f64 = raw.parse().map_err(|_| Fault::Decline)?;
                if x.is_infinite() {
                    return Err(Fault::Error(HandlerError::Failure).into());
                }
                let t = x.trunc();
                if t >= i64::MAX as f64 {
                    i64::MAX
                } else if t <= i64::MIN as f64 {
                    i64::MIN
                } else {
                    t as i64
                }
            }
        },
        _ => return Ok(None),
    };
    Ok(Some(n.clamp(lo, hi)))
}

fn is_one_of(value: Option<&Value>, options: &[&str]) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|s| options.contains(s))
        .map(str::to_owned)
}

/// `update_prefs` (7768) sin la escritura: mismas comprobaciones y mismo
/// orden de asignación (las claves nuevas se añaden al final, como en un dict).
fn update_prefs(
    mut prefs: Map<String, Value>,
    data: &Map<String, Value>,
) -> Result<Map<String, Value>, PrefsFault> {
    if let Some(patch) = data.get("favorite") {
        let session = patch.get("session").and_then(Value::as_str).filter(|s| {
            (1..=160).contains(&s.chars().count())
                && *s != "local"
                && !s.chars().any(|c| (c as u32) < 32)
        });
        let enabled = patch.get("enabled").and_then(Value::as_bool);
        let (Some(session), Some(enabled)) = (session, enabled) else {
            return Err(PrefsFault::Bad("Favorito inválido"));
        };
        let mut favorites = dedupe(
            stored_favorites(&prefs)?
                .into_iter()
                .filter_map(|x| x.as_str().filter(|s| *s != "local").map(str::to_owned)),
        );
        if enabled && !favorites.iter().any(|f| f == session) {
            if favorites.len() >= 200 {
                return Err(PrefsFault::Bad("Máximo de 200 favoritos alcanzado"));
            }
            favorites.push(session.to_owned());
        } else if !enabled {
            favorites.retain(|f| f != session);
        }
        prefs.insert("favorites".into(), json!(favorites));
    }
    if let Some(Value::Object(dismiss)) = data.get("nfDismiss") {
        let mut out = Map::new();
        for key in dismiss.keys().take(600) {
            out.insert(py::take_chars(key, 160), json!(1));
        }
        prefs.insert("nfDismiss".into(), Value::Object(out));
    }
    if let Some(Value::Object(snooze)) = data.get("nfSnooze") {
        let mut out = Map::new();
        for (key, value) in snooze.iter().take(200) {
            if let Some(number) = py_float_value(value)? {
                out.insert(py::take_chars(key, 160), number);
            }
        }
        prefs.insert("nfSnooze".into(), Value::Object(out));
    }
    if !data.contains_key("favorite")
        && let Some(Value::Array(list)) = data.get("favorites")
    {
        let kept = dedupe(
            list.iter()
                .filter_map(Value::as_str)
                .filter(|s| !s.is_empty() && *s != "local")
                .map(|s| py::take_chars(s, 160)),
        );
        prefs.insert("favorites".into(), json!(kept.into_iter().take(200).collect::<Vec<_>>()));
    }
    const THEMES: [&str; 9] = [
        "noche", "dia", "calido", "termius", "bruno", "superglass", "neon", "contraste", "ubuntu",
    ];
    if let Some(theme) = is_one_of(data.get("theme"), &THEMES) {
        prefs.insert("theme".into(), json!(theme));
    }
    if let Some(style) = is_one_of(data.get("button_style"), &["sutil", "arcade", "tecla", "pixel", "consola"]) {
        prefs.insert("button_style".into(), json!(style));
    }
    if let Some(layout) = is_one_of(data.get("tabs_layout"), &["row", "rows"]) {
        prefs.insert("tabs_layout".into(), json!(layout));
    }
    if let Some(Value::String(family)) = data.get("font_family") {
        let family = py::take_chars(family, 120);
        let family = py::strip(&family);
        let family = if family.is_empty() { "Ubuntu Sans Mono" } else { family };
        prefs.insert("font_family".into(), json!(family));
    }
    if let Some(size) = clamped(data.get("font_size"), 8, 28)? {
        prefs.insert("font_size".into(), json!(size));
    }
    if let Some(shape) = is_one_of(data.get("cursor_shape"), &["block", "ibeam", "underline"]) {
        prefs.insert("cursor_shape".into(), json!(shape));
    }
    if let Some(Value::Bool(blink)) = data.get("cursor_blink") {
        prefs.insert("cursor_blink".into(), json!(blink));
    }
    if let Some(padding) = clamped(data.get("terminal_padding"), 0, 40)? {
        prefs.insert("terminal_padding".into(), json!(padding));
    }
    if let Some(opacity) = clamped(data.get("terminal_opacity"), 30, 100)? {
        prefs.insert("terminal_opacity".into(), json!(opacity));
    }
    if let Some(Value::Bool(ligatures)) = data.get("ligatures") {
        prefs.insert("ligatures".into(), json!(ligatures));
    }
    if let Some(pos) = is_one_of(data.get("notif_pos"), &["tl", "tr", "bl", "br", "free"]) {
        prefs.insert("notif_pos".into(), json!(pos));
    }
    Ok(prefs)
}

async fn prefs_set(native: &Native, data: &Map<String, Value>) -> Answer {
    // `_PREFS_LOCK`: un solo leer-modificar-escribir a la vez en este proceso.
    let _guard = native.prefs_lock.lock().await;
    let path = native.options().hooks.join("prefs.json");
    let prefs = match update_prefs(read_prefs(&native.options().hooks), data) {
        Ok(prefs) => Value::Object(prefs),
        Err(PrefsFault::Bad(message)) => return error(StatusCode::BAD_REQUEST, message),
        Err(PrefsFault::Fault(fault)) => return Err(fault),
    };
    // Si el codificador portado no lo puede escribir, el Python sí: se declina antes de tocar nada.
    response_dumps(&prefs).map_err(|_| Fault::Decline)?;
    let favorites = prefs.get("favorites").cloned().unwrap_or_else(|| json!([]));
    let written = tokio::task::spawn_blocking(move || files::write_json_atomic(&path, &prefs))
        .await
        .map_err(|_| HandlerError::Failure)?;
    match written {
        Ok(()) => reply(StatusCode::OK, &json!({"ok": true, "favorites": favorites})),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE, "No se pudieron guardar las preferencias"),
    }
}

// ---------------------------------------------------------------- pestañas

pub const HIDDEN_SESSIONS: [&str; 3] = ["hub", "local", "control"];

/// `tab_labels` (6430): pares `(sesión, etiqueta)` en el orden del archivo.
pub fn tab_labels(hooks: &Path) -> Vec<(String, String)> {
    match files::read_json(&hooks.join("app-tabs.json")) {
        Some(Value::Object(map)) => map
            .into_iter()
            .filter_map(|(k, v)| match v {
                Value::String(s) if !s.is_empty() => Some((k, s)),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `set(favorites)` tal como lo usa `ordered_tab_keys`.
pub fn favorites_set(prefs: &Map<String, Value>) -> Result<HashSet<String>, Fault> {
    let mut set = HashSet::new();
    for item in stored_favorites(prefs)? {
        match item {
            Value::String(s) => {
                set.insert(s);
            }
            // Listas y objetos no son hashables: TypeError no capturado.
            Value::Array(_) | Value::Object(_) => return Err(HandlerError::Failure.into()),
            // Números, booleanos y None nunca igualan a un nombre de sesión.
            _ => {}
        }
    }
    Ok(set)
}

/// `lib/session_tabs.ordered_tab_keys`: orden estable local → favoritas → resto.
pub fn ordered_tab_keys<'a>(keys: &'a [(String, String)], favorites: &HashSet<String>) -> Vec<&'a (String, String)> {
    let mut out: Vec<_> = keys.iter().collect();
    out.sort_by_key(|(key, _)| {
        if key == "local" {
            0
        } else if favorites.contains(key) {
            1
        } else {
            2
        }
    });
    out
}

/// `tmux_sessions` (5876): las excepciones no se capturan.
pub async fn tmux_sessions(tmux: &Tmux) -> Result<HashSet<String>, Fault> {
    let out = tmux
        .run(&["list-sessions", "-F", "#{session_name}"])
        .await
        .map_err(|e| Fault::Error(e.uncaught()))?;
    if !out.ok {
        return Ok(HashSet::new());
    }
    Ok(py::splitlines(&out.stdout)
        .into_iter()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect())
}

async fn tabs(hooks: &Path, tmux: &Tmux) -> Answer {
    let live = tmux_sessions(tmux).await?;
    let mut out = Vec::new();
    if live.contains("local") {
        out.push(json!({"session": "local", "label": "⌂ local", "closable": false}));
    }
    let labels = tab_labels(hooks);
    let favorites = favorites_set(&read_prefs(hooks))?;
    for (sess, label) in ordered_tab_keys(&labels, &favorites) {
        if live.contains(sess) && !HIDDEN_SESSIONS.contains(&sess.as_str()) {
            out.push(json!({"session": sess, "label": label}));
        }
    }
    reply(StatusCode::OK, &Value::Array(out))
}

/// `str(it.get(key) or default)[:n]`.
fn text_or(item: &Map<String, Value>, key: &str, default: &str, n: usize) -> Result<String, Fault> {
    let text = match item.get(key).filter(|v| truthy(v)) {
        None => default.to_owned(),
        Some(v) => py::str_scalar(v).ok_or(Fault::Decline)?,
    };
    Ok(py::take_chars(&text, n))
}

/// `read_tab_history` (5318).
fn read_tab_history(hooks: &Path) -> Result<Vec<Map<String, Value>>, Fault> {
    let Some(Value::Array(items)) = files::read_json(&hooks.join("app-tabs-history.json")) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for item in items.iter().take(80) {
        let Value::Object(item) = item else { continue };
        let session = match item.get("session") {
            None => String::new(),
            Some(v) => match py::str_scalar(v) {
                Some(s) => s,
                // repr de lista/objeto lleva corchetes o llaves: nunca casa SESSION_RE.
                None if v.is_array() || v.is_object() => continue,
                None => return Err(Fault::Decline),
            },
        };
        if !py::is_session(&session) {
            continue;
        }
        let cwd = match item.get("cwd") {
            Some(Value::String(s)) if s.starts_with('/') => s.clone(),
            _ => String::new(),
        };
        let ts = match item.get("ts").filter(|v| truthy(v)) {
            None => 0,
            Some(v) => match py::int_of(v) {
                Ok(n) => n,
                Err(py::Conversion::Exotic) => return Err(Fault::Decline),
                Err(_) => return Err(HandlerError::Failure.into()),
            },
        };
        let mut row = Map::new();
        row.insert("label".into(), json!(text_or(item, "label", &session, 80)?));
        row.insert("cwd".into(), json!(cwd));
        row.insert("agent".into(), json!(text_or(item, "agent", "claude", 16)?));
        row.insert("ts".into(), json!(ts));
        row.insert("reason".into(), json!(text_or(item, "reason", "closed", 32)?));
        let mut full = Map::new();
        full.insert("session".into(), json!(session));
        full.extend(row);
        out.push(full);
    }
    Ok(out)
}

async fn tab_history(hooks: &Path, tmux: &Tmux) -> Answer {
    // Se lee antes que tmux solo para poder declinar pronto; un 500 del
    // historial se devuelve después de tmux, en el orden del Python.
    let history = read_tab_history(hooks);
    if matches!(history, Err(Fault::Decline)) {
        return Err(Fault::Decline);
    }
    let labels: HashSet<String> = tab_labels(hooks).into_iter().map(|(k, _)| k).collect();
    let live = tmux_sessions(tmux).await?;
    let mut out = Vec::new();
    for mut item in history? {
        let session = item["session"].as_str().unwrap_or_default().to_owned();
        if labels.contains(&session) {
            continue;
        }
        item.insert("alive".into(), json!(live.contains(&session)));
        out.push(Value::Object(item));
    }
    out.truncate(40);
    reply(StatusCode::OK, &Value::Array(out))
}

fn tab_models(hooks: &Path, target: &str) -> Answer {
    let query = Query::parse(target)?;
    let sess = query.first("session").unwrap_or("");
    let models = files::read_json(&hooks.join("app-tab-models.json")).unwrap_or_else(|| json!({}));
    let entry = models.as_object().and_then(|m| m.get(sess));
    let panes = match entry {
        None => json!([]),
        Some(e) if !truthy(e) => json!([]),
        Some(Value::Object(o)) => o.get("panes").filter(|p| truthy(p)).cloned().unwrap_or_else(|| json!([])),
        // `(entry or {}).get(...)` sobre lista/str/número: AttributeError.
        Some(_) => return Err(HandlerError::Failure.into()),
    };
    reply(StatusCode::OK, &json!({"session": sess, "panes": panes}))
}

async fn active_tab(hooks: &Path, tmux: &Tmux) -> Answer {
    let mut active =
        files::read_json(&hooks.join("app-tab-active.json")).unwrap_or_else(|| json!({}));
    let Value::Object(map) = &mut active else {
        return Err(HandlerError::Failure.into());
    };
    let sess = match map.get("session").filter(|v| truthy(v)) {
        None => String::new(),
        Some(v) => py::str_scalar(v).ok_or(Fault::Decline)?,
    };
    if py::is_session(&sess) {
        let uncaught = |e: TmuxError| Fault::Error(e.uncaught());
        let target = format!("={sess}");
        if tmux.run(&["has-session", "-t", &target]).await.map_err(uncaught)?.ok {
            let window = format!("={sess}:");
            let rp = tmux
                .run(&["display-message", "-p", "-t", &window, "#{pane_id}"])
                .await
                .map_err(uncaught)?;
            let pane = if rp.ok { py::strip(&rp.stdout).to_owned() } else { String::new() };
            if py::is_pane(&pane) {
                map.insert("pane".into(), json!(pane));
            }
        }
    }
    reply(StatusCode::OK, &active)
}

/// `str(exc).strip() or "tmux fallo"`; sin texto seguro, declinar.
fn caught(error: &TmuxError) -> Result<String, Fault> {
    let message = error.python_message().ok_or(Fault::Decline)?;
    let message = py::strip(&message);
    Ok(if message.is_empty() { "tmux fallo".into() } else { message.to_owned() })
}

fn stderr_or(stderr: &str) -> String {
    let text = py::strip(stderr);
    if text.is_empty() { "tmux fallo".into() } else { text.to_owned() }
}

/// `get_tmux_mouse` (5883).
async fn get_tmux_mouse(tmux: &Tmux, sess: &str) -> Result<Result<bool, String>, Fault> {
    let exists = match tmux.run(&["has-session", "-t", &format!("={sess}")]).await {
        Ok(out) => out,
        Err(e) => return Ok(Err(caught(&e)?)),
    };
    if !exists.ok {
        return Ok(Err(format!("No hay sesion tmux '{sess}'")));
    }
    let shown = match tmux.run(&["show-options", "-A", "-v", "-t", sess, "mouse"]).await {
        Ok(out) => out,
        Err(e) => return Ok(Err(caught(&e)?)),
    };
    if !shown.ok {
        return Ok(Err(stderr_or(&shown.stderr)));
    }
    Ok(Ok(py::strip(&shown.stdout) == "on"))
}

/// `set_tmux_mouse` (5905): `None` si fue bien.
async fn set_tmux_mouse(tmux: &Tmux, sess: &str, enabled: bool) -> Result<Option<String>, Fault> {
    let exists = match tmux.run(&["has-session", "-t", &format!("={sess}")]).await {
        Ok(out) => out,
        Err(e) => return Ok(Some(caught(&e)?)),
    };
    if !exists.ok {
        return Ok(Some(format!("No hay sesion tmux '{sess}'")));
    }
    let value = if enabled { "on" } else { "off" };
    let set = match tmux.run(&["set-option", "-t", sess, "mouse", value]).await {
        Ok(out) => out,
        // Un plazo vencido siempre tiene texto; solo un fallo de arranque (sin
        // efecto) puede declinar.
        Err(e) => return Ok(Some(caught(&e)?)),
    };
    Ok((!set.ok).then(|| stderr_or(&set.stderr)))
}
```

En `native/mod.rs`:
- `pub mod light;`
- `NativeRoute` pasa a `pub enum NativeRoute { Light(light::LightRoute) }` (sin variantes vacías).
- `const TABLES: &[&[Entry]] = &[light::ROUTES];`
- Ayudante compartido:

```rust
pub fn reply(status: http::StatusCode, value: &serde_json::Value) -> Answer {
    Reply::json(status, value).map_err(Fault::from)
}
```

- Campos nuevos en `Native` (inicializados en `new`):

```rust
    /// `_PREFS_LOCK` del Python.
    pub(crate) prefs_lock: tokio::sync::Mutex<()>,
    /// `_FONT_CACHE`: familias de `fc-list` y cuándo se leyeron.
    pub(crate) fonts: Mutex<Option<(std::time::Instant, std::collections::HashSet<String>)>>,
```

- `answer` pasa a:

```rust
    async fn answer(&self, route: NativeRoute, request: &Request) -> Answer {
        match route {
            NativeRoute::Light(route) => light::answer(self, route, request).await,
        }
    }
```

(`Option::is_none_or` es estable desde Rust 1.82.)

- [ ] **Step 5: Ver pasar el dominio**

Run: `$C test -p comandos-server --test dash_native_light -j 6`
Expected: PASS (12 pruebas; las de tmux y la del oráculo se saltan con aviso si falta `tmux` o `python3`).

- [ ] **Step 6: Líneas de fixture**

Añadir al final de `xtask/parity/frente.jsonl`:

```
# --- 2b · D: lecturas ligeras y preferencias (nativas; el orden importa: los POST escriben)
{"name":"d-prefs","method":"GET","path":"/prefs","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"d-tabs","method":"GET","path":"/tabs","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"d-tab-history","method":"GET","path":"/tab-history","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"d-tab-models","method":"GET","path":"/tab-models?session=local","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"d-tab-models-vacia","method":"GET","path":"/tab-models","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"d-active-tab","method":"GET","path":"/active-tab","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"d-tmux-mouse","method":"GET","path":"/tmux-mouse?session=local","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"d-tmux-mouse-nadie","method":"GET","path":"/tmux-mouse?session=no-existe","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"d-tmux-mouse-mala","method":"GET","path":"/tmux-mouse?session=a%20b","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"d-tmux-mouse-post","method":"POST","path":"/tmux-mouse","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"local","enabled":true},"volatile":[],"expect":"same"}
{"name":"d-prefs-set-malo","method":"POST","path":"/prefs-set","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"favorite":{"session":"local","enabled":true}},"volatile":[],"expect":"same"}
{"name":"d-prefs-set","method":"POST","path":"/prefs-set","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"theme":"neon","font_size":14.7,"nfSnooze":{"x":3}},"volatile":[],"expect":"same"}
{"name":"d-prefs-tras-set","method":"GET","path":"/prefs","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"d-prefs-remoto-sin-token","method":"GET","path":"/prefs","headers":{"Host":"x.ts.net","X-Forwarded-For":"100.64.0.9"},"body":null,"volatile":[],"expect":"same"}
```

- [ ] **Step 7: Paridad**

Run: `.build/target/debug/xtask parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks --state-db ~/.local/state/comandos/app-state.sqlite3 --comandos .build/target/debug/comandos` (tras `$C build -p comandos-cli -p xtask -j 6`)
Expected: 0 DIFF; en «reenviadas al heredado» ya no aparecen `GET /prefs`, `GET /tabs`, `GET /tab-history`, `GET /tab-models`, `GET /active-tab`, `GET /tmux-mouse`, `POST /tmux-mouse`, `POST /prefs-set`. Repetir con `--no-native`: 0 DIFF y las ocho vuelven a la lista.

- [ ] **Step 8: fmt, clippy, commit**

```bash
$C fmt --all -- --check
$C clippy --workspace --all-targets -j 6 -- -D warnings
git add crates/comandos-server/src/dash/native/mod.rs crates/comandos-server/src/dash/native/light.rs \
  crates/comandos-server/tests/support/mod.rs crates/comandos-server/tests/support/oracle.rs \
  crates/comandos-server/tests/dash_native_light.rs xtask/parity/frente.jsonl
git commit -m "feat(dash): dominio D nativo — prefs, prefs-set, tabs, tab-history, tab-models, active-tab, tmux-mouse

Rama a rama de bin/cc-dash: mismo orden de claves, mismos textos de error,
mismos 504/500 cuando tmux cuelga o falta. Entradas que el Rust no reproduce
con certeza (flotantes en etiquetas, favoritos que no son lista, U+FFFD)
declinan al heredado sin escribir nada.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Dominio B — eventos y marcas (4 rutas sobre `EventRoutes`)

Desplegable sola: GET/POST `/work-marks` y GET/POST `/events/v2` pasan a Rust. La lógica ya existe y está fijada contra el Python en `crates/comandos-server/src/events_routes.rs` (`tests/events_routes.rs`, `events_routes_socket.rs` y los fixtures `events_routes_{marks,query,reference}.json`); esta tarea la monta sobre el worker de la base y comprueba la paridad que esos fixtures no cubren: la importación única de `events.jsonl` heredado entre los dos procesos y la autenticación de productor interno a través del frente.

Correspondencia con el Python (todas en el mismo hilo del worker, como el `workspace_store().conn` del Python):

- **GET `/work-marks`** (8712, `work_marks_payload` 8117): `{"marks","panes","activity"}`. Solo lee.
- **POST `/work-marks`** (8867, `work_marks_write` 8130): `{"mark": …}`; 409 `{"error": "Revisión desactualizada", "current": …}`; 400 `str(exc) or "Marca inválida"`.
- **GET `/events/v2`** (8714, `events_v2_list` 8081): `after`/`limit` con `[0-9]{1,15}` o 400 `{name} inválido`; `{"events","nextAfter","latest"}` + `turns` con `turns=1`. **Efecto lateral** (§9.7): la primera llamada del proceso importa `~/.claude/hooks/events.jsonl` con `event_intake.import_legacy`; la marca `events.legacy_import` en `workspace_meta` hace que la importación ocurra una sola vez entre los dos procesos (`comandos_runtime::legacy::import_legacy` usa la misma marca). El nativo lo reproduce tal cual.
- **POST `/events/v2`** (8870, `events_v2_record` 8142): 403 `Solo productores internos de este equipo` si no es `_internal_producer` (8322: loopback, sin `X-Forwarded-For`, sin `Origin`, `X-Comandos-Token` correcto — el transporte ya calcula `Request.internal_producer` igual); 200 `{"event": …}`, 202 `{"ignored": true}`, 400 `str(exc) or "Evento inválido"`. Mismo efecto lateral de importación. Sin otros efectos (ni popups ni push: los dispara cc-notifyd leyendo la base).

**Files:**
- Create: `crates/comandos-server/src/dash/native/events.rs`
- Modify: `crates/comandos-server/src/dash/native/state.rs`, `crates/comandos-server/src/dash/native/mod.rs`
- Create: `crates/comandos-server/tests/dash_native_events.rs`
- Modify: `xtask/parity/frente.jsonl`

**Interfaces:**
- Produces: `events::{ROUTES, answer}`, `NativeRoute::Events`, `StateBackend::events(&mut self, legacy: &Path, &Request)`.
- Consumes: `crate::events_routes::{EventRoutes, NativeFacts}`.

- [ ] **Step 1: Pruebas que fallan**

Crear `crates/comandos-server/tests/dash_native_events.rs`:

```rust
//! Dominio B: marcas y eventos v2 por el frente nativo, y la importación
//! única de events.jsonl compartida con el Python.
mod support;

use serde_json::Value;
use support::{TestHome, dead_port, front, get, oracle::oracle, request_body};

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[tokio::test]
async fn events_and_marks_are_answered_natively() {
    let home = TestHome::new("events");
    let front = front(&home, dead_port(), home.options()).await;
    let mark = request_body(
        front.port,
        "POST",
        "/work-marks",
        "",
        r#"{"scope": "session", "key": "s", "value": "resolved", "expectedRevision": 0}"#,
    )
    .await;
    assert_eq!(mark.status, 200, "{}", mark.text());
    assert!(mark.text().starts_with(
        r#"{"mark": {"scope": "session", "key": "s", "mark": "resolved", "favorite": false, "revision": 1, "#
    ));
    let stale = request_body(
        front.port,
        "POST",
        "/work-marks",
        "",
        r#"{"scope": "session", "key": "s", "value": "frozen", "expectedRevision": 0}"#,
    )
    .await;
    assert_eq!(stale.status, 409);
    assert!(stale.text().starts_with(r#"{"error": "Revisión desactualizada", "current": {"#));
    let marks = parse(&get(front.port, "/work-marks").await.text());
    assert_eq!(marks["marks"][0]["key"], "s");
    let bad = get(front.port, "/events/v2?after=x").await;
    assert_eq!((bad.status, bad.text().as_str()), (400, r#"{"error": "after inválido"}"#));
    let origin = format!("Origin: http://127.0.0.1:{}\r\n", front.port);
    let foreign = request_body(front.port, "POST", "/events/v2", &origin, r#"{"hookEvent": "Stop"}"#).await;
    assert_eq!(
        (foreign.status, foreign.text().as_str()),
        (403, r#"{"error": "Solo productores internos de este equipo"}"#)
    );
    let ignored = request_body(front.port, "POST", "/events/v2", "", r#"{"hookEvent": "Unknown"}"#).await;
    assert_eq!((ignored.status, ignored.text().as_str()), (202, r#"{"ignored": true}"#));
    let accepted = request_body(
        front.port,
        "POST",
        "/events/v2",
        "",
        r#"{"hookEvent": "UserPromptSubmit", "agent": "codex", "session": "s", "pane": "%3", "turnId": "t1", "occurredAtMs": 1000}"#,
    )
    .await;
    assert_eq!(accepted.status, 200, "{}", accepted.text());
    assert_eq!(parse(&accepted.text())["event"]["kind"], "prompt_accepted");
    let page = parse(&get(front.port, "/events/v2?turns=1").await.text());
    assert_eq!(page["latest"], 1);
    assert!(page.get("turns").is_some());
    front.stop().await;
}

#[tokio::test]
async fn legacy_timeline_is_imported_once_across_rust_and_python() {
    let home = TestHome::new("events-legacy");
    home.write(
        "events.jsonl",
        "{\"ts\": 1790000000, \"project\": \"p\", \"status\": \"done\", \"detail\": \"hola\"}\n\
         {\"ts\": 1790000060, \"project\": \"p\", \"status\": \"waiting\", \"detail\": \"permiso\"}\n",
    );
    let front = front(&home, dead_port(), home.options()).await;
    let first = parse(&get(front.port, "/events/v2").await.text());
    let imported = first["latest"].as_i64().unwrap();
    assert!(imported >= 1, "la importación heredada crea eventos: {first}");
    let again = parse(&get(front.port, "/events/v2").await.text());
    assert_eq!(again["latest"], imported, "una sola importación");
    // El Python sobre la misma base ve la marca y no reimporta.
    if let Some(py) = oracle(&home).await {
        let python = get(py.port, "/events/v2").await.text();
        assert_eq!(parse(&python)["latest"], imported);
        assert_eq!(python, get(front.port, "/events/v2").await.text());
        assert_eq!(
            get(py.port, "/work-marks").await.text(),
            get(front.port, "/work-marks").await.text()
        );
    }
    front.stop().await;
}

#[tokio::test]
async fn python_written_mark_is_read_by_rust_and_vice_versa() {
    let home = TestHome::new("events-interop");
    let front = front(&home, dead_port(), home.options()).await;
    let Some(py) = oracle(&home).await else {
        front.stop().await;
        return;
    };
    let body = r#"{"scope": "session", "key": "py", "value": "frozen", "expectedRevision": 0}"#;
    assert_eq!(request_body(py.port, "POST", "/work-marks", "", body).await.status, 200);
    let body = r#"{"scope": "session", "key": "rs", "value": "resolved", "expectedRevision": 0}"#;
    assert_eq!(request_body(front.port, "POST", "/work-marks", "", body).await.status, 200);
    assert_eq!(
        get(py.port, "/work-marks").await.text(),
        get(front.port, "/work-marks").await.text()
    );
    front.stop().await;
}
```

- [ ] **Step 2: Ejecutar y ver el fallo**

Run: `$C test -p comandos-server --test dash_native_events -j 6`
Expected: FAIL: 502 `Servidor heredado no disponible` (las rutas aún se reenvían al puerto muerto).

- [ ] **Step 3: Implementar**

En `native/state.rs`: `use crate::{HandlerError, Reply, Request, events_routes::{EventRoutes, NativeFacts}};`, y `StateBackend` pasa a

```rust
pub struct StateBackend {
    pub conn: Connection,
    /// Se crea al primer uso: su `import_done` es el `_EVENTS_V2_LEGACY` del Python.
    events: Option<EventRoutes<NativeFacts>>,
}
```

(con `events: None` en `open`), más

```rust
impl StateBackend {
    /// Las cuatro rutas de eventos y marcas, en el hilo del worker.
    pub fn events(&mut self, legacy: &Path, request: &Request) -> Result<Option<Reply>, HandlerError> {
        let routes = self
            .events
            .get_or_insert_with(|| EventRoutes::new(legacy.to_path_buf(), NativeFacts));
        routes.handle(&self.conn, request)
    }
}
```

Crear `crates/comandos-server/src/dash/native/events.rs`:

```rust
//! B. Eventos y marcas. La lógica (y su paridad con el Python) vive en
//! `events_routes.rs`; aquí solo se monta sobre el worker de la base.
use super::{Answer, Entry, Fault, Key, Native, NativeRoute, Verb};
use crate::{HandlerError, Request};

pub const ROUTES: &[Entry] = &[
    Entry { verb: Verb::Get, key: Key::Path("/work-marks"), route: NativeRoute::Events },
    Entry { verb: Verb::Get, key: Key::Path("/events/v2"), route: NativeRoute::Events },
    Entry { verb: Verb::Post, key: Key::Raw("/work-marks"), route: NativeRoute::Events },
    Entry { verb: Verb::Post, key: Key::Raw("/events/v2"), route: NativeRoute::Events },
];

pub async fn answer(native: &Native, request: &Request) -> Answer {
    let legacy = native.options().hooks.join("events.jsonl");
    let request = request.clone();
    match native
        .with_state(move |backend| backend.events(&legacy, &request))
        .await?
    {
        Ok(Some(reply)) => Ok(reply),
        // La tabla y `EventRoutes` reconocen las mismas cuatro rutas.
        Ok(None) => Err(Fault::Error(HandlerError::Failure)),
        Err(error) => Err(error.into()),
    }
}
```

En `native/mod.rs`: `pub mod events;`, variante `Events` en `NativeRoute`, `TABLES = &[light::ROUTES, events::ROUTES]` y en `answer` el brazo `NativeRoute::Events => events::answer(self, request).await,`.

- [ ] **Step 4: Ver pasar**

Run: `$C test -p comandos-server --test dash_native_events --test events_routes --test events_routes_socket -j 6`
Expected: PASS (3 pruebas nuevas; las dos de oráculo se saltan con aviso sin `python3`).

- [ ] **Step 5: Fixture**

En `xtask/parity/frente.jsonl`, en la línea `"name":"workmarks-local"` quitar `,"forwarded":true` (ahora es nativa). Añadir al final:

```
# --- 2b · B: eventos y marcas (nativas)
{"name":"b-events-v2","method":"GET","path":"/events/v2?limit=5","headers":{"Host":"127.0.0.1"},"body":null,"volatile":["/events/*/receivedAtMs","/events/*/eventId"],"expect":"same"}
{"name":"b-events-v2-turns","method":"GET","path":"/events/v2?after=0&limit=3&turns=1","headers":{"Host":"127.0.0.1"},"body":null,"volatile":["/events/*/receivedAtMs","/events/*/eventId"],"expect":"same"}
{"name":"b-events-v2-malo","method":"GET","path":"/events/v2?after=-1","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"b-work-marks-post","method":"POST","path":"/work-marks","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"scope":"session","key":"parity-b","value":"resolved","expectedRevision":0},"volatile":["/mark/updatedAtMs"],"expect":"same"}
{"name":"b-work-marks-conflicto","method":"POST","path":"/work-marks","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"scope":"session","key":"parity-b","value":"frozen","expectedRevision":0},"volatile":["/current/updatedAtMs"],"expect":"same"}
{"name":"b-work-marks-malo","method":"POST","path":"/work-marks","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"scope":"nope","key":"x","value":"resolved","expectedRevision":0},"volatile":[],"expect":"same"}
{"name":"b-work-marks-tras","method":"GET","path":"/work-marks","headers":{"Host":"127.0.0.1"},"body":null,"volatile":["/marks/*/updatedAtMs"],"expect":"same"}
{"name":"b-events-v2-post-ajeno","method":"POST","path":"/events/v2","headers":{"Host":"127.0.0.1","Origin":"http://127.0.0.1","Content-Type":"application/json"},"body":{"hookEvent":"Stop"},"volatile":[],"expect":"same"}
{"name":"b-events-v2-post-ignorado","method":"POST","path":"/events/v2","headers":{"Host":"127.0.0.1","X-Comandos-Token":"{{token}}","Content-Type":"application/json"},"body":{"hookEvent":"Unknown"},"volatile":[],"expect":"same"}
```

- [ ] **Step 6: Paridad**

Run: `$C build -p comandos-cli -p xtask -j 6 && .build/target/debug/xtask parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks --state-db ~/.local/state/comandos/app-state.sqlite3 --comandos .build/target/debug/comandos`
Expected: 0 DIFF; `GET /work-marks`, `POST /work-marks`, `GET /events/v2`, `POST /events/v2` ya no aparecen en «reenviadas al heredado».

- [ ] **Step 7: fmt, clippy, commit**

```bash
$C fmt --all -- --check
$C clippy --workspace --all-targets -j 6 -- -D warnings
git add crates/comandos-server/src/dash/native/mod.rs crates/comandos-server/src/dash/native/state.rs \
  crates/comandos-server/src/dash/native/events.rs crates/comandos-server/tests/dash_native_events.rs \
  xtask/parity/frente.jsonl
git commit -m "feat(dash): dominio B nativo — work-marks y events/v2 sobre el worker de app-state

EventRoutes (ya fijado contra el Python) se monta en el BackendWorker del
frente. La importación de events.jsonl ocurre una sola vez entre Rust y Python
gracias a la marca events.legacy_import.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Dominio A — notificaciones (8 rutas, long-poll incluido)

Desplegable sola: GET `/notices`, `/notices/prefs`, `/notices/watch`, `/notifs/count` y POST `/presence`, `/notices/read`, `/notices/sound`, `/notices/prefs` pasan a Rust. Quedan reenviadas `/notify-popup`, `/test` y push.

Correspondencia (todas las consultas a la base en el worker; `is_live` por tmux fuera de él):

- **`notices_is_live`** (980): `tmux list-panes -a -F "#{session_name}\t#{pane_id}"`; rc≠0 o cualquier excepción → `None` (no se filtra); si no, `(s, p) in live` si `p` es verdadero, `s in sessions` si no.
- **GET `/notices`** (8639): `int(after or "0")`, `int(limit or "100")` (en ese orden; `ValueError` → 400 `invalid literal for int() with base 10: '<x>'`), luego `is_live`, `list_notices(conn, after, limit, now, deviceId, focus_active, is_live)` → `{"notices","nextAfter","pending","prefs","focusActive"}` y se añade `badge`. Solo lee.
- **GET `/notices/prefs`**: `load_prefs(conn)`. Solo lee.
- **GET `/notices/watch`** (8616): `rev` (por defecto `""`), `wait = max(0, min(25, float(wait or "25")))` (`ValueError` → 25); bucle: `revision()` cada 200 ms mientras no cambie y no venza; luego `is_live` y, en un solo trabajo, `{"rev","badge","unread"(últimos 2000),"pending","latest"}`. Cada vuelta es un trabajo corto del worker; la espera es un `tokio::time::sleep` (ruling 3).
- **GET `/notifs/count`** (8506): `{"count": badge_count(conn, is_live)}`; cualquier excepción → 0.
- **POST `/presence`**: `record_presence(conn, deviceId, visible is True, canPlayAudio is True, interaction is True, now, kind=str(kind or "web"))` → `{"ok": true}`; 400 `deviceId inválido`.
- **POST `/notices/read`**: `ids = eventIds`; con `all is True`, `ids = unread_notice_ids(conn, project)`; no-lista → 400 `eventIds inválido`; → `{"ok": true, "read": mark_read(conn, ids, now)}`.
- **POST `/notices/sound`**: `claim_sound(conn, str(eventId or ""), str(deviceId or ""), now, focus_active)`.
- **POST `/notices/prefs`**: `save_prefs(conn, data)`.
- En los cuatro POST, `ValueError`/`TypeError` del store → 400 con su texto; errores de SQLite → 500.

**Files:**
- Create: `crates/comandos-server/src/dash/native/notices.rs`
- Modify: `crates/comandos-server/src/dash/native/mod.rs`
- Modify: `crates/comandos-store/src/notifications.rs` (`fn recent` → `pub fn recent`, sin otro cambio)
- Create: `crates/comandos-server/tests/dash_native_notices.rs`
- Modify: `xtask/parity/frente.jsonl`

**Interfaces:**
- Produces: `notices::{NoticesRoute, ROUTES, answer, LiveSet, is_live, with_live}`; `NativeRoute::Notices`; `comandos_store::notifications::recent` público.
- Consumes: `comandos_store::{notifications as nd, latest_sequence, Error}`, `comandos_core::notifications::{LiveCheck, live_pending}`.

- [ ] **Step 1: Pruebas que fallan**

Crear `crates/comandos-server/tests/dash_native_notices.rs`:

```rust
//! Dominio A: avisos, campana, presencia, sonido y long-poll por el frente.
mod support;

use comandos_server::dash::native::{
    tmux::{Program, Tmux},
    wall_clock_ms,
};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use support::{FakeLegacy, TestHome, dead_port, front, get, oracle::oracle, request_body};

fn seed_permission(home: &TestHome, id: &str) {
    let conn = comandos_runtime::open_state(&home.state_db(), 5000).unwrap();
    comandos_store::append_event(
        &conn,
        &json!({"eventId": id, "source": "test", "kind": "permission_requested",
                "evidence": "confirmed", "correlation": "local", "sessionKey": "s1",
                "occurredAtMs": 1_791_115_100_000_u64, "receivedAtMs": 1_791_115_100_000_u64}),
        0,
        "generated",
        &format!("r-{id}"),
    )
    .unwrap();
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[tokio::test]
async fn notices_list_count_read_presence_sound_prefs() {
    let home = TestHome::new("notices");
    seed_permission(&home, "e1");
    let front = front(&home, dead_port(), home.options()).await;
    let list = get(front.port, "/notices?deviceId=d1").await;
    assert_eq!(list.status, 200);
    let page = parse(&list.text());
    let keys: Vec<&str> = page.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["notices", "nextAfter", "pending", "prefs", "focusActive", "badge"]);
    assert_eq!(page["notices"][0]["eventId"], "e1");
    assert_eq!(page["badge"], 1);
    let bad = get(front.port, "/notices?after=x").await;
    assert_eq!(
        (bad.status, bad.text().as_str()),
        (400, r#"{"error": "invalid literal for int() with base 10: 'x'"}"#)
    );
    let bad = get(front.port, "/notices?after=1&limit=1.5").await;
    assert_eq!(bad.text(), r#"{"error": "invalid literal for int() with base 10: '1.5'"}"#);
    assert_eq!(get(front.port, "/notifs/count").await.text(), r#"{"count": 1}"#);
    assert_eq!(get(front.port, "/notices/prefs").await.status, 200);
    let presence = request_body(front.port, "POST", "/presence", "", r#"{"deviceId": ""}"#).await;
    assert_eq!((presence.status, presence.text().as_str()), (400, r#"{"error": "deviceId inválido"}"#));
    let presence = request_body(
        front.port,
        "POST",
        "/presence",
        "",
        r#"{"deviceId": "d1", "visible": true, "canPlayAudio": true, "kind": "desktop"}"#,
    )
    .await;
    assert_eq!(presence.text(), r#"{"ok": true}"#);
    let sound = request_body(front.port, "POST", "/notices/sound", "", r#"{"eventId": "e1", "deviceId": "d1"}"#).await;
    assert_eq!(sound.status, 200);
    assert!(parse(&sound.text()).get("play").is_some());
    let bad = request_body(front.port, "POST", "/notices/read", "", r#"{"eventIds": "e1"}"#).await;
    assert_eq!((bad.status, bad.text().as_str()), (400, r#"{"error": "eventIds inválido"}"#));
    let read = request_body(front.port, "POST", "/notices/read", "", r#"{"eventIds": ["e1", 3, ""]}"#).await;
    assert_eq!(read.text(), r#"{"ok": true, "read": ["e1"]}"#);
    assert_eq!(get(front.port, "/notifs/count").await.text(), r#"{"count": 0}"#);
    seed_permission(&home, "e2");
    let all = request_body(front.port, "POST", "/notices/read", "", r#"{"all": true}"#).await;
    assert_eq!(all.text(), r#"{"ok": true, "read": ["e2"]}"#);
    let prefs = request_body(front.port, "POST", "/notices/prefs", "", "{}").await;
    assert_eq!(prefs.status, 200);
    assert_eq!(prefs.text(), get(front.port, "/notices/prefs").await.text());
    front.stop().await;
}

#[tokio::test]
async fn watch_returns_when_revision_changes() {
    let home = TestHome::new("watch");
    seed_permission(&home, "e1");
    let front = front(&home, dead_port(), home.options()).await;
    let now = parse(&get(front.port, "/notices/watch?wait=0").await.text());
    assert_eq!(now["unread"], json!(["e1"]));
    let rev = now["rev"].as_str().unwrap().to_owned();
    let port = front.port;
    let started = Instant::now();
    let waiting = tokio::spawn(async move { get(port, &format!("/notices/watch?rev={rev}&wait=10")).await });
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(!waiting.is_finished(), "sin cambios la petición sigue abierta");
    request_body(front.port, "POST", "/notices/read", "", r#"{"eventIds": ["e1"]}"#).await;
    let woke = parse(&waiting.await.unwrap().text());
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(woke["unread"], json!([]));
    assert_eq!(woke["badge"], 0);
    let keys: Vec<&str> = woke.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["rev", "badge", "unread", "pending", "latest"]);
    // `wait` inválido es 25 s; negativo es 0.
    let quick = Instant::now();
    get(front.port, "/notices/watch?rev=otra&wait=-5").await;
    assert!(quick.elapsed() < Duration::from_secs(1));
    front.stop().await;
}

#[tokio::test]
async fn watch_storm_keeps_worker_responsive() {
    let home = TestHome::new("watch-storm");
    seed_permission(&home, "e1");
    let front = front(&home, dead_port(), home.options()).await;
    let rev = parse(&get(front.port, "/notices/watch?wait=0").await.text())["rev"]
        .as_str()
        .unwrap()
        .to_owned();
    let port = front.port;
    let watchers: Vec<_> = (0..50)
        .map(|_| {
            let rev = rev.clone();
            tokio::spawn(async move { get(port, &format!("/notices/watch?rev={rev}&wait=5")).await })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(500)).await;
    for _ in 0..5 {
        let t = Instant::now();
        assert_eq!(get(port, "/notifs/count").await.status, 200);
        assert!(t.elapsed() < Duration::from_secs(1), "la campana no espera a los long-polls");
    }
    request_body(port, "POST", "/notices/read", "", r#"{"eventIds": ["e1"]}"#).await;
    let woke = Instant::now();
    for watcher in watchers {
        assert_eq!(watcher.await.unwrap().status, 200);
    }
    assert!(woke.elapsed() < Duration::from_secs(2), "todos despiertan con el cambio");
    front.stop().await;
}

#[tokio::test]
async fn notices_survive_hung_tmux() {
    let home = TestHome::new("notices-hung");
    seed_permission(&home, "e1");
    let mut opts = home.options();
    opts.tmux = Tmux {
        program: Program {
            path: "tail".into(),
            prefix: vec!["-f".into(), "/dev/null".into(), "--".into()],
            env: vec![],
            env_remove: vec![],
        },
        timeout: Duration::from_millis(300),
    };
    let front = front(&home, dead_port(), opts).await;
    // is_live es None: no se filtra por terminal viva.
    assert_eq!(get(front.port, "/notifs/count").await.text(), r#"{"count": 1}"#);
    assert_eq!(get(front.port, "/notices").await.status, 200);
    front.stop().await;
}

#[tokio::test]
async fn exotic_inputs_decline_to_legacy() {
    let home = TestHome::new("notices-exotic");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    for target in [
        "/notices?after=%D9%A1",
        "/notices?limit=99999999999999999999999",
        "/notices/watch?wait=%D9%A1",
    ] {
        assert_eq!(get(front.port, target).await.text(), r#"{"legacy": true}"#, "{target}");
    }
    for (path, body) in [
        ("/presence", r#"{"deviceId": "d", "kind": {"a": 1}}"#),
        ("/notices/sound", r#"{"eventId": 1.5, "deviceId": "d"}"#),
        ("/notices/read", r#"{"all": true, "project": 5}"#),
    ] {
        let wire = request_body(front.port, "POST", path, "", body).await;
        assert_eq!(wire.text(), r#"{"legacy": true}"#, "{path}");
    }
    let conn = comandos_runtime::open_state(&home.state_db(), 5000).unwrap();
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM client_presence", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0, "declinar nunca escribe");
    front.stop().await;
}

#[tokio::test]
async fn notices_forwarded_after_schema_bump() {
    let home = TestHome::new("notices-bump");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    assert_eq!(get(front.port, "/notifs/count").await.text(), r#"{"count": 0}"#);
    let conn = rusqlite::Connection::open(home.state_db()).unwrap();
    conn.execute(
        "INSERT INTO schema_migrations (version, name, applied_at) VALUES (9999, 'futuro', 0)",
        [],
    )
    .unwrap();
    assert_eq!(get(front.port, "/notifs/count").await.text(), r#"{"legacy": true}"#);
    // Todo el conjunto nativo queda apagado, también lo que no toca la base.
    assert_eq!(get(front.port, "/prefs").await.text(), r#"{"legacy": true}"#);
    front.stop().await;
}

#[tokio::test]
async fn notices_match_python_oracle() {
    let home = TestHome::new("notices-oracle");
    seed_permission(&home, "e1");
    seed_permission(&home, "e2");
    let mut opts = home.options();
    opts.clock = Arc::new(wall_clock_ms);
    let front = front(&home, dead_port(), opts).await;
    let Some(py) = oracle(&home).await else {
        front.stop().await;
        return;
    };
    for target in ["/notices/prefs", "/notifs/count", "/notices/watch?wait=0", "/notices?after=0&limit=1", "/notices?after=z"] {
        let (a, b) = (get(py.port, target).await, get(front.port, target).await);
        assert_eq!((a.status, a.text()), (b.status, b.text()), "{target}");
    }
    // Lo que escribe uno lo ve el otro.
    request_body(front.port, "POST", "/notices/read", "", r#"{"eventIds": ["e1"]}"#).await;
    assert_eq!(get(py.port, "/notifs/count").await.text(), r#"{"count": 1}"#);
    let (a, b) = (
        request_body(py.port, "POST", "/notices/read", "", r#"{"all": true}"#).await,
        request_body(front.port, "POST", "/notices/read", "", r#"{"all": true}"#).await,
    );
    assert_eq!(a.text(), r#"{"ok": true, "read": ["e2"]}"#);
    assert_eq!(b.text(), r#"{"ok": true, "read": []}"#);
    front.stop().await;
}
```

- [ ] **Step 2: Ejecutar y ver el fallo**

Run: `$C test -p comandos-server --test dash_native_notices -j 6`
Expected: FAIL: 502 desde el puerto muerto en las rutas de avisos.

- [ ] **Step 3: Implementar**

En `crates/comandos-store/src/notifications.rs` cambiar `fn recent(conn: &Connection)` por `pub fn recent(conn: &Connection)` (el `_recent` del Python, que `/notices/watch` usa directamente).

Crear `crates/comandos-server/src/dash/native/notices.rs`:

```rust
//! A. Notificaciones (`bin/cc-dash` 8506, 8616, 8639, 8818;
//! `lib/notification_delivery.py`). La base, en el worker; tmux, fuera.
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb,
    light::{data, error},
    py::{self, NumError},
    query::Query,
    reply,
    tmux::Tmux,
};
use crate::{HandlerError, Request};
use comandos_core::{
    json::truthy,
    notifications::{LiveCheck, live_pending},
};
use comandos_store::{Error as StoreError, latest_sequence, notifications as nd};
use http::StatusCode;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticesRoute {
    List,
    Prefs,
    Watch,
    Count,
    Presence,
    Read,
    Sound,
    SavePrefs,
}

const fn entry(verb: Verb, key: Key, route: NoticesRoute) -> Entry {
    Entry { verb, key, route: NativeRoute::Notices(route) }
}

pub const ROUTES: &[Entry] = &[
    entry(Verb::Get, Key::Path("/notices"), NoticesRoute::List),
    entry(Verb::Get, Key::Path("/notices/prefs"), NoticesRoute::Prefs),
    entry(Verb::Get, Key::Path("/notices/watch"), NoticesRoute::Watch),
    entry(Verb::Get, Key::Path("/notifs/count"), NoticesRoute::Count),
    entry(Verb::Post, Key::Raw("/presence"), NoticesRoute::Presence),
    entry(Verb::Post, Key::Raw("/notices/read"), NoticesRoute::Read),
    entry(Verb::Post, Key::Raw("/notices/sound"), NoticesRoute::Sound),
    entry(Verb::Post, Key::Raw("/notices/prefs"), NoticesRoute::SavePrefs),
];

/// `notices_is_live()` (980): conjunto de terminales vivas o `None`.
#[derive(Clone)]
pub struct LiveSet {
    panes: HashSet<(String, String)>,
    sessions: HashSet<String>,
}

impl LiveSet {
    /// `(s, p or "") in live if p else s in sessions`.
    pub fn check(&self, session: &Value, pane: &Value) -> bool {
        let Some(s) = session.as_str() else { return false };
        if truthy(pane) {
            pane.as_str()
                .is_some_and(|p| self.panes.contains(&(s.to_owned(), p.to_owned())))
        } else {
            self.sessions.contains(s)
        }
    }
}

pub async fn is_live(tmux: &Tmux) -> Option<LiveSet> {
    let out = tmux
        .run(&["list-panes", "-a", "-F", "#{session_name}\t#{pane_id}"])
        .await
        .ok()?;
    if !out.ok {
        return None;
    }
    let panes: HashSet<(String, String)> = py::splitlines(&out.stdout)
        .into_iter()
        .filter_map(|line| line.split_once('\t'))
        .map(|(s, p)| (s.to_owned(), p.to_owned()))
        .collect();
    let sessions = panes.iter().map(|(s, _)| s.clone()).collect();
    Some(LiveSet { panes, sessions })
}

/// Presta el `LiveCheck` del store durante `body`.
pub fn with_live<T>(live: &Option<LiveSet>, body: impl FnOnce(LiveCheck<'_>) -> T) -> T {
    match live {
        Some(set) => {
            let check = |s: &Value, p: &Value| set.check(s, p);
            body(Some(&check))
        }
        None => body(None),
    }
}

/// `int(text)`; su `ValueError` es un 400 con el texto del Python.
fn int_param(text: &str) -> Result<i64, Answer> {
    match py::int(text) {
        Ok(value) => Ok(value),
        Err(NumError::Exotic) => Err(Err(Fault::Decline)),
        Err(NumError::Invalid) => Err(match py::int_error_message(text) {
            Some(message) => error(StatusCode::BAD_REQUEST, &message),
            None => Err(Fault::Decline),
        }),
    }
}

/// `except (ValueError, TypeError) as exc: 400 str(exc)`; SQLite → 500.
fn store_answer(result: Result<Value, StoreError>) -> Answer {
    match result {
        Ok(value) => reply(StatusCode::OK, &value),
        Err(StoreError::Validation(message)) => error(StatusCode::BAD_REQUEST, &message),
        Err(_) => Err(HandlerError::Failure.into()),
    }
}

/// `str(value or "")` con escalares seguros.
fn text_or_empty(value: Option<&Value>) -> Result<String, Fault> {
    match value.filter(|v| truthy(v)) {
        None => Ok(String::new()),
        Some(v) => py::str_scalar(v).ok_or(Fault::Decline),
    }
}

async fn revision(native: &Native) -> Result<String, Fault> {
    native
        .with_state(|b| nd::revision(&b.conn))
        .await?
        .map_err(|_| Fault::Error(HandlerError::Failure))
}

pub async fn answer(native: &Native, route: NoticesRoute, request: &Request) -> Answer {
    let tmux = &native.options().tmux;
    let now = (native.options().clock)();
    match route {
        NoticesRoute::List => {
            let query = Query::parse(&request.target)?;
            let after = match int_param(query.first("after").unwrap_or("0")) {
                Ok(v) => v,
                Err(answer) => return answer,
            };
            let limit = match int_param(query.first("limit").unwrap_or("100")) {
                Ok(v) => v,
                Err(answer) => return answer,
            };
            let device = query.first("deviceId").map(str::to_owned);
            let live = is_live(tmux).await;
            store_answer(
                native
                    .with_state(move |b| {
                        let focus = nd::focus_block_active(&b.conn);
                        with_live(&live, |check| {
                            let mut page =
                                nd::list_notices(&b.conn, after, limit, now, device.as_deref(), focus, check)?;
                            let badge = nd::badge_count(&b.conn, check)?;
                            if let Some(map) = page.as_object_mut() {
                                map.insert("badge".into(), json!(badge));
                            }
                            Ok(page)
                        })
                    })
                    .await?,
            )
        }
        NoticesRoute::Prefs => {
            store_answer(native.with_state(|b| nd::load_prefs(&b.conn)).await?)
        }
        NoticesRoute::Watch => {
            let query = Query::parse(&request.target)?;
            let seen = query.first("rev").unwrap_or("").to_owned();
            let wait = match py::float(query.first("wait").unwrap_or("25")) {
                Ok(x) => py::clamp_py_float(x, 0.0, 25.0),
                Err(NumError::Invalid) => 25.0,
                Err(NumError::Exotic) => return Err(Fault::Decline),
            };
            let deadline = Instant::now() + Duration::from_secs_f64(wait);
            let mut rev = revision(native).await?;
            // Ruling 3: la espera es async; cada vuelta es un trabajo corto.
            while rev == seen && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(200)).await;
                rev = revision(native).await?;
            }
            let live = is_live(tmux).await;
            store_answer(
                native
                    .with_state(move |b| {
                        with_live(&live, |check| {
                            let history = nd::recent(&b.conn)?;
                            let badge = nd::badge_count(&b.conn, check)?;
                            let unread = nd::unread_notice_ids(&b.conn, None)?;
                            let tail = unread[unread.len().saturating_sub(2000)..].to_vec();
                            let pending = live_pending(&history, check);
                            let latest = latest_sequence(&b.conn)?;
                            Ok(json!({"rev": rev, "badge": badge, "unread": tail, "pending": pending, "latest": latest}))
                        })
                    })
                    .await?,
            )
        }
        NoticesRoute::Count => {
            let live = is_live(tmux).await;
            let count = match native
                .with_state(move |b| with_live(&live, |check| nd::badge_count(&b.conn, check)))
                .await
            {
                Ok(Ok(n)) => n,
                // Esquema nuevo: todo se reenvía; cualquier otra excepción es 0.
                Err(Fault::Decline) => return Err(Fault::Decline),
                _ => 0,
            };
            reply(StatusCode::OK, &json!({"count": count}))
        }
        NoticesRoute::Presence => {
            let data = data(request)?;
            let device = match data.get("deviceId") {
                Some(Value::String(s)) => s.clone(),
                _ => return error(StatusCode::BAD_REQUEST, "deviceId inválido"),
            };
            let kind = match data.get("kind").filter(|v| truthy(v)) {
                None => "web".to_owned(),
                Some(v) => py::str_scalar(v).ok_or(Fault::Decline)?,
            };
            let is_true = |key: &str| data.get(key) == Some(&Value::Bool(true));
            let (visible, audio, interaction) =
                (is_true("visible"), is_true("canPlayAudio"), is_true("interaction"));
            store_answer(
                native
                    .with_state(move |b| {
                        nd::record_presence(&b.conn, &device, visible, audio, interaction, now, &json!(kind))
                            .map(|()| json!({"ok": true}))
                    })
                    .await?,
            )
        }
        NoticesRoute::Read => {
            let data = data(request)?;
            let all = data.get("all") == Some(&Value::Bool(true));
            let ids = data.get("eventIds").cloned();
            let project = match data.get("project") {
                Some(v) if !truthy(v) => None,
                None => None,
                Some(Value::String(s)) => Some(s.clone()),
                // Un proyecto verdadero que no es str no casa con nada: raro, se declina.
                Some(_) if all => return Err(Fault::Decline),
                Some(_) => None,
            };
            if !all && !matches!(ids, Some(Value::Array(_))) {
                return error(StatusCode::BAD_REQUEST, "eventIds inválido");
            }
            store_answer(
                native
                    .with_state(move |b| {
                        let ids: Vec<Value> = if all {
                            nd::unread_notice_ids(&b.conn, project.as_deref())?
                                .into_iter()
                                .map(Value::String)
                                .collect()
                        } else {
                            match ids {
                                Some(Value::Array(ids)) => ids,
                                _ => Vec::new(),
                            }
                        };
                        let read = nd::mark_read(&b.conn, &ids, now)?;
                        Ok(json!({"ok": true, "read": read}))
                    })
                    .await?,
            )
        }
        NoticesRoute::Sound => {
            let data = data(request)?;
            let event = text_or_empty(data.get("eventId"))?;
            let device = text_or_empty(data.get("deviceId"))?;
            store_answer(
                native
                    .with_state(move |b| {
                        let focus = nd::focus_block_active(&b.conn);
                        nd::claim_sound(&b.conn, &event, &device, now, Some(focus))
                    })
                    .await?,
            )
        }
        NoticesRoute::SavePrefs => {
            let update = request.data.clone().ok_or(Fault::Error(HandlerError::Failure))?;
            store_answer(native.with_state(move |b| nd::save_prefs(&b.conn, &update)).await?)
        }
    }
}
```

En `native/mod.rs`: `pub mod notices;`, variante `Notices(notices::NoticesRoute)`, `TABLES = &[light::ROUTES, events::ROUTES, notices::ROUTES]`, brazo `NativeRoute::Notices(route) => notices::answer(self, route, request).await,`.

- [ ] **Step 4: Ver pasar**

Run: `$C test -p comandos-server --test dash_native_notices -j 6 && $C test -p comandos-store -j 6`
Expected: PASS (7 pruebas; la del oráculo se salta sin `python3`). `watch_storm_keeps_worker_responsive` termina en < 4 s.

- [ ] **Step 5: Fixture**

```
# --- 2b · A: notificaciones (nativas)
{"name":"a-notices","method":"GET","path":"/notices?limit=20","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"a-notices-device","method":"GET","path":"/notices?after=0&limit=5&deviceId=parity","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"a-notices-malo","method":"GET","path":"/notices?after=x","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"a-notices-prefs","method":"GET","path":"/notices/prefs","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"a-notifs-count","method":"GET","path":"/notifs/count","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"a-watch-ya","method":"GET","path":"/notices/watch?wait=0","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"a-watch-espera","method":"GET","path":"/notices/watch?rev=distinta&wait=1","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"a-presence-malo","method":"POST","path":"/presence","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"deviceId":""},"volatile":[],"expect":"same"}
{"name":"a-presence","method":"POST","path":"/presence","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"deviceId":"parity-a","visible":true,"canPlayAudio":false,"kind":"web"},"volatile":[],"expect":"same"}
{"name":"a-read-malo","method":"POST","path":"/notices/read","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"eventIds":"x"},"volatile":[],"expect":"same"}
{"name":"a-read-nada","method":"POST","path":"/notices/read","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"eventIds":["no-existe"]},"volatile":[],"expect":"same"}
{"name":"a-sound","method":"POST","path":"/notices/sound","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"eventId":"no-existe","deviceId":"parity-a"},"volatile":[],"expect":"same"}
{"name":"a-prefs-post","method":"POST","path":"/notices/prefs","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"a-notifs-count-tras","method":"GET","path":"/notifs/count","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
```

(`a-read-nada` usa un id inexistente a propósito: «marcar todo leído» sobre la copia cambiaría la campana del resto de casos y no añade cobertura que no dé la prueba Rust.)

- [ ] **Step 6: Paridad**

Run: `$C build -p comandos-cli -p xtask -j 6 && .build/target/debug/xtask parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks --state-db ~/.local/state/comandos/app-state.sqlite3 --comandos .build/target/debug/comandos`
Expected: 0 DIFF; las ocho rutas de avisos fuera de «reenviadas al heredado».

- [ ] **Step 7: fmt, clippy, commit**

```bash
$C fmt --all -- --check
$C clippy --workspace --all-targets -j 6 -- -D warnings
git add crates/comandos-server/src/dash/native/mod.rs crates/comandos-server/src/dash/native/notices.rs \
  crates/comandos-store/src/notifications.rs crates/comandos-server/tests/dash_native_notices.rs \
  xtask/parity/frente.jsonl
git commit -m "feat(dash): dominio A nativo — avisos, campana, presencia, sonido y long-poll

El long-poll de /notices/watch espera con tokio y consulta la revisión cada
200 ms como trabajos cortos del worker: 50 esperas abiertas no retrasan la
campana. is_live por tmux fuera del worker; tmux colgado = sin filtro, como
el Python.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: Dominio C — workspace (6 rutas nativas; POST `/workspace/close-group` sigue reenviada)

Desplegable sola: GET `/workspace` (cc-app lo consulta cada 2 s), GET `/workspace/close-group`, GET `/workspace/client`, POST `/workspace`, POST `/workspace/sort` (solo `restore`) y POST `/workspace/client` pasan a Rust. POST `/workspace/close-group` cierra pestañas con `close_app_tab` (tmux + `app-tab-close.json`), pieza que `comandos-runtime` no tiene: se queda en el Python.

Correspondencia:

- **`workspace_sync`** (6533), en un solo trabajo del worker, hasta 3 intentos: `current = store.current() or {revision 0, empty_document()}`; inventario = `[("local","⌂ local")]` + `ordered_tab_keys(_tab_registry(), favorites)` sin ocultas y con `SESSION_RE` (`_tab_registry` 6466: ausente → `{}`, ilegible o no-objeto → devolver `current` sin tocar nada); paneles = `pane_bindings(read_snapshot(app-sessions-v2.json)["sessions"].get(tab))` (un `ValueError` aquí no se captura → 500); `wanted = reconcile(...)`; si `wanted == document` y `revision != 0` → `current`; si no, `commit(revision, wanted, "sync-<rev>-<sha256(json.dumps(wanted, sort_keys=True))[:24]>", reason="auto")`; `Conflict` → reintentar; `EmptyInventory`/`NotReady` → `current`. **Efecto lateral en GET** (§9.7): puede confirmar una revisión. El `requestId` es determinista, así que si Rust y Python sincronizan a la vez el store deduplica: nunca hay dos revisiones para el mismo inventario (Review Focus 1).
- **`read_snapshot`** (`lib/tmux_snapshot.py` 244): el archivo o su `.bak`, el primero que pase `valid_snapshot` (215); si ninguno, `{"version": 2, "sessions": {}}`. Se porta `valid_snapshot` a `comandos_core::workspace::snapshot` con tres salidas: `Valid`, `Invalid` y `Exotic` (índices o tamaños booleanos o flotantes, layouts no ASCII — donde `\d` de Python casaría dígitos Unicode): `Exotic` declina antes de cualquier commit.
- **`workspace_payload`** (6566): el documento + `revision` + `ready: true`.
- **GET `/workspace`** (8614): `workspace_payload(workspace_sync())`.
- **GET `/workspace/close-group`** (8657): `workspace_sync()` y `close_group_preview(document, groupId, workspace_session_identity)`; `ValueError` → 404 `str(exc)`; si no, la vista previa + `revision`. `workspace_session_identity` (6555): `local` → `"local"`; nombre inválido → `None`; si no `tmux display-message -p -t =<s>: '#{session_id}'` → el valor si es `$<dígitos>`. Como la identidad es asíncrona (tmux) y `close_group_preview` es una función pura con callback síncrono, se llama dos veces: la primera solo anota qué sesiones preguntaría el Python (y detecta el 404 sin tocar tmux), luego se consultan en ese orden y la segunda construye la respuesta.
- **GET `/workspace/client`** (8666): `client(deviceId or "") or {}`; `ValueError` (JSON guardado ilegible) → 400 con texto de Python → se declina.
- **POST `/workspace`** (8792): `expectedRevision` entero no booleano o 400 `expectedRevision inválido`; `workspace_sync()`; `validate_document` → 400 `str(exc) or "Workspace inválido"`; tabs distintas → 400 `La distribución no coincide con las tabs abiertas`; `commit(expected, document, requestId, "user")` → 200 payload, 409 `{"error": str(exc), "current": payload(exc.current)}`, 400 `requestId inválido`.
- **POST `/workspace/sort`** (8789, `workspace_sort` 6497) con `restore` lista de cadenas: hasta 3 intentos de `sync` + `restore_order` + `commit(..., token_hex(12), "user")`; 200 payload + `previous`; 400 `str(exc) or "Orden inválido"`; tras 3 conflictos, 409 `El acomodo cambió mientras ordenaba; intenta de nuevo`. Con `by` (o `restore` con no-cadenas) → `Decline` (necesita `/state`).
- **POST `/workspace/client`** (8878): `save_client(deviceId, data)`; 400 `deviceId inválido` / `Estado de cliente inválido`.

**Files:**
- Create: `crates/comandos-core/src/workspace/snapshot.rs`; Modify: `crates/comandos-core/src/workspace.rs` (`pub mod snapshot;` junto a `pub mod layout;`)
- Create: `crates/comandos-core/tests/workspace_snapshot.rs`
- Create: `crates/comandos-server/src/dash/native/workspace.rs`
- Modify: `crates/comandos-server/src/dash/native/mod.rs`
- Create: `crates/comandos-server/tests/dash_native_workspace.rs`
- Modify: `xtask/parity/frente.jsonl`

**Interfaces:**
- Produces: `comandos_core::workspace::snapshot::{Snapshot, check_snapshot, leaf_ids, layout_checksum}`; `workspace::{WorkspaceRoute, ROUTES, answer, sync, read_snapshot, payload}`; `NativeRoute::Workspace`.
- Consumes: `comandos_store::workspace::{WorkspaceStore, WorkspaceState, Error}`, `comandos_core::workspace::{empty_document, validate_document, reconcile, pane_bindings, close_group_preview, layout::restore_order}`, `comandos_core::json::{python_eq, workspace_dumps_with_options}`, `light::{read_prefs, favorites_set, ordered_tab_keys, HIDDEN_SESSIONS, data, error}`.

- [ ] **Step 1: Pruebas que fallan del snapshot (core)**

Crear `crates/comandos-core/tests/workspace_snapshot.rs`. Los layouts válidos y sus checksums (`b25e`, `020a`) salen del `remap_layout` del Python; se dejan escritos como constantes:

```rust
//! `valid_snapshot` de lib/tmux_snapshot.py: válido, inválido o exótico (declinar).
use comandos_core::workspace::snapshot::{Snapshot, check_snapshot, layout_checksum, leaf_ids};
use serde_json::{Value, json};

const ONE: &str = "b25e,80x24,0,0,1";
const TWO: &str = "020a,80x24,0,0{40x24,0,0,1,39x24,41,0,2}";

fn snapshot(layout: &str, panes: Value) -> Value {
    json!({"version": 2, "sessions": {"s1": {"windows": [
        {"index": 0, "name": "w", "active": 1, "width": 80, "height": 24, "layout": layout, "panes": panes}
    ]}}})
}

fn two_panes() -> Value {
    json!([{"id": "%1", "active": true}, {"id": "%2", "active": false}])
}

#[test]
fn checksum_and_leaves_match_python() {
    assert_eq!(format!("{:04x}", layout_checksum("80x24,0,0,1")), "b25e");
    assert_eq!(format!("{:04x}", layout_checksum("80x24,0,0{40x24,0,0,1,39x24,41,0,2}")), "020a");
    assert_eq!(leaf_ids(TWO), ["1", "2"]);
    assert_eq!(leaf_ids("80x24,0,0,7\n"), ["7"], "`$` casa antes del salto final");
    assert_eq!(leaf_ids("80x24,0,0,7x"), Vec::<&str>::new());
}

#[test]
fn valid_snapshots() {
    assert_eq!(check_snapshot(&snapshot(TWO, two_panes())), Snapshot::Valid);
    assert_eq!(check_snapshot(&snapshot(ONE, json!([{"id": "%1", "active": 1}]))), Snapshot::Valid);
    assert_eq!(check_snapshot(&json!({"version": 2.0, "sessions": {}})), Snapshot::Valid);
}

#[test]
fn invalid_snapshots() {
    let mut cases = vec![
        json!({"version": 3, "sessions": {}}),
        json!({"version": 2, "sessions": []}),
        json!({"version": 2, "sessions": {"s": []}}),
        json!({"version": 2, "sessions": {"s": {"windows": []}}}),
        snapshot("0000,80x24,0,0{40x24,0,0,1,39x24,41,0,2}", two_panes()),
        snapshot(TWO, json!([{"id": "%1", "active": true}, {"id": "%3", "active": false}])),
        snapshot(TWO, json!([{"id": "%1", "active": true}, {"id": "%2", "active": true}])),
        snapshot("80x24", json!([{"id": "%1", "active": true}])),
        snapshot(&format!("{TWO}\n"), two_panes()),
    ];
    let mut no_name = snapshot(TWO, two_panes());
    no_name["sessions"]["s1"]["windows"][0]["name"] = json!(5);
    cases.push(no_name);
    let mut narrow = snapshot(TWO, two_panes());
    narrow["sessions"]["s1"]["windows"][0]["width"] = json!(0);
    cases.push(narrow);
    let mut dup = snapshot(TWO, two_panes());
    let window = dup["sessions"]["s1"]["windows"][0].clone();
    dup["sessions"]["s1"]["windows"] = json!([window.clone(), {"index": 0, "active": 0, "name": "x", "width": 1, "height": 1, "layout": ONE, "panes": [{"id": "%1", "active": true}]}]);
    cases.push(dup);
    for case in cases {
        assert_eq!(check_snapshot(&case), Snapshot::Invalid, "{case}");
    }
}

#[test]
fn exotic_snapshots_decline() {
    // Python acepta `index: true` (bool es int): Rust no lo reproduce, declina.
    let mut flag = snapshot(TWO, two_panes());
    flag["sessions"]["s1"]["windows"][0]["index"] = json!(true);
    assert_eq!(check_snapshot(&flag), Snapshot::Exotic);
    let mut wide = snapshot(TWO, two_panes());
    wide["sessions"]["s1"]["windows"][0]["width"] = json!(true);
    assert_eq!(check_snapshot(&wide), Snapshot::Exotic);
    assert_eq!(check_snapshot(&snapshot("020a,80x24,0,0,١", two_panes())), Snapshot::Exotic);
}
```

- [ ] **Step 2: Ver el fallo**

Run: `$C test -p comandos-core --test workspace_snapshot -j 6`
Expected: FAIL de compilación: `no snapshot in workspace`.

- [ ] **Step 3: Implementar `snapshot.rs`**

Crear `crates/comandos-core/src/workspace/snapshot.rs`:

```rust
//! `valid_snapshot` y `remap_layout` de `lib/tmux_snapshot.py`, para leer
//! `app-sessions-v2.json` exactamente como el Python. Lo que dependería de
//! reglas de Python difíciles de reproducir (bool como int, `\d` Unicode) es
//! `Exotic`: quien llama declina.
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Snapshot {
    Valid,
    Invalid,
    Exotic,
}

fn digit_run(b: &[u8], from: usize) -> usize {
    let mut end = from;
    while end < b.len() && b[end].is_ascii_digit() {
        end += 1;
    }
    end
}

/// Una hoja `(\d+x\d+,\d+,\d+),(\d+)(?=[,}\]]|$)` que empieza en `i`:
/// `(inicio del grupo 2, fin)`. Cada `\d+` va seguido de un no-dígito literal,
/// así que el retroceso nunca da otra coincidencia.
fn leaf_at(b: &[u8], i: usize) -> Option<(usize, usize)> {
    let mut at = i;
    for sep in [b'x', b',', b',', b','] {
        let end = digit_run(b, at);
        if end == at || b.get(end) != Some(&sep) {
            return None;
        }
        at = end + 1;
    }
    let end = digit_run(b, at);
    if end == at {
        return None;
    }
    let lookahead = match b.get(end) {
        None => true,
        Some(b',' | b'}' | b']') => true,
        // `$` sin MULTILINE también casa antes de un `\n` final.
        Some(b'\n') => end + 1 == b.len(),
        Some(_) => false,
    };
    lookahead.then_some((at, end))
}

/// Los grupos 2 de `_LEAF.finditer(layout)` (solo ASCII).
pub fn leaf_ids(layout: &str) -> Vec<&str> {
    let b = layout.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match leaf_at(b, i) {
            Some((start, end)) => {
                out.push(&layout[start..end]);
                i = end;
            }
            None => i += 1,
        }
    }
    out
}

/// El checksum de tmux que recalcula `remap_layout`.
pub fn layout_checksum(body: &str) -> u16 {
    let mut sum: u32 = 0;
    for c in body.chars() {
        sum = (sum >> 1) | ((sum & 1) << 15);
        sum = (sum + c as u32) & 0xffff;
    }
    sum as u16
}

fn is_int_text(raw: &str) -> bool {
    !raw.contains(['.', 'e', 'E']) && !matches!(raw, "NaN" | "Infinity" | "-Infinity")
}

/// `isinstance(v, int) and v >= 1`, o exótico si es bool.
fn positive_int(v: Option<&Value>) -> Result<bool, Snapshot> {
    match v {
        Some(Value::Bool(_)) => Err(Snapshot::Exotic),
        Some(Value::Number(n)) if is_int_text(n.as_str()) => {
            Ok(!n.as_str().starts_with('-') && n.as_str() != "0")
        }
        None => Err(Snapshot::Invalid),
        Some(_) => Ok(false),
    }
}

fn active_count(items: &[Value]) -> Option<usize> {
    let mut n = 0;
    for item in items {
        // `w['active']` sobre un no-dict o sin la clave: excepción → False.
        let active = item.as_object()?.get("active")?;
        if crate::json::truthy(active) {
            n += 1;
        }
    }
    Some(n)
}

#[derive(PartialEq, Eq, Hash)]
enum IndexKey {
    Int(String),
    Str(String),
    Null,
}

fn check_window(window: &Value) -> Result<(), Snapshot> {
    use Snapshot::{Exotic, Invalid};
    let window = window.as_object().ok_or(Invalid)?;
    let panes = match window.get("panes") {
        Some(Value::Array(p)) if !p.is_empty() => p,
        _ => return Err(Invalid),
    };
    if active_count(panes) != Some(1) {
        return Err(Invalid);
    }
    for key in ["width", "height"] {
        if !positive_int(window.get(key))? {
            return Err(Invalid);
        }
    }
    match window.get("index") {
        Some(Value::Number(n)) if is_int_text(n.as_str()) => {}
        Some(Value::Bool(_)) => return Err(Exotic),
        _ => return Err(Invalid),
    }
    if !matches!(window.get("name"), Some(Value::String(_))) {
        return Err(Invalid);
    }
    let mut ids = HashSet::new();
    for pane in panes {
        match pane.get("id") {
            Some(Value::String(id)) => {
                ids.insert(id.clone());
            }
            // Ids no-str nunca igualan a las hojas: el Python acaba en False.
            _ => return Err(Invalid),
        }
    }
    let Some(Value::String(layout)) = window.get("layout") else {
        return Err(Invalid);
    };
    if !layout.is_ascii() {
        return Err(Exotic);
    }
    let leaves: Vec<String> = leaf_ids(layout).into_iter().map(|d| format!("%{d}")).collect();
    let leaf_set: HashSet<String> = leaves.iter().cloned().collect();
    if ids.len() != panes.len() || leaves.len() != panes.len() || leaf_set != ids {
        return Err(Invalid);
    }
    // remap_layout con la identidad: el cuerpo no cambia; solo el checksum.
    let Some((_, body)) = layout.split_once(',') else {
        return Err(Invalid);
    };
    if format!("{:04x},{body}", layout_checksum(body)) != *layout {
        return Err(Invalid);
    }
    Ok(())
}

fn check_session(session: &Value) -> Result<(), Snapshot> {
    use Snapshot::{Exotic, Invalid};
    let windows = match session.as_object().and_then(|s| s.get("windows")) {
        Some(Value::Array(w)) if !w.is_empty() => w,
        _ => return Err(Invalid),
    };
    if active_count(windows) != Some(1) {
        return Err(Invalid);
    }
    let mut indexes = HashSet::new();
    for window in windows {
        let key = match window.get("index") {
            Some(Value::Number(n)) if is_int_text(n.as_str()) => {
                IndexKey::Int(if n.as_str() == "-0" { "0".into() } else { n.as_str().to_owned() })
            }
            Some(Value::String(s)) => IndexKey::Str(s.clone()),
            Some(Value::Null) => IndexKey::Null,
            // bool y float colisionan con enteros en un set de Python.
            Some(Value::Bool(_) | Value::Number(_)) => return Err(Exotic),
            _ => return Err(Invalid),
        };
        indexes.insert(key);
    }
    if indexes.len() != windows.len() {
        return Err(Invalid);
    }
    windows.iter().try_for_each(check_window)
}

pub fn check_snapshot(data: &Value) -> Snapshot {
    let Some(obj) = data.as_object() else {
        return Snapshot::Invalid;
    };
    let version_two = match obj.get("version") {
        Some(Value::Number(n)) => n.as_f64() == Some(2.0),
        _ => false,
    };
    let Some(Value::Object(sessions)) = obj.get("sessions") else {
        return Snapshot::Invalid;
    };
    if !version_two {
        return Snapshot::Invalid;
    }
    match sessions.values().try_for_each(check_session) {
        Ok(()) => Snapshot::Valid,
        Err(result) => result,
    }
}
```

En `crates/comandos-core/src/workspace.rs`, junto a `pub mod layout;`, añadir `pub mod snapshot;`.

- [ ] **Step 4: Ver pasar el core**

Run: `$C test -p comandos-core --test workspace_snapshot -j 6`
Expected: PASS (4 pruebas).

- [ ] **Step 5: Pruebas que fallan de las rutas**

Crear `crates/comandos-server/tests/dash_native_workspace.rs`:

```rust
//! Dominio C: workspace por el frente nativo, interoperando con el Python.
mod support;

use serde_json::{Value, json};
use support::{
    FakeLegacy, TestHome, dead_port, front, get, oracle::oracle, request_body, tmux_available,
};

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// El documento de un payload: sin `revision` ni `ready`.
fn document_of(payload: &Value) -> Value {
    let mut doc = payload.as_object().unwrap().clone();
    doc.remove("revision");
    doc.remove("ready");
    Value::Object(doc)
}

fn revisions(home: &TestHome) -> i64 {
    let conn = rusqlite::Connection::open(home.state_db()).unwrap();
    conn.query_row("SELECT COALESCE(MAX(revision), 0) FROM workspace_current", [], |r| r.get(0))
        .unwrap_or(0)
}

#[tokio::test]
async fn workspace_get_syncs_registry_once() {
    let home = TestHome::new("ws-get");
    home.write("app-tabs.json", r#"{"s1": "Uno"}"#);
    let front = front(&home, dead_port(), home.options()).await;
    let first = parse(&get(front.port, "/workspace").await.text());
    assert_eq!(first["revision"], 1);
    assert_eq!(first["ready"], true);
    let keys: Vec<&str> = first.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(&keys[keys.len() - 2..], ["revision", "ready"]);
    assert_eq!(parse(&get(front.port, "/workspace").await.text())["revision"], 1, "sin cambios no hay revisión");
    home.write("app-tabs.json", r#"{"s1": "Uno", "s2": "Dos"}"#);
    assert_eq!(parse(&get(front.port, "/workspace").await.text())["revision"], 2);
    home.write("app-tabs.json", "{roto");
    assert_eq!(parse(&get(front.port, "/workspace").await.text())["revision"], 2, "registro ilegible: no se toca");
    front.stop().await;
}

#[tokio::test]
async fn workspace_post_validation_conflict_and_replay() {
    let home = TestHome::new("ws-post");
    home.write("app-tabs.json", r#"{"s1": "Uno"}"#);
    let front = front(&home, dead_port(), home.options()).await;
    let current = parse(&get(front.port, "/workspace").await.text());
    let rev = current["revision"].as_i64().unwrap();
    let doc = document_of(&current);
    let post = |body: Value| {
        let port = front.port;
        async move { request_body(port, "POST", "/workspace", "", &body.to_string()).await }
    };
    for bad in [json!("1"), json!(true), json!(1.0)] {
        let wire = post(json!({"document": doc, "expectedRevision": bad, "requestId": "r"})).await;
        assert_eq!((wire.status, wire.text().as_str()), (400, r#"{"error": "expectedRevision inválido"}"#));
    }
    let wire = post(json!({"document": doc, "expectedRevision": rev})).await;
    assert_eq!((wire.status, wire.text().as_str()), (400, r#"{"error": "requestId inválido"}"#));
    let mut other = doc.clone();
    other["tabs"].as_object_mut().unwrap().insert("zz".into(), other["tabs"]["s1"].clone());
    let wire = post(json!({"document": other, "expectedRevision": rev, "requestId": "r0"})).await;
    assert_eq!(wire.status, 400);
    let saved = post(json!({"document": doc, "expectedRevision": rev, "requestId": "r1"})).await;
    assert_eq!(saved.status, 200, "{}", saved.text());
    assert_eq!(parse(&saved.text())["revision"], rev + 1);
    let replay = post(json!({"document": doc, "expectedRevision": rev, "requestId": "r1"})).await;
    assert_eq!(parse(&replay.text())["revision"], rev + 1, "mismo requestId: misma revisión");
    let stale = post(json!({"document": doc, "expectedRevision": rev, "requestId": "r2"})).await;
    assert_eq!(stale.status, 409);
    let body = parse(&stale.text());
    assert_eq!(body["error"], "Revisión desactualizada");
    assert_eq!(body["current"]["revision"], rev + 1);
    front.stop().await;
}

#[tokio::test]
async fn sort_restore_and_close_group_preview() {
    let home = TestHome::new("ws-sort");
    let legacy = FakeLegacy::start().await;
    home.write("app-tabs.json", r#"{"s1": "Uno", "s2": "Dos"}"#);
    if tmux_available() {
        let ok = std::process::Command::new("tmux")
            .args(["new-session", "-d", "-s", "s1", "cat"])
            .env_remove("TMUX")
            .env("TMUX_TMPDIR", home.tmux_dir())
            .status()
            .unwrap()
            .success();
        assert!(ok);
    }
    let front = front(&home, legacy.port, home.options()).await;
    let current = parse(&get(front.port, "/workspace").await.text());
    let ids: Vec<String> = current["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["id"].as_str().unwrap().to_owned())
        .collect();
    assert!(ids.len() >= 2, "{current}");
    let mut reversed = ids.clone();
    reversed.reverse();
    let sorted = request_body(front.port, "POST", "/workspace/sort", "", &json!({"restore": reversed}).to_string()).await;
    assert_eq!(sorted.status, 200, "{}", sorted.text());
    let sorted = parse(&sorted.text());
    assert_eq!(sorted["previous"], json!(ids));
    let keys: Vec<&str> = sorted.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys.last(), Some(&"previous"));
    // `by` necesita /state: lo responde el heredado.
    let by = request_body(front.port, "POST", "/workspace/sort", "", r#"{"by": "name"}"#).await;
    assert_eq!(by.text(), r#"{"legacy": true}"#);
    let missing = get(front.port, "/workspace/close-group?groupId=nope").await;
    assert_eq!(missing.status, 404);
    let group = &sorted["groups"][0]["id"];
    let preview = parse(&get(front.port, &format!("/workspace/close-group?groupId={}", group.as_str().unwrap())).await.text());
    let keys: Vec<&str> = preview.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["groupId", "members", "revision"]);
    front.stop().await;
}

#[tokio::test]
async fn workspace_client_get_and_save() {
    let home = TestHome::new("ws-client");
    let front = front(&home, dead_port(), home.options()).await;
    assert_eq!(get(front.port, "/workspace/client?deviceId=d1").await.text(), "{}");
    let saved = request_body(front.port, "POST", "/workspace/client", "", r#"{"deviceId": "d1", "activeTabId": "local"}"#).await;
    assert_eq!(saved.status, 200, "{}", saved.text());
    assert_eq!(get(front.port, "/workspace/client?deviceId=d1").await.text(), saved.text());
    let bad = request_body(front.port, "POST", "/workspace/client", "", r#"{"deviceId": 5}"#).await;
    assert_eq!((bad.status, bad.text().as_str()), (400, r#"{"error": "deviceId inválido"}"#));
    front.stop().await;
}

#[tokio::test]
async fn exotic_snapshot_declines_before_any_commit() {
    let home = TestHome::new("ws-exotic");
    let legacy = FakeLegacy::start().await;
    home.write("app-tabs.json", r#"{"s1": "Uno"}"#);
    home.write(
        "app-sessions-v2.json",
        &json!({"version": 2, "sessions": {"s1": {"windows": [
            {"index": true, "name": "w", "active": 1, "width": 80, "height": 24, "layout": "b25e,80x24,0,0,1",
             "panes": [{"id": "%1", "active": true}]}]}}})
        .to_string(),
    );
    let front = front(&home, legacy.port, home.options()).await;
    assert_eq!(get(front.port, "/workspace").await.text(), r#"{"legacy": true}"#);
    assert_eq!(revisions(&home), 0, "declinar nunca confirma una revisión");
    front.stop().await;
}

#[tokio::test]
async fn workspace_sync_interop_no_duplicate_revision() {
    let home = TestHome::new("ws-interop");
    home.write("app-tabs.json", r#"{"s1": "Uno"}"#);
    let front = front(&home, dead_port(), home.options()).await;
    let Some(py) = oracle(&home).await else {
        front.stop().await;
        return;
    };
    let rs = get(front.port, "/workspace").await.text();
    let python = get(py.port, "/workspace").await.text();
    assert_eq!(rs, python, "mismos bytes y misma revisión");
    home.write("app-tabs.json", r#"{"s1": "Uno", "s2": "Dos"}"#);
    // Veinte sincronizaciones concurrentes, mitad en cada proceso.
    let mut tasks = Vec::new();
    for i in 0..20 {
        let port = if i % 2 == 0 { front.port } else { py.port };
        tasks.push(tokio::spawn(async move { get(port, "/workspace").await.text() }));
    }
    let mut bodies = Vec::new();
    for task in tasks {
        bodies.push(task.await.unwrap());
    }
    assert!(bodies.iter().all(|b| *b == bodies[0]), "todas ven el mismo acomodo");
    assert_eq!(parse(&bodies[0])["revision"], 2, "una sola revisión nueva para el mismo inventario");
    for target in ["/workspace/client?deviceId=x", "/workspace/close-group?groupId=nope"] {
        let (a, b) = (get(py.port, target).await, get(front.port, target).await);
        assert_eq!((a.status, a.text()), (b.status, b.text()), "{target}");
    }
    front.stop().await;
}
```

- [ ] **Step 6: Ver el fallo**

Run: `$C test -p comandos-server --test dash_native_workspace -j 6`
Expected: FAIL: 502 del puerto muerto en las rutas de workspace.

- [ ] **Step 7: Implementar `workspace.rs`**

Crear `crates/comandos-server/src/dash/native/workspace.rs`:

```rust
//! C. Workspace (`bin/cc-dash` 6446–6570, 8614, 8657, 8666, 8789, 8792, 8878).
//! `workspace_sync` corre entera en el worker; la identidad de sesión (tmux)
//! fuera. POST /workspace/close-group sigue en el Python (close_app_tab).
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb, files,
    light::{HIDDEN_SESSIONS, data, error, favorites_set, ordered_tab_keys, read_prefs},
    py,
    query::Query,
    reply,
    state::StateBackend,
    tmux::Tmux,
};
use crate::{HandlerError, Request};
use comandos_core::{
    json::{python_eq, truthy, workspace_dumps_with_options},
    workspace::{
        close_group_preview, empty_document, layout::restore_order, pane_bindings, reconcile,
        snapshot::{Snapshot, check_snapshot},
        validate_document,
    },
};
use comandos_store::workspace::{Error as WsError, WorkspaceState, WorkspaceStore};
use http::StatusCode;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRoute {
    Get,
    ClosePreview,
    ClientGet,
    Save,
    Sort,
    ClientSave,
}

const fn entry(verb: Verb, key: Key, route: WorkspaceRoute) -> Entry {
    Entry { verb, key, route: NativeRoute::Workspace(route) }
}

pub const ROUTES: &[Entry] = &[
    entry(Verb::Get, Key::Path("/workspace"), WorkspaceRoute::Get),
    entry(Verb::Get, Key::Path("/workspace/close-group"), WorkspaceRoute::ClosePreview),
    entry(Verb::Get, Key::Path("/workspace/client"), WorkspaceRoute::ClientGet),
    entry(Verb::Post, Key::Raw("/workspace"), WorkspaceRoute::Save),
    entry(Verb::Post, Key::Raw("/workspace/sort"), WorkspaceRoute::Sort),
    entry(Verb::Post, Key::Raw("/workspace/client"), WorkspaceRoute::ClientSave),
];

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

/// `workspace_store()`: fase `ready` desde el primer uso (6446).
fn store(conn: &rusqlite::Connection) -> Result<WorkspaceStore<'_>, Fault> {
    let mut store = WorkspaceStore::new(conn);
    store.set_phase("ready").map_err(|_| failure())?;
    Ok(store)
}

/// `read_snapshot` (tmux_snapshot.py 244): archivo, luego `.bak`, luego vacío.
pub fn read_snapshot(path: &Path) -> Result<Value, Fault> {
    let backup = PathBuf::from(format!("{}.bak", path.display()));
    for candidate in [path.to_path_buf(), backup] {
        if let Some(data) = files::read_json(&candidate) {
            match check_snapshot(&data) {
                Snapshot::Valid => return Ok(data),
                Snapshot::Exotic => return Err(Fault::Decline),
                Snapshot::Invalid => {}
            }
        }
    }
    Ok(json!({"version": 2, "sessions": {}}))
}

/// `workspace_inventory` (6479); `None` = `_RegistryUnreadable`.
fn inventory(hooks: &Path) -> Result<Option<Vec<(String, Option<String>)>>, Fault> {
    let labels: Vec<(String, String)> = match files::read_json_strict(&hooks.join("app-tabs.json")) {
        files::Strict::Missing => Vec::new(),
        files::Strict::Value(Value::Object(map)) => map
            .into_iter()
            .filter_map(|(k, v)| match v {
                Value::String(s) if !s.is_empty() => Some((k, s)),
                _ => None,
            })
            .collect(),
        _ => return Ok(None),
    };
    let favorites = favorites_set(&read_prefs(hooks))?;
    let mut out = vec![("local".to_owned(), Some("⌂ local".to_owned()))];
    for (sess, label) in ordered_tab_keys(&labels, &favorites) {
        if !HIDDEN_SESSIONS.contains(&sess.as_str()) && py::is_session(sess) {
            out.push((sess.clone(), Some(label.clone())));
        }
    }
    Ok(Some(out))
}

/// `workspace_panes` (6493).
fn panes(
    hooks: &Path,
    inventory: &[(String, Option<String>)],
) -> Result<HashMap<String, Vec<(String, Value)>>, Fault> {
    let snapshot = read_snapshot(&hooks.join("app-sessions-v2.json"))?;
    let sessions = &snapshot["sessions"];
    let mut out = HashMap::new();
    for (tab, _) in inventory {
        let bindings = pane_bindings(sessions.get(tab).unwrap_or(&Value::Null))
            .map_err(|_| failure())?;
        out.insert(tab.clone(), bindings);
    }
    Ok(out)
}

fn sync_request_id(revision: i64, wanted: &Value) -> Result<String, Fault> {
    let encoded = workspace_dumps_with_options(wanted, true, false).map_err(|_| Fault::Decline)?;
    let digest: String = Sha256::digest(encoded.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(format!("sync-{revision}-{}", &digest[..24]))
}

/// `workspace_sync` (6533). Solo declina antes de su commit.
pub fn sync(backend: &StateBackend, hooks: &Path, now_seconds: f64) -> Result<WorkspaceState, Fault> {
    let store = store(&backend.conn)?;
    for _ in 0..3 {
        let current = store.current().map_err(|_| failure())?.unwrap_or(WorkspaceState {
            revision: 0,
            document: empty_document(),
            recovered: false,
        });
        let Some(inventory) = inventory(hooks)? else {
            return Ok(current);
        };
        let panes = panes(hooks, &inventory)?;
        let wanted = reconcile(&current.document, &inventory, &panes).map_err(|_| failure())?;
        if current.revision != 0 && python_eq(&wanted, &current.document) {
            return Ok(current);
        }
        let request_id = sync_request_id(current.revision, &wanted)?;
        match store.commit(&json!(current.revision), &wanted, &request_id, "auto", now_seconds) {
            Ok(saved) => return Ok(saved),
            Err(WsError::Conflict { .. }) => continue,
            Err(WsError::EmptyInventory | WsError::NotReady(_)) => return Ok(current),
            Err(_) => return Err(failure()),
        }
    }
    store.current().map_err(|_| failure())?.ok_or_else(failure)
}

/// `workspace_payload` (6566).
pub fn payload(state: &WorkspaceState) -> Value {
    let mut doc = state.document.as_object().cloned().unwrap_or_default();
    doc.insert("revision".into(), json!(state.revision));
    doc.insert("ready".into(), json!(true));
    Value::Object(doc)
}

fn message_or(error: impl ToString, fallback: &str) -> Value {
    let message = error.to_string();
    json!({"error": if message.is_empty() { fallback.to_owned() } else { message }})
}

fn is_int(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Number(n)) if !n.as_str().contains(['.', 'e', 'E'])
        && !matches!(n.as_str(), "NaN" | "Infinity" | "-Infinity"))
}

/// `secrets.token_hex(12)`.
fn token_hex12() -> Result<String, Fault> {
    let mut bytes = [0u8; 12];
    getrandom::fill(&mut bytes).map_err(|_| failure())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// `workspace_session_identity` (6555). Las excepciones de tmux no se capturan.
async fn session_identity(tmux: &Tmux, sess: &str) -> Result<Value, Fault> {
    if sess == "local" {
        return Ok(json!("local"));
    }
    if !py::is_session(sess) {
        return Ok(Value::Null);
    }
    let out = tmux
        .run(&["display-message", "-p", "-t", &format!("={sess}:"), "#{session_id}"])
        .await
        .map_err(|e| Fault::Error(e.uncaught()))?;
    let value = if out.ok { py::strip(&out.stdout) } else { "" };
    let is_id = value
        .strip_prefix('$')
        .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()));
    Ok(if is_id { json!(value) } else { Value::Null })
}

type Job = Result<(StatusCode, Value), Fault>;

async fn run(native: &Native, job: impl FnOnce(&mut StateBackend) -> Job + Send + 'static) -> Answer {
    let (status, body) = native.with_state(job).await??;
    reply(status, &body)
}

pub async fn answer(native: &Native, route: WorkspaceRoute, request: &Request) -> Answer {
    let hooks = native.options().hooks.clone();
    let now_seconds = (native.options().clock)() as f64 / 1000.0;
    match route {
        WorkspaceRoute::Get => {
            run(native, move |b| Ok((StatusCode::OK, payload(&sync(b, &hooks, now_seconds)?)))).await
        }
        WorkspaceRoute::ClosePreview => {
            let query = Query::parse(&request.target)?;
            let group = query.first("groupId").unwrap_or("").to_owned();
            let state = native.with_state(move |b| sync(b, &hooks, now_seconds)).await??;
            // Primer pase, puro: qué sesiones preguntaría el Python y si es un 404.
            let mut asked: Vec<String> = Vec::new();
            if let Err(e) = close_group_preview(&state.document, &group, |sess| {
                asked.push(sess.to_owned());
                Value::Null
            }) {
                return error(StatusCode::NOT_FOUND, &e.to_string());
            }
            let mut identities: HashMap<String, Value> = HashMap::new();
            for sess in asked {
                if !identities.contains_key(&sess) {
                    let id = session_identity(&native.options().tmux, &sess).await?;
                    identities.insert(sess, id);
                }
            }
            let preview = close_group_preview(&state.document, &group, |sess| {
                identities.get(sess).cloned().unwrap_or(Value::Null)
            })
            .map_err(|_| failure())?;
            let mut out: Map<String, Value> = preview.as_object().cloned().ok_or_else(failure)?;
            out.insert("revision".into(), json!(state.revision));
            reply(StatusCode::OK, &Value::Object(out))
        }
        WorkspaceRoute::ClientGet => {
            let query = Query::parse(&request.target)?;
            let device = query.first("deviceId").unwrap_or("").to_owned();
            run(native, move |b| match store(&b.conn)?.client(&device) {
                Ok(Some(state)) if truthy(&state) => Ok((StatusCode::OK, state)),
                Ok(_) => Ok((StatusCode::OK, json!({}))),
                // `json.loads` de un estado roto: el texto del ValueError es de Python.
                Err(WsError::Json(_)) => Err(Fault::Decline),
                Err(_) => Err(failure()),
            })
            .await
        }
        WorkspaceRoute::Save => {
            let data = data(request)?.clone();
            if !is_int(data.get("expectedRevision")) {
                return error(StatusCode::BAD_REQUEST, "expectedRevision inválido");
            }
            run(native, move |b| {
                let current = sync(b, &hooks, now_seconds)?;
                let store = store(&b.conn)?;
                let document = data.get("document").cloned().unwrap_or(Value::Null);
                if let Err(e) = validate_document(&document) {
                    return Ok((StatusCode::BAD_REQUEST, message_or(e, "Workspace inválido")));
                }
                let tabs = |d: &Value| -> Option<HashSet<String>> {
                    d.get("tabs").and_then(Value::as_object).map(|m| m.keys().cloned().collect())
                };
                if tabs(&document) != tabs(&current.document) {
                    return Ok((
                        StatusCode::BAD_REQUEST,
                        json!({"error": "La distribución no coincide con las tabs abiertas"}),
                    ));
                }
                // `_ident(request_id)`: un no-str es el mismo 400.
                let Some(Value::String(request_id)) = data.get("requestId") else {
                    return Ok((StatusCode::BAD_REQUEST, json!({"error": "requestId inválido"})));
                };
                let expected = data.get("expectedRevision").cloned().unwrap_or(Value::Null);
                match store.commit(&expected, &document, request_id, "user", now_seconds) {
                    Ok(saved) => Ok((StatusCode::OK, payload(&saved))),
                    Err(WsError::Conflict { current: Some(c), message }) => {
                        Ok((StatusCode::CONFLICT, json!({"error": message, "current": payload(&c)})))
                    }
                    Err(WsError::Invalid(m)) => Ok((StatusCode::BAD_REQUEST, message_or(m, "Workspace inválido"))),
                    Err(_) => Err(failure()),
                }
            })
            .await
        }
        WorkspaceRoute::Sort => {
            let data = data(request)?;
            // `by` usa read_states_cached (/state, no portada): al Python.
            let Some(Value::Array(restore)) = data.get("restore") else {
                return Err(Fault::Decline);
            };
            let ids: Vec<String> = restore
                .iter()
                .map(|v| v.as_str().map(str::to_owned))
                .collect::<Option<_>>()
                .ok_or(Fault::Decline)?;
            run(native, move |b| {
                for _ in 0..3 {
                    let current = sync(b, &hooks, now_seconds)?;
                    let store = store(&b.conn)?;
                    // `[g["id"] for g in doc.get("groups", [])]`, fuera del try: KeyError → 500.
                    let previous: Vec<Value> = match current.document.get("groups").and_then(Value::as_array) {
                        None => Vec::new(),
                        Some(groups) => groups
                            .iter()
                            .map(|g| g.get("id").cloned().ok_or_else(failure))
                            .collect::<Result<Vec<Value>, Fault>>()?,
                    };
                    let wanted = match restore_order(&current.document, &ids) {
                        Ok(w) => w,
                        Err(e) => return Ok((StatusCode::BAD_REQUEST, message_or(e, "Orden inválido"))),
                    };
                    match store.commit(&json!(current.revision), &wanted, &token_hex12()?, "user", now_seconds) {
                        Ok(saved) => {
                            let mut body = payload(&saved);
                            if let Some(map) = body.as_object_mut() {
                                map.insert("previous".into(), Value::Array(previous));
                            }
                            return Ok((StatusCode::OK, body));
                        }
                        Err(WsError::Conflict { .. }) => continue,
                        Err(WsError::Invalid(m)) => {
                            return Ok((StatusCode::BAD_REQUEST, message_or(m, "Orden inválido")));
                        }
                        Err(_) => return Err(failure()),
                    }
                }
                Ok((
                    StatusCode::CONFLICT,
                    json!({"error": "El acomodo cambió mientras ordenaba; intenta de nuevo"}),
                ))
            })
            .await
        }
        WorkspaceRoute::ClientSave => {
            let data = Value::Object(data(request)?.clone());
            let Some(Value::String(device)) = data.get("deviceId").cloned() else {
                return error(StatusCode::BAD_REQUEST, "deviceId inválido");
            };
            run(native, move |b| match store(&b.conn)?.save_client(&device, &data, now_seconds) {
                Ok(state) => Ok((StatusCode::OK, state)),
                Err(WsError::Invalid(m)) => Ok((StatusCode::BAD_REQUEST, json!({"error": m}))),
                Err(_) => Err(failure()),
            })
            .await
        }
    }
}
```

En `native/mod.rs`: `pub mod workspace;`, variante `Workspace(workspace::WorkspaceRoute)`, `TABLES = &[light::ROUTES, events::ROUTES, notices::ROUTES, workspace::ROUTES]`, brazo `NativeRoute::Workspace(route) => workspace::answer(self, route, request).await,`. `light::{data, error, read_prefs, favorites_set, ordered_tab_keys, HIDDEN_SESSIONS}` ya son `pub`/`pub(crate)`.

Nota sobre `sync(&StateBackend)`: recibe `&StateBackend` (no `&mut`) porque `WorkspaceStore` solo presta la conexión; los cierres de `with_state` reciben `&mut StateBackend` y lo represtan.

- [ ] **Step 8: Ver pasar**

Run: `$C test -p comandos-server --test dash_native_workspace -j 6 && $C test -p comandos-core -j 6`
Expected: PASS (6 pruebas; las de oráculo y tmux se saltan con aviso si faltan).

- [ ] **Step 9: Fixture**

```
# --- 2b · C: workspace (nativas salvo POST /workspace/close-group)
{"name":"c-workspace","method":"GET","path":"/workspace","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"c-workspace-otra-vez","method":"GET","path":"/workspace","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"c-close-preview-nadie","method":"GET","path":"/workspace/close-group?groupId=no-existe","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"c-client","method":"GET","path":"/workspace/client?deviceId=parity-c","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"c-client-post","method":"POST","path":"/workspace/client","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"deviceId":"parity-c","activeTabId":"local"},"volatile":[],"expect":"same"}
{"name":"c-client-post-malo","method":"POST","path":"/workspace/client","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"deviceId":5},"volatile":[],"expect":"same"}
{"name":"c-save-malo","method":"POST","path":"/workspace","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"document":{},"expectedRevision":"1"},"volatile":[],"expect":"same"}
{"name":"c-save-tabs","method":"POST","path":"/workspace","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"document":{"schema":1,"groups":[],"tabs":{}},"expectedRevision":0,"requestId":"parity-c"},"volatile":[],"expect":"same"}
{"name":"c-sort-vacio","method":"POST","path":"/workspace/sort","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"restore":[]},"volatile":["/revision"],"expect":"same"}
{"name":"c-close-group-post","method":"POST","path":"/workspace/close-group","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"expectedRevision":"x"},"volatile":[],"expect":"same","forwarded":true}
```

(`c-sort-vacio` restaura un orden vacío: `restore_order` deja los grupos como estaban y confirma una revisión con `requestId` aleatorio en ambos lados; por eso `revision` es volátil solo si la copia tenía revisiones en vuelo — normalmente coincide.)

- [ ] **Step 10: Paridad**

Run: `$C build -p comandos-cli -p xtask -j 6 && .build/target/debug/xtask parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks --state-db ~/.local/state/comandos/app-state.sqlite3 --comandos .build/target/debug/comandos`
Expected: 0 DIFF; de workspace solo `POST /workspace/close-group` sigue en «reenviadas al heredado».

- [ ] **Step 11: fmt, clippy, commit**

```bash
$C fmt --all -- --check
$C clippy --workspace --all-targets -j 6 -- -D warnings
git add crates/comandos-core/src/workspace.rs crates/comandos-core/src/workspace/snapshot.rs \
  crates/comandos-core/tests/workspace_snapshot.rs crates/comandos-server/src/dash/native/mod.rs \
  crates/comandos-server/src/dash/native/workspace.rs crates/comandos-server/tests/dash_native_workspace.rs \
  xtask/parity/frente.jsonl
git commit -m "feat(dash): dominio C nativo — workspace, close-group (vista previa), client y sort por restore

workspace_sync entera en el worker con el requestId determinista del Python:
Rust y Python sincronizando a la vez no duplican revisiones. valid_snapshot
portado al core; layouts exóticos declinan antes de confirmar nada.
POST /workspace/close-group sigue en el heredado (close_app_tab).

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Dominio E — 37 rutas retiradas responden 410 como `GET /operator`

Desplegable sola. De las 39 rutas sin llamador vivo (inventario §1.12) se quitan GET/POST `/events/v2`, que ya son nativas (Tarea 4); las 37 restantes responden `410` con exactamente el cuerpo y las cabeceras de `GET /operator` (`_json(410, OPERATOR_RETIRED)`, 8370 y 5751): `Content-Type: application/json`, `Cache-Control: no-store`, cuerpo `{"error": "El chat de CommandOS se retiró; usa la barra de comandos", "code": "retired"}`. `/operator*` sigue reenviado (ya da ese 410 en el Python). La puerta de seguridad se aplica antes, igual que hoy.

Llaves (el estilo de comparación del Python en cada rama):
- 13 GET con `Key::Path`: `/pomodoro/report`, `/session-config-history`, `/project-profiles`, `/events`, `/dedication`, `/ui-log/summary`, `/session-brain`, `/usage/guard`, `/usage/changes`, `/usage/provider-compare`, `/usage/experiments`, `/usage/analytics`, `/usage/interactions`. Con `Path`, `/events` no captura `/events/v2` ni `/eventsx` (que siguen yendo a su ruta o al Python).
- 1 GET con `Key::Raw`: `/proxy` (sin consulta).
- 23 POST con `Key::Raw`: `/project-profile`, `/session/recover`, `/pause`, `/usage/capture`, `/chains/delete`, `/app/command`, `/event`, `/proxy`, `/optimization/default`, `/skill-toggle`, `/mcp-toggle`, `/usage/experiment`, `/usage/rating`, `/usage/refresh`, `/usage/quota`, `/usage/subscription`, `/usage/settings`, `/news/refresh`, `/models/refresh`, `/open-with-account`, `/tab-new`, `/harness/switch`, `/model/switch-cancel`.

Cambios de comportamiento aceptados (van a «Diferencias aceptadas» en la Tarea 8): todas dejan de ejecutar su acción; POST `/harness/switch` y `/model/switch-cancel` sin `session` pasaban por el `400 Nombre de sesion invalido` genérico y ahora dan 410. `dash/sw.js` no precachea `/events` (solo `/` y `/manifest.webmanifest`; `/events` está en su lista de bypass «live»), así que retirarla no rompe la PWA.

**Files:**
- Create: `crates/comandos-server/src/dash/native/retired.rs`
- Modify: `crates/comandos-server/src/dash/native/mod.rs`
- Create: `crates/comandos-server/tests/dash_native_retired.rs`
- Modify: `xtask/parity/frente.jsonl`

**Interfaces:**
- Produces: `retired::{GET_PATHS, POST_PATHS, ROUTES, answer, OPERATOR_RETIRED_ERROR}`, `NativeRoute::Retired`.

- [ ] **Step 1: Pruebas que fallan**

Crear `crates/comandos-server/tests/dash_native_retired.rs`:

```rust
//! Dominio E: rutas retiradas → 410 idéntico a GET /operator, y una guarda
//! que falla si alguna vuelve a tener llamador en el código del tablero.
mod support;

use comandos_server::dash::native::retired::{GET_PATHS, POST_PATHS};
use std::{fs, path::Path};
use support::{TestHome, dead_port, front, get, oracle::oracle, request_body};

const BODY: &str =
    r#"{"error": "El chat de CommandOS se retiró; usa la barra de comandos", "code": "retired"}"#;

#[tokio::test]
async fn retired_routes_answer_operator_410() {
    let home = TestHome::new("retired");
    let front = front(&home, dead_port(), home.options()).await;
    assert_eq!(GET_PATHS.len() + 1 + POST_PATHS.len(), 37);
    for path in GET_PATHS {
        for target in [path.to_string(), format!("{path}?x=1")] {
            let wire = get(front.port, &target).await;
            assert_eq!(wire.status, 410, "{target}");
            assert_eq!(wire.header("content-type"), Some("application/json"));
            assert_eq!(wire.header("cache-control"), Some("no-store"));
            assert_eq!(wire.text(), BODY);
        }
    }
    assert_eq!(get(front.port, "/proxy").await.status, 410);
    for path in POST_PATHS {
        let wire = request_body(front.port, "POST", path, "", "{}").await;
        assert_eq!((wire.status, wire.text().as_str()), (410, BODY), "{path}");
    }
    // Lo que no es exactamente una ruta retirada sigue su camino.
    assert_eq!(get(front.port, "/proxy?x=1").await.status, 502, "reenviada al heredado (muerto)");
    assert_eq!(get(front.port, "/eventsx").await.status, 502);
    assert_eq!(get(front.port, "/events/v2").await.status, 200, "nativa del dominio B");
    assert_eq!(request_body(front.port, "POST", "/proxy?x=1", "", "{}").await.status, 502);
    front.stop().await;
}

#[tokio::test]
async fn retired_body_is_pythons_operator_body() {
    let home = TestHome::new("retired-oracle");
    let front = front(&home, dead_port(), home.options()).await;
    if let Some(py) = oracle(&home).await {
        let python = get(py.port, "/operator").await;
        let rust = get(front.port, "/usage/guard").await;
        assert_eq!(
            (python.status, python.header("content-type"), python.text()),
            (rust.status, rust.header("content-type"), rust.text())
        );
    }
    front.stop().await;
}

/// Ninguna ruta retirada tiene llamador en el tablero ni en las apps.
#[test]
fn retired_routes_have_no_live_caller() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut sources = vec![
        repo.join("bin/cc-app"),
        repo.join("bin/cc-app-mac"),
        repo.join("bin/cc-notifyd"),
        repo.join("bin/cc-next"),
    ];
    for entry in fs::read_dir(repo.join("dash")).unwrap().flatten() {
        let path = entry.path();
        if matches!(path.extension().and_then(|e| e.to_str()), Some("js" | "html")) {
            sources.push(path);
        }
    }
    // sw.js solo nombra /events en su lista de rutas que nunca se cachean.
    let allowed = [("sw.js", "/events")];
    let paths = GET_PATHS.iter().chain(["/proxy"].iter()).chain(POST_PATHS.iter());
    let mut callers = Vec::new();
    for path in paths {
        for source in &sources {
            let Ok(text) = fs::read_to_string(source) else { continue };
            let name = source.file_name().unwrap().to_string_lossy().into_owned();
            if allowed.contains(&(name.as_str(), *path)) {
                continue;
            }
            for open in ['"', '\'', '`'] {
                for close in ['"', '\'', '`', '?'] {
                    if text.contains(&format!("{open}{path}{close}")) {
                        callers.push(format!("{name}: {path}"));
                    }
                }
            }
        }
    }
    assert!(callers.is_empty(), "rutas retiradas con llamador: {callers:?}");
}
```

- [ ] **Step 2: Ver el fallo**

Run: `$C test -p comandos-server --test dash_native_retired -j 6`
Expected: FAIL de compilación (`no retired in native`).

- [ ] **Step 3: Implementar**

Crear `crates/comandos-server/src/dash/native/retired.rs`:

```rust
//! E. Rutas sin llamador vivo (inventario §1.12): 410 con el cuerpo exacto de
//! `GET /operator` (`OPERATOR_RETIRED`, `bin/cc-dash:5751`).
use super::{Answer, Entry, Key, NativeRoute, Verb, reply};
use http::StatusCode;
use serde_json::json;

pub const OPERATOR_RETIRED_ERROR: &str = "El chat de CommandOS se retiró; usa la barra de comandos";

/// GET por ruta (consulta opcional). `/proxy` va aparte, sin consulta.
pub const GET_PATHS: [&str; 13] = [
    "/pomodoro/report",
    "/session-config-history",
    "/project-profiles",
    "/events",
    "/dedication",
    "/ui-log/summary",
    "/session-brain",
    "/usage/guard",
    "/usage/changes",
    "/usage/provider-compare",
    "/usage/experiments",
    "/usage/analytics",
    "/usage/interactions",
];

pub const POST_PATHS: [&str; 23] = [
    "/project-profile",
    "/session/recover",
    "/pause",
    "/usage/capture",
    "/chains/delete",
    "/app/command",
    "/event",
    "/proxy",
    "/optimization/default",
    "/skill-toggle",
    "/mcp-toggle",
    "/usage/experiment",
    "/usage/rating",
    "/usage/refresh",
    "/usage/quota",
    "/usage/subscription",
    "/usage/settings",
    "/news/refresh",
    "/models/refresh",
    "/open-with-account",
    "/tab-new",
    "/harness/switch",
    "/model/switch-cancel",
];

const fn get(path: &'static str) -> Entry {
    Entry { verb: Verb::Get, key: Key::Path(path), route: NativeRoute::Retired }
}

const fn post(path: &'static str) -> Entry {
    Entry { verb: Verb::Post, key: Key::Raw(path), route: NativeRoute::Retired }
}

pub const ROUTES: &[Entry] = &[
    get(GET_PATHS[0]), get(GET_PATHS[1]), get(GET_PATHS[2]), get(GET_PATHS[3]),
    get(GET_PATHS[4]), get(GET_PATHS[5]), get(GET_PATHS[6]), get(GET_PATHS[7]),
    get(GET_PATHS[8]), get(GET_PATHS[9]), get(GET_PATHS[10]), get(GET_PATHS[11]),
    get(GET_PATHS[12]),
    Entry { verb: Verb::Get, key: Key::Raw("/proxy"), route: NativeRoute::Retired },
    post(POST_PATHS[0]), post(POST_PATHS[1]), post(POST_PATHS[2]), post(POST_PATHS[3]),
    post(POST_PATHS[4]), post(POST_PATHS[5]), post(POST_PATHS[6]), post(POST_PATHS[7]),
    post(POST_PATHS[8]), post(POST_PATHS[9]), post(POST_PATHS[10]), post(POST_PATHS[11]),
    post(POST_PATHS[12]), post(POST_PATHS[13]), post(POST_PATHS[14]), post(POST_PATHS[15]),
    post(POST_PATHS[16]), post(POST_PATHS[17]), post(POST_PATHS[18]), post(POST_PATHS[19]),
    post(POST_PATHS[20]), post(POST_PATHS[21]), post(POST_PATHS[22]),
];

pub fn answer() -> Answer {
    reply(
        StatusCode::GONE,
        &json!({"error": OPERATOR_RETIRED_ERROR, "code": "retired"}),
    )
}
```

En `native/mod.rs`: `pub mod retired;`, variante `Retired`, `TABLES = &[light::ROUTES, events::ROUTES, notices::ROUTES, workspace::ROUTES, retired::ROUTES]`, brazo `NativeRoute::Retired => retired::answer(),`. `retired` va la última: ninguna de sus rutas coincide con las de otro dominio (`/events` con `Path` no es `/events/v2`).

- [ ] **Step 4: Ver pasar**

Run: `$C test -p comandos-server --test dash_native_retired -j 6`
Expected: PASS (3 pruebas).

- [ ] **Step 5: Fixture (el oráculo responde con `/operator`)**

El arnés compara la respuesta del frente en la ruta retirada con la del Python en `oracle_path` (Tarea 2). Añadir:

```
# --- 2b · E: retiradas → 410 de /operator (oracle_path)
{"name":"e-get-pomodoro-report","method":"GET","path":"/pomodoro/report","oracle_path":"/operator","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"e-get-session-config-history","method":"GET","path":"/session-config-history?session=a&pane=b","oracle_path":"/operator","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"e-get-project-profiles","method":"GET","path":"/project-profiles","oracle_path":"/operator","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"e-get-events","method":"GET","path":"/events","oracle_path":"/operator","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"e-get-dedication","method":"GET","path":"/dedication","oracle_path":"/operator","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"e-get-proxy","method":"GET","path":"/proxy","oracle_path":"/operator","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"e-get-ui-log-summary","method":"GET","path":"/ui-log/summary","oracle_path":"/operator","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"e-get-session-brain","method":"GET","path":"/session-brain","oracle_path":"/operator","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"e-get-usage-guard","method":"GET","path":"/usage/guard","oracle_path":"/operator","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"e-get-usage-changes","method":"GET","path":"/usage/changes","oracle_path":"/operator","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"e-get-usage-provider-compare","method":"GET","path":"/usage/provider-compare","oracle_path":"/operator","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"e-get-usage-experiments","method":"GET","path":"/usage/experiments","oracle_path":"/operator","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"e-get-usage-analytics","method":"GET","path":"/usage/analytics","oracle_path":"/operator","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"e-get-usage-interactions","method":"GET","path":"/usage/interactions","oracle_path":"/operator","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"e-post-project-profile","method":"POST","path":"/project-profile","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-session-recover","method":"POST","path":"/session/recover","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-pause","method":"POST","path":"/pause","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-usage-capture","method":"POST","path":"/usage/capture","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-chains-delete","method":"POST","path":"/chains/delete","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-app-command","method":"POST","path":"/app/command","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-event","method":"POST","path":"/event","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-proxy","method":"POST","path":"/proxy","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-optimization-default","method":"POST","path":"/optimization/default","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-skill-toggle","method":"POST","path":"/skill-toggle","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-mcp-toggle","method":"POST","path":"/mcp-toggle","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-usage-experiment","method":"POST","path":"/usage/experiment","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-usage-rating","method":"POST","path":"/usage/rating","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-usage-refresh","method":"POST","path":"/usage/refresh","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-usage-quota","method":"POST","path":"/usage/quota","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-usage-subscription","method":"POST","path":"/usage/subscription","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-usage-settings","method":"POST","path":"/usage/settings","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-news-refresh","method":"POST","path":"/news/refresh","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-models-refresh","method":"POST","path":"/models/refresh","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-open-with-account","method":"POST","path":"/open-with-account","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-tab-new","method":"POST","path":"/tab-new","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-harness-switch","method":"POST","path":"/harness/switch","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-post-model-switch-cancel","method":"POST","path":"/model/switch-cancel","oracle_path":"/operator","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{},"volatile":[],"expect":"same"}
{"name":"e-operator-sigue-reenviada","method":"GET","path":"/operator","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same","forwarded":true}
```

- [ ] **Step 6: Paridad**

Run: `$C build -p comandos-cli -p xtask -j 6 && .build/target/debug/xtask parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks --state-db ~/.local/state/comandos/app-state.sqlite3 --comandos .build/target/debug/comandos`
Expected: 0 DIFF; ninguna de las 37 rutas aparece en «reenviadas al heredado» (sí `GET /operator`).

- [ ] **Step 7: fmt, clippy, commit**

```bash
$C fmt --all -- --check
$C clippy --workspace --all-targets -j 6 -- -D warnings
git add crates/comandos-server/src/dash/native/mod.rs crates/comandos-server/src/dash/native/retired.rs \
  crates/comandos-server/tests/dash_native_retired.rs xtask/parity/frente.jsonl
git commit -m "feat(dash): 37 rutas sin llamador vivo responden el 410 de /operator

Inventario §1.12 menos /events/v2 (nativa). Mismo cuerpo y cabeceras que
GET /operator; una prueba falla si alguna vuelve a tener llamador en dash/,
cc-app, cc-app-mac, cc-notifyd o cc-next.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: Documento de cutover «2b: dominios nativos» y corrida completa

Desplegable sola (solo documentación); el cutover lo ejecuta el controlador.

**Files:**
- Modify: `docs/verification/cutover-dash.md` (la sección «Rutas sin llamador vivo (39)» y una sección nueva al final)

**Interfaces:** ninguna de código.

- [ ] **Step 1: Corrida completa antes de escribir**

```bash
$C test --workspace -j 6
$C build --release -p comandos-cli -j 6 && $C build -p xtask -j 6
.build/target/debug/xtask parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 --comandos .build/target/release/comandos
.build/target/debug/xtask parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 --comandos .build/target/release/comandos --no-native
.build/target/debug/xtask poll --shadow --minutes 10 --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 --comandos .build/target/release/comandos
```

Expected: todo en verde; las dos corridas de paridad con 0 DIFF (con `--no-native` las 37 retiradas dan DIFF por diseño: esa corrida se hace sin el bloque E del fixture, comentándolo con `#` en una copia en `/tmp`, y su lista de reenviadas incluye todas las rutas API); `poll` con pendiente de Pss ≈ 0 entre el minuto 5 y el 10. Anotar las cifras (Pss inicial/final del frente, número de rutas reenviadas con y sin nativo).

- [ ] **Step 2: Corregir la sección de rutas retiradas**

En `docs/verification/cutover-dash.md`, sustituir el párrafo que empieza «Siguen reenviadas al Python en esta fase. En la Fase 2b responderán…» por:

```markdown
Hasta la Fase 2a se reenviaban al Python. Desde la 2b, 37 de ellas responden en el frente
`410` con el cuerpo y las cabeceras exactas de `GET /operator`
(`{"error": "El chat de CommandOS se retiró; usa la barra de comandos", "code": "retired"}`);
GET/POST `/events/v2` no se retiran: son nativas (dominio B). Fuente: inventario §1.12.
La prueba `retired_routes_have_no_live_caller` falla si alguna vuelve a tener llamador.
```

y en las dos listas quitar `GET /events/v2` y `POST /events/v2` (la primera pasa a «Sin ningún llamador (11)» y el título de la sección a «Rutas sin llamador vivo (39 → 37 retiradas)»).

- [ ] **Step 3: Añadir la sección «2b: dominios nativos»**

Al final de `docs/verification/cutover-dash.md`:

````markdown
## 2b: dominios nativos

### Qué cambia

El frente responde él mismo, sin el Python, 63 rutas de cinco dominios; lo demás sigue igual
(reenviado a 4781). Detalle y líneas del Python en
`docs/superpowers/plans/2026-10-04-fase-2b-dominios-nativos-i.md` («Mapa de rutas nativas»).

| Dominio | Rutas nativas |
|---|---|
| D · lecturas ligeras | GET `/prefs`, `/tabs`, `/tab-history`, `/tab-models`, `/active-tab`, `/tmux-mouse`; POST `/prefs-set`, `/tmux-mouse` |
| B · eventos y marcas | GET/POST `/work-marks`, GET/POST `/events/v2` |
| A · notificaciones | GET `/notices`, `/notices/prefs`, `/notices/watch`, `/notifs/count`; POST `/presence`, `/notices/read`, `/notices/sound`, `/notices/prefs` |
| C · workspace | GET `/workspace`, `/workspace/close-group`, `/workspace/client`; POST `/workspace`, `/workspace/sort` (solo `restore`), `/workspace/client` |
| E · retiradas | las 37 de la sección anterior → 410 de `/operator` |

Siguen en el Python: POST `/workspace/close-group` (cierra pestañas con tmux), `/workspace/sort`
con `by`, `/notify-popup`, `/test`, push, `/operator*`, `/state` y todo lo no listado. Una
petición nativa con una entrada que Rust no reproduce con certeza (dígitos no ASCII, enteros
enormes, formas JSON raras, snapshot de tmux exótico) también se reenvía, sin haber escrito nada.

El frente abre `~/.local/state/comandos/app-state.sqlite3` (misma resolución que
`lib/app_state.py`: `COMANDOS_STATE_DB`, `XDG_STATE_HOME`, `~/.local/state`) con un único hilo
de base. Si la base tiene una migración que el binario no conoce (un Python más nuevo migró),
**todo** el conjunto nativo se apaga hasta reiniciar y el journal muestra una sola línea
`rutas nativas desactivadas, todo se reenvía al heredado`. Nunca se baja de versión.

Interruptores: `comandos dash --no-native` o `COMANDOS_DASH_NATIVE=0` (todo reenviado, la 2a
exacta) y `COMANDOS_DASH_TRACE_FORWARD=1` (una línea `comandos dash: reenvío MÉTODO /ruta` por
reenvío, sin consulta).

### Diferencias aceptadas (2b)

- Las 37 rutas retiradas dejan de ejecutar su acción y dan 410; POST `/harness/switch` y
  `/model/switch-cancel` sin `session` daban 400 `Nombre de sesion invalido` y ahora 410.
- GET `/workspace` y `/workspace/close-group` pueden confirmar una revisión desde Rust (como ya
  hacía el Python). El `requestId` de sincronización es determinista: Rust y Python a la vez no
  duplican revisiones.
- La caché de familias de `fc-list` (60 s) es por proceso: tras instalar una fuente, Rust y el
  Python pueden tardar distinto en verla (≤ 60 s).

### 0. Previos

```sh
cd ~/codebase/0xJesus/ComandOS && git log -1 --oneline        # el commit de la Tarea 8
CARGO_TARGET_DIR=$PWD/.build/target nice -n 10 cargo build --release -p comandos-cli -j 6
NEW=$PWD/.build/target/release/comandos
grep -qa COMANDOS_DASH_NATIVE "$NEW" || echo "BINARIO SIN 2b: no seguir"   # `dash --help` arrancaría el servidor
systemctl --user is-active cc-dash.service cc-dash-legacy.service    # active active
~/.local/share/comandos/bin/comandos install --releases              # anotar la actual (17aa1bea2309 o posterior)
```

### 1. Sombra en 4782 con nativo, contra el heredado 4781

La sombra usa la base real: sus rutas nativas escriben lo mismo que el tablero (presencia,
avisos leídos, workspace). **Nunca** `xtask poll` contra la sombra ni contra 4777: la carga va
siempre con `--shadow` (pila aislada).

```sh
COMANDOS_DASH_TRACE_FORWARD=1 "$NEW" dash 4782 --legacy-port 4781 2> /tmp/sombra-2b.log
```

Desde otra terminal:

```sh
xtask() { "$PWD/.build/target/debug/xtask" "$@"; }
xtask parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 --comandos "$NEW"          # 0 DIFF
xtask poll --shadow --minutes 10 --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 --comandos "$NEW"          # Pss plano
```

Navegación manual en `http://127.0.0.1:4782` con `chrome-bg` (vía `cc-browser-expose start 4782`):
índice, cajón de avisos (abrirlo, marcar uno leído, dejar abierto 30 s para ver el long-poll),
cambiar el tema (POST `/prefs-set`), activar el ratón de una pestaña, ordenar grupos y deshacer
(POST `/workspace/sort` con `restore`). Luego:

```sh
grep -c 'reenvío' /tmp/sombra-2b.log
grep 'reenvío' /tmp/sombra-2b.log | sed 's/.*reenvío //' | sort | uniq -c | sort -rn
```

Ninguna ruta de la tabla «Qué cambia» debe aparecer (salvo `GET /operator`). Ctrl+C para parar.

### 2. Cutover

```sh
"$NEW" install --stage
grep -qa COMANDOS_DASH_NATIVE "$(readlink -f ~/.local/share/comandos/bin/comandos)" || echo "STAGE MALO: comandos install --rollback-release"
~/.local/share/comandos/bin/comandos hook claude-status >/dev/null && echo "hooks OK con la release nueva"
systemctl --user restart cc-dash.service
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:4777/        # 200
journalctl --user -u cc-dash.service -n 20 --no-pager | grep -v 'rutas nativas desactivadas'   # sin esa línea
```

El tablero queda sin respuesta unos 3 s; `cc-app` reintenta solo. El Python de 4781 no se toca.

### 3. Verificación por dominio

Trazar 10 minutos de tráfico real y comprobar qué sigue llegando al heredado:

```sh
systemctl --user set-environment COMANDOS_DASH_TRACE_FORWARD=1 && systemctl --user restart cc-dash.service
sleep 600
journalctl --user -u cc-dash.service --since -11min --no-pager | grep 'reenvío' \
  | sed 's/.*reenvío //' | sort | uniq -c | sort -rn
systemctl --user unset-environment COMANDOS_DASH_TRACE_FORWARD && systemctl --user restart cc-dash.service
```

- **D**: no aparecen `GET /active-tab` (antes ~600 por 10 min con el tablero abierto), `/tab-models`, `/prefs`, `/tabs`, `/tab-history`, `/tmux-mouse`, `POST /prefs-set`, `POST /tmux-mouse`. Cambiar el tema en cc-app se refleja en el tablero y viceversa.
- **B**: no aparecen `/work-marks` ni `/events/v2`. Una marca puesta desde el tablero se ve en cc-app.
- **A**: no aparecen `/notices*`, `/notifs/count`, `/presence`. Con el cajón abierto, un aviso nuevo de un agente aparece en ≤ 1 s (long-poll) y la campana coincide con la de cc-app.
- **C**: de workspace solo aparece `POST /workspace/close-group` (al cerrar un grupo). Mover pestañas entre grupos en el tablero y en cc-app mantiene la misma `revision` en ambos (`curl -s 127.0.0.1:4777/workspace | jq .revision` y `curl -s 127.0.0.1:4781/workspace | jq .revision` iguales).
- **E**: ninguna de las 37.
- Memoria: `grep Pss /proc/$(systemctl --user show -p MainPID --value cc-dash.service)/smaps_rollup` al minuto 1 y al 10: plano (± 1 MiB).

### 4. Reversión

A/B inmediato sin cambiar binario (todo reenviado, comportamiento 2a):

```sh
systemctl --user set-environment COMANDOS_DASH_NATIVE=0 && systemctl --user restart cc-dash.service
# deshacer: systemctl --user unset-environment COMANDOS_DASH_NATIVE && systemctl --user restart cc-dash.service
```

Volver a la release anterior (17aa1bea2309, la 2a):

```sh
~/.local/share/comandos/bin/comandos install --releases
~/.local/share/comandos/bin/comandos install --rollback-release
systemctl --user restart cc-dash.service
```

Las escrituras hechas por las rutas nativas están en los mismos archivos y la misma base que
usa el Python, con el mismo formato: revertir no necesita migrar ni limpiar nada.
````

- [ ] **Step 4: Verificar el documento**

Run: `grep -n "2b: dominios nativos\|37 retiradas\|COMANDOS_DASH_NATIVE" docs/verification/cutover-dash.md`
Expected: las tres aparecen; ninguna mención queda de `{"error":"Ruta retirada"}`.

- [ ] **Step 5: Commit**

```bash
git add docs/verification/cutover-dash.md
git add -f docs/superpowers/plans/2026-10-04-fase-2b-dominios-nativos-i.md
git commit -m "docs(verification): cutover 2b — dominios nativos, verificación por dominio y reversión

Sombra con traza de reenvíos, paridad con copia de app-state, poll aislado
10 min, qué rutas deben desaparecer del tráfico del heredado por dominio y
reversión por entorno (COMANDOS_DASH_NATIVE=0) o por release.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

## Self-review

**Cobertura del encargo.**
- A (notificaciones, 8 rutas, long-poll ≤25 s con sondeo de 200 ms sin bloquear el worker): Tarea 5. `/notify-popup`, `/test` y push quedan fuera, como se pidió.
- B (eventos y marcas por `events_routes.rs`, con importación heredada y `_internal_producer`): Tarea 4.
- C (workspace contado del inventario: son 7, no 8; `POST /workspace/close-group` reenviada porque no hay piezas de tmux/`close_app_tab` en el runtime, dicho explícitamente): Tarea 6.
- D (`/prefs`, `POST /prefs` → `/prefs-set`, `/tabs`, `/tab-history`, `/tab-models`, `/active-tab`, GET/POST `/tmux-mouse`): Tarea 3.
- E (39 de §1.12 menos `/events/v2` = 37, 410 con el cuerpo de `/operator`; `/events` comprobado contra `sw.js`): Tarea 7.
- Rulings: 1 (bytes idénticos, `Reply::json` = `response_dumps`, textos del Python en cada prueba); 2 (un `BackendWorker` por base, ruta de `app_state.py`, migraciones del store, puerta de esquema que apaga todo con una línea): Tarea 1; 3 (long-poll async): Tarea 5; 4 (`tokio::process`, 5 s, `TMUX_TMPDIR`): Tarea 2; 5 (`Native` antes de `Forward`, coincidencia exacta por método, `--no-native`/`COMANDOS_DASH_NATIVE=0`): Tarea 1; 6 (fixture por tarea, netns, `--sequence` innecesario): Tareas 2–7; 7 (cutover): Tarea 8; 8 (convenciones): Global Constraints y cada commit; 9 (orden infra → D → B → A → C → E → docs, cada tarea desplegable): orden de las tareas.
- GET con efectos laterales (§9.7): señalados en «Decisiones» y en las tareas 4 y 6, con su reproducción (marca de importación única, `requestId` determinista).

**Huecos buscados.** Sin «TBD»/«similar a»: cada paso de código trae el código. Tipos coherentes entre tareas: `Native::with_state` → `Result<T, Fault>` (Tareas 1, 4, 5, 6); `NativeRoute` crece de vacío a `Light`/`Events`/`Notices`/`Workspace`/`Retired`; `TABLES` se amplía una vez por tarea; `light::{data, error, read_prefs, favorites_set, ordered_tab_keys, HIDDEN_SESSIONS}` (Tarea 3) los reutilizan las Tareas 5 y 6; `NativeOptions.{tmux, fc_list}` (Tarea 2) los usan `support::TestHome::options` y las Tareas 3, 5 y 6; `Stack::start` se conserva para `poll` y `parity` vía `start_with`.

**Riesgos que el implementador debe vigilar.**
- Las funciones del store y del core que se consumen (`list_notices`, `claim_sound`, `reconcile`, `close_group_preview`, `restore_order`, `WorkspaceStore::commit`) ya tienen fixtures contra el Python; si una prueba de oráculo de este plan diverge, el fallo está en la capa portada, y se corrige allí con su fixture, no en el adaptador.
- `prefs_set_errors_are_pythons` envía `NaN`/`Infinity` en el cuerpo: depende de que la admisión del transporte acepte lo que acepta `json.loads`. Si no, el defecto es de la admisión (2a) y se arregla allí con su propia prueba.
- `sort_restore_and_close_group_preview` asume que `reconcile` crea un grupo por pestaña nueva; si el core agrupa distinto, la prueba lo dice con el documento en el mensaje.

