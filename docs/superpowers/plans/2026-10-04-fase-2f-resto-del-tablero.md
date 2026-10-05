# Fase 2f — Resto del tablero: plan maestro

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** que después de la 2f el frente Rust `comandos dash` (4777) no reenvíe al `cc-dash` Python (4781) ninguna ruta por clasificación —solo las `Decline` documentadas, contadas por un censo—, que los hilos de fondo del Python tengan su equivalente Rust listo para tomar el relevo y que `cc-notifyd` exista en Rust, de modo que la 2g solo tenga que apagar el Python.

**Architecture:** este archivo es el plan maestro. Fija el inventario exacto de lo que sigue reenviado, las reglas comunes, cuatro tareas compartidas (andamio para trabajar en paralelo, kit de procesos/candados/censo/cortes, gemelo de pruebas para mutaciones, `tmux_snapshot`) y la tarea final del cutover «2f». El trabajo de dominio vive en cinco sub-planes con fronteras de estado disjuntas: **2f-1** pestañas, sesiones y teclas; **2f-2** operaciones de sesión (cuenta, modelo, motor, extensiones, perfiles); **2f-3** servicios del tablero (remoto, SSH, ajustes, catálogos, Pomodoro, hilos de fondo, residuo del despachador); **2f-4** noticias; **2f-5** `comandos-notifyd`. Cada sub-plan es un **corte**: un conjunto de rutas que comparten un estado con un solo dueño y que se activa y se revierte entero (`COMANDOS_DASH_CUTS_OFF`).

**Tech Stack:** Rust 1.96 (edition 2024, `unsafe_code = "forbid"`), tokio 1.53 (`rt`, `process`, `sync`, `time`, `net`), hyper 1.11, rusqlite 0.40 (bundled), serde_json con `arbitrary_precision` + `preserve_order`, `regex = "=1.13.1"`, crates `comandos-core`/`comandos-store`/`comandos-runtime`/`comandos-server`; `gtk = "0.18"` solo en el crate nuevo `comandos-notifyd` (2f-5); arnés `xtask parity`/`xtask poll` en un netns (`unshare -Urn`).

**Spec:** `docs/superpowers/specs/2026-10-04-comandos-rust-complete-design.md` (§4.2, §4.3, §5, §7, Enmiendas 1–8). Inventario: `docs/research/2026-10-04-fase-2-inventario.md` (§1–§9). Planes previos, cuyos bloques **Interfaces** dan los nombres que este plan consume: `docs/superpowers/plans/2026-10-04-fase-2c-dominios-nativos-ii.md`, `docs/superpowers/plans/2026-10-04-fase-2d-state-nativo.md` (con su pre-flight `.superpowers/sdd/2026-10-04-fase-2d-state-nativo/preflight.md` del worktree `rust-fase2d`, adoptado) y `docs/superpowers/plans/2026-10-04-fase-2e-uso-nativo.md`. Procedimiento vivo: `docs/verification/cutover-dash.md` (secciones 2a–2c ejecutadas). Oráculo: `bin/cc-dash` y `lib/*.py` de este checkout; las líneas citadas son las de `bin/cc-dash` en `0aa4ae1` (9 820 líneas; las del inventario difieren en ≈ 120). Los implementadores localizan el Python **por nombre de función**, no por línea.

**Sub-planes:**

| Archivo | Corte | Contenido |
|---|---|---|
| `docs/superpowers/plans/2026-10-04-fase-2f-1-pestanas-y-sesiones.md` | `tabs` | registro de pestañas (dueño único de `app-tabs*.json`), creación/cierre/recuperación de sesiones, cuentas nuevas, SSH que abre pestañas, teclas y foco, `/terminal-panes` con `close` |
| `docs/superpowers/plans/2026-10-04-fase-2f-2-operaciones-de-sesion.md` | `ops` | adaptador de configuración de sesión (cambio de cuenta en vivo, modelo, motor, extensiones por pane), journal y `motor-results.json` con un solo dueño, `/model/status` sin declinaciones, `/proxy`, perfiles de sesión |
| `docs/superpowers/plans/2026-10-04-fase-2f-3-servicios-del-tablero.md` | `services` y `residue` | remoto/Tailscale/terminal web, SSH config, ajustes, catálogos de CLIs y cadenas, modelos de OpenCode, Web Push, POST `/pomodoro` y su planificador, vigilante de modelos y bucle de límites, residuo del despachador |
| `docs/superpowers/plans/2026-10-04-fase-2f-4-noticias.md` | `news` | lectura de ediciones, notas, guardadas, chat y traducción con agentes, planificador de ediciones |
| `docs/superpowers/plans/2026-10-04-fase-2f-5-notifyd.md` | (binario aparte) | `comandos-notifyd`: `POST /notify` en 4778 y popups GTK3 |

**Precondición:** las Fases 2c, 2d y 2e están fusionadas en `main`, que ya contiene `accab21` (tmux privado con `-S` en `Tmux::private`, el `Drop` de `TestHome` y `xtask` `private_tmux`), `cf193ba` (`/analytics/week?sidebar=1` y `/accounts` con `list_accounts` y `?usage=0` confirmados, de la 2e) y `c87973f`. Si un nombre de sus bloques Interfaces no coincide con el código, se usa el real y se anota en el commit. Rama de integración: `migration/rust-fase2f` desde `main`; cada grupo paralelo trabaja en `migration/rust-fase2f-<grupo>` y se fusiona en la de integración; `main` recibe la 2f solo en el cutover (puede ser por sub-plan, nunca a mitad de un corte).

---

## Rulings del controlador que fijan este plan

1. **Respuestas idénticas byte a byte** vía `comandos_core::json::response_dumps` (status, `Content-Type`, `Cache-Control: no-store`, cuerpo); textos de error literales del Python.
2. **`Decline` solo antes de cualquier efecto**; declinar reenvía la petición original al Python.
3. **SQLite solo por los carriles existentes**: app-state por `BackendWorker<StateBackend>`; la base de uso por `Lane<UsageBackend>` (y `Lane<UsageImportBackend>` de la 2e); el journal de operaciones por `Lane<JournalBackend>`. Nunca desde el hilo del runtime.
4. **Nada que bloquee en el runtime `current_thread`**: escaneos de directorios, `/proc`, copias de transcripts, escrituras con `fsync`, `flock` con espera → `tokio::task::spawn_blocking`; procesos → `tokio::process`.
5. **Red y procesos en tareas async con los plazos del Python** (los de `subprocess.run(timeout=…)` y `urlopen(timeout=…)` de cada función, citados en cada tarea).
6. **Paridad**: líneas de fixture por ruta y pruebas contra el oráculo Python por ruta; entorno del oráculo = `fakebin` en `PATH` (con el `tmux` guardián de la Tarea 2, que fuerza `-S`) + `XDG_RUNTIME_DIR` y `TMUX_TMPDIR` privados.
7. **Cutover**: sección «2f» en `docs/verification/cutover-dash.md` con rutas explícitas como la 2c.
8. Comentarios en español, identificadores en inglés, sin `unsafe`, sin `unwrap`/`expect`/indexado en código que no sea de prueba, clippy `-D warnings`, rustfmt, trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, `git add` explícito, `git add -f` para `docs/superpowers` y `*.jsonl`; cero Python o bash nuevos en el repositorio.
9. **Por cada ruta que muta**: exactamente-una-vez/idempotencia descrita, orden frente al Python mientras ambos corren, declinar antes del efecto, pruebas solo contra tmux privado.
10. **Tareas ordenadas por riesgo e independencia**, marcando qué tareas pueden ir en worktrees paralelos.

Reglas de oro heredadas: los cutovers los ejecuta el controlador; el Python no se modifica; las pruebas nunca tocan el tmux del usuario, `~/.claude` real, `~/.local/state`, `~/.local/share/comandos`, el systemd de usuario ni los puertos 4777–4782.

## Global Constraints

- Todo se compila y prueba con
  `CARGO_TARGET_DIR=/home/someguy/codebase/0xJesus/ComandOS/.build/target nice -n 10 cargo <cmd> -j 6`
  (abreviado `$C <cmd>`). Antes de cada commit: `$C fmt --all -- --check`, `$C clippy --workspace --all-targets -j 6 -- -D warnings` y las pruebas de los paquetes tocados.
- Commits con `git add <rutas>` explícitas; mensajes `feat(dash): …` / `feat(runtime): …` / `feat(notifyd): …` / `fix(…): …` / `docs(verification): …` en español, con línea en blanco y `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Los planes: `git add -f docs/superpowers/plans/2026-10-04-fase-2f*.md`; los fixtures: `git add -f xtask/parity/2f/<dominio>.jsonl`.
- **Regla tmux (vinculante, `CLAUDE.md` del repositorio; el 4 de octubre a las 21:59 un subagente mató las ~20 sesiones vivas del usuario con `TMUX_TMPDIR=<dir borrado> tmux kill-server`):**
  - Todo tmux de prueba lleva socket explícito: `-S <dir>/tmux-<uid>/default` o `-L <etiqueta propia>`. En Rust: `Tmux::private(dir)` (frente), `TestHome::tmux_command()` (pruebas, Tarea 2), `private_tmux(dir)` (`xtask`). **`TMUX_TMPDIR` solo nunca basta**: tmux 3.2a lo ignora en silencio si el directorio no existe y cae en `/tmp/tmux-1000/default`.
  - El Python del oráculo llama a `tmux` sin `-S`: por eso el `fakebin` del oráculo y del gemelo contiene un `tmux` guardián generado por la prueba (`support::oracle::TMUX_GUARD`, Tarea 2) que añade `-S <socket privado>` y sale con 97 si el directorio del socket no existe. Toda prueba que ejecute código Python que pueda llamar a tmux usa ese entorno; ninguna lo hace con el `PATH` desnudo.
  - Orden de limpieza: primero `kill-server` con el `-S` propio, después borrar el directorio (`Drop` de `TestHome` ya lo hace así).
  - Prohibido `tmux kill-server`, `kill-session`, `pkill tmux` o `kill -9 -1` sin `-S`/`-L` propio, aunque sea «para probar». **Prohibido ejecutar tmux a mano** (ni el implementador ni el revisor): todo tmux va dentro de una prueba con su socket.
  - Las opciones del frente en pruebas pasan por `support::assert_private_tmux(&opts)` (Tarea 2): falla si `opts.tmux` o el `tmux` que lanza `opts.scope` no llevan `-S`.
  - `systemd-run` en pruebas es siempre el falso `SCOPE_RUNNER` (Tarea 2), que ejecuta su cola sin unidad ni scope; nunca el real (crearía unidades en el gestor del usuario).
  - Cada tarea que muta tmux (crear/cerrar pestañas o panes, matar o recuperar sesiones, cambiar cuenta o modelo) incluye un apartado **«Confinamiento»** (cómo sus pruebas se limitan al tmux privado y al `systemd-run` falso) y un apartado **«Efectos en vivo»** (qué órdenes ejecuta el código de producción y la prueba de que son exactamente las del Python: mismas órdenes, mismos destinos `=<sesión>`/`%<pane>`, ninguna sobre sesiones que el Python no tocaría).
- Pruebas: `support::TestHome` (HOME temporal, socket tmux privado con `-S`, sin `COMANDOS_STATE_DB`/`COMANDOS_USAGE_DB`/`CLAUDE_CONFIG_DIR`/`CODEX_HOME`/`GROK_HOME`). Las mutaciones se comparan con el gemelo de la Tarea 2 (`support::twin`). Sin `tmux` o sin `python3` la prueba se salta con un aviso (no `#[ignore]`).
- Oráculo de capas: `python3 -c <texto>` dentro de las pruebas Rust (precedente: `crates/comandos-runtime/tests/hook_claude_parity.rs`), con el entorno de `support::oracle` (Tarea 2). Ningún `.py` nuevo. Los ejecutables falsos que una prueba necesite (p. ej. un `systemd-run` que solo ejecuta su cola) los **genera la prueba** dentro del HOME temporal: no son archivos del repositorio.
- Excepción no capturada del Python → `HandlerError::Failure` (500 `{"error": "Error interno del tablero"}`); `subprocess.TimeoutExpired` no capturado → `HandlerError::Timeout` (504).
- Directorios en el orden de `std::fs::read_dir`; los conjuntos que el Python ordena se ordenan igual (`sorted` de cadenas = orden de bytes UTF-8 de `str` en CPython para BMP; ver cada tarea).

## Decisiones del plan (no fijadas por el controlador)

