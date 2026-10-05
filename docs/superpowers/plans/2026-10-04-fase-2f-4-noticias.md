# Fase 2f-4 — Noticias (corte `news`): plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** que el frente `comandos dash` sirva él mismo, byte a byte igual que el `cc-dash` Python, el lector de noticias (ediciones, fuentes, imágenes, notas, guardadas, chat y traducción con agentes) y que tenga el planificador de ediciones listo para encenderse con `--background=front`.

**Architecture:** el almacenamiento de noticias vive en app-state (`lib/news_editions.py`, `lib/news_reading.py` sobre `app_state.connect()`): las lecturas y escrituras cortas van por el worker de app-state (`BackendWorker<StateBackend>`); los trabajos de agente (chat, traducción, entrada de la edición) corren en hilos de sistema con su propia conexión, como el Python, limitados por un semáforo de 2. El cliente ACP del Python (`lib/acp.py`, la parte cliente) se porta a `comandos-runtime` para hablar con los agentes. El planificador (`EditionScheduler`) es una tarea de tokio que solo arranca con `Background::Front` (D6 del maestro).

**Tech Stack:** Rust 1.96, tokio 1.53 (`process`, `time`, `sync`), rusqlite 0.40, serde_json (`preserve_order`, `arbitrary_precision`), `reqwest` (ya en el workspace por la 2e) para las fuentes HTTP y el resumidor HTTP, `regex =1.13.1`.

**Spec:** `docs/superpowers/specs/2026-10-04-comandos-rust-complete-design.md` (§4.2). Plan maestro: `docs/superpowers/plans/2026-10-04-fase-2f-resto-del-tablero.md` (D2, D6, D12; Tareas 0–3). Oráculo: `bin/cc-dash` 784–1037 y 8389–8406, 9056 (líneas de `0aa4ae1`); `lib/news_editions.py` (1 208), `lib/news_reading.py` (327), `lib/news_radar.py` (937), `lib/news_watch.py` (333, portado en 2f-3/T6), `lib/acp.py` (512).

**Precondición:** Tareas 0–3 del maestro. Grupo **G4** (T1 → T2 → T3 → T4); rama `migration/rust-fase2f-news`. T4 usa `comandos_runtime::news_watch` de 2f-3/T6: si esa tarea no está fusionada, T4 espera (T1–T3 no).

---

## Rulings que aplican

1. Respuestas idénticas byte a byte (incluidas las imágenes de `/news/media/*` con su `Content-Type`).
2. `Decline` solo antes de efectos; en este corte el único efecto de las rutas POST es la escritura en app-state o el arranque de un trabajo de agente.
3. app-state por el worker; los trabajos de agente con conexión propia en su hilo (`app_state.connect()` + `migrate` del Python = `comandos_store::app_state::open` + `migrate` ya portados).
4. Agentes y red en hilos/tareas con los plazos del Python (`timeoutSeconds` del paso, 180 s por omisión; `read_article` 10 s; resumidor HTTP con el suyo).
5. Nada bloqueante en el runtime.
6. Paridad: `xtask/parity/2f/news.jsonl` + pruebas contra el oráculo.

## Global Constraints

