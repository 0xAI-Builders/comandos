# Fase 2e — Uso y analítica nativos: plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** que el frente Rust `comandos dash` (4777) responda él mismo, byte a byte igual que el `cc-dash` Python (4781), todo lo de uso, cuotas y analítica que sigue reenviado y tiene llamador vivo —GET `/usage/state` (cada 10 s), GET `/analytics/week` (cada 60 s), GET `/accounts`, GET `/providers`, GET `/optimization/plans`, GET `/extension-usage`—, que el contexto de sugerencias de `/state` (2d) deje de pedirle `/usage/guard` y `/usage/analytics` al heredado, y que los efectos de `/usage/state` (importación de transcripts, bordes de pane, `pane-models.txt`, avisos de nivel de modelo, `record_pane`) tengan un único dueño en cada momento.

**Architecture:** cuatro capas. (1) **Puro** en `comandos-core::usage_state`: ensamblado de `build_usage_state`, ventanas de tokens, `attach_token_counts`, salud de credenciales, `fromisoformat` de Python 3.10, `percentile`/`wilson_interval`. (2) **SQL** en `comandos-store::usage_read` (lecturas y las dos escrituras ligeras: `record_pane`, `record_quota_snapshots`) y `comandos-store::usage_import` (importadores locales, poda, reconciliación). (3) **Lectores de archivos** en `comandos-runtime::limits` (rollouts de Codex, log de Grok, cuota de agy, cabeceras de Groq, credenciales OAuth). (4) **Frente** en `crates/comandos-server/src/dash/native/usage/`: caché de límites con la red en una tarea (`limits.rs`), rutas (`week.rs`, `providers.rs`, `extensions.rs`, `state.rs`), escritor de bordes y avisos de nivel (`pane_models.rs`), dueño de la importación (`import.rs`) sobre un segundo carril SQLite. La regla de dueño único: **quien responde `/usage/state` es dueño de sus efectos**; el frente solo la declina con el carril de uso apagado, que es permanente hasta reiniciar.

**Tech Stack:** Rust 1.96 (edition 2024, `unsafe_code = "forbid"`), tokio 1.53 (`rt`, `process`, `sync`, `time`, `net`), hyper 1.11 (cliente HTTP/1 hacia cc-notifyd), `reqwest = "=0.13.5"` (`default-features = false`, `features = ["rustls"]`; ya está en `Cargo.lock` por `comandos-extensions`) para `https://api.anthropic.com/api/oauth/usage`, rusqlite 0.40 (bundled), serde_json con `arbitrary_precision` + `preserve_order`, chrono/chrono-tz, `futures-util` (`BoxFuture`); arnés `xtask parity`/`xtask poll` en un netns (`unshare -Urn`).

**Spec:** `docs/superpowers/specs/2026-10-04-comandos-rust-complete-design.md` (§4.2, §5, §7, Enmiendas 1–8). Inventario: `docs/research/2026-10-04-fase-2-inventario.md` (§1.4, §1.6, §1.11, §3.3, §3.6, §9.6–9.8). Planes previos: `docs/superpowers/plans/2026-10-04-fase-2c-dominios-nativos-ii.md` y `docs/superpowers/plans/2026-10-04-fase-2d-state-nativo.md`; sus bloques **Interfaces** son la fuente de los nombres que este plan consume: de la 2c `Lane<B: LaneBackend>`, `UsageBackend`, `Native.usage`, `NativeOptions.{usage_db, repo_root, hooks, clock, tmux}`, `files::{read_json_strict, loads_strict, write_text_atomic, FileLock}`, `tmux::{run_program, Program, Tmux}`, `py::{int, int_error_message, strip, repr_ascii}`, `query::Query`, `support::{TestHome, front, oracle::oracle, NOW_MS}`; de la 2d `states::{StateFault, Engine, States}`, `Native::states_cached`, `states::context::Context`, `suggest::{SuggestContext, latency_from}`, `subrequest`, `comandos_runtime::{Unsure, providers::{RegistryCache, runtime_facts, selectable_routes, which, model_tier, proxy_port, read_conf, py_regex, py_search}, agent_procs::{AgentInfo, AgentMaps, AccountCache}}`, `NativeOptions.{proc_root, cwd, search_path, codex_home, grok_home, legacy, legacy_token}`, `support::oracle::run_python`. Procedimiento vivo: `docs/verification/cutover-dash.md`. Oráculo: `bin/cc-dash`, `bin/cc_usage.py`, `lib/providers.py`, `lib/accounts.py`, `lib/allocation.py`, `lib/analytics_week.py`, `lib/session_profiles.py`, `lib/grok_state.py`, `lib/model_catalog.py` de este checkout; las líneas citadas son las de `65528ca` (`bin/cc-dash` y `bin/cc_usage.py` no cambian en la 2c ni en la 2d). El intérprete del heredado es **Python 3.10.12**: `datetime.fromisoformat` no acepta `Z` ni la forma básica.

**Precondición:** las Fases 2c y 2d están fusionadas en `main`. Si al empezar algún nombre de esos bloques no coincide, se usa el real y se anota en el commit; no se reescriben la 2c ni la 2d. Si una función que este plan necesita existe en la 2d como detalle privado (`session_labels`, la recolección de `AgentMaps`, el correo por carpeta de `AccountCache`), la tarea que la necesita la extrae a `pub(crate)`/`pub` sin cambiar su comportamiento y lo dice en el commit.

**Cómo se dan los ports:** el código completo va donde hay diseño nuevo (caché de límites y su tarea de red, memo de un solo cómputo, escritor de bordes, dueño de la importación, orden de efectos, rutas). Los ports mecánicos de `cc_usage.py` (consultas, importadores, ensamblado) se dan por firma, regla con la cita de la línea y una prueba diferencial que corre el Python real sobre las mismas entradas (`python3 -c` sobre `bin/cc_usage.py`) y compara bytes o filas de la base: esa prueba es la especificación ejecutable.

---

## Rulings del controlador que fijan este plan

1. **Respuestas idénticas byte a byte** vía `comandos_core::json::response_dumps` (status, `Content-Type`, `Cache-Control: no-store`, cuerpo); textos de error literales del Python.
2. **SQLite solo a través de workers**: la base de uso por el carril `Lane<UsageBackend>` de la 2c (lecturas y escrituras cortas) y por un carril nuevo `Lane<UsageImportBackend>` sobre el mismo archivo (importación, que tarda segundos); app-state por el `BackendWorker<StateBackend>` (registros de Pomodoro, avisos de nivel). Puerta de esquema `user_version` ≤ `comandos_store::usage::SCHEMA_VERSION` (11) antes de abrir y antes de cada trabajo; la base de OpenCode se abre de solo lectura dentro del trabajo del carril de importación, como el Python.
3. **`Decline` antes de cualquier efecto.** Efectos de esta fase: `record_pane` (escritura por pane vivo en cada `/usage/state`), lanzar la importación, lanzar el refresco de límites, bordes de tmux, `H/pane-models.txt`, avisos de nivel. Orden obligatorio en `/usage/state`: primer trabajo del carril = lectura (punto de declinar); desde ahí nunca se declina (Decisión D1).
4. **Red y procesos en tareas async con los plazos y cachés del Python**: OAuth de Anthropic con plazo de 8 s por operación de socket (`_http_json` 3055) y caché de 60 s (180 s tras error de la cuenta `main`, `usage_provider_limits` 1309); `git rev-parse` 3 s (`git_root_for_path` 423); tmux de bordes 2 s (4540-4640); cc-notifyd 2 s (`usage_alert_send` 455). Nunca en el hilo del runtime.
5. **Nada que bloquee en el runtime `current_thread`**: recorridos de `~/.codex/sessions`, colas de rollouts, el log de Grok (hasta 64 MiB), credenciales y escrituras con `fsync` → `tokio::task::spawn_blocking`; la importación entera corre en el hilo del carril de importación.
6. **Memo y cachés equivalentes con cotas explícitas**: memo de `/usage/state` = una entrada (generación, firma de panes); caché de límites = una entrada; `seen` de la importación = solo las rutas del corte vigente (el `_IMPORT_SEEN` del Python crece sin tope); avisos de nivel = un registro por (sesión, pane) vivo; correos de cuentas = firma de archivo, máx. 64.
7. **Paridad**: líneas de fixture por ruta (`--usage-db`) y pruebas contra el oráculo Python por ruta y por capa.
8. **Cutover**: sección «2e» en `docs/verification/cutover-dash.md` con rutas explícitas como 2b/2c/2d.
9. Comentarios en español, identificadores en inglés, sin `unsafe`, sin `unwrap`/`expect`/indexado en código que no sea de prueba, clippy `-D warnings`, rustfmt, trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, `CARGO_TARGET_DIR=<checkout>/.build/target`, `nice -n 10 cargo … -j 6`, `git add` explícito, `git add -f` para `docs/superpowers` y `*.jsonl`; cero archivos Python o bash nuevos.
10. **Las pruebas nunca tocan** el HOME, las bases, el tmux, la red ni los puertos 4777–4782 reales: la red de límites va tras el rasgo `OauthHttp` y cc-notifyd tras `NotifyPost`, con dobles en las pruebas.
11. **Tareas desplegables una a una**, ordenadas por riesgo y peso de sondeo: lecturas puras (1) → límites sin ruta (2) → `/analytics/week` (3) → catálogos y `/accounts` (4) → contexto de sugerencias y `/extension-usage` (5) → motor de `/usage/state` latente (6) → bordes y avisos latentes (7) → importación y activación de `/usage/state` (8) → medición y cutover (9). Las Tareas 6 y 7 no añaden la ruta a la tabla nativa: se despliegan sin cambiar el comportamiento en producción y la Tarea 8 la activa cuando todos los efectos existen.

Reglas de oro heredadas: los cutovers los ejecuta el controlador; el Python no se modifica.

## Global Constraints

- Todo se compila y prueba con
  `CARGO_TARGET_DIR=/home/someguy/codebase/0xJesus/ComandOS/.build/target nice -n 10 cargo <cmd> -j 6`
  (abreviado `$C <cmd>`). Antes de cada commit: `$C fmt --all -- --check`, `$C clippy --workspace --all-targets -j 6 -- -D warnings` y las pruebas de los paquetes tocados.
- Commits con `git add <rutas>` explícitas; mensajes `feat(core): …` / `feat(store): …` / `feat(runtime): …` / `feat(dash): …` / `docs(verification): …` en español, con línea en blanco y `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. El plan: `git add -f docs/superpowers/plans/2026-10-04-fase-2e-uso-nativo.md`; el fixture: `git add -f xtask/parity/frente.jsonl`.
- Pruebas: `support::TestHome` (HOME temporal, `TMUX_TMPDIR` propio, sin `COMANDOS_STATE_DB`/`COMANDOS_USAGE_DB`/`CLAUDE_CONFIG_DIR`/`CODEX_HOME`/`GROK_HOME`); `TestHome::options()` pone `oauth = FakeOauth` y `notifyd = FakeNotify` (Tarea 2 y 7). Sin `tmux`, `git` o `python3` la prueba se salta con un aviso (no `#[ignore]`).
- Oráculo de capas: `python3 -c <texto>` que inserta `bin/` y `lib/` en `sys.path`, importa `cc_usage`, fija `cc_usage.time.time = lambda: NOW` cuando la función lee el reloj, y pasa `TZ=America/Mexico_City` en el entorno; el lado Rust recibe el mismo instante y la misma zona por parámetro. Ningún archivo `.py` nuevo (precedente: `crates/comandos-runtime/tests/hook_claude_parity.rs`).
- Excepción no capturada del Python → `HandlerError::Failure` (500 `{"error": "Error interno del tablero"}`). `sqlite3` con BLOB en una columna que llega a la salida (`json.dumps` falla) o texto no UTF-8 (`OperationalError`) → 500 igual.
- Directorios en el orden de `std::fs::read_dir` (el `getdents` de `os.walk`/`os.scandir`/`glob`); los conjuntos que el Python ordena (`files.sort(reverse=True)` sobre tuplas `(mtime, ruta)`) se ordenan igual: `mtime` como `f64` de `st_mtime` (segundos con fracción: `mtime_ns as f64 / 1e9`, el `float` de `os.path.getmtime`) y luego la ruta como bytes.
- El reloj de las rutas es `NativeOptions.clock` (ms); el Python usa `time.time()` en cada llamada, así que `generated_at`, `captured_at` y `lastSeen` son volátiles en la paridad.

## Decisiones del plan (no fijadas por el controlador)

- **D1 — Dueño único de los efectos de `/usage/state`.** El Python solo importa transcripts cuando su manejador de `/usage/state` corre (`refresh_local_usage(False)` 235 desde 8442; `/usage/refresh` está retirada con 410 y sin llamador). Si el frente responde `/usage/state`, el hilo de importación del Python no vuelve a arrancar; si el frente la declinara de vez en cuando, el Python importaría en paralelo. No hay un candado que el Python tome y no se puede modificar, así que la regla es: **quien responde `/usage/state` es dueño de la importación, los bordes, `pane-models.txt`, los avisos de nivel y `record_pane`**. Para que la regla sea exacta, el frente **no declina `/usage/state` salvo con el carril de uso apagado** (base más nueva, fallo al abrir, worker retirado): un estado permanente hasta reiniciar en el que el frente deja de importar y de escribir bordes y el Python, que recibe todas las peticiones, vuelve a ser dueño. Todo lo demás que podría ser incierto se resuelve sin declinar:
  - el cuerpo de `/usage/state` **no depende de `/state`** (`_pane_models_for_live_state` 286 copia filas y su resultado solo alimenta `write_pane_models`); si `Native::states_cached` declina, esa vuelta no escribe bordes (el sondeo siguiente, ≤ 10 s después, sí);
  - BLOB, texto no UTF-8, `OverflowError` y la configuración no UTF-8 son 500 deterministas, como en el Python.
  Alternativas descartadas: importar solo en el Python y dispararlo con un GET `/usage/state` de fondo al heredado duplicaría los bordes, los avisos de nivel y la red de límites; un candado compartido exigiría tocar el Python.
  Complementos: (a) candado `flock` no bloqueante `H/comandos-usage-import.lock` alrededor de cada importación del frente, para que dos frentes (producción y sombra, o el solape de un reinicio) nunca importen a la vez; (b) `--no-usage-effects` (o `COMANDOS_DASH_USAGE_EFFECTS=0`) apaga todos esos efectos en la sombra; (c) la primera importación del frente espera `usage_import_grace_ms` (75 s por omisión, `COMANDOS_DASH_USAGE_IMPORT_GRACE_MS`) desde que el frente arranca: un hilo de importación del Python que estuviera en curso al reiniciar termina antes (tarda ~3 s y no lo dispara nadie más).
- **D2 — Carril de importación separado.** La importación escribe durante segundos; en el carril de uso pararía `/pomodoro`, `/analytics/week` y `/usage/state`. `Lane<UsageImportBackend>` (Tarea 8) abre su propia conexión al mismo archivo con la misma puerta; la base está en WAL (`open_usage_db_at`), así que las lecturas del otro carril no esperan. Sus trabajos llaman a `git` con `Handle::block_on` desde el hilo del worker (el puente de la 2c).
- **D3 — Límites: caché del frente.** Mismo contrato que `_limits_cache` (414-419): `{at, limits, health}`, TTL 60 s (180 s si la salud `claude_oauth` es `error`), un solo refresco en vuelo, el que llega con la caché vencida lanza el refresco y responde **con lo que hay** (el Python también: la red corre en un hilo). El frente lanza un refresco al arrancar (el `_limits_snapshot_loop` del Python lo hace en su arranque, 9802) pero **no** repite cada 300 s: el heredado conserva su bucle de 5 min hasta la 2g. Llamadas a `api.anthropic.com`: antes, ≤ 1/min del Python mientras se sondea; después, ≤ 1/min del frente + 1/5 min del Python (+20 %); `record_quota_snapshots` converge porque su `upsert` solo acepta la foto más nueva.
- **D4 — Texto de error de red.** `health.error` es `str(e)[:240]` del `urllib` de Python. Se reproducen las clases comunes: DNS (resuelto por el frente con `tokio::net::lookup_host` antes de conectar) → `<urlopen error [Errno -3] Temporary failure in name resolution>` y hermanos; conexión rechazada/inalcanzable → `<urlopen error [Errno 111] Connection refused>`/`[Errno 101] Network is unreachable`; plazo al conectar → `<urlopen error timed out>`; plazo leyendo → `The read operation timed out`; HTTP ≥ 400 → `HTTP Error 429: Too Many Requests` (frase canónica; el cliente fuerza HTTP/1.1). Errores de TLS o un cuerpo que no es JSON dan un texto propio del frente: diferencia aceptada, documentada en el cutover (es un diagnóstico).
- **D5 — Mutación compartida de las filas de límites.** `usage_provider_limits` devuelve `list(_limits_cache["limits"])`: las mismas `dict`; `attach_token_counts` (964) les añade `tokens_7d`/`tokens_today` en el sitio y quedan hasta el siguiente refresco (solo rellena claves vacías: el primer valor se queda). El frente lo replica: `LimitsCache::attach_tokens` aplica la regla bajo el candado sobre las filas cacheadas y devuelve copias.
- **D6 — Memo de `/usage/state`.** `cached_usage_state` (269): clave `(generación, firma)` con `firma = sorted((tmux_session, tmux_pane, pane_pwd, agent))`; se construye con el candado tomado (las peticiones que llegan esperan y reutilizan). El frente: `tokio::sync::Mutex<Option<(MemoKey, Arc<Value>)>>` tomado durante la construcción; la generación sube solo al terminar una importación completa (`_do_refresh_local_usage` 416).
- **D7 — Entorno.** `usage_runtime_env` (179) mezcla `cc-notify.conf`, `usage.env`, `os.environ` del heredado y `usage_settings`. El frente toma de su entorno al arrancar solo las claves que el Python lee (`NativeOptions.usage_env`: `COMANDOS_DAILY_BUDGET_USD`, `COMANDOS_USAGE_DAILY_BUDGET_USD`, las cuatro `*_TOKEN_LIMIT` con y sin `COMANDOS_`, `COMANDOS_USAGE_LOCAL_DAYS`, `COMANDOS_USAGE_CLAUDE_MAX_FILES`, `COMANDOS_USAGE_CODEX_MAX_FILES`, `COMANDOS_CLAUDE_PROJECTS_DIR`, `COMANDOS_OPENCODE_DB`, `OPENAI_ADMIN_KEY`, `ANTHROPIC_ADMIN_KEY`, `LANG`) y la zona local (`chrono::Local`, la del `TZ`/`/etc/localtime` del proceso, como `datetime.fromtimestamp`). El paso 0 del cutover compara esas variables y `TZ` entre las dos unidades.
- **D8 — `/extension-usage` sin crear la base.** `extension_usage` abre `mode=ro` y responde vacío si el archivo no existe. El frente comprueba el archivo antes de tocar el carril; si existe, lee por el carril (que puede migrar una base vieja, como cualquier otra ruta de uso del Python).
- **D9 — `sidebar` de `/analytics/week`.** La rama `sidebar=1` (`analytics_week.sidebar_accounts`, ya portada en `comandos_core::analytics_week::sidebar_accounts`) está en el árbol de trabajo del checkout principal, no en `65528ca`. Si al empezar la Tarea 3 `bin/cc-dash` de `main` contiene `sidebar=(query.get("sidebar")`, se implementa con las columnas extra de `turns`; si no, la consulta con cualquier `sidebar` se declina (el Python commiteado la ignora y el `/analytics/week` sin `sidebar` no cambia).
- **D10 — Todas las rutas de uso declinan con el carril de uso apagado**, también `/accounts` (sus límites leen la base para Grok y Groq): carril apagado = el Python es dueño del dominio entero.
- **D11 — Rutas que siguen en el Python**, con la razón:
  - GET y POST `/session-profiles`, POST `/session-profile-apply`: `session_profile_store.inventory` lee TOML, plugins y frontmatter de skills (`lib/session_profiles.py:97-372`, `lib/capabilities.py`) y `apply` lanza sesiones; van con el dominio de extensiones y MCP (inventario §1.5).
  - GET `/opencode/models`: proceso `opencode` con caché en hilo (4721-4760); dominio de modelos.
  - `/usage/refresh`, `/usage/settings`, `/usage/quota`, `/usage/subscription`, `/usage/guard`, `/usage/analytics`, `/dedication` y el resto de §1.12: ya responden 410 nativo desde la 2b (sin llamador vivo). Sus funciones se portan aquí solo donde un llamador interno las usa (`token_guard_with_forecast`, `experiment_analytics` para el contexto de la 2d).
- **D12 — SQLite del sistema frente a la empaquetada.** El Python usa la `libsqlite3` del sistema y rusqlite la suya. Las consultas se copian **con el mismo texto SQL** (mismo plan de índices); los empates de orden (`order by turn_finished_at desc` con segundos iguales, `group by` antes de un `sort` estable) tienen filas de prueba propias en la Tarea 1.
- **D13 — `Native` comparte piezas por `Arc`.** Las tareas de fondo (refresco de límites, reconciliación de bordes, importación, envío a cc-notifyd) necesitan `'static`: `Native.usage` pasa a `Arc<Lane<UsageBackend>>` y las piezas nuevas (`limits`, `pane_models`, `import`) son `Arc`; las tareas capturan esos `Arc` y un clon de `NativeOptions` (ya es `Clone`), nunca `&Native`.

## Mapa de rutas nativas

