# Fase 2f-2 — Operaciones de sesión (corte `ops`): plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** que el frente `comandos dash` ejecute él mismo los cambios de cuenta, modelo, motor y extensiones de un pane vivo (`/session/configure`, `/account/switch`, `/model/switch`, `/pane-extensions*`), responda `/model/status` en todos sus casos, gestione los perfiles de sesión y las 6 rutas de la barra de comandos, con el journal `session-operations.sqlite3` y `motor-results.json` con un solo dueño mientras el corte está activo; respuestas byte a byte iguales a las del `cc-dash` Python.

**Architecture:** el adaptador `SessionConfiguration` del Python se porta a `comandos-runtime` como código síncrono (`session_configuration.rs`) que implementa el trait `Adapter` ya portado (`session_operations::run_operation`). Cada operación corre en un hilo de sistema dedicado (`std::thread`, como el `threading.Thread(daemon=True)` del Python) con su propia conexión al journal y tmux por `Tmux::run_blocking`; el runtime de tokio nunca espera esas operaciones. Las piezas puras (comando de lanzamiento, envolturas de extensiones, confianza de carpeta, diálogos) van en módulos propios con pruebas diferenciales contra las funciones Python. En el servidor, `native/ops/` tiene el dueño de `motor-results.json` (`MotorResults`), la entrada `session_configure`/`session_recover`/`refresh_session_confirmation` y las rutas.

**Tech Stack:** Rust 1.96, tokio 1.53, rusqlite 0.40 (journal en WAL, conexiones por llamada como `OperationStore.connect()`), serde_json (`preserve_order`, `arbitrary_precision`), `regex =1.13.1`, `getrandom` para `secrets.token_hex`/`uuid4`. Sin crates nuevos.

**Spec:** `docs/superpowers/specs/2026-10-04-comandos-rust-complete-design.md` (§4.2). Plan maestro: `docs/superpowers/plans/2026-10-04-fase-2f-resto-del-tablero.md` (D2, D5, D6, D8, D12; Tareas 0–3). Oráculo: `bin/cc-dash` (líneas de `0aa4ae1`), `lib/session_operations.py`, `lib/extension_launch.py`, `lib/pane_extensions.py`, `lib/session_profiles.py`, `lib/claude_trust.py`, `lib/tmux_snapshot.py`, `lib/pane_snapshot.py`.

**Precondición:** Tareas 0–3 del maestro en `migration/rust-fase2f`. Grupo **G2**; rama `migration/rust-fase2f-ops`. Memoria del proyecto «Cambio de cuenta en vivo» leída (causa raíz `model_unavailable`, `main` sin `CLAUDE_CONFIG_DIR`, `session-operations.sqlite3`).

---

## Rulings que aplican

1. Respuestas idénticas byte a byte; textos de error literales (incluidos los de `ValueError`/`RuntimeError` de cada paso del adaptador, que el cliente muestra).
2. `Decline` solo antes de efectos. En este corte el primer efecto de `session_configure` es `store.claim` (escritura en el journal): todo lo incierto (registro de proveedores `Unsure`, identidad de pane ilegible, journal ilegible) se decide antes; después, un fallo es el `409`/`500` del Python o un `stage('failed')` del journal, nunca `Decline`.
3. Journal por conexiones propias (`open_journal`), como el Python (`OperationStore.connect()` abre una por llamada): el carril `Lane<JournalBackend>` de la 2c sigue para las lecturas de `/model/status`; las operaciones largas usan su conexión en su hilo. Base de uso (perfiles, `record_runtime_config`) por `Lane<UsageBackend>`.
4. Nada bloqueante en el runtime: las operaciones (hasta 45 min de espera a que el agente quede libre) viven en hilos de sistema; cualquier E/S de archivos de una ruta, en `spawn_blocking`.
5. Procesos con los plazos del Python: `systemctl --user` 15 s (`/proxy`), tmux 5 s, `capture-pane` 5 s.
6. Paridad: `xtask/parity/2f/ops.jsonl` + pruebas contra el oráculo por ruta (gemelo).

## Global Constraints

