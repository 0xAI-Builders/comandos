# Fase 6 — Estado único, retirada de Python/bash/ttyd/JS propio y `comandos install` como único instalador: plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** cerrar la migración: (1) consolidar los ~80 archivos JSON/JSONL y los cinco SQLite de estado en un solo `~/.local/share/comandos/comandos.sqlite3` con un migrador idempotente, reanudable, con ensayo en seco, respaldos con hash y vuelta atrás probada; (2) portar las herramientas que ningún plan anterior porta (`cc-agents`, `cc-doctor`, `ccx`, `cc-acp`, `cc-mobile`, `cc-keys`, `cc-winstart`, `cc-next`, `cc-session-snapshot`, `cc-centro`, `cc-term`, `tools/*`); (3) hacer de `comandos install` el único instalador; (4) retirar del repo y del HOME todo Python, bash, ttyd y JS propio o de terceros entregado, artefacto por artefacto y con su sustituto verificado; (5) dejar `cargo xtask lint --strict` como prueba permanente de que el repo es solo Rust (más las excepciones de la spec §3.2).

**Architecture:** el estado pasa por cuatro modos **por dominio** (`legacy` → `mirror` → `unified` → `sealed`) guardados en la propia base; mientras un dominio está en `mirror` o `unified`, cada escritura llega a los dos formatos, de modo que cualquier release vieja (que solo lee los archivos) sigue viendo datos frescos y la vuelta atrás es inmediata. Los documentos que hoy se leen y escriben enteros se guardan como bytes exactos en una tabla `documents`; las colecciones (estado por pane, registros JSONL, procesos nativos, instantáneas de layout, órdenes a la app) tienen tabla propia; las bases SQLite actuales se trasladan tabla a tabla con un centinela de «movida» en la base vieja que las releases capaces siguen y las incapaces respetan cerrando su carril. La retirada de cada artefacto la decide `cargo xtask retire-check`, que comprueba en el HOME, en systemd, en las configuraciones de los agentes y en `/proc` que nada lo usa, y una lista de comprobación en datos (`docs/verification/retirement.json`).

**Tech Stack:** Rust 1.96 (edition 2024, `unsafe_code = "forbid"`), rusqlite 0.40.2 (`bundled`, `backup`), serde_json del workspace (`arbitrary_precision` + `preserve_order`), `sha2` del workspace, `nix = "=0.31.3"` (`fs` para `flock`), `getrandom` del workspace; sin dependencias nuevas salvo que una tarea lo diga con versión exacta.

**Spec:** `docs/superpowers/specs/2026-10-04-comandos-rust-complete-design.md` (§2 reglas de oro, §3.1 filas «install.sh…», «71 archivos JSON…», «173 tests Python», §3.2, §4.6, §5, §6, §7 fila 6, §8, Enmiendas 1, 3, 6 y 8). Inventario de componentes: `docs/rust-component-inventory.json` (375 entradas; 159 de producto). Planes previos cuyas **Interfaces** se consumen: `2026-10-04-fase-0-1-base-proxy-broker-hooks.md` (`install --stage/--link/--rollback`, releases), `2026-10-04-fase-2a-…` a `2026-10-04-fase-2e-…` (carriles SQLite, `files::{read_json_strict, write_text_atomic, FileLock}`, puerta de esquema por `user_version`/`schema_migrations`), `2026-10-04-fase-2f-resto-del-tablero.md` y sus sub-planes 2f-1…2f-5 (dueño único de `app-tabs*.json`, operaciones de sesión, servicios, noticias, `comandos-notifyd`; Task 3 `tmux_snapshot`), `2026-10-04-fase-3-term-y-web.md` (fin de ttyd, xterm.js, markdown-it, DOMPurify, uisfx y `dash/*.js` servidos; los archivos quedan en disco hasta esta fase), `2026-10-04-fase-4-app-gtk.md` (`comandos-app`, popups), `2026-10-04-fase-5-mac.md` (broker y clientes del navegador, `comandos-app-mac`). La 2g (apagar el Python del tablero) no tiene plan escrito al redactar este: se asume que al empezar la Fase 6 `cc-dash-legacy.service` está parado y el frente no reenvía nada (Etapa 0 lo comprueba).

**Precondición:** Fases 2 (incluida la 2g), 3 y 4 fusionadas y en vivo; Fase 5 Grupo B en vivo. Los grupos de esta fase que no tocan estado vivo (A, C, I-código, O) pueden empezar antes, en paralelo con las fases que falten; ver «Paralelismo».

---

## Hechos (leídos el 2026-10-04, solo lectura)

1. **Estado en `~/.claude/hooks/` (H):** 482 archivos en `state/`, 70 en `native-processes/`, 9 987 instantáneas de minuto en `app-sessions-v2.json.history/` (143 MB), 33 notas en `session-handoffs/`, y unos 45 archivos sueltos: `app-tabs.json` (532 B), `app-tabs-meta.json`, `app-tabs-history.json`, `app-tab-models.json`, `app-tab-active.json`, `app-tabs-snapshot.json`, `app-sessions-v2.json` (+`.bak`), `app-layout.json`, `app-pane-position.json`, `app-extension-shelf.json`, `app-focus.json`, `app-tab-open.json`, `app-tab-close.json` (y `app-command.json`, `app-tab-back.json` cuando existen), `events.jsonl` (84 KB), `ui-events.jsonl` (437 KB), `focus-queue.jsonl`, `snippets.json`, `prefs.json`, `model-watch.json`, `motor-queue.json`, `motor-results.json`, `news-editions.json`, `news-watch.json`, `notifyd-pos.json`, `pane-models.txt`, `pane-resume.json`, `session-effort.json`, `proxy-env-since.json`, `optimization-default.json`, `provider-quotas.json`, `provider-subs.json`, `groq-ratelimit.json`, `agy-quota.json`, `acp-panes.json`, `webterm-enabled`, `dash-token`, `cc-notify.conf`, `providers.env`, `telegram.env`, `operator/conversations.json`; bases `comandos-usage.sqlite` (172 MB + WAL), `session-operations.sqlite3` (560 KB), `news-history.sqlite` (112 KB), `operator/actions.sqlite` (12 KB) y `usage.db` (228 KB, **sin ningún lector** en `bin/`, `lib/` ni `crates/`). Restos: `md2tg.py` (enlace roto a `hooks/md2tg.py`, que ya no existe), `notify.sh`, `cc-status.sh.bak-20260914`, `.app-tab-models.json.*.tmp`, `*.bak*`, `backup-pre-restart-20260923/`, `backups-analytics-20261002-031419/`, `dash/` (enlaces a `<repo>/dash/*`).
2. **`~/.local/state/comandos/`:** `app-state.sqlite3` (6,4 MB, migraciones 1–11 en `schema_migrations`) con sus respaldos `pre-migration-*`/`pre-cleanup-*`; `extensions/` (`sizes`, `snapshot.json`, `skills.json`, `client-policies.json`, `last-check.json`, `backups/`, candados). `~/.local/share/comandos/`: `bin/comandos`, `releases/`, `rollback/*.target`, `extensions-venv/` (venv Python del proxy MCP antiguo).
3. **Choques de nombres de tabla** al juntar las bases: `events` existe en `app-state` (registro de eventos) y en `news-history.sqlite` (radar); `actions` en `operator/actions.sqlite`. Las tablas de uso son todas `usage_*` salvo `focus_blocks`, `focus_settings`, `model_presets`, `provider_cost_buckets`, `provider_usage_buckets`, que no chocan. La base de uso versiona con `PRAGMA user_version` (11, `crates/comandos-store/src/usage.rs:20`); `app-state` con la tabla `schema_migrations` (`crates/comandos-store/src/state_db/mod.rs:40`): **las dos marcas pueden convivir en un mismo archivo**.
4. **El Python de uso ignora versiones mayores**: `cc_usage.py:217-218` sale de la migración si `user_version ≥ 11` y sigue escribiendo; el Rust (2c/2e) apaga su carril si la versión es mayor. Por eso el traslado de una base exige que ningún proceso Python la abra (Etapa 0).
5. **Ejecutables sin plan de port en las fases 1–5** (búsqueda en `docs/superpowers/plans/2026-10-04-fase-*`): `cc-acp` (+`lib/acp.py`), `cc-agents`, `cc-doctor`, `cc-keys`, `cc-mobile`, `cc-winstart`, `ccx`, `cc-next`, `cc-session-snapshot`, `cc-centro`, `cc-term`, `cc-pane-model`, `install.sh`, `lib/platform.sh`, `lib/retire-telegram.sh`, `scripts/install-extensions.py`, `tools/*.py`, `tools/*.cjs`. En el checkout principal hay además `bin/cc-codex-full-access` y `lib/codex_{full_access,thread_release,yolo_install,yolo_policy}.py` **sin commitear** (otra sesión).
6. **Archivos de otro lenguaje dentro de `crates/` y `xtask/`**: oráculos Python y bash de paridad (`crates/comandos-runtime/tests/fixtures/hooks/oracle/**`, `crates/comandos-extensions/tests/*.py`, `crates/comandos-core/tests/notification_oracle.py`, fixtures de CPython en `crates/comandos-extensions/tests/fixtures/unicode14/upstream/`) y pruebas Rust que lanzan `python3` (`crates/*/tests/support/{python,oracle}.rs`, `xtask/src/parity.rs`).
7. **Archivos de producto en el repo** (`git ls-files`, 372 con extensión `.py/.sh/.js/.cjs/.mjs/.html`): `tests/` 209 (175 `.py`, 21 `.cjs`, 5 `.js`, 1 `.mjs`, 6 `.sh`), `lib/` 56, `bin/` 24, `dash/` 20 JS + 3 HTML + `vendor/` 2, `dash/prototypes/` 20, `assets/xterm` 6 JS, `assets/opentype`, `assets/uisfx`, `adapters/` 7, `hooks/` 3, `services/browser/` 2, `tools/` 8, `scripts/` 1, `design/` 2 HTML, `docs/research/extensiones-por-sesion/grok-ns.py`, `install.sh`.

## Rulings del controlador que fijan este plan

1. **Nunca se rompe ni se interrumpe una sesión viva.** Ningún paso reinicia tmux, `tmux.service`, la app ni un agente. Los daemons (`cc-dash`, `cc-notifyd`, broker MCP) se reinician solo con `systemctl --user restart <unidad>` por el controlador, como en las fases anteriores. Un dominio no cambia de modo mientras algún proceso que lo escriba corra una release que no lo entiende (preflight, Tarea S2).
2. **Todo es reversible con un comando** hasta `sealed`; `sealed` solo se aplica a un dominio tras 7 días en `unified` sin incidencias y con el OK de Jesús, y aun entonces `comandos state export-legacy` devuelve los archivos.
3. **Nada se borra del HOME**: lo que se retira se mueve a `~/.local/share/comandos/backups/<AAAAMMDD-HHMMSS>/` con un `manifest.json` de hashes. Del repo se borra con `git rm` solo cuando `xtask retire-check` está limpio para ese artefacto.
4. Comentarios en español, identificadores en inglés; sin `unsafe`, sin `unwrap`/`expect`/indexado con `[]` en código que no sea de prueba; `cargo clippy -D warnings`; `rustfmt`; versiones exactas `=x.y.z`; trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; **cero archivos Python, bash o JS nuevos** (los oráculos se pasan como texto a `python3 -c` o `bash -c` mientras existan los originales).
5. Los cutovers, las migraciones sobre el estado real, los `git rm` de artefactos vivos y toda lectura de `/proc` o del HOME real los ejecuta el **controlador**; los subagentes trabajan sobre HOME y `/proc` falsos.

## Global Constraints

- Todo se compila y prueba con
  `CARGO_TARGET_DIR=/home/someguy/codebase/0xJesus/ComandOS/.build/target nice -n 10 cargo <cmd> -j 6` (abreviado `$C <cmd>`). Antes de cada commit: `$C fmt --all -- --check`, `$C clippy --workspace --all-targets -j 6 -- -D warnings` y las pruebas de los paquetes tocados.