- **D1 — Alcance exacto.** Lo que sigue reenviado tras 2c/2d/2e está en «Inventario» abajo (79 pares método+ruta, 6 de ellos condicionados por la fusión pendiente de la barra de comandos) más el residuo del despachador (prefijos, `/operator*` GET, POST/DELETE desconocidos, estáticos que no son archivo, HEAD de rutas reenviadas). **Fuera de alcance**, con razón: el bucle de instantáneas de 30 s y la resurrección de pestañas son de `cc-app` (`bin/cc-app`, hilo `app-tabs-snapshot.json`), que se porta en la Fase 4; ttyd y los scripts `cc-webterm`/`cc-webterm-attach` se retiran con `comandos-term` en la Fase 3 (Enmienda 6): la 2f los **invoca** por la misma ruta que el Python; SSE no existe hoy (Enmienda 5) y el único long-poll (`/notices/watch`) ya es nativo desde la 2b.
- **D2 — Dueño único por estado: los cortes.** El Python guarda estado de proceso que el Rust no comparte: `TAB_METADATA_LOCK` (un `RLock` de hilo, `bin/cc-dash:5249`) protege `app-tabs-meta.json`; `MOTOR_RESULT` (un `dict` en memoria reescrito entero en `motor-results.json` en cada `_set_motor_result`, 3533) es el estado de progreso de todas las operaciones; `_POMODORO_WAKE`, `_model_watch_state`, `_news_scheduler` son de proceso. Si Rust y Python escriben a la vez, el último que reescribe pierde lo del otro. Por eso las rutas se agrupan en **cortes**: todas las rutas que escriben un mismo estado sin candado compartido pasan al frente juntas y se revierten juntas. Cortes: `tabs` (2f-1: todo escritor de `app-tabs.json`, `app-tabs-meta.json`, `app-tabs-history.json`, `app-tab-open.json`, `app-tab-close.json`, `app-focus.json`), `ops` (2f-2: journal, `motor-results.json`, borradores de extensiones, `session-effort.json`, perfiles), `services` (2f-3), `news` (2f-4), `residue` (2f-3, último). Dentro de un corte activo, el frente solo declina en la validación previa a toda lectura de esos archivos.
- **D3 — Reversión por corte.** `COMANDOS_DASH_CUTS_OFF=tabs,ops` (o `--cuts-off=tabs,ops`) hace que `Native::dispatch` declinará, sin evaluar nada, toda ruta cuyo `NativeRoute::cut()` esté en la lista. Junto a `COMANDOS_DASH_NATIVE=0` (todo) y `--rollback-release`, da reversión por dominio sin cambiar binario. `Cut::Base` (rutas 2b–2e) no se puede desactivar por esta vía.
- **D4 — Trabajo en paralelo sin conflictos.** La Tarea 0 crea por adelantado los módulos de dominio vacíos (`pub enum XRoute {}`, `ROUTES = &[]`), sus variantes de `NativeRoute`, su línea en `TABLES` y en `answer`, los módulos de apoyo de pruebas y un archivo de fixture por dominio (`xtask/parity/2f/<dominio>.jsonl`, que `xtask parity` acepta con `--fixture` repetido). Después, cada tarea de dominio solo toca **su** módulo, **su** archivo de pruebas, **su** fixture y, si lo dice su bloque Files, una librería de `comandos-runtime` propia. Nadie más edita `native/mod.rs`, `tests/support/mod.rs` ni `frente.jsonl`. El documento de cutover lo escribe solo la tarea final.
- **D5 — Gemelo para mutaciones.** `support::oracle` comparte el HOME con el frente, lo que sirve para lecturas. Para comparar una mutación hacen falta dos estados de partida idénticos: `support::twin::Twin` crea dos `TestHome` (A para el frente, B para el oráculo), cada uno con su servidor tmux privado, siembra ambos con la misma función, lanza la petición a cada lado y compara (a) los bytes de respuesta tras normalizar lo que depende del reloj (`term-r<n>`, `ts`, `closedAt`) y (b) los archivos que la tarea declara (`app-tabs*.json`, `~/.ssh/config`…), con las mismas normalizaciones. El `fakebin` del gemelo incluye un `systemd-run` generado por la prueba que descarta sus opciones `--…` y ejecuta el resto: sin él, `scope_cmd` del Python no crearía la sesión.
- **D6 — Hilos de fondo con dueño explícito.** `DashConfig.background: Background` (`--background=legacy|front`, `COMANDOS_DASH_BACKGROUND`). Con `legacy` (por omisión en toda la 2f) el frente **solo** arranca lo que es idempotente con el Python vivo: el planificador de Pomodoro (sus `settle_due` son transacciones y el sonido se reclama una vez por dispositivo; ver 2f-3) y la limpieza propia de cada dominio. El vigilante de modelos, el planificador de ediciones de noticias, el bucle de 5 min de límites, el envío Web Push y la restauración de la terminal web al arrancar existen y tienen pruebas, pero solo corren con `front`. La 2g pone `front` en el mismo paso en que para el Python.
- **D7 — Censo de declinaciones.** `Native::dispatch` cuenta cada `Decline` por `(método, ruta sin consulta)`; cada 60 s y al apagarse escribe `$XDG_RUNTIME_DIR/comandos-dash-declines.json` (`{"since": ms, "counts": {"GET /state": n, …}}`, atómico). Es la entrada de la 2g: cada fila que quede tiene que estar en la lista «Declinaciones que heredan la 2g» de la tarea final con su sustituto.
- **D8 — Oráculo confirmado frente al árbol vivo.** `cc-dash-legacy` ejecuta el árbol de trabajo sin commit del checkout principal. A 4 de octubre ese árbol cambia, para esta fase: `/account/add` (alias automático `cuenta-N`, 409 si ya tiene sesión, `config.toml` de Codex, `env -u …` en el comando de login, candado de proceso), y la herencia de confianza (Codex además de Claude) (`/accounts?usage=0` ya está en `main` por `cf193ba`); y retira los llamadores de 6 rutas (`dash/session-controls.js` borrado, `dash/index.html` sin `/proxy` ni `/harness/switch`). Regla, como la resolución X1 del pre-flight de la 2d: **antes de empezar una tarea, el controlador confirma o descarta en `main` los cambios del árbol vivo que toquen las funciones de esa tarea**; se porta la versión que ejecutará el heredado. Cada tarea afectada empieza con un paso que lo comprueba (`git -C ~/codebase/0xJesus/ComandOS diff -- bin/cc-dash lib | grep <función>`). Las 6 rutas de la barra de comandos (GET y POST `/proxy`, POST `/harness/switch`, POST `/model/switch-cancel`, GET `/session-config-history`, POST `/session/recover`): si la fusión llegó a `main`, la Tarea 2f-2/T6 las pasa a `retired.rs` (410) con su prueba; si no, las porta.
- **D9 — `POST /remote-off` no ejecuta `tailscale serve reset`.** El Python lo hace (`remote_dashboard_off`, 4997) y hoy borraría los servicios de otros proyectos en 8444–8447 (inventario §9.5). El frente apaga solo lo que el tablero configuró (`--https=443 off` y `--https=8443 off`, más `cc-webterm off`). Efecto distinto a propósito; el cuerpo de respuesta es `remote_state()` posterior, así que `serveStatus` mostrará los servicios ajenos que el Python habría borrado. Diferencia aceptada y documentada en el cutover.
- **D10 — Web Push sigue «no disponible».** `pywebpush` no está instalado en el Python del sistema (`python3 -c 'import pywebpush'` → `ModuleNotFoundError`): `/push/key`, POST/DELETE `/push/subscription` y `/push/test` responden hoy `503` con `Push no disponible: falta pywebpush (ModuleNotFoundError); ver requirements-push.txt` y el bucle de envío no hace nada. El frente reproduce exactamente eso. Portar VAPID/RFC 8291 es otra fase, si alguien lo pide.
- **D11 — `cc-notifyd` en un binario GTK aparte.** La Enmienda 4 lo dejaba para la Fase 4 para mantener `comandos` sin GTK. Este plan lo adelanta (lo pide el alcance de la 2f) **como crate y binario propios** (`crates/comandos-notifyd`, `comandos-notifyd`), así `comandos` sigue headless; adelanta también la validación de gtk3-rs que el §8 pedía en la Fase 4. Es independiente de todo lo demás: si el controlador prefiere respetar la Enmienda 4, 2f-5 se aplaza sin tocar el resto.
- **D12 — Trabajo largo en tareas del frente.** Las operaciones de sesión (espera hasta 45 min, verificación hasta 30 s), los `send-keys` diferidos 1,5 s y los agentes de noticias corren en `tokio::spawn` registradas en `Native.tasks: TaskTracker` (Tarea 1). Al apagar el frente se abandonan como el Python abandona sus hilos `daemon`: las operaciones quedan con dueño muerto y `recover_abandoned` las marca en el siguiente arranque (comportamiento ya portado en la 2c). No se reintenta nada.

## Inventario: rutas que siguen reenviadas tras 2c/2d/2e

Base: ramas de `Handler._do_GET`/`do_POST`/`do_DELETE` de `bin/cc-dash` (`0aa4ae1`), menos lo nativo en `crates/comandos-server/src/dash/native/*` (`main`, 2b/2c) y lo que hacen nativo la 2d (`/state`, `/workspace/sort` `by`, `/terminal/quick` `place:"sidebar"`) y la 2e (`/usage/state`, `/analytics/week`, `/accounts`, `/providers`, `/optimization/plans`, `/extension-usage`). Llamadores: `git grep` en `HEAD` de `dash/`, `bin/cc-app`, `bin/cc-app-mac`, `bin/cc-notifyd`, `bin/cc-next`, `bin/cc-webterm`, `hooks/`; «sucio» = árbol vivo del checkout principal cuando difiere (solo cambian números de línea salvo donde se indica).