- `$C` = `CARGO_TARGET_DIR=/home/someguy/codebase/0xJesus/ComandOS/.build/target nice -n 10 cargo <cmd> -j 6`. Antes de cada commit: `$C fmt --all -- --check`, `$C clippy --workspace --all-targets -j 6 -- -D warnings`, pruebas de `comandos-runtime` y `comandos-server`.
- `git add <rutas>` explícitas; `git add -f xtask/parity/2f/ops.jsonl`; mensajes en español con `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Comentarios en español, identificadores en inglés; sin `unsafe`/`unwrap`/`expect`/indexado fuera de pruebas; cero Python o bash nuevos. Solo los archivos que nombra cada tarea (D4 del maestro).
- **Regla tmux (vinculante, `CLAUDE.md`; el 4 de octubre a las 21:59 un subagente mató las ~20 sesiones vivas del usuario con `TMUX_TMPDIR=<dir borrado> tmux kill-server`):**
  - Todo tmux de prueba con socket explícito `-S <dir>/tmux-<uid>/default` (o `-L <etiqueta propia>`): `Tmux::private(dir)` en el frente, `TestHome::tmux_command()`/`support::run_tmux` en las pruebas del servidor, `PrivateTmux` (`-S`) en las de `comandos-runtime`, `private_tmux(dir)` en `xtask`. **Nunca `TMUX_TMPDIR` solo.**
  - El Python del oráculo y del gemelo corre solo con el `fakebin` que contiene el `tmux` guardián (`support::oracle::tmux_guard`) y el `systemd-run` falso (`support::twin::SCOPE_RUNNER`).
  - Limpieza: `kill-server` con el `-S` propio y después borrar el directorio.
  - Prohibido `kill-server`, `kill-session`, `pkill tmux` o `kill -9 -1` sin `-S`/`-L` propio; prohibido ejecutar tmux a mano.
  - **Señales a procesos:** el adaptador manda `SIGTERM` a un pid como último recurso (`_pane_exit_current`). En pruebas el pid es siempre el del agente falso que la prueba lanzó en su tmux privado; la prueba `exit_never_signals_foreign_pid` comprueba que un pid cuyo inicio (`/proc/<pid>/stat` campo 22) no coincide con el anotado nunca recibe señal.
  - Cada tarea mutadora tiene sus apartados **Confinamiento** y **Efectos en vivo**.

## Decisiones de este sub-plan

- **O1 — Adaptador síncrono en un hilo propio.** `Adapter` (`comandos_runtime::session_operations`) es síncrono y `run_operation` duerme entre intentos. Cada operación aceptada arranca `std::thread::Builder::new().name("comandos-op".into()).spawn(...)`; el hilo recibe un `TmuxSync` (closure `Fn(&[&str]) -> Output` que llama a `Tmux::run_blocking` con el `Handle` del runtime) y abre su conexión al journal. El `TaskTracker` (D12) no lo espera: el Python tampoco espera sus hilos al salir; una operación cortada por el apagado queda en el journal y `recover_abandoned` la marca por pid, como hoy.
- **O2 — Dueño del journal por fila, no por proceso.** El journal ya es exactamente-una-vez entre procesos (`claim` por `requestId`, `claim_recovery`, `cancel_waiting` son transacciones). Mientras ambos procesos corren, cada operación pertenece a quien la reclamó (pid en la fila); `recover_abandoned(alive)` solo marca abandonadas las filas cuyo pid no existe. El frente usa su propio pid.
- **O3 — `motor-results.json` con un dueño.** Con el corte `ops` activo, solo el frente llama a `motor_result_set`/`motor_stage` (todas sus llamadas están en este corte; `/model/status` y `/proxy` leen el mapa). `MotorResults` replica el `dict` del Python: carga perezosa del archivo al primer uso (`_motor_result_load`: no-objeto o error → `{}`), mutación bajo `std::sync::Mutex`, recorte (>300 → se quitan los más viejos por `float(ts or 0)` dejando 200, orden estable de `sorted`) y reescritura completa con `write_json_file` (`except Exception: pass`). El Python heredado conserva su copia en memoria: si se reinicia con el corte activo, su arranque (`motor_queue_resume`) relee el archivo, así que no pierde lo que escribió el frente; sin el corte, el frente nunca escribe el archivo.
- **O4 — `motor_queue_resume` al arrancar solo con `Background::Front`** (D6 del maestro): recuperar abandonadas es idempotente, pero reescribir `motor-results.json` desde dos procesos no.
- **O5 — El lanzador de extensiones sigue siendo `lib/extension_launch.py`.** `wrap_command`/`wrap_environment` devuelven un comando que ejecuta `HELPER` (`lib/extension_launch.py` como programa) dentro del pane del usuario; para que el comando sea el mismo byte a byte, el frente construye la misma cadena con la misma ruta (`<repo>/lib/extension_launch.py`). No es Python nuevo ni lo ejecuta el frente; su paso a Rust es de una fase posterior (anotado en la lista de la 2g).

## Mapa de rutas

| Ruta | Llave | Python | Tarea |
|---|---|---|---|
| POST `/session/configure`, `/account/switch`, `/model/switch` | Raw | 9566, 9586 (`session_configure` 3336) | T3 |
| GET `/model/status` (casos sin fila y `awaiting_confirmation`) | ExactOrQuery (2c) | 8325 (`session_operation_status` 6217, `refresh_session_confirmation` 3300) | T3 |
| GET `/pane-extensions` | Prefix | 8258 (`pane_extensions_state` 2202) | T4 |
| POST `/pane-extensions`, `/pane-extensions/apply`, `/template`, `/cancel`, `/recover` | Raw | 8659 (`pane_extensions_write` 2245) | T4 |
| GET `/session-profiles` (prefijo), POST `/session-profiles`, `/session-profile-apply` | Prefix / Raw | 8348, 8829, 8840 | T5 |
| GET y POST `/proxy`, POST `/harness/switch`, `/model/switch-cancel`, `/session/recover`, GET `/session-config-history` | — | D8 del maestro | T6 |

## Idempotencia y orden frente al Python (ruling 9)

| Ruta | Exactamente una vez | Mientras el Python también corre |
|---|---|---|
| `/session/configure`, `/account/switch`, `/model/switch` | `requestId` (o 16 bytes aleatorios en hex): una petición repetida con el mismo id no relanza nada; devuelve el estado guardado (`store.get` antes de resolver identidad, como el Python). Dos ids distintos sobre el mismo pane: el segundo `claim` falla y responde `202 pending` sin hilo | El journal es compartido y transaccional; una operación del Python en curso bloquea la del frente en el mismo pane y viceversa (`claim` mira las filas activas por `pane_key`) |
| `/pane-extensions` (guardar borrador) | Revisión optimista (`require_revision`, `save(key, revision, …)`): una segunda escritura con la misma revisión es `409` | Mismo almacén SQLite (`pane_extensions.py` sobre el journal) |
| `/pane-extensions/apply` | `requestId` repetido con el mismo cuerpo → misma operación; con otro cuerpo → `409 requestId ya se usó con otra configuración` | Ídem |
| `/cancel`, `/recover` | `cancel_waiting` y `claim_recovery` son transacciones de una vez | Ídem |
| `/session-profiles` | Guardado por `id` (reemplazo); borrar dos veces responde `200` dos veces (`DELETE` sin filas) | Base de uso compartida, por el carril |
| `/session-profile-apply` | Sin efectos salvo `launch_args(dry_run=True)` (no escribe) | — |
| `/proxy` POST | Reaplicar el mismo estado deja el mismo `settings.json` | `systemctl` y `settings.json` son externos; mismas órdenes |

## Review Focus

1. **Cambio de cuenta de `main` a `relotto` y vuelta**: el comando tecleado lleva `CLAUDE_CONFIG_DIR=<…>/relotto` en la ida y **ninguna** asignación de `CLAUDE_CONFIG_DIR` en la vuelta (`env -u CLAUDE_CONFIG_DIR …` sí). Prueba `account_switch_main_has_no_config_dir` (T1).
2. **El modelo observado no está en el registro** (CLI más nuevo que `providers.json`): el cambio solo de cuenta no falla (`_prepare_account_only` no valida el modelo; `_launch_model_id` antepone `claude-`). Prueba `account_only_unknown_model_still_switches` (T2).
3. **El agente original no sale**: tras `/exit`, `C-c` y `SIGTERM` el pid sigue vivo → `apply` lanza `el agente original no cerró`, el journal queda `recovery_required` o `failed` como en el Python y no se teclea el comando destino. Prueba `stuck_origin_types_nothing` (T2).
4. **Dos `/session/configure` concurrentes con distinto `requestId` sobre el mismo pane**: uno se ejecuta, el otro `202 pending` sin hilo. Prueba `concurrent_configure_runs_once` (T3).
5. **`motor-results.json` con 301 entradas**: tras una escritura quedan 200, las más nuevas, mismo archivo que el Python. Prueba `motor_results_trim_matches_python` (T2).

## Estructura de archivos

```
crates/comandos-runtime/src/launch_command.rs        (configuration_command, launch_model_id, same_model)   T1
crates/comandos-runtime/src/extension_launch.rs      (subconjunto de lib/extension_launch.py)              T1
crates/comandos-runtime/src/claude_trust.rs          (lib/claude_trust.py)                                 T1
crates/comandos-runtime/src/dialogs.rs               (dialog_patterns, screen_dialog, verify_attempts)     T1
crates/comandos-runtime/src/session_configuration.rs (SessionConfiguration, PaneExtensionConfiguration)    T2
crates/comandos-runtime/src/pane_exit.rs             (_pane_exit_current, _restore_shell_tty, handoff)     T2
crates/comandos-runtime/src/pane_extensions.rs       (lib/pane_extensions.py: ExtensionStore)              T4
crates/comandos-runtime/src/extension_observations.rs (conversation_usage)                                 T4
crates/comandos-store/src/session_profiles.rs        (lib/session_profiles.py: perfiles e inventario)      T5
crates/comandos-server/src/dash/native/ops/mod.rs    (OpsRoute, ROUTES, answer)                            T3–T6
crates/comandos-server/src/dash/native/ops/results.rs  (MotorResults)                                      T2
crates/comandos-server/src/dash/native/ops/configure.rs (session_configure, session_recover, refresh)     T2
crates/comandos-server/src/dash/native/ops/extensions.rs, profiles.rs, bar.rs                              T4–T6
crates/comandos-runtime/tests/{launch_command,claude_trust,session_configuration}_oracle.rs                T1–T2
crates/comandos-server/tests/support/ops.rs          (FakeCodex compilado, registro de prueba)             T2
crates/comandos-server/tests/dash_native_ops*.rs                                                           T2–T6
xtask/parity/2f/ops.jsonl                                                                                  T3–T6
```

---

### Task 1: Piezas puras del cambio de configuración

Sin rutas. Cada función con su prueba diferencial contra la función Python cargada con `SourceFileLoader` (`bin/cc-dash`) o importada de `lib/` en el entorno del oráculo.

Comportamiento portado:

- **`configuration_command(harness, motor, model, effort, account, resume, flags, preserve_model_flags)`** (`_configuration_command` 2308): binario por `_harness_bin` (tabla del registro: `harnesses.<h>.bin` o el nombre) y `providers::which` (si no → `ValueError("no encuentro el ejecutable <bin>")`); entorno de cuenta (`accounts::account_environment` si `capabilities.accounts`); Claude con motor ajeno: `proxy_alive()` (conexión TCP a `127.0.0.1:<port>` con plazo 1 s, como el Python) o `ValueError("el gateway local no responde")`, `ANTHROPIC_BASE_URL` y `motor_lock_env`; los 9 `-u`; asignaciones en orden de inserción; resume/model/effort por harness; filtro de `flags` literal; `shlex.join` (`py::shlex_join` = `shlex_quote` por palabra unidas con espacio).
  - **`main` nunca lleva `CLAUDE_CONFIG_DIR`**: lo garantiza `account_environment` (`main` → sin variable); la prueba lo fija.
- **`launch_model_id`** (2715) y **`same_model`** (2730): literales; `model_spec` y `model_key` de `providers` (si la 2d no los expone, se añaden en `providers.rs` con prueba).
- **`extension_launch`** (subconjunto usado por el adaptador y T4): `wrap_command`, `wrap_environment`, `launch_from_pid`, `verify_launch`, `configuration_status`, `inventory`, `_internal_inventory`, `_normalize`, `capture_opencode_environment`, `prepare_launch` (lo usa `_preserve_extension_plan`). `HELPER` = `<repo_root>/lib/extension_launch.py` (O5). `uuid4().hex` y `hashlib.sha256` con `getrandom`/`sha2` (si `sha2` no está en el workspace, se usa la implementación que ya trae `comandos-core` para `/webterm-token`; si tampoco, se añade `sha2` al `Cargo.toml` de `comandos-runtime` — crate pequeño, sin dependencias nativas).
- **`claude_trust`** (`lib/claude_trust.py` entero, 143 líneas): `ensure_cwd_trusted`, `inherit_cwd_trust`, `cwd_trusted_in`; escritura del `.claude.json` con su `flock` y su volcado (`json.dump(indent=2)` del módulo: comprobar el volcado exacto en `_stamp_locked` y replicarlo).
- **`inherit_trust_for_switch(cwd, from_alias, to_alias, harness)`** (2398, árbol vivo: también Codex — **Step 0** de D8).
- **`dialog_patterns`**, **`screen_dialog`**, **`verify_attempts`** (2461–2548): patrones de `config/detectors.json` con caché por `mtime` (`_DIALOG_CACHE`), compilados con `providers::py_regex` (si un patrón no es traducible → `Unsure` → el patrón por omisión equivalente del Python no existe: se trata como **ningún diálogo detectado** solo si el Python, con ese patrón, tampoco lo detectaría; si no es decidible, `verify_attempts`/`screen_dialog` devuelven `Err(Unsure)` y la tarea que los llama declina antes del `claim`).

**Files:**
- Create: `crates/comandos-runtime/src/{launch_command,extension_launch,claude_trust,dialogs}.rs`; Modify: `crates/comandos-runtime/src/lib.rs`, `crates/comandos-runtime/src/providers.rs` (solo si faltan `model_spec`/`model_key`/`route_for`)
- Create: `crates/comandos-runtime/tests/launch_command_oracle.rs`, `crates/comandos-runtime/tests/claude_trust_oracle.rs`

**Interfaces:**
- Produces: `launch_command::{configuration_command(&Ctx, &str, &str, &str, &str, &str, &[String], bool) -> Result<String, ConfigError>, launch_model_id(&Value, &str, &str) -> String, same_model(&Value, &str, &str, &str) -> bool, Ctx { registry: Value, home: PathBuf, search_path: Option<OsString>, proxy_port: u16, repo_root: PathBuf }}`, `ConfigError::{Value(String), Unsure}`; `extension_launch::{wrap_command, wrap_environment, launch_from_pid, verify_launch, configuration_status, inventory, internal_inventory, normalize, capture_opencode_environment, prepare_launch}`; `claude_trust::{ensure_cwd_trusted, inherit_cwd_trust, cwd_trusted_in}`; `launch_command::inherit_trust_for_switch`; `dialogs::{DialogCache, screen_dialog, verify_attempts}`.

**Confinamiento:** ningún tmux; archivos solo bajo un `tempdir` usado como HOME; `which` sobre un `PATH` con binarios falsos (`/bin/true` enlazado como `claude`, `codex`, `grok`). La prueba de gateway abre un `TcpListener` en `127.0.0.1:0` y pasa ese puerto.
**Efectos en vivo:** solo escribe `.claude.json` de la carpeta de cuenta destino (confianza de carpeta) con el mismo candado y volcado, y archivos `environment-*.json`/manifiestos 0600 en `H/extension-launches`, como el Python.

- [ ] **Step 0: Oráculo vivo (D8)** — Run: `git -C ~/codebase/0xJesus/ComandOS diff -- bin/cc-dash lib/claude_trust.py | grep -n 'inherit_trust_for_switch\|_inherit_codex_trust\|def inherit'`. Si hay cambios sin `main`, parar y pedir al controlador que los confirme o descarte; se porta lo que corre el heredado.
- [ ] **Step 1: Pruebas que fallan**

```rust
//! launch_command contra `_configuration_command` del Python.
use comandos_runtime::launch_command::{configuration_command, Ctx};
use std::process::Command;