| Ruta | Llave | Python (`65528ca`) | Respuesta 200 | Errores / efectos | T |
|---|---|---|---|---|---|
| GET `/analytics/week?offset&demo[&sidebar]` | Path | 8317 `analytics_week_query` 6610 → `analytics_week_payload` 6589 | modelo de `analytics_week.build_week` | 400 `offset inválido`, `solo esta semana (0) o la anterior (-1)`; `demo` → 404 `no hay datos de ejemplo con ese nombre`; JSON roto en el ejemplo → 500; lanza refresco de límites si vencidos | 3 |
| GET `/providers` | Path | 8277 `provider_public_state` 1722 | registro público + `matrix`, `midSessionRoutes`, `matrixSummary` | 500 `providers.json invalido: {e}` (registro incierto → declina) | 4 |
| GET `/optimization/plans` | Path | 8282 `optimization_plans` 1463 | `{plans, active}` | — | 4 |
| GET `/accounts?harness=` | Path | 8266 | `{harness, accounts}` (`account_menu`) | 404 `Ese CLI no maneja cuentas`; 500 `providers.json invalido: {e}`; lanza refresco de límites | 4 |
| GET `/extension-usage?session&pane&days` | Path | 8360 `extension_usage` (`lib/session_profiles.py:449`) | `{scope, days, provenance, extensions, …}` | 400 `sesión inválida`, `panel inválido`, `invalid literal for int() with base 10: '…'` | 5 |
| GET `/usage/state` | Path | 8441-8462 | estado de uso (`build_usage_state` + salud, alertas, límites enriquecidos, `lastInteraction`) | carril apagado → declina; **efectos**: `record_pane`, importación (≥ 60 s), refresco de límites, bordes, `pane-models.txt`, avisos de nivel | 6–8 |

`Path` = el `startswith` del Python reclamando solo la ruta exacta con consulta opcional (`/usage/stateX` y `/accountsX` siguen reenviadas). Interno (sin ruta): `token_guard_with_forecast` (1429) y `experiment_analytics(USAGE_DB, 7)` sustituyen la subconsulta de la 2d (Tarea 5).

## Review Focus

1. **El carril de uso se apaga con el frente vivo** (un Python más nuevo migra a `user_version = 12`): `/usage/state` se reenvía, el frente no lanza más importaciones ni escribe bordes, y una importación del frente ya en curso termina sin escribir (su carril aplica la misma puerta). Prueba `usage_lane_down_hands_effects_back` (Tarea 8).
2. **`api.anthropic.com` no responde** (conexión colgada): `/usage/state`, `/accounts` y `/analytics/week` responden al instante con la caché anterior, hay un solo refresco en vuelo y el runtime atiende otras rutas. Prueba `limits_hung_network_never_blocks` (Tarea 2).
3. **429 de la cuenta `main`**: se conservan las últimas filas buenas de esa cuenta con `stale: true` y el siguiente refresco espera 180 s, no 60. Prueba `limits_429_keeps_last_rows_and_backs_off` (Tarea 2).
4. **Dos frentes sobre el mismo HOME** (sombra y producción, o el solape de un reinicio): solo uno importa; el otro salta la vuelta sin esperar. Prueba `import_lock_contended_skips_cycle` (Tarea 8).
5. **Ráfaga de `/usage/state` justo después de una importación** (tablero, móvil y cc-app): un solo cálculo del memo; los demás esperan y comparten. Prueba `usage_state_memo_single_flight` (Tarea 6).

## Estructura de archivos

```
crates/comandos-core/src/usage_state.rs        (as_int/as_float/text, real_model, fromisoformat_epoch,
                                                LocalZone, normalize_pane_identity, build_state,
                                                attach_token_counts, credential_health, parse_env_text,
                                                percentile, wilson_interval)                         T1
crates/comandos-core/src/lib.rs                (pub mod usage_state)                                 T1
crates/comandos-core/tests/usage_state_oracle.rs                                                     T1
crates/comandos-store/src/usage.rs             (stable_id, TURN_INSERT_SQL, as_int… → pub(crate))    T1, T8
crates/comandos-store/src/usage_read.rs        (lecturas + record_pane + record_quota_snapshots)     T1
crates/comandos-store/src/usage_import.rs      (importadores, poda, reconciliación, configs)         T8
crates/comandos-store/tests/usage_read_oracle.rs, usage_import_oracle.rs                             T1, T8
crates/comandos-runtime/src/limits.rs          (lectores locales de límites, OAuth sin red)          T2
crates/comandos-runtime/src/agent_procs.rs     (EmailCache/email_for_dir pub si hace falta)          T2
crates/comandos-runtime/tests/limits_oracle.rs                                                       T2
crates/comandos-server/Cargo.toml              (reqwest)                                             T2
crates/comandos-server/src/dash/mod.rs         (DashConfig.usage_effects, flags, Native::start)      T2, T8
crates/comandos-server/src/dash/native/
  mod.rs          (NativeOptions nuevas, Native.{limits, pane_models, import}, rutas, Arc)          T2–T8
  lanes.rs        (UsageBackend::ROUTES, UsageImportBackend)                                         T3–T5, T8
  tmux.rs         (run_program_in con cwd)                                                           T6
  states/context.rs (guard y latency nativos)                                                        T5
  subrequest.rs   (se borra si no queda llamador)                                                    T5
  usage/mod.rs    (UsageRoute, ROUTES, answer)                                                       T3
  usage/limits.rs (OauthHttp, ReqwestOauth, LimitsCache, refresco)                                   T2
  usage/week.rs   (GET /analytics/week)                                                              T3
  usage/providers.rs (GET /providers, /optimization/plans, /accounts)                                T4
  usage/extensions.rs (GET /extension-usage)                                                         T5
  usage/guard.rs  (token_guard_with_forecast, contexto de latencia)                                  T5
  usage/state.rs  (motor de /usage/state: paneles vivos, entorno, memo, respuesta)                   T6
  usage/pane_models.rs (valores de borde, escritor, avisos de nivel, NotifyPost)                     T7
  usage/import.rs (ImportOwner: gracia, intervalo, flock, generación)                                T8
crates/comandos-server/tests/support/mod.rs    (FakeOauth, FakeNotify, seed_usage)                   T2, T7
crates/comandos-server/tests/dash_native_limits.rs, dash_native_week.rs, dash_native_providers.rs,
  dash_native_usage_guard.rs, dash_native_usage_state.rs, dash_native_pane_models.rs,
  dash_native_usage_import.rs                                                                        T2–T8
xtask/src/parity.rs                            (COMANDOS_DASH_USAGE_IMPORT_GRACE_MS=0 al frente)    T8
xtask/parity/frente.jsonl                                                                            T3–T8
docs/verification/cutover-dash.md              (sección «2e»)                                        T9
```

---

### Task 1: Lecturas de la base de uso y ensamblado puro (`usage_state`, `usage_read`)

Todo lo que las rutas de esta fase leen de `comandos-usage.sqlite`, más el ensamblado sin E/S de `build_usage_state`. Desplegable sola: nadie la llama aún; sus pruebas comparan con el Python.

Comportamiento portado (líneas de `bin/cc_usage.py`):

- **Conversiones** (`comandos_core::usage_state`): `as_int(&Value, i64) -> Result<i64, UsageError>` = `_as_int` (3126): `null`/`""` → defecto; entero → sí; float → truncado hacia cero; `NaN` → defecto (`ValueError`); `±inf` → `Err(UsageError::Overflow)` (el `OverflowError` no se captura); bool → 0/1; texto → `py::int`-equivalente (espacios, `_`, signo); otro → defecto. `as_float(&Value, f64) -> f64` = `_as_float` (3135). `text(&Value) -> String` = `_text` (3172): `null` → `""`, si no `str()` (enteros sin `.0`, floats con el `repr` de Python, bools `True`/`False`). `real_model(&Value) -> String` = `_real_model` (450). `as_epoch(&Value, &dyn LocalZone) -> Result<i64, UsageError>` = `_as_epoch` (3144) y `iso_epoch(&str, &dyn LocalZone) -> i64` = `_iso_epoch` (1928), con `fromisoformat_epoch` de **Python 3.10**: `YYYY-MM-DD` exacto, un separador cualquiera, `HH[:MM[:SS[.fff|.ffffff]]]`, zona `±HH:MM[:SS[.ffffff]]`; sin zona → hora local por `LocalZone::epoch_of` (ambigua → la primera, `fold=0`); cualquier otra forma → `None` (→ 0, el `except ValueError`). `as_epoch` de texto prueba antes `int(float(s))` (`"inf"` → `Err(Overflow)`).
- **`LocalZone`**: `epoch_of(NaiveDateTime) -> Option<i64>` y `day_start(i64) -> Option<i64>` (`datetime.fromtimestamp(ts).replace(hour=0, …).timestamp()`); impl para `chrono::Local` (producción) y `chrono_tz::Tz` (pruebas).
- **`normalize_pane_identity(raw: &Value, labels: &HashMap<String, String>, now: i64) -> Result<Map<String, Value>, UsageError>`** (469): claves en el orden del Python; `raw` = `json.dumps(raw, sort_keys=True)` (`comandos_core::json::dumps(raw, true, false)`); recortes por caracteres (`str[:n]`).
- **`build_state(now: i64, panes: Vec<Map>, turns: &[Map], provider_usage: &[Map], provider_costs: &[Map], settings: &Map) -> Result<Value, UsageError>`** (871-961 sin la E/S): `_attach_pane_turn_usage` (823, modelo dominante del día por tokens: `max(mt.items(), key=…)` devuelve el **primero** con el máximo en orden de inserción), `_project_rollups` (682, `setdefault` en orden de aparición, `round(…, 6)` por suma acumulada), `_usage_windows`/`_token_window` (795/759: `percent = min(100, round(used/limit*100, 1))` devuelve el **entero** `100` si el redondeo es ≥ 100 —`min` devuelve el primer argumento en empate— y el float si es menor), totales, `unattributed`, `providers`, `series`; claves de salida en el orden de 949-961. El llamador decide `panes` (vivos o `list_panes`).
- **`attach_token_counts(limits: &mut [Map], windows: &Value)`** (964): solo rellena claves ausentes o falsy y solo con valores truthy.
- **`credential_health(env: &Map<String, Value>) -> Value`** (`usage_credential_health`, `bin/cc-dash:216`).
- **`parse_env_text(&str) -> Vec<(String, String)>`** (`_parse_env_file` 2157): `comandos_core::text::splitlines`, `strip` de Python, `export `, `split("=", 1)`, comillas simétricas; claves con algún carácter no ASCII se descartan (nunca se consultan, así que no cambian la salida); último gana al convertir en mapa.
- **`percentile(&[f64], f64) -> Option<f64>`** (2751) y **`wilson_interval(i64, i64) -> (f64, f64)`** (2764), con las mismas operaciones de punto flotante en el mismo orden.

Lecturas (`comandos_store::usage_read`; todas reciben `&Connection` del carril y devuelven filas como `Map` en el orden de columnas, `REAL` como float de Python, `INTEGER` como entero, `NULL` → `null`; BLOB o texto no UTF-8 → `Err(ReadError::Undecodable)`, que la ruta convierte en 500):

| Función Rust | Python | SQL |
|---|---|---|
| `usage_settings(conn) -> Result<Vec<(String, Value)>, ReadError>` | `read_usage_settings` 543 | `select key, value from usage_settings` filtrado por `USAGE_LIMIT_KEYS` (21), orden de filas |
| `state_rows(conn, since: i64) -> Result<StateRows, ReadError>` | 876-886 | los tres `select` de `build_usage_state`, texto idéntico |
| `list_panes(conn)` | 535 | `select * from usage_panes order by git_root, tmux_session, tmux_pane` |
| `list_alerts(conn, limit: i64)` | 992 | `select * … order by created_at desc limit ?` |
| `recent_interactions(conn, limit: i64)` | 2713 | la consulta de 2717-2727 con `max(1, min(100, limit))` |
| `week_rows(conn, since: f64, wide: bool) -> Result<(Vec<Value>, Vec<Value>), ReadError>` | `analytics_week_payload` 6593-6603 | turnos `{provider, account, git_root, pane_pwd, started, finished, tokens}` (+`cost, model, session, agent` con `wide`, D9) y tramos |
| `quota_snapshots(conn, since: i64)` | 2036 | `{provider, account, window, scope, resets_at, percent}` |
| `measured_usage(conn, provider: &str, now: i64, day_start: i64) -> Result<Option<Value>, ReadError>` | `_measured_usage` 1935 | `COUNT/SUM/MAX`, 14 cubetas diarias; sin turnos en 7 d → `None` |
| `token_guard_report(conn, now: i64) -> Result<Value, ReadError>` | 2806 | proyectos por `git_root`, `subagentFiles` del `raw` (JSON inválido se salta), orden `(nivel, -tokensHour)` estable |
| `experiment_analytics(conn, days: i64, task_type: &str, now: f64) -> Result<Result<Value, String>, ReadError>` | 2975 + `_paired_experiment_analytics` 2919 | `days` acotado 1–30; `task_type` fuera de `TASK_TYPES` → `Err("invalid task type")` interior; `activeDays` por fecha UTC |
| `extension_usage(conn, session: &str, pane: &str, days: &str, now: f64) -> Result<Result<Value, String>, ReadError>` | `lib/session_profiles.py:449` | validaciones → `Err(texto)` interior (400); `int(days)` con `py::int_error_message`; la consulta con `limit 2000`; agrupación y orden `(-count, name)` estable |
| `record_pane(conn, pane: &Map) -> rusqlite::Result<()>` | 495 | el `insert … on conflict` de 506-531 |
| `record_quota_snapshots(conn, rows: &[Value], now: i64) -> rusqlite::Result<usize>` | 2012 | filtro, `int(round(resets_at / 3600) * 3600)` con `round` de Python (mitad al par), `upsert` de 2028-2032 |

`StateRows { turns: Vec<Map<String, Value>>, provider_usage: Vec<Map<String, Value>>, provider_costs: Vec<Map<String, Value>> }`.

**Files:**
- Create: `crates/comandos-core/src/usage_state.rs`; Modify: `crates/comandos-core/src/lib.rs`
- Create: `crates/comandos-store/src/usage_read.rs`; Modify: `crates/comandos-store/src/lib.rs`, `crates/comandos-store/src/usage.rs` (`stable_id`, `TURN_INSERT_SQL`, `as_int`, `as_float`, `py_text`, `cell` → `pub(crate)`)
- Create: `crates/comandos-core/tests/usage_state_oracle.rs`, `crates/comandos-store/tests/usage_read_oracle.rs`

**Interfaces:**
- Consumes: `comandos_core::{json::{response_dumps, dumps, truthy}, text::splitlines}`, `comandos_store::usage::{open_usage_db_at, ensure_schema}`.
- Produces: `comandos_core::usage_state::{UsageError { Overflow }, LocalZone, as_int, as_float, text, real_model, as_epoch, iso_epoch, fromisoformat_epoch(&str, &dyn LocalZone) -> Option<i64>, normalize_pane_identity, build_state, attach_token_counts, credential_health, parse_env_text, percentile, wilson_interval}`; `comandos_store::usage_read::{ReadError { Sql(rusqlite::Error), Undecodable }, StateRows, usage_settings, state_rows, list_panes, list_alerts, recent_interactions, week_rows, quota_snapshots, measured_usage, token_guard_report, experiment_analytics, extension_usage, record_pane, record_quota_snapshots}` con las firmas de la tabla.

- [ ] **Step 1: Prueba diferencial que falla**

Crear `crates/comandos-store/tests/usage_read_oracle.rs`:

```rust
//! Lecturas de la base de uso contra bin/cc_usage.py y lib/session_profiles.py.
use chrono::TimeZone;
use comandos_core::{json::response_dumps, usage_state::{self, LocalZone}};
use comandos_store::{usage, usage_read};
use rusqlite::params;
use serde_json::{Map, Value, json};
use std::{path::{Path, PathBuf}, process::Command};

const NOW: i64 = 1_791_115_200;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `python3 -c` con el repo y la base como argumentos; `None` sin python3.
fn run_python(script: &str, db: &Path) -> Option<String> {
    let out = Command::new("python3")
        .args(["-c", script])
        .arg(repo())
        .arg(db)
        .arg(NOW.to_string())
        .env("TZ", "America/Mexico_City")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env_remove("COMANDOS_USAGE_DB")
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

const ORACLE: &str = r#"
import json, os, sys
repo, db, now = sys.argv[1], sys.argv[2], int(sys.argv[3])
sys.path[:0] = [os.path.join(repo, "bin"), os.path.join(repo, "lib")]
import cc_usage, session_profiles
cc_usage.time.time = lambda: float(now)
panes = cc_usage.list_panes(db)
state = cc_usage.build_usage_state(db, [], now=now, settings={"COMANDOS_CLAUDE_DAILY_TOKEN_LIMIT": "900"})
print(json.dumps({
  "settings": cc_usage.read_usage_settings(db),
  "state": state,
  "alerts": cc_usage.list_alerts(db),
  "recent": cc_usage.recent_interactions(db, 1),
  "snapshots": cc_usage.quota_snapshots(db, since=now - 40 * 86400),
  "grok": cc_usage.grok_measured_usage(db, now),
  "guard": cc_usage.token_guard_report(db, now),
  "analytics": cc_usage.experiment_analytics(db, 7, ""),
  "ext": session_profiles.extension_usage(db, "s1", "%1", "7", now=float(now)),
}))
"#;

fn seed(db: &Path) {
    let conn = usage::open_usage_db_at(db).unwrap();
    usage::ensure_schema(&conn).unwrap();
    for (key, value) in [("COMANDOS_DAILY_BUDGET_USD", "5"), ("NO_ES_LIMITE", "1")] {
        conn.execute("insert into usage_settings(key,value) values(?,?)", params![key, value]).unwrap();
    }
    // Dos turnos con el mismo segundo (empate de orden, D12) y uno viejo (fuera de 14 d).
    let turns = [
        ("t1", "claude", "/r/a", "claude-fable-5", NOW - 60, 1200, 0.25),
        ("t2", "claude", "/r/a", "claude-sonnet-5", NOW - 60, 1200, 0.1),
        ("t3", "grok", "/r/b", "grok-4.5", NOW - 3600, 50, 0.0),
        ("t4", "codex", "/r/c", "gpt-5.6-sol", NOW - 20 * 86400, 7, 0.0),
    ];
    for (id, provider, root, model, at, tokens, cost) in turns {
        conn.execute(
            "insert into usage_turns(id,provider,agent,tmux_session,tmux_pane,pane_pwd,git_root,model,\
             turn_started_at,turn_finished_at,total_tokens,cost_usd,source,raw) \
             values(?,?,?,'s1','%1',?,?,?,?,?,?,?,'claude_jsonl',?)",
            params![id, provider, provider, root, root, model, at, at, tokens, cost,
                    r#"{"path": "/x/subagents/a.jsonl"}"#],
        ).unwrap();
    }
    conn.execute(
        "insert into usage_panes(tmux_session,tmux_pane,pane_pwd,git_root,agent,provider,started_at,last_seen_at,raw) \
         values('s1','%1','/r/a','/r/a','claude','claude',?,?,'{}')",
        params![NOW - 100, NOW - 5],
    ).unwrap();
    conn.execute(
        "insert into usage_quota_snapshots(limit_id,provider,account,win,scope,resets_at,percent,captured_at) \
         values('claude_weekly','claude','main','7d','',?,41.5,?)",
        params![NOW + 86400, NOW - 10],
    ).unwrap();
}

struct Mx(chrono_tz::Tz);
impl LocalZone for Mx {
    fn epoch_of(&self, naive: chrono::NaiveDateTime) -> Option<i64> {
        self.0.from_local_datetime(&naive).earliest().map(|d| d.timestamp())
    }
    fn day_start(&self, epoch: i64) -> Option<i64> {
        let local = self.0.timestamp_opt(epoch, 0).single()?;
        self.epoch_of(local.date_naive().and_hms_opt(0, 0, 0)?)
    }
}

#[test]
fn usage_reads_match_python_oracle() {
    let dir = std::env::temp_dir().join(format!("cmd-usage-read-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("comandos-usage.sqlite");
    seed(&db);
    let Some(expected) = run_python(ORACLE, &db) else { return };
    let conn = usage::open_usage_db_at(&db).unwrap();
    let zone = Mx(chrono_tz::America::Mexico_City);
    let rows = usage_read::state_rows(&conn, NOW - 14 * 86400).unwrap();
    let panes: Vec<Map<String, Value>> = usage_read::list_panes(&conn).unwrap();
    let mut settings = Map::new();
    settings.insert("COMANDOS_CLAUDE_DAILY_TOKEN_LIMIT".into(), json!("900"));
    let state = usage_state::build_state(NOW, panes, &rows.turns, &rows.provider_usage,
                                         &rows.provider_costs, &settings).unwrap();
    let day = zone.day_start(NOW).unwrap();
    let got = json!({
        "settings": Value::Object(usage_read::usage_settings(&conn).unwrap().into_iter().collect()),
        "state": state,
        "alerts": usage_read::list_alerts(&conn, 12).unwrap(),
        "recent": usage_read::recent_interactions(&conn, 1).unwrap(),
        "snapshots": usage_read::quota_snapshots(&conn, NOW - 40 * 86400).unwrap(),
        "grok": usage_read::measured_usage(&conn, "grok", NOW, day).unwrap(),
        "guard": usage_read::token_guard_report(&conn, NOW).unwrap(),
        "analytics": usage_read::experiment_analytics(&conn, 7, "", NOW as f64).unwrap().unwrap(),
        "ext": usage_read::extension_usage(&conn, "s1", "%1", "7", NOW as f64).unwrap().unwrap(),
    });
    assert_eq!(response_dumps(&got).unwrap(), expected.trim_end());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn extension_usage_validations_are_python_messages() {
    let dir = std::env::temp_dir().join(format!("cmd-usage-ext-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let conn = usage::open_usage_db_at(&dir.join("u.sqlite")).unwrap();
    usage::ensure_schema(&conn).unwrap();
    let err = |s: &str, p: &str, d: &str| usage_read::extension_usage(&conn, s, p, d, 0.0).unwrap().unwrap_err();
    assert_eq!(err("a b", "", "7"), "sesión inválida");
    assert_eq!(err("", "%1", "7"), "panel inválido");
    assert_eq!(err("s", "", "7.5"), "invalid literal for int() with base 10: '7.5'");
    let _ = std::fs::remove_dir_all(&dir);
}
```

