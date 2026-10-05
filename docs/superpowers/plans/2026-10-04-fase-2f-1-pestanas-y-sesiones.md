# Fase 2f-1 — Pestañas, sesiones y teclas (corte `tabs`): plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** que el frente `comandos dash` responda él mismo, byte a byte igual que el `cc-dash` Python, todas las rutas del corte `tabs` —registro de pestañas, creación/revivido/cierre de sesiones, cuentas nuevas, conexiones SSH que abren pestañas, teclas, foco, desplazamiento, exportación y el cierre de panes— siendo el **único** escritor de `app-tabs-meta.json` mientras el corte está activo.

**Architecture:** un módulo de registro (`native/tab_registry.rs`) porta `register_app_tab`, `write_tab_metadata`, `remove_tab_metadata`, `tab_metadata_for_session`, `remember_tab`, `write_app_tab` y `close_app_tab` con el mismo `flock` de `<archivo>.lock` que el Python y cc-app (con espera, en `spawn_blocking`) y un `tokio::sync::Mutex` del frente en el papel de `TAB_METADATA_LOCK`. Las rutas se reparten en `tabs.rs` (registro y cierre), `sessions.rs` (crear y revivir sesiones) e `input.rs` (teclas, foco, desplazamiento, exportación); el cierre de panes completa `terminal.rs` (2c). El preámbulo común de los POST (`target::post_target`), la identidad de pane, `tmux_snapshot` y `closed_panes` vienen del plan maestro (Tarea 3).

**Tech Stack:** Rust 1.96, tokio 1.53 (`process`, `sync`, `time`), hyper 1.11, rusqlite 0.40 (worker de app-state), serde_json (`preserve_order`, `arbitrary_precision`), `shlex`-equivalente propio (`py::shlex_quote`, Tarea 3).

**Spec:** `docs/superpowers/specs/2026-10-04-comandos-rust-complete-design.md` (§4.2, §5). Plan maestro: `docs/superpowers/plans/2026-10-04-fase-2f-resto-del-tablero.md` (D2, D4, D5, D8, D12; Tareas 0–3). Inventario: `docs/research/2026-10-04-fase-2-inventario.md` §1.3. Oráculo: `bin/cc-dash` (líneas de `0aa4ae1`; localizar por nombre de función).

**Precondición:** Tareas 0, 1, 2 y 3 del plan maestro en `migration/rust-fase2f`. Este sub-plan es el grupo **G1**; trabaja en `migration/rust-fase2f-tabs`.

---

## Rulings que aplican (del plan maestro)

1. Respuestas idénticas byte a byte (`response_dumps`), textos de error literales.
2. `Decline` solo antes de cualquier efecto. **En el corte `tabs` no hay `Decline` después de leer los archivos del registro** (D2 del maestro): un caso incierto en un archivo del registro se responde `500 {"error": "Error interno del tablero"}` y una línea en stderr (`comandos dash: <archivo> incierto; <ruta> responde 500`). Diferencia aceptada: el Python habría tratado ese archivo con `load_json_file` (cualquier excepción → valor por omisión); solo ocurre con anidamiento ≥ 1000 o sustitutos sueltos en un JSON del registro.
3. app-state solo por `BackendWorker<StateBackend>` (`workspace::sync`, `close_group`, reclamos de terminal rápida).
4. tmux por `Tmux` (`tokio::process`, 5 s); `scope_cmd` (`systemd-run --user --scope --collect --quiet` + argv) con el plazo de 15 s del Python; `ssh` con sus plazos (14 s la prueba de llave, 3 s `ssh -O check`); `git` con 8 s / 30 s (`make_worktree`).
5. Nada bloqueante en el runtime: `flock` con espera, `glob` de `~/codebase`, `H/state/*.json` y escrituras con `fsync` → `spawn_blocking`.
6. Paridad: fixture `xtask/parity/2f/tabs.jsonl` y pruebas contra el oráculo por ruta con el gemelo (`support::twin`).

## Global Constraints

- Todo con `CARGO_TARGET_DIR=/home/someguy/codebase/0xJesus/ComandOS/.build/target nice -n 10 cargo <cmd> -j 6` (`$C`). Antes de cada commit: `$C fmt --all -- --check`, `$C clippy --workspace --all-targets -j 6 -- -D warnings` y las pruebas de `comandos-server`.
- Commits con `git add <rutas>`; mensajes en español, línea en blanco y `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; fixture con `git add -f xtask/parity/2f/tabs.jsonl`. Comentarios en español, identificadores en inglés; sin `unsafe`, `unwrap`, `expect` ni indexado fuera de pruebas; cero Python o bash nuevos.
- Solo se editan los archivos que nombra cada tarea (D4 del maestro): nunca `native/mod.rs`, `tests/support/mod.rs` ni `frente.jsonl`.
- **Regla tmux (vinculante, `CLAUDE.md`; el 4 de octubre a las 21:59 un subagente mató las ~20 sesiones vivas del usuario con `TMUX_TMPDIR=<dir borrado> tmux kill-server`):**
  - Todo tmux de prueba con socket explícito `-S <dir>/tmux-<uid>/default` (o `-L <etiqueta propia>`): `Tmux::private(dir)` en el frente, `TestHome::tmux_command()`/`support::run_tmux` en las pruebas, `private_tmux(dir)` en `xtask`. **Nunca `TMUX_TMPDIR` solo.**
  - El Python del oráculo y del gemelo solo corre con el `fakebin` que contiene el `tmux` guardián (`support::oracle::tmux_guard`, que fuerza `-S` y sale con 97 si el directorio no existe) y el `systemd-run` falso (`support::twin::SCOPE_RUNNER`).
  - Limpieza: `kill-server` con el `-S` propio y después borrar el directorio (el `Drop` de `TestHome`).
  - Prohibido `kill-server`, `kill-session`, `pkill tmux` o `kill -9 -1` sin `-S`/`-L` propio; **prohibido ejecutar tmux a mano**. Las pruebas de `/kill` matan solo sesiones del servidor privado sembradas por la propia prueba.
  - `support::assert_private_tmux(&opts)` en toda prueba con frente (lo hace `support::front`).
  - Cada tarea mutadora tiene sus apartados **Confinamiento** y **Efectos en vivo**.

## Decisiones de este sub-plan

- **E1 — Candados.** `app-tabs.json` y `app-tabs-history.json` llevan el `flock` de `<archivo>.lock` (`file_lock` del Python, que cc-app también toma): el frente lo pide **con espera** (`FileLock::acquire` en `spawn_blocking`), como el Python. `app-tabs-meta.json` no lleva `flock` en el Python, solo `TAB_METADATA_LOCK` (de hilo): el frente usa `Native`-local `tab_meta: tokio::sync::Mutex<()>` (campo en `tab_registry::Registry`, guardado en un `OnceLock` del módulo para no tocar `native/mod.rs`). Con el corte activo nadie más escribe ese archivo (todo escritor está en este corte; cc-app-mac lo hace por HTTP).
- **E2 — Orden de efectos idéntico.** Cada ruta ejecuta las mismas órdenes de tmux y escrituras en el mismo orden que el Python; las pruebas del gemelo comparan además el registro de órdenes de tmux (`support::tabs::TmuxLog`: un `tmux` envoltorio en el `fakebin` del gemelo que anota sus argumentos en `<HOME>/tmux.log` antes de delegar en el guardián) para el lado Python, y el mismo envoltorio en `opts.tmux.program` para el frente.
- **E3 — `send-keys` diferidos.** `/session-new` y `/account/add` teclean el comando del agente 1,5 s después en un hilo del Python; el frente lo hace con `native.tasks().spawn(…)` (D12): `tokio::time::sleep(1.5 s)` + `send-keys -t =<s>: <cmd> Enter`. Si el frente se apaga antes, el comando no se teclea (igual que si el Python muere). Ninguna respuesta depende de ello.
- **E4 — Reloj de nombres.** `term-r<n>` con `n = int(time.time()) % 100000` (`/session-new`) o `int(time.time()*10) % 1000000` (`/account/add`) desde `opts.clock_seconds`; colisiones se resuelven con `has-session` incrementando, como el Python.

## Mapa de rutas

| Ruta | Llave | Python | Tarea |
|---|---|---|---|
| POST `/tab-register`, `/tab-metadata`, `/tab-metadata-remove`, `/tab-close` | Raw | 9494–9530 | T2 |
| POST `/workspace/close-group` | Raw | 8716 | T2 |
| POST `/recover-tab`, `/ensure`, `/new`, `/shell`, `/up` | Raw | 9531, 9704, 9728, 9749, 9765 | T3 |
| POST `/session-new`, `/account/add` | Raw | 9206, 9411 (árbol vivo: `account_add_request`) | T4 |
| POST `/terminal/quick` sin `place:"sidebar"` | Raw | 9393 | T5 |
| POST `/ssh-connect`, `/ssh-new-tab` | Raw | 9121, 9128 | T5 |
| POST `/send`, `/paste`, `/key`, `/focus`, `/kill`, `/tmux-scroll`, `/export` | Raw | 9625–9703, 9487 | T6 |
| POST `/terminal-panes` con `action:"close"` | Raw (rama de 2c) | 8771 | T7 |

## Idempotencia y orden frente al Python (ruling 9)

| Ruta | Exactamente una vez / idempotencia | Mientras el Python también corre |
|---|---|---|
| `/tab-register` | Idempotente: si la pestaña ya está en `app-tabs.json` no se toca; `meta` se reescribe igual | Corte activo: el Python no la recibe. `app-tabs.json` bajo el mismo `flock` que cc-app |
| `/tab-metadata(-remove)` | Idempotentes (sobrescriben/quitan una clave) | Solo el frente escribe `app-tabs-meta.json` |
| `/tab-close` | Repetir cierra de nuevo: `remember_tab` reordena el historial y reescribe `app-tab-close.json`, igual que el Python | Ídem |
| `/workspace/close-group` | Exactamente una vez por `requestId`: la meta `close-group:<rid>` se guarda en app-state en la misma transacción lógica (`comandos_core::workspace::close_group`, ya portado) | El resultado guardado lo ve también el Python si se revierte el corte |
| `/recover-tab`, `/ensure`, `/new`, `/shell`, `/up` | `has-session` antes de crear: dos peticiones seguidas crean una sola sesión; dos concurrentes pueden competir como en el Python (tmux rechaza el duplicado y la segunda responde 500 con el `stderr` de tmux) | Ídem |
| `/session-new`, `/account/add` | No idempotentes en el Python (cada llamada abre `term-r<n>` nuevo); igual aquí. `/account/add` del árbol vivo serializa con `_ACCOUNT_ADD_LOCK` → `tokio::sync::Mutex` del módulo | Ídem |
| `/terminal/quick` | Idempotente por `requestId` (reclamo en app-state, 2d) | Ídem |
| `/ssh-connect`, `/ssh-new-tab` | `ssh-connect` reutiliza `ssh-<host>`; `ssh-new-tab` siempre crea | Ídem |
| `/send`, `/paste`, `/key`, `/kill`, `/focus`, `/tmux-scroll` | No idempotentes (teclean); el llamador no reintenta | Ídem |
| `/export` | Cada llamada escribe un archivo con la marca de minuto (sobrescribe en el mismo minuto) | Ídem |
| `/terminal-panes close` | El `if-shell` de tmux (2c) cierra solo si la identidad coincide; la copia se guarda antes | `close` ahora toma el `SERIAL` del frente como el resto de acciones (desaparece la diferencia de candados de la 2c) |

## Review Focus

1. **cc-app tiene el `flock` de `app-tabs.json`** durante un `/tab-close`: el frente espera (sin bloquear el runtime) y no pierde la escritura de cc-app. Prueba `tab_close_waits_for_cc_app_lock` (T2).
2. **Cierre de grupo con un miembro recreado** (misma etiqueta, otro `session_id`): no se cierra nada y el 400 es el del Python. Prueba `close_group_recreated_member_closes_nothing` (T2).
3. **`/up` de un proyecto sin sesión ni carpeta**: 400 sin crear sesión ni escribir nada. Prueba `up_without_dir_writes_nothing` (T3).
4. **`/session-new` cuando la ruta pide un agente pero el registro de proveedores es incierto**: 409 del Python, sin sesión tmux creada. Prueba `session_new_bad_route_creates_no_session` (T4).
5. **`/key` con un pane pedido que ya murió**: 404 sin teclear en el pane activo de la sesión. Prueba `key_dead_pane_never_falls_back` (T6).

## Estructura de archivos

```
crates/comandos-server/src/dash/native/tab_registry.rs   (registro: E1, close_app_tab)        T1
crates/comandos-server/src/dash/native/tabs.rs           (T2: 5 rutas)                          T2
crates/comandos-server/src/dash/native/sessions.rs       (T3: 5 rutas; T4: 2; T5: 3)           T3–T5
crates/comandos-server/src/dash/native/input.rs          (T6: 7 rutas)                          T6
crates/comandos-server/src/dash/native/terminal.rs       (T7: rama close)                       T7
crates/comandos-server/tests/support/tabs.rs             (TmuxLog, seed_tabs, systemd fakes)    T1
crates/comandos-server/tests/dash_native_tab_registry.rs, dash_native_tabs.rs,
  dash_native_sessions.rs, dash_native_input.rs, dash_native_pane_close.rs                     T1–T7