fn python(home: &std::path::Path, call: &str) -> Option<String> {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let code = format!(
        "import sys, json; sys.path.insert(0, {lib:?}); \
         from importlib.machinery import SourceFileLoader; \
         dash = SourceFileLoader('dash', {dash:?}).load_module(); \
         import json as j; dash.load_provider_registry = lambda: j.load(open({reg:?})); \
         try:\n    print(json.dumps({call}))\nexcept Exception as e:\n    print(json.dumps({{'error': str(e)}}))",
        lib = repo.join("lib").to_string_lossy(),
        dash = repo.join("bin/cc-dash").to_string_lossy(),
        reg = home.join("providers.json").to_string_lossy(),
    );
    let out = Command::new("python3").arg("-c").arg(code)
        .env("HOME", home).env("PATH", home.join("bin")).env_remove("TMUX")
        .output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[test]
fn account_switch_main_has_no_config_dir() {
    let home = support::fixture_home(); // providers.json con accountsRoot en el HOME, bin/ con claude→/bin/true
    let ctx = Ctx::for_tests(&home);
    let to_relotto = configuration_command(&ctx, "claude", "claude", "", "", "relotto", "sid-1", &[], true).unwrap();
    let to_main = configuration_command(&ctx, "claude", "claude", "", "", "main", "sid-1", &[], true).unwrap();
    assert!(to_relotto.contains("CLAUDE_CONFIG_DIR=") && to_relotto.contains("relotto"));
    assert!(!to_main.contains("CLAUDE_CONFIG_DIR="));
    assert!(to_main.contains("-u CLAUDE_CONFIG_DIR"));
    for (alias, ours) in [("relotto", &to_relotto), ("main", &to_main)] {
        let Some(theirs) = python(&home, &format!("dash._configuration_command('claude','claude','','',{alias:?},'sid-1',(),preserve_model_flags=True)")) else { return };
        assert_eq!(serde_json::to_string(ours).unwrap(), theirs, "{alias}");
    }
}

#[test]
fn flags_filter_matches_python() {
    let home = support::fixture_home();
    let ctx = Ctx::for_tests(&home);
    let flags: Vec<String> = ["--model", "x", "--effort=high", "-c", "model=\"y\"", "-c", "sandbox=\"ro\"",
        "--config=model_reasoning_effort=low", "--yolo"].iter().map(|s| s.to_string()).collect();
    for preserve in [false, true] {
        let ours = configuration_command(&ctx, "codex", "codex", "gpt-5.5", "high", "main", "sid", &flags, preserve);
        let call = format!("dash._configuration_command('codex','codex','gpt-5.5','high','main','sid',{flags:?},preserve_model_flags={})",
            if preserve { "True" } else { "False" });
        let Some(theirs) = python(&home, &call) else { return };
        assert_eq!(support::as_python_json(&ours), theirs);
    }
}
```

Más casos en el mismo archivo: `acp` con `--agent` y `--account`, `opencode --session`, `agy --conversation`, binario ausente, gateway caído (`claude` con motor `codex` y puerto sin escuchar), `launch_model_id("claude","opus-5-5")`, `same_model` con y sin registro, `wrap_command` con `;` en el comando (error literal), `capture_opencode_environment` con `OPENCODE_CONFIG`. `claude_trust_oracle.rs`: `inherit_cwd_trust` y `ensure_cwd_trusted` sobre dos HOME gemelos (uno para cada lado) y comparación de `.claude.json` byte a byte. `support` aquí es un módulo de pruebas de `comandos-runtime` (`tests/support/mod.rs` de ese crate, nuevo) con `fixture_home()` y `as_python_json(&Result<String, ConfigError>)` (`Ok(s)` → `json.dumps(s)`; `Err(Value(m))` → `{"error": m}`).

- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-runtime --test launch_command_oracle --test claude_trust_oracle` · Expected: FAIL de compilación.
- [ ] **Step 3: Implementar** según las reglas.
- [ ] **Step 4: Ver que pasa** — misma orden · Expected: PASS.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-runtime/src/launch_command.rs crates/comandos-runtime/src/extension_launch.rs \
  crates/comandos-runtime/src/claude_trust.rs crates/comandos-runtime/src/dialogs.rs crates/comandos-runtime/src/lib.rs \
  crates/comandos-runtime/tests/launch_command_oracle.rs crates/comandos-runtime/tests/claude_trust_oracle.rs \
  crates/comandos-runtime/tests/support/mod.rs
git commit -m "feat(runtime): comando de lanzamiento, envolturas de extensiones y confianza de carpeta portados

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

(Añadir `crates/comandos-runtime/src/providers.rs` a la lista si se tocó.)

---

### Task 2: Adaptador `SessionConfiguration`, dueño de `motor-results.json` y entrada `session_configure`

Comportamiento portado (todas las líneas de `bin/cc-dash`):

- **`SessionConfiguration`** (2748–3192) implementando `Adapter`: `__init__` (agente original por `agent_info_for_pane` de la 2d, `original_start`, `frm` con la regla de shells), `check_identity`, `prepare` y `_prepare_account_only`, `wait_idle` (1350 × 2 s; `harness_pane_busy` 1914), `snapshot` (con `tmux_snapshot::capture_session` del maestro, `observe_pane` de la 2d, `_snapshot_transcript` 2366, `_copy_conversation` y `_compatible_copy`, `capture_handoff` 1926 con `open(path, 'x')` → `OpenOptions::create_new` y `chmod 0600` después de crear como el Python, `extension_launch`), `apply` (`_pane_exit_current` 2075, `_restore_shell_tty` 1846, `inherit_trust_for_switch`, `_send_configuration_command` 2648 con `env 'COMANDOS_OPERATION_ID=<id>' <comando>`), `_verify` (60 × 0,5 s; `_read_environ`, `pin` del proceso destino y `store.stage('verifying')`), `verify` (`verify_attempts`), `pending_confirmation`, `rollback` (3150). Los textos de `ValueError`/`RuntimeError` son literales.
- **`PaneExtensionConfiguration`** (3194–3298): subclase; `prepare`, `snapshot`, `_verify`, `rollback` propios. En Rust: `enum Kind { Session, Extensions }` dentro del mismo `struct` con `match` en esos cuatro métodos.
- **`_pane_exit_current`**: secuencia de teclas con las esperas literales (0,12 / 0,15 / 0,1 / 0,3 s), 30 × 0,5 s, `SIGTERM` al pid **solo** si `_process_start(pid) == started` (comparación previa con `expected_start`), espera 1 s. La señal: `rustix`/`nix` no están en el workspace y `unsafe` está prohibido; se usa `/bin/kill -15 <pid>` por `std::process::Command` con plazo 2 s (mismo efecto; el `except Exception: pass` del Python se replica ignorando el resultado), y la comprobación `os.kill(pid, 0)` se hace leyendo `/proc/<pid>` (existe ↔ vivo; un zombi cuenta como vivo igual que `kill(0)`).
- **`run_operation`**: el ya portado (`comandos_runtime::session_operations::run_operation(store, id, adapter, notify)`).
- **`MotorResults`** (`ops/results.rs`, O3): `get_all() -> Map`, `set(key, ok, detail, fields)` = `motor_result_set` (detail con `str(detail)[:200]` en caracteres, `ts = time.time()`, campos `None` omitidos), `stage(key, stage, code, fields)` = `motor_stage` (`[:120]`, `stageCode[:40]`); recorte y escritura de O3, en el hilo que llama (las rutas lo llaman dentro de `spawn_blocking`; los hilos de operación directamente).
- **`configure.rs`**: `session_configure(native, data) -> (StatusCode, Value)` literal de 3336 (validaciones, `requestId` = `secrets.token_hex(16)`, `store.get` primero, identidad, `claim`, `require_revision` para extensiones, `notify`, `motor_stage("validando configuración", "validating")`, hilo de O1, `202`); `session_recover` (3387); `refresh_session_confirmation` (3300); `record_runtime_config` por `Lane<UsageBackend>` (desde el hilo de operación: `Handle::block_on(native.usage.call(...))` — permitido porque el hilo no es del runtime); `motor_queue_resume` (3521) como función que T3 registra en el arranque con `Background::Front` (O4).

**Files:**
- Create: `crates/comandos-runtime/src/session_configuration.rs`, `crates/comandos-runtime/src/pane_exit.rs`; Modify: `crates/comandos-runtime/src/lib.rs`
- Create: `crates/comandos-server/src/dash/native/ops/results.rs`, `crates/comandos-server/src/dash/native/ops/configure.rs`; Modify: `crates/comandos-server/src/dash/native/ops/mod.rs` (declaraciones de submódulos)
- Create: `crates/comandos-runtime/tests/session_configuration_oracle.rs`, `crates/comandos-server/tests/support/ops.rs` (contenido), `crates/comandos-server/tests/dash_native_ops_results.rs`

**Interfaces:**
- Consumes: T1; maestro T3 (`tmux_snapshot`, `target::{pane_identity, identity_key}`); 2d `agent_procs::{agent_info_for_pane, process_start, read_environ}`, `pane_snapshot::PaneInspector`, `observe_pane` (si vive en `comandos-server` 2d, se mueve a `comandos-runtime` en este paso sin cambiarlo y se dice en el commit); `session_operations::{OperationStore, run_operation, open_journal, Adapter}`.
- Produces: `session_configuration::{SessionConfiguration, Kind, Env { tmux: TmuxSync, home, hooks, proc_root, registry, repo_root, search_path, sleep: fn(Duration) }, TmuxSync}`; `pane_exit::{exit_current, restore_shell_tty, send_configuration_command, capture_handoff}`; `ops::results::MotorResults::{shared(&NativeOptions) -> Arc<MotorResults>, set, stage, all}`; `ops::configure::{session_configure, session_recover, refresh_session_confirmation, motor_queue_resume}`; `support::ops::{FakeCodex, seed_registry}`.

**Confinamiento:** las pruebas de `comandos-runtime` crean su servidor tmux con `PrivateTmux` (`-S <tempdir>/tmux-<uid>/default`, `-f /dev/null`, sin `TMUX`) y le pasan al adaptador un `TmuxSync` que antepone ese `-S`; los agentes son `FakeCodex` (el stub en C de `tests/test_session_tmux.py`, compilado por la prueba con `cc` en el tempdir; sin `cc`, la prueba avisa y sale) y un `claude` falso equivalente (mismo stub que escribe `projects/<slug>/<sid>.jsonl`); `CODEX_HOME`/`CLAUDE_CONFIG_DIR` de cada cuenta bajo el tempdir; `sleep` del `Env` acorta las esperas a ≤ 40 ms como el `monkeypatch` de `time.sleep` del Python. Las únicas señales van al pid del agente falso lanzado por la prueba (y `exit_never_signals_foreign_pid` lo comprueba con un `sleep 30` cuyo inicio se falsea en la anotación). Sin `systemd-run`.
**Efectos en vivo:** idénticos al Python, en el mismo orden: espera a que el agente quede libre (salvo `interrupt`), copia del layout y del historial (solo lectura salvo `_copy_conversation`, que copia el transcript a la cuenta destino tras validarlo y nunca sobrescribe un historial que divergió), `session-handoffs/<id>.md` 0600, salida del agente del pane **pedido** (`Escape`, `C-u`, `/exit` o `C-c`; `SIGTERM` solo al pid anotado y con el mismo inicio), `stty sane` del shell, confianza de carpeta en la cuenta destino, el comando `env COMANDOS_OPERATION_ID=… <comando>` tecleado en ese pane, verificación y, si falla, `rollback` con el comando de vuelta guardado. Nada fuera de ese pane salvo `capture_session` (lectura de toda la sesión).

- [ ] **Step 1: Pruebas que fallan**

`session_configuration_oracle.rs` (comandos-runtime), con el patrón de `tests/test_session_tmux.py`:

```rust
//! El adaptador portado contra el escenario real de tests/test_session_tmux.py:
//! tmux privado (-S), Codex falso compilado, HOME temporal. Ningún proveedor real.
mod support;

use comandos_runtime::session_configuration::{Kind, SessionConfiguration};
use comandos_runtime::session_operations::{run_operation, OperationStore};
use support::{fake_codex, PrivateTmux};

const SID: &str = "11111111-1111-1111-1111-111111111111";

#[test]
fn account_switch_keeps_conversation_and_other_pane() {
    let Some(lab) = support::OpsLab::start() else { return }; // sin tmux o cc: aviso y salida
    let pane = lab.pane.clone();
    let other_before = lab.tmux.capture(&lab.other);
    let data = serde_json::json!({"session": "audit", "pane": pane, "requestId": "req-account-0001",
        "harnessAccount": "work", "motorAccount": "work", "accountOnly": true, "interrupt": true});
    let store = OperationStore::new(&lab.journal);
    assert!(store.claim("req-account-0001", &lab.identity_key(), &data).unwrap());
    let mut adapter = SessionConfiguration::new(Kind::Session, data, lab.identity(), lab.env()).unwrap();
    run_operation(&store, "req-account-0001", &mut adapter, |_, _| {}).unwrap();
    let row = store.get("req-account-0001").unwrap().unwrap();
    assert_eq!(row["state"], "confirmed", "{row}");
    assert_eq!(row["result"]["observed"]["conversationId"], SID);
    assert_eq!(row["result"]["observed"]["harnessAccount"], "work");
    assert_eq!(lab.tmux.capture(&lab.other), other_before, "el otro pane no se toca");
    assert!(lab.home.join(".codex-accounts/work/sessions").read_dir().unwrap().count() > 0, "historial copiado");
}

#[test]
fn stuck_origin_types_nothing() {
    let Some(lab) = support::OpsLab::start_with(support::Stub::IgnoresExit) else { return };
    // … misma petición; el stub ignora /exit, C-c y SIGTERM (atrapa la señal).
    // Expected: estado failed o recovery_required con 'el agente original no cerró'
    // y el pane sin la línea 'env COMANDOS_OPERATION_ID'.
}

#[test]
fn exit_never_signals_foreign_pid() {
    let Some(lab) = support::OpsLab::start() else { return };
    let victim = std::process::Command::new("sleep").arg("30").spawn().unwrap();
    let pid = i64::from(victim.id());
    let started = comandos_runtime::agent_procs::process_start(std::path::Path::new("/proc"), pid);
    let ok = comandos_runtime::pane_exit::exit_current(&lab.env(), &lab.pane, pid, "codex", Some("otro-inicio"));
    assert!(!ok);
    assert_eq!(comandos_runtime::agent_procs::process_start(std::path::Path::new("/proc"), pid), started, "sigue vivo");
    let mut victim = victim;
    let _ = victim.kill(); // proceso hijo propio de la prueba
    let _ = victim.wait();
}
```

`support::OpsLab` (en `crates/comandos-runtime/tests/support/mod.rs`, ampliado): compila los stubs, crea `PrivateTmux` con `-S`, siembra dos panes `/bin/sh`, lanza en el primero el Codex falso con `_configuration_command` y espera la observación confirmada (como la fixture `live` del Python), y en `Drop` hace `kill-server` con su `-S` y después borra el directorio. Cada caso del escenario Python (`change`, `account`, `launch-failure`, `old prompt`) tiene su réplica; las tres primeras se comparan además con el Python ejecutando la misma fixture (`python3 -m pytest tests/test_session_tmux.py -k <caso>` no: el oráculo es el comportamiento **observable** — estado final del journal, comando tecleado, archivos copiados — sobre el mismo escenario; se ejecuta el Python con `SourceFileLoader` y un `tmux` que fuerza el mismo `-S`).

`dash_native_ops_results.rs` (servidor):

```rust
#[test]
fn motor_results_trim_matches_python() {
    let home = support::TestHome::new("motor-trim");
    let mut seed = serde_json::Map::new();
    for i in 0..301 { seed.insert(format!("s{i}|%{i}"), serde_json::json!({"ok": true, "detail": "x", "ts": f64::from(i)})); }
    home.write("motor-results.json", &serde_json::to_string(&seed).unwrap());
    let twin = home.clone_dir("motor-trim-py");
    let results = comandos_server::dash::native::ops::results::MotorResults::load(&home.hooks().join("motor-results.json"));
    results.set_with_ts("nuevo|%1", false, "detalle ñ", serde_json::Map::new(), 1000.0);
    let Some(()) = support::tabs::python_dash(&twin, "dash.time.time = lambda: 1000.0\ndash.motor_result_set('nuevo|%1', False, 'detalle ñ')") else { return };
    assert_eq!(std::fs::read(home.hooks().join("motor-results.json")).unwrap(),
               std::fs::read(twin.hooks().join("motor-results.json")).unwrap());
}
```

(`TestHome::clone_dir` copia el HOME a otro temporal; si no existe, se añade en `support/ops.rs` como función libre `clone_home(&TestHome, tag) -> TestHome`.)

- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-runtime --test session_configuration_oracle && $C test -p comandos-server --test dash_native_ops_results` · Expected: FAIL de compilación.
- [ ] **Step 3: Implementar.** Esqueleto del arranque del hilo en `configure.rs`:

```rust
/// O1: la operación corre en un hilo de sistema propio, como el `threading.Thread`
/// del Python; el runtime de tokio nunca espera sus esperas de minutos.
fn spawn_operation(native: &Native, request_id: String, adapter: SessionConfiguration, opkey: String) -> Result<(), Fault> {
    let journal = native.options().hooks.join("session-operations.sqlite3");
    let results = MotorResults::shared(native.options());
    let usage = native.usage_handle();
    let handle = tokio::runtime::Handle::current();
    std::thread::Builder::new()
        .name("comandos-op".into())
        .spawn(move || {
            let Ok(conn) = open_journal(&journal) else {
                eprintln!("comandos dash: journal ilegible; la operación {request_id} no corre");
                return;
            };
            let store = OperationStore::new_with(&conn, std::process::id());
            let mut adapter = adapter;
            let notify = |stage: &str, result: Option<&Value>| notify(&results, &usage, &handle, &opkey, &request_id, stage, result);
            if let Err(e) = run_operation(&store, &request_id, &mut adapter, notify) {
                eprintln!("comandos dash: operación {request_id}: {e}");
            }
        })
        .map(|_| ())
        .map_err(|_| Fault::Error(HandlerError::Failure))
}
```

(Si `OperationStore::new` del runtime toma otra forma, usar la real. `notify` = la función interna del Python: `motor_result_set` en estados finales, `record_runtime_config` si confirmado, `motor_stage` en los intermedios.)

- [ ] **Step 4: Ver que pasa** — mismas órdenes · Expected: PASS.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-runtime/src/session_configuration.rs crates/comandos-runtime/src/pane_exit.rs \
  crates/comandos-runtime/src/lib.rs crates/comandos-runtime/tests/session_configuration_oracle.rs \
  crates/comandos-runtime/tests/support/mod.rs \
  crates/comandos-server/src/dash/native/ops/results.rs crates/comandos-server/src/dash/native/ops/configure.rs \
  crates/comandos-server/src/dash/native/ops/mod.rs crates/comandos-server/tests/support/ops.rs \
  crates/comandos-server/tests/dash_native_ops_results.rs
git commit -m "feat(runtime): adaptador de configuración de sesión portado; dueño único de motor-results.json

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Rutas de configuración y `/model/status` completo

Rutas: POST `/session/configure`, `/account/switch`, `/model/switch`; GET `/model/status` (los casos que la 2c declina).

Comportamiento portado:

- Las tres POST tras `target::post_target` con `CONFIG_ROUTES` (sin sustituir la sesión resuelta): `configuration = dict(data, session=sess, pane=str(data.get("pane") or pane))`; `/account/switch` → `account_switch_configuration` (2703: quita `model`, `effort`, `routeId`, `motor`, `toHarness`; añade `harnessAccount`, `motorAccount` = `str(alias or "main")`, `accountOnly: true`, `interrupt: data.get("interrupt") is not False`); `/model/switch` → `session_configure(dict(data, session=sess, pane=pane))` (sin el `str(data.get("pane") or pane)`: el pane del preámbulo); respuesta `(code, body)`.
- **`/model/status`**: la 2c resolvía el caso con fila y estado no pendiente; ahora, en `operations.rs` de la 2c (excepción a D4 documentada: ningún otro grupo de la 2f toca ese archivo), el caso «sin fila» lee `MotorResults::all()` (`session_operation_status` 6217) y el caso `awaiting_confirmation` llama a `refresh_session_confirmation` (T2) en `spawn_blocking` con su conexión al journal. Con `cuts_off ∋ Ops` se conserva el `Decline` de la 2c para esos dos casos.
- Arranque: `Background::Front` → `motor_queue_resume` una vez (O4), en `spawn_blocking`, registrado por `ops::start(native)` — llamada que añade la tarea final del maestro en `Native::new` (el maestro TF ya lista los `start` de cada dominio; este paso solo exporta `ops::start`).

**Files:**
- Modify: `crates/comandos-server/src/dash/native/ops/mod.rs`, `crates/comandos-server/src/dash/native/operations.rs`
- Create: `crates/comandos-server/tests/dash_native_ops.rs`
- Modify: `xtask/parity/2f/ops.jsonl`

**Interfaces:**
- Consumes: T2; maestro T3 (`target`), T1 (`Background`, `Cut`).
- Produces: `OpsRoute::{SessionConfigure, AccountSwitch, ModelSwitch}`, `ops::ROUTES`, `ops::start(&Native)`.

**Confinamiento:** pruebas con el gemelo (`support::twin::Twin`): cada lado con su HOME, su tmux privado (`-S`) sembrado con un pane que corre el `FakeCodex` (lanzado con `run_tmux` + `send-keys` del comando de configuración), su journal y su `motor-results.json`. Las peticiones que llegan a teclear lo hacen en ese pane privado. Sin `systemd-run`.
**Efectos en vivo:** los de T2 sobre el pane pedido y nada más; escritura del journal (`claim`) y de `motor-results.json`, como el Python.

- [ ] **Step 1: Pruebas que fallan**

```rust
//! Corte ops, T3: configuración de sesión contra el Python (gemelo, tmux privado -S).
mod support;

use support::{ops::seed_fake_codex, twin::Twin};

#[tokio::test]
async fn configure_routes_match_python() {
    let Some(t) = Twin::start("ops-cfg", seed_fake_codex).await else { return };
    for (path, body) in [
        ("/session/configure", r#"{"session":"audit","pane":"%0","model":"gpt-5.6-luna","requestId":"req-cfg-00000001"}"#),
        ("/session/configure", r#"{"session":"audit","pane":"%0","requestId":"corto"}"#),
        ("/session/configure", r#"{"session":"audit","pane":"0"}"#),
        ("/account/switch", r#"{"session":"audit","pane":"%0","alias":"work","requestId":"req-acc-00000001"}"#),
        ("/account/switch", r#"{"session":"audit","pane":"%0","alias":"nadie","requestId":"req-acc-00000002"}"#),
        ("/model/switch", r#"{"session":"audit","pane":"%0","model":"gpt-5.5","effort":"low","requestId":"req-mod-00000001"}"#),
    ] {
        t.post(path, body).await.assert_same();
        t.wait_operations_idle().await; // sondea GET /model/status en ambos lados hasta estado final (≤ 20 s)
        t.files_equal(&["motor-results.json"]).unwrap();
    }
    assert_eq!(t.journal_rows_a(), t.journal_rows_b(), "mismas filas finales (ids, estados, resultados normalizados)");
}

#[tokio::test]
async fn concurrent_configure_runs_once() {
    let Some(t) = Twin::start("ops-conc", seed_fake_codex).await else { return };
    let a = t.post_front("/session/configure", r#"{"session":"audit","pane":"%0","effort":"low","requestId":"req-conc-0000001"}"#);
    let b = t.post_front("/session/configure", r#"{"session":"audit","pane":"%0","effort":"medium","requestId":"req-conc-0000002"}"#);
    let (a, b) = tokio::join!(a, b);
    let bodies = [a.text(), b.text()];
    assert_eq!(bodies.iter().filter(|s| s.contains("\"queued\": true")).count(), 1, "{bodies:?}");
}

#[tokio::test]
async fn model_status_without_row_reads_motor_results() {
    let Some(t) = Twin::start("ops-status", |h| {
        h.write("motor-results.json", r#"{"audit|%0": {"ok": false, "detail": "x", "ts": 1.0}}"#);
    }).await else { return };
    t.get("/model/status?operationKey=audit%7C%250").await.assert_same();
}
```

`Twin::{post_front, wait_operations_idle, journal_rows_a, journal_rows_b}` se añaden en `support/ops.rs` como funciones sobre `&Twin` (no en `twin.rs`): `journal_rows_*` lee `session-operations.sqlite3` de cada lado con `rusqlite` y normaliza `created/updated/owner_pid`.

- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_ops` · Expected: FAIL.
- [ ] **Step 3: Implementar** las rutas y los dos casos de `/model/status`.
- [ ] **Step 4: Fixture** — `xtask/parity/2f/ops.jsonl`: `o-configure-pane-invalido`, `o-configure-requestid-invalido`, `o-account-switch-sin-pane`, `o-model-status-sin-fila` (con `setup` vacío; sin efectos que dependan de un agente).

```json
{"name":"o-configure-pane-invalido","method":"POST","path":"/session/configure","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"s","pane":"0"},"volatile":[],"expect":"same"}
{"name":"o-configure-requestid-invalido","method":"POST","path":"/session/configure","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"s","pane":"%0","requestId":"x"},"volatile":[],"expect":"same"}
{"name":"o-model-status-sin-fila","method":"GET","path":"/model/status?operationKey=nadie%7C%250","headers":{"Host":"127.0.0.1"},"volatile":[],"expect":"same"}
```

- [ ] **Step 5: Ver que pasa** — Run: `$C test -p comandos-server --test dash_native_ops` · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-server/src/dash/native/ops/mod.rs crates/comandos-server/src/dash/native/operations.rs \
  crates/comandos-server/tests/dash_native_ops.rs crates/comandos-server/tests/support/ops.rs
git add -f xtask/parity/2f/ops.jsonl
git commit -m "feat(dash): configure, account/switch y model/switch nativos; /model/status sin declinaciones

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Extensiones por pane

Rutas: GET `/pane-extensions` (prefijo), POST `/pane-extensions`, `/pane-extensions/apply`, `/template`, `/cancel`, `/recover`.

Comportamiento portado:

- **`pane_extensions.py`** (113 líneas: `ExtensionStore` sobre el journal: `state`, `save`, `require_revision`, `templates`, `save_template`; `ExtensionConflict`; `selection_value`) → `comandos_runtime::pane_extensions`, con prueba diferencial de filas.
- **`extension_observations.conversation_usage`** (197 líneas) → `comandos_runtime::extension_observations`.
- **`_extension_target`**, **`_extension_operation`**, **`pane_extensions_state`**, **`_extension_guards`**, **`_extension_template_name`**, **`pane_extensions_write`** (2158–2306) literales. GET: `query = {k: v[-1]}` (última aparición; `parse_qs` sin `keep_blank_values`); `ValueError`/`RuntimeError` → `409 {"ok": false, "error": "No se pudo identificar el panel y su conversación."}`; otras excepciones → 500 del manejador (`OSError` de `pane_extensions_state` no se captura en el GET: replicarlo como `Fault::Error(HandlerError::Failure)`). POST: los tres grupos de excepciones del Python con sus códigos.
- `/apply` y `/recover` entran en `session_configure`/`session_recover` (T2): mismo exactamente-una-vez.

**Files:**
- Create: `crates/comandos-runtime/src/pane_extensions.rs`, `crates/comandos-runtime/src/extension_observations.rs`; Modify: `crates/comandos-runtime/src/lib.rs`
- Create: `crates/comandos-server/src/dash/native/ops/extensions.rs`; Modify: `crates/comandos-server/src/dash/native/ops/mod.rs`
- Create: `crates/comandos-server/tests/dash_native_ops_extensions.rs`; Modify: `xtask/parity/2f/ops.jsonl`

**Interfaces:**
- Consumes: T1, T2, T3 (`session_configure`, `session_recover`, `refresh_session_confirmation`).
- Produces: `OpsRoute::{ExtensionsGet, ExtensionsWrite(Action)}`, `ExtensionStore`, `conversation_usage`.

**Confinamiento:** gemelo con el `FakeCodex` en el tmux privado (`-S`) de cada lado; el inventario de extensiones de la prueba sale de un `~/.codex/config.toml` y `~/.agents/skills/*` del HOME temporal (sin MCP reales); `/apply` relanza el agente falso. Sin `systemd-run`.
**Efectos en vivo:** guardar borradores y plantillas en el journal; `/apply` = un `session_configure` con `extensionsOnly` (efectos de T2 en ese pane); `/cancel` y `/recover` las transacciones del journal y el `rollback` de T2.

- [ ] **Step 1: Pruebas que fallan** — `extensions_routes_match_python`: GET con y sin `harness`, POST de borrador con la revisión correcta y con una vieja (409), `/template` con `name` y con `templateId`, `/template` con ambos (409 `indica nombre o plantilla`), campos sobrantes (400), `/apply` con `interrupt: "si"` (409 `interrupt inválido`), `/cancel` de una operación ajena (409), `/apply` repetido con el mismo `requestId` y cuerpo distinto (409); comparar cuerpos (normalizados: `revision`, `ts`) y las filas de las tablas de extensiones del journal.
- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_ops_extensions` · Expected: FAIL.
- [ ] **Step 3: Implementar**.
- [ ] **Step 4: Fixture** — `o-pane-extensions-sin-pane` (GET `?session=s`), `o-pane-extensions-campos` (POST con un campo desconocido → 400).
- [ ] **Step 5: Ver que pasa** — misma orden · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-runtime/src/pane_extensions.rs crates/comandos-runtime/src/extension_observations.rs \
  crates/comandos-runtime/src/lib.rs crates/comandos-server/src/dash/native/ops/extensions.rs \
  crates/comandos-server/src/dash/native/ops/mod.rs crates/comandos-server/tests/dash_native_ops_extensions.rs
git add -f xtask/parity/2f/ops.jsonl
git commit -m "feat(dash): estante de extensiones por pane nativo (borradores, plantillas, aplicar, cancelar, recuperar)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Perfiles de sesión — independiente de T1–T4

Rutas: GET `/session-profiles` (prefijo), POST `/session-profiles`, POST `/session-profile-apply` (8348, 8829, 8840).

Comportamiento portado: `lib/session_profiles.py` salvo `extension_usage` (ya en `comandos_store::usage_read`, 2e): `list_profiles`, `get_profile`, `save_profile`, `delete_profile`, `inventory` (con `_claude_mcp_sources`, `_codex_configs`, `_skill_roots`, `_skills`, `_frontmatter`, `launch_capabilities`), `launch_draft`, `launch_args(dry_run=True)` (con `dry_run` no escribe nada en `profile-launches`). La consulta de perfiles va por `Lane<UsageBackend>`; el inventario lee archivos en `spawn_blocking`. GET: `cwd` por omisión `os.path.expanduser("~")`, `harness` `codex`, `account` `main` (primera aparición: `(q.get(x) or [d])[0]`); `ValueError`/`OSError` → 400 `{"error": str(e)}`. `/session-profile-apply`: `resolve_route_selection(draft, "new_session")` (la de 2f-1/T4 si ya está fusionada; si no, esta tarea la porta a `comandos_runtime::providers::resolve_route_selection` y 2f-1/T4 la importa — quien llegue segundo borra su copia); errores → `409 {"error", "code": "profile_unavailable"}`.

**Files:**
- Create: `crates/comandos-store/src/session_profiles.rs`; Modify: `crates/comandos-store/src/lib.rs`
- Create: `crates/comandos-server/src/dash/native/ops/profiles.rs`; Modify: `crates/comandos-server/src/dash/native/ops/mod.rs`
- Create: `crates/comandos-store/tests/session_profiles_oracle.rs`, `crates/comandos-server/tests/dash_native_ops_profiles.rs`; Modify: `xtask/parity/2f/ops.jsonl`

**Interfaces:**
- Produces: `comandos_store::session_profiles::{list_profiles, get_profile, save_profile, delete_profile, inventory, launch_draft, launch_args}`; `OpsRoute::{ProfilesGet, ProfilesPost, ProfileApply}`.

**Confinamiento:** sin tmux ni procesos; base de uso y archivos de skills/MCP bajo el HOME temporal de cada lado del gemelo.
**Efectos en vivo:** filas de `session_profiles` en la base de uso (mismas sentencias SQL que el Python); ningún archivo (`dry_run`).

- [ ] **Step 1: Pruebas que fallan** — `session_profiles_oracle.rs`: guardar, listar, borrar y `inventory` sobre dos bases gemelas (Rust y `lib/session_profiles.py`), comparar filas y `json.dumps` del inventario; `dash_native_ops_profiles.rs`: las tres rutas con el gemelo (perfil válido, perfil sin nombre → 400, `profileId` inexistente → 409, `harness=claude` con `~/.claude/settings.json` y MCP del HOME).
- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-store --test session_profiles_oracle && $C test -p comandos-server --test dash_native_ops_profiles` · Expected: FAIL.
- [ ] **Step 3: Implementar**.
- [ ] **Step 4: Fixture** — `o-session-profiles-get`, `o-session-profile-apply-inexistente`, `o-session-profiles-post-invalido`.
- [ ] **Step 5: Ver que pasa** · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-store/src/session_profiles.rs crates/comandos-store/src/lib.rs \
  crates/comandos-store/tests/session_profiles_oracle.rs \
  crates/comandos-server/src/dash/native/ops/profiles.rs crates/comandos-server/src/dash/native/ops/mod.rs \
  crates/comandos-server/tests/dash_native_ops_profiles.rs
git add -f xtask/parity/2f/ops.jsonl
git commit -m "feat(dash): perfiles de sesión nativos (lista, guardado, borrado, aplicar en la próxima sesión)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Las 6 rutas de la barra de comandos (D8) y `/proxy`

- [ ] **Step 0: Decidir la rama (D8)** — Run: `git -C ~/codebase/0xJesus/ComandOS log --oneline -- dash/session-controls.js | head -3; git -C ~/codebase/0xJesus/ComandOS show main:dash/session-controls.js >/dev/null 2>&1 && echo EXISTE || echo BORRADO`. Si en `main` el archivo está **borrado** y `dash/index.html` ya no llama a `/proxy` ni a `/harness/switch` (`git -C … grep -n "'/proxy'\|/harness/switch" main -- dash bin`, sin resultados): **rama A** (retirar). Si no: **rama B** (portar). Anotar la rama en el mensaje del commit.

**Rama A — retiro (410).** Las rutas GET y POST `/proxy`, POST `/harness/switch`, `/model/switch-cancel`, `/session/recover`, GET `/session-config-history` pasan a `retired.rs` con el cuerpo de la 2c (`410` y el texto de retiro existente). Excepción: `cc-app` (`bin/cc-app:5323` y siguientes) y `cc-notifyd` no las llaman (comprobar con `git grep` en el Step 0); si alguna sí, esa ruta va por la rama B.

**Rama B — portar.** Comportamiento:
- **GET `/proxy`** (8333): `load_proxy_cfg` (`config/proxy.json` + `~/.claude/hooks/proxy.json`, ya en la 2d `providers::proxy_port`; ampliar a la configuración completa), `motor_global` (3672), `proxy_alive` (TCP 1 s), `proxy_settings_enabled`, `effort_defaults`, `session_effort_all` (`session-effort.json`), `"switchResult": MotorResults::all()`, `claude_accounts` sin `dir`, `engines` (`provider_public_state().claudeEngines`, 2d), `installed` (`which cc-model-proxy` o `~/.cargo/bin/claude-codex`), `codexLogin`, `grokLogin` (`grok_state.auth_identity`, 2d).
- **POST `/proxy`** (8802): `motor ∈ {claude, codex, grok}` → (`codex`/`grok` sin proxy → `proxy_set_enabled(True)`) + `motor_set_global` (3682) → 200; si no `proxy_set_enabled(bool(enable))`, `sleep(1.0)` (`tokio::time::sleep`), 200; cualquier excepción → `500 {"error": str(e)}`. `proxy_set_enabled` (3439): `systemctl --user enable|disable --now cc-proxy.service` (15 s), `_proxy_env_write` a cada cuenta Claude no `main`, `update_json_object` de `~/.claude/settings.json` (propaga el error).
- **POST `/harness/switch`** (`harness_switch_apply` 3429 + la rama de 9566), **`/model/switch-cancel`** (9578, `model_switch_cancel` 3484 con `MOTOR_GEN` como contador en memoria del módulo), **`/session/recover`** (8758 → `session_recover` de T2), **GET `/session-config-history`** (8250, `config_history` ya portado en `session_operations`).

**Files:**
- Rama A: Modify `crates/comandos-server/src/dash/native/retired.rs`; Create `crates/comandos-server/tests/dash_native_ops_bar.rs`.
- Rama B: Create `crates/comandos-server/src/dash/native/ops/bar.rs`; Modify `crates/comandos-server/src/dash/native/ops/mod.rs`; Create `crates/comandos-server/tests/dash_native_ops_bar.rs`.
- Ambas: Modify `xtask/parity/2f/ops.jsonl`.

**Interfaces:**
- Consumes: T2 (`MotorResults`, `session_recover`), T3 (`session_configure` vía `/harness/switch`).
- Produces: rama A: entradas nuevas en `retired::ROUTES`; rama B: `OpsRoute::{ProxyGet, ProxyPost, HarnessSwitch, SwitchCancel, SessionRecover, ConfigHistory}`.

**Confinamiento:** rama A sin efectos. Rama B: `systemctl` es el del `fakebin` (anota sus argumentos en `<HOME>/systemctl.log`, sale 0), el proxy «vivo» es un `TcpListener` de la prueba en el puerto que dice `config/proxy.json` del HOME temporal (si el puerto está fijado en el repo, la prueba no abre nada y comprueba el caso «no vivo»); `/harness/switch` y `/session/recover` usan el tmux privado (`-S`) y el `FakeCodex` de T3.
**Efectos en vivo (rama B):** `systemctl --user enable|disable --now cc-proxy.service` y la reescritura de los `settings.json` de las cuentas Claude, exactamente como el Python; el resto, los efectos de T2/T3.

- [ ] **Step 1: Pruebas que fallan** — rama A: las seis rutas responden el 410 de `retired.rs` (mismo cuerpo para todas) y no tocan archivos. Rama B: gemelo con cada ruta (GET `/proxy`; POST `/proxy {"enable":false}` y `{"motor":"codex"}`; `/model/switch-cancel` sin operación; `/session/recover` con `operationId` inexistente → 409; `/session-config-history?session=s&pane=%0`), comparar cuerpos, `settings.json` de `main` y de una cuenta y `systemctl.log`.
- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_ops_bar` · Expected: FAIL.
- [ ] **Step 3: Implementar** la rama elegida.
- [ ] **Step 4: Fixture** — rama A: `o-proxy-retirada` (GET) y `o-harness-switch-retirada` con `"expect":"same"` contra el heredado solo si el heredado también las retiró; si no, `"expect":"differs"` con `"reason":"D8: retirada en el frente, viva en el heredado sin llamadores"`. Rama B: `o-session-recover-inexistente`, `o-session-config-history-invalida`.
- [ ] **Step 5: Ver que pasa** · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
# Rama A
git add crates/comandos-server/src/dash/native/retired.rs crates/comandos-server/tests/dash_native_ops_bar.rs
# Rama B (en su lugar)
# git add crates/comandos-server/src/dash/native/ops/bar.rs crates/comandos-server/src/dash/native/ops/mod.rs \
#   crates/comandos-server/tests/dash_native_ops_bar.rs
git add -f xtask/parity/2f/ops.jsonl
git commit -m "feat(dash): rutas de la barra de comandos (D8: rama <A|B>) y /proxy

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Self-review

- **Cobertura**: las 18 entradas del corte `ops` del inventario del maestro están en el mapa (T3–T6); `motor_queue_resume` (hilo de arranque) en T2/T3.
- **Confinamiento y efectos en vivo**: en T1–T6; las señales a procesos solo al agente falso de la prueba (y la prueba negativa de pid ajeno).
- **Nombres**: `MotorResults`, `session_configure`, `session_recover`, `refresh_session_confirmation`, `motor_queue_resume` (T2) usados con esas firmas en T3, T4 y T6; `configuration_command`/`inherit_trust_for_switch` (T1) en T2 y en 2f-1/T4.
- **Independencia**: T1→T2→T3; T4 y T6 tras T3 (usan `session_configure` cableado); T5 independiente de todo el grupo (solo G0).