- Commits con `git add <rutas>` explícitas (o `git rm <rutas>` explícitas); mensajes `feat(store): …`, `feat(cli): …`, `feat(xtask): …`, `chore(retire): …`, `docs(verification): …` en español, línea en blanco y `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. El plan: `git add -f docs/superpowers/plans/2026-10-04-fase-6-retiro-y-estado-unico.md`.
- **tmux (regla de `CLAUDE.md` del repo, vinculante)**: todo tmux de prueba con socket explícito, `tmux -S <dir>/tmux-1000/default` o `-L <etiqueta-propia>`; **nunca** `TMUX_TMPDIR` solo (tmux 3.2a lo ignora si el directorio no existe y cae en `/tmp/tmux-1000/default`, el servidor de Jesús); prohibido `kill-server`, `kill-session`, `pkill tmux` o `kill -9 -1` sin `-S`/`-L` propio; primero `kill-server` sobre el socket propio y después borrar su directorio. En Rust `Tmux::private(dir)` (`crates/comandos-server/src/dash/native/tmux.rs`); en `xtask` `private_tmux(dir)`. Los ports de `ccx`, `cc-acp`, `cc-next` y `cc-session-snapshot` reciben el `Tmux` por parámetro y sus pruebas solo construyen el privado. El 2026-10-04 a las 21:59 un subagente mató las 20 sesiones vivas de Jesús por saltarse esta regla.
- Pruebas: nunca el HOME real, `~/.claude`, `~/.local/state`, `~/.local/share/comandos`, systemd, el tmux de Jesús, `/proc` real para decisiones (se inyecta `proc_root`), la red ni los puertos 4777–4782. Puertos fijos de prueba: banda de devhost 7200–7399.
- Ruta de la base única: `~/.local/share/comandos/comandos.sqlite3`, o `COMANDOS_DB` si está definida (la usan las pruebas y el ensayo). WAL, `busy_timeout = 5000`, `foreign_keys = ON`, modo `0600`, directorio `0700`.
- Los archivos que **se quedan como archivos** (D3) no se migran ni se borran: `cc-notify.conf`, `providers.env`, `dash-token`, `session-handoffs/*.md`, `config/*` del repo, `~/.config/comandos/extensions/{catalog,credentials}.json`, `rollback/*.target` y `releases/` de `~/.local/share/comandos`, y todo lo de `XDG_RUNTIME_DIR` (candados, `cc-browser-forwards`, censos).

## Review Focus

1. **Escritura concurrente durante el relleno inicial** (un hook escribe `state/x.json` mientras el migrador importa ese dominio): el dominio ya está en `mirror` antes del relleno, la escritura llega a los dos lados y el relleno no pisa una fila más nueva. Prueba `backfill_never_overwrites_newer_mirror_write` (Tarea S3).
2. **Corte de luz a mitad de la migración** (proceso muerto entre dominios o dentro de un traslado SQLite): `comandos state migrate --resume` continúa sin duplicar filas; una base vieja nunca queda marcada como movida sin que la nueva tenga sus datos. Pruebas `resume_after_kill_between_domains` y `move_crash_before_sentinel_is_redone` (Tareas S3, S4).
3. **Volver a una release anterior con dominios en `unified`**: `comandos install --rollback-release` degrada primero esos dominios a `mirror` (los archivos ya están al día por el espejo) y la release vieja ve exactamente los mismos datos. Prueba `rollback_release_demotes_unified_domains` y el ensayo `xtask state-drill` (Tarea S6).
4. **Borrar del repo un artefacto que un proceso vivo todavía usa** (un proxy Python `cc-extensions serve` de una sesión de hace días importa `lib/extension_proxy.py` en caliente; un enlace del HOME apunta a `bin/cc-webterm-attach`): `xtask retire-check` lo detecta y el borrado no ocurre. Prueba `retire_check_flags_live_importer_process` (Tarea R1).
5. **Base de uso de 172 MB bajo candado**: el traslado no bloquea a los hooks más que su `busy_timeout`; si el ensayo en seco mide más de 4 s de copia, el traslado se niega y pide una ventana sin agentes. Prueba `usage_move_refuses_when_estimate_exceeds_budget` (Tarea S4).

---

## Decisiones del plan (no fijadas por el controlador)

- **D1 — El archivo único extiende `app-state`.** `comandos.sqlite3` aplica las mismas migraciones 1–11 de `app-state` (mismo SQL de `crates/comandos-store/migrations/`) y después las nuevas desde la 100. Las tablas de uso se crean con el esquema v11 de `comandos_store::usage::ensure_schema` y la base lleva `user_version = 11` (la marca de uso), así que las puertas de esquema de las fases 2c/2e siguen funcionando sin cambio sobre el archivo nuevo. Choques (Hecho 3): `news-history.sqlite:events` → `news_radar_events`; `operator/actions.sqlite:actions` → `operator_actions`; el código que las lee se cambia en su tarea de dominio.
- **D2 — Documentos como bytes exactos.** Todo archivo que su único dueño lee y escribe entero (lista en «Mapa de fuentes») se guarda en `documents(name, body BLOB, revision, updated_at_ms)` con los bytes que el escritor produce. Así el espejo y `export-legacy` reproducen el archivo byte a byte y la verificación es comparar hashes. Normalizar esos documentos en columnas no cambia ningún comportamiento y añade riesgo; queda fuera de esta fase (YAGNI) y se puede hacer por dominio cuando esté `sealed`.
- **D3 — Lo que no se migra.** Secretos y configuración que Jesús edita a mano o que otros programas leen siguen siendo archivos: `providers.env` (lo carga `~/.zshrc` y `tmux_new_session`), `cc-notify.conf` (editable, documentado), `dash-token` (secreto `0600`), `session-handoffs/*.md` (documentos que se pasan a agentes), `~/.config/comandos/extensions/{catalog,credentials}.json` (configuración y secreto), los registros de instalación (`rollback/*.target`, `releases/`: la vuelta atrás tiene que funcionar aunque la base esté rota), `extension-launches/` (configuraciones privadas efímeras por lanzamiento), `extensions/backups/`, y todo lo de `XDG_RUNTIME_DIR`. `telegram.env`, `md2tg.py`, `notify.sh` y `usage.db` se archivan (D9).
- **D4 — Modos por dominio y quién los respeta.** `legacy`: solo archivos. `mirror`: el archivo manda; el escritor escribe el archivo bajo su `flock` y, dentro de la misma sección crítica, la fila. `unified`: la base manda; el escritor escribe la fila en una transacción `IMMEDIATE` y, después del `COMMIT` y bajo el mismo `flock` del archivo, el archivo. `sealed`: solo la base; los archivos se movieron al respaldo. Las lecturas van al archivo en `legacy`/`mirror` y a la base en `unified`/`sealed`. El modo vive en `domain_modes` de la propia base; base ausente o ilegible → `legacy` en `legacy`/`mirror`/`unified` (los archivos están al día) y error explícito en `sealed`.
- **D5 — Releases capaces.** `comandos install --stage` escribe `releases/<id>/manifest.json` con `{"state_protocol": 2}` (constante `STATE_PROTOCOL` de `comandos-cli`; las releases anteriores, sin archivo, cuentan como 0). Un dominio solo pasa a `unified` si todos los procesos que lo escriben corren releases con protocolo ≥ 2 (lectura de `/proc/<pid>/exe` y `cmdline`) y ningún proceso Python del repo lo escribe. Los escritores de cada dominio están en la tabla «Mapa de fuentes».
- **D6 — Traslado de bases SQLite con centinela.** Para `operator/actions.sqlite`, `news-history.sqlite`, `session-operations.sqlite3`, `app-state.sqlite3` y `comandos-usage.sqlite` no hay espejo (los escritores son pocos y todos Rust tras la Etapa 0): el traslado toma el candado de escritura de la base vieja (`BEGIN IMMEDIATE`), copia sus tablas a la nueva en una transacción propia de la nueva, la confirma, y solo después marca la vieja como movida (`user_version = 1000` en las que versionan así; fila `(1000, 'moved-to-comandos.sqlite3')` en `schema_migrations` para `app-state`; `user_version = 1000` también en las que no versionaban) y confirma. Una release capaz que abre una base con la marca 1000 usa la nueva; una release vieja de las fases 2c/2e ve una versión mayor y apaga su carril (comportamiento ya probado); por eso el preflight exige releases capaces antes. La vuelta atrás copia en sentido inverso y quita la marca.
- **D7 — Registro de migración dentro de la base nueva** (`migration_runs`, `migration_steps`) y respaldos fuera (`backups/<fecha>/` con `manifest.json`). Cada paso es una transacción por dominio y fuente; reanudar = saltar los pasos `done` cuyo `source_sha256` no cambió y rehacer el resto (cada importación de dominio es «borrar lo importado de esa fuente + insertar», idempotente).
- **D8 — Ensayo en seco sin tocar el HOME.** `--dry-run` copia la base nueva (si existe) a un temporal con la API de respaldo, corre la importación ahí leyendo las fuentes reales en solo lectura, imprime el informe JSON (filas, bytes, tiempo por dominio, estimación del traslado de uso) y borra el temporal. Nunca crea respaldos ni escribe en el HOME.
- **D9 — Limpieza del HOME por archivo, no por borrado.** `comandos install --cleanup-legacy` mueve al respaldo los restos (Hecho 1) y los enlaces del HOME que apunten al checkout del repo para artefactos ya retirados; cada movimiento queda en el `manifest.json` con su destino original para que `comandos install --restore-legacy <fecha>` lo deshaga.
- **D10 — Oráculos congelados.** Antes de borrar un oráculo Python/bash, sus resultados se graban como fixtures dorados (`tests/golden/` de cada crate) con una clave por hash de la entrada; las pruebas pasan a modo «reproducir». Mientras el original exista, `COMANDOS_ORACLE=check` vuelve a correrlo y compara contra el dorado (detecta deriva).
- **D11 — Prototipos y diseño fuera de `main`.** `dash/prototypes/`, `design/*.html`, `design/sidebar-comparison/` y `docs/research/extensiones-por-sesion/grok-ns.py` se conservan en la etiqueta `archive/prototipos-2026-10` y se borran de `main`; `docs/archive/README.md` dice dónde están. Las capturas PNG de `design/shots/` se quedan (no son código).

---

## Estado único

### Esquema objetivo (migraciones nuevas en `crates/comandos-store/migrations/unified/`)

```sql
-- 100-unified-meta.sql
CREATE TABLE domain_modes (
    domain TEXT PRIMARY KEY,
    mode TEXT NOT NULL CHECK (mode IN ('legacy', 'mirror', 'unified', 'sealed')),
    changed_at_ms INTEGER NOT NULL,
    changed_by TEXT NOT NULL);
CREATE TABLE migration_runs (
    run_id TEXT PRIMARY KEY,
    started_at_ms INTEGER NOT NULL,
    finished_at_ms INTEGER,
    backup_dir TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('running', 'done', 'failed')));
CREATE TABLE migration_steps (
    run_id TEXT NOT NULL REFERENCES migration_runs (run_id),
    domain TEXT NOT NULL,
    source TEXT NOT NULL,
    source_sha256 TEXT NOT NULL,
    source_size INTEGER NOT NULL,
    rows INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('done', 'skipped', 'failed')),
    detail TEXT NOT NULL DEFAULT '',
    finished_at_ms INTEGER NOT NULL,
    PRIMARY KEY (run_id, domain, source));

-- 101-documents.sql
CREATE TABLE documents (
    name TEXT PRIMARY KEY,               -- ruta relativa al dueño: 'hooks/app-tabs.json'
    domain TEXT NOT NULL,
    body BLOB NOT NULL,                  -- bytes exactos del escritor (D2)
    revision INTEGER NOT NULL DEFAULT 1,
    updated_at_ms INTEGER NOT NULL,
    origin TEXT NOT NULL CHECK (origin IN ('import', 'mirror', 'unified')));
CREATE INDEX documents_by_domain ON documents (domain);

-- 102-collections.sql
CREATE TABLE session_status (           -- hooks/state/<clave>.json
    file_key TEXT PRIMARY KEY,
    body BLOB NOT NULL,
    mtime_ns INTEGER NOT NULL,
    origin TEXT NOT NULL);
CREATE TABLE native_processes (         -- hooks/native-processes/<pid>.json
    pid INTEGER PRIMARY KEY,
    body BLOB NOT NULL,
    mtime_ns INTEGER NOT NULL,
    origin TEXT NOT NULL);
CREATE TABLE log_lines (                -- hooks/events.jsonl, ui-events.jsonl, focus-queue.jsonl
    log TEXT NOT NULL CHECK (log IN ('events', 'ui-events', 'focus-queue')),
    seq INTEGER NOT NULL,
    line BLOB NOT NULL,                 -- sin el '\n' final
    PRIMARY KEY (log, seq));
CREATE TABLE layout_snapshots (         -- app-sessions-v2.json, .bak, .history/<stamp>.json
    stamp INTEGER NOT NULL,
    generation TEXT NOT NULL CHECK (generation IN ('current', 'previous', 'minute')),
    body BLOB NOT NULL,
    PRIMARY KEY (generation, stamp));
CREATE TABLE app_commands (             -- app-focus.json, app-tab-open.json, app-tab-close.json, app-command.json, app-tab-back.json
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL CHECK (kind IN ('focus', 'open', 'close', 'command', 'back')),
    body BLOB NOT NULL,
    created_at_ms INTEGER NOT NULL,
    consumed_at_ms INTEGER);
CREATE INDEX app_commands_pending ON app_commands (kind, seq) WHERE consumed_at_ms IS NULL;

-- 103-renamed.sql (D1)
CREATE TABLE news_radar_events (
    url TEXT PRIMARY KEY, source TEXT NOT NULL, kind TEXT NOT NULL,
    title TEXT NOT NULL, at INTEGER NOT NULL,
    first_seen INTEGER NOT NULL, last_seen INTEGER NOT NULL,
    meta TEXT NOT NULL DEFAULT '{}');
CREATE TABLE operator_actions (
    id TEXT PRIMARY KEY, tool TEXT, status TEXT, detail TEXT, created REAL, updated REAL);

-- 104-session-operations.sql: copia literal del esquema de lib/session_operations.py:22-35
-- (session_operations + índices + session_operation_events), mismo SQL que
-- crates/comandos-runtime/src/session_operations.rs:37.
```

Más, en el mismo archivo: las tablas de `app-state` (migraciones 1–11, sin cambios) y las de uso (`comandos_store::usage::ensure_schema`, con `user_version = 11`).

Retenciones (las del escritor actual, aplicadas en la base): `layout_snapshots` generación `minute` 7 días (`lib/tmux_snapshot.py:258`); `log_lines` la cota que aplique hoy el escritor de cada registro (se lee de `crates/comandos-runtime/src/hooks/events_jsonl.rs` y de `crates/comandos-server/src/dash/native/ui_log.rs` en la Tarea S5); `app_commands` consumidas > 1 día.

### Mapa de fuentes → destino, dueño y escritores

| Dominio | Fuente (H = `~/.claude/hooks`) | Destino | Escritores que bloquean `unified` (D5) |
|---|---|---|---|
| `tabs` | H/app-tabs.json, app-tabs-meta.json, app-tabs-history.json, app-tab-models.json, app-tab-active.json, app-tabs-snapshot.json | `documents` | `comandos dash` (2f-1), `comandos-app` (4), `comandos-app-mac` (5), `bin/cc-app` Python vivo |
| `layout` | H/app-sessions-v2.json, .bak, .history/ | `layout_snapshots` | `comandos-app`, `comandos snapshot`, `bin/cc-app`, `bin/cc-session-snapshot` |
| `app-ui` | H/app-layout.json, app-pane-position.json, app-extension-shelf.json, notifyd-pos.json | `documents` | `comandos-app` (y su modo popups) |
| `app-commands` | H/app-focus.json, app-tab-open.json, app-tab-close.json, app-command.json, app-tab-back.json | `app_commands` (el espejo reescribe el archivo con el último cuerpo) | `comandos dash`, `comandos next`, `comandos-app` |
| `session-status` | H/state/*.json | `session_status` | `comandos hook` (todas las releases desde la Fase 1 lo escriben) |
| `processes` | H/native-processes/*.json | `native_processes` | `comandos hook` (agy, opencode) |
| `logs` | H/events.jsonl, ui-events.jsonl, focus-queue.jsonl | `log_lines` | `comandos hook`, `comandos dash` |
| `ui-docs` | H/snippets.json, prefs.json, model-watch.json, motor-queue.json, motor-results.json, pane-resume.json, session-effort.json, proxy-env-since.json, optimization-default.json, acp-panes.json, webterm-enabled | `documents` | `comandos dash`, `comandos acp` (acp-panes), `bin/cc-acp` Python vivo |
| `quota-docs` | H/provider-quotas.json, provider-subs.json, groq-ratelimit.json, agy-quota.json, pane-models.txt | `documents` | `comandos dash`, `comandos hook agy-status` |
| `news-docs` | H/news-editions.json, news-watch.json, operator/conversations.json | `documents` | `comandos dash` (2f-4) |
| `extensions` | ~/.local/state/comandos/extensions/{sizes, snapshot.json, skills.json, client-policies.json, last-check.json} | `documents` | `comandos ext` (toda release), proxies Python `cc-extensions serve` de sesiones antiguas |
| `db-operator` | H/operator/actions.sqlite | `operator_actions` (D6) | `comandos dash` |
| `db-news` | H/news-history.sqlite | `news_radar_events` (D6) | `comandos dash` |
| `db-operations` | H/session-operations.sqlite3 | `session_operations*` (D6) | `comandos dash` |
| `db-app-state` | ~/.local/state/comandos/app-state.sqlite3 (o `COMANDOS_STATE_DB`) | tablas 1–11 (D6) | `comandos dash`, `comandos events`, `comandos hook` |
| `db-usage` | H/comandos-usage.sqlite (o `COMANDOS_USAGE_DB`) | tablas de uso (D6) | `comandos dash`, `comandos hook claude-usage` |

Cada fila tiene un `Domain` en el código (Tarea S2) con su lista de fuentes, su tipo de destino, sus escritores (patrones de `argv`) y su formateador legado (el escritor Rust que ya existe en las fases 2–5).

### Etapas de la transición (orden que nunca deja algo vivo sin lector)

| Etapa | Qué pasa | Lectores de archivos | Lectores de la base | Vuelta atrás |
|---|---|---|---|---|
| 0 | Comprobación: `cc-dash-legacy.service` parado, ningún `python3` del repo vivo salvo los listados (proxies antiguos, `cc-app` si Jesús no reinició, `cc-acp` en algún pane) | todos | ninguno | — |
| 1 | Release con protocolo 2 desplegada (`--stage`), todos los dominios `legacy`; `state migrate --dry-run` | todos | ninguno | `install --rollback-release` |
| 2 | `state migrate`: respaldos, base creada, dominios de archivo a `mirror` y relleno | todos | ninguno (solo se escribe) | `state demote --all` (nada que hacer: el archivo manda) |
| 3 | `state verify` tres veces limpias en ≥ 24 h por dominio; luego `state flip <dominio> unified` con preflight | releases viejas (archivos al día por el espejo) | releases ≥ 2 | `state demote <dominio>` (instantáneo) |
| 4 | Traslados SQLite uno a uno (operator, news, operations, app-state, uso al final) | — | releases ≥ 2 | `state demote db-<x>` (copia inversa) |
| 5 | 7 días en `unified` sin incidencias + OK de Jesús → `state seal <dominio>`: los archivos se mueven al respaldo | ninguno | todos | `state export-legacy <dominio>` + `demote` |

---

## Estructura de archivos

```
crates/comandos-store/migrations/unified/10{0..4}-*.sql                                         S1
crates/comandos-store/src/unified/{mod,open,modes,documents,collections,logs}.rs                  S1, S2
crates/comandos-store/src/domains/{mod,catalog,legacy,mirror}.rs   Domain, DomainStore, Mode      S2
crates/comandos-store/src/migrate/{mod,backup,journal,import,verify,dry_run}.rs                   S3
crates/comandos-store/src/migrate/move_db.rs                       traslados con centinela        S4
crates/comandos-store/tests/{unified,domains,migrate,move_db,drill}.rs                            S1–S6
crates/comandos-cli/src/state/{mod,preflight,cli}.rs               `comandos state …`             S3, S6
crates/comandos-cli/src/install/{manifest,guard}.rs                state_protocol, guardia de rollback  S2, S6
(llamadores por dominio en comandos-server, comandos-runtime, comandos-app, comandos-app-mac)     S5
xtask/src/{lint,state_drill,retire_check,oracle,test_map}.rs                                      A1, S6, R1, O1
xtask/lint-allowlist.json                                                                          A1
docs/verification/{state-inventory.json,state-drill.md,cutover-estado.md,retirement.json,retiro.md}  A2, S6, R1–R6
crates/comandos-oracle/{Cargo.toml,src/lib.rs}                     ayuda de pruebas: dorados      O1
crates/comandos-cli/src/{agents,doctor,keys,winstart,x,raise,next,snapshot,mobile,acp}/…          C1–C5
crates/comandos-cli/src/install/{plan,assets,platform,hooks_register,cleanup}.rs                  I1, I2
```

### Paralelismo y dependencias (worktrees independientes)

| Grupo | Tareas | Depende de | En paralelo con |
|---|---|---|---|
| A | A1 (lint en modo informe), A2 (inventario de estado) | — | todo |
| S | S1 → S2 → S3 → S4; S5a…S5e en paralelo tras S3; S6 tras S4 y S5 | Fase 2 completa en `main` | C, I, O |
| C | C1, C2, C3, C4, C5 independientes entre sí | Fase 0/1 (`dispatch.rs`) | S, I, O |
| I | I1 → I2 | A1, Fase 3 (estáticos embebidos) | S, C, O |
| O | O1 → O2 | — | S, C, I |
| R | R1 → R2…R6 (R2–R6 en paralelo entre sí, cada lote cuando su `retire-check` está limpio) | R1; sustitutos en vivo (fases 2–5, C, I); O2 para borrar oráculos | — |
| F | F1 | todo | — |

Cada grupo en su rama `migration/rust-fase6-<grupo>` y worktree `.worktrees/rust-fase6-<grupo>` con `.build/target` compartido. Dentro de S5, cada sub-tarea toca los llamadores de su dominio y una sola línea de registro en `crates/comandos-store/src/domains/catalog.rs` (sección «Registro de dominios», ordenada alfabéticamente) para no chocar en los merges.

---

## Grupo A — medir antes de cambiar

### Task A1: `cargo xtask lint` (modo informe ahora, estricto al final)

**Files:**
- Create: `xtask/src/lint.rs`, `xtask/lint-allowlist.json`, `xtask/tests/lint.rs`
- Modify: `xtask/src/main.rs` (subcomando `lint`)

**Interfaces:**
- Produces: `cargo xtask lint [--report | --strict] [--root DIR]`; `pub fn classify(path: &str, head: &[u8]) -> Option<Lang>`; `pub enum Lang { Python, Shell, JavaScript, TypeScript, Html, Other(String) }`; `pub fn scan(root: &Path, allow: &Allowlist) -> Report`; `pub struct Report { pub violations: Vec<Violation>, pub by_lang: BTreeMap<String, usize> }`.

Reglas:
- Fuente de verdad: `git ls-files -z` en `--root` (por omisión la raíz del repo). Lo no rastreado no cuenta.
- Lenguaje por extensión: `.py .pyw` → Python; `.sh .bash .zsh` → Shell; `.js .mjs .cjs .jsx` → JavaScript; `.ts .tsx` → TypeScript; `.html .htm` → Html; `.pl .rb .lua` → `Other`. Sin extensión o con extensión desconocida: primera línea `#!` que contenga `python`, `sh`, `bash`, `zsh`, `node`, `deno`, `perl`, `ruby` (incluido `env <x>`). `.svg` con `<script` → JavaScript.
- Lista blanca (`xtask/lint-allowlist.json`, datos editables):

```json
{
  "version": 1,
  "allow": [
    {"path": "adapters/opencode-comandos.js", "max_lines": 3, "reason": "spec §3.2: opencode solo carga plugins JavaScript; shim de 3 líneas sin lógica"},
    {"prefix": "vendor/claude-codex/", "reason": "spec §3.1: cc-model-proxy, tercero en Rust; sus scripts de CI y hooks son del upstream"}
  ]
}
```

  `max_lines` cuenta líneas no vacías; si se supera, es violación. No hay comodines: `path` exacto o `prefix` que termina en `/`.
- `--report`: imprime JSON `{"total": n, "by_lang": {...}, "violations": [...]}` y sale 0. `--strict`: sale 1 si hay alguna violación. Sin bandera = `--report`.

- [ ] **Step 1: Pruebas que fallan**

```rust
// xtask/tests/lint.rs
use xtask::lint::{classify, Lang};

#[test]
fn shebang_without_extension_is_detected() {
    assert_eq!(classify("bin/cc-next", b"#!/usr/bin/env python3\n"), Some(Lang::Python));
    assert_eq!(classify("bin/cc-centro", b"#!/usr/bin/env bash\n"), Some(Lang::Shell));
    assert_eq!(classify("bin/cc-browser-remote", b"#!/bin/sh\n"), Some(Lang::Shell));
    assert_eq!(classify("config/tmux.conf", b"# tmux\n"), None);
}

#[test]
fn svg_with_script_counts_as_js() {
    assert_eq!(classify("dash/icons/x.svg", b"<svg><script>alert(1)</script></svg>"), Some(Lang::JavaScript));
    assert_eq!(classify("dash/icons/y.svg", b"<svg><path d=\"M0\"/></svg>"), None);
}

#[test]
fn shim_over_three_lines_is_a_violation() {
    let repo = FakeRepo::new()
        .file("adapters/opencode-comandos.js", "a\nb\nc\nd\n")
        .file("crates/x/src/lib.rs", "fn f() {}\n");
    let r = repo.scan_with_default_allowlist();
    assert_eq!(r.violations.len(), 1);
}
```

`xtask` pasa a tener `src/lib.rs` (con `pub mod lint;` y los módulos nuevos de esta fase) además de `main.rs`, para que las pruebas de integración importen `xtask::…`. `FakeRepo` crea un `git init` en un temporal (sin red) y añade archivos.

- [ ] **Step 2: Verificar que fallan**, **Step 3: implementar**, **Step 4: ejecutar** `cargo xtask lint --report > docs/verification/lint-inicio.json` (lectura del repo; se commitea como línea base: hoy ~372 violaciones).
- [ ] **Step 5: Commit**

```bash
git add xtask docs/verification/lint-inicio.json
git commit -m "feat(xtask): lint de lenguajes con lista blanca de la spec, en modo informe

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task A2: Inventario del estado (`comandos state inventory`)

**Files:**
- Create: `crates/comandos-store/src/domains/catalog.rs` (solo la tabla de fuentes por dominio, sin modos todavía), `crates/comandos-cli/src/state/{mod,cli}.rs` (subcomando `inventory`)
- Test: `crates/comandos-store/tests/domains_catalog.rs`
- Produce (lo ejecuta el controlador): `docs/verification/state-inventory.json`

**Interfaces:**
- Produces: `pub struct SourceSpec { pub domain: &'static str, pub pattern: SourcePattern, pub kind: TargetKind }`, `pub enum SourcePattern { File(&'static str), Dir { dir: &'static str, suffix: &'static str }, Sqlite(&'static str) }` con raíces simbólicas `H/`, `STATE/`, `SHARE/`; `pub fn catalog() -> &'static [SourceSpec]`; `comandos state inventory [--home DIR] [--json]` que recorre las raíces, empareja cada archivo con su dominio o lo marca `sin-dominio`/`se-queda-como-archivo` (D3)/`resto` (D9), y da tamaño, `sha256`, `mtime` y número de archivos por dominio.

- [ ] **Step 1: Prueba que falla**: HOME falso con un archivo por fila del «Mapa de fuentes», más `usage.db`, `md2tg.py` roto y `providers.env`; el inventario clasifica cada uno como espera la tabla, y **ningún archivo real del HOME queda sin clasificar**: la prueba falla si `catalog()` no cubre un nombre que `bin/`, `lib/` o `crates/` mencionan como estado (lista fija en la prueba, sacada del Hecho 1).
- [ ] **Step 2: Implementar y verificar**; el controlador corre `comandos state inventory --json > docs/verification/state-inventory.json` en la máquina real (solo lectura) y revisa que `sin-dominio` esté vacío; si no, añade la fila al catálogo antes de seguir con S.
- [ ] **Step 3: Commit**

```bash
git add crates/comandos-store crates/comandos-cli docs/verification/state-inventory.json
git commit -m "feat(store): catálogo de fuentes de estado por dominio e inventario de solo lectura

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Grupo S — estado único

### Task S1: Base única: apertura, migraciones y tablas

**Files:**
- Create: `crates/comandos-store/migrations/unified/{100-unified-meta,101-documents,102-collections,103-renamed,104-session-operations}.sql`, `crates/comandos-store/src/unified/{mod,open,documents,collections,logs}.rs`
- Modify: `crates/comandos-store/src/lib.rs` (`pub mod unified;`), `crates/comandos-store/src/state_db/migrations.rs` (lista extendida `UNIFIED_MIGRATIONS` = las 11 de `app-state` + 100–104)
- Test: `crates/comandos-store/tests/unified.rs`

**Interfaces:**
- Produces:

```rust
pub const MOVED_SENTINEL: i64 = 1000;
pub fn unified_path(home: &Path) -> PathBuf;                     // COMANDOS_DB o SHARE/comandos.sqlite3
pub fn open_unified(path: &Path) -> Result<Connection>;          // connect + migraciones 1–11 y 100–104 + ensure_schema de uso + user_version=11
pub struct Document { pub name: String, pub domain: String, pub body: Vec<u8>, pub revision: i64, pub updated_at_ms: i64 }
pub fn doc_get(conn: &Connection, name: &str) -> Result<Option<Document>>;
pub fn doc_put(conn: &Connection, name: &str, domain: &str, body: &[u8], origin: Origin, now_ms: i64) -> Result<i64>; // devuelve revision
pub fn doc_put_if_newer(conn: &Connection, name: &str, domain: &str, body: &[u8], mtime_ms: i64) -> Result<bool>;   // relleno (Review Focus 1)
pub enum Origin { Import, Mirror, Unified }
pub fn status_put(conn: &Connection, file_key: &str, body: &[u8], mtime_ns: i64, origin: Origin) -> Result<()>;
pub fn status_put_if_newer(conn: &Connection, file_key: &str, body: &[u8], mtime_ns: i64) -> Result<bool>;
pub fn log_append(conn: &Connection, log: LogName, line: &[u8]) -> Result<i64>;
pub fn log_tail(conn: &Connection, log: LogName, max_lines: usize) -> Result<Vec<Vec<u8>>>;
pub fn layout_put(conn: &Connection, generation: Generation, stamp: i64, body: &[u8]) -> Result<()>;
pub fn layout_prune_minutes(conn: &Connection, older_than: i64) -> Result<usize>;
pub fn command_push(conn: &Connection, kind: CommandKind, body: &[u8], now_ms: i64) -> Result<i64>;
pub fn command_take(conn: &Connection, kind: CommandKind, now_ms: i64) -> Result<Option<(i64, Vec<u8>)>>;
```

- [ ] **Step 1: Pruebas que fallan**

```rust
#[test]
fn unified_carries_both_version_marks() {
    let dir = tempdir();
    let Ok(c) = comandos_store::unified::open_unified(&dir.join("comandos.sqlite3")) else { panic!("abre") };
    let uv: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap_or(-1);
    assert_eq!(uv, comandos_store::usage::SCHEMA_VERSION);
    let max: i64 = c.query_row("SELECT MAX(version) FROM schema_migrations", [], |r| r.get(0)).unwrap_or(-1);
    assert_eq!(max, 104);
    for t in ["documents", "session_status", "log_lines", "layout_snapshots", "app_commands",
              "news_radar_events", "operator_actions", "session_operations", "usage_turns", "pomodoro_state"] {
        let n: i64 = c.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1", [t], |r| r.get(0)).unwrap_or(0);
        assert_eq!(n, 1, "{t}");
    }
}

#[test]
fn doc_put_if_newer_respects_mirror_write() {
    let c = mem_unified();
    let rev = comandos_store::unified::doc_put(&c, "hooks/snippets.json", "ui-docs", b"[2]", comandos_store::unified::Origin::Mirror, 2_000);
    assert!(rev.is_ok());
    let wrote = comandos_store::unified::doc_put_if_newer(&c, "hooks/snippets.json", "ui-docs", b"[1]", 1_000).ok();
    assert_eq!(wrote, Some(false));
    let body = comandos_store::unified::doc_get(&c, "hooks/snippets.json").ok().flatten().map(|d| d.body);
    assert_eq!(body.as_deref(), Some(&b"[2]"[..]));
}

#[test]
fn reopen_is_idempotent_and_never_lowers_user_version() { /* abrir dos veces; con user_version=12 puesto a mano, open_unified devuelve Error::Validation("base más nueva") */ }
```

- [ ] **Step 2–4:** verificar que fallan, implementar (`open_unified` niega abrir si `user_version > 11` o si `schema_migrations` tiene versiones desconocidas > 104: «base creada por una versión más nueva de ComandOS»), verificar.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-store
git commit -m "feat(store): base única comandos.sqlite3 con documentos, colecciones y tablas renombradas

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task S2: Dominios, modos, `DomainStore` y releases capaces

**Files:**
- Create: `crates/comandos-store/src/unified/modes.rs`, `crates/comandos-store/src/domains/{mod,legacy,mirror}.rs`, `crates/comandos-cli/src/install/manifest.rs`, `crates/comandos-cli/src/state/preflight.rs`
- Modify: `crates/comandos-store/src/domains/catalog.rs` (escritores por dominio), `crates/comandos-cli/src/install/release.rs` (escribe `manifest.json` al hacer `stage_release`)
- Test: `crates/comandos-store/tests/domains.rs`, `crates/comandos-cli/tests/preflight.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode { Legacy, Mirror, Unified, Sealed }
pub fn mode_of(conn: Option<&Connection>, domain: &str) -> Result<Mode>; // sin base → Legacy (D4)
pub fn set_mode(conn: &Connection, domain: &str, mode: Mode, by: &str, now_ms: i64) -> Result<()>;