Y `crates/comandos-core/tests/usage_state_oracle.rs` con los casos sueltos que la base no ejercita:

```rust
use comandos_core::usage_state::{self, LocalZone, UsageError};
use serde_json::json;

struct Utc;
impl LocalZone for Utc {
    fn epoch_of(&self, n: chrono::NaiveDateTime) -> Option<i64> { Some(n.and_utc().timestamp()) }
    fn day_start(&self, e: i64) -> Option<i64> { Some(e - e.rem_euclid(86400)) }
}

#[test]
fn python_conversions() {
    assert_eq!(usage_state::as_int(&json!(" 1_000 "), 0), Ok(1000));
    assert_eq!(usage_state::as_int(&json!("1.5"), 7), Ok(7));
    assert_eq!(usage_state::as_int(&json!(3.9), 0), Ok(3));
    assert_eq!(usage_state::as_epoch(&json!("inf"), &Utc), Err(UsageError::Overflow));
    // Python 3.10: sin `Z`, fracción de 3 o 6 dígitos, zona con dos puntos.
    assert_eq!(usage_state::fromisoformat_epoch("2026-10-04T10:00:00+00:00", &Utc), Some(1_791_108_000));
    assert_eq!(usage_state::fromisoformat_epoch("2026-10-04T10:00:00Z", &Utc), None);
    assert_eq!(usage_state::fromisoformat_epoch("2026-10-04 10:00:00.1234", &Utc), None);
    assert_eq!(usage_state::fromisoformat_epoch("2026-10-04T10:00:00.123", &Utc), Some(1_791_108_000));
    assert_eq!(usage_state::text(&json!(1.0)), "1.0");
    assert_eq!(usage_state::text(&json!(true)), "True");
}

#[test]
fn window_percent_is_int_100_when_rounding_reaches_100() {
    let turns = vec![json!({"provider": "claude", "total_tokens": 1000, "turn_finished_at": 10})
        .as_object().cloned().unwrap()];
    let mut settings = serde_json::Map::new();
    settings.insert("CLAUDE_DAILY_TOKEN_LIMIT".into(), json!("500"));
    let state = usage_state::build_state(20, vec![], &turns, &[], &[], &settings).unwrap();
    let item = &state["windows"]["items"][2];
    assert_eq!(comandos_core::json::response_dumps(&item["percent"]).unwrap(), "100");
}
```

- [ ] **Step 2: Ver el fallo**

Run: `$C test -p comandos-core --test usage_state_oracle && $C test -p comandos-store --test usage_read_oracle`
Expected: FAIL de compilación (`usage_state` y `usage_read` no existen).

- [ ] **Step 3: Implementar** `usage_state.rs` y `usage_read.rs` con las reglas de arriba; `pub mod usage_state;` y `pub mod usage_read;`. Cada función lleva un comentario de una línea con su cita (`/// `_token_window` (cc_usage.py:759).`). En `usage_read`, una sola función privada `row_object(&rusqlite::Row) -> Result<Map<String, Value>, ReadError>` convierte filas (nombres de `Row::as_ref().column_names()`, `ValueRef::Blob` → `Undecodable`, `ValueRef::Text` no UTF-8 → `Undecodable`, `Real` → `Value::from(f64)` que `response_dumps` imprime como `repr`). `token_guard_report` y `experiment_analytics` usan `percentile`/`wilson_interval` de `usage_state`.

- [ ] **Step 4: Verde**

Run: `$C test -p comandos-core --test usage_state_oracle && $C test -p comandos-store --test usage_read_oracle` → 4 PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/comandos-core/src/lib.rs crates/comandos-core/src/usage_state.rs \
  crates/comandos-core/tests/usage_state_oracle.rs crates/comandos-store/src/lib.rs \
  crates/comandos-store/src/usage.rs crates/comandos-store/src/usage_read.rs \
  crates/comandos-store/tests/usage_read_oracle.rs
git commit -m "feat(store): lecturas de la base de uso y ensamblado puro de build_usage_state

usage_settings, filas del estado, paneles, alertas, interacciones, semana, fotos de
cuota, consumo medido, guardia, analítica de experimentos y uso de extensiones con el
mismo SQL del Python; conversiones de Python 3.10 (int, float, fromisoformat).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Límites de proveedor — lectores locales, OAuth tras un rasgo y caché con refresco en tarea

Port de `_refresh_provider_limits` (1160), `usage_provider_limits` (1309), `_groq_limit_rows` (1279), `read_groq_rate_limits` (1261), `user_quotas` (540), `_claude_account_creds` (492) y de los lectores de `cc_usage.py`. Sin ruta todavía: desplegable sola (el refresco de arranque solo lee archivos locales y, si hay credenciales, consulta OAuth; el resultado aún no se usa).

Lectores (`comandos_runtime::limits`, síncronos, sin red; corren en `spawn_blocking`):

- `claude_account_creds(home: &Path) -> Vec<(String, PathBuf)>` (492): `main` = `~/.claude/.credentials.json` y cada `~/.claude-accounts/<n>/.credentials.json` existente (orden `sorted(os.listdir)`, sin `*.lock`).
- `oauth_token(path: &Path) -> Result<String, AbortRefresh>` (1103-1107): ausente, ilegible o JSON inválido (incluido no UTF-8) → `Ok("")`; JSON que no es objeto o `claudeAiOauth` que no es objeto ni null → `Err(AbortRefresh)` (el `AttributeError` del Python mata el hilo de refresco: la caché no cambia).
- `parse_claude_oauth_limits(payload: &Value, now: i64, zone: &dyn LocalZone) -> Vec<Value>` (1040): ids, etiquetas y ventanas exactas; slug con `str.isalnum` sobre ASCII y `lower()`; respaldo `five_hour`/`seven_day`.
- `read_codex_rate_limits(sessions_root: &Path, now: i64, max_files: usize) -> Vec<Value>` (1221) + `_last_codex_rate_limit_snapshot` (1199): cola de 256 KiB, `decode("utf-8", "replace")`, `splitlines` de Python al revés, primera línea con `"rate_limits"`; ventanas 300/10080; descarte por antigüedad `> wm*60*1.5`; orden final por `window`.
- `read_grok_credit_limits(homes: &[PathBuf], now: i64, zone: &dyn LocalZone) -> Option<Map<String, Value>>` (1842) + `_grok_billing_lines` (1825): cola de 512 KiB que crece ×8 hasta 64 MiB; `stale_period`.
- `grok_account_homes(home: &Path) -> Vec<(String, PathBuf)>` (`lib/grok_state.py:16`: `_home` + `~/.grok-accounts/*` con `resolve()`).
- `read_agy_quota(path: &Path, now: f64, zone: &dyn LocalZone) -> Vec<Value>` (1893).
- `read_groq_headers(path: &Path) -> Vec<Value>` (1261) + `parse_groq_ratelimit_headers(&Map, now: i64) -> Vec<Value>` (1126).
- `user_quotas(path: &Path) -> Map<String, Value>` (540).
- `grok_row(measured: Option<&Value>, official: Option<&Map>, quota: i64, now: i64) -> Option<Value>` (1199-1238, con el ajuste `stale_period` de 1203-1208) y `groq_rows(measured: Option<&Value>, headers: Vec<Value>, quota: i64, now: i64) -> Vec<Value>` (1279-1306).
- `EmailCache` + `email_for_dir(&mut EmailCache, dir: &Path, agent: &str) -> String` (3761): reutiliza el de `agent_procs` de la 2d; si allí es privado, se hace `pub` (máx. 64 entradas: al pasar se vacía entera, como `_ACCOUNT_EMAIL_CACHE` con su tope de 4096 reducido porque aquí solo entran cuentas).

Frente (`native/usage/limits.rs`): código completo abajo. `RefreshDeps` lleva lo que la tarea necesita sin `&Native` (D13).

**Files:**
- Create: `crates/comandos-runtime/src/limits.rs`; Modify: `crates/comandos-runtime/src/lib.rs`, `crates/comandos-runtime/src/agent_procs.rs` (solo si `EmailCache` es privado)
- Create: `crates/comandos-runtime/tests/limits_oracle.rs`
- Modify: `crates/comandos-server/Cargo.toml` (`reqwest = { version = "=0.13.5", default-features = false, features = ["rustls"] }`)
- Create: `crates/comandos-server/src/dash/native/usage/mod.rs` (solo `pub mod limits;` en esta tarea), `crates/comandos-server/src/dash/native/usage/limits.rs`
- Modify: `crates/comandos-server/src/dash/native/mod.rs` (`pub mod usage;`, `NativeOptions.{oauth, usage_effects, usage_env, zone}`, `Native.usage: Arc<Lane<UsageBackend>>`, `Native.limits: Arc<LimitsCache>`, `Native::start_background`), `crates/comandos-server/src/dash/mod.rs` (llamar a `start_background` tras crear el `Arc<Native>` en `serve_with`)
- Modify: `crates/comandos-server/tests/support/mod.rs` (`FakeOauth`, `seed_usage`)
- Create: `crates/comandos-server/tests/dash_native_limits.rs`

**Interfaces:**
- Consumes: `usage_read::{measured_usage, record_quota_snapshots}` (T1), `usage_state::LocalZone` (T1), `Lane<UsageBackend>` (2c).
- Produces: `comandos_runtime::limits::{AbortRefresh, claude_account_creds, oauth_token, parse_claude_oauth_limits, read_codex_rate_limits, read_grok_credit_limits, grok_account_homes, read_agy_quota, read_groq_headers, parse_groq_ratelimit_headers, user_quotas, grok_row, groq_rows, EmailCache, email_for_dir, CLAUDE_OAUTH_USAGE_URL}`; `native::usage::limits::{OauthHttp, HttpFuture, ReqwestOauth, LimitsCache, Limits { rows: Vec<Map<String, Value>>, health: Map<String, Value> }, RefreshDeps, LIMITS_TTL_S: i64 = 60}`; `LimitsCache::{default, get(self: &Arc<Self>, deps: &RefreshDeps) -> Limits, attach_tokens(&self, windows: &Value) -> Vec<Map<String, Value>>, refreshing(&self) -> bool}`; `Native::refresh_deps(&self) -> RefreshDeps`; `NativeOptions.{oauth: Arc<dyn OauthHttp>, usage_effects: bool, usage_env: Arc<BTreeMap<String, String>>, zone: Arc<dyn LocalZone + Send + Sync>}`; `support::{FakeOauth { calls(), set(…) }, seed_usage(&TestHome, &str)}`.

- [ ] **Step 1: Prueba diferencial de los lectores**

Crear `crates/comandos-runtime/tests/limits_oracle.rs`: un HOME temporal con `~/.codex/sessions/2026/10/04/rollout-a.jsonl` (dos líneas con `rate_limits`, una con `primary` de 300 min y otra con `secondary` de 10080 y `plan_type`), `~/.grok/logs/unified.jsonl` con 600 KiB de relleno y una línea `fetched credits config` al principio (obliga a crecer la cola), `H/agy-quota.json` con las tres cubetas (una vencida), `H/groq-ratelimit.json` con `x-ratelimit-reset-tokens: "1m30s"`, y un payload OAuth en `case.json`. El oráculo:

```rust
const ORACLE: &str = r#"
import json, os, sys
repo, home, now = sys.argv[1], sys.argv[2], int(sys.argv[3])
sys.path[:0] = [os.path.join(repo, "bin"), os.path.join(repo, "lib")]
import cc_usage
payload = json.load(open(os.path.join(home, "case.json")))
print(json.dumps({
  "oauth": cc_usage.parse_claude_oauth_limits(payload, now=now),
  "codex": cc_usage.read_codex_rate_limits(os.path.join(home, ".codex/sessions"), now=now),
  "grok": cc_usage.read_grok_credit_limits([os.path.join(home, ".grok")], now=now),
  "agy": cc_usage.read_agy_quota(os.path.join(home, ".claude/hooks/agy-quota.json"), now=float(now)),
  "groq": cc_usage.parse_groq_ratelimit_headers(json.load(open(os.path.join(home, ".claude/hooks/groq-ratelimit.json")))["headers"], now=now),
}))
"#;
```

El lado Rust llama a las funciones de `limits` con el mismo `now` y una zona `chrono_tz::America::Mexico_City` (la del `TZ` del subproceso) y compara `response_dumps` con la salida. Segunda prueba `oauth_token_shapes`: archivo ausente → `Ok("")`; `[]` → `Err(AbortRefresh)`; `{"claudeAiOauth": {"accessToken": 5}}` → `Ok("5")`.

- [ ] **Step 2: Ver el fallo**

Run: `$C test -p comandos-runtime --test limits_oracle`
Expected: FAIL de compilación (`limits` no existe).

- [ ] **Step 3: Implementar `comandos_runtime::limits`** con las reglas de arriba y `pub mod limits;`.

- [ ] **Step 4: Verde de los lectores**

Run: `$C test -p comandos-runtime --test limits_oracle` → 2 PASS.

- [ ] **Step 5: Pruebas de la caché que fallan**

Añadir a `crates/comandos-server/tests/support/mod.rs`:

```rust
/// OAuth falso: respuestas programadas por token; cuenta las llamadas.
#[derive(Default)]
pub struct FakeOauth {
    pub script: std::sync::Mutex<std::collections::HashMap<String, FakeAnswer>>,
    pub calls: std::sync::atomic::AtomicUsize,
}

#[derive(Clone)]
pub enum FakeAnswer {
    Json(serde_json::Value),
    Error(String),
    /// Nunca responde (la tarea de refresco queda colgada).
    Hang,
}

impl comandos_server::dash::native::usage::limits::OauthHttp for FakeOauth {
    fn get_json(&self, _url: &'static str, token: String, _timeout: std::time::Duration)
        -> comandos_server::dash::native::usage::limits::HttpFuture
    {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let answer = self.script.lock().unwrap().get(&token).cloned()
            .unwrap_or(FakeAnswer::Error("sin guion".into()));
        Box::pin(async move {
            match answer {
                FakeAnswer::Json(v) => Ok(v),
                FakeAnswer::Error(e) => Err(e),
                FakeAnswer::Hang => std::future::pending().await,
            }
        })
    }
}

pub fn seed_usage(home: &TestHome, sql: &str) {
    let conn = comandos_store::usage::open_usage_db_at(&home.usage_db()).unwrap();
    comandos_store::usage::ensure_schema(&conn).unwrap();
    conn.execute_batch(sql).unwrap();
}
```

y en `TestHome::options()`: `opts.oauth = Arc::new(FakeOauth::default());`, `opts.zone = Arc::new(chrono_tz::America::Mexico_City)`, `opts.usage_env = Arc::default()`.

Crear `crates/comandos-server/tests/dash_native_limits.rs`:

```rust
//! Caché de límites: TTL, un refresco en vuelo, 429 y red colgada.
mod support;
use comandos_server::dash::native::{Native, usage::limits::LimitsCache};
use serde_json::json;
use std::{sync::{Arc, atomic::Ordering}, time::Duration};
use support::{FakeAnswer, FakeOauth, TestHome};

fn creds(home: &TestHome, rel: &str, token: &str) {
    let path = home.root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, json!({"claudeAiOauth": {"accessToken": token}}).to_string()).unwrap();
}

fn payload(percent: f64) -> serde_json::Value {
    json!({"limits": [{"kind": "weekly_all", "percent": percent, "resets_at": "2026-10-06T00:00:00+00:00"}]})
}

async fn settle(cache: &LimitsCache) {
    for _ in 0..200 {
        if !cache.refreshing() { return; }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("el refresco no terminó");
}

#[tokio::test]
async fn limits_429_keeps_last_rows_and_backs_off() {
    let home = TestHome::new("limits-429");
    creds(&home, ".claude/.credentials.json", "tok-main");
    let oauth = Arc::new(FakeOauth::default());
    oauth.script.lock().unwrap().insert("tok-main".into(), FakeAnswer::Json(payload(41.0)));
    let clock = Arc::new(std::sync::atomic::AtomicI64::new(support::NOW_MS));
    let mut opts = home.options();
    opts.oauth = oauth.clone();
    let c2 = clock.clone();
    opts.clock = Arc::new(move || c2.load(Ordering::SeqCst));
    let native = Native::new(opts);
    let deps = native.refresh_deps();
    let cache = Arc::new(LimitsCache::default());
    assert!(cache.get(&deps).rows.is_empty()); // primera vez: vacío y lanza el refresco
    settle(&cache).await;
    assert_eq!(cache.get(&deps).rows[0]["percent"], json!(41.0));
    // 61 s después: 429; se conservan las filas de `main` con stale=true.
    oauth.script.lock().unwrap().insert("tok-main".into(), FakeAnswer::Error("HTTP Error 429: Too Many Requests".into()));
    clock.fetch_add(61_000, Ordering::SeqCst);
    cache.get(&deps);
    settle(&cache).await;
    let after = cache.get(&deps);
    assert_eq!(after.rows[0]["percent"], json!(41.0));
    assert_eq!(after.health["claude_oauth"]["stale"], json!(true));
    assert_eq!(after.health["claude_oauth"]["error"], json!("HTTP Error 429: Too Many Requests"));
    // Con error, el TTL es 180 s: a los 120 s no hay llamada nueva; a los 181 s sí.
    let calls = oauth.calls.load(Ordering::SeqCst);
    clock.fetch_add(120_000, Ordering::SeqCst);
    cache.get(&deps);
    assert_eq!(oauth.calls.load(Ordering::SeqCst), calls);
    clock.fetch_add(61_000, Ordering::SeqCst);
    cache.get(&deps);
    settle(&cache).await;
    assert_eq!(oauth.calls.load(Ordering::SeqCst), calls + 1);
}

#[tokio::test]
async fn limits_hung_network_never_blocks() {
    let home = TestHome::new("limits-hang");
    creds(&home, ".claude/.credentials.json", "tok-main");
    let oauth = Arc::new(FakeOauth::default());
    oauth.script.lock().unwrap().insert("tok-main".into(), FakeAnswer::Hang);
    let mut opts = home.options();
    opts.oauth = oauth.clone();
    let native = Native::new(opts);
    let deps = native.refresh_deps();
    let cache = Arc::new(LimitsCache::default());
    for _ in 0..20 {
        let started = std::time::Instant::now();
        assert!(cache.get(&deps).rows.is_empty());
        assert!(started.elapsed() < Duration::from_millis(50));
        tokio::task::yield_now().await;
    }
    assert_eq!(oauth.calls.load(Ordering::SeqCst), 1, "un solo refresco en vuelo");
    assert!(cache.refreshing());
}

#[tokio::test]
async fn attach_tokens_mutates_cached_rows_once() {
    let cache = LimitsCache::with_rows(vec![json!({"provider": "codex", "tokens_7d": 0}).as_object().cloned().unwrap()]);
    let windows = json!({"items": [{"metric": "tokens", "provider": "codex", "window": "weekly", "used": 70}]});
    assert_eq!(cache.attach_tokens(&windows)[0]["tokens_7d"], json!(70));
    let later = json!({"items": [{"metric": "tokens", "provider": "codex", "window": "weekly", "used": 90}]});
    // D5: el primer valor se queda hasta el siguiente refresco.
    assert_eq!(cache.attach_tokens(&later)[0]["tokens_7d"], json!(70));
}
```

(`LimitsCache::with_rows` es `#[doc(hidden)] pub` solo para pruebas.)

- [ ] **Step 6: Ver el fallo**

Run: `$C test -p comandos-server --test dash_native_limits`
Expected: FAIL de compilación (`usage::limits` no existe).

- [ ] **Step 7: Implementar `native/usage/limits.rs`**

```rust
//! Límites de proveedor: `_limits_cache` (bin/cc-dash:414-419),
//! `usage_provider_limits` (1309) y `_refresh_provider_limits` (1160).
//! La red y la lectura de archivos corren en una tarea; quien pide responde
//! con lo que hay (D3).
use super::super::{NativeOptions, lanes::{Lane, UsageBackend}};
use comandos_runtime::limits as lim;
use comandos_store::usage_read;
use futures_util::future::BoxFuture;
use serde_json::{Map, Value, json};
use std::{sync::{Arc, Mutex}, time::Duration};

pub const LIMITS_TTL_S: i64 = 60;
pub type HttpFuture = BoxFuture<'static, Result<Value, String>>;

/// GET JSON con `Authorization: Bearer <token>`, `anthropic-beta:
/// oauth-2025-04-20` y `Content-Type: application/json`; `Err` = `str(e)`.
pub trait OauthHttp: Send + Sync + 'static {
    fn get_json(&self, url: &'static str, token: String, timeout: Duration) -> HttpFuture;
}

pub struct Limits {
    pub rows: Vec<Map<String, Value>>,
    pub health: Map<String, Value>,
}

#[derive(Default)]
struct LimitsState {
    /// `int(time.time())` del último refresco completo; 0 = nunca.
    at: i64,
    rows: Vec<Map<String, Value>>,
    health: Map<String, Value>,
    refreshing: bool,
}

#[derive(Default)]
pub struct LimitsCache {
    state: Mutex<LimitsState>,
    emails: Mutex<lim::EmailCache>,
}

#[derive(Clone)]
pub struct RefreshDeps {
    pub opts: NativeOptions,
    pub usage: Arc<Lane<UsageBackend>>,
}