| Método y ruta | Python (`0aa4ae1`) | Llamadores vivos (commit) | Árbol sucio | Corte · tarea |
|---|---|---|---|---|
| POST `/tab-register` | 9494 `register_app_tab` 5321 | `dash/index.html:4567` | igual | tabs · 2f-1/T2 |
| POST `/tab-metadata` | 9507 `write_tab_metadata` 5274 | `bin/cc-app-mac:665` | igual | tabs · 2f-1/T2 |
| POST `/tab-metadata-remove` | 9516 `remove_tab_metadata` 5289 | `bin/cc-app-mac:991,1287,1289` | igual | tabs · 2f-1/T2 |
| POST `/tab-close` | 9520 `close_app_tab` 5350 | `dash/index.html:4510` | igual | tabs · 2f-1/T2 |
| POST `/workspace/close-group` | 8716 `workspace_state.close_group` + `close_app_tab` | `dash/index.html:4520,4545`, `bin/cc-app:4335,4345` | igual | tabs · 2f-1/T2 |
| POST `/recover-tab` | 9531 `tmux_new_session` 5441 | `dash/index.html:5177` | igual | tabs · 2f-1/T3 |
| POST `/ensure` | 9704 | `dash/index.html:2062,2071`, `bin/cc-app:439`, `bin/cc-app-mac:929,1156,1266` | igual | tabs · 2f-1/T3 |
| POST `/new` | 9728 `scope_cmd` 5420 | `bin/cc-app:949,2874` | igual | tabs · 2f-1/T3 |
| POST `/up` | 9765 | `dash/index.html:2074,2082`, `bin/cc-next:49` | igual | tabs · 2f-1/T3 |
| POST `/shell` | 9749 `ensure_shell_window` | `dash/index.html:2080` | igual | tabs · 2f-1/T3 |
| POST `/session-new` | 9206 | `dash/index.html:2561` | igual | tabs · 2f-1/T4 |
| POST `/account/add` | 9411 | `dash/index.html:3171` | **cambia**: `account_add_request` (alias automático, 409, Codex), llamadores `dash/term.html:2256`, `bin/cc-app:5324` | tabs · 2f-1/T4 |
| POST `/terminal/quick` (fuera de la barra) | 9393 `quick_terminal_request` 5513 | `bin/cc-app:3459` (sin `place`) | igual | tabs · 2f-1/T5 |
| POST `/ssh-connect` | 9121 `ssh_connect` 7744 | `dash/index.html:5670`, `bin/cc-app:2865`, `bin/cc-app-mac:1143` | igual | tabs · 2f-1/T5 |
| POST `/ssh-new-tab` | 9128 `ssh_open_new_tab` 7795 | `dash/index.html:5677`, `bin/cc-app:2867`, `bin/cc-app-mac:1149` | igual | tabs · 2f-1/T5 |
| POST `/send` | 9625 | `dash/index.html:3513`, `bin/cc-notifyd:426` | igual | tabs · 2f-1/T6 |
| POST `/paste` | 9637 `snippet_paste_to_pane` 5724 | `dash/index.html:6243` | igual | tabs · 2f-1/T6 |
| POST `/key` | 9650 | `dash/index.html:3512`, `bin/cc-notifyd:406` | igual | tabs · 2f-1/T6 |
| POST `/focus` | 9696 `focus_session` 5533 | `dash/index.html:2080,2566,5672,5680,5688,6510`, `bin/cc-notifyd:440`, `bin/cc-next:46` | igual | tabs · 2f-1/T6 |
| POST `/kill` | 9684 | `dash/index.html:4748` | igual | tabs · 2f-1/T6 |
| POST `/tmux-scroll` | 9487 `tmux_scroll` 5850 | `dash/term.html:1128`, `bin/cc-app:708` | igual | tabs · 2f-1/T6 |
| POST `/export` | 9669 `export_response` 7904 | `bin/cc-app:1081` | igual | tabs · 2f-1/T6 |
| POST `/terminal-panes` `action:"close"` | 8771 `save_closed_pane_snapshot` 5684 | `dash/term.html`, `dash/index.html:6500,6503`, `bin/cc-app:4365,4371,8843,8847` | igual | tabs · 2f-1/T7 |
| POST `/account/switch` | 9566 `account_switch_configuration` 2703 → `session_configure` 3336 | `dash/index.html:3157`, `dash/term.html:2273`, `bin/cc-app:5323` | igual | ops · 2f-2/T3 |
| POST `/session/configure` | 9566 | `dash/index.html:2548`, `dash/session-controls.js:64` | `dash/index.html:2404` | ops · 2f-2/T3 |
| POST `/model/switch` | 9586 `model_switch_apply` 7942 | `dash/index.html:2690,3152,3229,3514` | `dash/index.html:2546,2921` | ops · 2f-2/T3 |
| POST `/harness/switch` | 9566 | `dash/index.html:3123,3213` | **sin llamador** | ops · 2f-2/T6 (D8) |
| POST `/model/switch-cancel` | 9578 `model_switch_cancel` 3484 | `dash/index.html:3137`, `dash/session-controls.js:103` | **sin llamador** | ops · 2f-2/T6 (D8) |
| POST `/session/recover` | 8758 `session_recover` 3387 | `dash/session-controls.js:203` | **sin llamador** | ops · 2f-2/T6 (D8) |
| GET `/session-config-history` | 8250 `session_config_history` 2137 | `dash/session-controls.js:20` | **sin llamador** | ops · 2f-2/T6 (D8) |
| GET `/proxy` | 8333 | `dash/index.html:2727,3196` | **sin llamador** | ops · 2f-2/T6 (D8) |
| POST `/proxy` | 8802 `proxy_set_enabled` 3439, `motor_set_global` 3682 | `dash/index.html` (mismas) | **sin llamador** | ops · 2f-2/T6 (D8) |
| GET `/model/status` (casos que la 2c declina) | 8325 `refresh_session_confirmation` 3300 | `dash/index.html:2743`, `dash/term.html:2258`, `bin/cc-app:5273` | igual | ops · 2f-2/T3 |
| GET `/pane-extensions` (prefijo) | 8258 `pane_extensions_state` 2202 | `dash/extensions.js:51` | igual | ops · 2f-2/T4 |
| POST `/pane-extensions`, `/apply`, `/template`, `/cancel`, `/recover` | 8659 `pane_extensions_write` 2245 | `dash/extensions.js:71` | igual | ops · 2f-2/T4 |
| GET `/session-profiles` | 8348 | `dash/workspace.js:47` | igual | ops · 2f-2/T5 |
| POST `/session-profiles` | 8829 | `dash/workspace.js:97` | igual | ops · 2f-2/T5 |
| POST `/session-profile-apply` | 8840 | `dash/workspace.js:108` | igual | ops · 2f-2/T5 |
| GET `/webterm-token` (prefijo) | 8262 `access_token` 4730 | `dash/index.html:2106`, `bin/cc-app:3554`, `bin/cc-app-mac:200` | igual | services · 2f-3/T1 |
| GET `/conf` (prefijo) | 8468 `read_conf` 4819 | `dash/index.html:5905,6106`, `bin/cc-app:180`, `bin/cc-app-mac:119` | igual | services · 2f-3/T1 |
| POST `/conf-set` | 8819 `write_conf_key` 7322 | `dash/index.html` (7 sitios) | igual | services · 2f-3/T1 |
| GET `/fs/dirs` (prefijo) | 8410 `fs_dirs` 620 | `dash/index.html:2456` | igual | services · 2f-3/T1 |
| POST `/fs/mkdir` | 9080 `fs_mkdir` 658 | `dash/index.html:2480` | igual | services · 2f-3/T1 |
| POST `/open-path` | 8781 `_open_url` 34 | `dash/index.html:2199` | igual | services · 2f-3/T1 |
| POST `/open-url` | 9067 | `dash/index.html:3352,3399,3503` | igual | services · 2f-3/T1 |
| POST `/notify-popup` | 8684 `desktop_popup` 475 | `dash/index.html:2516,2527` | igual | services · 2f-3/T1 |
| POST `/test` | 8915 `play_test` 7287 | `dash/index.html:6056,6059,6062` | igual | services · 2f-3/T1 |
| GET `/remote-state` (prefijo) | 8567 `remote_state_cached` 4938 | `dash/index.html:3858,5606` | igual | services · 2f-3/T2 |
| GET `/remote-qr.png` (prefijo) | 8569 | `dash/index.html:5544` | igual | services · 2f-3/T2 |
| POST `/remote-on`, `/remote-off`, `/remote-webterm-on`, `/remote-webterm-off` | 8924–8953 | `dash/index.html:5659-5662` | igual | services · 2f-3/T2 |
| GET `/ssh` (prefijo) | 8466 `parse_ssh_config` 7344 | `dash/index.html:5709` | igual | services · 2f-3/T3 |
| POST `/ssh-add`, `/ssh-del`, `/ssh-update` | 8955–8971 | `dash/index.html:5891,5739,5890` | igual | services · 2f-3/T3 |
| POST `/ssh-key-setup` | 9135 `ssh_key_setup` 7510 | `dash/index.html:5685` | igual | services · 2f-3/T3 |
| GET `/commands/catalog` (prefijo) | 8604 `cli_catalog_payload` 1533 | `dash/command-sidebar.js:248,249`, `dash/index.html:4920` | igual | services · 2f-3/T4 |
| GET y POST `/chains` | 8615, 9141 (`lib/command_chains.py`) | `dash/chain-builder.js:134`, `dash/command-sidebar.js:250,297` | igual | services · 2f-3/T4 |
| GET `/opencode/models` (prefijo) | 8284 `opencode_models` 4711 | `dash/index.html:2635` | igual | services · 2f-3/T4 |
| GET `/push/key`, POST y DELETE `/push/subscription`, POST `/push/test` | 8391, 9051, 8632, 9053 | `dash/push-settings.js:72,77,88,98,110` | igual | services · 2f-3/T5 |
| POST `/pomodoro` | 8817 `pomodoro_post` 6674 | `dash/pomodoro.js:71,89,298,602`, `dash/index.html:3417`, `bin/cc-app:6497` | igual | services · 2f-3/T5 |
| GET `/models/latest` (prefijo) | 8407 | `dash/index.html:2498,3414` | igual | services · 2f-3/T6 |
| GET `/news/latest` (prefijo) | 8389 | `dash/index.html:3415` | igual | news · 2f-4/T1 |
| GET `/news/editions`, `/news/edition`, `/news/media/<32hex>.<ext>`, `/news/source`, `/news/chat`, `/news/notes`, `/news/saved` | 8393–8406 | `dash/news-reader.js` | igual | news · 2f-4/T1 |
| POST `/news/saved`, `/news/notes`, `/news/chat/note` | 9056 `news_post` 906 | `dash/news-reader.js` | igual | news · 2f-4/T2 |
| POST `/news/chat`, `/news/translate` | 9056 | `dash/news-reader.js:619,649,1003` | igual | news · 2f-4/T3 |
| Residuo: GET con prefijo de una ruta nativa (`/stateX`, `/prefs/…`, `/tabsX`…), GET `/operator*`, POST/DELETE desconocidos, GET/HEAD de algo que no es un archivo regular | `_do_GET` (`super().do_GET()`, 8618), `do_POST` (9780), `do_DELETE` (8634) | — | igual | residue · 2f-3/T7 |

Hilos de fondo de `cc-dash` (`main`, 9788): `motor_queue_resume` al arrancar (→ 2f-2/T2), `_model_watch_loop` (→ 2f-3/T6), `_news_editions_loop` (→ 2f-4/T4), `_notices_push_loop` (inerte, D10 → 2f-3/T5), `_limits_snapshot_loop` (la 2e lo dejó en el Python, D3 de la 2e → 2f-3/T6), `start_pomodoro_scheduler` (→ 2f-3/T5), `restore_requested_webterm` (→ 2f-3/T2). Hilos efímeros por petición: cubiertos por la tarea de su ruta.

`bin/cc-notifyd` (1 139 líneas, `POST /notify` en 4778 y popups GTK) → 2f-5.

## Review Focus

Cinco condiciones transversales que ninguna prueba de dominio cubre sola; cada una tiene su prueba en la tarea que la posee:

1. **Un corte a medias**: con `COMANDOS_DASH_CUTS_OFF=tabs`, ninguna ruta de 2f-1 escribe nada desde el frente aunque esté en la tabla, y el resto del nativo sigue. Prueba `cuts_off_declines_whole_cut_before_effects` (Tarea 1).
2. **El censo bajo carga**: miles de declinaciones de rutas distintas no hacen crecer la memoria sin tope (máximo 512 claves; el resto cuenta en `"(otras)"`) y el archivo siempre es JSON válido aunque se mate el proceso a mitad de escritura. Prueba `census_bounded_and_atomic` (Tarea 1).
3. **Un hijo `spawn_detached` que nunca termina** (un `xdg-open` colgado) no deja zombis ni retiene el runtime; uno que termina enseguida tampoco. Prueba `detached_children_are_reaped` (Tarea 1).
4. **El gemelo diverge por el reloj**: dos lados que crean `term-r<n>` en segundos distintos se comparan igual tras normalizar, pero una diferencia real en el resto del cuerpo sigue fallando. Prueba `twin_normalizes_clock_but_not_content` (Tarea 2).
5. **Tras la 2f, la clasificación nunca reenvía**: para cada literal de ruta de `_do_GET`/`do_POST`/`do_DELETE` y un muestreo de prefijos y métodos, `classify_with(native=true)` da `Native` o `Static`. Prueba `no_route_classifies_as_forward` (tarea final; requiere 2f-1…2f-4 fusionados).

## Grupos de independencia

| Grupo | Tareas | Puede empezar cuando | Comparte con otros grupos |
|---|---|---|---|
| G0 (serie) | Maestro T0 → T1 → T2 → T3 | 2c/2d/2e en `main` | crea todo lo compartido |
| G1 | 2f-1 T1→T2, T1→T3→T4→T5 en serie; T6 y T7 en paralelo con el resto del grupo | G0 | `tmux_snapshot`, `target`, `ssh_config` (T3 del maestro) |
| G2 | 2f-2 T1→T2→T3; T4 y T6 tras T3; T5 independiente (solo G0) | G0 | `tmux_snapshot`, `target`; lee `app-tabs*.json` en solo lectura |
| G3a | 2f-3 T1, T3, T4 (mutuamente independientes) | G0 | nada |
| G3b | 2f-3 T2 (remoto; usa `access_token` de T1) | G0 y 2f-3 T1 | nada |
| G3c | 2f-3 T5 → T6 (T6 usa el `BackgroundRunner` de T5 y la caché de catálogo de T4) | G0 (T6 además 2f-3 T4) | `NotifyPost`, `LimitsCache` de la 2e |
| G4 | 2f-4 T1 → T2 → T3 → T4 (T4 además tras 2f-3 T6) | G0 | `NotifyPost` de la 2e |
| G5 | 2f-5 T1 → T2 → T3 | inmediatamente (no depende de G0) | nada |
| G6 (serie, al final) | 2f-3 T7 (residuo) → maestro TF (cutover) | G1–G4 fusionados | todos |

G1, G2, G3a, G3b, G3c, G4 y G5 pueden ir en worktrees paralelos. Dentro de G3a las tres tareas también.

## Estructura de archivos (tareas del maestro)

```
crates/comandos-server/src/dash/mod.rs           (DashConfig.{background, cuts_off}, flags, censo)   T1
crates/comandos-server/src/dash/native/
  mod.rs        (Verb::Delete, Key::Prefix, NativeRoute nuevos, Cut, TABLES, answer,
                 Native.{census, tasks}, NativeOptions.{background, cuts_off})                     T0, T1
  tabs.rs, sessions.rs, input.rs                (stubs; los llena 2f-1)                            T0
  ops/mod.rs                                    (stub; lo llena 2f-2)                              T0
  remote.rs, ssh.rs, settings.rs, catalog_cli.rs, push.rs, residue.rs, background/mod.rs
                                                (stubs; los llena 2f-3)                            T0
  news/mod.rs                                   (stub; lo llena 2f-4)                              T0
  census.rs     (DeclineCensus)                                                                    T1
  target.rs     (PostTarget, pane_identity, resolve_project_session)                               T3
  procs.rs      (run_program_input, spawn_detached, gui_env, TaskTracker)                          T1
  files.rs      (FileLock::acquire)                                                                T1
crates/comandos-runtime/src/tmux_snapshot.rs     (capture_session)                                  T3
crates/comandos-runtime/src/closed_panes.rs      (save_closed_pane_snapshot)                        T3
crates/comandos-runtime/tests/tmux_snapshot_oracle.rs                                               T3
crates/comandos-runtime/src/ssh_config.rs        (parse, read, host_entry, is_host)                 T3
crates/comandos-server/tests/support/mod.rs      (pub mod twin; pub mod <dominio> stubs)            T0, T2
crates/comandos-server/tests/support/twin.rs     (Twin, normalize, fakebin con systemd-run)         T2
crates/comandos-server/tests/support/{tabs,ops,services,news}.rs  (vacíos; los llena cada dominio)  T0
crates/comandos-server/tests/dash_native_kit.rs, dash_twin.rs                                       T1, T2
xtask/src/parity.rs                              (--fixture repetible, setup, files)                T2
xtask/parity/2f/{tabs,ops,services,news,residue}.jsonl  (vacíos)                                    T0
docs/verification/cutover-dash.md                (sección «2f»)                                     TF
```

---

### Task 0: Andamio para trabajar en paralelo

Sin cambio de comportamiento: crea los módulos de dominio vacíos y los cabos de pruebas y fixtures para que los grupos G1–G4 nunca editen los mismos archivos.

**Files:**
- Modify: `crates/comandos-server/src/dash/native/mod.rs`
- Create: `crates/comandos-server/src/dash/native/{tabs,sessions,input,remote,ssh,settings,catalog_cli,push,residue}.rs`, `crates/comandos-server/src/dash/native/ops/mod.rs`, `crates/comandos-server/src/dash/native/news/mod.rs`, `crates/comandos-server/src/dash/native/background/mod.rs`
- Modify: `crates/comandos-server/tests/support/mod.rs`
- Create: `crates/comandos-server/tests/support/{tabs,ops,services,news}.rs`, `xtask/parity/2f/{tabs,ops,services,news,residue}.jsonl`