/// Acceso a un documento de dominio según su modo. El `flock` del archivo es el
/// mismo `FileLock` que usan hoy los escritores (`<archivo>.lock`).
pub struct DocHandle<'a> { pub home: &'a Path, pub name: &'static str, pub domain: &'static str, pub file: PathBuf, pub lock: PathBuf }
impl DocHandle<'_> {
    pub fn read(&self, db: Option<&Connection>) -> Result<Option<Vec<u8>>>;
    pub fn write(&self, db: Option<&Connection>, body: &[u8], now_ms: i64) -> Result<()>;
}
pub struct StatusDir<'a> { /* H/state */ }
impl StatusDir<'_> { pub fn read(&self, db: Option<&Connection>, key: &str) -> Result<Option<Vec<u8>>>; pub fn write(&self, db: Option<&Connection>, key: &str, body: &[u8], now_ns: i64) -> Result<()>; pub fn list(&self, db: Option<&Connection>) -> Result<Vec<(String, Vec<u8>)>>; }
pub struct LogHandle<'a> { /* events.jsonl, ui-events.jsonl, focus-queue.jsonl */ }
impl LogHandle<'_> { pub fn append(&self, db: Option<&Connection>, line: &[u8]) -> Result<()>; pub fn tail(&self, db: Option<&Connection>, n: usize) -> Result<Vec<Vec<u8>>>; }