impl LimitsCache {
    #[doc(hidden)]
    pub fn with_rows(rows: Vec<Map<String, Value>>) -> Self {
        Self { state: Mutex::new(LimitsState { rows, ..LimitsState::default() }), ..Self::default() }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LimitsState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn refreshing(&self) -> bool {
        self.lock().refreshing
    }

    /// `usage_provider_limits(force=False)`: si venció y no hay refresco, lanza
    /// uno en una tarea; siempre devuelve la copia actual.
    pub fn get(self: &Arc<Self>, deps: &RefreshDeps) -> Limits {
        let now = (deps.opts.clock)() as f64 / 1000.0;
        let start = {
            let mut st = self.lock();
            let error = st.health.get("claude_oauth").and_then(|h| h.get("status"))
                == Some(&json!("error"));
            let ttl = if error { LIMITS_TTL_S * 3 } else { LIMITS_TTL_S };
            let stale = now - st.at as f64 > ttl as f64;
            let start = stale && !st.refreshing;
            if start {
                st.refreshing = true;
            }
            start
        };
        if start {
            tokio::spawn(refresh(self.clone(), deps.clone()));
        }
        let st = self.lock();
        Limits { rows: st.rows.clone(), health: st.health.clone() }
    }

    /// D5: `attach_token_counts` sobre las filas cacheadas, como el Python.
    pub fn attach_tokens(&self, windows: &Value) -> Vec<Map<String, Value>> {
        let mut st = self.lock();
        comandos_core::usage_state::attach_token_counts(&mut st.rows, windows);
        st.rows.clone()
    }
}

/// `finally: _limits_refreshing = False`, también si la tarea se cancela.
struct Reset(Arc<LimitsCache>);
impl Drop for Reset {
    fn drop(&mut self) {
        self.0.lock().refreshing = false;
    }
}

async fn refresh(cache: Arc<LimitsCache>, deps: RefreshDeps) {
    let _reset = Reset(cache.clone());
    let Ok((rows, snapshot, health)) = collect(&cache, &deps).await else {
        return; // el hilo del Python moría: la caché no cambia
    };
    let now = (deps.opts.clock)() / 1000;
    if deps.opts.usage_effects {
        // `try: record_quota_snapshots(...) except: pass`.
        let _ = deps.usage.with(move |u| usage_read::record_quota_snapshots(&u.conn, &snapshot, now)).await;
    }
    let mut st = cache.lock();
    st.at = now;
    st.rows = rows;
    st.health = health;
}

type Collected = (Vec<Map<String, Value>>, Vec<Value>, Map<String, Value>);

async fn collect(cache: &LimitsCache, deps: &RefreshDeps) -> Result<Collected, lim::AbortRefresh> {
    let o = &deps.opts;
    let home = o.hooks.join("../..");
    let now = o.clock.as_ref()() / 1000;
    let zone = o.zone.clone();
    let codex = blocking(move || lim::read_codex_rate_limits(&home.join(".codex/sessions"), now, 16)).await;
    let mut claude: Vec<Value> = Vec::new();
    let mut health = Map::new();
    let home = o.hooks.join("../..");
    for (alias, path) in blocking({ let h = home.clone(); move || lim::claude_account_creds(&h) }).await {
        let token = blocking({ let p = path.clone(); move || lim::oauth_token(&p) }).await?;
        let mut h = json!({"provider": "claude", "source": "oauth", "configured": false, "status": "missing"});
        let mut rows = Vec::new();
        if !token.is_empty() {
            h["configured"] = json!(true);
            match o.oauth.get_json(lim::CLAUDE_OAUTH_USAGE_URL, token, Duration::from_secs(8)).await {
                Ok(payload) => {
                    h["status"] = json!("ok");
                    h["last_success_at"] = json!(now);
                    rows = lim::parse_claude_oauth_limits(&payload, now, zone.as_ref());
                }
                Err(e) => {
                    h["status"] = json!("error");
                    h["error"] = json!(e.chars().take(240).collect::<String>());
                }
            }
        }
        let dir = path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
        let email = {
            let mut emails = cache.emails.lock().unwrap_or_else(|p| p.into_inner());
            lim::email_for_dir(&mut emails, &dir, "claude")
        };
        annotate_claude(&mut rows, &alias, &email);
        if rows.is_empty() && h["status"] == json!("error") {
            rows = cache.lock().rows.iter()
                .filter(|r| r.get("provider") == Some(&json!("claude"))
                    && account_of(r) == alias)
                .map(|r| Value::Object(r.clone()))
                .collect();
            h["stale"] = json!(!rows.is_empty());
        }
        h["account"] = json!(alias);
        let key = if alias == "main" { "claude_oauth".to_owned() } else { format!("claude_oauth:{alias}") };
        health.insert(key, h);
        claude.extend(rows);
    }
    let grok = grok_rows(deps, now).await;
    let groq = groq_rows(deps, now).await;
    let agy = blocking({
        let p = o.hooks.join("agy-quota.json");
        let z = zone.clone();
        let t = o.clock.as_ref()() as f64 / 1000.0;
        move || lim::read_agy_quota(&p, t, z.as_ref())
    }).await;
    let snapshot: Vec<Value> = claude.iter().chain(&codex).chain(&grok).chain(&agy).cloned().collect();
    let rows = snapshot.iter().chain(&groq)
        .filter_map(|v| v.as_object().cloned())
        .collect();
    Ok((rows, snapshot, health))
}
```

Completar en el mismo archivo: `blocking<T>(f) -> T` (= `spawn_blocking(f).await`; un pánico del lector se trata como el `except` del Python de ese bloque: lista vacía o `None`, y para `oauth_token` como `AbortRefresh`); `account_of(&Map) -> String` (`(r.get("account") or "main")`); `annotate_claude` (1168-1174: `account`, `accountEmail` si hay correo, prefijo `alias:` en `id` salvo `main`); `grok_rows` = `measured_usage(conn, "grok", now, zone.day_start(now))` por `deps.usage.with` (error SQL o carril que declina → `None`, el `except`), `read_grok_credit_limits(grok_account_homes(home))` en `spawn_blocking` y `lim::grok_row` con la cuota de `user_quotas(H/provider-quotas.json)["grok"]["tokens_7d"]`; `groq_rows` análogo con `read_groq_headers(H/groq-ratelimit.json)`.

`ReqwestOauth` (mismo archivo): `reqwest::Client` construido por petición con `http1_only()`, `connect_timeout(8 s)`, `read_timeout(8 s)`, `resolve_to_addrs(host, &addrs)` con las direcciones de `tokio::net::lookup_host((host, 443))`; errores mapeados por `python_error` según D4 (de `lookup_host`: el texto tras `failed to lookup address information: ` → código `EAI_*` por tabla `{"Temporary failure in name resolution": -3, "Name or service not known": -2, "No address associated with hostname": -5, "Non-recoverable failure in name resolution": -4}`; de `reqwest::Error`: `is_connect()` con `io::Error::raw_os_error()` → `<urlopen error [Errno N] texto>` con el texto de `strerror` sin el sufijo ` (os error N)`; `is_connect() && is_timeout()` → `<urlopen error timed out>`; `is_timeout()` → `The read operation timed out`; `status()` ≥ 400 → `HTTP Error {code}: {canonical_reason}`; lo demás → `e.to_string()`). El cuerpo: `bytes()` → UTF-8 estricto → `comandos_core::json::parse_slice`.

En `native/mod.rs`: los campos y `NativeOptions::for_home` (`oauth: Arc::new(ReqwestOauth)`, `usage_effects: true`, `usage_env` de `std::env::vars()` filtrado por las claves de D7, `zone: Arc::new(chrono::Local)`); `usage: Arc::new(Lane::new(…))`; `limits: Arc::new(LimitsCache::default())`; `pub fn refresh_deps(&self) -> RefreshDeps { RefreshDeps { opts: self.opts.clone(), usage: self.usage.clone() } }`; `pub fn start_background(&self) { if self.enabled() { let _ = self.limits.get(&self.refresh_deps()); } }` (el refresco de arranque de D3). En `dash/mod.rs`, `serve_with` llama a `native.start_background()` después de crear el `Arc<Native>`.

- [ ] **Step 8: Verde**

Run: `$C test -p comandos-server --test dash_native_limits` → 3 PASS. Run: `$C test -p comandos-server` → sin regresiones.

- [ ] **Step 9: Commit**

```bash
git add crates/comandos-runtime/src/lib.rs crates/comandos-runtime/src/limits.rs \
  crates/comandos-runtime/tests/limits_oracle.rs crates/comandos-server/Cargo.toml Cargo.lock \
  crates/comandos-server/src/dash/mod.rs crates/comandos-server/src/dash/native/mod.rs \
  crates/comandos-server/src/dash/native/usage/mod.rs crates/comandos-server/src/dash/native/usage/limits.rs \
  crates/comandos-server/tests/support/mod.rs crates/comandos-server/tests/dash_native_limits.rs
# solo si cambió: git add crates/comandos-runtime/src/agent_procs.rs
git commit -m "feat(dash): límites de proveedor con caché del frente y red en una tarea

Rollouts de Codex, log de Grok, cuota de agy, cabeceras de Groq y OAuth de Claude
detrás de OauthHttp; TTL 60/180 s, un refresco en vuelo, filas viejas con stale tras
un error y errores de red con el texto del urllib de Python.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: GET `/analytics/week` nativo

Port de `analytics_week_query` (6610) y `analytics_week_payload` (6589). Peso: 60 s por tablero abierto (panel y barra lateral). Primera ruta del dominio; abre `native/usage/mod.rs` con su tabla.

Comportamiento:

1. `Query::parse(target)`; `offset` = primer valor (`parse_qs` sin blancos) o `"0"`; `py::int` → error → 400 `{"error": "offset inválido"}`; fuera de `{0, -1}` → 400 `{"error": "solo esta semana (0) o la anterior (-1)"}`.
2. `demo` = primer valor con todo lo que no sea `[a-z]` quitado (ASCII; `re.sub(r"[^a-z]", "", …)`). No vacío → sin `repo_root` declina; `<repo_root>/tests/fixtures/analytics/week-<demo>[-prev].json` (`-prev` si `offset != 0`) leído con `files::read_json_strict`: `Missing`/`Unreadable` (el `OSError`) → 404 `{"error": "no hay datos de ejemplo con ese nombre"}`; `Unsure` → declina; `Value` → 200 con el valor (`read_reply`). JSON inválido con certeza → 500 (el `json.load` no capturado): `files::read_json_strict` lo clasifica `Unreadable`; distinguirlo de `OSError` con `files::Strict::Invalid` si la 2c lo separa; si no, se declina (nunca se responde 404 por un JSON roto).
3. `sidebar` (D9): `query.first("sidebar") == Some("1")` con el Python que lo soporta; si no lo soporta y aparece la clave, declina.
4. Payload: `now = clock()/1000` (float); `since = now - 17*86400`; carril de uso `week_rows(conn, since, wide=sidebar)` y `quota_snapshots(conn, (now - 40*86400) as i64)` (truncado como `int(since)`) en **un** trabajo; app-state: política de Pomodoro una vez (como `/pomodoro` de la 2c) y `pomodoro::records(conn, Some((since*1000.0) as i64), None, None)` (`int(since * 1000)` trunca); `limits = native.limits.get(&deps).rows`; `analytics_week::build_week(&WeekInput { now, offset, limits, turns, spans, snapshots, records, tz_name: analytics_week::TZ })`; con `sidebar`, `accounts = analytics_week::sidebar_accounts(&accounts, &limits, &turns, now)`. Error de `build_week` (TypeError/ValueError del port) → 500.
5. El orden de efectos: los dos trabajos de base son lecturas; `limits.get` lanza el refresco (efecto) después de las lecturas y antes de responder. Carril de uso apagado → declina (D10).

**Files:**
- Modify: `crates/comandos-server/src/dash/native/usage/mod.rs` (`UsageRoute`, `ROUTES`, `answer`), `crates/comandos-server/src/dash/native/mod.rs` (`NativeRoute::Usage(usage::UsageRoute)`, `usage::ROUTES` en `TABLES` antes de `retired::ROUTES`), `crates/comandos-server/src/dash/native/lanes.rs` (`UsageBackend::ROUTES = "GET /pomodoro, GET /sovereignty, GET /state y GET /analytics/week"`)
- Create: `crates/comandos-server/src/dash/native/usage/week.rs`
- Create: `crates/comandos-server/tests/dash_native_week.rs`
- Modify: `xtask/parity/frente.jsonl`

**Interfaces:**
- Consumes: `usage_read::{week_rows, quota_snapshots}` (T1), `LimitsCache::get` (T2), `comandos_core::analytics_week::{build_week, sidebar_accounts, WeekInput, TZ}`, `comandos_store::pomodoro::records`, `Native::with_state` (2b), `StateBackend.pomodoro_policy` (2c).
- Produces: `usage::{UsageRoute { Week }, ROUTES, answer(&Native, UsageRoute, &Request) -> Answer}` (las tareas 4–8 añaden variantes), `week::answer(&Native, &Request) -> Answer`.

- [ ] **Step 1: Prueba contra el oráculo que falla**

Crear `crates/comandos-server/tests/dash_native_week.rs`:

```rust
//! GET /analytics/week: nativa, byte a byte con el cc-dash del repo sobre el mismo HOME.
mod support;
use comandos_server::dash::native::wall_clock_ms;
use std::sync::Arc;
use support::{TestHome, dead_port, front, get, oracle::oracle, seed_usage};

fn masked(text: &str) -> String {
    // `measuredAt` y `now` dependen del minuto; `captured_at` de las filas medidas, del segundo.
    let v: serde_json::Value = serde_json::from_str(text).unwrap();
    let mut v = v;
    if let Some(week) = v.get_mut("week") {
        week["measuredAt"] = serde_json::json!("<hora>");
        week["now"] = serde_json::json!(0);
    }
    comandos_core::json::response_dumps(&v).unwrap()
}

#[tokio::test]
async fn analytics_week_matches_python() {
    let home = TestHome::new("week");
    let now = wall_clock_ms() / 1000;
    seed_usage(&home, &format!(
        "insert into usage_turns(id,provider,agent,tmux_session,tmux_pane,pane_pwd,git_root,model,harness_account,\
         turn_started_at,turn_finished_at,total_tokens,cost_usd,source,raw) values\
         ('a','claude','claude','s','%1','/r/x','/r/x','claude-fable-5','main',{s},{e},900,0.5,'claude_jsonl','{{}}'),\
         ('b','grok','grok','s','%2','/r/y','/r/y','grok-4.5','unknown',{s2},{e2},60,0,'grok_updates','{{}}');",
        s = now - 7200, e = now - 7000, s2 = now - 90_000, e2 = now - 89_000));
    let Some(py) = oracle(&home).await else { return };
    let mut opts = home.options();
    opts.clock = Arc::new(wall_clock_ms);
    let front = front(&home, dead_port(), opts).await;
    for target in ["/analytics/week", "/analytics/week?offset=-1", "/analytics/week?offset=x",
                   "/analytics/week?offset=2", "/analytics/week?demo=Base1", "/analytics/week?demo=nada"] {
        // Calentar la caché de límites en los dos (refresco de arranque).
        let _ = get(front.port, target).await;
        let _ = get(py.port, target).await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let a = get(front.port, target).await;
        let b = get(py.port, target).await;
        assert_eq!(a.status, b.status, "{target}");
        if a.status == 200 {
            assert_eq!(masked(&a.text()), masked(&b.text()), "{target}");
        } else {
            assert_eq!(a.text(), b.text(), "{target}");
        }
    }
    front.stop().await;
}

#[tokio::test]
async fn analytics_week_prefix_is_still_forwarded() {
    let home = TestHome::new("week-prefix");
    let legacy = support::FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let wire = get(front.port, "/analytics/weekly").await;
    assert_eq!(wire.text(), r#"{"legacy": true}"#);
    front.stop().await;
}
```

(`demo=Base1` se limpia a `base`; si `tests/fixtures/analytics/week-base.json` no existe en el repo, usar el nombre que exista: `ls tests/fixtures/analytics/`.)

- [ ] **Step 2: Ver el fallo**

Run: `$C test -p comandos-server --test dash_native_week`
Expected: FAIL (`analytics_week_matches_python`: el frente no tiene la ruta y `dead_port` responde 502).

- [ ] **Step 3: Implementar** `usage/week.rs` con los pasos 1–5, `usage/mod.rs`:

```rust
//! Dominio de uso, cuotas y analítica (Fase 2e).
pub mod limits;
pub mod week;

use super::{Answer, Entry, Key, Native, NativeRoute, Verb};
use crate::Request;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageRoute {
    Week,
}

pub const ROUTES: &[Entry] = &[Entry {
    verb: Verb::Get,
    key: Key::Path("/analytics/week"),
    route: NativeRoute::Usage(UsageRoute::Week),
}];

pub async fn answer(native: &Native, route: UsageRoute, request: &Request) -> Answer {
    match route {
        UsageRoute::Week => week::answer(native, request).await,
    }
}
```

y la rama `NativeRoute::Usage(route) => usage::answer(self, route, request).await` en `Native::answer`.

- [ ] **Step 4: Fixture de paridad**

En `xtask/parity/frente.jsonl`, tras el bloque `# --- 2c · F3`:

```json
# --- 2e · uso y analítica (nativas; leen la base de uso)
{"name":"u-week-calentar","method":"GET","path":"/analytics/week","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"status-only"}
{"name":"u-week","method":"GET","path":"/analytics/week","headers":{"Host":"127.0.0.1"},"body":null,"volatile":["/week/now","/week/measuredAt","/accounts/*/week/captured_at"],"expect":"same"}
{"name":"u-week-prev","method":"GET","path":"/analytics/week?offset=-1","headers":{"Host":"127.0.0.1"},"body":null,"volatile":["/week/measuredAt"],"expect":"same"}
{"name":"u-week-offset-malo","method":"GET","path":"/analytics/week?offset=x","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"u-week-offset-fuera","method":"GET","path":"/analytics/week?offset=3","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"u-week-demo-nada","method":"GET","path":"/analytics/week?demo=zz9","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
```

(Los punteros `volatile` se ajustan a los campos de tiempo que de verdad lleve la salida de `build_week`: comprobarlo con `curl` contra el heredado y anotar en el commit los que se añadan.)

- [ ] **Step 5: Verde**

Run: `$C test -p comandos-server --test dash_native_week` → 2 PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/comandos-server/src/dash/native/mod.rs crates/comandos-server/src/dash/native/lanes.rs \
  crates/comandos-server/src/dash/native/usage/mod.rs crates/comandos-server/src/dash/native/usage/week.rs \
  crates/comandos-server/tests/dash_native_week.rs
git add -f xtask/parity/frente.jsonl
git commit -m "feat(dash): GET /analytics/week nativo

Semana de Analytics con turnos, tramos, fotos de cuota, registros de Pomodoro y la
caché de límites del frente; datos de ejemplo del mockup y errores del Python.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: GET `/providers`, `/optimization/plans` y `/accounts`

Port de `provider_public_state` (1722), `provider_runtime_facts` (1610), `capability_matrix` (1638), `session_change_support` (2628), `optimization_plans` (1463) y la rama `/accounts` (8266). Se cargan al abrir el tablero y al pulsar «Cuenta»; poco peso, riesgo medio (registro).

Extensiones de `comandos_runtime::providers` (la 2d dio `RegistryCache`, `runtime_facts`, `selectable_routes`, `which`, `proxy_port`):

- `public_state(registry: &Value, which: &dyn Fn(&str) -> bool, home: &Path) -> Value` (`lib/providers.py:287`): copia profunda; `available` (binario nulo o `which`), `authenticated` (con `authFile`: `defaultHome` expandido no vacío y archivo regular; sin `authFile`: `null`); quitar `authFile`, `defaultHome`, `accountsRoot`, `accountEnv` y el `authSource` de `claudeEngines`.
- `evaluate_capability_matrix(registry: &Value, facts: &Value) -> Vec<Value>` (219): las celdas completas (la 2d solo devuelve los ids seleccionables; `selectable_routes` pasa a ser `evaluate_capability_matrix(...).filter(selectable).id`), exclusiones, `not_routed` y el orden final `(matrixHarnesses, motors)` estable con 99 para lo desconocido.
- `session_change_support(registry: &Value) -> Value` (`bin/cc-dash:2628`, sin `observed`).
- `model_key(&str) -> String` (`_model_key`), `model_spec(registry, owner, model_id, section) -> Option<Value>` (con el respaldo de familia: el `max` por la tupla de enteros devuelve el primero de los máximos), `model_spec_for_route`, `validate_selection(registry, matrix, selection, scope) -> Result<Value, String>` (código de error como texto; el Python lanza `ProviderRegistryError(code)`).
- `grok_models(home: &Path) -> Vec<Value>` (`lib/grok_state.py:47`, `GROK_HOME` = `NativeOptions.grok_home` o `~/.grok`, orden `id` descendente).

Frente (`usage/providers.rs`):

- **`public_state(native) -> Result<Value, Fault>`**: `RegistryCache::load` (incierto → `Fault::Decline`; el `Exception` del Python con mensaje desconocido no se puede reproducir), `which` con `NativeOptions.search_path` en `spawn_blocking`, `accounts::public_accounts` (2c/2d), el bucle de 1726-1731 (`accounts` con `motorSelectable` y `authenticated`), `grok.models = grok_models() or grok.models or []`, `matrix = evaluate_capability_matrix(registry, provider_runtime_facts(public))` con `gateway.installed = which("cc-model-proxy")` y `gateway.alive = proxy_alive()` (TCP a `127.0.0.1:proxy_port(repo_root)` con plazo 300 ms, async), `midSessionRoutes`, `matrixSummary` (`sum(bool)` y `sum(not)`). Claves en el orden de inserción del Python.
- **GET `/providers`**: 200 con `public_state`. Un `validate_registry` que falla con mensaje conocido → 500 `{"error": "providers.json invalido: <mensaje>"}`.
- **GET `/optimization/plans`**: `config/optimization-plans.json` (`files::read_json_strict`; `Missing`/`Unreadable` → `{"plans": []}`; `Unsure` → declina), `matrix` como arriba; por plan `{id, label, icon, intent}` con `plan.get` (claves ausentes → `null`), `variants` por harness: `validate_selection(…, "session_model")` y la celda; cualquier error → `{**selection, "selectable": false, "reason": "configuración no disponible"}`; `selection` que no es objeto → el `**` del Python falla dentro del `try` → mismo respaldo pero `{**selection}` también fallaría: declina. `active` = `H/optimization-default.json` `profile` o `""`.
- **GET `/accounts?harness=`**: `harness` = primer valor o `"claude"`; `spec = public_state.harnesses[harness]` (ausente → `{}`); sin `capabilities.accounts` truthy → 404 `{"error": "Ese CLI no maneja cuentas"}`; si no, `{"harness", "accounts": accounts::account_menu(spec.accounts or [], limits.rows, harness)}` con `limits = native.limits.get(&deps)` (lanza el refresco si venció). Carril de uso apagado → declina (D10). Si al empezar la tarea `bin/cc-dash` de `main` ya usa `load_provider_registry()` en vez de `provider_public_state()` en esta rama (cambio del árbol de trabajo del checkout principal), se sigue el código fusionado.

**Files:**
- Modify: `crates/comandos-runtime/src/providers.rs`; Create: `crates/comandos-runtime/tests/providers_public_oracle.rs`
- Create: `crates/comandos-server/src/dash/native/usage/providers.rs`
- Modify: `crates/comandos-server/src/dash/native/usage/mod.rs` (`Providers`, `OptimizationPlans`, `Accounts`), `crates/comandos-server/src/dash/native/lanes.rs` (`ROUTES`: `, GET /accounts` antes de `GET /extension-usage`, que ya está desde la 5b; tras el rebase el texto es `"GET /pomodoro, GET /sovereignty, GET /state, GET /analytics/week, GET /accounts y GET /extension-usage"`)
- Create: `crates/comandos-server/tests/dash_native_providers.rs`
- Modify: `xtask/parity/frente.jsonl`

**Interfaces:**
- Consumes: `providers::{RegistryCache, which, proxy_port}` (2d), `accounts::{public_accounts, account_menu}`, `LimitsCache::get` (T2).
- Produces: `providers::{public_state, evaluate_capability_matrix, session_change_support, model_key, model_spec, model_spec_for_route, validate_selection, grok_models}`; `usage::providers::{public_state(&Native) -> Result<Value, Fault>, capability_matrix(&Native) -> Result<Vec<Value>, Fault>, answer_providers, answer_plans, answer_accounts}`; `UsageRoute::{Providers, OptimizationPlans, Accounts}`.

- [ ] **Step 1: Prueba de capa que falla**

`crates/comandos-runtime/tests/providers_public_oracle.rs`: el registro real de `config/providers.json` hidratado con `model_catalog` (como en la 2d), `which` falso (`claude`, `codex`), HOME temporal con `~/.codex/auth.json`; el oráculo carga `bin/cc-dash` con `SourceFileLoader` (como `providers_oracle.rs` de la 2d), sustituye `provider_registry.which`, `account_registry.public_accounts`, `proxy_alive`, `shutil.which` y `grok_state.models`, e imprime `provider_public_state()` y `session_change_support(reg)`; además tres `validate_selection` (válida, `model_unavailable`, `effort_unavailable`) y `model_spec(reg, "claude", "fable", section="motors")`. El lado Rust compone lo mismo con las funciones nuevas y compara `response_dumps`.

- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-runtime --test providers_public_oracle` → FAIL de compilación.

- [ ] **Step 3: Implementar** las funciones de `providers` y reescribir `selectable_routes` sobre `evaluate_capability_matrix` (las pruebas de la 2d siguen en verde).

- [ ] **Step 4: Verde de capa** — Run: `$C test -p comandos-runtime --test providers_public_oracle --test providers_oracle` → PASS.

- [ ] **Step 5: Prueba de rutas que falla**

`crates/comandos-server/tests/dash_native_providers.rs`:

```rust
//! /providers, /optimization/plans y /accounts contra el cc-dash del repo.
mod support;
use comandos_server::dash::native::wall_clock_ms;
use std::sync::Arc;
use support::{TestHome, dead_port, front, get, oracle::oracle};

#[tokio::test]
async fn provider_routes_match_python() {
    let home = TestHome::new("providers");
    let accounts = home.root.join(".claude-accounts/relotto");
    std::fs::create_dir_all(&accounts).unwrap();
    std::fs::write(accounts.join(".credentials.json"), "{}").unwrap();
    std::fs::write(home.hooks().join("optimization-default.json"), r#"{"profile": "ahorro"}"#).unwrap();
    let Some(py) = oracle(&home).await else { return };
    let mut opts = home.options();
    opts.clock = Arc::new(wall_clock_ms);
    // El mismo PATH que el oráculo (con su fakebin delante): `which` debe coincidir.
    opts.search_path = Some(format!("{}:{}", home.root.join("fakebin").display(),
                                    std::env::var("PATH").unwrap_or_default()).into());
    let front = front(&home, dead_port(), opts).await;
    for target in ["/providers", "/optimization/plans", "/accounts", "/accounts?harness=codex",
                   "/accounts?harness=shell", "/accounts?harness="] {
        let _ = get(front.port, target).await;
        let a = get(front.port, target).await;
        let b = get(py.port, target).await;
        assert_eq!((a.status, a.text()), (b.status, b.text()), "{target}");
    }
    front.stop().await;
}

#[tokio::test]
async fn invalid_registry_declines() {
    let home = TestHome::new("providers-bad");
    let repo = home.root.join("repo");
    std::fs::create_dir_all(repo.join("config")).unwrap();
    std::fs::write(repo.join("config/providers.json"), "{roto").unwrap();
    let legacy = support::FakeLegacy::start().await;
    let mut opts = home.options();
    opts.repo_root = Some(repo);
    let front = front(&home, legacy.port, opts).await;
    assert_eq!(get(front.port, "/providers").await.text(), r#"{"legacy": true}"#);
    front.stop().await;
}
```

- [ ] **Step 6: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_providers` → FAIL (502 del puerto muerto).

- [ ] **Step 7: Implementar** `usage/providers.rs`, las variantes y entradas `Key::Path("/providers")`, `Key::Path("/optimization/plans")`, `Key::Path("/accounts")`.

- [ ] **Step 8: Fixture**

```json
{"name":"u-providers","method":"GET","path":"/providers","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"u-optimization-plans","method":"GET","path":"/optimization/plans","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"u-accounts","method":"GET","path":"/accounts","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
{"name":"u-accounts-shell","method":"GET","path":"/accounts?harness=shell","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
```

- [ ] **Step 9: Verde** — Run: `$C test -p comandos-server --test dash_native_providers` → 2 PASS.

- [ ] **Step 10: Commit**

```bash
git add crates/comandos-runtime/src/providers.rs crates/comandos-runtime/tests/providers_public_oracle.rs \
  crates/comandos-server/src/dash/native/lanes.rs crates/comandos-server/src/dash/native/usage/mod.rs \
  crates/comandos-server/src/dash/native/usage/providers.rs crates/comandos-server/tests/dash_native_providers.rs
git add -f xtask/parity/frente.jsonl
git commit -m "feat(dash): GET /providers, /optimization/plans y /accounts nativos

Estado público del registro, matriz de capacidades completa, rutas a mitad de sesión,
planes de optimización y el menú de cuentas con los límites de la caché del frente.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Contexto de sugerencias sin el heredado y GET `/extension-usage`

Dos cambios pequeños con el mismo carril. (a) `states/context.rs` de la 2d deja de pedir `/usage/guard` y `/usage/analytics?days=7` al heredado: los calcula el frente. (b) `/extension-usage` nativa.

(a) **Guardia y latencia** (`usage/guard.rs`):

```rust
//! `token_guard_with_forecast` (bin/cc-dash:1429) y la latencia de
//! `_suggestion_context` (6953-6973) sin el heredado.
use super::super::{Fault, Native};
use comandos_store::usage_read;
use serde_json::{Map, Value, json};

/// Guardia con pronóstico. `Ok(None)`: el `except Exception` del Python (`{}`).
pub async fn token_guard_with_forecast(native: &Native) -> Result<Option<Value>, Fault> {
    let now_ms = (native.options().clock)();
    let report = native
        .usage
        .with(move |u| usage_read::token_guard_report(&u.conn, now_ms / 1000))
        .await?;
    let Ok(Value::Object(mut report)) = report else {
        return Ok(None);
    };
    let limits = native.limits.get(&native.refresh_deps()).rows;
    let now = now_ms as f64 / 1000.0;
    let Some(forecasts) = forecasts(&limits, now) else {
        return Ok(None); // `float()` imposible: excepción → {}
    };
    let level = if forecasts.iter().any(|f| f["level"] == json!("critical")) {
        "critical"
    } else if forecasts.iter().any(|f| f["level"] == json!("warning")) {
        "warning"
    } else {
        "normal"
    };
    report.insert("forecasts".into(), Value::Array(forecasts));
    report.insert("forecastLevel".into(), json!(level));
    Ok(Some(Value::Object(report)))
}
```

`forecasts(&[Map<String, Value>], f64) -> Option<Vec<Value>>` replica 1433-1451 literalmente: filtro `provider == "claude"`, `(account or "main") == "main"`, `window == "7d"`; `percent = float(percent or 0)`, `reset = float(resets_at or 0)` (`None` si `float()` fallaría: texto no numérico, contenedor); `rate`, `exhaustion` (`10 * 86400` sin ritmo), `downtime`, `scope = str(scope or "General")`; `downtimeHours = round(downtime/3600, 1)`; mensaje con `round(downtime/3600)` (entero de Python, mitad al par). La latencia: `usage_read::experiment_analytics(&conn, 7, "", now)` por el carril; `Ok(Ok(stats))` → `suggest::latency_from(&stats)` (2d); error SQL o interior → vacío (el `except`).

En `states/context.rs`: sustituir los dos `subrequest::get` por `guard::token_guard_with_forecast(native)` (`Ok(Some(v))` → `v`; `Ok(None)` → `{}`; `Err(Fault::Decline)` → `Err(StateFault::Decline)` sin cachear, como el heredado caído de la 2d) y por la latencia nativa (declinar del carril → `Err(StateFault::Decline)`). La firma de `Context::get` cambia de `opts: &NativeOptions` a `native: &Native` (el único llamador es `gather::compute`). Si `subrequest` no tiene más llamadores (`grep -rn 'subrequest::' crates/`), se borra `subrequest.rs`, su `pub mod` y sus pruebas; `NativeOptions.{legacy, legacy_token}` se quedan si otro código los usa.

(b) **GET `/extension-usage`** (`usage/extensions.rs`): `session`, `pane`, `days` = primer valor o `""`, `""`, `"7"` (`parse_qs`); si `H/comandos-usage.sqlite` no es un archivo regular → el resultado vacío de `lib/session_profiles.py:455-460` **tras** las validaciones (las validaciones van antes de mirar el archivo, como en el Python: `extension_usage` valida y luego mira `is_file`), sin tocar el carril (D8); si existe → `usage_read::extension_usage(&conn, session, pane, days, now)` por el carril; `Err(msg)` interior → 400 `{"error": msg}`; `ReadError` → 500. Para no duplicar las validaciones, `usage_read` expone `extension_usage_check(session, pane, days) -> Result<i64, String>` y `extension_usage_empty(session, pane, days: i64) -> Value`, que `extension_usage` usa por dentro.

**Files:**
- Create: `crates/comandos-server/src/dash/native/usage/guard.rs`, `crates/comandos-server/src/dash/native/usage/extensions.rs`
- Modify: `crates/comandos-server/src/dash/native/states/context.rs`, `crates/comandos-server/src/dash/native/states/gather.rs` (llamada a `Context::get`), `crates/comandos-server/src/dash/native/usage/mod.rs` (`Extensions`), `crates/comandos-server/src/dash/native/lanes.rs` (`ROUTES` ya nombra `GET /extension-usage` desde la 5b; no cambia), `crates/comandos-store/src/usage_read.rs` (`extension_usage_check`, `extension_usage_empty`)
- Delete (si queda sin llamador): `crates/comandos-server/src/dash/native/subrequest.rs`
- Create: `crates/comandos-server/tests/dash_native_usage_guard.rs`; Modify: la prueba `suggestion_context_legacy_500_is_empty_and_down_declines` de `crates/comandos-server/tests/dash_native_state.rs` (2d) pasa a `suggestion_context_is_native`
- Modify: `xtask/parity/frente.jsonl`

**Interfaces:**
- Consumes: `usage_read::{token_guard_report, experiment_analytics, extension_usage}` (T1), `LimitsCache::get` (T2), `suggest::latency_from`, `states::context::Context` (2d).
- Produces: `usage::guard::{token_guard_with_forecast(&Native) -> Result<Option<Value>, Fault>, latency(&Native) -> Result<Vec<((Value, Value), (Value, Value))>, Fault>, forecasts(&[Map<String, Value>], f64) -> Option<Vec<Value>>}`; `Context::get(&self, native: &Native, registry: &Value, now_ms: i64) -> Result<Arc<SuggestContext>, StateFault>`; `usage_read::{extension_usage_check, extension_usage_empty}`; `UsageRoute::Extensions`.

- [ ] **Step 1: Pruebas que fallan**

`crates/comandos-server/tests/dash_native_usage_guard.rs`:

```rust
//! Guardia con pronóstico contra bin/cc-dash, y /extension-usage contra el oráculo.
mod support;
use comandos_server::dash::native::{Native, usage::guard, wall_clock_ms};
use serde_json::json;
use std::sync::Arc;
use support::{TestHome, dead_port, front, get, oracle::oracle, seed_usage};

#[test]
fn forecasts_match_python_rules() {
    let now = 1_791_115_200.0;
    let rows = vec![
        json!({"provider": "claude", "window": "7d", "percent": 80, "resets_at": now + 86400.0, "scope": "Fable"}),
        json!({"provider": "claude", "account": "relotto", "window": "7d", "percent": 99}),
        json!({"provider": "claude", "window": "5h", "percent": 50}),
    ].into_iter().map(|v| v.as_object().cloned().unwrap()).collect::<Vec<_>>();
    let got = guard::forecasts(&rows, now).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0]["level"], json!("normal"));
    assert_eq!(got[0]["message"], json!("Fable llegaría al reset con margen."));
    let bad = vec![json!({"provider": "claude", "window": "7d", "percent": "x"}).as_object().cloned().unwrap()];
    assert!(guard::forecasts(&bad, now).is_none());
}

#[tokio::test]
async fn guard_is_native_and_lane_down_declines() {
    let home = TestHome::new("guard");
    seed_usage(&home, "pragma user_version = 12;");
    let native = Native::new(home.options());
    assert!(matches!(guard::token_guard_with_forecast(&native).await,
                     Err(comandos_server::dash::native::Fault::Decline)));
}

#[tokio::test]
async fn extension_usage_matches_python_and_never_creates_the_db() {
    let home = TestHome::new("ext");
    let legacy = support::FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let wire = get(front.port, "/extension-usage?session=s1&pane=%251").await;
    assert_eq!(wire.status, 200);
    assert!(!home.usage_db().exists(), "una ruta de lectura no crea la base");
    front.stop().await;
    let now = wall_clock_ms();
    seed_usage(&home, &format!(
        "insert into usage_interactions(id,tmux_session,tmux_pane,started_at_ms,source) values('i1','s1','%1',{now},'hook:claude');\
         insert into usage_tool_calls(id,interaction_id,tool_name,skill_name,started_at_ms,finished_at_ms,duration_ms,status,confidence) values\
         ('c1','i1','Skill','tdd',{now},{now},20,'ok','exact'),('c2','i1','mcp__mobbin__search','',{now},{now},null,'failed','exact'),\
         ('c3','i1','Skill','',{now},{now},5,'ok','exact');"));
    let Some(py) = oracle(&home).await else { return };
    let mut opts = home.options();
    opts.clock = Arc::new(wall_clock_ms);
    let front = front(&home, dead_port(), opts).await;
    for target in ["/extension-usage?session=s1&pane=%251", "/extension-usage", "/extension-usage?days=x",
                   "/extension-usage?pane=%251", "/extension-usage?session=a%20b"] {
        let a = get(front.port, target).await;
        let b = get(py.port, target).await;
        let mask = |t: &str| {
            let mut v: serde_json::Value = serde_json::from_str(t).unwrap();
            if let Some(list) = v.get_mut("extensions").and_then(|e| e.as_array_mut()) {
                for e in list { e["lastSeen"] = json!(0); }
            }
            comandos_core::json::response_dumps(&v).unwrap()
        };
        assert_eq!((a.status, mask(&a.text())), (b.status, mask(&b.text())), "{target}");
    }
    front.stop().await;
}
```

(Las columnas de `usage_tool_calls`/`usage_interactions` del `insert` se ajustan al esquema v11 real: `sqlite3 <base> '.schema usage_tool_calls'` en una base de prueba; las que sean `not null` sin defecto se rellenan.)

- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_usage_guard` → FAIL de compilación.

- [ ] **Step 3: Implementar** `guard.rs`, `extensions.rs`, el cambio de `context.rs`/`gather.rs`, la entrada `Key::Path("/extension-usage")`, y adaptar la prueba de la 2d: con la base de uso sembrada (un turno `claude_jsonl` reciente con modelo `claude-*`) el contexto trae `guard.projects` del frente; con `user_version = 12` y una tarjeta que lo necesita, `/state` declina.

- [ ] **Step 4: Fixture**

```json
{"name":"u-extension-usage","method":"GET","path":"/extension-usage","headers":{"Host":"127.0.0.1"},"body":null,"volatile":["/extensions/*/lastSeen"],"expect":"same"}
{"name":"u-extension-usage-dias","method":"GET","path":"/extension-usage?days=x","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same"}
```

- [ ] **Step 5: Verde** — Run: `$C test -p comandos-server --test dash_native_usage_guard --test dash_native_state` → PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/comandos-store/src/usage_read.rs crates/comandos-server/src/dash/native/lanes.rs \
  crates/comandos-server/src/dash/native/usage/mod.rs crates/comandos-server/src/dash/native/usage/guard.rs \
  crates/comandos-server/src/dash/native/usage/extensions.rs crates/comandos-server/src/dash/native/states/context.rs \
  crates/comandos-server/src/dash/native/states/gather.rs crates/comandos-server/tests/dash_native_usage_guard.rs \
  crates/comandos-server/tests/dash_native_state.rs
# si se borró: git rm crates/comandos-server/src/dash/native/subrequest.rs (y git add del mod.rs que lo declaraba)
git add -f xtask/parity/frente.jsonl
git commit -m "feat(dash): contexto de sugerencias sin el heredado y GET /extension-usage nativo

token_guard_with_forecast y la latencia de experiment_analytics por el carril de uso;
/state ya no pide /usage/guard ni /usage/analytics. /extension-usage nunca crea la base.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Motor de GET `/usage/state` (latente, sin entrada en la tabla)

El cálculo completo de la respuesta, con `record_pane` y el memo, sin bordes ni importación (Tareas 7 y 8). **No** se añade a `usage::ROUTES`: se prueba llamando a `usage::state::compute` directamente, y en producción `/usage/state` sigue reenviada (el Python sigue siendo dueño de sus efectos). Desplegable sola.

Orden del motor (`compute(native) -> Result<UsageStateReply, Fault>`), contra 8441-8462:

1. **Punto de declinar**: `settings = native.usage.with(usage_settings).await?` (carril apagado → `Fault::Decline`). Desde aquí nunca se devuelve `Fault::Decline` (D1): un trabajo posterior del carril que declina se trata según la regla de cada paso.
2. **Paneles vivos** (`usage_live_panes` 7230): `labels = gather::session_labels(native)` y `maps = gather::agent_maps(native)` (2d; extraídas si eran internas); por cada `info` de `maps.by_cwd` en orden, deduplicado por `(session, pane, agent)`: `git_root = git_root_for_path(cwd)` (abajo), `normalize_pane_identity({session, pane, cwd, git_root, agent or "claude", pid}, labels, now)`. Los `git rev-parse` de distintos paneles se lanzan concurrentes (`futures_util::future::join_all`); el resultado no depende del orden. Un fallo de la recolección de la 2d que el Python no tendría (`StateFault::Decline`) → lista vacía para esta vuelta **sin** escribir `record_pane` (el Python vería paneles; el cuerpo usaría `list_panes`: diferencia acotada a una vuelta, anotada en el cutover); `StateFault::Timeout`/`Failure` (tmux colgado/roto) → 504/500, como el Python.
3. **`git_root_for_path`** (423): `""` → `""`; `tmux::run_program_in(&Program::named("git"), &["rev-parse", "--show-toplevel"], cwd, Duration::from_secs(3))`: éxito con `stdout.strip()` no vacío → ese texto; cualquier otra cosa (plazo, no existe el directorio, salida no UTF-8) → `path`.
4. **Entorno** (D7): `parse_env_text` de `H/cc-notify.conf` y `H/usage.env` (ausente → vacío; no UTF-8 → `HandlerError::Failure`, el `UnicodeDecodeError` no capturado), luego `usage_env`, luego `settings`.
5. **Límites**: `limits = native.limits.get(&deps)` (efecto: puede lanzar el refresco).
6. **Memo** (D6): clave `(native.usage_engine.generation(), firma)`; si no coincide, con el `tokio::sync::Mutex` del memo tomado: `rows = native.usage.with(state_rows(now - 14*86400) [+ list_panes si no hay vivos])` (declina → `HandlerError::Failure`: el carril se acaba de apagar a mitad; el Python habría leído) y `build_state` en `spawn_blocking`; guardar `(clave, Arc<Value>)`. La respuesta parte de una copia superficial del memo.
7. `state["limits"] = list(limits)` → `state["credential_health"] = credential_health(env)` + `limits.health`; `state["alerts"] = list_alerts(12)`; `state["limits"] = enrich_limits(native.limits.attach_tokens(&state["windows"]), now, ui_lang)` (`comandos_core::allocation::enrich_limits`; `ui_lang` = `CC_LANG` de `cc-notify.conf` si es `es`/`en`, si no `es` cuando el `LANG` capturado empieza por `es`); `state["lastInteraction"] = recent_interactions(1)[0]` o `null`. Las dos lecturas (alertas y última interacción) van en un trabajo del carril; si declina → `HandlerError::Failure`.
8. **`record_pane`** por cada panel vivo (efecto; solo con `usage_effects`), en un trabajo del carril, errores ignorados (el `except Exception: pass` de 7253).
9. Cuerpo = `response_dumps(state)`; además `UsageStateReply.live_panes` (para el escritor de bordes de la Tarea 7) y `UsageStateReply.state: Arc<Value>`.

Python hace `record_pane` en el paso 2 (antes del memo); hacerlo en el 8 no cambia la respuesta (`build_state` no lee `usage_panes` cuando hay vivos, y sin vivos no hay `record_pane`).

**Files:**
- Create: `crates/comandos-server/src/dash/native/usage/state.rs`
- Modify: `crates/comandos-server/src/dash/native/usage/mod.rs` (`pub mod state;` sin entrada en `ROUTES`), `crates/comandos-server/src/dash/native/mod.rs` (`Native.usage_engine: usage::state::UsageEngine`), `crates/comandos-server/src/dash/native/tmux.rs` (`run_program_in`), `crates/comandos-server/src/dash/native/states/gather.rs` (`session_labels`, `agent_maps` → `pub(crate)` si eran internas)
- Create: `crates/comandos-server/tests/dash_native_usage_state.rs`

**Interfaces:**
- Consumes: T1 (`usage_read`, `usage_state`), T2 (`LimitsCache`), `allocation::enrich_limits`, 2d (`gather::{session_labels, agent_maps}`, `AgentMaps`).
- Produces: `usage::state::{UsageEngine { default, generation(&self) -> u64, bump(&self) }, UsageStateReply { body: bytes::Bytes, state: Arc<Value>, live_panes: Vec<Map<String, Value>> }, compute(&Native) -> Result<UsageStateReply, Fault>, git_root_for_path(&Path) -> impl Future<Output = String>, ui_lang(&Path, &BTreeMap<String, String>) -> &'static str}`; `tmux::run_program_in(&Program, &[&str], &Path, Duration) -> Result<Output, RunError>`.

Memo y motor (código nuevo):

```rust
/// Memo de `build_usage_state` (`cached_usage_state` 269): una entrada.
#[derive(Default)]
pub struct UsageEngine {
    generation: std::sync::atomic::AtomicU64,
    memo: tokio::sync::Mutex<Option<(MemoKey, Arc<Value>)>>,
}

#[derive(Clone, PartialEq, Eq)]
struct MemoKey {
    generation: u64,
    /// `sorted((tmux_session, tmux_pane, pane_pwd, agent))`.
    panes: Vec<(String, String, String, String)>,
}

impl UsageEngine {
    pub fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::Acquire)
    }
    /// Al terminar una importación completa (`_usage_state_generation += 1`).
    pub fn bump(&self) {
        self.generation.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }

    async fn state(&self, native: &Native, key: MemoKey, live: &[Map<String, Value>],
                   settings: Map<String, Value>, now: i64) -> Result<Arc<Value>, Fault> {
        // El candado se sostiene durante el cálculo: los que llegan esperan y
        // reutilizan (el `with _usage_state_lock` del Python).
        let mut memo = self.memo.lock().await;
        if let Some((k, state)) = memo.as_ref() {
            if *k == key {
                return Ok(state.clone());
            }
        }
        let want_panes = live.is_empty();
        let rows = native
            .usage
            .with(move |u| -> Result<_, usage_read::ReadError> {
                let rows = usage_read::state_rows(&u.conn, now - 14 * 86_400)?;
                let panes = if want_panes { usage_read::list_panes(&u.conn)? } else { Vec::new() };
                Ok((rows, panes))
            })
            .await
            .map_err(|_| failure())?
            .map_err(|_| failure())?;
        let live = live.to_vec();
        let built = tokio::task::spawn_blocking(move || {
            let (rows, stored) = rows;
            let panes = if live.is_empty() { stored } else { live };
            usage_state::build_state(now, panes, &rows.turns, &rows.provider_usage,
                                     &rows.provider_costs, &settings)
        })
        .await
        .map_err(|_| failure())?
        .map_err(|_| failure())?;
        let state = Arc::new(built);
        *memo = Some((key, state.clone()));
        Ok(state)
    }
}
```

(`failure()` = `Fault::Error(HandlerError::Failure)`.) Un cliente que se desconecta con el candado tomado suelta el futuro: el siguiente que espera toma el candado y calcula (el Python nunca abandona; repetir solo cuesta trabajo, D4 de la 2d).

- [ ] **Step 1: Pruebas que fallan**

`crates/comandos-server/tests/dash_native_usage_state.rs`:

```rust
//! Motor de /usage/state: bytes contra el oráculo, memo de un solo cómputo.
mod support;
use comandos_server::dash::native::{Native, usage::state, wall_clock_ms};
use std::{sync::Arc, time::Duration};
use support::{TestHome, oracle::oracle, seed_usage, get};

fn mask(text: &str) -> String {
    let mut v: serde_json::Value = serde_json::from_str(text).unwrap();
    v["generated_at"] = serde_json::json!(0);
    for p in v["panes"].as_array_mut().into_iter().flatten() { p["last_seen_at"] = serde_json::json!(0); }
    for l in v["limits"].as_array_mut().into_iter().flatten() { l["captured_at"] = serde_json::json!(0); }
    comandos_core::json::response_dumps(&v).unwrap()
}

#[tokio::test]
async fn usage_state_body_matches_python_without_live_panes() {
    let home = TestHome::new("ustate");
    let now = wall_clock_ms() / 1000;
    seed_usage(&home, &format!(
        "insert into usage_panes(tmux_session,tmux_pane,pane_pwd,git_root,agent,provider,started_at,last_seen_at,raw) \
         values('s1','%1','/r/a','/r/a','codex','codex',{a},{b},'{{}}');\
         insert into usage_turns(id,provider,agent,tmux_session,tmux_pane,pane_pwd,git_root,model,turn_started_at,\
         turn_finished_at,total_tokens,cost_usd,source,raw) values('t1','codex','codex','s1','%1','/r/a','/r/a',\
         'gpt-5.6-sol',{c},{c},420,0.0,'codex_rollout','{{}}');\
         insert into usage_settings(key,value) values('COMANDOS_CODEX_WEEKLY_TOKEN_LIMIT','1000');",
        a = now - 500, b = now - 5, c = now - 30));
    home.write("cc-notify.conf", "CC_LANG=es\nCOMANDOS_DAILY_BUDGET_USD='3'\n");
    let Some(py) = oracle(&home).await else { return };
    let mut opts = home.options();
    opts.clock = Arc::new(wall_clock_ms);
    opts.usage_effects = false;
    let native = Native::new(opts);
    let _ = state::compute(&native).await; // calienta la caché de límites
    tokio::time::sleep(Duration::from_millis(300)).await;
    let ours = state::compute(&native).await.ok().unwrap();
    let theirs = get(py.port, "/usage/state").await;
    assert_eq!(theirs.status, 200);
    assert_eq!(mask(std::str::from_utf8(&ours.body).unwrap()), mask(&theirs.text()));
}

#[tokio::test]
async fn usage_state_memo_single_flight() {
    let home = TestHome::new("ustate-flight");
    seed_usage(&home, "");
    let mut opts = home.options();
    opts.usage_effects = false;
    let native = Arc::new(Native::new(opts));
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let n = native.clone();
        tasks.push(tokio::spawn(async move { state::compute(&n).await.ok().map(|r| r.state) }));
    }
    let mut states = Vec::new();
    for t in tasks { states.push(t.await.unwrap().unwrap()); }
    assert!(states.windows(2).all(|w| Arc::ptr_eq(&w[0], &w[1])), "un solo cálculo, el mismo Arc");
    native.usage_engine.bump();
    let after = state::compute(&native).await.ok().unwrap().state;
    assert!(!Arc::ptr_eq(&after, &states[0]), "otra generación: memo nuevo");
}

#[tokio::test]
async fn usage_state_newer_schema_declines() {
    let home = TestHome::new("ustate-newer");
    seed_usage(&home, "pragma user_version = 12;");
    let native = Native::new(home.options());
    assert!(matches!(state::compute(&native).await, Err(comandos_server::dash::native::Fault::Decline)));
}
```

Más una prueba `usage_state_live_pane_records_and_git_root` (con `tmux` y `git`): `support::start_session` (2d) arranca un pane con un agente falso (`support::fake_agent`) en un repositorio `git init` dentro del HOME temporal; con `usage_effects = true`, `compute` devuelve `panes[0].git_root` = la raíz del repo y la fila de `usage_panes` existe en la base; con `usage_effects = false`, no existe.

- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_usage_state` → FAIL de compilación.

- [ ] **Step 3: Implementar** `usage/state.rs` (pasos 1–9, `UsageEngine`), `run_program_in` (como `run_program` con `cmd.current_dir(cwd)`; un `cwd` inexistente da `Spawn`, que el llamador trata como «devuelve `path`») y las extracciones de la 2d.

- [ ] **Step 4: Verde** — Run: `$C test -p comandos-server --test dash_native_usage_state` → 4 PASS (las que necesitan `tmux`/`git`/`python3` se saltan con aviso si faltan).

- [ ] **Step 5: Commit**

```bash
git add crates/comandos-server/src/dash/native/mod.rs crates/comandos-server/src/dash/native/tmux.rs \
  crates/comandos-server/src/dash/native/usage/mod.rs crates/comandos-server/src/dash/native/usage/state.rs \
  crates/comandos-server/src/dash/native/states/gather.rs crates/comandos-server/tests/dash_native_usage_state.rs
git commit -m "feat(dash): motor de /usage/state (latente)

Paneles vivos con raíz git, entorno de uso, memo por generación y firma de paneles
con un solo cálculo, salud, alertas, límites enriquecidos y última interacción.
Aún sin entrada en la tabla nativa: el Python sigue siendo dueño de sus efectos.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Bordes de pane, `pane-models.txt` y avisos de nivel (latente)

Port de `_pane_models_for_live_state` (286), `_pane_model_values` (4481), `_maybe_tier_alert` (4440), `usage_alert_send` (455), `notice_emit` (422), `_pane_models_need_reconcile`, `_pane_model_snapshot` (4540), `_pane_model_live_ids`, `_reconcile_pane_models` (4567) y `write_pane_models` (4641). Sin ruta todavía: se prueba llamando a `PaneModelWriter::apply` con valores de prueba; la Tarea 8 lo engancha.

Comportamiento:

- **`live_rows(live_panes, state, cards: Option<&[Value]>) -> Vec<Map>`** (286): ids vivos que empiezan por `%`; `cards` = las tarjetas de `Native::states_cached` (`None` si declinó: D1, esa vuelta no se escribe); por fila del memo con pane vivo: copia; si la tarjeta del pane tiene `model` truthy → `model`, `agent = card.agent or row.agent`, `provider = agent`, `reasoning_effort` si `card.effort`.
- **`pane_values(rows, inputs: &ValueInputs) -> (BTreeMap<String, Option<String>>, String, Vec<TierAlert>)`** (4481-4530): `model` sin `claude-` y con el último segmento tras `/`; cuenta = `AccountCache::account_for_pid(proc_root, home, agent_pid or pid, agent).account` (2d; en `spawn_blocking`); `head`; `switching` con `H/motor-results.json` (la lectura de la 2d, D1 de la 2d: archivo incierto → no se escribe esta vuelta) y `time.time() - ts < 300`; `model_tier`/`tier_style` sobre `config/model-tiers.json` (`providers::model_tier` de la 2d; `tier_style` = `tiers[tier]` si es objeto, si no `{}`); colores `PANE_MODEL_COLORS` (4472-4475) con `244` por defecto; las cuatro formas del valor (cambiando, modelo, detectando, solo cabeza) y `None`; `file_text` = líneas `plain` ordenadas por pane + `\n` final si hay alguna. Las alertas se calculan aquí (abajo) en el orden de los paneles.
- **`TierAlerts`** (4440): `last: HashMap<(String, String), String>` y `alerted: HashMap<(String, String), f64>`; `prev = last.insert(key, tier)`; sin `prev`, mismo nivel o nivel distinto de `alertTier` (por omisión `high`) → nada; cooldown 3600 s; `NOTIFY_MODEL_TIER == "0"` en `cc-notify.conf` → nada (pero `alerted` ya se actualizó, como el Python). Cota (rul. 6): al terminar cada `pane_values`, se quitan de `last` y `alerted` las claves cuyo pane no está entre los vivos de esa vuelta **solo si** su `alerted` venció (> 3600 s): así la deduplicación por pane no cambia y el mapa no crece con panes muertos.
- **Envío** (`usage_alert_send`): texto ES/EN de 4459-4463 (`ui_lang`), `notice_emit("usage_alert", title, msg, project)` = `comandos_store::append_event` por `native.with_state` con el evento de 427-434 (`eventId = "usage_alert:<ms>:<8 hex>"` con `getrandom`, `evidence: "confirmed"`, `correlation: "unknown"`, `projectKey = sess`, títulos recortados a 200/500); error → una línea `notice_emit usage_alert: <error>` en stderr; luego, si `DESKTOP_NOTIFY` (por omisión `"1"`) es `"1"`, `NotifyPost::post` con `response_dumps({"title", "body", "session": "", "kind", "project", "options": "", "full"})` (plazo 2 s, errores ignorados). Todo en una tarea (el `threading.Thread` del Python).
- **`PaneModelWriter::apply(values, file_text, tmux, hooks)`** (4641): bajo un `tokio::sync::Mutex<()>` de archivo: si `file_text` cambió → `files::write_text_atomic(H/pane-models.txt)` en `spawn_blocking`; si `OSError`, no se recuerda. Bajo el candado de estado: `desired`/`generation`/`retry_after`; lanzar la reconciliación si no corre, ya pasó `retry_after` y (no descubierto o `need_reconcile`).
- **Reconciliación** (4567-4638): código completo abajo; tmux con `run_program(&tmux.program, args, 2 s)`.

**Files:**
- Create: `crates/comandos-server/src/dash/native/usage/pane_models.rs`
- Modify: `crates/comandos-server/src/dash/native/usage/mod.rs` (`pub mod pane_models;`), `crates/comandos-server/src/dash/native/mod.rs` (`NativeOptions.notifyd: Arc<dyn NotifyPost>`, `Native.pane_models: Arc<PaneModelWriter>`, `Native.tier_alerts: std::sync::Mutex<TierAlerts>`)
- Modify: `crates/comandos-server/tests/support/mod.rs` (`FakeNotify`)
- Create: `crates/comandos-server/tests/dash_native_pane_models.rs`

**Interfaces:**
- Consumes: `AccountCache`, `providers::model_tier`, la lectura de `motor-results.json` de la 2d (`states::motor_results(&Path) -> Result<Map<String, Value>, StateFault>`; si en la 2d tiene otro nombre, el real), `comandos_store::append_event`, `Native::with_state`, `tmux::run_program`, `files::write_text_atomic`.
- Produces: `usage::pane_models::{NotifyPost, HyperNotify, PaneModelWriter { default, apply(self: &Arc<Self>, BTreeMap<String, Option<String>>, String, Tmux, PathBuf) -> impl Future<Output = ()>, applied(&self) -> BTreeMap<String, Option<String>>, running(&self) -> bool }, TierAlerts, TierAlert { session, agent, model, tier }, live_rows, pane_values(&Native, &[Map<String, Value>]) -> impl Future<Output = Option<(BTreeMap<String, Option<String>>, String, Vec<TierAlert>)>>, send_alerts(&Native, Vec<TierAlert>) -> impl Future<Output = ()>}`; `support::FakeNotify { bodies() -> Vec<String> }`.

Reconciliación (código nuevo; los nombres de los campos son los del Python):

```rust
#[derive(Default)]
struct WriterState {
    desired: BTreeMap<String, Option<String>>,
    applied: BTreeMap<String, Option<String>>,
    file_text: Option<String>,
    generation: u64,
    running: bool,
    discovered: bool,
    retry_after: Option<std::time::Instant>,
}

const RETRY: Duration = Duration::from_secs(5); // PANE_MODEL_RETRY_SECONDS
const TMUX_PLAZO: Duration = Duration::from_secs(2);

async fn reconcile(writer: Arc<PaneModelWriter>, tmux: Tmux) {
    loop {
        let (generation, desired, applied, discovered) = {
            let st = writer.lock();
            (st.generation, st.desired.clone(), st.applied.clone(), st.discovered)
        };
        if !discovered {
            // `_pane_model_snapshot`: una lectura de las opciones físicas.
            let physical = list_panes(&tmux, "#{pane_id}\t#{@ccmodel}").await.map(|out| {
                out.lines()
                    .filter_map(|l| l.split_once('\t'))
                    .filter(|(id, v)| id.starts_with('%') && !v.is_empty())
                    .map(|(id, v)| (id.to_owned(), Some(v.to_owned())))
                    .collect::<BTreeMap<_, _>>()
            });
            let mut st = writer.lock();
            match physical {
                None => {
                    st.retry_after = Some(std::time::Instant::now() + RETRY);
                    st.running = false;
                    return;
                }
                Some(applied) => {
                    st.applied = applied;
                    st.discovered = true;
                    st.retry_after = None;
                }
            }
            continue;
        }
        let ids: BTreeSet<String> = desired.keys().chain(applied.keys()).cloned().collect();
        let mut live: Option<BTreeSet<String>> = None;
        let mut retry_failed = false;
        for id in ids {
            let value = match desired.get(&id) {
                Some(v) if applied.get(&id) == Some(v) => continue,
                Some(v) => v.clone(),
                None if applied.get(&id) == Some(&None) => continue,
                None => None,
            };
            let ok = match &value {
                None => set_option(&tmux, &["set-option", "-p", "-u", "-t", &id, "@ccmodel"]).await,
                Some(v) => set_option(&tmux, &["set-option", "-p", "-t", &id, "@ccmodel", v]).await,
            };
            if ok {
                writer.lock().applied.insert(id, value);
                continue;
            }
            let mut needs_retry = true;
            if value.is_none() {
                if live.is_none() {
                    live = list_panes(&tmux, "#{pane_id}").await
                        .map(|o| o.lines().filter(|l| l.starts_with('%')).map(str::to_owned).collect());
                }
                if live.as_ref().is_some_and(|l| !l.contains(&id)) {
                    writer.lock().applied.insert(id, None);
                    needs_retry = false;
                }
            }
            retry_failed |= needs_retry;
        }
        let mut st = writer.lock();
        if generation != st.generation {
            continue; // llegaron valores nuevos durante la vuelta
        }
        let stale: Vec<String> = st.applied.iter()
            .filter(|(id, v)| v.is_none() && !st.desired.contains_key(*id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            st.applied.remove(&id);
        }
        st.retry_after = retry_failed.then(|| std::time::Instant::now() + RETRY);
        st.running = false;
        return;
    }
}
```

`list_panes(tmux, format) -> Option<String>` = `run_program(&tmux.program, &["list-panes", "-a", "-F", format], TMUX_PLAZO)` con éxito → stdout; si no `None`. `set_option` = éxito de `run_program` con `returncode == 0`. Un `None` en `applied` significa «opción quitada» (el `None` del Python); el `Drop` de la tarea no deja `running` en `true`: envolver el cuerpo con una guarda que lo pone a `false` si la tarea se cancela.

- [ ] **Step 1: Pruebas que fallan**

`crates/comandos-server/tests/dash_native_pane_models.rs`:

```rust
//! Escritor de bordes contra un tmux privado; avisos de nivel una vez por pane y hora.
mod support;
use comandos_server::dash::native::usage::pane_models::{PaneModelWriter, TierAlerts};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use support::{TestHome, tmux_available};

async fn tmux(home: &TestHome, args: &[&str]) -> String {
    let out = tokio::process::Command::new("tmux").arg("-f").arg("/dev/null").args(args)
        .env("TMUX_TMPDIR", home.tmux_dir()).env_remove("TMUX").output().await.unwrap();
    String::from_utf8(out.stdout).unwrap()
}

#[tokio::test]
async fn writer_sets_and_clears_pane_options_and_file() {
    if !tmux_available() { eprintln!("sin tmux: se salta"); return; }
    let home = TestHome::new("pane-models");
    tmux(&home, &["new-session", "-d", "-s", "p", "sleep 60"]).await;
    let pane = tmux(&home, &["display-message", "-p", "-t", "=p:", "#{pane_id}"]).await.trim().to_owned();
    // Una opción vieja de otro pane muerto no existe; una del pane vivo sí: se descubre.
    tmux(&home, &["set-option", "-p", "-t", &pane, "@ccmodel", "viejo"]).await;
    let writer = Arc::new(PaneModelWriter::default());
    let opts = home.options();
    let mut values = BTreeMap::new();
    values.insert(pane.clone(), Some("#[fg=colour43,bold]▸ codex#[default]".to_owned()));
    writer.apply(values, format!("{pane} codex\n"), opts.tmux.clone(), opts.hooks.clone()).await;
    for _ in 0..100 { if !writer.running() { break; } tokio::time::sleep(Duration::from_millis(20)).await; }
    assert_eq!(tmux(&home, &["show-options", "-p", "-v", "-t", &pane, "@ccmodel"]).await.trim(),
               "#[fg=colour43,bold]▸ codex#[default]");
    assert_eq!(std::fs::read_to_string(home.hooks().join("pane-models.txt")).unwrap(), format!("{pane} codex\n"));
    let mut cleared = BTreeMap::new();
    cleared.insert(pane.clone(), None);
    writer.apply(cleared, String::new(), opts.tmux.clone(), opts.hooks.clone()).await;
    for _ in 0..100 { if !writer.running() { break; } tokio::time::sleep(Duration::from_millis(20)).await; }
    assert_eq!(tmux(&home, &["show-options", "-p", "-v", "-t", &pane, "@ccmodel"]).await.trim(), "");
    assert_eq!(std::fs::read_to_string(home.hooks().join("pane-models.txt")).unwrap(), "");
}

#[test]
fn tier_alert_once_per_pane_per_hour() {
    let mut alerts = TierAlerts::default();
    let key = ("s".to_owned(), "%1".to_owned());
    assert!(!alerts.observe(&key, "low", "high", 0.0));    // primera vista: nunca avisa
    assert!(alerts.observe(&key, "high", "high", 10.0));   // cambia a alerta: avisa
    assert!(!alerts.observe(&key, "low", "high", 20.0));
    assert!(!alerts.observe(&key, "high", "high", 30.0));  // dentro de la hora: no
    assert!(!alerts.observe(&key, "low", "high", 3700.0));
    assert!(alerts.observe(&key, "high", "high", 3711.0)); // pasó la hora: sí
}
```

(`TierAlerts::observe(&mut self, key, tier, alert_tier, now) -> bool` es la parte pura de `_maybe_tier_alert` sin la lectura de `NOTIFY_MODEL_TIER`; `pane_values` la llama y luego filtra por la configuración.) Más `tier_alert_sends_notice_and_popup` (con `python3` no hace falta): un `Native` con `FakeNotify`; `send_alerts(&native, vec![alerta])` → un evento `usage_alert` en `app-state` (consulta directa a la base de prueba) y un cuerpo en `FakeNotify` con `"title": "Modelo Alto en uso"` (la etiqueta sale de `config/model-tiers.json` del repo).

- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_pane_models` → FAIL de compilación.

- [ ] **Step 3: Implementar** `pane_models.rs` (todo lo de arriba; `HyperNotify` con `hyper::client::conn::http1` a `127.0.0.1:4778`, `POST /notify`, `Content-Type: application/json`, todo bajo `tokio::time::timeout(2 s)`), `FakeNotify` y los campos nuevos (`for_home`: `notifyd: Arc::new(HyperNotify)`; `TestHome::options`: `FakeNotify`).

- [ ] **Step 4: Verde** — Run: `$C test -p comandos-server --test dash_native_pane_models` → 3 PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/comandos-server/src/dash/native/mod.rs crates/comandos-server/src/dash/native/usage/mod.rs \
  crates/comandos-server/src/dash/native/usage/pane_models.rs crates/comandos-server/tests/support/mod.rs \
  crates/comandos-server/tests/dash_native_pane_models.rs
git commit -m "feat(dash): bordes de pane, pane-models.txt y avisos de nivel (latente)

Valores de borde con la reconciliación de las tarjetas, escritor con descubrimiento,
reintento a 5 s y limpieza de panes muertos; aviso de nivel una vez por pane y hora
por el registro de avisos y cc-notifyd detrás de NotifyPost.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Importación de uso, dueño único y activación de GET `/usage/state`

Port de `_do_refresh_local_usage` (347), `refresh_local_usage` (235), `ensure_observed_configs` (315), `record_runtime_config` (185, rama `observed`), y de `record_local_codex_rollouts` (1418), `prune_old_turns` (1521), `record_local_opencode_db` (1539), `record_local_grok_updates` (1611), `record_local_claude_jsonl` (2055), `record_spans` (1994), `record_turns`/`_turn_values` (672/618), `_changed_files` (2045), `account_homes` (1977), `record_session_config` (2349), `latest_session_config` (2370), `reconcile_orphan_interactions` (2395) + `_latest_config`/`_upsert_config`. Al final, `/usage/state` entra en la tabla nativa con todos sus efectos.

Importadores (`comandos_store::usage_import`, síncronos, corren en el hilo del carril de importación; `git` por el rasgo `GitRoots`):

```rust
/// `git_root_for_path`: el carril de importación lo implementa con
/// `Handle::block_on(git_root_for_path(path))` (puente de la 2c); las pruebas, con un mapa.
pub trait GitRoots {
    fn root(&self, path: &str) -> String;
}

pub struct ImportPlan<'a> {
    pub now: i64,
    pub max_age_days: i64,
    pub claude_max_files: Option<usize>,
    pub codex_max_files: Option<usize>,
    pub home: &'a Path,
    /// `COMANDOS_CLAUDE_PROJECTS_DIR` (solo cuenta `main`) y `COMANDOS_OPENCODE_DB`.
    pub claude_projects_main: Option<PathBuf>,
    pub opencode_db: PathBuf,
    pub zone: &'a dyn LocalZone,
}