- `$C` = `CARGO_TARGET_DIR=/home/someguy/codebase/0xJesus/ComandOS/.build/target nice -n 10 cargo <cmd> -j 6`. Antes de cada commit: `$C fmt --all -- --check`, `$C clippy --workspace --all-targets -j 6 -- -D warnings`, pruebas de los crates tocados.
- `git add <rutas>`; `git add -f xtask/parity/2f/news.jsonl`; mensajes en español con `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Comentarios en español, identificadores en inglés; sin `unsafe`/`unwrap`/`expect`/indexado fuera de pruebas; cero Python o bash nuevos. Solo los archivos que nombra cada tarea.
- **Regla tmux (vinculante, `CLAUDE.md`; el 4 de octubre a las 21:59 un subagente mató las ~20 sesiones vivas del usuario con `TMUX_TMPDIR=<dir borrado> tmux kill-server`):**
  - Todo tmux de prueba con socket explícito `-S <dir>/tmux-<uid>/default` (o `-L <etiqueta propia>`): `Tmux::private(dir)` en el frente, `TestHome::tmux_command()`/`support::run_tmux` en pruebas, `private_tmux(dir)` en `xtask`. **Nunca `TMUX_TMPDIR` solo.** (Este corte no usa tmux; la regla rige para el frente de prueba que lo arranca igual.)
  - El Python del oráculo y del gemelo corre solo con el `fakebin` que contiene el `tmux` guardián y el `systemd-run` falso.
  - Limpieza: `kill-server` con el `-S` propio y después borrar el directorio.
  - Prohibido `kill-server`, `kill-session`, `pkill tmux` o `kill -9 -1` sin `-S`/`-L` propio; prohibido ejecutar tmux a mano.
  - Cada tarea mutadora tiene sus apartados **Confinamiento** y **Efectos en vivo**.
- **Agentes:** ninguna prueba lanza un agente real (`claude`, `codex`, `cc-acp`…) ni hace red externa. Los agentes ACP de prueba son un binario falso que habla el protocolo con respuestas fijas (`support::news::FakeAcp`); las fuentes HTTP, un servidor local de la prueba.

## Mapa de rutas

| Ruta | Llave | Python | Tarea |
|---|---|---|---|
| GET `/news/latest` (prefijo) | Prefix | 8389 | T1 |
| GET `/news/editions`, `/news/edition`, `/news/media/<32hex>.<ext>`, `/news/source`, `/news/chat`, `/news/notes`, `/news/saved` | PathExact / PathPrefix | 8393–8406 | T1 |
| POST `/news/saved`, `/news/notes`, `/news/chat/note` | Raw | 9056 (`news_post` 906) | T2 |
| POST `/news/chat`, `/news/translate` | Raw | 9056 | T3 |
| Hilo `_news_editions_loop` | — | 1127 | T4 |

## Idempotencia y orden frente al Python (ruling 9)

| Ruta / hilo | Exactamente una vez | Mientras el Python también corre |
|---|---|---|
| `/news/saved` | Idempotente (fija el estado) | Mismas filas de app-state; SQLite serializa |
| `/news/notes` `add` | No idempotente (cada `add` crea una nota), igual que el Python | Ídem |
| `/news/notes` `update`/`delete`, `/news/chat/note` | `update` idempotente; `delete` repetido → `deleted: false`; `toggle` alterna (no idempotente), como el Python | Ídem |
| `/news/chat` | `start_chat` crea el mensaje pendiente en una transacción; el trabajo del agente completa ese `pending_id` una vez | Un chat iniciado por el Python lo completa el hilo del Python; `news_reading.recover` al arrancar marca los huérfanos (solo con `front`) |
| `/news/translate` | `start_translation` devuelve `start=false` si ya hay una en curso: un solo trabajo | Ídem |
| Planificador de ediciones | `claim_due_job` reclama cada edición en una transacción; `EditionScheduler.tick` sin solape en el proceso | Solo con `Background::Front` (D6): nunca dos planificadores |

## Review Focus

1. **Chat sin cadena de agentes configurada**: `503` con el texto de `make_asker` y **sin** mensaje pendiente creado (el Python llama a `_news_asker()` antes de `start_chat`). Prueba `chat_without_chain_writes_nothing` (T3).
2. **Más de dos trabajos de agente a la vez**: el tercero espera al semáforo; ninguno se pierde. Prueba `agent_jobs_limited_to_two` (T3).
3. **`/news/media/` con un nombre que no casa la expresión o un archivo ausente**: `404 {"error": "Imagen no encontrada"}` sin leer fuera de `media_dir()`. Prueba `media_rejects_traversal` (T1).
4. **El planificador con `legacy`** no arranca ni toca la base. Prueba `scheduler_only_with_front` (T4).

## Estructura de archivos

```
crates/comandos-store/src/news.rs                      (lecturas/escrituras de news_editions y news_reading)  T1–T2
crates/comandos-runtime/src/acp_client.rs              (cliente ACP: lib/acp.py, parte cliente)              T3
crates/comandos-runtime/src/news_agents.rs             (make_asker, make_lead_writer, prompts, _extract_json) T3
crates/comandos-runtime/src/news_editions.rs           (scheduler, fetch, radar, summarizer, build)          T4
crates/comandos-runtime/src/news_radar.rs              (lib/news_radar.py)                                   T4
crates/comandos-server/src/dash/native/news/{mod,read,write,agents,scheduler}.rs                             T1–T4
crates/comandos-server/tests/support/news.rs           (FakeAcp, siembra de ediciones)                       T1, T3
crates/comandos-store/tests/news_oracle.rs, crates/comandos-runtime/tests/{acp_client,news_editions}_oracle.rs
crates/comandos-server/tests/dash_native_news*.rs
xtask/parity/2f/news.jsonl
```

---

### Task 1: Lectura (`news/read.rs`)

Comportamiento portado:

- **GET `/news/latest`** (prefijo): `_read_json_quiet(H/news-watch.json) or {}`.
- **GET `/news/editions`** (`news_editions_payload` 794): `load_config` (`H/news-editions.json`), `config_status` (con el entorno del proceso: claves de API que mira; `env` = `opts.env` filtrado si la 2e lo expone, si no `std::env::vars`), `policy_from_config`, `latest_readable`, `list_editions(conn, 30, until_ms=now)`, cadena `agent:model`, `next_scheduled`, `policy` con 4 claves.
- **GET `/news/edition?id=`** (`news_edition_payload` 812): `parse_qs` → exactamente un `id` o `400 {"error": "Falta la edición"}`; `latest`; `^\d{4}-\d{2}-\d{2}@\d{2}:\d{2}$` (`\d` de Python en `str`: dígitos Unicode → `Decline` si aparece un dígito no ASCII); `get_edition`; `story_counts`.
- **GET `/news/media/<nombre>`** (`news_media`): `^[0-9a-f]{32}\.(png|jpg|webp|gif|avif)$`, `media_dir()` (`$XDG_STATE_HOME` o `~/.local/state` + `comandos/news-media`), tipo por extensión; cualquier fallo → `404 {"error": "Imagen no encontrada"}`; éxito → `_bytes(200, tipo, cuerpo)` (mismas cabeceras que la 2c usó para `/remote-qr.png` o, si la 2c no portó `_bytes`, sus cabeceras leídas de `bin/cc-dash`).
- **GET `/news/source`, `/news/chat`, `/news/notes`, `/news/saved`** (`news_get` 870) con `_news_int` (0 < n < 2**53, `int(str(v))`: espacios y `+`/`-` y `_` de Python incluidos; replicar `int()` de Python para cadenas: blancos alrededor, signo, dígitos ASCII con `_` entre dígitos; dígitos Unicode → `Decline`).
- Las funciones de `news_editions`/`news_reading` que se usan aquí van a `comandos_store::news` con las mismas sentencias SQL y el mismo orden de filas; las columnas JSON se decodifican como en el Python (`_json_obj`).

**Files:**
- Create: `crates/comandos-store/src/news.rs`; Modify: `crates/comandos-store/src/lib.rs`
- Create: `crates/comandos-server/src/dash/native/news/read.rs`; Modify: `crates/comandos-server/src/dash/native/news/mod.rs`
- Create: `crates/comandos-store/tests/news_oracle.rs`, `crates/comandos-server/tests/support/news.rs` (contenido), `crates/comandos-server/tests/dash_native_news.rs`
- Modify: `xtask/parity/2f/news.jsonl`

**Interfaces:**
- Produces: `comandos_store::news::{latest_readable, list_editions, next_scheduled, get_edition, get_source, story_counts, story_row, chat_history, list_notes, saved_stories, translation, load_config, config_status, policy_from_config}`; `NewsRoute::{Latest, Editions, Edition, Media, Source, Chat, Notes, Saved}`; `support::news::{seed_editions, FakeAcp}` (FakeAcp vacío hasta T3).

**Confinamiento:** sin tmux ni procesos; app-state y `news-media` bajo el HOME temporal (`XDG_STATE_HOME` apuntado al tempdir en ambos lados del gemelo).
**Efectos en vivo:** ninguno (lecturas).

- [ ] **Step 1: Pruebas que fallan** — `news_oracle.rs`: base sembrada con dos ediciones, fuentes, notas y chat (con `seed_editions`, que inserta por SQL las filas que produciría `_store` — copiar el esquema de `app_state.migrate`, ya portado); cada función contra el Python (`python3 -c` con `sys.path` en `lib/`, `news_editions`/`news_reading` sobre la misma base copiada). `dash_native_news.rs`: gemelo con las ocho rutas (incluidos `?id=latest`, `?id=x`, dos `id`, `/news/media/<válido>`, `<inválido>`, `../x`, `/news/notes?story=1&q=palabra`), comparar estado, cabeceras relevantes y cuerpo; `media_rejects_traversal`.
- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-store --test news_oracle && $C test -p comandos-server --test dash_native_news` · Expected: FAIL.
- [ ] **Step 3: Implementar**.
- [ ] **Step 4: Fixture** — `n-news-latest`, `n-news-editions`, `n-news-edition-falta`, `n-news-media-invalida`, `n-news-saved`.