// comandos-cli
pub const STATE_PROTOCOL: u32 = 2;
pub fn release_protocol(release_dir: &Path) -> u32;                        // sin manifest.json → 0
pub struct WriterProc { pub pid: i32, pub exe: PathBuf, pub argv: Vec<String>, pub protocol: u32, pub python_repo: bool }
pub fn domain_writers(proc_root: &Path, home: &Path, repo: &Path, domain: &str) -> Vec<WriterProc>;
pub fn can_unify(writers: &[WriterProc]) -> Result<(), Vec<String>>;     // razones legibles
```

Reglas de `DocHandle::write` (D4), con el código completo porque es el núcleo de la transición:

```rust
pub fn write(&self, db: Option<&Connection>, body: &[u8], now_ms: i64) -> Result<()> {
    let mode = mode_of(db, self.domain)?;
    match (mode, db) {
        (Mode::Legacy, _) | (Mode::Mirror, None) | (Mode::Unified, None) => {
            // Sin base utilizable el archivo es la verdad (en Unified el espejo lo tenía al día).
            let _g = FileLock::exclusive(&self.lock)?;
            write_atomic(&self.file, body)
        }
        (Mode::Mirror, Some(db)) => {
            let _g = FileLock::exclusive(&self.lock)?;
            write_atomic(&self.file, body)?;
            // Dentro de la misma sección crítica: nadie más escribe el archivo entre medias.
            crate::unified::doc_put(db, self.name, self.domain, body, Origin::Mirror, now_ms).map(|_| ())
        }
        (Mode::Unified, Some(db)) => {
            let _g = FileLock::exclusive(&self.lock)?;
            crate::unified::doc_put(db, self.name, self.domain, body, Origin::Unified, now_ms)?;
            write_atomic(&self.file, body) // espejo para releases viejas y para la vuelta atrás
        }
        (Mode::Sealed, Some(db)) => crate::unified::doc_put(db, self.name, self.domain, body, Origin::Unified, now_ms).map(|_| ()),
        (Mode::Sealed, None) => Err(Error::Validation(format!("{}: dominio sellado y base no disponible", self.domain))),
    }
}
```

`write_atomic` = temporal en el mismo directorio + `fsync` + `rename`, con el modo del archivo existente (o `0600` si no existía): el mismo contrato que `files::write_text_atomic` de la 2c, que se reutiliza en lugar de duplicarlo.

`domain_writers`: recorre `proc_root` (inyectable), y para cada proceso cuyo `exe` resuelve dentro de `SHARE/releases/<id>/` o cuyo `argv` coincide con un patrón de escritor del dominio, lee el protocolo de esa release; marca `python_repo` si `argv[0]` es un intérprete Python y algún argumento resuelve dentro de `repo` (checkout principal). `can_unify` falla con una razón por proceso (`"pid 1234 (comandos dash, release 2bae7f9cd7d6, protocolo 0)"`, `"pid 777 python3 …/bin/cc-app"`).

- [ ] **Step 1: Pruebas que fallan** (`tests/domains.rs`):
  - `legacy_writes_only_file`, `mirror_writes_file_then_row_same_lock`, `unified_writes_row_then_file`, `sealed_without_db_is_error`, `unified_without_db_falls_back_to_file_read` (D4).
  - `mirror_never_leaves_row_newer_than_file`: dos hilos escriben el mismo documento 1 000 veces en `mirror`; al final `sha256(archivo) == sha256(fila)`.
  - `preflight.rs`: `/proc` falso con un `comandos dash` de una release sin `manifest.json` → `can_unify` falla con su razón; con todas las releases en protocolo 2 → pasa; un `python3 <repo>/bin/cc-app` → falla para `tabs` y pasa para `logs`.
- [ ] **Step 2–4:** fallan, implementar, pasan. `stage_release` escribe `manifest.json` (`{"state_protocol": 2}`, `0644`) dentro del directorio de la release antes del `rename` del binario; prueba en `crates/comandos-cli/tests/release_manifest.rs`.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-store crates/comandos-cli
git commit -m "feat(store): modos por dominio con espejo bajo el mismo candado y preflight de releases capaces

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task S3: Migrador — respaldos, registro, relleno, verificación, ensayo en seco y reanudación

**Files:**
- Create: `crates/comandos-store/src/migrate/{mod,backup,journal,import,verify,dry_run}.rs`
- Modify: `crates/comandos-cli/src/state/cli.rs` (`migrate`, `verify`, `status`, `backups`)
- Test: `crates/comandos-store/tests/migrate.rs`, `crates/comandos-cli/tests/state_cli.rs`

**Interfaces:**
- Produces:

```rust
pub struct MigrateOptions { pub home: PathBuf, pub db: PathBuf, pub dry_run: bool, pub resume: bool, pub domains: Option<Vec<String>>, pub now_ms: i64 }
pub struct MigrateReport { pub run_id: String, pub backup_dir: Option<PathBuf>, pub steps: Vec<StepReport>, pub usage_move_estimate_ms: Option<u64> }
pub fn migrate(opts: &MigrateOptions) -> Result<MigrateReport>;
pub struct BackupManifest { pub created_at_ms: i64, pub entries: Vec<BackupEntry> }
pub struct BackupEntry { pub source: PathBuf, pub copy: PathBuf, pub sha256: String, pub size: u64, pub mode: u32, pub mtime_ns: i64, pub kind: EntryKind } // File | Sqlite | Dir
pub fn backup_sources(home: &Path, dest: &Path, specs: &[SourceSpec]) -> Result<BackupManifest>;
pub fn verify_backup(manifest_path: &Path) -> Result<()>;               // rehash de cada copia
pub struct VerifyReport { pub domain: String, pub mismatches: Vec<String> }
pub fn verify(home: &Path, db: &Connection, domain: &str) -> Result<VerifyReport>;
```

CLI (`comandos state …`):
- `migrate [--dry-run] [--resume] [--domain D]…` — sin `--dry-run`: (1) preflight de que la base no es más nueva; (2) `run_id` (`AAAAMMDD-HHMMSS-<4 hex>`) y `backups/<run_id>/` con todas las fuentes de archivo (los SQLite **no** se respaldan aquí: los respalda S4 justo antes de trasladar cada uno, con la API de respaldo); `manifest.json` escrito al final y verificado (`verify_backup`) antes de seguir; (3) para cada dominio de archivo: `set_mode(mirror)` **primero**, luego relleno con `*_if_newer` por fuente y registro del paso. Con `--resume`: toma el último `run_id` en `running`, verifica su respaldo y salta los pasos `done` cuya fuente tenga el mismo `sha256`.
- `verify [--domain D]` — compara archivo y fila por cada documento/clave/línea; sale 0 sin diferencias. Guarda el resultado en `kv`-equivalente: tabla `migration_steps` con `domain = 'verify:<D>'` para que `flip` pueda exigir tres limpios en ≥ 24 h.
- `status` — modo de cada dominio, último `verify`, escritores vivos (`domain_writers`), releases y protocolo.
- `backups` — lista `backups/*/manifest.json` con fecha, tamaño y verificación.

Importación por tipo (todas dentro de una transacción por fuente):
- documento: `doc_put_if_newer(name, bytes, mtime_ms_del_archivo)`.
- `state/` y `native-processes/`: por archivo, `*_if_newer` con `mtime_ns`.
- registros JSONL: el relleno solo corre si la tabla de ese registro está vacía (si no, el espejo ya escribe y una reimportación duplicaría): inserta las líneas en orden; una línea final sin `\n` (escritura a medias) se ignora y se anota.
- `layout`: `current` y `previous` como documentos de `layout_snapshots`; cada `.history/<stamp>.json` dentro de la retención de 7 días; los más viejos se cuentan en el informe y quedan solo en el respaldo.

- [ ] **Step 1: Pruebas que fallan** (`tests/migrate.rs`, HOME falso generado desde los formatos reales de las fases 2–5):

```rust
#[test]
fn migrate_is_idempotent() {
    let h = FakeHome::with_all_sources();
    let first = migrate(&h.opts()).ok();
    let second = migrate(&MigrateOptions { resume: false, ..h.opts() }).ok();
    assert!(first.is_some() && second.is_some());
    assert_eq!(h.row_counts(), h.row_counts_after(|| { let _ = migrate(&h.opts()); }));
}

#[test]
fn dry_run_touches_nothing_in_home() {
    let h = FakeHome::with_all_sources();
    let before = h.tree_hash();
    let r = migrate(&MigrateOptions { dry_run: true, ..h.opts() }).ok();
    assert!(r.is_some_and(|r| r.backup_dir.is_none()));
    assert_eq!(h.tree_hash(), before, "ni base, ni respaldos, ni archivos");
}

#[test]
fn resume_after_kill_between_domains() {
    let h = FakeHome::with_all_sources();
    h.fail_after_domain("session-status"); // el siguiente paso devuelve error inyectado
    assert!(migrate(&h.opts()).is_err());
    let r = migrate(&MigrateOptions { resume: true, ..h.opts() }).ok();
    assert!(r.is_some_and(|r| r.steps.iter().all(|s| s.status != "failed")));
    assert_eq!(h.duplicate_rows(), 0);
}

#[test]
fn backfill_never_overwrites_newer_mirror_write() {
    let h = FakeHome::with_all_sources();
    h.write_file_with_mtime("state/p--s--1.json", br#"{"status":"done"}"#, 1_000);
    h.set_mode("session-status", Mode::Mirror);
    // El relleno lee la fuente (instantánea vieja, mtime 1 000) y, antes de aplicarla,
    // un hook en espejo escribe la versión nueva en archivo y fila (mtime 2 000).
    h.pause_import_after_read("session-status", || {
        h.writer_write_status_at("p--s--1", br#"{"status":"working"}"#, 2_000);
    });
    let _ = migrate(&h.opts());
    assert_eq!(h.status_row("p--s--1").as_deref(), Some(&br#"{"status":"working"}"#[..]));
}

#[test]
fn backup_manifest_rehash_detects_tampering() { /* cambia un byte de una copia; verify_backup falla */ }