**Interfaces:**
- Consumes: `native::{Answer, Entry, Fault, Key, Native, NativeRoute, Verb}` (2b).
- Produces: `Verb::Delete`; `Key::Prefix(&'static str)` (= `self.path.startswith(p)` sobre la ruta cruda); `NativeRoute::{Tabs(tabs::TabsRoute), Sessions(sessions::SessionsRoute), Input(input::InputRoute), Ops(ops::OpsRoute), Remote(remote::RemoteRoute), Ssh(ssh::SshRoute), Settings(settings::SettingsRoute), Cli(catalog_cli::CliRoute), Push(push::PushRoute), News(news::NewsRoute), Residue(residue::ResidueRoute)}`; cada módulo con `pub enum XRoute {}`, `pub const ROUTES: &[Entry] = &[];` y `pub async fn answer(native: &Native, route: XRoute, request: &Request) -> Answer`; `native::Cut::{Base, Tabs, Ops, Services, News, Residue}` y `NativeRoute::cut(self) -> Cut`.

- [ ] **Step 1: Prueba que falla**

En `crates/comandos-server/src/dash/native/mod.rs`, dentro de un `#[cfg(test)] mod scaffold_tests` nuevo al final:

```rust
#[cfg(test)]
mod scaffold_tests {
    use super::*;

    #[test]
    fn delete_and_prefix_keys_match_like_python() {
        assert!(Key::Prefix("/state").matches("/state"));
        assert!(Key::Prefix("/state").matches("/stateful?x=1"));
        assert!(!Key::Prefix("/state").matches("/stat"));
        assert_eq!(route(&Method::DELETE, "/no-existe"), None);
    }

    #[test]
    fn every_route_has_a_cut() {
        assert_eq!(NativeRoute::Retired.cut(), Cut::Base);
        assert_eq!(NativeRoute::PaneType.cut(), Cut::Base);
    }
}
```

- [ ] **Step 2: Ver el fallo**

Run: `$C test -p comandos-server --lib scaffold_tests`
Expected: FAIL de compilación (`no variant Prefix`, `no Cut`).

- [ ] **Step 3: Implementar**

En `Key` añadir la variante y su rama de `matches`:

```rust
    /// `self.path.startswith(p)` del Python sobre la ruta cruda (con consulta):
    /// el residuo del despachador (2f-3/T7) reclama así `/stateX`, `/prefs/…`.
    Prefix(&'static str),
```

```rust
            Key::Prefix(p) => target.starts_with(p),
```

En `Verb` añadir `Delete` y en `route`:

```rust
    let verb = if *method == Method::GET {
        Verb::Get
    } else if *method == Method::POST {
        Verb::Post
    } else if *method == Method::DELETE {
        Verb::Delete
    } else {
        return None;
    };
```

Declarar los módulos nuevos junto a los existentes (`pub mod background; pub mod catalog_cli; pub mod input; pub mod news; pub mod ops; pub mod push; pub mod remote; pub mod residue; pub mod sessions; pub mod settings; pub mod ssh; pub mod tabs;`), añadir las variantes a `NativeRoute`, y el corte:

```rust
/// Grupo de rutas que comparte un estado con un solo dueño (D2 del plan 2f):
/// se activa y se revierte entero con `COMANDOS_DASH_CUTS_OFF`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Cut {
    /// Rutas de 2b–2e: solo `COMANDOS_DASH_NATIVE=0` las apaga.
    Base,
    Tabs,
    Ops,
    Services,
    News,
    Residue,
}

impl Cut {
    pub fn parse(name: &str) -> Option<Cut> {
        match name.trim() {
            "tabs" => Some(Cut::Tabs),
            "ops" => Some(Cut::Ops),
            "services" => Some(Cut::Services),
            "news" => Some(Cut::News),
            "residue" => Some(Cut::Residue),
            _ => None,
        }
    }
}

impl NativeRoute {
    pub fn cut(self) -> Cut {
        match self {
            NativeRoute::Tabs(_) | NativeRoute::Sessions(_) | NativeRoute::Input(_) => Cut::Tabs,
            NativeRoute::Ops(_) => Cut::Ops,
            NativeRoute::Remote(_)
            | NativeRoute::Ssh(_)
            | NativeRoute::Settings(_)
            | NativeRoute::Cli(_)
            | NativeRoute::Push(_) => Cut::Services,
            NativeRoute::News(_) => Cut::News,
            NativeRoute::Residue(_) => Cut::Residue,
            _ => Cut::Base,
        }
    }
}
```

Añadir las tablas nuevas **al final** de `TABLES`, con `residue::ROUTES` la última (sus prefijos no deben tapar entradas exactas), y las ramas en `answer` (`NativeRoute::Tabs(route) => tabs::answer(self, route, request).await,` y análogas). Cada stub, p. ej. `tabs.rs`:

```rust
//! Corte `tabs` (plan 2f-1): registro de pestañas y sus escritores. Vacío
//! hasta la Tarea 2f-1/T2.
use super::{Answer, Entry, Native};
use crate::Request;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabsRoute {}

pub const ROUTES: &[Entry] = &[];

pub async fn answer(_native: &Native, route: TabsRoute, _request: &Request) -> Answer {
    match route {}
}
```

`ops/mod.rs`, `news/mod.rs` y `background/mod.rs` igual (`background/mod.rs` sin rutas: solo `//! Hilos de fondo (2f-3/T5–T6).`). En `tests/support/mod.rs` añadir `pub mod tabs; pub mod ops; pub mod services; pub mod news;` (archivos con `#![allow(dead_code)]` y un comentario). Crear los cinco `.jsonl` vacíos.

- [ ] **Step 4: Ver que pasa y que nada cambió**

Run: `$C test -p comandos-server` y `$C clippy --workspace --all-targets -j 6 -- -D warnings`
Expected: PASS; las pruebas existentes de clasificación (`router`, `dash_boot`) siguen en verde.

- [ ] **Step 5: Commit**

```bash
git add crates/comandos-server/src/dash/native/mod.rs crates/comandos-server/src/dash/native/tabs.rs \
  crates/comandos-server/src/dash/native/sessions.rs crates/comandos-server/src/dash/native/input.rs \
  crates/comandos-server/src/dash/native/remote.rs crates/comandos-server/src/dash/native/ssh.rs \
  crates/comandos-server/src/dash/native/settings.rs crates/comandos-server/src/dash/native/catalog_cli.rs \
  crates/comandos-server/src/dash/native/push.rs crates/comandos-server/src/dash/native/residue.rs \
  crates/comandos-server/src/dash/native/ops/mod.rs crates/comandos-server/src/dash/native/news/mod.rs \
  crates/comandos-server/src/dash/native/background/mod.rs crates/comandos-server/tests/support/mod.rs \
  crates/comandos-server/tests/support/tabs.rs crates/comandos-server/tests/support/ops.rs \
  crates/comandos-server/tests/support/services.rs crates/comandos-server/tests/support/news.rs
git add -f xtask/parity/2f/tabs.jsonl xtask/parity/2f/ops.jsonl xtask/parity/2f/services.jsonl \
  xtask/parity/2f/news.jsonl xtask/parity/2f/residue.jsonl
git commit -m "feat(dash): andamio 2f — módulos por corte, Verb::Delete, Key::Prefix y fixtures por dominio

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 1: Kit común — procesos, candado con espera, censo, cortes y dueño de fondo

**Files:**
- Create: `crates/comandos-server/src/dash/native/census.rs`, `crates/comandos-server/src/dash/native/procs.rs`
- Modify: `crates/comandos-server/src/dash/native/files.rs`, `crates/comandos-server/src/dash/native/mod.rs`, `crates/comandos-server/src/dash/mod.rs`
- Create: `crates/comandos-server/tests/dash_native_kit.rs`

**Interfaces:**
- Consumes: `tmux::{Program, Output, RunError, run_program}` (2b), `files::FileLock` (2c).
- Produces:
  - `procs::run_program_input(program: &Program, args: &[&str], input: &[u8], timeout: Duration) -> Result<Output, RunError>` (= `subprocess.run(..., input=..., text=True, capture_output=True, timeout=...)`).
  - `procs::spawn_detached(program: &Program, args: &[OsString], env: &[(OsString, OsString)]) -> io::Result<()>` (= `subprocess.Popen(..., start_new_session=True, stdout=DEVNULL, stderr=DEVNULL)`; el hijo se recoge en una tarea).
  - `procs::gui_env() -> Vec<(OsString, OsString)>` (= `gui_env()` 4837: `DISPLAY` por omisión `:1`).
  - `procs::TaskTracker` (`spawn(fut)`, `len()`), campo `Native.tasks: Arc<TaskTracker>`.
  - `files::FileLock::acquire(path: &Path) -> io::Result<FileLock>` (`flock(LOCK_EX)` con espera, solo desde `spawn_blocking`).
  - `census::DeclineCensus` (`note(&Method, &str)`, `snapshot() -> serde_json::Value`, `flush(&Path) -> io::Result<()>`), campo `Native.census: Arc<DeclineCensus>`.
  - `native::Background { pomodoro, model_watch, news, limits_snapshot, notices_push, webterm_restore: bool }` con `Background::legacy()` y `Background::front()`; `NativeOptions.{background: Background, cuts_off: BTreeSet<Cut>}`; `DashConfig.{background, cuts_off}`; flags `--background=legacy|front`, `--cuts-off=a,b`; entornos `COMANDOS_DASH_BACKGROUND`, `COMANDOS_DASH_CUTS_OFF`.

- [ ] **Step 1: Pruebas que fallan**

Crear `crates/comandos-server/tests/dash_native_kit.rs`:

```rust
//! Kit 2f: procesos sueltos, censo de declinaciones y cortes desactivados.
mod support;

use comandos_server::dash::native::{
    Cut, census::DeclineCensus,
    procs::{run_program_input, spawn_detached},
    tmux::Program,
};
use http::Method;
use std::{ffi::OsString, time::Duration};
use support::{FakeLegacy, TestHome, front, request_body};

#[tokio::test]
async fn input_reaches_stdin_like_subprocess_run() {
    let out = run_program_input(&Program::named("cat"), &[], "a\r\nb".as_bytes(), Duration::from_secs(2))
        .await
        .unwrap();
    assert!(out.ok);
    assert_eq!(out.stdout, "a\nb"); // saltos universales, como text=True
}

#[tokio::test]
async fn detached_children_are_reaped() {
    let home = TestHome::new("kit-detached");
    let mark = home.root.join("mark");
    for _ in 0..20 {
        spawn_detached(
            &Program::named("sh"),
            &[OsString::from("-c"), OsString::from(format!("echo x >> {}", mark.display()))],
            &[],
        )
        .unwrap();
    }
    spawn_detached(&Program::named("sleep"), &[OsString::from("30")], &[]).unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    // Runtime current_thread: los hijos cuelgan del hilo principal (tid = pid).
    let pid = std::process::id();
    let children = std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")).unwrap_or_default();
    let zombies = children
        .split_whitespace()
        .filter(|child| {
            std::fs::read_to_string(format!("/proc/{child}/stat"))
                .ok()
                .and_then(|stat| stat.rsplit_once(") ").map(|(_, rest)| rest.starts_with('Z')))
                .unwrap_or(false)
        })
        .count();
    assert_eq!(zombies, 0, "hijos sin recoger: {children}");
    // Solo el `sleep 30` sigue vivo y no retiene el runtime (esta prueba terminó de esperar).
    assert!(children.split_whitespace().count() <= 1, "hijos vivos: {children}");
    assert_eq!(std::fs::read_to_string(&mark).unwrap().lines().count(), 20);
}

#[test]
fn census_bounded_and_atomic() {
    let home = TestHome::new("kit-census");
    let census = DeclineCensus::default();
    for i in 0..2000 {
        census.note(&Method::GET, &format!("/ruta-{i}?q=1"));
    }
    census.note(&Method::GET, "/state?x=1");
    census.note(&Method::GET, "/state");
    let snap = census.snapshot();
    let counts = snap["counts"].as_object().unwrap();
    // 512 rutas con 1; las 1488 distintas siguientes y las dos de /state, a «(otras)».
    assert_eq!(counts.len(), 513);
    assert!(!counts.contains_key("GET /state"));
    assert_eq!(counts["(otras)"].as_u64(), Some(1490));
    assert_eq!(counts["GET /ruta-0"].as_u64(), Some(1));
    let file = home.root.join("declines.json");
    census.flush(&file).unwrap();
    let back: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(back["counts"], snap["counts"]);
}