xtask/parity/2f/tabs.jsonl                                                                      T2–T7
```

`sessions.rs` lo tocan T3, T4 y T5 en serie (mismo archivo); T6 y T7 son independientes de T1–T5 y entre sí (marcados abajo).

---

### Task 1: Registro de pestañas con dueño único (`tab_registry.rs`)

Sin ruta: las funciones que usan T2–T5. Desplegable sola (nadie la llama aún).

Comportamiento portado (líneas de `0aa4ae1`):

- **`load_json_file(path, default)`** (5034): cualquier fallo o `null` → `default`. Rust: `files::read_json_strict`; `Missing`/`Invalid` → default; `Unsure` → `RegistryError::Unsure(path)` (ruling 2: 500).
- **`read_tab_metadata`** (5252): no-objeto → `{}`; por clave cadena con valor objeto y `kind ∈ TAB_KINDS = {project, scratch, shell, ssh, ssh-tab}`: `{kind}` + `host` si es cadena no vacía + `cwd` si es cadena que empieza por `/`. Orden de claves del archivo.
- **`write_tab_metadata(sess, kind, host, cwd)`** (5274): `SESSION_RE.fullmatch` y `kind` válido, si no `None`; bajo el candado de metadatos: leer, `{kind}` + `host` si no vacío (`str(host)`) + `cwd` si `os.path.isabs(cwd)` (empieza por `/`), escribir con `write_json_file` (`files::write_json_atomic`), devolver el item.
- **`remove_tab_metadata(sess)`** (5289): escribe solo si la clave estaba.
- **`tab_metadata_for_session(sess)`** (5297): guardado → ese; `find_project_dir` → `{kind:project, cwd}`; `ssh-<host>` con `ssh_config::host_entry` → `{kind:ssh, host}`; `sshtab-<host>-<n>` (`rpartition("-")`, `n.isdigit()` ASCII) con entrada → `{kind:ssh-tab, host}`; `term-*` → `{kind:scratch}`; si no `{kind:shell}`.
- **`write_app_tab(sess, label)`** (5241): bajo `flock(app-tabs.json)`: no-dict → `{}`; `tabs[sess] = label or sess`.
- **`register_app_tab(sess, label, kind, host, cwd)`** (5321): `local` → nada; `flock`: si falta, `tabs[sess] = (label or sess)[:80]` (80 **caracteres**); `meta = write_tab_metadata(...)` si `kind ∈ TAB_KINDS`, si no `tab_metadata_for_session`; si `meta` y aún no está guardado, se guarda; `app-tab-open.json` = `{"session", "label": (label or sess)[:80], "ts": time.time()}` + `"meta"` si hay (`OSError` ignorado).
- **`remember_tab(sess, label, cwd, agent, reason)`** (5223): validación; item `{session, label[:80], cwd si empieza por "/", agent[:16] (o "claude"), reason[:32], ts:int(time.time())}`; bajo `flock(app-tabs-history.json)`: `read_tab_history` (2b, `light::read_tab_history`), quitar la misma sesión, `[item] + hist[:79]`.
- **`close_app_tab(sess, ephemeral)`** (5350): `local` → `"La pestaña local permanece abierta"`; `ephemeral` sin prefijo `comandos-e2e-` → `"ephemeral requiere comandos-e2e-"`; si no efímera: `session_labels().get(sess, sess)`, `display-message -p -t =<s>: '#{pane_current_path}'` (cwd si rc 0), `remember_tab(…, state_agent(sess), "closed")`; `flock(app-tabs.json)` quitar; `remove_tab_metadata`; `workspace_sync(reason="user")` (excepción → línea en stderr y sigue); `app-tab-close.json` = `{session, ts}` (excepción ignorada). Devuelve `None`.

**Files:**
- Create: `crates/comandos-server/src/dash/native/tab_registry.rs`
- Modify: `crates/comandos-server/src/dash/native/tabs.rs` (solo `pub(crate) use super::tab_registry;` no hace falta: el módulo se declara desde `tabs.rs` con `#[path = "tab_registry.rs"] pub(crate) mod tab_registry;` para no tocar `native/mod.rs`)
- Create: `crates/comandos-server/tests/support/tabs.rs` (contenido), `crates/comandos-server/tests/dash_native_tab_registry.rs`

**Interfaces:**
- Consumes: `files::{read_json_strict, write_json_atomic, FileLock::acquire}`, `light::read_tab_history`, `target::{find_project_dir, state_agent}`, `comandos_runtime::ssh_config::host_entry`, `workspace::sync`, `Native::with_state`, el `session_labels` de la 2d (`states::gather::session_labels`, extraído a `pub(crate)` si hace falta).
- Produces: `tabs::tab_registry::{RegistryError::{Unsure(PathBuf), Io(io::Error)}, TAB_KINDS, read_tab_metadata(&Path) -> Result<Map<String,Value>, RegistryError>, write_tab_metadata(&Native, &str, &str, &str, &str) -> Result<Option<Value>, RegistryError>, remove_tab_metadata(&Native, &str) -> Result<(), RegistryError>, tab_metadata_for_session(&Native, &str) -> Result<Value, RegistryError>, write_app_tab(&Native, &str, &str) -> Result<(), RegistryError>, register_app_tab(&Native, &str, Option<&str>, &str, &str, &str) -> Result<(), RegistryError>, remember_tab(&Native, &str, Option<&str>, &str, &str, &str) -> Result<(), RegistryError>, close_app_tab(&Native, &str, bool) -> Result<Option<String>, RegistryError>}` (todas `async`); `impl From<RegistryError> for Fault` (→ 500 con la línea de stderr del ruling 2); `support::tabs::{TmuxLog, seed_registry(&TestHome)}`.

**Confinamiento:** `close_app_tab` lee tmux (`display-message`, `list-sessions`) solo por `opts.tmux = Tmux::private`; las sesiones las siembra `run_tmux`. Ninguna orden de esta tarea crea o mata sesiones.
**Efectos en vivo:** escribe `app-tabs.json`, `app-tabs-meta.json`, `app-tabs-history.json`, `app-tab-open.json`, `app-tab-close.json` y commitea el workspace: exactamente los archivos que escribe el Python en las mismas funciones, con el mismo volcado (`write_json_file`: ASCII escapado, separadores por defecto) y los mismos candados.

- [ ] **Step 1: Pruebas que fallan**

`dash_native_tab_registry.rs`, con una función de prueba por regla y un caso diferencial global:

```rust
//! Registro de pestañas: mismo resultado en disco que el Python.
mod support;

use comandos_server::dash::native::tabs::tab_registry as reg;
use support::{TestHome, run_tmux, tabs::{native_for, python_dash, seed_registry}};

#[tokio::test]
async fn register_close_and_history_match_python() {
    let a = TestHome::new("reg-a");
    let b = TestHome::new("reg-b");
    for h in [&a, &b] {
        seed_registry(h);
        run_tmux(h, &["new-session", "-d", "-s", "p2f", "-c", h.root.to_str().unwrap(), "cat"]);
    }
    let native = native_for(&a).await;
    reg::register_app_tab(&native, "p2f", Some("Proyecto ñ"), "project", "", "/tmp").await.unwrap();
    reg::write_tab_metadata(&native, "ssh-x", "ssh", "x", "").await.unwrap();
    reg::close_app_tab(&native, "p2f", false).await.unwrap();
    let Some(()) = python_dash(&b, r#"
dash.register_app_tab("p2f", "Proyecto ñ", kind="project", cwd="/tmp")
dash.write_tab_metadata("ssh-x", "ssh", host="x")
dash.close_app_tab("p2f")
"#) else { return };
    for name in ["app-tabs.json", "app-tabs-meta.json", "app-tabs-history.json", "app-tab-open.json", "app-tab-close.json"] {
        let read = |h: &TestHome| support::twin::normalize(&std::fs::read_to_string(h.hooks().join(name)).unwrap_or_default());
        assert_eq!(read(&a), read(&b), "{name}");
    }
}

#[tokio::test]
async fn ephemeral_and_local_close_nothing() {
    let home = TestHome::new("reg-eph");
    seed_registry(&home);
    let native = native_for(&home).await;
    assert_eq!(reg::close_app_tab(&native, "local", false).await.unwrap().as_deref(), Some("La pestaña local permanece abierta"));
    assert_eq!(reg::close_app_tab(&native, "p2f", true).await.unwrap().as_deref(), Some("ephemeral requiere comandos-e2e-"));
    assert!(!home.hooks().join("app-tab-close.json").exists());
}

#[tokio::test]
async fn unsure_registry_is_500_not_decline() {
    let home = TestHome::new("reg-unsure");
    seed_registry(&home);
    home.write("app-tabs-meta.json", &format!("{}{}", "[".repeat(1100), "]".repeat(1100)));
    let native = native_for(&home).await;
    assert!(matches!(reg::read_tab_metadata(&home.hooks().join("app-tabs-meta.json")),
        Err(reg::RegistryError::Unsure(_))));
}
```

`support::tabs::python_dash(home, code) -> Option<()>` y `support::tabs::native_for(home) -> Native` (un `Native` con `home.options()` sin servidor) se definen en `support/tabs.rs`: `python_dash` ejecuta `python3 -c "import sys; from importlib.machinery import SourceFileLoader; dash = SourceFileLoader('dash', '<repo>/bin/cc-dash').load_module(); <code>"` con `HOME` = el del `TestHome`, el `fakebin` del oráculo (con el `tmux` guardián y `SCOPE_RUNNER`) al frente del `PATH`, `XDG_RUNTIME_DIR` y `TMUX_TMPDIR` privados y sin `TMUX`; sin `python3`, aviso y `None`. `seed_registry` escribe `app-tabs.json` `{"local":"local","otra":"Otra"}`, `app-tabs-meta.json` con una entrada válida y una con `kind` desconocido, `app-tabs-history.json` con tres items, `~/codebase/p2f/` y `~/.ssh/config` con `Host x`.

- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_tab_registry` · Expected: FAIL de compilación (`no module tab_registry`).

- [ ] **Step 3: Implementar** con las reglas de arriba. El candado de metadatos y el patrón de escritura con `flock`:

```rust
/// `TAB_METADATA_LOCK` del Python: solo el frente escribe `app-tabs-meta.json`
/// con el corte `tabs` activo (D2 del plan maestro).
fn meta_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// `with file_lock(path): leer; mutar; write_json_file(path, …)` en un hilo de bloqueo.
async fn locked_update<F>(path: PathBuf, default: Value, mutate: F) -> Result<(), RegistryError>
where
    F: FnOnce(Value) -> Option<Value> + Send + 'static,
{
    tokio::task::spawn_blocking(move || -> Result<(), RegistryError> {
        let _guard = FileLock::acquire(&path).map_err(RegistryError::Io)?;
        let current = match read_json_strict(&path) {
            Strict::Value(v) if !v.is_null() => v,
            Strict::Unsure => return Err(RegistryError::Unsure(path)),
            _ => default,
        };
        if let Some(next) = mutate(current) {
            write_json_atomic(&path, &next).map_err(RegistryError::Io)?;
        }
        Ok(())
    })
    .await
    .map_err(|_| RegistryError::Io(std::io::Error::other("hilo de bloqueo")))?
}
```

(Si las variantes de `files::Strict` se llaman distinto en `main`, usar las reales.) `[:80]` en caracteres: `py::take_chars(text, 80)` (2c). `ts` de `app-tab-open.json` y `app-tab-close.json` con `opts.clock_seconds` (`time.time()`); `ts` del historial entero (`int`).

- [ ] **Step 4: Ver que pasa** — Run: `$C test -p comandos-server --test dash_native_tab_registry` · Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/comandos-server/src/dash/native/tab_registry.rs crates/comandos-server/src/dash/native/tabs.rs \
  crates/comandos-server/tests/support/tabs.rs crates/comandos-server/tests/dash_native_tab_registry.rs
git commit -m "feat(dash): registro de pestañas con dueño único y los mismos candados que el Python

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Rutas de registro y cierre (`tabs.rs`)

Rutas: POST `/tab-register`, `/tab-metadata`, `/tab-metadata-remove`, `/tab-close`, `/workspace/close-group`.

Comportamiento portado:

- Las cuatro primeras van tras el preámbulo de `do_POST` **sin** `resolve_project_session` (están antes de la línea 9550 en el Python, pero después de la validación `SESSION_RE.match(sess)` de 9466 → `400 {"error": "Nombre de sesion invalido"}`; `session` no cadena → 500).
- **`/tab-register`** (9494): `SESSION_RE.match` (redundante, mismo texto `session invalida` nunca alcanzable); `has-session -t =<s>` falla → `404 {"error": "'<s>' no existe"}`; `register_app_tab(sess, str(label or sess)[:80], kind=str(kind or ""), host=str(host or ""), cwd=str(cwd or ""))`; `200 {"ok": true}`. `str()` de no-cadenas: `py::str_scalar`; contenedor → `Decline` (antes de efectos: la validación va primero).
- **`/tab-metadata`** (9507): `write_tab_metadata` → `None` → `400 {"error": "metadata de pestana invalida"}`; si no `200 {"ok": true, "metadata": item}`.
- **`/tab-metadata-remove`** (9516): `200 {"ok": true}`.
- **`/tab-close`** (9520): `ephemeral = data.get("ephemeral") is True`; error → `400 {"error": err}`; si no `200 {"ok": true}`.
- **`/workspace/close-group`** (8716): `expectedRevision` entero no booleano y `members` lista, si no `400 {"error": "Solicitud de cierre inválida"}`; `workspace_sync()`; `close_group(store, groupId, expected, members, requestId, workspace_session_identity, close_app_tab)`: `Conflict` → `409 {"error": "El workspace cambió. Revisa la lista antes de cerrar", "current": workspace_payload(current)}`; `ValueError/TypeError` → `400 {"error": str(exc)}`; si hubo cierres y no es repetición → `app-tab-close.json` = `{"session": closed[-1], "sessions": closed, "ts"}`; `200 result`.
  - Diseño: todo dentro de **un** trabajo del worker de app-state (`native.with_state`), porque `close_group` necesita la conexión y llama a `close_app_tab` en medio. Las lecturas de tmux (`display-message` de identidad por miembro, `list-sessions`/`display-message` de `close_app_tab`) se hacen con `Tmux::run_blocking` desde el hilo del worker (puente de la 2c; precedente D2 de la 2e). El coste: una o dos órdenes de tmux de milisegundos por miembro mientras el worker está ocupado; un tmux colgado lo retendría hasta 5 s por orden, como al Python. Los archivos del registro dentro del trabajo usan las versiones síncronas de T1 (`tab_registry::blocking::*`, mismas funciones sin `spawn_blocking`, añadidas en este paso a `tab_registry.rs`) y el candado de metadatos con `blocking_lock()`.
  - `workspace_session_identity(sess)` (6439): `local` → `"local"`; nombre inválido → `None`; `display-message -p -t =<s>: '#{session_id}'` → valor si casa `^\$\d+$`.

**Files:**
- Modify: `crates/comandos-server/src/dash/native/tabs.rs`, `crates/comandos-server/src/dash/native/tab_registry.rs` (submódulo `blocking`)
- Create: `crates/comandos-server/tests/dash_native_tabs.rs`
- Modify: `xtask/parity/2f/tabs.jsonl`

**Interfaces:**
- Consumes: T1; `comandos_core::workspace::{close_group, CloseGroupState}`; `workspace::{store, sync, payload}` (2b, `pub(crate)`); `Tmux::run_blocking`; `target::post_target` no se usa (estas rutas no resuelven proyecto).
- Produces: `TabsRoute::{Register, Metadata, MetadataRemove, Close, CloseGroup}`, `tabs::ROUTES`.

**Confinamiento:** las pruebas siembran con `run_tmux` dos sesiones `g1`, `g2` y un workspace con un grupo de ambas en cada lado del gemelo; el cierre de pestañas **no mata sesiones** (solo el registro): se comprueba con `run_tmux(&home, &["has-session","-t","=g1"])` tras cerrar. Sin `systemd-run` en esta tarea.
**Efectos en vivo:** ninguna orden de tmux que mute; solo `display-message`/`list-sessions`, igual que el Python. Escrituras: las de T1 y la meta del workspace.

- [ ] **Step 1: Pruebas que fallan**

```rust
//! Corte tabs, T2: registro y cierre de pestañas contra el Python (gemelo).
mod support;

use support::{run_tmux, tabs::seed_registry, twin::Twin};

fn seed(h: &support::TestHome) {
    seed_registry(h);
    run_tmux(h, &["new-session", "-d", "-s", "g1", "cat"]);
    run_tmux(h, &["new-session", "-d", "-s", "g2", "cat"]);
}