```json
{"name":"n-news-edition-falta","method":"GET","path":"/news/edition","headers":{"Host":"127.0.0.1"},"volatile":[],"expect":"same"}
{"name":"n-news-media-invalida","method":"GET","path":"/news/media/nada.png","headers":{"Host":"127.0.0.1"},"volatile":[],"expect":"same"}
```

- [ ] **Step 5: Ver que pasa** · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-store/src/news.rs crates/comandos-store/src/lib.rs crates/comandos-store/tests/news_oracle.rs \
  crates/comandos-server/src/dash/native/news/read.rs crates/comandos-server/src/dash/native/news/mod.rs \
  crates/comandos-server/tests/support/news.rs crates/comandos-server/tests/dash_native_news.rs
git add -f xtask/parity/2f/news.jsonl
git commit -m "feat(dash): lector de noticias nativo (ediciones, fuentes, imágenes, notas, guardadas)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Escrituras sin agente (`news/write.rs`)

Comportamiento portado (`news_post` 906, ramas `/news/saved`, `/news/notes`, `/news/chat/note`): `set_saved`, `add_note`, `update_note`, `delete_note`, `toggle_chat_note` de `news_reading` (con su `_clip` y su `now`), en el worker de app-state; excepciones: `LookupError` → `404 {"error": str(exc).strip("'")}` (el `str()` de un `KeyError` lleva comillas: replicar `repr` de la cadena del `KeyError` y quitarle las comillas de los extremos), `ValueError` → 400, `RuntimeError` → 409; acción de nota desconocida → `400 {"error": "Acción de nota desconocida"}`.