#[tokio::test]
async fn cuts_off_declines_whole_cut_before_effects() {
    let home = TestHome::new("kit-cuts");
    let legacy = FakeLegacy::start().await;
    let mut opts = home.options();
    opts.cuts_off.insert(Cut::Tabs);
    let fr = front(&home, legacy.port, opts).await;
    // Ruta del corte `tabs` aún sin entrada: igual se reenvía; la prueba fija el
    // contrato para cuando 2f-1/T2 la añada (ver dash_native_tabs.rs).
    let wire = request_body(fr.port, "POST", "/tab-register", "Content-Type: application/json\r\n",
        r#"{"session":"s1","label":"x"}"#).await;
    assert_eq!(wire.text(), r#"{"legacy": true}"#);
    assert!(!home.hooks().join("app-tab-open.json").exists());
    fr.stop().await;
}
```

Y en `native/mod.rs` (`scaffold_tests`), la parte unitaria del corte:

```rust
    #[test]
    fn cut_names_parse() {
        assert_eq!(Cut::parse(" ops"), Some(Cut::Ops));
        assert_eq!(Cut::parse("base"), None);
    }
```

- [ ] **Step 2: Ver el fallo**

Run: `$C test -p comandos-server --test dash_native_kit`
Expected: FAIL de compilación (`no module census`, `no procs`).

- [ ] **Step 3: Implementar `procs.rs`**

```rust
//! Procesos del frente que no son tmux: entrada por stdin y lanzamientos
//! sueltos (`Popen(..., start_new_session=True)`), y el registro de tareas
//! largas del frente (D12 del plan 2f).
use super::tmux::{Output, Program, RunError};
use std::{
    ffi::OsString,
    future::Future,
    io,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::io::AsyncWriteExt;

fn command(program: &Program, args: &[OsString]) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(&program.path);
    cmd.args(&program.prefix).args(args);
    for key in &program.env_remove {
        cmd.env_remove(key);
    }
    for (key, value) in &program.env {
        cmd.env(key, value);
    }
    cmd
}

fn universal(bytes: Vec<u8>) -> Result<String, RunError> {
    let text = String::from_utf8(bytes).map_err(|_| RunError::Decode)?;
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// `subprocess.run(args, input=text, capture_output=True, text=True, timeout=…)`.
pub async fn run_program_input(
    program: &Program,
    args: &[&str],
    input: &[u8],
    timeout: Duration,
) -> Result<Output, RunError> {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    let mut cmd = command(program, &args);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(RunError::Spawn)?;
    let mut stdin = child.stdin.take();
    let feed = async {
        if let Some(pipe) = stdin.as_mut() {
            // Un hijo que cierra stdin antes de leerlo todo no es un error del Python.
            let _ = pipe.write_all(input).await;
        }
        drop(stdin.take());
    };
    let run = async {
        feed.await;
        child.wait_with_output().await
    };
    let output = tokio::time::timeout(timeout, run)
        .await
        .map_err(|_| RunError::Timeout)?
        .map_err(RunError::Spawn)?;
    Ok(Output {
        ok: output.status.success(),
        stdout: universal(output.stdout)?,
        stderr: universal(output.stderr)?,
    })
}

/// `subprocess.Popen(args, env=…, start_new_session=True, stdout=DEVNULL,
/// stderr=DEVNULL)`: no se espera; una tarea recoge al hijo para no dejar zombis.
pub fn spawn_detached(
    program: &Program,
    args: &[OsString],
    env: &[(OsString, OsString)],
) -> io::Result<()> {
    let mut cmd = command(program, args);
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = cmd.spawn()?;
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(())
}

/// `gui_env()` del Python: el entorno del proceso con `DISPLAY=:1` si falta.
pub fn gui_env() -> Vec<(OsString, OsString)> {
    if std::env::var_os("DISPLAY").is_some() {
        Vec::new()
    } else {
        vec![(OsString::from("DISPLAY"), OsString::from(":1"))]
    }
}

/// Tareas largas del frente (operaciones de sesión, `send-keys` diferidos,
/// agentes de noticias). Se abandonan al apagar, como los hilos `daemon`.
#[derive(Default)]
pub struct TaskTracker {
    running: Arc<AtomicUsize>,
}

impl TaskTracker {
    pub fn spawn<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let running = Arc::clone(&self.running);
        running.fetch_add(1, Ordering::AcqRel);
        tokio::spawn(async move {
            fut.await;
            running.fetch_sub(1, Ordering::AcqRel);
        });
    }

    pub fn len(&self) -> usize {
        self.running.load(Ordering::Acquire)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
```

`process_group(0)` es el `setsid` de `start_new_session` a efectos de señales del terminal (la sesión del frente no tiene terminal: equivalente). Comprobar que `tokio::process::Command::process_group` existe en tokio 1.53 (sí, unix).

- [ ] **Step 4: Implementar `FileLock::acquire` y `census.rs`**

En `files.rs`, junto a `try_acquire`:

```rust
    /// `fcntl.flock(fd, LOCK_EX)` con espera: el `file_lock` del Python. Solo
    /// desde `spawn_blocking` (bloquea hasta que el otro dueño lo suelte).
    pub fn acquire(path: &Path) -> io::Result<FileLock> {
        let lock = lock_file_for(path)?;
        lock.lock()?;
        Ok(FileLock { file: lock })
    }
```

(`lock_file_for` es la apertura que ya usa `try_acquire`; si se llama distinto, reutilizar esa función y no duplicar la apertura.)

`census.rs`:

```rust
//! Censo de declinaciones (D7 del plan 2f): qué pide aún el Python y cuántas
//! veces. Acotado a 512 claves; escritura atómica.
use super::files::write_text_atomic;
use http::Method;
use serde_json::{Map, Value, json};
use std::{collections::BTreeMap, io, path::Path, sync::Mutex};

const MAX_KEYS: usize = 512;
const OTHERS: &str = "(otras)";

pub struct DeclineCensus {
    since_ms: i64,
    counts: Mutex<BTreeMap<String, u64>>,
}

impl Default for DeclineCensus {
    fn default() -> Self {
        Self {
            since_ms: super::wall_clock_ms(),
            counts: Mutex::new(BTreeMap::new()),
        }
    }
}

impl DeclineCensus {
    pub fn note(&self, method: &Method, target: &str) {
        let path = target.split_once('?').map_or(target, |(path, _)| path);
        let key = format!("{method} {path}");
        let mut counts = self.counts.lock().unwrap_or_else(|p| p.into_inner());
        let full = counts.len() >= MAX_KEYS && !counts.contains_key(&key);
        let slot = if full { OTHERS.to_string() } else { key };
        *counts.entry(slot).or_insert(0) += 1;
    }

    pub fn snapshot(&self) -> Value {
        let counts = self.counts.lock().unwrap_or_else(|p| p.into_inner());
        let map: Map<String, Value> = counts.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
        json!({"since": self.since_ms, "counts": map})
    }

    pub fn flush(&self, file: &Path) -> io::Result<()> {
        let text = serde_json::to_string(&self.snapshot()).map_err(io::Error::other)?;
        write_text_atomic(file, &text)
    }
}
```

La clave `(otras)` cuenta dentro del límite: con 512 claves llenas, la 513.ª distinta va a `(otras)`; por eso la prueba admite 513.

- [ ] **Step 5: Cortes, fondo y censo en `Native` y `DashConfig`**

En `native/mod.rs`:

```rust
/// Qué hilos de fondo arranca el frente (D6 del plan 2f).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Background {
    pub pomodoro: bool,
    pub model_watch: bool,
    pub news: bool,
    pub limits_snapshot: bool,
    pub notices_push: bool,
    pub webterm_restore: bool,
}

impl Background {
    /// El Python sigue vivo: solo lo idempotente con él.
    pub const fn legacy() -> Self {
        Self { pomodoro: true, model_watch: false, news: false, limits_snapshot: false,
               notices_push: false, webterm_restore: false }
    }
    /// 2g: el frente es el único dueño.
    pub const fn front() -> Self {
        Self { pomodoro: true, model_watch: true, news: true, limits_snapshot: true,
               notices_push: true, webterm_restore: true }
    }
}
```

`NativeOptions` gana `pub background: Background` (`for_home`: `Background::legacy()`) y `pub cuts_off: std::collections::BTreeSet<Cut>` (vacío). `Native` gana `pub(crate) census: Arc<census::DeclineCensus>` y `pub(crate) tasks: Arc<procs::TaskTracker>` (con `Default`). En `dispatch`, antes de `self.answer`:

```rust
        if self.opts.cuts_off.contains(&route.cut()) {
            self.census.note(&request.method, &request.target);
            return Ok(Outcome::Decline);
        }
```

y en la rama `Err(Fault::Decline)` y en la de `!self.ready()` también `self.census.note(…)`. Exponer `pub fn census(&self) -> &census::DeclineCensus`.

En `dash/mod.rs`: `DashConfig.{background: Background, cuts_off: BTreeSet<Cut>}`; `parse_args` acepta `--background=legacy|front` (otro valor → error de uso, salida 2 como `--legacy-port`) y `--cuts-off=a,b` (nombre desconocido → error); si no hay flag, leer `COMANDOS_DASH_BACKGROUND` y `COMANDOS_DASH_CUTS_OFF` con la misma validación. `build` copia ambos a `NativeOptions` (también cuando las pruebas inyectan opciones, como `legacy` en la 2d). Tras arrancar el servidor, una tarea escribe el censo cada 60 s en `std::env::var_os("XDG_RUNTIME_DIR")` + `comandos-dash-declines.json` (sin `XDG_RUNTIME_DIR`, no escribe) y una vez más en el apagado ordenado.

- [ ] **Step 6: Ver que pasa**

Run: `$C test -p comandos-server --test dash_native_kit` y `$C test -p comandos-server --lib`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/comandos-server/src/dash/native/census.rs crates/comandos-server/src/dash/native/procs.rs \
  crates/comandos-server/src/dash/native/files.rs crates/comandos-server/src/dash/native/mod.rs \
  crates/comandos-server/src/dash/mod.rs crates/comandos-server/tests/dash_native_kit.rs
git commit -m "feat(dash): kit 2f — procesos sueltos, flock con espera, censo de declinaciones, cortes y dueño de fondo

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Gemelo de pruebas para mutaciones y arnés con preparación y archivos

**Files:**
- Create: `crates/comandos-server/tests/support/twin.rs`, `crates/comandos-server/tests/dash_twin.rs`
- Modify: `crates/comandos-server/tests/support/mod.rs`, `crates/comandos-server/tests/support/oracle.rs`
- Modify: `xtask/src/parity.rs`, `xtask/parity/README.md`

**Interfaces:**
- Consumes: `support::{TestHome, front, request_body, FakeLegacy}`, `support::oracle::oracle` (2b/2c).
- Produces:
  - `TestHome::tmux_command(&self) -> std::process::Command`, `support::run_tmux(&TestHome, &[&str]) -> String`, `support::assert_private_tmux(&NativeOptions)`, `support::oracle::tmux_guard(real: &Path, socket: &Path) -> String`.
  - `support::oracle::oracle_with(home: &TestHome, fakebin_extra: &[(&str, &str)]) -> Option<Oracle>` (`fakebin_extra`: nombre → texto de un ejecutable generado en el `fakebin` del HOME temporal).
  - `support::twin::{Twin, TwinRun, normalize(&str) -> String, SCOPE_RUNNER}`: `Twin::start(tag, seed: impl Fn(&TestHome)) -> Option<Twin>`; `Twin::post(&self, path, body) -> TwinRun`; `TwinRun { front: Wire, oracle: Wire }`; `Twin::files_equal(&self, rel: &[&str]) -> Result<(), String>`; `Twin::tmux_a/tmux_b(&self, args) -> String`.
  - `xtask parity`: `--fixture` repetible; campos de línea `"setup": [[…args de tmux…], …]` (se ejecutan en el tmux privado de cada lado antes de la línea) y `"files": ["app-tabs.json", …]` (relativos a `~/.claude/hooks` o empezando por `~/`; se comparan tras la línea con la misma normalización de `normalize`).

- [ ] **Step 1: Prueba que falla**

Crear `crates/comandos-server/tests/dash_twin.rs`:

```rust
//! El gemelo compara mutaciones sobre dos HOME sembrados igual.
mod support;

use support::twin::normalize;

#[test]
fn twin_normalizes_clock_but_not_content() {
    let a = r#"{"session": "term-r12345", "ts": 1791115200.123, "label": "x"}"#;
    let b = r#"{"session": "term-r12399", "ts": 1791115201.9, "label": "x"}"#;
    assert_eq!(normalize(a), normalize(b));
    let c = r#"{"session": "term-r12399", "ts": 1791115201.9, "label": "y"}"#;
    assert_ne!(normalize(a), normalize(c));
    assert_eq!(
        normalize(r#"{"closedAt": 1.5, "name": "1791115200123456789-0f3a.json"}"#),
        normalize(r#"{"closedAt": 2.5, "name": "1791115299123456789-aaaa.json"}"#)
    );
}
```

- [ ] **Step 2: Ver el fallo**

Run: `$C test -p comandos-server --test dash_twin`
Expected: FAIL de compilación (`no module twin`).

- [ ] **Step 3: Implementar `twin.rs`**

```rust
//! Gemelo (D5 del plan 2f): dos HOME sembrados igual, cada uno con su tmux
//! privado; el frente muta A y el `cc-dash` Python muta B. Se comparan bytes
//! y archivos tras normalizar lo que depende del reloj.
#![allow(dead_code)]
use super::{FakeLegacy, Front, TestHome, Wire, front, oracle::{Oracle, oracle_with}, request_body};
use regex::Regex;
use std::sync::OnceLock;

/// `systemd-run` falso que descarta sus opciones y ejecuta el resto, para que
/// `scope_cmd` del Python cree de verdad la sesión en el tmux privado.
/// La cola del Python es `tmux …` sin `-S`: el `PATH` del oráculo resuelve ese
/// `tmux` al guardián del `fakebin`, que añade el socket privado. La cola del
/// frente ya trae `tmux -f /dev/null -S <socket>` (`Tmux::private`).
pub const SCOPE_RUNNER: &str = "#!/bin/sh\nwhile [ \"${1#--}\" != \"$1\" ]; do shift; done\nexec \"$@\"\n";

pub struct Twin {
    pub a: TestHome,
    pub b: TestHome,
    pub front: Front,
    pub oracle: Oracle,
    _legacy: FakeLegacy,
}

pub struct TwinRun {
    pub front: Wire,
    pub oracle: Wire,
}

impl TwinRun {
    pub fn assert_same(&self) {
        assert_eq!(self.front.status, self.oracle.status, "status");
        assert_eq!(normalize(&self.front.text()), normalize(&self.oracle.text()), "cuerpo");
    }
}

pub fn normalize(text: &str) -> String {
    static RULES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    let rules = RULES.get_or_init(|| {
        [
            (r"term-r[0-9]+", "term-rN"),
            (r#""(ts|closedAt|updated|at|heartbeatAt)": [0-9.eE+-]+"#, r#""$1": T"#),
            (r"[0-9]{19}-[0-9a-f]+\.json", "NS-ID.json"),
        ]
        .into_iter()
        .filter_map(|(re, to)| Regex::new(re).ok().map(|re| (re, to)))
        .collect()
    });
    rules
        .iter()
        .fold(text.to_string(), |acc, (re, to)| re.replace_all(&acc, *to).into_owned())
}

impl Twin {
    pub async fn start(tag: &str, seed: impl Fn(&TestHome)) -> Option<Twin> {
        if !super::tmux_available() {
            eprintln!("tmux no está instalado: se salta");
            return None;
        }
        let a = TestHome::new(&format!("{tag}-a"));
        let b = TestHome::new(&format!("{tag}-b"));
        seed(&a);
        seed(&b);
        let oracle = oracle_with(&b, &[("systemd-run", SCOPE_RUNNER)]).await?;
        let legacy = FakeLegacy::start().await;
        let mut opts = a.options();
        opts.scope = Some(scope_program(&a));
        let front = front(&a, legacy.port, opts).await;
        Some(Twin { a, b, front, oracle, _legacy: legacy })
    }

    pub async fn post(&self, path: &str, body: &str) -> TwinRun {
        let headers = "Content-Type: application/json\r\n";
        TwinRun {
            front: request_body(self.front.port, "POST", path, headers, body).await,
            oracle: request_body(self.oracle.port, "POST", path, headers, body).await,
        }
    }

    pub fn files_equal(&self, rel: &[&str]) -> Result<(), String> {
        for name in rel {
            let path = |home: &TestHome| match name.strip_prefix("~/") {
                Some(rest) => home.root.join(rest),
                None => home.hooks().join(name),
            };
            let read = |home: &TestHome| std::fs::read_to_string(path(home)).ok().map(|t| normalize(&t));
            if read(&self.a) != read(&self.b) {
                return Err(format!("{name}: frente {:?} ≠ oráculo {:?}", read(&self.a), read(&self.b)));
            }
        }
        Ok(())
    }

    pub fn tmux_a(&self, args: &[&str]) -> String {
        super::run_tmux(&self.a, args)
    }

    pub fn tmux_b(&self, args: &[&str]) -> String {
        super::run_tmux(&self.b, args)
    }
}

/// El frente usa el mismo `systemd-run` falso (escrito por `oracle_with` en el
/// `fakebin` de su HOME) para que ambos lados creen la sesión igual.
fn scope_program(home: &TestHome) -> comandos_server::dash::native::tmux::Program {
    let path = home.root.join("fakebin/systemd-run");
    let _ = std::fs::create_dir_all(home.root.join("fakebin"));
    let _ = std::fs::write(&path, SCOPE_RUNNER);
    let _ = std::process::Command::new("chmod").arg("755").arg(&path).status();
    comandos_server::dash::native::tmux::Program::named(path)
}
```

`opts.scope` es el campo `NativeOptions.scope: Option<Program>` que añade la 2d (Tarea 6, `systemd-run` con prefijo `--user --scope --collect --quiet` según R1 del pre-flight). Si en `main` se llama distinto, usar el real. En `support/mod.rs` se añaden:

```rust
impl TestHome {
    /// `tmux -f /dev/null -S <tmux_dir>/tmux-<uid>/default`, sin `TMUX`: el ÚNICO
    /// modo de hablar con tmux en las pruebas (regla de `CLAUDE.md`).
    pub fn tmux_command(&self) -> Command {
        let socket = private_socket(&self.tmux_dir());
        assert!(
            socket.parent().is_some_and(|dir| dir.is_dir()),
            "socket tmux privado sin directorio: se negaría a caer en el servidor real"
        );
        let mut cmd = Command::new("tmux");
        cmd.args(["-f", "/dev/null", "-S"]).arg(socket).env_remove("TMUX");
        cmd
    }
}

/// Ejecuta tmux en el servidor privado de `home`; la prueba falla si tmux falla.
pub fn run_tmux(home: &TestHome, args: &[&str]) -> String {
    let out = home.tmux_command().args(args).output().unwrap();
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// Opciones del frente que solo pueden tocar el tmux privado.
pub fn assert_private_tmux(opts: &NativeOptions) {
    let has_socket = |p: &Program| p.prefix.iter().any(|a| a == "-S");
    assert!(has_socket(&opts.tmux.program), "opts.tmux sin -S");
}
```

`TestHome::new` crea el directorio del socket con `Tmux::private(&root.join("tmux"))` (que lo deja en 0700) antes de devolver. `support::front` llama a `assert_private_tmux(&opts)` antes de servir. El `tmux` de `dash_native_terminal.rs` pasa a usar `run_tmux` (mismo comportamiento, socket explícito). `support/mod.rs` añade `pub mod twin;` y `regex` va a `[dev-dependencies]` de `comandos-server` (`regex = "=1.13.1"`, ya en `Cargo.lock`).

En `oracle.rs`: extraer el cuerpo de `oracle` a `oracle_with(home, fakebin_extra)`, que tras crear los enlaces a `/bin/true` escribe **siempre** el `tmux` guardián y después cada `(nombre, texto)` de `fakebin_extra` con permisos 0755 (sustituyendo el enlace si existía); `oracle(home)` = `oracle_with(home, &[])`. El guardián se genera con la ruta absoluta del tmux real (resuelta una vez con `which tmux` fuera del `fakebin`) y el socket privado de ese HOME:

```rust
/// `tmux` del `fakebin`: el Python del oráculo llama a `tmux` sin `-S`; este
/// envoltorio lo fuerza y se niega si el directorio del socket no existe.
pub fn tmux_guard(real_tmux: &Path, socket: &Path) -> String {
    let dir = socket.parent().map(|d| d.display().to_string()).unwrap_or_default();
    format!(
        "#!/bin/sh\n[ -d '{dir}' ] || {{ echo 'tmux guardián: sin socket privado' >&2; exit 97; }}\nexec '{}' -S '{}' \"$@\"\n",
        real_tmux.display(),
        socket.display()
    )
}
```

Una prueba en `dash_twin.rs`, `tmux_guard_refuses_missing_socket_dir`, escribe el guardián para un directorio inexistente, lo ejecuta con `list-sessions` y comprueba salida 97 sin haber lanzado tmux (no hay ningún `kill-*` en la prueba).

- [ ] **Step 4: Arnés: `--fixture` repetible, `setup` y `files`**

En `xtask/src/parity.rs`: `Args.fixture: Vec<PathBuf>` (cada `--fixture` añade; sin ninguno, error de uso como hoy); las líneas se leen en el orden de los archivos. `Line` gana `setup: Vec<Vec<String>>` y `files: Vec<String>` (`#[serde(default)]`). Antes de enviar una línea con `setup`, se ejecuta cada lista con `private_tmux(<tmux_dir de cada lado>)` (socket explícito `-S`; el arnés ya lo usa para sus sesiones). Una lista cuyo primer elemento sea `kill-server` se rechaza al cargar el fixture (error de uso): el arnés nunca mata servidores por fixture. Tras la respuesta, cada nombre de `files` se lee en los dos HOME y se compara tras aplicar las mismas tres reglas de `normalize` (copiar la función; el arnés no depende de las pruebas). Una diferencia de archivo cuenta como `DIFF` con el nombre del archivo en la salida. Documentar ambos campos en `xtask/parity/README.md` con un ejemplo:

```json
{"name":"t-tab-register","method":"POST","path":"/tab-register","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"p2f","label":"Proyecto"},"setup":[["new-session","-d","-s","p2f"]],"files":["app-tabs.json","app-tabs-meta.json"],"volatile":[],"expect":"same"}
```

Añadir a `xtask/src/parity.rs` una prueba unitaria `setup_and_files_fields_parse` que deserializa esa línea.

- [ ] **Step 5: Ver que pasa**

Run: `$C test -p comandos-server --test dash_twin` y `$C test -p xtask`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/comandos-server/tests/support/twin.rs crates/comandos-server/tests/support/mod.rs \
  crates/comandos-server/tests/support/oracle.rs crates/comandos-server/tests/dash_twin.rs \
  crates/comandos-server/Cargo.toml Cargo.lock xtask/src/parity.rs xtask/parity/README.md
git commit -m "feat(dash): gemelo de pruebas para mutaciones y parity con setup de tmux y comparación de archivos

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: `tmux_snapshot`, copias de paneles cerrados y preámbulo de destino de los POST

Lo usan 2f-1 (T3–T6) y 2f-2 (T1–T4). Sin ruta propia. Va en G0 para que los dos grupos que lo consumen no lo escriban dos veces.

Comportamiento portado (`lib/tmux_snapshot.py`, 300 líneas; `save_closed_pane_snapshot` 5684):

- **`capture_session(tmux, session, inspector)`**: `list-windows -t =<s> -F` con el formato del Python (índice, id, nombre, layout, activa), por ventana `list-panes -t <id> -F` (id, índice, pid, ruta, comando, título, activo, ancho, alto, izquierda, arriba), `show-options -p -t <pane> -v @comandos-pane-key`, y por pane la inspección de `PaneInspector` (2d) fusionada **en el orden de claves del Python**. `_checked` → error `tmux <orden>: <stderr>` (`RuntimeError`).
- **`remap_layout`/`_layout_checksum`**: el árbol nativo con ids nuevos y su checksum de 16 bits (`csum = (csum >> 1) + ((csum & 1) << 15); csum += ord(c)`).
- **`save_closed_pane_snapshot(home, tmux, sess, pane)`**: captura; si el pane no está → `ValueError('El panel cambió antes de guardar la copia')`; `capture-pane -p -J -t <p> -S -2000` (fallo → `ValueError('No se pudo guardar el texto. El panel sigue abierto')`); carpeta `~/.local/state/comandos/closed-panes` 0700 (y `chmod` 0700 si ya existía); nombre `<time_ns>-<uuid4 hex>.json` creado con `O_CREAT|O_EXCL`, 0600; `json.dump(…, ensure_ascii=False)` = `comandos_core::json::response_dumps_unicode` (2c); `fsync`; borrar los que pasen de los 50 más nuevos (`sorted(glob('[0-9]*-*.json'), reverse=True)[50:]`, orden de cadenas). Devuelve el nombre.

**Files:**
- Create: `crates/comandos-runtime/src/tmux_snapshot.rs`, `crates/comandos-runtime/src/closed_panes.rs`
- Modify: `crates/comandos-runtime/src/lib.rs`
- Create: `crates/comandos-runtime/tests/tmux_snapshot_oracle.rs`

**Interfaces:**
- Consumes: `comandos_runtime::pane_snapshot::PaneInspector` (2d), `comandos_core::json::response_dumps_unicode` (2c), `comandos_runtime::terminal_panes::TmuxResult` (2c, el tipo de resultado que ya reciben los callbacks síncronos de tmux).
- Produces: `tmux_snapshot::capture_session(tmux: &mut impl FnMut(&[&str]) -> Result<TmuxResult>, session: &str, inspector: &mut PaneInspector) -> Result<Value>`; `tmux_snapshot::{remap_layout(&str, &BTreeMap<String,String>) -> String, layout_checksum(&str) -> u16}`; `closed_panes::save(home: &Path, tmux: &mut impl FnMut(&[&str]) -> Result<TmuxResult>, inspector: &mut PaneInspector, sess: &str, pane: &str, now_ns: i128, uuid_hex: &str, now_s: f64) -> Result<String>` (reloj y uuid inyectados).

- [ ] **Step 1: Prueba diferencial que falla**

Crear `crates/comandos-runtime/tests/tmux_snapshot_oracle.rs` con un tmux privado (directorio temporal propio; **toda** invocación con `-f /dev/null -S <dir>/tmux-<uid>/default` y sin `TMUX`, y un `kill-server` con ese mismo `-S` en el `Drop` antes de borrar el directorio; sesión `s1` con dos panes `cat` en una ventana y una segunda ventana): llama a `capture_session` con un callback que ejecuta `tmux -f /dev/null …` en ese socket y a `python3 -c` que inserta `lib/` en `sys.path`, define `tmux = lambda *a, timeout=5: subprocess.run(['tmux','-f','/dev/null','-S',SOCKET,*a], capture_output=True, text=True, timeout=timeout)` con el mismo socket explícito, y hace `print(json.dumps(tmux_snapshot.capture_session(tmux, 's1', pane_snapshot.PaneInspector())))`. Comparar el `Value` de Rust volcado con `response_dumps` contra la salida. Segundo caso: `remap_layout` y `layout_checksum` sobre tres layouts reales (`list-windows -F '#{window_layout}'`) contra el Python. Tercer caso: `closed_panes::save` dos veces con relojes fijos sobre un HOME temporal y 52 archivos previos: quedan 50, el JSON del registro es igual byte a byte al que escribe `pane_snapshot`/`save_closed_pane_snapshot` del Python con `time.time_ns`, `uuid.uuid4` y `time.time` sustituidos por los mismos valores (cargar `bin/cc-dash` con `SourceFileLoader` como `tests/test_agent_launch.py:load_dash_module`, en el entorno del oráculo de la Tarea 2: `fakebin` con el `tmux` guardián, `XDG_RUNTIME_DIR`, `TMUX_TMPDIR`). Esta prueba vive en `comandos-runtime`, que no tiene `TestHome`: define su propio `PrivateTmux { dir }` con `command()` (`-f /dev/null -S …`) y `Drop` (`kill-server` con `-S`, después `remove_dir_all`), y una copia de `tmux_guard`.

```rust
#[test]
fn checksum_matches_tmux() {
    // Layout real de tmux 3.2a: el checksum de 4 hex es el de la cadena tras la coma.
    let layout = "b25d,120x40,0,0{60x40,0,0,0,59x40,61,0,1}";
    let body = layout.split_once(',').map(|(_, b)| b).unwrap();
    assert_eq!(format!("{:04x}", comandos_runtime::tmux_snapshot::layout_checksum(body)), "b25d");
}
```

- [ ] **Step 2: Ver el fallo**

Run: `$C test -p comandos-runtime --test tmux_snapshot_oracle`
Expected: FAIL de compilación (`no module tmux_snapshot`).

- [ ] **Step 3: Implementar** el port línea a línea con las reglas de arriba; `layout_checksum`:

```rust
/// `_layout_checksum` de `lib/tmux_snapshot.py` (el mismo de `layout.c` de tmux).
pub fn layout_checksum(body: &str) -> u16 {
    body.chars().fold(0u16, |csum, c| {
        let rotated = (csum >> 1) | ((csum & 1) << 15);
        // `ord(c)` en tmux es un byte; los layouts son ASCII.
        rotated.wrapping_add(u16::from(u8::try_from(u32::from(c)).unwrap_or(0)))
    })
}
```

- [ ] **Step 4: Ver que pasa**

Run: `$C test -p comandos-runtime --test tmux_snapshot_oracle`
Expected: PASS (o aviso de salto sin tmux/python3).

- [ ] **Step 5: Commit**

```bash
git add crates/comandos-runtime/src/tmux_snapshot.rs crates/comandos-runtime/src/closed_panes.rs \
  crates/comandos-runtime/src/lib.rs crates/comandos-runtime/tests/tmux_snapshot_oracle.rs
git commit -m "feat(runtime): tmux_snapshot y copias de paneles cerrados portados de lib/tmux_snapshot.py

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```


#### Parte B: preámbulo de destino (`native/target.rs`)

El tramo común de `do_POST` (`bin/cc-dash` 9466–9565) que comparten `/send`, `/paste`, `/key`, `/focus`, `/kill`, `/ensure`, `/new`, `/shell`, `/up`, `/export` (2f-1) y `/session/configure`, `/account/switch`, `/harness/switch`, `/model/switch`, `/model/switch-cancel` (2f-2), más las funciones de identidad de pane que usan ambos sub-planes.

Comportamiento portado:

- **`find_project_dir(home, sess)`** (5409): `glob(~/codebase/*)` y luego `glob(~/codebase/*/*)` (orden de `read_dir`, sin ordenar; `glob` no devuelve nombres con punto inicial), el primer directorio cuyo `session_name(basename).lower() == sess.lower()` (`session_name` = `py::session_name` de la 2d: `[.:]`→`-`, 80 caracteres; `lower()` de Python: solo ASCII cierto; un nombre no ASCII → `Fault::Decline` antes de cualquier efecto).
- **`state_agent(state_dir, sess)`** (5428): primer `H/state/*.json` (orden de `glob`) parseable cuyo `session_name(project) == sess` → `agent or "claude"`; ninguno → `"claude"`. Registro `Unsure` → `Decline`.
- **`resolve_project_session(native, sess)`** (6183): literal; `agent_procs`, `agent_pane_maps` y `choose_agent_pane` de la 2d (`comandos_runtime::agent_procs`; si alguno es privado de `states/`, se extrae a `pub(crate)` sin cambiarlo y se dice en el commit); `session_labels` (2d `states::gather`, mismo trato). `display-message` con el plazo de 5 s de `tmux()`.
- **`pane_identity(native, sess, pane)`** (6763) y **`identity_key`** (6783): las 8 claves en ese orden, `server_start` = campo 22 de `/proc/<pid>/stat` tras el último `)`; errores `ValueError` con los textos del Python (`se necesita el panel exacto`, `el panel ya no existe`, `el panel no pertenece a esa sesión`) como `TargetError::Value(String)`.
- **`post_target(native, path, data)`**: valida `session` con `SESSION_RE` (`400 {"error": "Nombre de sesion invalido"}`); si la ruta no es de configuración, `resolve_project_session` sustituye la sesión; `target = "=" + sess`, `pane = "=" + sess + ":"`; `want = data.pane or resolved.pane`; si `PANE_RE` casa y `display-message -p -t <want> '#{pane_id}'` devuelve exactamente `want`, `pane = want`. Devuelve `PostTarget { sess, target, pane, resolved }`. Solo lecturas de tmux y archivos: declinar aquí sigue siendo «antes de efectos».

**Files (Parte B):**
- Create: `crates/comandos-server/src/dash/native/target.rs`; Modify: `crates/comandos-server/src/dash/native/mod.rs` (`pub mod target;`)
- Create: `crates/comandos-server/tests/dash_native_target.rs`

**Interfaces (Parte B):**
- Produces: `target::{PostTarget { sess: String, target: String, pane: String, resolved: Option<(String, String)> }, TargetError::{Value(String), Fault(Fault)}, find_project_dir(&Path, &str) -> Result<Option<PathBuf>, Fault>, state_agent(&Path, &str) -> Result<String, Fault>, resolve_project_session(&Native, &str) -> Result<Option<(String, String)>, Fault>, pane_identity(&Native, &str, &str) -> Result<Map<String, Value>, TargetError>, identity_key(&Map<String, Value>) -> String, post_target(&Native, &str, &Value) -> Result<PostTarget, Answer>, CONFIG_ROUTES: [&str; 4]}`.

**Confinamiento:** solo lecturas de tmux (`display-message`, `list-sessions`) contra `Tmux::private` de `TestHome::options()`; las sesiones de la prueba las crea `run_tmux` con `-S`; nada que mate o cree sesiones fuera de la siembra. **Efectos en vivo:** ninguno (solo lectura), mismas órdenes que el Python.

- [ ] **Step B1: Prueba diferencial que falla**

`dash_native_target.rs`: HOME con `~/codebase/Mi.Proyecto` y `~/codebase/grupo/otro`, `H/state/a.json` con `{"project": "Mi.Proyecto", "session": "s1", "pane": "%0"}` y una sesión privada `s1` (sembrada con `run_tmux(&home, &["new-session","-d","-s","s1","cat"])`). Casos: `find_project_dir("mi-proyecto")`, `find_project_dir("otro")`, `find_project_dir("nada")`, `state_agent("mi-proyecto")`, `resolve_project_session("mi-proyecto")` → `("s1", "%0")`, `pane_identity("s1","%0")` con `identity_key` y los tres errores. Cada caso se compara con `python3 -c` que carga `bin/cc-dash` con `SourceFileLoader` en el entorno del oráculo (`fakebin` con el `tmux` guardián del mismo socket) e imprime `json.dumps` del resultado (las excepciones como `{"error": str(e)}`).

- [ ] **Step B2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_target` · Expected: FAIL de compilación (`no module target`).

- [ ] **Step B3: Implementar** `target.rs` con las reglas de arriba; `post_target` así:

```rust
/// Rutas de configuración: el Python NO sustituye la sesión por la resuelta.
pub const CONFIG_ROUTES: [&str; 4] = ["/session/configure", "/account/switch", "/harness/switch", "/model/switch"];

pub async fn post_target(native: &Native, path: &str, data: &Value) -> Result<PostTarget, Answer> {
    let raw = data.get("session").and_then(Value::as_str).unwrap_or("");
    if !py::is_session(raw) {
        return Err(reply(StatusCode::BAD_REQUEST, &json!({"error": "Nombre de sesion invalido"})));
    }
    let resolved = resolve_project_session(native, raw).await.map_err(Err)?;
    let sess = match &resolved {
        Some((real, _)) if !CONFIG_ROUTES.contains(&path) => real.clone(),
        _ => raw.to_string(),
    };
    let target = format!("={sess}");
    let mut pane = format!("={sess}:");
    let want = match data.get("pane") {
        Some(Value::String(p)) if !p.is_empty() => p.clone(),
        Some(Value::Null) | None => resolved.as_ref().map(|(_, p)| p.clone()).unwrap_or_default(),
        // `str(data.get("pane"))` de un número o lista: nunca casa PANE_RE.
        Some(_) => String::new(),
    };
    if py::is_pane(&want) == Some(true) {
        let out = native.options().tmux.run(&["display-message", "-p", "-t", &want, "#{pane_id}"]).await;
        let same = matches!(&out, Ok(o) if o.stdout.trim() == want);
        if same {
            pane = want;
        }
    }
    Ok(PostTarget { sess, target, pane, resolved })
}
```

(`data.get("session", "")` del Python sin `str()`: un número llega a `SESSION_RE.match` y lanza `TypeError` → 500; reproducirlo: si `session` existe y no es cadena → `Err(Err(Fault::Error(HandlerError::Failure)))`. Un `pane` vacío con `resolved` cae al pane resuelto, como el `or` del Python. Un error de `tmux.run` en `display-message` es `TimeoutExpired`/fallo no capturado en el Python → `TmuxError::uncaught()`.)

- [ ] **Step B4: Ver que pasa** — Run: `$C test -p comandos-server --test dash_native_target` · Expected: PASS.

- [ ] **Step B5: Commit**

```bash
git add crates/comandos-server/src/dash/native/target.rs crates/comandos-server/src/dash/native/mod.rs \
  crates/comandos-server/tests/dash_native_target.rs
git commit -m "feat(dash): preámbulo de destino de los POST, identidad de pane y resolución de proyecto

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```


#### Parte C: lector de `~/.ssh/config` (`comandos_runtime::ssh_config`)

Lo consumen 2f-1/T5 (`/ssh-connect`, `/ssh-new-tab`) y 2f-3/T3 (altas, bajas, edición y `/ssh-key-setup`); va aquí para que esos dos grupos no dependan uno del otro.

- **`parse(text: &str) -> Vec<Map<String, Value>>`** = `parse_ssh_config(text)` (7344): `str.splitlines()` (`comandos_core::text::splitlines`), `strip()` de Python (`py::strip`, también U+001C–U+001F y U+0085), `split(None, 1)` (blancos de Python), clave en minúsculas (**solo** ASCII; una clave con letras no ASCII deja la entrada tal cual en Python —`lower()` Unicode—: se replica con `str::to_lowercase`, que coincide para las claves que importan, y cualquier otra clave se ignora igual), `Host` con varios nombres → una entrada por nombre sin `*` ni `?`; `hostname`, `user`, `port` → misma clave; `identityfile` → `identity`. Claves en orden de inserción.
- **`read(home: &Path) -> io::Result<Option<String>>`**: `~/.ssh/config` como texto; ausente → `None`; no UTF-8 → `Err(InvalidData)` (el `open().read()` del Python lanzaría `UnicodeDecodeError`: 500 en sus llamadores).
- **`host_entry(home, host) -> io::Result<Option<Map<String, Value>>>`** = `ssh_host_entry` (7487).
- **`SSH_HOST_RE`** (`^[A-Za-z0-9._-]{1,60}\Z`) como `is_host(&str) -> bool`.

**Files (Parte C):** Create `crates/comandos-runtime/src/ssh_config.rs`; Modify `crates/comandos-runtime/src/lib.rs`; Create `crates/comandos-runtime/tests/ssh_config_oracle.rs`.

- [ ] **Step C1: Prueba diferencial que falla** — diez textos (vacío, comentarios, `Host a b *.x`, claves en mayúsculas, tabuladores, `\r\n`, U+001F en una línea, `IdentityFile ~/.ssh/k`, `Host` sin valor, clave sin valor) pasan por `parse` y por `python3 -c` que inserta `bin/` en `sys.path`, carga `bin/cc-dash` con `SourceFileLoader` en el entorno del oráculo y hace `print(json.dumps(dash.parse_ssh_config(TEXT)))`; se comparan bytes con `response_dumps`.
- [ ] **Step C2: Ver el fallo** — Run: `$C test -p comandos-runtime --test ssh_config_oracle` · Expected: FAIL (`no module ssh_config`).
- [ ] **Step C3: Implementar** según las reglas.
- [ ] **Step C4: Ver que pasa** — misma orden · Expected: PASS.
- [ ] **Step C5: Commit**

```bash
git add crates/comandos-runtime/src/ssh_config.rs crates/comandos-runtime/src/lib.rs \
  crates/comandos-runtime/tests/ssh_config_oracle.rs
git commit -m "feat(runtime): lector de ~/.ssh/config portado de parse_ssh_config

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task F: Residuo cero, documento de cutover «2f» y lista de entrada de la 2g

Va al final (G6), con 2f-1…2f-4 y 2f-3/T7 fusionados en `migration/rust-fase2f`.

**Files:**
- Create: `crates/comandos-server/tests/dash_no_forward.rs`
- Modify: `crates/comandos-server/src/dash/native/mod.rs` (arranques y orden de `TABLES`), `docs/verification/cutover-dash.md`

**Interfaces:**
- Consumes: `comandos_server::dash::router::{classify_with, RouteClass}`.
- Produces: la sección «2f» del cutover y la lista «Declinaciones que heredan la 2g».

- [ ] **Step 0: Cablear los arranques y el orden de la tabla**

Los sub-planes exportan sus arranques sin llamarlos (D4: nadie más edita `native/mod.rs`). En `Native::start_background` (2e) añadir, después del refresco de límites existente y solo si `self.enabled()`:

```rust
// Arranques de la 2f. `background::start` respeta `Background` (D6): con
// `legacy` solo el planificador de Pomodoro; con `front`, todo.
background::start(self);              // 2f-3/T5: Pomodoro (siempre) y, con front, push inerte
background::models::start(self);      // 2f-3/T6: vigilante de modelos (front)
background::limits::start(self);      // 2f-3/T6: bucle de límites de 5 min (front)
remote::start(self);                  // 2f-3/T2: restaurar terminal web (front)
ops::start(self);                     // 2f-2/T3: motor_queue_resume (front)
news::scheduler::start(self);         // 2f-4/T4: ediciones (front)
```

y en `TABLES` poner `residue::ROUTES` (2f-3/T7, con su `Key::Prefix("/")`) **la última**, para que solo atienda lo que ninguna otra tabla casa. Prueba: `dash_native_kit.rs` gana `background_starts_follow_mode` (con `legacy`, solo la tarea de Pomodoro en el `TaskTracker`; con `front`, las seis).

- [ ] **Step 1: Prueba que falla (o pasa si todo está fusionado)**

```rust
//! Tras la 2f la clasificación nunca reenvía: solo `Decline` llega al Python.
use comandos_server::dash::router::{RouteClass, classify_with};
use http::Method;

const GET: &[&str] = &[
    "/operator", "/operator/x", "/session-config-history?session=s&pane=%250", "/pane-extensions?x=1",
    "/webterm-token", "/stateX", "/accounts", "/providers", "/optimization/plans", "/opencode/models",
    "/model-tiers", "/tab-models", "/active-tab", "/dedication", "/analytics/week", "/pomodoro/report",
    "/pomodoro", "/sovereignty", "/model/status", "/proxy", "/ui-log/summary", "/session-profiles",
    "/extension-usage", "/session-brain", "/usage/guard", "/usage/changes", "/notifs/count",
    "/news/latest", "/push/key", "/news/editions", "/news/media/0123456789abcdef0123456789abcdef.png",
    "/news/source", "/news/chat", "/news/notes", "/news/saved", "/news/edition?id=latest",
    "/models/latest", "/fs/dirs", "/usage/provider-compare", "/usage/experiments", "/usage/analytics",
    "/usage/interactions", "/usage/state", "/project-profiles", "/ssh", "/conf", "/prefs",
    "/tmux-mouse", "/workspace", "/notices/watch", "/notices", "/notices/prefs",
    "/workspace/close-group", "/workspace/client", "/tabs", "/tab-history", "/remote-state",
    "/remote-qr.png", "/work-marks", "/events/v2", "/events", "/commands/catalog", "/snippets",
    "/chains", "/no-existe.css", "/vendor/", "/assets",
];

const POST: &[&str] = &[
    "/push/subscription", "/operator", "/pane-extensions", "/pane-extensions/apply", "/workspace/sort",
    "/workspace", "/notify-popup", "/presence", "/workspace/close-group", "/work-marks", "/events/v2",
    "/workspace/client", "/session/recover", "/terminal-history", "/terminal-panes", "/open-path",
    "/proxy", "/ui-log", "/pomodoro", "/conf-set", "/session-profiles", "/session-profile-apply",
    "/test", "/remote-on", "/remote-off", "/remote-webterm-on", "/remote-webterm-off", "/ssh-add",
    "/ssh-del", "/ssh-update", "/prefs-set", "/push/test", "/news/saved", "/news/notes", "/news/chat",
    "/news/chat/note", "/news/translate", "/open-url", "/fs/mkdir", "/ssh-connect", "/ssh-new-tab",
    "/ssh-key-setup", "/chains", "/snippets", "/session-new", "/terminal/quick", "/account/add",
    "/tmux-mouse", "/tmux-scroll", "/tab-register", "/tab-metadata", "/tab-metadata-remove",
    "/tab-close", "/recover-tab", "/session/configure", "/account/switch", "/harness/switch",
    "/model/switch-cancel", "/model/switch", "/pane/type", "/send", "/paste", "/key", "/export",
    "/kill", "/focus", "/ensure", "/new", "/shell", "/up", "/ruta-que-no-existe",
];

#[test]
fn no_route_classifies_as_forward() {
    let exists = |_: &str| false;
    for path in GET {
        assert_ne!(classify_with(&Method::GET, path, &exists, true), RouteClass::Forward, "GET {path}");
        assert_ne!(classify_with(&Method::HEAD, path, &exists, true), RouteClass::Forward, "HEAD {path}");
    }
    for path in POST {
        assert_ne!(classify_with(&Method::POST, path, &exists, true), RouteClass::Forward, "POST {path}");
    }
    assert_ne!(classify_with(&Method::DELETE, "/x", &exists, true), RouteClass::Forward, "DELETE");
}
```

- [ ] **Step 2: Ejecutar**

Run: `$C test -p comandos-server --test dash_no_forward`
Expected: PASS con todo fusionado; cualquier fallo nombra la ruta que falta y la tarea que la debía.

- [ ] **Step 3: Escribir la sección «2f» en `docs/verification/cutover-dash.md`**

Con la misma forma que la 2c (Qué cambia · Diferencias aceptadas · 0 Previos · 1 Sombra · 2 Cutover · 3 Verificación por corte · 4 Reversión · Medido antes del cutover). Contenido obligatorio, con rutas explícitas:

- **0. Previos**: `NEW=$HOME/codebase/0xJesus/ComandOS/.build/target/release/comandos`, `XT=…/.build/target/debug/xtask`; `grep -qa COMANDOS_DASH_CUTS_OFF "$NEW" || echo "BINARIO SIN 2f: no seguir"`; las comprobaciones de la 2c (unidades `active active`, release actual, checkout de `~/.claude/hooks/dash/index.html`, `ExecStart` del heredado); el `git diff -U0 -- bin/cc-dash bin/cc-app dash lib` filtrado por las rutas de esta fase (lista del inventario de este plan) con la regla D8; comparar entre `cc-dash.service` y `cc-dash-legacy.service` el `PATH`, `WorkingDirectory`, `DISPLAY`, `XDG_RUNTIME_DIR`, `TZ` y las variables de la 2e (`systemctl --user show -p Environment …`); `command -v systemd-run tailscale qrencode ssh-copy-id wmctrl xdg-open pw-play` con el `PATH` del frente; `ss -ltn 'sport = :4782'` libre.
- **1. Sombra**: `COMANDOS_DASH_TRACE_FORWARD=1 "$NEW" dash 4782 --legacy-port 4781 2> /tmp/sombra-2f.log`; `"$XT" parity --fixture xtask/parity/frente.jsonl --fixture xtask/parity/2f/tabs.jsonl --fixture xtask/parity/2f/ops.jsonl --fixture xtask/parity/2f/services.jsonl --fixture xtask/parity/2f/news.jsonl --fixture xtask/parity/2f/residue.jsonl --hooks ~/.claude/hooks --state-db ~/.local/state/comandos/app-state.sqlite3 --usage-db ~/.claude/hooks/comandos-usage.sqlite --comandos "$NEW"` → 0 DIFF; `xtask poll --shadow --minutes 10` como en la 2c. La sombra comparte tmux, `~/.claude` y las bases reales: **solo** mutaciones sobre sesiones desechables con prefijo `comandos-e2e-` (las crea y cierra el propio procedimiento con `curl` contra 4782: `POST /new {"session":"comandos-e2e-2f"}`, `POST /tab-close {"session":"comandos-e2e-2f","ephemeral":true}`, `POST /kill`), nunca en pestañas o panes de agente, nunca un cambio de cuenta real; los cambios de cuenta se verifican en el paso 3. `POST /remote-*`, `POST /ssh-*` y `POST /conf-set` **no** se ejercitan en la sombra (efectos fuera del tablero; ya cubiertos por el gemelo). Al terminar: `grep -c 'se reenvían al heredado\|rutas nativas desactivadas' /tmp/sombra-2f.log` → 0 y `cat $XDG_RUNTIME_DIR/comandos-dash-declines.json` (el de la sombra se escribe en el mismo `XDG_RUNTIME_DIR`: borrarlo después).
- **2. Cutover**: nada en vuelo (la consulta del journal de la 2c; además ninguna `/session-new` ni `/account/add` en los últimos 2 min, comprobado con `ls -t ~/.claude/hooks/app-tab-open.json`); `"$NEW" install --stage`; humo del hook; `systemctl --user restart cc-dash.service`; tablero 200; `NRestarts` 0.
- **3. Verificación por corte**: traza de 10 min con `COMANDOS_DASH_TRACE_FORWARD=1`; criterio: **ninguna** línea `reenvío` salvo las declinaciones listadas abajo. Por corte, una acción real hecha por el usuario y su comprobación: `tabs` (abrir una pestaña con «+», cerrarla, recuperarla de Recientes; enfocar desde un popup), `ops` (un cambio de cuenta en vivo en un pane Claude **elegido por el usuario**, mirando que el journal pasa por `waiting → snapshot → applying → verifying → confirmed`, que el pane sigue en la misma conversación con `CLAUDE_CONFIG_DIR` de la cuenta nueva —y sin la variable si vuelve a `main`— y que `motor-results.json` tiene la entrada), `services` (abrir Remoto, ver el QR, conmutar un ajuste), `news` (abrir una edición, añadir y borrar una nota). Memoria e hilos como en la 2c.
- **4. Reversión**: por corte `systemctl --user set-environment COMANDOS_DASH_CUTS_OFF=ops && systemctl --user restart cc-dash.service` (y su drop-in persistente, copiable, como el de la 2c); todo el nativo con `COMANDOS_DASH_NATIVE=0`; release anterior con `--rollback-release`. Revertir `ops` con una operación en curso la deja con dueño muerto: `recover_abandoned` del Python la marca al primer `/model/status`.
- **Diferencias aceptadas (2f)**: D9 (`tailscale serve reset`), D10 (Web Push), los candados de proceso del Python que el frente no comparte (corte entero = dueño único; dentro de un corte activo solo se declina antes de leer), `motor-results.json` reescrito por el frente (el Python lo relee al reiniciar), y las de cada sub-plan.

- [ ] **Step 4: Escribir «Declinaciones que heredan la 2g»** (subsección de «2f»): una tabla con cada razón de `Decline` que queda en el código tras la 2f (buscar `Fault::Decline` en `crates/comandos-server/src/dash/native/`), su ruta, cuándo ocurre, si el censo la vio en la sombra y su sustituto en la 2g (p. ej. «candado tomado → esperar como el Python con `FileLock::acquire`», «JSON incierto → 500 como el `json.load` del Python», «carril apagado → 503 `{"error": …}`»). Más los pasos de la 2g: `--background=front`, parar `cc-dash-legacy.service`, decidir el destino de `Decline` sin heredado.

- [ ] **Step 5: Commit**

```bash
git add crates/comandos-server/tests/dash_no_forward.rs crates/comandos-server/src/dash/native/mod.rs \
  crates/comandos-server/tests/dash_native_kit.rs docs/verification/cutover-dash.md
git commit -m "docs(verification): procedimiento de cutover 2f y lista de declinaciones que hereda la 2g

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Self-review

- **Cobertura del alcance pedido**: mutaciones de sesión/tmux (2f-1), cuentas y modelos con interrupción y `CLAUDE_CONFIG_DIR` por cuenta (2f-2/T1, T3), harness y proxy (2f-2/T6), extensiones y noticias (2f-2/T4, 2f-4), SSH y remoto con tokens (2f-1/T4, 2f-3/T1–T3), `/conf` (2f-3/T1), `/chains`, `/commands/catalog`, `/models/latest`, `/opencode/models` (2f-3/T4, T6), perfiles (2f-2/T5), SSE/long-poll (no hay; D1), rutas de gestión de la terminal web (2f-3/T2), hilos de fondo (2f-3/T5–T6, 2f-4/T4, 2f-2/T2), `cc-notifyd` (2f-5). El bucle de 30 s es de `cc-app` (D1).
- **Rulings**: cada sub-plan repite en su cabecera los que le aplican y añade la tabla de idempotencia/orden por ruta mutadora (ruling 9).
- **Nombres compartidos**: `Cut`, `Background`, `procs::*`, `census::DeclineCensus`, `FileLock::acquire`, `support::twin::*`, `tmux_snapshot::*`, `closed_panes::save` se definen aquí y los sub-planes los consumen con esas firmas.
- **Placeholders**: los ports mecánicos se dan por regla y prueba diferencial (precedente 2d/2e); el código completo va en el diseño nuevo (kit, censo, cortes, gemelo, checksum).