#[test]
fn verify_reports_byte_difference_by_name() { /* escribe a mano un archivo distinto de su fila; verify lo nombra */ }
```

- [ ] **Step 2–4:** fallan, implementar, pasan. Las pruebas de CLI (`state_cli.rs`) ejecutan el binario con `--home <temporal>` y `COMANDOS_DB=<temporal>`.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-store crates/comandos-cli
git commit -m "feat(store): migrador idempotente y reanudable con respaldos verificados y ensayo en seco

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task S4: Traslado de las bases SQLite con centinela (D6)

**Files:**
- Create: `crates/comandos-store/src/migrate/move_db.rs`
- Modify: `crates/comandos-store/src/{usage.rs, state_db/mod.rs}` y los abridores de `session-operations`, `news-history`, `operator/actions` en `comandos-runtime`/`comandos-server` (2c `lanes.rs`): **resolución de ruta** `resolve_db(legacy_path) -> DbLocation` antes de abrir
- Modify: `crates/comandos-cli/src/state/cli.rs` (`state move <db-x>`, `state demote <db-x>`)
- Test: `crates/comandos-store/tests/move_db.rs`

**Interfaces:**
- Produces:

```rust
pub enum DbLocation { Legacy(PathBuf), Unified(PathBuf) }
/// Mira la marca de la base vieja (user_version o fila 1000 de schema_migrations):
/// con MOVED_SENTINEL devuelve la base única; si no, la vieja.
pub fn resolve_db(legacy: &Path, unified: &Path) -> Result<DbLocation>;
pub struct MoveSpec { pub domain: &'static str, pub legacy: PathBuf, pub tables: &'static [(&'static str, &'static str)], pub marker: Marker } // (tabla vieja, tabla nueva)
pub enum Marker { UserVersion, SchemaMigrationsRow }
pub struct MoveEstimate { pub bytes: u64, pub copy_ms: u64 }
pub fn estimate(spec: &MoveSpec, scratch: &Path) -> Result<MoveEstimate>;      // copia a un temporal y mide
pub fn move_db(spec: &MoveSpec, unified: &Path, backup_dir: &Path, budget_ms: u64) -> Result<()>;
pub fn demote_db(spec: &MoveSpec, unified: &Path) -> Result<()>;               // copia inversa y quita la marca
```

Algoritmo de `move_db` (código completo; es la pieza con más riesgo de pérdida):

```rust
pub fn move_db(spec: &MoveSpec, unified: &Path, backup_dir: &Path, budget_ms: u64) -> Result<()> {
    // 0) Respaldo consistente de la vieja con la API de respaldo, antes de cualquier candado.
    backup_sqlite(&spec.legacy, &backup_dir.join(backup_name(&spec.legacy)))?;
    let est = estimate(spec, backup_dir)?;
    if est.copy_ms > budget_ms {
        return Err(Error::Validation(format!(
            "{}: la copia estimada tarda {} ms (> {} ms); hacerlo en una ventana sin agentes",
            spec.domain, est.copy_ms, budget_ms)));
    }
    // 1) Candado de escritura en la vieja: los escritores esperan su busy_timeout.
    let old = rusqlite::Connection::open(&spec.legacy)?;
    old.busy_timeout(std::time::Duration::from_millis(5000))?;
    old.execute_batch("BEGIN IMMEDIATE")?;
    if is_moved(&old, spec.marker)? {
        old.execute_batch("ROLLBACK")?;
        return Ok(()); // idempotente: ya trasladada
    }
    // 2) Copia en una transacción de la nueva; la vieja se lee adjunta en solo lectura.
    let new = crate::unified::open_unified(unified)?;
    new.execute("ATTACH DATABASE ?1 AS legacy", [format!("file:{}?mode=ro", spec.legacy.display())])?;
    let copied = (|| -> Result<()> {
        new.execute_batch("BEGIN IMMEDIATE")?;
        for (from, to) in spec.tables {
            new.execute_batch(&format!("DELETE FROM main.\"{to}\"; INSERT INTO main.\"{to}\" SELECT * FROM legacy.\"{from}\";"))?;
        }
        verify_counts(&new, spec.tables)?;
        new.execute_batch("COMMIT")?;
        Ok(())
    })();
    if let Err(e) = copied {
        let _ = new.execute_batch("ROLLBACK");
        let _ = old.execute_batch("ROLLBACK");
        return Err(e);
    }
    new.execute_batch("DETACH DATABASE legacy")?;
    // 3) Solo con los datos ya confirmados en la nueva se marca la vieja (Review Focus 2).
    mark_moved(&old, spec.marker)?;
    old.execute_batch("COMMIT")?;
    Ok(())
}
```

`spec.tables` lista las tablas en el orden de sus claves foráneas; los nombres salen de `sqlite_master` de la vieja y se comparan con la lista (una tabla nueva no prevista → error, no se copia a medias). `verify_counts` compara `count(*)` de cada par dentro de la transacción. `mark_moved`: `PRAGMA user_version = 1000` o `INSERT INTO schema_migrations VALUES (1000, 'moved-to-comandos.sqlite3', <segundos>)`. Orden de los traslados en la Etapa 4: `db-operator`, `db-news`, `db-operations`, `db-app-state`, `db-usage` (presupuesto 4 000 ms).

Puerta en los abridores (la que ya tienen 2c/2e se extiende, no se sustituye): antes de abrir una base vieja, `resolve_db`; si devuelve `Unified`, se abre la única con la puerta de `user_version ≤ 11` y `schema_migrations ≤ 104`.

- [ ] **Step 1: Pruebas que fallan**:
  - `move_then_resolve_points_to_unified` y `demote_restores_legacy_and_removes_marker` (filas idénticas a la ida y a la vuelta, también `user_version` 11 en la vieja tras `demote`).
  - `move_crash_before_sentinel_is_redone`: falla inyectada tras el `COMMIT` de la nueva y antes de `mark_moved`; la vieja no está marcada; el reintento termina y no duplica.
  - `writer_blocked_during_move_resumes_on_unified`: un hilo escritor con `busy_timeout` 5 s intenta insertar durante el traslado; al liberarse, su siguiente apertura resuelve a la única y la fila aparece ahí, no en la vieja.
  - `old_release_lane_shuts_down_on_moved_db`: el carril de uso de la 2e (`Lane<UsageBackend>`) sobre una base con `user_version = 1000` queda apagado (comportamiento existente; la prueba lo fija).
  - `usage_move_refuses_when_estimate_exceeds_budget` con presupuesto de 1 ms.
- [ ] **Step 2–4:** fallan, implementar, pasan.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-store crates/comandos-runtime crates/comandos-server crates/comandos-cli
git commit -m "feat(store): traslado de bases SQLite a la base única con centinela y copia inversa

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Tasks S5a–S5e: Llamadores por dominio (en paralelo entre sí)

Cada sub-tarea cambia los lectores y escritores de sus dominios para que pasen por `DocHandle`/`StatusDir`/`LogHandle`/`layout_*`/`command_*` (S2), con el `Connection` de la base única abierto por el carril que ya tenga ese componente (en el frente, un carril nuevo `Lane<UnifiedBackend>` sobre `unified_path` con la misma puerta; en los hooks, apertura por invocación con `busy_timeout` 5 s y sin migrar: si la base no existe, `None` → `legacy`). Nada cambia de comportamiento mientras el dominio esté en `legacy`.

| Sub-tarea | Dominios | Componentes a tocar (buscar por la ruta del archivo en `crates/`) | Pruebas mínimas (además de las existentes, que deben seguir en verde) |
|---|---|---|---|
| S5a | `tabs`, `layout`, `app-ui`, `app-commands` | `comandos-server` (2f-1 registro de pestañas, `/tabs`, `/tab-*`), `comandos-app` (`state_files`, `ipc`, `snapshot`), `comandos-app-mac` (M4), `comandos snapshot` (C2) | `tabs_read_same_in_every_mode`, `app_commands_ipc_survives_mode_flip` (la app sigue viendo un `focus` escrito justo antes y justo después del cambio de modo), `layout_minute_retention_7_days` |
| S5b | `session-status`, `processes`, `logs` | `comandos-runtime::hooks::{state_file, events_jsonl, agy, opencode}`, `comandos-server` (`/state` lee `state/`, `ui_log`, `events`, `focus-queue`), `comandos next` | `hook_write_in_mirror_costs_under_3ms` (mide con `Instant` 200 escrituras; mediana < 3 ms), `jsonl_trim_rule_matches_legacy_writer` |
| S5c | `ui-docs` | `comandos-server` (snippets, prefs, model-watch, motor, pane-resume, session-effort, proxy-env-since, optimization-default, webterm-enabled), `comandos acp` (C3) | `snippets_lock_shared_with_mirror`, un caso por documento: escribir en cada modo y leer de vuelta los mismos bytes |
| S5d | `quota-docs`, `news-docs` | `comandos-server` (2e límites, 2f-3, 2f-4), `comandos-runtime::hooks::agy_status` | idem por documento; `pane_models_txt_stops_when_no_reader` (ver R3: `pane-models.txt` deja de escribirse cuando `cc-pane-model` se retira; hasta entonces sigue en `quota-docs`) |
| S5e | `extensions` | `comandos-extensions` (`sizes`, `snapshot.json`, `skills.json`, `client-policies.json`, `last-check.json`) | `extension_sizes_round_trip_every_mode` |

Pasos de cada sub-tarea:

- [ ] **Step 1:** pruebas que fallan de la tabla, con HOME falso y la base en un temporal (`COMANDOS_DB`).
- [ ] **Step 2:** cambiar los llamadores; el formateador de bytes es el escritor actual (no se reescribe la serialización: `DocHandle::write` recibe los bytes que ese escritor ya producía).
- [ ] **Step 3:** `$C test --workspace` en verde (paridad de las fases anteriores intacta).
- [ ] **Step 4:** commit `feat(store): dominio <x> por DocHandle con espejo` con las rutas tocadas.

### Task S6: Cambio de modo, vuelta atrás, `export-legacy`, guardia de releases y ensayo

**Files:**
- Modify: `crates/comandos-cli/src/state/cli.rs` (`flip`, `demote`, `seal`, `export-legacy`, `rollback`)
- Create: `crates/comandos-cli/src/install/guard.rs`, `xtask/src/state_drill.rs`, `docs/verification/cutover-estado.md`
- Modify: `crates/comandos-cli/src/install.rs` (`--rollback-release` llama a la guardia)
- Test: `crates/comandos-cli/tests/state_flip.rs`, `crates/comandos-store/tests/drill.rs`

**Interfaces:**
- Produces:
  - `comandos state flip <dominio> unified` — exige: modo `mirror`; tres `verify` limpios con ≥ 24 h entre el primero y el último; `can_unify(domain_writers(...))` sin razones. Si algo falta, lo dice y sale 1 sin cambiar nada.
  - `comandos state demote <dominio>|--all` — `unified` → `mirror` tras un `verify` limpio (los archivos están al día); `db-*` → `demote_db`; `sealed` → exige `export-legacy` antes.
  - `comandos state seal <dominio> --yes` — exige 7 días en `unified` (por `domain_modes.changed_at_ms`) y `verify` limpio; mueve los archivos del dominio a `backups/<fecha>-seal-<dominio>/` con manifiesto.
  - `comandos state export-legacy <dominio>` — escribe los archivos desde la base con los mismos bytes (D2) y con el `flock` de cada archivo; deja el dominio en `unified`.
  - `comandos state rollback <run_id> --perder-desde-el-respaldo` — último recurso: restaura las fuentes del respaldo de ese `run_id` verificando hashes, pone todos los dominios en `legacy`, renombra la base a `comandos.sqlite3.rolled-back-<ms>` (nunca la borra) e imprime el intervalo de datos que se pierde. Sin la bandera, explica que `demote --all` es la vuelta sin pérdida y sale 1.
  - Guardia de `install --rollback-release`: si la release destino tiene `state_protocol < 2` y algún dominio está en `unified`, ejecuta `state demote` de esos dominios antes de cambiar el enlace (y aborta si un dominio está `sealed` sin `export-legacy`).
  - `cargo xtask state-drill --source-home <ruta> [--keep]` — copia (lectura) las fuentes del catálogo desde `--source-home` a un HOME temporal, y en él: `migrate --dry-run`, `migrate`, escrituras sintéticas por dominio en `mirror`, `verify`, `flip` de todos con un `/proc` falso capaz, escrituras en `unified`, traslado de las bases, `install --rollback-release` hacia una release falsa de protocolo 0 (debe degradar), lectura con los lectores «viejos» (los lectores de archivo) comparando bytes con lo último escrito, `demote --all` (las fuentes deben contener la copia inicial más las escrituras sintéticas, byte a byte), y por último `rollback` al respaldo con su bandera (el árbol de fuentes debe volver a ser idéntico a la copia inicial). Informe en Markdown.

- [ ] **Step 1: Pruebas que fallan**: `flip_refuses_without_three_verifies`, `flip_refuses_with_incapable_writer`, `rollback_release_demotes_unified_domains`, `seal_refuses_before_seven_days`, `export_legacy_reproduces_bytes`, `rollback_requires_explicit_loss_flag`; `tests/drill.rs` ejecuta el ensayo sobre un HOME sintético completo.
- [ ] **Step 2–4:** fallan, implementar, pasan.
- [ ] **Step 5: Ensayo sobre una copia real** (controlador): `cargo xtask state-drill --source-home /home/someguy > docs/verification/state-drill.md` (solo lee el HOME real; todo se escribe en el temporal). Debe terminar sin diferencias.
- [ ] **Step 6: Escribir `docs/verification/cutover-estado.md`** con la secuencia por etapas de la tabla «Etapas de la transición», los comandos exactos, lo que se mide (tiempo de hook en espejo, tamaño de la base, RSS del frente) y la vuelta atrás de cada etapa. Incluye el **simulacro en vivo** obligatorio antes de la Etapa 3: dominio `ui-docs` → `unified`, una escritura de snippet desde el tablero, `state demote ui-docs`, comprobación de que el tablero muestra el snippet leyendo el archivo, y vuelta a `unified`.
- [ ] **Step 7: Commit**

```bash
git add crates/comandos-cli crates/comandos-store xtask docs/verification/state-drill.md docs/verification/cutover-estado.md
git commit -m "feat(cli): cambio de modo, vuelta atrás sin pérdida, export-legacy, guardia de releases y ensayo de estado

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Grupo C — herramientas sin port previo (independientes entre sí)