**Files:**
- Modify: `crates/comandos-store/src/news.rs` (escrituras), `crates/comandos-store/tests/news_oracle.rs`
- Create: `crates/comandos-server/src/dash/native/news/write.rs`; Modify: `crates/comandos-server/src/dash/native/news/mod.rs`
- Modify: `crates/comandos-server/tests/dash_native_news.rs`, `xtask/parity/2f/news.jsonl`

**Interfaces:**
- Produces: `comandos_store::news::{set_saved, add_note, update_note, delete_note, toggle_chat_note, NewsError::{Lookup(String), Value(String), Runtime(String)}}`; `NewsRoute::{SavedPost, NotesPost, ChatNotePost}`.

**Confinamiento:** sin tmux ni procesos; app-state del HOME temporal de cada lado.
**Efectos en vivo:** las mismas filas de app-state que el Python.

- [ ] **Step 1: Pruebas que fallan** — oráculo de cada escritura (filas resultantes iguales, `now` inyectado en ambos); gemelo: guardar/quitar, nota `add` con texto vacío (400), `update` inexistente (404), `delete`, acción desconocida, `toggle` de chat inexistente.
- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-store --test news_oracle && $C test -p comandos-server --test dash_native_news` · Expected: FAIL.
- [ ] **Step 3: Implementar**.
- [ ] **Step 4: Fixture** — `n-news-notes-accion-mala`, `n-news-saved-inexistente`.
- [ ] **Step 5: Ver que pasa** · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-store/src/news.rs crates/comandos-store/tests/news_oracle.rs \
  crates/comandos-server/src/dash/native/news/write.rs crates/comandos-server/src/dash/native/news/mod.rs \
  crates/comandos-server/tests/dash_native_news.rs
git add -f xtask/parity/2f/news.jsonl
git commit -m "feat(dash): notas, guardadas y notas de chat de noticias nativas

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Chat y traducción con agentes (`news/agents.rs`)

Comportamiento portado:

- **Cliente ACP** (`lib/acp.py`, parte cliente: `agent_specs`, `open_session` —proceso hijo con `stdin`/`stdout` en JSON-RPC por líneas, `initialize`, `session/new`, `session/prompt`, notificaciones `session/update`, `session/request_permission` respondido por `deny_agent_tools`—, cierre del proceso al terminar) → `comandos_runtime::acp_client`, síncrono (corre en el hilo del trabajo). El directorio de trabajo `XDG_STATE_HOME/comandos/news-acp` 0700 y `COMANDOS_SILENT_AGENT=1`, como `_default_acp_open`.
- **`make_asker`** (cadena de pasos, etiqueta `agent:model`, error agregado «ningún agente respondió (…)», el `RuntimeError` de cadena ausente con su texto), **`_acp_text`**, **`_extract_json`**, **`_prompt`**, y de `news_reading`: `start_chat`, `chat_prompt`, `_sources_payload`, `_blocks_text`, `run_chat`, `start_translation`, `run_translation` (trozos de 40) → `comandos_runtime::news_agents` + `comandos_store::news`.
- **POST `/news/chat`**: `_news_asker()` primero (sin cadena → `503 {"error": str(exc)}`, sin escribir); `start_chat` (excepciones como en T2); trabajo de agente; `200 {"messages": chat_history}`.
- **POST `/news/translate`**: `_news_asker()`; `start_translation` → `(start, state)`; si `start`, trabajo; `200 {"translation": state}`.
- **Trabajo** (`_news_agent_job`): `ask` se construye **antes** (falla antes de prometer nada); hilo de sistema (`std::thread`, nombre `comandos-news-agent`) que adquiere un semáforo de 2 (`std::sync` con `Condvar`, o `tokio::sync::Semaphore` con `blocking_acquire` — el hilo no es del runtime), abre su conexión a app-state, `migrate`, corre el trabajo; excepción → `cc-dash news agent: <repr>` en stderr (con prefijo `comandos dash news agent:` en el frente).

**Files:**
- Create: `crates/comandos-runtime/src/acp_client.rs`, `crates/comandos-runtime/src/news_agents.rs`; Modify: `crates/comandos-runtime/src/lib.rs`
- Modify: `crates/comandos-store/src/news.rs`
- Create: `crates/comandos-server/src/dash/native/news/agents.rs`; Modify: `crates/comandos-server/src/dash/native/news/mod.rs`
- Create: `crates/comandos-runtime/tests/acp_client_oracle.rs`; Modify: `crates/comandos-server/tests/support/news.rs` (`FakeAcp`), `crates/comandos-server/tests/dash_native_news.rs`, `xtask/parity/2f/news.jsonl`

**Interfaces:**
- Produces: `acp_client::{AgentSpec, agent_specs(&Value) -> Map<String, AgentSpec>, Session::{open, new_session, prompt, close}}`; `news_agents::{Asker, make_asker(&Value, Opener) -> Result<Asker, String>, make_lead_writer}`; `NewsRoute::{ChatPost, TranslatePost}`; `support::news::FakeAcp` (binario compilado por la prueba o script `sh` que lee JSON-RPC por líneas y responde un texto fijo con un objeto JSON dentro; registrado en un `providers.json` de prueba como agente ACP `falso`).

**Confinamiento:** el único agente es `FakeAcp` en el `fakebin`; `news-editions.json` de la prueba declara `{"summarizer": {"kind": "acp", "agent": "falso"}}`; el registro de proveedores que lee el cliente ACP es el del repositorio (solo lectura) más el agente falso inyectado por `opts.repo_root` apuntando a una copia temporal del directorio `config/` (el Python del gemelo recibe la misma copia con `COMANDOS_REPO_ROOT` si lo honra; si no, la prueba diferencial del cliente ACP compara contra `lib/acp.py` con `agent_specs` sobre el registro de prueba y el gemelo solo cubre el caso sin cadena y la escritura previa). Sin tmux, sin red.
**Efectos en vivo:** lanza el agente ACP configurado en `news-editions.json` (el mismo binario y argumentos que el Python, directorio `news-acp`, sin herramientas: todo permiso se deniega) y escribe sus respuestas en app-state.

- [ ] **Step 1: Pruebas que fallan** — `acp_client_oracle.rs`: el mismo `FakeAcp` contra `acp.open_session(...)` del Python y contra `Session::open` (secuencia de mensajes enviados igual, respuesta igual, proceso cerrado); `dash_native_news.rs`: `chat_without_chain_writes_nothing` (503 y ninguna fila nueva en ambos lados), `chat_with_fake_agent_completes` (200 inmediato con el pendiente; a los ≤ 5 s el historial tiene la respuesta en ambos lados, comparada normalizando `ts`), `translate_twice_starts_once`, `agent_jobs_limited_to_two` (`FakeAcp` que tarda 1 s; cuatro chats; nunca más de dos procesos `FakeAcp` vivos a la vez, comprobado contando procesos hijos por `/proc`).
- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-runtime --test acp_client_oracle && $C test -p comandos-server --test dash_native_news` · Expected: FAIL.
- [ ] **Step 3: Implementar**.
- [ ] **Step 4: Fixture** — `n-news-chat-sin-cadena` (el arnés no tiene `news-editions.json`: 503 en ambos lados).
- [ ] **Step 5: Ver que pasa** · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-runtime/src/acp_client.rs crates/comandos-runtime/src/news_agents.rs crates/comandos-runtime/src/lib.rs \
  crates/comandos-runtime/tests/acp_client_oracle.rs crates/comandos-store/src/news.rs \
  crates/comandos-server/src/dash/native/news/agents.rs crates/comandos-server/src/dash/native/news/mod.rs \
  crates/comandos-server/tests/support/news.rs crates/comandos-server/tests/dash_native_news.rs