#[tokio::test]
async fn tab_routes_match_python() {
    let Some(t) = Twin::start("tabs", seed).await else { return };
    for (path, body) in [
        ("/tab-register", r#"{"session":"g1","label":"Uno","kind":"project","cwd":"/tmp"}"#),
        ("/tab-register", r#"{"session":"nope","label":"X"}"#),
        ("/tab-register", r#"{"session":"bad name"}"#),
        ("/tab-metadata", r#"{"session":"g2","kind":"ssh","host":"x"}"#),
        ("/tab-metadata", r#"{"session":"g2","kind":"rara"}"#),
        ("/tab-metadata-remove", r#"{"session":"g2"}"#),
        ("/tab-close", r#"{"session":"g1"}"#),
        ("/tab-close", r#"{"session":"local"}"#),
        ("/tab-close", r#"{"session":"g2","ephemeral":true}"#),
    ] {
        let run = t.post(path, body).await;
        run.assert_same();
        t.files_equal(&["app-tabs.json", "app-tabs-meta.json", "app-tabs-history.json", "app-tab-open.json", "app-tab-close.json"]).unwrap();
    }
    assert!(t.tmux_a(&["list-sessions", "-F", "#{session_name}"]).contains("g1"), "cerrar la pestaña no mata la sesión");
}

#[tokio::test]
async fn close_group_recreated_member_closes_nothing() {
    let Some(t) = Twin::start("cg", seed).await else { return };
    // Preview en los dos lados con la revisión actual.
    let preview = |port| support::get(port, "/workspace/close-group?groupId=g");
    let (pa, pb) = (preview(t.front.port).await, preview(t.oracle.port).await);
    assert_eq!(pa.text(), pb.text());
    // g2 recreada: otro session_id con el mismo nombre (en el tmux privado de cada lado).
    for h in [&t.a, &t.b] {
        run_tmux(h, &["kill-session", "-t", "=g2"]); // socket privado -S de la prueba, sesión sembrada aquí
        run_tmux(h, &["new-session", "-d", "-s", "g2", "cat"]);
    }
    let body = support::tabs::close_group_body(&pa.text(), "req-cg-1");
    let run = t.post("/workspace/close-group", &body).await;
    run.assert_same();
    assert_eq!(run.front.status, 400);
    t.files_equal(&["app-tabs.json", "app-tab-close.json"]).unwrap();
}

#[tokio::test]
async fn tab_close_waits_for_cc_app_lock() {
    let home = support::TestHome::new("tabs-lock");
    seed(&home);
    let lock = comandos_server::dash::native::files::FileLock::acquire(&home.hooks().join("app-tabs.json")).unwrap();
    let legacy = support::FakeLegacy::start().await;
    let fr = support::front(&home, legacy.port, home.options()).await;
    let pending = tokio::spawn({
        let port = fr.port;
        async move { support::request_body(port, "POST", "/tab-close", "Content-Type: application/json\r\n", r#"{"session":"g1"}"#).await }
    });
    // Mientras espera el candado, otra ruta nativa responde (runtime libre).
    let other = support::get(fr.port, "/snippets").await;
    assert_eq!(other.status, 200);
    assert!(!pending.is_finished());
    // «cc-app» escribe y suelta.
    std::fs::write(home.hooks().join("app-tabs.json"), r#"{"g1":"Uno","cc-app":"suya"}"#).unwrap();
    drop(lock);
    let wire = pending.await.unwrap();
    assert_eq!(wire.status, 200);
    let tabs = std::fs::read_to_string(home.hooks().join("app-tabs.json")).unwrap();
    assert!(tabs.contains("cc-app") && !tabs.contains("\"g1\""));
    fr.stop().await;
}
```

El `kill-session` de la segunda prueba es la única orden destructiva del sub-plan fuera de `/kill`: va por `run_tmux` (socket `-S` del `TestHome`) sobre una sesión que la misma prueba sembró. `support::tabs::close_group_body(preview, rid)` construye `{"groupId","expectedRevision","members","requestId"}` desde la vista previa (la revisión la da `GET /workspace`).

- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_tabs` · Expected: FAIL (las rutas se reenvían al heredado falso).

- [ ] **Step 3: Implementar** `TabsRoute`, `ROUTES` (Raw) y `answer`. Esqueleto de `/workspace/close-group`:

```rust
async fn close_group(native: &Native, data: &Value) -> Answer {
    let expected = data.get("expectedRevision");
    let members = data.get("members");
    let valid = matches!(expected, Some(Value::Number(n)) if n.is_i64() || n.is_u64())
        && matches!(members, Some(Value::Array(_)));
    if !valid {
        return reply(StatusCode::BAD_REQUEST, &json!({"error": "Solicitud de cierre inválida"}));
    }
    let tmux = native.options().tmux.clone();
    let handle = tokio::runtime::Handle::current();
    let data = data.clone();
    let opts = native.options().clone();
    let outcome = native
        .with_state(move |backend| close_group_job(backend, &opts, &tmux, &handle, &data))
        .await?;
    match outcome {
        CloseOutcome::Done { result, signal } => {
            if let Some(closed) = signal {
                // `except Exception: pass` del Python: un fallo de escritura no cambia la respuesta.
                let _ = tab_registry::write_close_signal(native, &closed).await;
            }
            reply(StatusCode::OK, &result)
        }
        CloseOutcome::Conflict(current) => reply(
            StatusCode::CONFLICT,
            &json!({"error": "El workspace cambió. Revisa la lista antes de cerrar", "current": current}),
        ),
        CloseOutcome::Invalid(message) => reply(StatusCode::BAD_REQUEST, &json!({"error": message})),
    }
}
```

`close_group_job` hace `workspace::sync(backend, hooks, now)`, construye el `CloseGroupState` sobre `backend.conn` (el que ya usa la vista previa de la 2b) y llama a `comandos_core::workspace::close_group` con `session_identity` = `workspace_session_identity` por `Tmux::run_blocking` y `close_tab` = `tab_registry::blocking::close_app_tab(backend, opts, tmux, handle, session, false)`. `WorkspaceError::Conflict(state)` → `CloseOutcome::Conflict(payload(state))`; `Invalid(msg)` → `Invalid(msg)`; cualquier otro error → `Fault::Error(HandlerError::Failure)`. `signal` = `Some(closed)` si `result.closed` no está vacío y `result.replayed` no es verdadero.

- [ ] **Step 4: Fixture** — añadir a `xtask/parity/2f/tabs.jsonl` (cada línea con `setup` y `files`):

```json
{"name":"t-tab-register","method":"POST","path":"/tab-register","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"p2f","label":"Proyecto"},"setup":[["new-session","-d","-s","p2f","cat"]],"files":["app-tabs.json","app-tabs-meta.json","app-tab-open.json"],"volatile":[],"expect":"same"}
{"name":"t-tab-register-ausente","method":"POST","path":"/tab-register","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"no-existe-2f"},"volatile":[],"expect":"same"}
{"name":"t-tab-metadata","method":"POST","path":"/tab-metadata","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"p2f","kind":"shell","cwd":"/tmp"},"files":["app-tabs-meta.json"],"volatile":[],"expect":"same"}
{"name":"t-tab-metadata-mala","method":"POST","path":"/tab-metadata","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"p2f","kind":"x"},"volatile":[],"expect":"same"}
{"name":"t-tab-metadata-remove","method":"POST","path":"/tab-metadata-remove","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"p2f"},"files":["app-tabs-meta.json"],"volatile":[],"expect":"same"}
{"name":"t-tab-close","method":"POST","path":"/tab-close","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"p2f"},"files":["app-tabs.json","app-tabs-history.json","app-tab-close.json"],"volatile":[],"expect":"same"}
{"name":"t-close-group-invalida","method":"POST","path":"/workspace/close-group","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"expectedRevision":true,"members":[]},"volatile":[],"expect":"same"}
```

- [ ] **Step 5: Ver que pasa** — Run: `$C test -p comandos-server --test dash_native_tabs` · Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/comandos-server/src/dash/native/tabs.rs crates/comandos-server/src/dash/native/tab_registry.rs \
  crates/comandos-server/tests/dash_native_tabs.rs
git add -f xtask/parity/2f/tabs.jsonl
git commit -m "feat(dash): registro y cierre de pestañas nativos (tab-register, tab-metadata, tab-close, close-group)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Crear y revivir sesiones (`sessions.rs`): `/recover-tab`, `/ensure`, `/new`, `/shell`, `/up`

Comportamiento portado:

- **`agent_launch(agent)`** (5948): `AGENT_LAUNCH_<CLAVE>` de `cc-notify.conf` (`read_conf`, ya portado en la 2d `providers::read_conf`; clave = agente con `-`/`.` → `_` en mayúsculas ASCII); si no, la tabla `AGENT_LAUNCH` (copiada literal: `claude --continue 2>/dev/null || claude`, …, `acp → cc-acp`); si no, la de `claude`.
- **`agent_set()`** (5965): `AGENTS` de la conf o `DEFAULT_AGENTS` (`claude codex grok acp opencode gemini agy aider`) partido por blancos ∪ claves de `harnesses` del registro salvo `shell` (registro ilegible → solo la conf, el `except: pass`).
- **`scope_cmd(argv)`** (5420): con `opts.scope` (`Some`) = `[scope.path, --user, --scope, --collect, --quiet, argv…]`; `None` → `argv` (como el Python sin `systemd-run`; R2 del pre-flight de la 2d: en producción `scope == None` hace **declinar** las rutas que crean sesiones, antes de nada).
- **`tmux_new_session(sess, cwd, agent)`** (5441): `launch = 'set -a; . "$HOME/.claude/hooks/providers.env" 2>/dev/null; set +a; ' + agent_launch(agent)`; `scope_cmd([tmux, new-session, -d, -s, sess, -n, claude, -c, cwd, f"{launch}; exec $SHELL"])` con plazo 15 s (`TimeoutExpired` no capturado → 504); si rc 0, `new-window -d -t =<s> -n shell -c <cwd>`. El `tmux` de la cola es `opts.tmux.program` completo (ruta + prefijo), así en pruebas lleva `-S` (R1 del pre-flight).
- **`ensure_shell_window(sess, cwd)`** (5462): `list-windows -t =<s> -F '#{window_name}'`, `split()`; con `shell` → nada; `tab_metadata_for_session` `ssh`/`ssh-tab` con host válido → `new-window -d -t =<s> -n shell "ssh <shlex.quote(host)>; exec $SHELL"`; si no, `new-window -d -t =<s> -n shell -c <cwd si es dir, find_project_dir, o ~>`.
- **`select_claude_window`** (7729), **`focus_session`** (5533): la usa `/shell`/`/up` aquí y `/focus` en T6; se implementa aquí en `sessions.rs` como `pub(crate)` (T6 la consume). `focus_session`: `has-session` falla → `"No hay sesion tmux '<s>'"`; `list-clients -t =local -F '#{client_name}'` no vacío → app abierta → escribe `app-focus.json` con `json.dump` sin indentación (`{"session","win","label","ts"}`, `ensure_ascii` por omisión) **sin** escritura atómica (el Python abre con `"w"`; replicar con `std::fs::write`); si no, si la sesión no tiene clientes: `list-clients -F '#{client_activity} #{client_name}'` ordenados por actividad (entero) descendente; sin clientes → `spawn_terminal(sess)` (`pick_terminal` con `TERMINAL_CMD` de la conf y la tabla `TERMINALS` del Python, `procs::spawn_detached` con `systemd-run --user --collect --quiet` si existe y `gui_env()`); si no `switch-client -c <primero> -t =<s>`. Después `wmctrl` (plazo 2 s, `gui_env`) con el pulso keep-above: `-a`, `-r … -b add,above` y a los 0,6 s `remove,above` (tarea `tokio::spawn` con `sleep`, como el `threading.Timer`).
- Rutas (todas tras `target::post_target`):
  - **`/recover-tab`** (9531): `label = str(label or sess)[:80]`, `cwd = str(cwd or "")`, `agent = str(agent or state_agent(sess))[:16]` fuera de `agent_set()` → `claude`; `cwd` no dir → `find_project_dir(session_name(label)) or find_project_dir(sess) or ""`; vacío → `400 {"error": "No encontre cwd para recuperar '<label>'"}`; `has-session` falla → `tmux_new_session` (rc ≠ 0 → `500 {"error": stderr.strip() or "No se pudo recuperar la sesion"}`); `write_app_tab`, `write_tab_metadata(project, cwd)`, `remember_tab(…, "recovered")`; `200 {"ok", "session", "label", "cwd"}`. **Ojo**: esta rama va **antes** de `resolve_project_session` en el Python (9531 < 9550): usa `sess` sin resolver.
  - **`/ensure`** (9704), **`/new`** (9728), **`/shell`** (9749), **`/up`** (9765): literales del Python (leídos arriba en el plan maestro: `/new` usa `scope_cmd([tmux new-session -d -s <s> -c ~])` sin agente cuando no es proyecto, plazo 15 s).

**Files:**
- Modify: `crates/comandos-server/src/dash/native/sessions.rs`
- Create: `crates/comandos-server/tests/dash_native_sessions.rs`
- Modify: `xtask/parity/2f/tabs.jsonl`

**Interfaces:**
- Consumes: T1; `target::{post_target, find_project_dir, state_agent}`; `py::session_name`, `py::shlex_quote` (si no existe: añadirlo a `sessions.rs` como función privada = `shlex.quote`: vacío → `''`; solo `[A-Za-z0-9@%+=:,./_-]` → tal cual; si no `'` + reemplazar `'` por `'"'"'` + `'`); `providers::read_conf`; `procs::{spawn_detached, gui_env}`; `NativeOptions.scope` (2d).
- Produces: `SessionsRoute::{RecoverTab, Ensure, New, Shell, Up}`; `pub(crate) async fn {tmux_new_session, ensure_shell_window, focus_session, select_claude_window, agent_launch, agent_set, scope_cmd}` (las usan T4–T6).

**Confinamiento:** `opts.tmux = Tmux::private` (assert en `support::front`); `opts.scope = Some(<SCOPE_RUNNER del gemelo>)`, que ejecuta `tmux -f /dev/null -S <socket privado> new-session …` sin crear unidades; `wmctrl`, `kitty`/terminales y `systemd-run` del oráculo son enlaces a `/bin/true` del `fakebin` salvo `systemd-run` (= `SCOPE_RUNNER`) y `tmux` (= guardián); el frente resuelve `wmctrl` y los terminales con `procs::which_in(opts.search_path, …)` (sin campo nuevo en `NativeOptions`, D4) y las pruebas ponen el `fakebin` primero en `opts.search_path`. Las sesiones creadas viven solo en el servidor privado; `TestHome::drop` las mata con `-S`.
**Efectos en vivo:** crea sesiones con `systemd-run --user --scope` exactamente como `scope_cmd` (mismos argumentos, misma cola), escribe `app-focus.json`/registro igual que el Python, y `wmctrl` con los mismos argumentos. No toca sesiones distintas de `=<sesión pedida>` salvo `switch-client` del cliente más activo (lo mismo que el Python).

- [ ] **Step 1: Pruebas que fallan**

```rust
//! Corte tabs, T3: crear y revivir sesiones contra el Python (gemelo).
mod support;

use support::{tabs::seed_registry, twin::Twin};

#[tokio::test]
async fn create_and_revive_match_python() {
    let Some(t) = Twin::start("sess", |h| {
        seed_registry(h);
        std::fs::create_dir_all(h.root.join("codebase/p2f")).unwrap();
    })
    .await
    else {
        return;
    };
    for (path, body) in [
        ("/new", r#"{"session":"p2f"}"#),            // proyecto: claude + ventana shell
        ("/new", r#"{"session":"suelta"}"#),         // sin proyecto: shell en HOME
        ("/ensure", r#"{"session":"p2f","win":"shell"}"#),
        ("/shell", r#"{"session":"p2f"}"#),
        ("/up", r#"{"session":"p2f"}"#),
        ("/recover-tab", r#"{"session":"rec","label":"p2f","agent":"codex"}"#),
        ("/recover-tab", r#"{"session":"rec2","label":"nada"}"#),
    ] {
        let run = t.post(path, body).await;
        run.assert_same();
        t.files_equal(&["app-tabs.json", "app-tabs-meta.json", "app-tabs-history.json", "app-focus.json"]).unwrap();
        let shape = |s: String| s.lines().map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(
            shape(t.tmux_a(&["list-windows", "-a", "-F", "#{session_name}:#{window_name}"])),
            shape(t.tmux_b(&["list-windows", "-a", "-F", "#{session_name}:#{window_name}"])),
            "{path}"
        );
    }
    assert_eq!(support::tabs::tmux_log(&t.a), support::tabs::tmux_log(&t.b), "mismas órdenes de tmux");
}

#[tokio::test]
async fn up_without_dir_writes_nothing() {
    let Some(t) = Twin::start("up-nodir", seed_registry).await else { return };
    let run = t.post("/up", r#"{"session":"inexistente"}"#).await;
    run.assert_same();
    assert_eq!(run.front.status, 400);
    assert!(!t.tmux_a(&["list-sessions", "-F", "#{session_name}"]).contains("inexistente"));
    t.files_equal(&["app-tabs-meta.json", "app-focus.json"]).unwrap();
}

#[tokio::test]
async fn no_scope_in_production_declines_before_creating() {
    let home = support::TestHome::new("sess-noscope");
    seed_registry(&home);
    let legacy = support::FakeLegacy::start().await;
    let mut opts = home.options();
    opts.scope = None;
    let fr = support::front(&home, legacy.port, opts).await;
    let wire = support::request_body(fr.port, "POST", "/new", "Content-Type: application/json\r\n", r#"{"session":"x"}"#).await;
    assert_eq!(wire.text(), r#"{"legacy": true}"#);
    fr.stop().await;
}
```

`support::tabs::tmux_log(home)` lee `<HOME>/tmux.log` (E2) con las rutas temporales normalizadas a `<HOME>`.

- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_sessions` · Expected: FAIL.

- [ ] **Step 3: Implementar.** `tmux_new_session`:

```rust
pub(crate) async fn tmux_new_session(native: &Native, sess: &str, cwd: &str, agent: &str) -> Result<Output, Fault> {
    let opts = native.options();
    let Some(scope) = opts.scope.as_ref() else {
        // R2 del pre-flight 2d: sin systemd-run el servidor tmux nacería en el
        // cgroup del frente. Solo se llega aquí antes de cualquier efecto.
        return Err(Fault::Decline);
    };
    let launch = format!(
        "set -a; . \"$HOME/.claude/hooks/providers.env\" 2>/dev/null; set +a; {}",
        agent_launch(native, agent)?
    );
    let tail = format!("{launch}; exec $SHELL");
    let mut args: Vec<std::ffi::OsString> = ["--user", "--scope", "--collect", "--quiet"].iter().map(Into::into).collect();
    args.push(opts.tmux.program.path.clone().into_os_string());
    args.extend(opts.tmux.program.prefix.iter().cloned());
    args.extend(["new-session", "-d", "-s", sess, "-n", "claude", "-c", cwd, &tail].iter().map(Into::into));
    let mut program = scope.clone();
    program.env.extend(opts.tmux.program.env.iter().cloned());
    program.env_remove.extend(opts.tmux.program.env_remove.iter().cloned());
    let out = tmux::run_program_os(&program, &args, Duration::from_secs(15))
        .await
        .map_err(|e| match e {
            RunError::Timeout => Fault::Error(HandlerError::Timeout),
            _ => Fault::Error(HandlerError::Failure),
        })?;
    if out.ok {
        let target = format!("={sess}");
        let _ = opts.tmux.run(&["new-window", "-d", "-t", &target, "-n", "shell", "-c", cwd]).await;
    }
    Ok(out)
}
```

`run_program_os` = variante de `run_program` con `&[OsString]` (añadirla en `sessions.rs` como función privada sobre `tokio::process::Command` con las mismas reglas de decodificación si `tmux.rs` no la tiene; **no** editar `tmux.rs`). Las comprobaciones que el Python hace antes de crear (`cwd` vacío, `find_project_dir`) van antes del primer efecto y pueden declinar; `scope == None` se comprueba en cada ruta **antes** de su primera orden mutadora.

- [ ] **Step 4: Fixture** — `xtask/parity/2f/tabs.jsonl`: `t-new-proyecto` (`setup` vacío; `files: app-tabs-meta.json`), `t-new-suelta`, `t-ensure-shell` (`setup: [["new-session","-d","-s","p2f-e","cat"]]`), `t-up-sin-dir`, `t-recover-tab-sin-cwd`, con el formato de las líneas de T2. (El arnés antepone el `fakebin` con `SCOPE_RUNNER` y el guardián a ambos lados: añadir ese `systemd-run` a la lista de `xtask/src/parity.rs` **no** es de esta tarea; si el arnés aún enlaza `systemd-run` a `/bin/true`, estas líneas comparan el 500 de tmux en ambos lados igual — se dejan así y se anota en el README del arnés que la creación real se verifica con el gemelo.)

- [ ] **Step 5: Ver que pasa** — Run: `$C test -p comandos-server --test dash_native_sessions` · Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/comandos-server/src/dash/native/sessions.rs crates/comandos-server/tests/dash_native_sessions.rs
git add -f xtask/parity/2f/tabs.jsonl
git commit -m "feat(dash): crear y revivir sesiones nativo (recover-tab, ensure, new, shell, up) con scope de systemd

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: `/session-new` y `/account/add`

- [ ] **Step 0: Oráculo vivo (D8 del maestro)** — Run: `git -C ~/codebase/0xJesus/ComandOS diff -- bin/cc-dash | grep -n 'account_add_request\|_inherit_codex_trust'`. Si aparece (no confirmado en `main`), parar y pedir al controlador que confirme o descarte esos cambios; se porta lo que ejecutará el heredado. Las reglas de abajo describen el árbol vivo (`account_add_request`), que es el que corre hoy.

Comportamiento portado:

- **`/session-new`** (9206): literal. Orden de efectos que hay que respetar: (1) perfil (`session_profile_store.get_profile`/`launch_draft` → 409 `profile_unavailable`): lectura de la base de uso por `Native.usage` (2c); (2) validaciones 400/409 (`carpeta invalida`, `cwd_missing`, `route_unavailable`); (3) `resolve_route_selection(data, "new_session")` (2d `providers::selectable_routes` + la función de selección; si la 2d no la portó entera, se porta aquí en `sessions.rs` por regla y prueba diferencial contra `dash.resolve_route_selection`) → 409 con `str(e)`; (4) `validate_alias` (`comandos_runtime::accounts::validate_alias`) → 400 `account_invalid`; (5) **primer efecto posible**: `make_worktree` si `AUTO_WORKTREE != "0"`, agente y `_cwd_has_live_agent(cwd)` (2d `agent_procs`); (6) `launch_args` del perfil (puede escribir en `H/profile-launches/`) → 409; (7) `term-r<n>` + `tmux new-session -d -s <n> -c <final_cwd>` (**sin** scope, como el Python) → 500 con `stderr`; (8) `register_app_tab(kind=project)`; (9) entorno de cuenta (`account_environment`) → si falla, `kill-session -t =<nueva>` (solo la sesión recién creada) y 409; (10) comando por agente (tabla del Python, `dangerFlags` de `config/agent-roles.json` vía `load_agent_roles`); (11) `display-message` del pane, `record_runtime_config` (escritura en la base de uso por el carril), `inherit_trust_for_switch` (Claude; Codex en el árbol vivo) y el `send-keys` diferido (E3); (12) `200` con el cuerpo literal. Todo lo incierto se decide antes de (5); desde (5) no se declina.
- **`/account/add`** (árbol vivo, `account_add_request`): bajo un `tokio::sync::Mutex` de módulo (= `_ACCOUNT_ADD_LOCK`); `provider ∈ {claude, codex, grok}` si no 400; alias vacío → `cuenta-2`, `cuenta-3`… el primero cuyo `account_home` no existe; `validate_alias` y `account_home` → 400 con `str(e)`; `list_accounts` con la cuenta ya `selectable` → `409 {"error": "La cuenta <a> ya tiene sesión iniciada. Elígela para cambiar."}`; `makedirs(d, 0o700)`; Codex: `config.toml` con `cli_auth_credentials_store = "file"\n` creado con `O_EXCL` y 0600 si no existe; Claude: siembra de `settings.json` con `hooks` de `~/.claude/settings.json` y `preferredNotifChannel` (`update_json_object`, que deja intacto un archivo que no parsea); `cwd` (o `~`); `term-r<n>` con `int(time.time()*10) % 1000000`; `new-session -d -s <n> -c <cwd>` → 500; `register_app_tab(label="login <provider> · <alias>", kind=project)`; comando `env -u CLAUDECODE -u CLAUDE_CONFIG_DIR -u CODEX_HOME -u GROK_HOME -u ANTHROPIC_API_KEY -u ANTHROPIC_AUTH_TOKEN -u ANTHROPIC_BASE_URL -u OPENAI_API_KEY -u CODEX_API_KEY <env de la cuenta> <login>` con el login por proveedor del árbol vivo (`claude auth login --claudeai`; Codex con `--device-auth` si `deviceAuth`); `send-keys` diferido (E3); `200 {"ok", "session", "alias", "provider"}`. **`CLAUDE_CONFIG_DIR` por cuenta**: `account_environment` da `CLAUDE_CONFIG_DIR=<accountsRoot>/<alias>` para cuentas que no son `main` y **nada** para `main` (`main` vive en `~/.claude` sin la variable, memoria del proyecto «Cambio de cuenta en vivo»); la prueba lo fija.

**Files:**
- Modify: `crates/comandos-server/src/dash/native/sessions.rs`
- Modify: `crates/comandos-server/tests/dash_native_sessions.rs`
- Modify: `xtask/parity/2f/tabs.jsonl`

**Interfaces:**
- Consumes: T1, T3; `comandos_runtime::accounts::{validate_alias, account_home, account_environment, list_accounts, Paths}`; `comandos_runtime::providers` (2d); `Native.usage` (2c) para perfiles y `record_runtime_config` (`comandos_store::usage::record_runtime_config` si existe; si no, se porta en este paso con prueba de filas contra `cc_usage.record_runtime_config`).
- Produces: `SessionsRoute::{SessionNew, AccountAdd}`. `inherit_trust_for_switch` es `comandos_runtime::launch_command::inherit_trust_for_switch` (2f-2/T1); si esa tarea aún no está fusionada, esta la porta en ese mismo módulo y archivo con la prueba de 2f-2/T1 y quien llegue segundo conserva una sola copia en su rebase. Igual con `resolve_route_selection` (`comandos_runtime::providers`, compartida con 2f-2/T5).

**Confinamiento:** las pruebas usan `opts.tmux = Tmux::private`; `claude`, `codex`, `grok`, `git` reales nunca se lanzan: el comando del agente se **teclea** en un pane `cat` del tmux privado y la prueba lo lee con `capture-pane` por `run_tmux` tras 2 s; el registro de proveedores de la prueba apunta los binarios a `/bin/true`; `~/.claude-accounts` vive en el HOME temporal; `make_worktree` se prueba con un repositorio `git init` del HOME temporal. Sin `systemd-run` (estas rutas no usan scope).
**Efectos en vivo:** crea **una** sesión `term-r<n>` nueva por llamada (como el Python), la mata solo si es la recién creada y falla el entorno de cuenta, teclea exactamente el comando del Python 1,5 s después, crea los directorios/archivos de cuenta con los mismos modos. No toca otras sesiones ni cuentas.

- [ ] **Step 1: Pruebas que fallan** (añadir a `dash_native_sessions.rs`)

```rust
#[tokio::test]
async fn session_new_and_account_add_match_python() {
    let Some(t) = Twin::start("snew", |h| {
        support::tabs::seed_registry(h);
        support::tabs::seed_providers(h); // binarios → /bin/true, accountsRoot en el HOME temporal
        std::fs::create_dir_all(h.root.join("codebase/p2f")).unwrap();
    })
    .await
    else {
        return;
    };
    for (path, body) in [
        ("/session-new", r#"{"cwd":"~/codebase/p2f","agent":"shell"}"#),
        ("/session-new", r#"{"cwd":"relativa"}"#),
        ("/session-new", r#"{"cwd":"~/codebase/nada"}"#),
        ("/session-new", r#"{"cwd":"~/codebase/p2f","routeId":""}"#),
        ("/session-new", r#"{"cwd":"~/codebase/p2f","routeId":"claude:claude","harnessAccount":"main"}"#),
        ("/account/add", r#"{"provider":"claude","alias":"relotto"}"#),
        ("/account/add", r#"{"provider":"codex"}"#),
        ("/account/add", r#"{"provider":"otro"}"#),
    ] {
        let run = t.post(path, body).await;
        run.assert_same();
        t.files_equal(&["app-tabs.json", "app-tabs-meta.json", "app-tab-open.json",
            "~/.claude-accounts/relotto/settings.json", "~/.codex-accounts/cuenta-2/config.toml"]).unwrap();
    }
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let typed = |side: &dyn Fn(&[&str]) -> String| side(&["capture-pane", "-p", "-t", "=term-rN:"]);
    let _ = typed; // el nombre exacto sale de la respuesta: comparar por sesión creada
    let a = support::tabs::typed_commands(&t.a);
    let b = support::tabs::typed_commands(&t.b);
    assert_eq!(a, b, "mismo comando tecleado");
    assert!(a.iter().any(|c| c.contains("CLAUDE_CONFIG_DIR=") && c.contains("relotto")));
    assert!(a.iter().filter(|c| c.contains("claude --model") || c.ends_with(" claude")).all(|c| !c.contains("CLAUDE_CONFIG_DIR")),
        "main nunca lleva CLAUDE_CONFIG_DIR");
}

#[tokio::test]
async fn session_new_bad_route_creates_no_session() {
    let Some(t) = Twin::start("snew-bad", |h| {
        support::tabs::seed_registry(h);
        h.write("../../config-providers-roto", "");
        std::fs::create_dir_all(h.root.join("codebase/p2f")).unwrap();
    })
    .await
    else {
        return;
    };
    let run = t.post("/session-new", r#"{"cwd":"~/codebase/p2f","routeId":"nadie:nada"}"#).await;
    run.assert_same();
    assert_eq!(run.front.status, 409);
    assert!(!t.tmux_a(&["list-sessions", "-F", "#{session_name}"]).contains("term-r"));
}
```

`support::tabs::{seed_providers, typed_commands}` se añaden en `support/tabs.rs`: `seed_providers` copia `config/providers.json` del repositorio al checkout falso que ve el oráculo (`opts.repo_root` del frente y `REPO_ROOT` del Python apuntan al mismo repositorio real, así que en su lugar se fija `COMANDOS_PROVIDERS_FILE` si el Python lo honra; si no, la prueba usa el registro real del repositorio, que solo se **lee**); `typed_commands` hace `capture-pane -p -J -S -50` por `run_tmux` en cada sesión `term-r*` del servidor privado y devuelve las líneas no vacías normalizadas.

- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_sessions session_new` · Expected: FAIL.
- [ ] **Step 3: Implementar** según las reglas, en el orden de efectos descrito; el `send-keys` diferido:

```rust
fn type_later(native: &Native, sess: String, command: String) {
    let tmux = native.options().tmux.clone();
    native.tasks().spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let target = format!("={sess}:");
        // Igual que el hilo del Python: el resultado no se mira.
        let _ = tmux.run(&["send-keys", "-t", &target, &command, "Enter"]).await;
    });
}
```

- [ ] **Step 4: Fixture** — `t-session-new-relativa`, `t-session-new-sin-carpeta`, `t-session-new-ruta-vacia`, `t-account-add-proveedor-malo` (sin efectos: comparables en el arnés).
- [ ] **Step 5: Ver que pasa** — Run: `$C test -p comandos-server --test dash_native_sessions` · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-server/src/dash/native/sessions.rs crates/comandos-server/tests/dash_native_sessions.rs \
  crates/comandos-server/tests/support/tabs.rs
git add -f xtask/parity/2f/tabs.jsonl
git commit -m "feat(dash): /session-new y /account/add nativos con entorno por cuenta y tecleo diferido

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: `/terminal/quick` fuera de la barra, `/ssh-connect`, `/ssh-new-tab`

Comportamiento portado:

- **`/terminal/quick`** sin `place == "sidebar"`: el camino de la 2d (`quick.rs`) con `register = quick_terminal_register` (5505): `register_app_tab(sess, label, kind="scratch", cwd=cwd)` + `workspace_sync(reason="user")` (excepción → stderr). En la 2d, `quick::answer` declina si `place != "sidebar"` (D7 de la 2d). La entrada de `/terminal/quick` sigue siendo la de la 2d (dos tablas no pueden tener la misma llave); esta tarea añade en `sessions.rs` `pub(crate) async fn quick_register(native, sess, label, cwd)` y modifica **una línea** de `quick.rs` (la comprobación `place != "sidebar"` → llamada a `sessions::quick_register` dentro de la función `register` que ya recibe la librería). Excepción a D4 documentada: `quick.rs` no lo toca ningún otro grupo de la 2f.
- **`/ssh-connect`** (9121, `ssh_connect` 7744) y **`/ssh-new-tab`** (9128, `ssh_open_new_tab` 7795): literales. `ssh -o BatchMode=yes -o ConnectTimeout=6 <host> true` con plazo 14 s (`except Exception` → `keyok=False, stderr="timeout"`); `scope_cmd([tmux, new-session, -d, -s, <s>, -n, ssh, "ssh <shlex.quote(host)>; exec $SHELL"])` 15 s; `set-option -t <s> mouse off` (sin `=`: tal cual el Python); `ssh_state` (`list-panes -s -t =<s> -F '#{pane_current_command}'` → palabra `ssh`; si no, `ssh -O check <host>` 3 s); textos de nota literales. `ssh_new_tab_session` prueba `sshtab-<host>-<i>` truncado a 80 con `has-session`.
  - El programa `ssh` es `NativeOptions.ssh` si la 2d lo añadió (la 2d usa `ssh -O check` en `/state`); si no, `procs::which_in(opts.search_path, "ssh")` como en T3.

**Files:**
- Modify: `crates/comandos-server/src/dash/native/sessions.rs`, `crates/comandos-server/src/dash/native/quick.rs` (una línea)
- Modify: `crates/comandos-server/tests/dash_native_sessions.rs`, `xtask/parity/2f/tabs.jsonl`

**Interfaces:**
- Consumes: T1, T3; `comandos_runtime::ssh_config::{read, parse, host_entry, is_host}` (maestro T3 Parte C); `quick.rs` (2d).
- Produces: `SessionsRoute::{SshConnect, SshNewTab}`; `sessions::quick_register`.

**Confinamiento:** `ssh` en pruebas es un ejecutable generado por la prueba en el `fakebin` (`#!/bin/sh` que sale con 0 si `$*` contiene `true` y el host es `bueno`, con 255 y `Permission denied` en stderr si es `malo`, y con 255 para `-O check`); el frente lo recibe por `opts.search_path` con el `fakebin` primero. Las sesiones `ssh-*`/`sshtab-*` se crean en el tmux privado vía `SCOPE_RUNNER` y su `ssh` es ese falso. Nunca se conecta a ningún host.
**Efectos en vivo:** las mismas órdenes que el Python: una prueba `ssh BatchMode` al host guardado (mismo plazo), una sesión nueva con `scope_cmd`, `set-option mouse off`, el registro; `ssh -O check` solo consulta el socket de control.

- [ ] **Step 1: Pruebas que fallan** — `ssh_routes_match_python` con el gemelo: `~/.ssh/config` con `Host bueno` y `Host malo`; casos `/ssh-connect {"host":"bueno"}` (crea), repetido (sesión existe → `ssh_state`), `{"host":"malo"}`, `{"host":"desconocido"}`, `/ssh-new-tab {"host":"bueno"}` ×2 (`sshtab-bueno-1`, `-2`); comparar cuerpos, `app-tabs*.json`, `list-sessions` y `tmux.log`. `quick_terminal_outside_sidebar_registers_tab`: `POST /terminal/quick {"requestId":"rq-2f-0001"}` en el gemelo crea la shell, registra la pestaña `scratch` en ambos lados y el workspace queda con la misma pestaña (comparar `GET /workspace` sin `revision`).
- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_sessions ssh` · Expected: FAIL.
- [ ] **Step 3: Implementar** según las reglas.
- [ ] **Step 4: Fixture** — `t-ssh-connect-desconocido`, `t-ssh-new-tab-desconocido` (sin efectos).
- [ ] **Step 5: Ver que pasa** — Run: `$C test -p comandos-server --test dash_native_sessions` · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-server/src/dash/native/sessions.rs crates/comandos-server/src/dash/native/quick.rs \
  crates/comandos-server/tests/dash_native_sessions.rs
git add -f xtask/parity/2f/tabs.jsonl
git commit -m "feat(dash): terminal rápida fuera de la barra y pestañas SSH nativas

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Teclas, foco, desplazamiento y exportación (`input.rs`) — independiente de T1–T5

Requiere solo las Tareas 0–3 del maestro y `focus_session`/`select_claude_window`. Para no depender de T3, si T3 aún no está fusionada cuando empieza T6, T6 escribe `focus_session` y `select_claude_window` en `input.rs` y T3 las importa de ahí (quien llegue segundo borra su copia en su rebase). Ambas tareas lo comprueban en su Step 1.

Comportamiento portado (tras `target::post_target`):

- **`/send`** (9625): `str(text)[:4000]`; vacío tras `strip()` → `400 {"error": "Texto vacio"}`; `has-session` → `404 {"error": "No hay sesion tmux '<s>'. Levantala primero."}`; `send-keys -t <pane> -l -- <text>` (rc ≠ 0 → `500 {"error": stderr.strip() or "tmux fallo"}`); `send-keys -t <pane> Enter`; `200 {"ok": true}`.
- **`/paste`** (9637): `str(text)` sin recortar; vacío → 400; `> 20000` caracteres → `400 {"error": "body demasiado largo (max 20000)"}`; `has-session` → 404; `snippet_paste_to_pane` (5724): `load-buffer -b comandos-snip-<12 hex> -` con el texto por stdin (`procs::run_program_input`, 5 s), `paste-buffer -p -d -b <buf> -t <pane>`, `delete-buffer` si no pegó; errores → `500 {"error": …}`.
- **`/key`** (9650): `key ∈ ALLOWED_KEYS` (`Enter Escape Up Down Tab y n 1…9`) si no `400 {"error": "Tecla no permitida"}`; `pane` pedido inválido → `400 {"error": "Pane invalido"}`; pedido y distinto del resuelto → `404 {"error": "El pane <p> ya no existe"}` (**nunca** cae al pane activo); `has-session` → `404 {"error": "No hay sesion tmux '<s>'"}`; `send-keys -t <pane> <key>` → 500 o 200.
- **`/focus`** (9696): si la sesión existe, `select_claude_window`; `focus_session` → error → `404 {"error": err}`.
- **`/kill`** (9684): `sess ∈ HIDDEN_SESSIONS = {hub, local, control}` → `400 {"error": "Esa sesion es interna de la app"}`; `has-session` → 404; `kill-session -t =<s>` → 500 o 200.
- **`/tmux-scroll`** (9487, `tmux_scroll` 5850 con `tmux_pane_at` 5807): literal (rueda SGR/UTF-8/estándar con `send-keys -l`/`-H`, `copy-mode -e`, `send-keys -X -N`); error que empieza por `No hay sesion` → 404, otro → 400; esta ruta **no** usa `resolve_project_session` (va antes de 9550 en el Python): solo la validación de sesión.
- **`/export`** (9669): `format ∈ {txt, pdf}` → 400; `read_states()` = el `/state` de la 2d (`Native::states_cached` no: el Python llama a `read_states()` **sin** caché; se usa el cómputo directo de la 2d, `states::compute`, sin escribir `app-tab-models.json`; si la 2d solo expone el camino con caché, usarlo y anotar la diferencia: el resultado es el mismo con datos de ≤ 1,2 s); primer item con esa sesión y `detail or last` → si no `404 {"error": "Esa sesion no tiene respuesta guardada"}`; `export_response` (7904): `txt` escribe `export_dir()/claude-<nombre>-<YYYY-mm-dd_HHMM>.txt`; `pdf` con Chrome headless (plazo 40 s; sin Chrome → `500 {"error": "Para PDF instala Chrome o Chromium"}`); `_open_url(path, gui_env())` (`procs::spawn_detached` de `xdg-open`, o `wslview`/`open` con la misma regla); `200 {"ok": true, "path": p}`. `md_to_html` se porta literal (expresiones de `_md_inline`) con prueba diferencial.

**Files:**
- Modify: `crates/comandos-server/src/dash/native/input.rs`
- Create: `crates/comandos-server/tests/dash_native_input.rs`
- Modify: `xtask/parity/2f/tabs.jsonl`

**Interfaces:**
- Consumes: maestro T1 (`procs`), T3 (`target::post_target`); 2d `states`.
- Produces: `InputRoute::{Send, Paste, Key, Focus, Kill, TmuxScroll, Export}`.

**Confinamiento:** cada prueba siembra en su tmux privado `s1` con dos panes `cat` (y `hub` para el 400); `/kill` se prueba **solo** sobre `s1` sembrada por la prueba y sobre `hub` (que no se mata: 400); lo tecleado se verifica con `capture-pane` por `run_tmux`; `wmctrl`, `xdg-open`, Chrome: ejecutables del `fakebin` (`/bin/true`, o un script que anota sus argumentos). Ninguna orden sin `-S`.
**Efectos en vivo:** `send-keys`, `paste-buffer`, `kill-session`, `copy-mode`, `switch-client`, `select-window` sobre `=<sesión pedida>` o `%<pane>` resuelto exactamente igual que el Python (mismo preámbulo); `kill-session` nunca sobre `hub`/`local`/`control`.

- [ ] **Step 1: Pruebas que fallan**

```rust
//! Corte tabs, T6: teclas y foco contra el Python (gemelo).
mod support;

use support::{run_tmux, twin::Twin};

fn seed(h: &support::TestHome) {
    support::tabs::seed_registry(h);
    run_tmux(h, &["new-session", "-d", "-s", "s1", "-x", "100", "-y", "30", "cat"]);
    run_tmux(h, &["split-window", "-t", "=s1:", "cat"]);
    run_tmux(h, &["new-session", "-d", "-s", "hub", "cat"]);
}

#[tokio::test]
async fn input_routes_match_python() {
    let Some(t) = Twin::start("input", seed).await else { return };
    for (path, body) in [
        ("/send", r#"{"session":"s1","text":"hola ñ"}"#),
        ("/send", r#"{"session":"s1","text":"   "}"#),
        ("/send", r#"{"session":"nadie","text":"x"}"#),
        ("/paste", r#"{"session":"s1","text":"linea1\nlinea2"}"#),
        ("/key", r#"{"session":"s1","key":"Enter"}"#),
        ("/key", r#"{"session":"s1","key":"F1"}"#),
        ("/key", r#"{"session":"s1","key":"y","pane":"%999"}"#),
        ("/focus", r#"{"session":"s1"}"#),
        ("/tmux-scroll", r#"{"session":"s1","delta":-5}"#),
        ("/tmux-scroll", r#"{"session":"s1","delta":0}"#),
        ("/kill", r#"{"session":"hub"}"#),
        ("/kill", r#"{"session":"s1"}"#),
        ("/export", r#"{"session":"s1","format":"doc"}"#),
    ] {
        t.post(path, body).await.assert_same();
    }
    assert_eq!(support::tabs::tmux_log(&t.a), support::tabs::tmux_log(&t.b));
    assert!(t.tmux_a(&["list-sessions", "-F", "#{session_name}"]).contains("hub"));
}

#[tokio::test]
async fn key_dead_pane_never_falls_back() {
    let Some(t) = Twin::start("key-dead", seed).await else { return };
    let before = t.tmux_a(&["capture-pane", "-p", "-t", "=s1:"]);
    let run = t.post("/key", r#"{"session":"s1","key":"y","pane":"%77"}"#).await;
    run.assert_same();
    assert_eq!(run.front.status, 404);
    assert_eq!(t.tmux_a(&["capture-pane", "-p", "-t", "=s1:"]), before);
}
```

- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_input` · Expected: FAIL.
- [ ] **Step 3: Implementar** según las reglas.
- [ ] **Step 4: Fixture** — `t-send-vacio`, `t-key-no-permitida`, `t-kill-interna`, `t-export-formato` (sin efectos).
- [ ] **Step 5: Ver que pasa** — Run: `$C test -p comandos-server --test dash_native_input` · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-server/src/dash/native/input.rs crates/comandos-server/tests/dash_native_input.rs
git add -f xtask/parity/2f/tabs.jsonl
git commit -m "feat(dash): send, paste, key, focus, kill, tmux-scroll y export nativos

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: POST `/terminal-panes` con `action:"close"` — independiente de T1–T6

Completa la rama que la 2c declina (`terminal.rs`). `terminal_panes::execute` (2c) recibe `save_snapshot`; hoy el nativo declina antes de llamar. Ahora `save_snapshot` = `comandos_runtime::closed_panes::save(home, tmux_cb, &mut PaneInspector::new(…), sess, pane, now_ns, uuid_hex, now_s)` (maestro T3), dentro del mismo `spawn_blocking` con `Tmux::run_blocking`, bajo el `SERIAL` de la 2c. Errores: `ValueError` de la copia → `400 {"error": str(exc)}` (texto del Python); cualquier otro → el 503 de la 2c. `uuid4().hex` = 16 bytes de `getrandom` en hexadecimal minúsculo (32); `time.time_ns()` del reloj real.

**Files:**
- Modify: `crates/comandos-server/src/dash/native/terminal.rs`
- Create: `crates/comandos-server/tests/dash_native_pane_close.rs`
- Modify: `xtask/parity/2f/tabs.jsonl`

**Interfaces:**
- Consumes: maestro T3 (`closed_panes::save`, `tmux_snapshot`), 2d `PaneInspector`.
- Produces: la rama `close` de `TerminalRoute::Panes` (ninguna variante nueva; la ruta ya es nativa y su `cut()` es `Base` — **excepción**: para que `COMANDOS_DASH_CUTS_OFF=tabs` también revierta el `close`, `terminal.rs` comprueba `native.options().cuts_off.contains(&Cut::Tabs)` al entrar en la rama `close` y declina en ese caso antes de cualquier efecto).

**Confinamiento:** tmux privado con `s1` de tres panes `cat` sembrada por la prueba; el cierre mata **solo** un pane de esa sesión vía el `if-shell` de la 2c; la copia va a `~/.local/state/comandos/closed-panes` **del HOME temporal** (el frente la calcula desde `opts.home`/`hooks.parent().parent()`, nunca desde `$HOME` del proceso de pruebas).
**Efectos en vivo:** guarda la copia en la misma carpeta y con el mismo formato que el Python y después cierra el pane con la orden de la 2c (comprueba identidad y que no sea el último). Ninguna otra orden.

- [ ] **Step 1: Pruebas que fallan** — `pane_close_matches_python`: gemelo con `s1` de tres panes; `POST /terminal-panes {"session":"s1","action":"close","pane":"%1","identity":<identidad de list>}` en ambos lados (la identidad la da un `action:"list"` previo de cada lado); comparar cuerpos normalizados, que el pane desapareció en ambos y que cada lado escribió un archivo en su `closed-panes` con el mismo contenido tras normalizar (`closedAt`, nombre). `pane_close_identity_mismatch_writes_nothing`: identidad falsa → mismo 400/503 que el Python y ningún archivo nuevo. `pane_close_respects_cuts_off`: con `cuts_off = {Tabs}` se reenvía al heredado falso.
- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_pane_close` · Expected: FAIL (`close` se reenvía).
- [ ] **Step 3: Implementar** quitando el `Decline` de `close` de la 2c y pasando el `save_snapshot` real.
- [ ] **Step 4: Fixture** — cambiar `g-terminal-panes-close` de `frente.jsonl` **no** (D4): añadir en `xtask/parity/2f/tabs.jsonl` `t-terminal-panes-close-identidad` con `setup: [["new-session","-d","-s","pc2f","cat"],["split-window","-t","=pc2f:","cat"]]` y una identidad falsa (respuesta igual, sin efectos), y anotar en la descripción del commit que `g-terminal-panes-close` de la 2c deja de reenviarse (su campo `forwarded` lo corrige la tarea final del maestro).
- [ ] **Step 5: Ver que pasa** — Run: `$C test -p comandos-server --test dash_native_pane_close` · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-server/src/dash/native/terminal.rs crates/comandos-server/tests/dash_native_pane_close.rs
git add -f xtask/parity/2f/tabs.jsonl
git commit -m "feat(dash): cierre de pane nativo con copia de recuperación (terminal-panes close)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Self-review

- **Cobertura**: las 23 entradas del corte `tabs` del inventario del maestro están en el mapa (T2–T7).
- **Confinamiento y efectos en vivo**: cada tarea mutadora (T1–T7) tiene los dos apartados; las únicas órdenes destructivas de prueba (`kill-session` en T2 y T6, cierre de pane en T7) van por `-S` sobre sesiones sembradas por la propia prueba.
- **Nombres**: `tab_registry::*` (T1) se usan con esas firmas en T2–T5; `sessions::{tmux_new_session, focus_session, select_claude_window, agent_launch, agent_set, scope_cmd, quick_register}` (T3/T5) en T4–T6.
- **Independencia**: T1→T2 y T1→T3→T4→T5 en serie; T6 y T7 en paralelo con todo lo demás del grupo.