Reglas comunes: cada herramienta es un subcomando de `comandos` con alias `argv[0]` para su nombre `cc-*` (o `ccx`) en `crates/comandos-cli/src/dispatch.rs`; mismos argumentos, misma salida y mismos códigos de salida que el original, comprobados con una prueba diferencial que ejecuta el original como texto (`bash <ruta del worktree>` o `python3 <ruta>`) con un `PATH` de ejecutables falsos (binarios de prueba Rust que registran sus argumentos) y un HOME temporal; tmux siempre privado (Global Constraints). Las comprobaciones o rutas que el original hace sobre piezas ya retiradas (ttyd, `cc-webterm`, Telegram, Python) se sustituyen por las de su reemplazo Rust y la diferencia se lista en el commit.

### Task C1: `comandos agents` y `comandos doctor`

**Files:** `crates/comandos-cli/src/{agents,doctor}/…`, alias `cc-agents`, `cc-doctor`; pruebas `crates/comandos-cli/tests/{agents,doctor}.rs`.

- `agents` (de `bin/cc-agents`, 238 líneas bash): sin argumentos, estado de las integraciones; `setup`, configura codex (`notify` en `~/.codex/config.toml`), opencode (plugin), gemini y agy (hooks en `settings.json`, `~/.gemini/config/hooks.json`, statusLine de agy con `stack_with_default`). **Cambio que prepara la retirada**: las rutas que escribe en esas configuraciones dejan de apuntar a `<repo>/adapters/*` y pasan a `~/.local/bin/<nombre>` (enlaces al binario que `comandos install --link` crea: `codex-notify.sh`, `codex-hooks.sh`, `gemini-hooks.sh`, `agy-hooks.sh`, `agy-statusline.py`, `grok-hooks.py`); respaldo de cada archivo de configuración antes de escribir (`<archivo>.bak-comandos-<fecha>`), igual que hoy hace con el de agy. El plugin de opencode se instala como el shim de 3 líneas (`exec`/import de `comandos hook opencode`) que la lista blanca de A1 permite.
- `doctor` (de `bin/cc-doctor`, 396 líneas bash): secciones `core | desktop | audio | remote`, `--fix` con confirmación, `--json` (una línea por comprobación), salida distinta de 0 con algún FAIL. Comprobaciones nuevas: releases y su protocolo, enlaces `cc-*` apuntando a `bin/comandos`, unidades sin `ExecStart` hacia el repo, modo de cada dominio (`comandos state status`), lint del HOME (restos de D9). Quitadas: ttyd, Python, `pyobjc`.

### Task C2: `comandos x` (ccx), `keys`, `winstart`, `raise`, `next`, `snapshot`

**Files:** `crates/comandos-cli/src/{x,keys,winstart,raise,next,snapshot}/…`; alias `ccx`, `cc-keys`, `cc-winstart`, `cc-centro` (→ `raise app`), `cc-term` (→ `raise term`), `cc-next`, `cc-session-snapshot`.

- `x` (`bin/ccx`, 132): lista sesiones, abre o retoma la sesión del proyecto (búsqueda a 2 niveles en `~/codebase`, sin distinguir mayúsculas), `kill <nombre>` (solo de la sesión de ese proyecto y por el `Tmux` recibido), `-a <harness>`.
- `keys` (`bin/cc-keys`, 33): instala la llave pública con `ssh-copy-id` en los hosts de `~/.ssh/config` que piden contraseña; nunca guarda contraseñas.
- `winstart` (`bin/cc-winstart`, 83, solo WSL): genera y quita el `.lnk` del menú Inicio.
- `raise app|term` (`cc-centro`, `cc-term`): `wmctrl -x -a comandos` (o `kitty`/`tilix` para `term`) y, si no hay ventana, `setsid -f comandos-app` o `comandos next`.
- `next` (`bin/cc-next`, 60 Python): elige la sesión más urgente de `state/` (ahora vía `StatusDir`) y pide foco a `/focus`; `--dry`.
- `snapshot` (`bin/cc-session-snapshot`, 75 Python): guarda layouts ahora con `comandos_runtime::tmux_snapshot` (2f Task 3); el modo puente para una app GTK vieja se mantiene mientras `bin/cc-app` Python pueda estar vivo.

### Task C3: `comandos acp`

**Files:** `crates/comandos-cli/src/acp/{mod,protocol,agents,prompt}.rs` (o crate `comandos-acp` si supera ~1 500 líneas), alias `cc-acp`; prueba `crates/comandos-cli/tests/acp.rs`.

Port de `bin/cc-acp` (444) + `lib/acp.py` (448): cliente Agent Client Protocol que corre dentro de un pane y habla con Claude Code, Codex, Grok Build, OpenCode y Antigravity; banderas `--agent --model --effort --account --resume`; comandos del prompt; registro en `acp-panes.json` (dominio `ui-docs`, por `DocHandle`). Prueba diferencial con un agente ACP falso (binario de prueba que habla JSON-RPC por stdio) y transcripciones de los dos lados.

### Task C4: `comandos mobile`

**Files:** `crates/comandos-cli/src/mobile/…`, alias `cc-mobile`.

Port de `bin/cc-mobile` (150): `tailscale serve` solo hacia la tailnet (nunca Funnel), TLS automático, token de `dash-token`, QR con `qrencode` si existe. Rutas tras la Fase 3: solo `/` → 4777 (la terminal va por `/term/ws` del mismo frente); **no** publica 4779 ni 4780. El controlador aplica el cambio de `tailscale serve` en vivo (cambia el acceso remoto de Jesús) solo con su OK; vuelta atrás: `tailscale serve` con la configuración previa guardada por el propio comando en `backups/<fecha>/tailscale-serve.json`.

### Task C5: `xtask` para las herramientas de desarrollo, y `codex` si llegó a `main`

**Files:** `xtask/src/{css_orphans,cli_catalog}.rs`; `crates/comandos-cli/src/codex/…` si procede.

- `tools/css_orphans.py` → `cargo xtask css-orphans` (misma salida).
- `tools/cli-commands/{build,builtin_filter,scrape_prefix,verify_names}.py` → `cargo xtask cli-catalog` que regenera `config/cli-commands.json` **idéntico** al actual (prueba: regenerar y comparar bytes).
- `tools/png_diff.py` → ya sustituido por `xtask png-diff` (Fase 3 T2): solo se comprueba.
- `tools/analytics_extract.cjs`, `tools/analytics_fixture.cjs`, `tools/analytics_scope_css.py`: generaban `dash/analytics-render.js` y `dash/analytics.css` desde el mockup. Con la analítica en `comandos-web` (Fase 3) ya no hay JS que generar; el CSS generado se queda como archivo (es diseño). Se retiran sin sustituto si la Fase 3 no los usa (lo verifica R1); si la Fase 3 sigue regenerando `analytics.css` con ellos, se portan a `cargo xtask analytics-css`.
- `bin/cc-codex-full-access` y `lib/codex_*.py`: si al empezar están en `main`, se portan a `comandos codex full-access|thread-release|yolo-install|yolo-policy` con prueba diferencial; si siguen sin commitear en el checkout principal, no forman parte de esta fase y se anota en `docs/verification/retiro.md`.

Commit de cada tarea del grupo C: `feat(cli): comandos <x> sustituye a <original>` con las rutas tocadas.

---

## Grupo I — `comandos install` como único instalador

### Task I1: Plan de instalación con ensayo, recursos embebidos y plataformas

**Files:**
- Create: `crates/comandos-cli/src/install/{plan,assets,platform,hooks_register,cleanup}.rs`
- Modify: `crates/comandos-cli/src/install.rs` (sin argumentos = instalación completa; `--dry-run`; `--cleanup-legacy`; `--restore-legacy <fecha>`; `--extensions`)
- Test: `crates/comandos-cli/tests/install_full.rs`

**Interfaces:**
- Produces: `pub enum Action { Mkdir(PathBuf, u32), Link { name: String, at: PathBuf }, WriteIfAbsent { path: PathBuf, bytes: &'static [u8], mode: u32 }, WriteUnit { name: String, bytes: Vec<u8> }, RegisterClaudeHooks, AgentsSetup, BuildModelProxy, InstallFonts, DesktopEntry, Systemctl(Vec<String>), LaunchAgent, WslDeps(Vec<String>) }`; `pub fn plan(home: &Path, platform: Platform, release: &Path) -> Vec<Action>`; `pub fn apply(actions: &[Action], dry_run: bool) -> Result<Vec<String>, String>` (cada acción registra su vuelta atrás con `record.rs`).

Reglas (sustituye `install.sh`, `lib/platform.sh`, `lib/retire-telegram.sh`, `scripts/install-extensions.py`):
- Plataforma: `linux-native`, `linux-wsl-ubuntu`, `linux-other`, `darwin` (mismas reglas de detección que `cc_platform` de `lib/platform.sh`: `uname`, `/proc/sys/kernel/osrelease` con `microsoft`, `/etc/os-release`).
- Enlaces: todos los nombres de `ALIASES` (incluidos los de C y B5) en `~/.local/bin` o `~/.claude/hooks` → `~/.local/share/comandos/bin/comandos`; `cc-app` → binario `comandos-app` de la release.
- Recursos embebidos con `include_bytes!` (fuentes de `assets/fonts`, icono, sonidos, `dash/comandos.desktop.in`, plantillas de unidades de `systemd/`, `config/terminal-replies.conf`, `config/tmux.conf`, `config/kitty.conf`, `hooks/cc-notify.conf.example`): se escriben **solo si no existen**; si existe un enlace al repo (estado de hoy), se deja (Jesús lo eligió así y el archivo del repo no se retira: es configuración).
- Unidades: `cc-dash`, `cc-notifyd`, `cc-proxy`, `comandos-broker` con `ExecStart` hacia `%h/.local/share/comandos/bin/comandos …` (o el binario de app/proxy). **`tmux.service` nunca se reescribe** ni se recarga si ya existe (regla 1 del controlador); solo se escribe en una instalación nueva. Si una unidad existente es un enlace al repo con el mismo contenido que la plantilla, se sustituye por un archivo regular idéntico y `systemctl --user daemon-reload` (no reinicia nada); si el contenido difiere, se informa y no se toca.
- `RegisterClaudeHooks`: port de `cc_register_claude_hooks` (`lib/platform.sh:135-180`): mismas entradas en `~/.claude/settings.json`, respaldo previo, idempotente, sin tocar otras claves.
- `BuildModelProxy`: igual que `install.sh:90-104` (compila `vendor/claude-codex` solo si falta el binario o hay fuentes más nuevas).
- WSL: dependencias por `apt` con confirmación (`ask_yn`) e instrucciones de `wsl.conf` si falta systemd (mismos textos que `install.sh:15-56`); nunca `sudo` sin confirmación interactiva.
- `--extensions`: port de `scripts/install-extensions.py` (73).
- Telegram: `COMANDOS_RETIRE_TELEGRAM=1` ejecuta el port de `cc_retire_telegram`.
- `--cleanup-legacy` (D9): mueve a `backups/<fecha>/home-legacy/` con manifiesto: `H/md2tg.py`, `H/notify.sh`, `H/telegram.env*`, `H/usage.db*`, `H/*.bak*`, `H/.*.tmp`, `H/cc-status.sh.bak-*`, `H/dash/` (si `comandos dash` sirve los estáticos embebidos: se comprueba con `GET /` del frente y la cabecera que la Fase 3 defina; si no, se deja), `~/.local/share/comandos/extensions-venv/` (solo si `domain_writers` no ve ningún `cc-extensions serve` Python vivo), y los enlaces de `~/.local/bin` que resuelvan al checkout del repo para artefactos con `status: retired` en `retirement.json`. `--restore-legacy <fecha>` lo deshace desde el manifiesto.

- [ ] **Step 1: Pruebas que fallan** (`install_full.rs`): diferencial contra `bash install.sh` sobre un HOME temporal con `PATH` falso (`systemctl`, `sudo`, `cargo`, `launchctl`, `fc-cache`, `wmctrl` que solo registran): comparar el árbol resultante (enlaces y archivos) con las diferencias documentadas (destinos de los enlaces: binario en lugar del repo; unidades regulares con `ExecStart` al binario); `second_run_is_noop` (`apply` devuelve «nada que hacer»); `dry_run_writes_nothing`; `tmux_service_untouched_when_present`; `cleanup_legacy_then_restore_is_identity`.
- [ ] **Step 2–4:** fallan, implementar, pasan.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-cli
git commit -m "feat(cli): comandos install completo con ensayo, recursos embebidos, plataformas y limpieza reversible del HOME

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task I2: Independencia del checkout

**Files:** `crates/comandos-cli/src/install/plan.rs` (acción `RetargetAgentConfigs`), prueba `crates/comandos-cli/tests/install_retarget.rs`.

Las configuraciones de agentes (`~/.codex/config.toml`, `~/.gemini/settings.json`, `~/.gemini/config/hooks.json`, `~/.gemini/antigravity-cli/settings.json`, `~/.config/opencode/opencode.json(c)`, `~/.claude/settings.json`, `~/.claude-accounts/*/settings.json`) que aún mencionen `<repo>/adapters/`, `<repo>/hooks/` o `<repo>/bin/` se reescriben a los enlaces de `~/.local/bin` o `~/.claude/hooks` (respaldo previo por archivo; solo se cambian esas cadenas, comparación del resto byte a byte). La aplica el controlador en vivo: los agentes leen sus hooks en cada evento, así que el cambio vale para el siguiente turno sin reiniciar ninguna sesión. Commit `feat(cli): install reescribe las rutas de agentes al binario instalado`.

---

## Grupo O — oráculos congelados

### Task O1: `comandos-oracle` y grabación de dorados