git add -f xtask/parity/2f/news.jsonl
git commit -m "feat(dash): chat y traducción de noticias con agentes ACP nativos (semáforo de 2)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Planificador de ediciones (`news/scheduler.rs`)

Precondición adicional: 2f-3/T6 fusionada (`comandos_runtime::news_watch`).

Comportamiento portado:

- `lib/news_editions.py` resto: `default_policy`, `normalize_url`, `slot_times`, `schedule_editions`, `reconcile`, `requeue_orphans`, `claim_due_job`, `run_due`, `_prepare`, `_groups`, `_opportunity`, `build_edition`, `_build`, `_store`, `_enrich`, `notice_text`, `recent_story_urls`, `read_article` (con `_public_host`: sin redirecciones, solo hosts públicos, 10 s, 400 000 bytes, 6 000 caracteres, `_TextExtractor`), `make_fetcher`, `media_dir`, `make_radar_fetcher`, `make_summarizer` (HTTP y ACP), `_make_acp_summarizer`, `_make_chain_summarizer`, `EditionScheduler` → `comandos_runtime::news_editions`; `lib/news_radar.py` (937) → `comandos_runtime::news_radar`. Pruebas diferenciales por función con reloj y red inyectados.
- **Bucle** (`_news_editions_loop`): `news_reading.recover` al arrancar; `EditionScheduler(connect, config, news_watch, notify=_news_edition_notice, fetch=make_radar_fetcher(news_radar, recent_urls=…))`; espera 120 s; `tick()` cada 60 s (error → stderr). `tick` sin solape (`try_lock`). Corre en un hilo de sistema (los pasos son síncronos y largos), registrado para parar al apagar con una bandera que se mira entre `tick`s. **Solo con `Background::Front`.**
- `_news_edition_notice` → `notice_emit("news_edition", título, cuerpo, source_event_id="news-edition:<id>")` (2e).