/// `seen` acotado (rul. 6): por fuente, solo las rutas del corte de esta vuelta.
#[derive(Default)]
pub struct ImportSeen {
    sources: BTreeMap<String, BTreeMap<PathBuf, f64>>,
}
```

Funciones: `record_local_codex_rollouts(conn, plan, &mut ImportSeen, &dyn GitRoots) -> Result<usize, ImportError>`, `record_local_grok_updates(conn, plan, homes, &dyn GitRoots)`, `record_local_claude_jsonl(conn, plan, root, account, &mut ImportSeen, &dyn GitRoots)`, `record_local_opencode_db(conn, plan, &dyn GitRoots)`, `prune_old_turns(conn, now, max_age_days)`, `reconcile_orphan_interactions(conn, now, 14)`, `latest_session_config(conn, session, pane) -> Result<Map<String, Value>, ImportError>`, `record_session_config(conn, &Map) -> Result<(), ImportError>`, `turn_values(&Map) -> Vec<(&'static str, Cell)>` (orden de `TURN_FIELDS`), `account_homes(home, default_rel, accounts_rel) -> Vec<(String, PathBuf)>`. Reglas que se suelen romper:

- `_changed_files` sobre `files[:max_files]`: el `seen` de la fuente se **reconstruye** con exactamente esas rutas (las que salen del corte se olvidan); la salida es la misma que con el `dict` sin tope del Python, porque una ruta que vuelve al corte con el mismo `mtime` no estaba en la lista anterior… salvo que no cambiara: se compara contra el `seen` previo antes de reconstruirlo, así que un archivo que sale y vuelve sin cambiar se relee (el Python no lo releería). Diferencia sin efecto en la base: el `upsert` es idempotente.
- Lectura de líneas con `open(path, errors="replace")`: bytes no UTF-8 → U+FFFD, y el iterador de archivo de texto de Python parte por `\n`, `\r` y `\r\n` (nuevas líneas universales), no por los separadores Unicode de `str.splitlines`. `line_no` cuenta esas líneas desde 1. `json.loads(line)` con el parser de `comandos_core::json::parse_value` (acepta el `\n` final; una línea que el Python acepta y Rust no, p. ej. `NaN`, se salta en los dos: `parse_value` admite `NaN`/`Infinity` como el `json` de Python; comprobarlo en la prueba).
- `raw` = `json.dumps({...}, sort_keys=True)` → `comandos_core::json::dumps(&v, true, false)`.
- Codex: `turns[turn_id] = payload` sobrescribe; `cwd`/`thread` de `session_meta` se pisan con el último no vacío; `total = total_tokens or inp + out`; `input_tokens = max(0, inp - cached)`.
- Claude: `os.walk` podando `tool-results` y `memory`; `stable = f"{msg_id}:{requestId}"` si hay `message.id`; `finished = _as_epoch(timestamp) or int(_mtime)`; los tramos `turn_duration` usan `roots[cwd] or cwd`, los turnos `roots[cwd]`.
- Grok: `glob(…/sessions/**/updates.jsonl, recursive=True)` (incluye `sessions/updates.jsonl` y no entra en nombres que empiezan por `.`); orden `(mtime, ruta)` desc; tope 200; la consulta de interacción por línea dentro de la misma conexión; `finished_ms` en segundos si `< 1e11`; `next(iter(model_usage), "") or model`; `_stable_id([path, line_no, prompt_id])`.
- OpenCode: `sqlite3.connect(f"file:{oc_db}?mode=ro", uri=True)` → `Connection::open_with_flags(..., SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_URI)`; cualquier error → 0 sin escribir.
- Las cuatro fuentes escriben con `record_spans` y `record_turns` en su propia transacción (`with connect() as con`), en el orden del Python.

Orquestación (`native/usage/import.rs`, código nuevo):

```rust
//! Dueño de la importación (D1): intervalo de 60 s, gracia al arrancar, flock
//! entre frentes, generación del memo al terminar.
pub const IMPORT_INTERVAL_S: i64 = 60;

pub struct ImportOwner {
    started_ms: i64,
    state: std::sync::Mutex<ImportState>,
}

#[derive(Default)]
struct ImportState {
    /// `LOCAL_USAGE_REFRESH_AT`.
    last_at: i64,
    running: bool,
    seen: Option<usage_import::ImportSeen>,
}

impl ImportOwner {
    pub fn new(started_ms: i64) -> Self {
        Self { started_ms, state: std::sync::Mutex::new(ImportState::default()) }
    }

    /// `refresh_local_usage(False)`: decide y lanza; nunca espera.
    pub fn maybe_start(self: &Arc<Self>, native: &Native, states: Option<Arc<Vec<Value>>>) {
        let opts = native.options();
        if !opts.usage_effects || !native.import_lane.enabled() || !native.usage.enabled() {
            return;
        }
        let now_ms = (opts.clock)();
        let now = now_ms / 1000;
        if now_ms - self.started_ms < opts.usage_import_grace_ms {
            return;
        }
        let seen = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if now - st.last_at < IMPORT_INTERVAL_S || st.running {
                return;
            }
            st.running = true;
            st.last_at = now;
            st.seen.take().unwrap_or_default()
        };
        tokio::spawn(run(self.clone(), native.import_deps(), states, seen, now));
    }
}

/// `finally: _local_usage_refreshing = False`, también si la tarea se cancela.
struct Running(Arc<ImportOwner>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.state.lock().unwrap_or_else(|p| p.into_inner()).running = false;
    }
}

async fn run(owner: Arc<ImportOwner>, deps: ImportDeps, states: Option<Arc<Vec<Value>>>,
             seen: usage_import::ImportSeen, now: i64) {
    let _running = Running(owner.clone());
    // Dos frentes sobre el mismo HOME: el que no tiene el candado salta la vuelta.
    let lock_path = deps.opts.hooks.join("comandos-usage-import.lock");
    let lock = match tokio::task::spawn_blocking(move || files::FileLock::try_acquire(&lock_path)).await {
        Ok(Ok(Some(lock))) => lock,
        _ => return,
    };
    let handle = tokio::runtime::Handle::current();
    let observed = states.as_deref().map(|items| observed_configs(items)).unwrap_or_default();
    let env = deps.env.clone();
    let home = deps.home.clone();
    let zone = deps.opts.zone.clone();
    let outcome = deps
        .import_lane
        .with(move |b| import_cycle(&b.conn, &handle, &env, &home, zone.as_ref(), observed, seen, now))
        .await;
    drop(lock);
    if let Ok(Ok(seen)) = outcome {
        owner.state.lock().unwrap_or_else(|p| p.into_inner()).seen = Some(seen);
        deps.engine_bump.bump(); // `_usage_state_generation += 1`, solo si todo terminó
    }
}
```

`import_cycle(conn, handle, env, home, zone, observed, seen, now) -> Result<ImportSeen, ImportError>` replica 347-417 en orden: (1) `try:` `ensure_observed_configs` sobre `observed` (lista de `(session, pane, harness, motor, model, effort, route, harness_account, motor_account)` ya filtrada como en 321-334, calculada fuera a partir de las tarjetas de `Native::states_cached`; con `None` —`/state` declinó— se omite esta parte y se registra en el cutover) con `latest_session_config` y, si cambia, `record_runtime_config` = `record_session_config` con `source = "observed"`, `confidence = "exact"`, `effective_at = now`; luego `reconcile_orphan_interactions`; cualquier error de esas dos → se ignora el resto del bloque (el `except: pass` cubre ambas); (2) `max_age = int(env COMANDOS_USAGE_LOCAL_DAYS or 21)` (`py::int`; error → `Err`, el hilo del Python moría aquí); (3) `try: prune_old_turns`; (4) `max_files` de `*_MAX_FILES` (`int(...) or 0) or None`); (5) codex → grok → claude por cuenta (`account_homes("~/.claude", "~/.claude-accounts")`, `projects` = `COMANDOS_CLAUDE_PROJECTS_DIR` solo para `main`) → opencode; un error de cualquiera → `Err` (sin generación nueva). `GitRoots` del ciclo = `handle.block_on(git_root_for_path(path))` con una caché por ciclo (`roots` del Python está por importador; los resultados son los mismos).

Carril (`lanes.rs`): `UsageImportBackend { pub conn: Connection }` con `ROUTES = "la importación de uso de GET /usage/state"` y la misma puerta (`gate`) que `UsageBackend`; `open` igual que `UsageBackend::open`.

Activación de la ruta (`usage/mod.rs`): `UsageRoute::State` con `Key::Path("/usage/state")`; `answer`:

```rust
UsageRoute::State => {
    let reply = state::compute(native).await?;                 // punto de declinar (paso 1)
    let cards = native.states_cached().await.ok();             // D1: sin tarjetas no hay bordes
    native.import.maybe_start(native, cards.as_ref().map(|s| s.items.clone()));
    if native.options().usage_effects {
        if let Some(cards) = cards {
            let rows = pane_models::live_rows(&reply.live_panes, &reply.state, Some(&cards.items));
            if let Some((values, text, alerts)) = pane_models::pane_values(native, &rows).await {
                let o = native.options();
                native.pane_models.clone().apply(values, text, o.tmux.clone(), o.hooks.clone()).await;
                pane_models::send_alerts(native, alerts).await;
            }
        }
    }
    Ok(Reply::json_bytes(StatusCode::OK, reply.body))
}
```

(`Reply::json_bytes` = el constructor de `Reply` con el cuerpo ya serializado; si no existe, `reply(StatusCode::OK, &reply.state_with_extras)` con el `Value` final.) Python escribe bordes después de calcular el estado y antes de responder; aquí igual. `UsageBackend::ROUTES` pasa a `"GET /pomodoro, GET /sovereignty, GET /state, GET /analytics/week, GET /accounts, GET /extension-usage y GET /usage/state"`.

Opciones nuevas: `DashConfig.usage_effects` (`--no-usage-effects` o `COMANDOS_DASH_USAGE_EFFECTS=0` → `false`) copiado a `NativeOptions.usage_effects` en `build`; `NativeOptions.usage_import_grace_ms` (75 000 por omisión; `COMANDOS_DASH_USAGE_IMPORT_GRACE_MS` si es un entero ≥ 0); `Native.{import_lane: Arc<Lane<UsageImportBackend>>, import: Arc<ImportOwner>}`, `Native::import_deps()`. `xtask parity` arranca el frente con `COMANDOS_DASH_USAGE_IMPORT_GRACE_MS=0` (el Python importa en su primera petición; el frente debe hacer lo mismo para que las dos copias de la base sigan iguales).

**Files:**
- Create: `crates/comandos-store/src/usage_import.rs`; Modify: `crates/comandos-store/src/lib.rs`, `crates/comandos-store/src/usage.rs`
- Create: `crates/comandos-store/tests/usage_import_oracle.rs`
- Create: `crates/comandos-server/src/dash/native/usage/import.rs`
- Modify: `crates/comandos-server/src/dash/native/{mod.rs, lanes.rs, usage/mod.rs}`, `crates/comandos-server/src/dash/mod.rs` (flag y env), `xtask/src/parity.rs`
- Create: `crates/comandos-server/tests/dash_native_usage_import.rs`
- Modify: `xtask/parity/frente.jsonl`

**Interfaces:**
- Consumes: T1 (`turn_values` usa `stable_id`, `TURN_INSERT_SQL`), T6 (`compute`, `UsageEngine::bump`, `git_root_for_path`), T7 (`live_rows`, `pane_values`, `apply`, `send_alerts`), `files::FileLock` (2c), `Native::states_cached` (2d).
- Produces: `usage_import::{GitRoots, ImportPlan, ImportSeen, ImportError, …}` (arriba); `usage::import::{ImportOwner::{new, maybe_start}, ImportDeps, IMPORT_INTERVAL_S}`; `lanes::UsageImportBackend`; `UsageRoute::State`; `DashConfig.usage_effects`; `NativeOptions.usage_import_grace_ms`.

- [ ] **Step 1: Prueba diferencial de los importadores**

`crates/comandos-store/tests/usage_import_oracle.rs`: un HOME temporal con un rollout de Codex (un `session_meta`, dos `turn_context`, tres `token_usage_record` —uno repetido como en un fork—, un `task_complete`, una línea con bytes inválidos y una con `\r` en medio), un proyecto de Claude con dos líneas del mismo `message.id` (una sola fila), una `turn_duration` y un archivo bajo `tool-results/` (ignorado), una sesión de Grok con `summary.json` y dos `turn_completed` (uno en segundos), una base de OpenCode creada con `rusqlite` (`message`, `session`), y una base de uso con dos interacciones huérfanas (una resoluble por carrera, otra por proveedor) y un turno de hace 30 días. Se copian el HOME y la base a dos directorios; el oráculo corre en uno:

```rust
const ORACLE: &str = r#"
import json, os, sys
repo, home, db, now = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
sys.path[:0] = [os.path.join(repo, "bin"), os.path.join(repo, "lib")]
os.environ["HOME"] = home
import cc_usage
roots = json.load(open(os.path.join(home, "roots.json")))
cc_usage.git_root_for_path = lambda p, run=None: roots.get(p, p)
cc_usage.reconcile_orphan_interactions(db, now=now)
cc_usage.prune_old_turns(db, max_age_days=21, now=now)
seen = {}
cc_usage.record_local_codex_rollouts(db, now=now, max_age_days=21, max_files=None, seen=seen.setdefault("codex", {}))
cc_usage.record_local_grok_updates(db, [os.path.join(home, ".grok")], now=now, max_age_days=21)
for alias, h in cc_usage.account_homes("~/.claude", "~/.claude-accounts"):
    cc_usage.record_local_claude_jsonl(db, os.path.join(h, "projects"), now=now, max_age_days=21,
        max_files=None, account=alias, seen=seen.setdefault("claude:" + alias, {}))
cc_usage.record_local_opencode_db(db, os.path.join(home, "opencode.db"), now=now, max_age_days=21)
"#;
```

y el lado Rust ejecuta las funciones de `usage_import` en el mismo orden sobre la otra copia (con `GitRoots` del mismo `roots.json`). Se compara el volcado ordenado de `usage_turns`, `usage_spans`, `usage_session_configs` y `usage_interactions` (`select * from <t> order by 1`) de las dos bases, fila a fila (`response_dumps` de cada fila como lista). Segunda pasada sobre las dos con el mismo `seen`: ninguna fila cambia y `record_local_*` de Codex/Claude no relee (contador de archivos leídos en Rust = 0).

- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-store --test usage_import_oracle` → FAIL de compilación.

- [ ] **Step 3: Implementar** `usage_import.rs`.

- [ ] **Step 4: Verde de los importadores** — Run: `$C test -p comandos-store --test usage_import_oracle` → PASS.

- [ ] **Step 5: Pruebas del dueño y de la ruta que fallan**

`crates/comandos-server/tests/dash_native_usage_import.rs`:

```rust
//! Dueño único de los efectos de /usage/state (D1).
mod support;
use comandos_server::dash::native::files::FileLock;
use std::{sync::Arc, time::Duration};
use support::{FakeLegacy, TestHome, front, get, seed_usage};

fn claude_line(home: &TestHome, id: &str) {
    let dir = home.root.join(".claude/projects/-r-a");
    std::fs::create_dir_all(&dir).unwrap();
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
    std::fs::write(dir.join("x.jsonl"), format!(
        "{{\"type\":\"assistant\",\"timestamp\":\"{now}\",\"cwd\":\"/r/a\",\"sessionId\":\"s\",\
         \"message\":{{\"id\":\"{id}\",\"model\":\"claude-fable-5\",\"usage\":{{\"input_tokens\":5,\"output_tokens\":7}}}}}}\n")).unwrap();
}

async fn turns(home: &TestHome) -> i64 {
    let conn = rusqlite::Connection::open(home.usage_db()).unwrap();
    conn.query_row("select count(*) from usage_turns", [], |r| r.get(0)).unwrap()
}

async fn wait_turns(home: &TestHome, want: i64) -> bool {
    for _ in 0..100 {
        if turns(home).await == want { return true; }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    false
}

#[tokio::test]
async fn usage_state_is_native_and_imports_after_grace() {
    let home = TestHome::new("import-grace");
    seed_usage(&home, "");
    claude_line(&home, "m1");
    let mut opts = home.options();
    opts.clock = Arc::new(comandos_server::dash::native::wall_clock_ms);
    opts.usage_import_grace_ms = 0;
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, opts).await;
    let wire = get(front.port, "/usage/state").await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    assert!(legacy.requests().is_empty(), "el Python no ve /usage/state");
    assert!(wait_turns(&home, 1).await, "la importación del frente escribió el turno");
    front.stop().await;
}

#[tokio::test]
async fn import_lock_contended_skips_cycle() {
    let home = TestHome::new("import-lock");
    seed_usage(&home, "");
    claude_line(&home, "m2");
    let held = FileLock::try_acquire(&home.hooks().join("comandos-usage-import.lock")).unwrap().unwrap();
    let mut opts = home.options();
    opts.clock = Arc::new(comandos_server::dash::native::wall_clock_ms);
    opts.usage_import_grace_ms = 0;
    let front = front(&home, support::dead_port(), opts).await;
    assert_eq!(get(front.port, "/usage/state").await.status, 200);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(turns(&home).await, 0, "otro frente tiene el candado: esta vuelta no importa");
    drop(held);
    front.stop().await;
}

#[tokio::test]
async fn usage_lane_down_hands_effects_back() {
    let home = TestHome::new("import-handover");
    seed_usage(&home, "pragma user_version = 12;");
    claude_line(&home, "m3");
    let mut opts = home.options();
    opts.usage_import_grace_ms = 0;
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, opts).await;
    assert_eq!(get(front.port, "/usage/state").await.text(), r#"{"legacy": true}"#);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let conn = rusqlite::Connection::open(home.usage_db()).unwrap();
    let n: i64 = conn.query_row("select count(*) from usage_turns", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0, "con el carril apagado el frente no importa");
    assert!(!home.hooks().join("pane-models.txt").exists(), "ni escribe bordes");
    front.stop().await;
}

#[tokio::test]
async fn no_usage_effects_writes_nothing() {
    let home = TestHome::new("import-shadow");
    seed_usage(&home, "");
    claude_line(&home, "m4");
    let mut opts = home.options();
    opts.usage_effects = false;
    opts.usage_import_grace_ms = 0;
    let front = front(&home, support::dead_port(), opts).await;
    assert_eq!(get(front.port, "/usage/state").await.status, 200);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(turns(&home).await, 0);
    assert!(!home.hooks().join("pane-models.txt").exists());
    front.stop().await;
}
```

Más `grace_delays_first_import` (con el reloj inyectado: a `+74 s` no hay turno; a `+76 s`, tras otro `/usage/state`, sí) y `import_failure_keeps_generation` (`COMANDOS_USAGE_LOCAL_DAYS = "x"` en `usage_env`: la importación termina en error y `native.usage_engine.generation()` sigue en 0).

- [ ] **Step 6: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_usage_import` → FAIL (502/compilación).

- [ ] **Step 7: Implementar** `import.rs`, `UsageImportBackend`, la variante y entrada de `/usage/state`, las opciones y flags, y en `xtask/src/parity.rs` el `env("COMANDOS_DASH_USAGE_IMPORT_GRACE_MS", "0")` del proceso del frente.

- [ ] **Step 8: Fixture**

En `frente.jsonl`:

```json
{"name":"u-usage-state-calentar","method":"GET","path":"/usage/state","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"status-only"}
{"name":"u-usage-state","method":"GET","path":"/usage/state","headers":{"Host":"127.0.0.1"},"body":null,"volatile":["/generated_at","/panes/*/last_seen_at","/limits/*/captured_at","/limits/*/runsOutIn","/limits/*/pace","/limits/*/burn","/lastInteraction"],"expect":"same"}
{"name":"u-usage-state-prefijo","method":"GET","path":"/usage/stateX","headers":{"Host":"127.0.0.1"},"body":null,"volatile":[],"expect":"same","forwarded":true}
```

`/lastInteraction` es volátil solo en el arnés: las dos importaciones (Python y frente) reconcilian huérfanas en momentos distintos de la corrida; su igualdad la fijan las pruebas de la Tarea 6 y de este paso 1.

- [ ] **Step 9: Verde** — Run: `$C test -p comandos-store && $C test -p comandos-server` → PASS.

- [ ] **Step 10: Commit**

```bash
git add crates/comandos-store/src/lib.rs crates/comandos-store/src/usage.rs crates/comandos-store/src/usage_import.rs \
  crates/comandos-store/tests/usage_import_oracle.rs crates/comandos-server/src/dash/mod.rs \
  crates/comandos-server/src/dash/native/mod.rs crates/comandos-server/src/dash/native/lanes.rs \
  crates/comandos-server/src/dash/native/usage/mod.rs crates/comandos-server/src/dash/native/usage/import.rs \
  crates/comandos-server/tests/dash_native_usage_import.rs xtask/src/parity.rs
git add -f xtask/parity/frente.jsonl
git commit -m "feat(dash): GET /usage/state nativo con importación y dueño único de sus efectos

Importadores de Codex, Grok, Claude y OpenCode, poda y reconciliación en un carril
propio; gracia de 75 s al arrancar, flock entre frentes y --no-usage-effects para la
sombra. Con el carril de uso apagado, el Python vuelve a ser dueño de todo.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Medición y documento de cutover «2e»

- [ ] **Step 1: Suite completa**

Run: `$C test --workspace` → 0 fallos (anotar N). Run: `$C clippy --workspace --all-targets -j 6 -- -D warnings` y `$C fmt --all -- --check` → limpios.

- [ ] **Step 2: Paridad y carga en el netns** (copias de `~/.claude/hooks`, `app-state.sqlite3` y `comandos-usage.sqlite`, como en la 2d)

```sh
CARGO_TARGET_DIR=$PWD/.build/target nice -n 10 cargo build --release -p comandos-cli -j 6
CARGO_TARGET_DIR=$PWD/.build/target nice -n 10 cargo build -p xtask -j 6
NEW=$PWD/.build/target/release/comandos; XT=$PWD/.build/target/debug/xtask
"$XT" parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 \
  --usage-db ~/.claude/hooks/comandos-usage.sqlite --comandos "$NEW"
"$XT" poll --shadow --minutes 10 --comandos "$NEW"
"$XT" poll --shadow --minutes 10 --comandos "$NEW" --no-native
```

Anotar: OK/DIFF/SKIP de la paridad (0 DIFF); Pss del frente minuto 1 → 10 y, desde el minuto 5, mín–máx y pendiente; Pss del heredado minuto 1 → 10 con y sin nativo; latencia p50/p95/p99 de `GET /usage/state` y `GET /analytics/week` con y sin nativo.

- [ ] **Step 3: Escribir la sección** al final de `docs/verification/cutover-dash.md`, sustituyendo cada `N`, «(lista)» y «(sí/no)» de «Medido antes del cutover» por lo anotado:

````markdown
## 2e: uso y analítica nativos

Procedimiento para el controlador, como el de la 2d. El frente está en 4777 y el Python heredado
en 4781; la 2e solo cambia el binario del frente. Ni el Python ni las unidades ni tmux se tocan.

### Qué cambia

| Ruta | Nativa | Sigue en el Python |
|---|---|---|
| GET `/usage/state` | sí, con sus efectos | solo con la base de uso apagada para el frente (más nueva, ilegible) |
| GET `/analytics/week` | sí | `?sidebar` si el Python commiteado no lo soporta |
| GET `/accounts`, `/providers`, `/optimization/plans` | sí | registro de proveedores que no valida con certeza |
| GET `/extension-usage` | sí | — |
| GET y POST `/session-profiles`, POST `/session-profile-apply`, GET `/opencode/models` | no | todo |

`/state` ya no pide `/usage/guard` ni `/usage/analytics` al heredado: la guardia y la latencia de
las sugerencias las calcula el frente.

**Dueño único.** Desde el cutover, el frente hace lo que el `/usage/state` del Python hacía como
efecto: importar transcripts de Claude, Codex, Grok y OpenCode cada ≥ 60 s (la primera vez 75 s
después de arrancar), escribir los bordes `@ccmodel` de tmux y `~/.claude/hooks/pane-models.txt`,
avisar al pasar un pane a un modelo de nivel alto y registrar los paneles vivos en la base. El
Python deja de hacerlo porque ya no recibe `/usage/state`. Si el frente apaga su carril de uso
(una base más nueva), reenvía `/usage/state` y el Python vuelve a hacerlo todo. Dos frentes sobre
el mismo HOME no importan a la vez (`~/.claude/hooks/comandos-usage-import.lock`).

### Diferencias y comportamientos aceptados (2e)

- Límites de proveedor: el frente tiene su propia caché (60 s; 180 s tras un error de la cuenta
  `main`) y el Python conserva su bucle de 5 min hasta la 2g: ≈ +20 % de llamadas a
  `api.anthropic.com` frente a antes. Las fotos de cuota convergen (solo gana la más nueva).
- `credential_health.*.error` de un fallo de TLS o de un cuerpo que no es JSON tiene el texto del
  frente, no el de `urllib`.
- Si `/state` es incierto en el momento de un `/usage/state`, esa vuelta no escribe bordes (la
  siguiente, ≤ 10 s después, sí) y la importación de ese ciclo no registra configuraciones
  observadas (el ciclo siguiente sí).
- Tras reiniciar el frente, la deduplicación de avisos de nivel empieza de cero (como tras
  reiniciar el Python).
- Un archivo de transcript que sale del corte de 21 días y vuelve sin cambiar se relee (el
  Python lo recordaba); la base queda igual.

### 0. Previos

```sh
cd ~/codebase/0xJesus/ComandOS && git log -1 --oneline        # main con la Fase 2e fusionada
CARGO_TARGET_DIR=$PWD/.build/target nice -n 10 cargo build --release -p comandos-cli -j 6
CARGO_TARGET_DIR=$PWD/.build/target nice -n 10 cargo build -p xtask -j 6
NEW=$HOME/codebase/0xJesus/ComandOS/.build/target/release/comandos
XT=$HOME/codebase/0xJesus/ComandOS/.build/target/debug/xtask
grep -qa 'GET /extension-usage y GET /usage/state' "$NEW" || echo "BINARIO SIN 2e: no seguir"
systemctl --user is-active cc-dash.service cc-dash-legacy.service    # active active
~/.local/share/comandos/bin/comandos install --releases              # anotar la actual ('*')
for u in cc-dash.service cc-dash-legacy.service; do
  systemctl --user show -p Environment -p WorkingDirectory "$u"; done # mismo PATH, WorkingDirectory y TZ;
                                                                      # COMANDOS_*USAGE*, *_TOKEN_LIMIT,
                                                                      # COMANDOS_CLAUDE_PROJECTS_DIR,
                                                                      # COMANDOS_OPENCODE_DB, LANG iguales o ausentes
ss -ltn 'sport = :4782'                                               # libre
```

### 1. Sombra en 4782 con nativo, contra el heredado 4781

La sombra arranca **sin efectos de uso**: no importa, no escribe bordes ni avisa (el dueño sigue
siendo el Python, que recibe el `/usage/state` de producción).

```sh
COMANDOS_DASH_TRACE_FORWARD=1 "$NEW" dash 4782 --legacy-port 4781 --no-usage-effects 2> /tmp/sombra-2e.log
```

Desde otra terminal, en `~/codebase/0xJesus/ComandOS`:

```sh
"$XT" parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 \
  --usage-db ~/.claude/hooks/comandos-usage.sqlite --comandos "$NEW"        # 0 DIFF
T=$(cat ~/.claude/hooks/dash-token)
for r in /providers /optimization/plans '/accounts?harness=claude' '/analytics/week?offset=-1'; do
  A=$(curl -s -H "X-Comandos-Token: $T" "127.0.0.1:4782$r"); B=$(curl -s -H "X-Comandos-Token: $T" "127.0.0.1:4781$r")
  [ "$A" = "$B" ] && echo "igual $r" || echo "DISTINTO $r (límites: repetir tras 60 s)"
done
for r in /usage/state /analytics/week; do
  curl -s -o /dev/null -w "$r %{time_total}s\n" -H "X-Comandos-Token: $T" "127.0.0.1:4782$r"
done
```

`/usage/state` y `/analytics/week` de esta semana no se comparan a mano: dependen de cuándo cada
proceso refrescó límites e importó; los compara el arnés con sus campos de tiempo enmascarados.
Navegación manual en `http://127.0.0.1:4782` con `chrome-bg` (vía `cc-browser-expose start 4782`):
tarjetas de uso por proveedor, Analytics (Cuentas, Comparar, Pomodoro), la columna de límites de
la barra lateral, el botón «Cuenta» de un pane y el selector de motor (proveedores y planes).
Luego:

```sh
grep -c 'se reenvían al heredado\|rutas nativas desactivadas' /tmp/sombra-2e.log   # 0
grep 'reenvío' /tmp/sombra-2e.log | sed 's/.*reenvío //' | sort | uniq -c | sort -rn
```

Ni `GET /usage/state`, ni `GET /analytics/week`, ni `GET /accounts` deben aparecer. Ctrl+C.

### 2. Cutover

```sh
"$NEW" install --stage
grep -qa 'GET /extension-usage y GET /usage/state' "$(readlink -f ~/.local/share/comandos/bin/comandos)" || echo "STAGE MALO: ~/.local/share/comandos/bin/comandos install --rollback-release"
~/.local/share/comandos/bin/comandos hook claude-status >/dev/null && echo "hooks OK con la release nueva"
systemctl --user restart cc-dash.service
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:4777/        # 200
journalctl --user -u cc-dash.service --since -2min --no-pager | grep -c 'se reenvían al heredado\|rutas nativas desactivadas'   # 0
```

Sin un cambio de modelo o cuenta en vuelo en el momento del reinicio.

### 3. Verificación

```sh
systemctl --user set-environment COMANDOS_DASH_TRACE_FORWARD=1 && systemctl --user restart cc-dash.service
sleep 600
journalctl --user -u cc-dash.service --since -11min --no-pager | grep 'reenvío' \
  | sed 's/.*reenvío //' | sort | uniq -c | sort -rn
systemctl --user unset-environment COMANDOS_DASH_TRACE_FORWARD && systemctl --user restart cc-dash.service
sqlite3 ~/.claude/hooks/comandos-usage.sqlite 'select max(turn_finished_at), count(*) from usage_turns'  # avanza
P=$(systemctl --user show -p MainPID --value cc-dash.service)
L=$(systemctl --user show -p MainPID --value cc-dash-legacy.service)
grep Pss /proc/$P/smaps_rollup; grep Pss /proc/$L/smaps_rollup     # al minuto 1 y al 10
```

- Ninguna ruta de uso en la traza; `GET /usage/guard` y `GET /usage/analytics` tampoco (la 2d ya
  no los pide).
- Tras un turno de Claude o Codex, la tarjeta de uso lo refleja en ≤ 2 min; el borde del pane
  cambia al cambiar de modelo en ≤ 10 s; `pane-models.txt` coincide con los bordes.
- Pss del frente plano (± 1 MiB entre el minuto 1 y el 10); el heredado crece menos que antes del
  cutover en el mismo intervalo.

### 4. Reversión

A/B sin cambiar binario (el Python vuelve a ser dueño de importación y bordes en su primer
`/usage/state`; su primera respuesta puede traer el estado de antes del cutover durante un ciclo):

```sh
systemctl --user set-environment COMANDOS_DASH_NATIVE=0 && systemctl --user restart cc-dash.service
# persistente: drop-in ~/.config/systemd/user/cc-dash.service.d/no-native.conf con
# [Service]\nEnvironment=COMANDOS_DASH_NATIVE=0, daemon-reload y restart (deshacer: rm + daemon-reload + restart)
```

Volver a la release anterior (la 2d):

```sh
~/.local/share/comandos/bin/comandos install --releases
~/.local/share/comandos/bin/comandos install --rollback-release
systemctl --user restart cc-dash.service
grep -qa 'GET /extension-usage y GET /usage/state' "$(readlink -f ~/.local/share/comandos/bin/comandos)" && echo "SIGUE LA 2e"
grep -qa 'GET /pomodoro, GET /sovereignty y GET /state' "$(readlink -f ~/.local/share/comandos/bin/comandos)" || echo "NO ES LA 2d: revisar releases/previous"
```

El centinela de la 2d (`'GET /pomodoro, GET /sovereignty y GET /state'`) solo lo lleva el binario
de la 2d. Desde el rebase de la 2e sobre main, `UsageBackend::ROUTES` dice `"GET /pomodoro, GET
/sovereignty, GET /state, GET /analytics/week y GET /extension-usage"`; tras la Tarea 8, `"…, GET
/analytics/week, GET /accounts, GET /extension-usage y GET /usage/state"`. El centinela de la 2e
(`'GET /extension-usage y GET /usage/state'`) exige ese orden.

Las filas que escribe el frente (turnos, tramos, configuraciones, paneles, fotos de cuota) tienen
el formato y las claves del Python: revertir no necesita limpiar nada. `comandos-usage-import.lock`
puede quedarse en `~/.claude/hooks`.

### Medido antes del cutover

Rama de la 2e, binario release, arnés en namespace de red privado, copias de `~/.claude/hooks`,
`app-state.sqlite3` y `comandos-usage.sqlite`.

- Suite del workspace: N pruebas, 0 fallos.
- `xtask parity`: N OK, 0 DIFF, 0 SKIP. Reenviadas: (lista).
- `xtask poll --shadow --minutes 10` con nativo: Pss del frente minuto 1 → 10 (KiB), desde el
  minuto 5 min–max y pendiente; heredado minuto 1 → 10; latencia de `GET /usage/state` y
  `GET /analytics/week` p50/p95/p99.
- Lo mismo con `--no-native`.
- Criterios: frente plano (sí/no); heredado ≤ 50 % del crecimiento sin nativo (sí/no); p95 nativo
  de `/usage/state` ≤ p95 sin nativo y p99 ≤ 1000 ms (sí/no).
````

- [ ] **Step 4: Verificar el documento**

Run: `grep -n '^## 2e' docs/verification/cutover-dash.md` → una línea. `grep -qa 'GET /extension-usage y GET /usage/state' .build/target/release/comandos && echo ok` → `ok`. «Medido antes del cutover» sin `N` ni «(sí/no)». Cada orden usa `"$NEW"`/`"$XT"` con ruta explícita.

- [ ] **Step 5: Commit**

```bash
git add docs/verification/cutover-dash.md
git add -f docs/superpowers/plans/2026-10-04-fase-2e-uso-nativo.md
git commit -m "docs(verification): cutover 2e — uso y analítica nativos

Sombra sin efectos de uso, dueño único de importación y bordes, verificación,
A/B con --no-native, reversión por env/drop-in o --rollback-release y mediciones.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Self-review

- **Cobertura del alcance**: `/usage/state` (Tareas 6–8, con `build_usage_state` en 1, importación ≥ 60 s en 8, `write_pane_models` y `pane-models.txt` en 7, límites con red y caché 60/180 s en 2); `/analytics/week` con `offset`, `demo` y `sidebar` (3, D9); `/accounts` (4); `/usage/guard` y `/usage/analytics?days=7` como dependencias internas de la 2d (5, y la subconsulta desaparece); POST `/usage/settings`, `/usage/quota`, `/usage/subscription`, GET `/dedication`: sin llamador vivo en `dash/` ni en `bin/cc-app` commiteados (solo `lib/operator_catalog.py`), ya 410 nativas desde la 2b (D11); `/providers` y `/optimization/plans` sobre el registro de la 2d (4); `/extension-usage` (5); `/session-profiles` queda reenviada con su razón (D11). Importación: decisión de dueño único con alternativas descartadas (D1).
- **Rulings**: 1 → cada ruta compara bytes con el oráculo (pruebas de 3, 4, 5, 6) y por capa (1, 2, 4, 8); 2 → carril de uso para lecturas y escrituras cortas, carril de importación (D2), puerta compartida, OpenCode de solo lectura dentro del trabajo; 3 → punto de declinar en el primer trabajo de lectura de cada ruta, efectos después (6 paso 1, 8 `answer`); 4 → plazos 8/3/2/2 s en `ReqwestOauth`, `git_root_for_path`, reconciliación y `HyperNotify`; 5 → lectores en `spawn_blocking`, importación en el hilo del carril, red en tareas (`limits_hung_network_never_blocks`); 6 → memo de una entrada, caché de límites de una entrada, `ImportSeen` acotado, `TierAlerts` con poda, correos máx. 64; 7 → fixture `u-*` por ruta y `--usage-db`; 8 → Tarea 9; 9 → Global Constraints; 10 → `OauthHttp`/`FakeOauth`, `NotifyPost`/`FakeNotify`, tmux privado, `dead_port`; 11 → orden 1–9 con 6 y 7 latentes.
- **Placeholders**: ninguno salvo «Medido antes del cutover», que el Step 3 de la Tarea 9 exige rellenar con mediciones. Los nombres de la 2d que podrían diferir (`session_labels`, `agent_maps`, `motor_results`, `EmailCache`) llevan la instrucción de usar el real o extraerlo.
- **Tipos**: `Map<String, Value>` para filas en `usage_read`, `usage_state` y `LimitsCache`; `ReadError` → 500 en las rutas; `Fault::Decline` solo desde trabajos de carril que declinan y desde el registro incierto; `UsageEngine::bump` lo llama solo `import::run`; `LimitsCache::get(self: &Arc<Self>, &RefreshDeps)` en 3, 4, 5 y 6; `PaneModelWriter::apply(self: &Arc<Self>, …)` en 7 y 8; `Context::get(&self, &Native, …)` en 5.
- **Review Focus**: cada línea tiene su prueba en la tarea dueña (8, 2, 2, 8, 6).