**Files:**
- Create: `crates/comandos-oracle/{Cargo.toml,src/lib.rs}` (crate de ayuda de pruebas, `publish = false`, solo `dev-dependency`)
- Modify: `crates/comandos-runtime/tests/support/python.rs`, `crates/comandos-server/tests/support/oracle.rs`, `crates/comandos-extensions/tests/support/mod.rs`, `crates/comandos-core/tests/usage_state_oracle.rs`, `crates/comandos-store/tests/usage_parity.rs` y demás pruebas que lancen `python3`/`bash` (lista con `grep -rln 'python3\|"bash"' crates/*/tests xtask/src`)
- Create: `xtask/src/test_map.rs` (`cargo xtask test-map`: qué módulo Python o script ejercita cada prueba Python de `tests/` y cada oráculo de `crates/`)

**Interfaces:**
- Produces: `pub fn oracle(name: &str, input: &serde_json::Value, run: impl FnOnce() -> Result<Vec<u8>, String>) -> Vec<u8>`: con `COMANDOS_ORACLE=record` ejecuta `run`, guarda `tests/golden/<name>/<sha256(input)[..16]>.json` (`{"input": …, "output_b64": …}`) y devuelve; con `replay` (por omisión si existe el dorado) lee el dorado; con `check` ejecuta y compara contra el dorado (falla si difieren); sin dorado y sin `record`, falla con «falta el dorado de <name>: COMANDOS_ORACLE=record».

- [ ] **Step 1:** prueba del propio crate (grabar, reproducir, `check` con deriva).
- [ ] **Step 2:** envolver cada llamada a oráculo existente con `oracle(...)`; las entradas deben ser deterministas (instantes fijos `NOW_MS`, zona fija): las que no lo sean se arreglan en la prueba.
- [ ] **Step 3:** `COMANDOS_ORACLE=record $C test --workspace` (sin red ni HOME real: las pruebas ya cumplen las Global Constraints) y luego `$C test --workspace` en modo reproducir; ambos en verde.
- [ ] **Step 4:** commit de los dorados con `git add -f crates/*/tests/golden` y el código.

### Task O2: Pruebas sin intérprete

- [ ] Quitar de las pruebas Rust toda ruta que dependa del original (el modo `check` queda disponible mientras exista el archivo, para quien lo quiera correr a mano); borrar los oráculos dentro de `crates/` (`crates/comandos-runtime/tests/fixtures/hooks/oracle/**`, `crates/comandos-extensions/tests/*.py`, `crates/comandos-core/tests/notification_oracle.py`, `crates/comandos-extensions/tests/fixtures/unicode14/upstream/**`, y los fixtures `regex_frontend`/`unicode14` que solo servían al motor de regex CPython ya descartado por la spec §3.3) con `git rm`; `xtask parity` pasa a reproducir las respuestas grabadas (`xtask/parity/frente.jsonl` con salidas) y pierde el modo que lanza `bin/cc-dash` (que la 2g ya apagó).
- [ ] `$C test --workspace` en verde con `PATH` sin `python3` (`env PATH=/usr/bin/false-python:$(…)`: se ejecuta con un directorio de `PATH` donde `python3` es un binario de prueba que sale 127) para demostrar que nada lo necesita. Commit `chore(retire): pruebas Rust sin oráculos Python ni bash`.

---

## Grupo R — retirada artefacto por artefacto

### Task R1: `cargo xtask retire-check` y la lista de comprobación en datos

**Files:**
- Create: `xtask/src/retire_check.rs`, `docs/verification/retirement.json`, `docs/verification/retiro.md`
- Test: `xtask/tests/retire_check.rs`

**Interfaces:**
- Produces: `cargo xtask retire-check <ruta-del-repo>… [--home DIR] [--proc DIR] [--repo-live DIR]` (`--repo-live` = checkout principal, al que apuntan hoy los enlaces); `pub struct Finding { pub kind: FindingKind, pub detail: String }` con `FindingKind::{HomeLink, Unit, AgentConfig, LiveProcess, LiveImporter, TmuxConf, Crontab, ReferencedByTracked, MissingReplacement}`.

Comprobaciones por artefacto (todas de lectura):
1. **HomeLink**: enlaces bajo `~/.local/bin`, `~/.claude/hooks` (y `dash/`), `~/.config/systemd/user`, `~/.config/kitty`, `~/.local/share/applications`, `~/.tmux.conf` que resuelven a `<repo-live>/<ruta>`.
2. **Unit**: archivos de `~/.config/systemd/user/*.service` y `/run/user/<uid>/systemd/transient/*.service` que contienen la ruta absoluta o, para `bin/`, su nombre en `ExecStart`.
3. **AgentConfig**: las configuraciones de I2 que contienen la ruta.
4. **LiveProcess**: `proc/*/cmdline` con la ruta absoluta (o el enlace del HOME que resuelve a ella).
5. **LiveImporter** (solo `lib/*.py`): procesos Python que ejecutan cualquier archivo del repo que importe ese módulo, directa o transitivamente (grafo por `import X`/`from X import` estático sobre `bin/` y `lib/`): un Python vivo puede importarlo más tarde.
6. **TmuxConf**: `~/.tmux.conf` y `config/tmux.conf` mencionan el nombre (p. ej. `cc-pane-model`).
7. **Crontab**: `crontab -l` (lectura) lo menciona.
8. **ReferencedByTracked**: otro archivo rastreado que no esté también en la lista de retirada y no sea de `docs/` lo menciona por ruta.
9. **MissingReplacement**: su fila de `retirement.json` no tiene `rust` o sus `verified_by` (nombres de prueba `crate::archivo::función`) no existen en el árbol.

`retirement.json` (una fila por artefacto de la tabla siguiente):

```json
{"version": 1, "rows": [
  {"path": "bin/cc-dash", "rust": "comandos dash (comandos-server)", "phase": "2a–2g", "verified_by": ["comandos-server::dash_native_*", "xtask::parity (reproducción)"], "status": "pending"}
]}
```

`status`: `pending` → `retired` cuando el `git rm` se ha hecho. `retiro.md` es la versión legible (la tabla de abajo con fecha y commit de retirada).

- [ ] **Step 1: Pruebas que fallan** con HOME, `/proc` y repo falsos: `retire_check_flags_live_importer_process` (un `python3 …/bin/cc-extensions serve x` vivo bloquea `lib/extension_proxy.py`), `flags_home_link_into_repo`, `flags_transient_unit`, `flags_tmux_conf_reference`, `clean_when_nothing_uses_it`, `missing_replacement_test_is_reported`.
- [ ] **Step 2–4:** fallan, implementar, pasan.
- [ ] **Step 5:** escribir `retirement.json` con todas las filas de la tabla siguiente en `pending`; commit:

```bash
git add xtask docs/verification/retirement.json docs/verification/retiro.md
git commit -m "feat(xtask): retire-check y lista de comprobación de retirada en datos

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Lista de comprobación de retirada

Para cada fila: (a) el sustituto está en vivo; (b) su verificación pasa; (c) `cargo xtask retire-check <ruta> --repo-live /home/someguy/codebase/0xJesus/ComandOS` (lo corre el controlador) sale limpio; (d) `git rm <ruta>`, `status: retired` y commit `chore(retire): <ruta> → <sustituto>`. «Verificación» nombra la evidencia específica además de los dorados de O1 y del `retire-check`. Las fases citadas como «2f-N» son supuestos sobre sub-planes que aún se están escribiendo; si al llegar aquí el dominio quedó en otro sub-plan, se corrige la fila.

**Ejecutables `bin/`**

| Artefacto | Sustituto Rust | Fase | Verificación específica |
|---|---|---|---|
| `bin/cc-dash` | `comandos dash` | 2a–2g | censo de declinaciones vacío (2f D7) y `cc-dash-legacy.service` parado ≥ 7 días |
| `bin/cc_usage.py` | `comandos_store::{usage, usage_read, usage_import}`, `comandos_core::usage_state` | 2e | `/usage/state` nativo sin declinar; ningún `python3 … cc_usage` vivo |
| `bin/cc-app` | `comandos-app` | 4 | `cutover-app.md`: pestañas idénticas tras reiniciar; ningún `python3 …/cc-app` vivo |
| `bin/cc-app-mac` | `comandos-app-mac` | 5 | `cutover-mac.md` (M6) cerrado o decisión de Jesús anotada |
| `bin/cc-notifyd` | popups de `comandos-app` / `comandos-notifyd` | 4 / 2f-5 | `cc-notifyd.service` con `ExecStart` al binario Rust ≥ 7 días |
| `bin/cc-acp` | `comandos acp` | 6 C3 | diferencial ACP; `acp-panes.json` por `DocHandle` |
| `bin/cc-browser-expose` | `comandos browser expose` | 5 B5 | diferencial con ssh falso |
| `bin/cc-browser-remote` | `comandos browser remote` | 5 B5 | `browser-e2e` contra el broker Rust |
| `bin/cc-browser-npx-guard` | `comandos browser npx-guard` | 5 B5 | prueba de reconocimiento de paquete |
| `bin/cc-extensions` | `comandos ext` | 1 | `pgrep -fc 'python.*cc-extensions serve'` = 0 (sesiones antiguas terminadas) |
| `bin/cc-extension-session` | `comandos ext session` | 2f-2 (si no, 6 C5) | lanzamiento con configuración privada verificada |
| `bin/cc-next` | `comandos next` | 6 C2 | diferencial `--dry` |
| `bin/cc-session-snapshot` | `comandos snapshot` | 6 C2 | layouts guardados idénticos a `tmux_snapshot` |
| `bin/cc-agents` | `comandos agents` | 6 C1 | diferencial `setup` en HOME temporal |
| `bin/cc-doctor` | `comandos doctor` | 6 C1 | `--json` con las comprobaciones nuevas |
| `bin/cc-keys` | `comandos keys` | 6 C2 | diferencial con `ssh`/`ssh-copy-id` falsos |
| `bin/cc-mobile` | `comandos mobile` | 6 C4 | `tailscale serve status` sin 4779/4780 |
| `bin/cc-winstart` | `comandos winstart` | 6 C2 | diferencial del `.lnk` generado |
| `bin/ccx` | `comandos x` | 6 C2 | diferencial con tmux privado |
| `bin/cc-centro` | `comandos raise app` | 6 C2 | `wmctrl` falso: mismos argumentos |
| `bin/cc-term` | `comandos raise term` | 6 C2 | idem |
| `bin/cc-pane-model` | ninguno (tmux usa `@ccmodel` desde 2026-07-12) | — | `retire-check` sin `TmuxConf`; después S5d deja de escribir `pane-models.txt` |
| `bin/cc-webterm` | `comandos dash` `/term/ws` | 2b / 3 | sin unidades transitorias `cc-webterm*` |
| `bin/cc-webterm-attach` | idem | 2b / 3 | idem; ningún `ttyd` vivo |
| `bin/cc-codex-full-access` | `comandos codex full-access` | 6 C5 | solo si está en `main` |

**Hooks y adaptadores**

| Artefacto | Sustituto | Fase | Verificación |
|---|---|---|---|
| `hooks/cc-notify.sh`, `hooks/cc-status.sh`, `hooks/cc-usage-tool.sh` | `comandos hook claude`, `claude-status`, `claude-usage` | 1 | `readlink ~/.claude/hooks/cc-*.sh` → binario desde el 4-oct |
| `hooks/cc-notify.conf.example` | plantilla embebida en `comandos install` | 6 I1 | no es código; se mueve a `crates/comandos-cli/assets/` |
| `adapters/agy-hooks.sh`, `codex-hooks.sh`, `codex-notify.sh`, `gemini-hooks.sh` | `comandos hook agy|codex-hooks|codex|gemini` | 1 | I2 aplicada: ninguna configuración apunta a `adapters/` |
| `adapters/agy-statusline.py`, `adapters/grok-hooks.py` | `comandos hook agy-status`, `grok` | 1 | idem; `~/.local/bin/grok-hooks.py` → binario |
| `adapters/opencode-comandos.js` | se queda como shim ≤ 3 líneas | §3.2 | lista blanca de A1 (`max_lines: 3`) |

**Módulos `lib/` (Python y bash)**

| Artefacto | Sustituto Rust | Fase |
|---|---|---|
| `lib/accounts.py` | `comandos_runtime::accounts` | 2d/2e |
| `lib/acp.py` | `comandos acp` | 6 C3 |
| `lib/agent_stop.py` | operaciones de sesión | 2f-2 |
| `lib/allocation.py` | `comandos_core::allocation` | 2b |
| `lib/analytics_week.py` | `comandos_core::analytics_week` | 2e |
| `lib/app_state.py` | `comandos_store::state` | 2b |
| `lib/browser_config_migration.py` | `comandos browser migrate-config` | 5 B5 |
| `lib/capabilities.py` | perfiles de sesión | 2f-2 |
| `lib/claude_trust.py` | lanzamiento de sesiones | 2f-1 |
| `lib/cli_catalog.py`, `lib/cli_help.py` | catálogos | 2f-3 |
| `lib/command_chains.py` | cadenas | 2f-3 |
| `lib/event_intake.py`, `lib/event_store.py` | `comandos_runtime::events_cli`, `comandos_store::intake` | 1/2b |
| `lib/extension_auth.py`, `lib/extension_catalog.py`, `lib/extension_metadata.py`, `lib/extension_proxy.py` | `comandos_extensions::{auth, catalog, metadata, serve}` | 1 |
| `lib/extension_launch.py`, `lib/extension_observations.py`, `lib/pane_extensions.py` | extensiones por pane | 2f-2 |
| `lib/focus_progress.py` | `comandos_core::focus`, `comandos_store::focus` | 2b |
| `lib/grok_state.py` | lectores de límites (`comandos_runtime::limits`) | 2e |
| `lib/gtk_tabstrip.py`, `lib/gtk_workspace.py` | `comandos-app` `ui/tabs`, `ui/workspace` | 4 |
| `lib/mcp_descriptions.py` | `comandos_core::mcp_descriptions` | 1 |
| `lib/model_catalog.py` | `comandos_runtime::model_catalog` | 2c |
| `lib/model_watch.py` | vigilante de modelos | 2f-3 |
| `lib/news_editions.py`, `lib/news_radar.py`, `lib/news_reading.py`, `lib/news_watch.py` | noticias | 2f-4 |
| `lib/notification_delivery.py` | `comandos_store::notifications`, `comandos_core::notifications` | 2b |
| `lib/operator_catalog.py`, `lib/operator_dispatch.py`, `lib/operator_receipts.py` | residuo del despachador | 2f-3 |
| `lib/pane_snapshot.py` | `comandos_runtime::pane_snapshot` | 2d |
| `lib/pane_typing.py` | `comandos_runtime::pane_typing` | 2c |
| `lib/platform.sh` | `comandos install` (`platform.rs`, `hooks_register.rs`) | 6 I1 |
| `lib/pomodoro.py` | `comandos_core::pomodoro`, `comandos_store::pomodoro` | 2b/2f-3 |
| `lib/providers.py` | `comandos_runtime::providers` | 2d |
| `lib/quick_terminal.py` | `comandos_runtime::quick_terminal` | 2d |
| `lib/retire-telegram.sh` | `comandos install` (`COMANDOS_RETIRE_TELEGRAM`) | 6 I1 |
| `lib/session_operations.py` | `comandos_runtime::session_operations` | 2c/2f-2 |
| `lib/session_profiles.py` | perfiles de sesión | 2f-2 |
| `lib/session_tabs.py` | registro de pestañas | 2f-1 |
| `lib/terminal_history.py` | `comandos_runtime::terminal_history` | 2c |
| `lib/terminal_panes.py` | `comandos_runtime::terminal_panes` | 2c/2f-1 |
| `lib/tmux_clipboard.py` | `comandos-app` `ui/clipboard` | 4 |
| `lib/tmux_snapshot.py` | `comandos_runtime::tmux_snapshot` | 2f Task 3 |
| `lib/tui_state.py` | `comandos_runtime::tui_state` | 2d |
| `lib/turn_state.py` | `comandos_core::turn` | 1 |
| `lib/web_push.py` | Web Push | 2f-3 |
| `lib/work_marks.py` | `comandos_core::work_marks`, `comandos_store::marks` | 2b |
| `lib/workspace_layout.py`, `lib/workspace_state.py` | `comandos_core::workspace`, `comandos_store::workspace` | 2b |
| `lib/codex_*.py` (4, si están en `main`) | `comandos codex …` | 6 C5 |

Verificación específica común a `lib/`: `retire-check` sin `LiveImporter` (el criterio que de verdad bloquea: un Python vivo que pueda importarlo) y la entrada del inventario `docs/rust-component-inventory.json` en `Migrado` con su prueba Rust.

**Interfaz web y JS (propio y de terceros)**

| Artefacto | Sustituto | Fase | Verificación específica |
|---|---|---|---|
| `dash/index.html`, `dash/term.html`, `dash/extensions.html` | plantillas `maud` de `comandos-web`, página de terminal de `comandos-term` | 3 | `comandos dash` no lee esos archivos (búsqueda en `crates/` y prueba con `COMANDOS_DASH_DIR` vacío: el tablero sigue sirviendo) |
| `dash/{analytics,analytics-render,chain-builder,command-sidebar,device-drafts,extensions,news-reader,notifications,pomodoro,push-settings,quick-terminal,session-config,ui-sounds,work-marks,workspace,workspace-dock,workspace-layout}.js` | componentes WASM de `comandos-web` | 3 | `xtask dom-diff`/`png-diff` de la Fase 3 en verde; ningún HTML servido los referencia |
| `dash/sw.js` | shim generado por `xtask web-build` | 3 | el service worker servido es el generado |
| `dash/vendor/markdown-it-15.0.2.umd.min.js`, `dash/vendor/purify-3.4.16.min.js` (+ `LICENSE`, `vendored.json`) | `pulldown-cmark` + `ammonia` en el servidor | 3 | nada servido los referencia |
| `assets/xterm/{xterm,addon-attach,addon-canvas,addon-fit,addon-ligatures-web,addon-web-links}.js`, `assets/xterm/xterm.css` | `comandos-term` (canvas) | 3 | la pestaña experimental de la Fase 3 también retirada |
| `assets/opentype/opentype.min.js` | `rustybuzz` + `swash` | 3 | idem |
| `assets/uisfx/uisfx-0.4.0.js` (+ licencia) | sonidos UI por Web Audio desde WASM | 3 | sonidos verificados en la Fase 3 |
| `dash/prototypes/**` (incluido `vendor/uisfx-0.4.0.js`), `design/*.html`, `design/sidebar-comparison/**`, `docs/research/extensiones-por-sesion/grok-ns.py` | etiqueta `archive/prototipos-2026-10` (D11) | 6 R6 | `git show archive/prototipos-2026-10:dash/prototypes/README-v1-grill.md` existe |
| `tools/*.py`, `tools/*.cjs`, `tools/cli-commands/*.py` | `xtask css-orphans`, `cli-catalog`, `png-diff` | 3 / 6 C5 | salidas idénticas |
| `scripts/install-extensions.py` | `comandos install --extensions` | 6 I1 | diferencial |
| `install.sh` | `comandos install` | 6 I1 | diferencial de árbol |
| `services/browser/broker.py` | `comandos-broker-mac` | 5 B | drenaje completado; unidad Python de macmini deshabilitada ≥ 7 días |
| `services/browser/apparmor/install-chrome-apparmor.sh` | `comandos-broker-mac apparmor install` | 5 B4 | `apparmor print` idéntico; el perfil se mueve a `crates/comandos-browser/assets/` (única copia) |
| `services/browser/comandos-browser.service` | plantilla `comandos-browser-rs.service` | 5 B4 | — |
| `tests/*.py` (175), `tests/*.cjs` (21), `tests/*.js` (5), `tests/*.mjs` (1), `tests/*.sh` (6) | pruebas Rust + dorados | 6 O | `xtask test-map`: cada prueba Python apunta a un módulo ya `retired` o a una prueba Rust equivalente |
| oráculos en `crates/**` (Hecho 6) | dorados | 6 O2 | `$C test` sin `python3` |

**Unidades systemd y piezas del sistema**

| Unidad / pieza | Destino | Verificación |
|---|---|---|
| `cc-dash-legacy.service` (y `systemd/cc-dash-legacy.service`) | se retira | `systemctl --user is-enabled` = `disabled`, sin proceso; el archivo del repo se borra y el del HOME se mueve al respaldo |
| `cc-dash.service` | se queda; `ExecStart` al binario (I1) | `systemctl --user cat cc-dash` sin rutas del repo |
| `cc-notifyd.service` | se queda; binario Rust | idem |
| `cc-proxy.service` | se queda (`cc-model-proxy`, Rust de tercero) | idem |
| `comandos-broker.service` | se queda | idem |
| `tmux.service` | se queda **intacta** (no se reescribe ni se recarga; plantilla solo para instalaciones nuevas) | el enlace al repo sigue válido porque `systemd/tmux.service` no se borra (es una unidad, no código) |
| `cc-webterm.service`, `cc-webterm-path.service` (transitorias, ttyd) | desaparecen con la Fase 3 | `systemctl --user list-units 'cc-webterm*'` vacío; `pgrep -x ttyd` vacío; `tailscale serve status` sin 4779/4780 |
| paquete `ttyd` (apt) | lo desinstala Jesús si quiere (`sudo apt remove ttyd`) | `comandos doctor` ya no lo pide |
| `comandos-browser.service` (macmini, Python) | deshabilitada en la Fase 5; se borra aquí | `ssh macmini 'systemctl --user is-enabled comandos-browser.service'` = `disabled` ≥ 7 días; archivo movido a un respaldo en macmini |
| `~/Library/LaunchAgents/com.0xai.cc-dash.plist` (Mac) | apunta a `comandos dash` (5 M5) | solo si hubo `MAC_HOST` |

### Tasks R2–R6: Lotes de retirada (paralelos entre sí)

Cada lote sigue los cuatro pasos de la lista de comprobación para cada una de sus filas y termina con `cargo xtask lint --report` (la cuenta baja) y `$C test --workspace` en verde.

- [ ] **R2 — interfaz web, JS y ttyd**: filas de «Interfaz web y JS» de la Fase 3 y las unidades transitorias. Precondición: Fase 3 completa y su cutover con ≥ 7 días.
- [ ] **R3 — ejecutables y `lib/`**: filas de `bin/` y `lib/`, en orden inverso de dependencia (primero los módulos sin importadores vivos). `cc-pane-model` aquí, y en el mismo commit S5d deja de escribir `pane-models.txt` (el archivo se mueve al respaldo con `--cleanup-legacy`).
- [ ] **R4 — hooks, adaptadores e instalador**: tras I1 e I2 en vivo; `install.sh`, `lib/platform.sh`, `lib/retire-telegram.sh`, `scripts/`, `hooks/*.sh`, `adapters/*` salvo el shim.
- [ ] **R5 — pruebas y oráculos**: tras O2; `tests/**` (los `.json`/`.txt` de fixtures que queden sin uso también).
- [ ] **R6 — servicios, macmini, prototipos y HOME**: `services/browser/**` (tras el drenaje de la Fase 5), etiqueta y borrado de prototipos (D11), unidad Python de macmini (controlador, con OK de Jesús), `comandos install --cleanup-legacy` en el HOME real (controlador; respaldo verificado antes).

Commits: uno por fila o por grupo de filas con el mismo sustituto, mensaje `chore(retire): <rutas> → <sustituto>`, con la fila actualizada a `retired` en `docs/verification/retirement.json` en el mismo commit.

---

## Task F1: Repo solo Rust, prueba permanente y cierre

**Files:**
- Create: `xtask/tests/repo_rust_only.rs`
- Modify: `docs/rust-component-inventory.json` (`"complete": true`, `"cutover": true`, cada entrada `Migrado` o `Retirado` con su sustituto), `docs/verification/retiro.md` (fecha y commit de cada fila), `README.md` y `README.es.md` (instalación = `comandos install`; sin Python, ttyd ni `pip`), `CLAUDE.md` (sin cambios de las reglas de tmux)

- [ ] **Step 1: Prueba permanente**

```rust
// xtask/tests/repo_rust_only.rs
//! Fase 6: el repo no contiene Python, bash ni JS propio fuera de la lista blanca de la spec §3.2.
#[test]
fn repo_is_rust_only() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let allow = xtask::lint::Allowlist::load(&root.join("xtask/lint-allowlist.json"));
    let Ok(allow) = allow else { panic!("lista blanca ilegible") };
    let report = xtask::lint::scan(&root, &allow);
    assert!(report.violations.is_empty(), "archivos no Rust: {:#?}", report.violations);
}

#[test]
fn retirement_list_is_closed() {
    let raw = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/verification/retirement.json"));
    let Ok(raw) = raw else { panic!("falta retirement.json") };
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap_or_default();
    let pending: Vec<_> = v["rows"].as_array().into_iter().flatten()
        .filter(|r| r["status"] != "retired" && r["status"] != "kept").collect();
    assert!(pending.is_empty(), "filas sin retirar: {pending:#?}");
}
```

- [ ] **Step 2:** `cargo xtask lint --strict` → salida 0; `$C test --workspace` → PASS (incluye las dos pruebas).
- [ ] **Step 3: Ensayos finales de vuelta atrás** (controlador, documentados en `docs/verification/retiro.md`): (a) `comandos install --rollback-release` a la release anterior y vuelta (con la guardia degradando y re-subiendo un dominio `unified`); (b) `comandos state demote ui-docs` y `flip` de nuevo; (c) `comandos install --restore-legacy <fecha>` de un elemento de la limpieza y vuelta a limpiar. Las tres con evidencia (`readlink`, `comandos state status`, diff de árbol).
- [ ] **Step 4: Reglas de oro con evidencia** (la verificación que pide `memory/migracion-rust-completa.md`): `readlink -f` de los enlaces `cc-*`, `tmux list-sessions` (mismo número que antes de la fase), `pgrep -fc python3` sin procesos del repo, RSS y número de procesos del sistema ComandOS frente a la línea base de la Fase 0/1 (`docs/verification/rss.jsonl`).
- [ ] **Step 5: Commit**

```bash
git add xtask/tests/repo_rust_only.rs docs/rust-component-inventory.json docs/verification/retiro.md README.md README.es.md
git commit -m "chore(retire): repo solo Rust — lint estricto permanente, inventario completo y ensayos de vuelta atrás

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Self-review

- **Cobertura de la spec:** §4.6 (tabla por dominio, migrador, respaldo con hash, `state rollback <fecha>`) → S1–S6, con la vuelta sin pérdida (`demote`) además del `rollback` destructivo que pide la spec; §3.1 filas «install.sh…» → I1/I2 y C5, «71 JSON + session-operations» → S (son ~80 archivos más cinco bases, Hechos 1–2), «173 tests Python + 25 JS» → O1/O2/R5; §3.2 excepciones → lista blanca de A1; §5 (sin romper nada, reversión) → etapas, preflight D5, guardia de releases, `retire-check`; §6 → dorados, `xtask lint`, ensayos; §7 fila 6 («repo sin Python/bash/JS propio; rollback probado») → F1; §3.3 (motor regex CPython fuera) → O2 borra sus fixtures.
- **Placeholders:** las tareas de port de herramientas (C) y los lotes de retirada (R2–R6) se dan por regla, con la cita de su original, su prueba diferencial y la fila de la lista de comprobación; las piezas nuevas con riesgo (escritura en espejo, traslado con centinela, lint, prueba permanente) llevan el código completo.
- **Tipos:** `Mode`, `mode_of`, `DocHandle`, `StatusDir`, `LogHandle` (S2) los usan S5 y S6; `MOVED_SENTINEL`, `open_unified`, `doc_put_if_newer`, `status_put_if_newer` (S1) los usan S3 y S4; `resolve_db`/`move_db`/`demote_db` (S4) los usa S6; `STATE_PROTOCOL`, `domain_writers`, `can_unify` (S2) los usan S6 e I1 (`--cleanup-legacy`); `Allowlist`, `scan` (A1) los usa F1; `retirement.json` (R1) lo usan R2–R6 y F1.
- **Review Focus:** 1 → `backfill_never_overwrites_newer_mirror_write` (S3); 2 → `resume_after_kill_between_domains` (S3) y `move_crash_before_sentinel_is_redone` (S4); 3 → `rollback_release_demotes_unified_domains` (S6) y `xtask state-drill`; 4 → `retire_check_flags_live_importer_process` (R1); 5 → `usage_move_refuses_when_estimate_exceeds_budget` (S4).