**Files:**
- Create: `crates/comandos-runtime/src/news_editions.rs`, `crates/comandos-runtime/src/news_radar.rs`; Modify: `crates/comandos-runtime/src/lib.rs`
- Create: `crates/comandos-server/src/dash/native/news/scheduler.rs`; Modify: `crates/comandos-server/src/dash/native/news/mod.rs`
- Create: `crates/comandos-runtime/tests/news_editions_oracle.rs`, `crates/comandos-server/tests/dash_native_news_scheduler.rs`

**Interfaces:**
- Consumes: T1–T3; 2f-3/T6 (`news_watch`); 2e (`notice_emit`); maestro T1 (`Background`).
- Produces: `news_editions::{EditionScheduler, run_due, read_article, make_fetcher, make_radar_fetcher, make_summarizer}`; `news::scheduler::start(&Native)` (la tarea final del maestro lo llama).

**Confinamiento:** fuentes y radar contra un servidor HTTP local de la prueba (el `resolver` de `_public_host` se inyecta para aceptar `127.0.0.1` solo en pruebas; el comportamiento por omisión rechaza hosts privados igual que el Python); resumidor = `FakeAcp` o un servidor HTTP local; reloj inyectado; app-state del HOME temporal. Sin tmux.
**Efectos en vivo (solo con `front`):** las peticiones HTTP a las fuentes y al resumidor que hace el Python, las filas de ediciones y el aviso `news_edition`; con `legacy`, ninguno.

- [ ] **Step 1: Pruebas que fallan** — `news_editions_oracle.rs`: `slot_times`, `schedule_editions`, `claim_due_job`, `run_due` con `fetch`/`summarize` falsos deterministas en ambos lados (mismas filas finales), `read_article` contra el servidor local (HTML con scripts, redirección rechazada, tamaño máximo), `normalize_url` con veinte URLs; `dash_native_news_scheduler.rs`: `scheduler_only_with_front` (con `legacy`, ninguna fila nueva tras forzar el reloj), `scheduler_tick_builds_edition` (con `front` y reloj adelantado, una edición `ready` y un aviso).
- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-runtime --test news_editions_oracle && $C test -p comandos-server --test dash_native_news_scheduler` · Expected: FAIL.
- [ ] **Step 3: Implementar**.
- [ ] **Step 4: Ver que pasa** · Expected: PASS.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-runtime/src/news_editions.rs crates/comandos-runtime/src/news_radar.rs crates/comandos-runtime/src/lib.rs \
  crates/comandos-runtime/tests/news_editions_oracle.rs \
  crates/comandos-server/src/dash/native/news/scheduler.rs crates/comandos-server/src/dash/native/news/mod.rs \
  crates/comandos-server/tests/dash_native_news_scheduler.rs
git commit -m "feat(dash): planificador de ediciones de noticias en el frente (solo con --background=front)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Self-review

- **Cobertura**: las 4 filas `news` del inventario del maestro y el hilo `_news_editions_loop`.
- **Confinamiento y efectos en vivo**: en T1–T4; ningún agente ni red reales en pruebas.
- **Nombres**: `comandos_store::news::*` (T1–T3), `acp_client`/`news_agents` (T3) usados por T4.
