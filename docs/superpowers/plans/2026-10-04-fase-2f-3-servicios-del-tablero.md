# Fase 2f-3 — Servicios del tablero (cortes `services` y `residue`): plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** que el frente `comandos dash` responda él mismo, byte a byte igual que el `cc-dash` Python, los ajustes del tablero, el acceso remoto por Tailscale y la terminal web, la configuración SSH, los catálogos de CLIs, cadenas y modelos de OpenCode, Web Push (inerte), `POST /pomodoro` con su planificador, `/models/latest` con el vigilante de modelos y el bucle de límites, y por último el residuo del despachador (todo lo que hoy cae en `super().do_GET()` o en los 404 finales), de forma que la clasificación de rutas ya no reenvíe nada.

**Architecture:** siete tareas casi independientes en módulos propios de `native/`: `settings.rs` (T1), `remote.rs` (T2), `ssh.rs` (T3), `catalog_cli.rs` (T4), `push.rs` + `background/pomodoro.rs` (T5), `background/models.rs` + `background/limits.rs` (T6) y `residue.rs` (T7). Los hilos de fondo del Python se vuelven tareas de tokio registradas en el `TaskTracker` (D12 del maestro) y gobernadas por `Background` (D6): con `legacy` solo arranca el planificador de Pomodoro (idempotente con el Python vivo); con `front`, todos.

**Tech Stack:** Rust 1.96, tokio 1.53 (`process`, `time`, `sync`, `net`), hyper 1.11, rusqlite 0.40, serde_json (`preserve_order`, `arbitrary_precision`), `regex =1.13.1`; `getrandom` para `secrets.token_urlsafe`.

**Spec:** `docs/superpowers/specs/2026-10-04-comandos-rust-complete-design.md` (§4.2). Plan maestro: `docs/superpowers/plans/2026-10-04-fase-2f-resto-del-tablero.md` (D2, D6, D9, D10, D12; Tareas 0–3). Oráculo: `bin/cc-dash` (líneas de `0aa4ae1`), `lib/cli_catalog.py`, `lib/cli_help.py`, `lib/command_chains.py`, `lib/model_watch.py`, `lib/news_watch.py`, `lib/model_catalog.py`, `lib/pomodoro.py`.

**Precondición:** Tareas 0–3 del maestro. Grupos: **G3a** (T1, T3, T4: independientes entre sí), **G3b** (T2, tras T1 por `access_token`), **G3c** (T5 → T6; T6 además tras T4) y **G6** (T7, al final, tras G1–G4 fusionados). Ramas `migration/rust-fase2f-services-<tarea>`.

---

## Rulings que aplican

1. Respuestas idénticas byte a byte (incluido `image/png` de `/remote-qr.png` byte a byte con el mismo `qrencode`).
2. `Decline` solo antes de efectos.
3. app-state por el worker (`push`, Pomodoro); base de uso por `Lane<UsageBackend>`.
4. Procesos con los plazos del Python en tareas de tokio: `tailscale` 8 s (estado) y 12 s (`serve`), `cc-webterm` 20 s (on) y 12 s (off), `pkill` 6 s, `qrencode` 5 s, `systemctl` 15 s; HTTP de salud 0,4 s; `urlopen` a `4778/notify` 2–3 s.
5. Nada bloqueante en el runtime (`scandir` de `/fs/dirs`, lecturas de binarios del vigilante, escrituras atómicas → `spawn_blocking`).
6. Paridad: `xtask/parity/2f/services.jsonl` y `xtask/parity/2f/residue.jsonl` + pruebas contra el oráculo.

## Global Constraints

- `$C` = `CARGO_TARGET_DIR=/home/someguy/codebase/0xJesus/ComandOS/.build/target nice -n 10 cargo <cmd> -j 6`. Antes de cada commit: `$C fmt --all -- --check`, `$C clippy --workspace --all-targets -j 6 -- -D warnings`, pruebas de `comandos-server` (y del crate que se toque).
- `git add <rutas>`; `git add -f xtask/parity/2f/<archivo>.jsonl`; mensajes en español con `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Comentarios en español, identificadores en inglés; sin `unsafe`/`unwrap`/`expect`/indexado fuera de pruebas; cero Python o bash nuevos. Solo los archivos que nombra cada tarea (D4 del maestro).
- **Regla tmux (vinculante, `CLAUDE.md`; el 4 de octubre a las 21:59 un subagente mató las ~20 sesiones vivas del usuario con `TMUX_TMPDIR=<dir borrado> tmux kill-server`):**
  - Todo tmux de prueba con socket explícito `-S <dir>/tmux-<uid>/default` (o `-L <etiqueta propia>`): `Tmux::private(dir)` en el frente, `TestHome::tmux_command()`/`support::run_tmux` en pruebas, `private_tmux(dir)` en `xtask`. **Nunca `TMUX_TMPDIR` solo.**
  - El Python del oráculo y del gemelo corre solo con el `fakebin` que contiene el `tmux` guardián (`support::oracle::tmux_guard`) y el `systemd-run` falso (`support::twin::SCOPE_RUNNER`).
  - Limpieza: `kill-server` con el `-S` propio y después borrar el directorio.
  - Prohibido `kill-server`, `kill-session`, `pkill tmux` o `kill -9 -1` sin `-S`/`-L` propio; prohibido ejecutar tmux a mano.
  - **`pkill` de la terminal web:** `webterm_off` sin el script ejecuta `pkill -f 'ttyd.*cc-webterm-attach'`. En pruebas `pkill` es un ejecutable del `fakebin` que solo anota sus argumentos; nunca el real.
  - Cada tarea mutadora tiene sus apartados **Confinamiento** y **Efectos en vivo**.
- **Tailscale:** ninguna prueba ejecuta el `tailscale` real. El `fakebin` lleva un `tailscale` generado por la prueba que responde según un guion (`status --json`, `serve status`, `serve …`) y anota sus argumentos. La máquina del usuario tiene servicios ajenos en 8444–8447: el frente nunca ejecuta `tailscale serve reset` (D9).

## Mapa de rutas

| Ruta | Llave | Python | Tarea |
|---|---|---|---|
| GET `/webterm-token`, `/conf` (prefijos), `/fs/dirs` (prefijo) | Prefix | 8262, 8468, 8410 | T1 |
| POST `/conf-set`, `/fs/mkdir`, `/open-path`, `/open-url`, `/notify-popup`, `/test` | Raw | 8819, 9080, 8781, 9067, 8684, 8915 | T1 |
| GET `/remote-state`, `/remote-qr.png` (prefijos) | Prefix | 8567, 8569 | T2 |
| POST `/remote-on`, `/remote-off`, `/remote-webterm-on`, `/remote-webterm-off` | Raw | 8924–8953 | T2 |
| GET `/ssh` (prefijo); POST `/ssh-add`, `/ssh-del`, `/ssh-update`, `/ssh-key-setup` | Prefix / Raw | 8466, 8955–8971, 9135 | T3 |
| GET `/commands/catalog` (prefijo), GET y POST `/chains`, GET `/opencode/models` (prefijo) | Prefix / Raw | 8604, 8615, 9141, 8284 | T4 |
| GET `/push/key`, POST y DELETE `/push/subscription`, POST `/push/test`; POST `/pomodoro` | PathExact / Raw | 8391, 9051, 8632, 9053, 8817 | T5 |
| GET `/models/latest` (prefijo) | Prefix | 8407 | T6 |
| Residuo (GET/HEAD estáticos y prefijos, `/operator*`, POST/DELETE desconocidos) | — | `super().do_GET()`, 8246, 8654, 9780, 8634 | T7 |

Hilos: `start_pomodoro_scheduler` (T5), `_notices_push_loop` (T5, inerte), `restore_requested_webterm` (T2), `_model_watch_loop` y `_limits_snapshot_loop` (T6).

## Idempotencia y orden frente al Python (ruling 9)

| Ruta / hilo | Exactamente una vez | Mientras el Python también corre |
|---|---|---|
| `/conf-set` | Reescribir la misma clave deja el mismo archivo | `flock(<conf>.lock)` con espera, el mismo que el Python |
| `/fs/mkdir` | `exist_ok=False` tras comprobar: dos iguales seguidas → la segunda devuelve la ruta (ya es dir) | Ídem |
| `/open-*`, `/notify-popup`, `/test` | No idempotentes (abren/suenan); el cliente no reintenta | — |
| `/remote-*` | Reaplicar `serve --set-path` es idempotente en Tailscale | Mismas órdenes; dos procesos no se pisan porque Tailscale serializa |
| `/ssh-add`, `/ssh-update`, `/ssh-del` | Bajo `flock(~/.ssh/config.lock)`; repetir un alta → «ya existe»; repetir una baja → «No existe» | Mismo candado que el Python |
| `/ssh-key-setup` | `has-session` antes: reutiliza `ssh-key-<host>` | — |
| POST `/chains` | Guardado por `slug` (reemplazo atómico) | Mismo directorio, escritura atómica |
| POST `/pomodoro` | `requestId` + `expectedRevision` (ya portado en `comandos_store::pomodoro`) | Transacciones SQLite; el planificador de cada proceso hace `settle_due` idempotente |
| Planificador Pomodoro | `settle_due` es una transacción por bloque vencido; el sonido se reclama una vez por dispositivo | Los dos planificadores pueden correr a la vez sin duplicar (D6) |
| Vigilante de modelos, bucle de límites, envío push, restaurar terminal web | Solo con `Background::Front` (D6) | Con `legacy`, solo el Python |

## Review Focus

1. **`/remote-off` nunca ejecuta `tailscale serve reset`** y sí `--https=443 off`, `--https=8443 off` y `cc-webterm off` (D9). Prueba `remote_off_never_resets` (T2).
2. **`/conf-set` mientras cc-app tiene el candado del archivo**: el frente espera sin bloquear el runtime y conserva las líneas de cc-app. Prueba `conf_set_waits_for_lock` (T1).
3. **`/ssh-update` con un alta inválida**: `~/.ssh/config` queda byte a byte igual. Prueba `ssh_update_invalid_keeps_file` (T3).
4. **`POST /pomodoro` despierta el planificador del frente** (equivalente de `_POMODORO_WAKE.set()`), así un bloque que vence antes de 30 s se cierra a tiempo. Prueba `pomodoro_post_wakes_scheduler` (T5).
5. **El residuo**: un GET `/stateX` (prefijo de una ruta nativa) responde lo mismo que el Python (que también lo trata con `startswith`), y un GET de un directorio sin barra final da el mismo `301`. Pruebas `prefix_quirks_match_python` y `directory_redirect_matches_python` (T7).

## Estructura de archivos

```
crates/comandos-server/src/dash/native/settings.rs        T1
crates/comandos-server/src/dash/native/remote.rs          T2
crates/comandos-server/src/dash/native/ssh.rs             T3
crates/comandos-server/src/dash/native/catalog_cli.rs     T4
crates/comandos-server/src/dash/native/push.rs            T5
crates/comandos-server/src/dash/native/background/{mod,pomodoro,models,limits}.rs   T5, T6
crates/comandos-server/src/dash/native/residue.rs         T7
crates/comandos-runtime/src/{cli_catalog,cli_help,command_chains,model_watch,news_watch}.rs   T4, T6
crates/comandos-server/tests/support/services.rs          (fakebin de tailscale/qrencode/xdg-open/pkill)   T1–T3
crates/comandos-server/tests/dash_native_{settings,remote,ssh,catalog_cli,push_pomodoro,background,residue}.rs
xtask/parity/2f/services.jsonl, xtask/parity/2f/residue.jsonl
```

---

### Task 1: Ajustes (`settings.rs`)

Comportamiento portado:

- **GET `/webterm-token`** (prefijo): `access_token` (4730): `TOKEN_FILE` (`H/webterm-token`, comprobar la constante) leído con `strip()`; vacío o ausente → `secrets.token_urlsafe(32)` (32 bytes de `getrandom`, base64 URL sin relleno) escrito con `O_WRONLY|O_CREAT|O_TRUNC` 0600 (`OpenOptions` + `mode(0o600)`). `200 {"token"}`. Un fallo de escritura es una excepción no capturada → 500.
- **GET `/conf`** (prefijo): `read_conf` (2d `providers::read_conf`, `Unsure` → `Decline`), `{k: conf.get(k, CONF_DEFAULTS.get(k, "1")) for k in sorted(CONF_KEYS)}` + `"_lang": ui_lang(conf)` (tablas `CONF_KEYS`, `CONF_DEFAULTS` y `ui_lang` copiadas del Python con su línea en el comentario).
- **POST `/conf-set`** (8819): validación literal (`VOLUME` dígitos 0–100; `CC_LANG ∈ {auto, es, en}`; otras de `CONF_KEYS` ∈ `{"0","1"}`) → `400 {"error": "Clave o valor invalido"}`; `write_conf_key` (7322) con `FileLock::acquire(CONF_PATH)` en `spawn_blocking`, `splitlines()`, reemplazo de toda línea no comentada con `split("=")[0].strip() == key`, `write_file_atomic` (conserva el modo de un archivo existente, como `files::write_text_atomic` de la 2c). `str(data.get("value", ""))` con `py::str_scalar` (contenedor → `Decline`, antes de efectos).
- **GET `/fs/dirs`** (prefijo, `fs_dirs` 620) y **POST `/fs/mkdir`** (9080, `fs_mkdir` 658): `_fs_safe` (comprobar su definición junto a `FS_HOME`: `realpath` dentro de `HOME`), `os.scandir` con `is_dir(follow_symlinks=False)`, ocultos, filtro por fragmento en minúsculas (`str.lower` → ASCII; nombre no ASCII con mayúsculas fuera de ASCII → `Decline`), orden `key=str.lower`, máximo 150. `query.get("path") or ["~"]` con `[:512]` en caracteres.
- **POST `/open-path`** (8781) y **`/open-url`** (9067): validaciones literales; `_open_url` (34): WSL (`/proc/sys/kernel/osrelease` contiene `microsoft`) y `wslview` → `wslview`; si no `xdg-open`; `procs::spawn_detached` (nueva sesión, salidas a `/dev/null`, errores ignorados); `/open-path` con `env=gui_env()`, `/open-url` con el entorno del frente.
- **POST `/notify-popup`** (8684, `desktop_popup` 475): `title` cadena no vacía tras `strip()` y `body` cadena, si no `400 {"error": "title y body son obligatorios"}`; `DESKTOP_NOTIFY != "1"` → `popup: false` sin red; si no, POST a `http://127.0.0.1:4778/notify` (plazo 2 s; cualquier fallo → `false`) con el cuerpo `json.dumps` literal (orden de claves del Python, `ensure_ascii`). El envío usa el `NotifyPost` de la 2e (sin campo nuevo en `NativeOptions`, D4): si trae URL configurable, las pruebas lo apuntan a un servidor propio; si no, se usa la constante y las pruebas siguen el plan B del **Step 0**.
- **POST `/test`** (8915, `play_test` 7287): `kind ∈ {voice, chime, done}`; `conf_volume`, `play_cmd` (comprobar sus definiciones: `pw-play --volume`/`paplay` según disponibilidad); `piper` + modelo → `sh -c <script literal>`; si no `spd-say -l es -i <vol*2-100>`; si no el error literal; sonidos con existencia del archivo. Todos con `procs::spawn_detached`.

**Files:**
- Modify: `crates/comandos-server/src/dash/native/settings.rs`
- Create: `crates/comandos-server/tests/support/services.rs` (contenido), `crates/comandos-server/tests/dash_native_settings.rs`
- Modify: `xtask/parity/2f/services.jsonl`

**Interfaces:**
- Consumes: maestro T1 (`procs::{spawn_detached, gui_env}`, `FileLock::acquire`); 2d `providers::read_conf`; 2e `NotifyPost`.
- Produces: `SettingsRoute::{WebtermToken, Conf, ConfSet, FsDirs, FsMkdir, OpenPath, OpenUrl, NotifyPopup, Test}`, `settings::{access_token, read_conf_view, write_conf_key, open_url}` (`open_url` lo usa 2f-1/T6 si llega después; si llega antes, 2f-1/T6 tiene su copia en `input.rs` y la que llegue segunda la sustituye por esta).

**Confinamiento:** sin tmux. `xdg-open`, `wslview`, `piper`, `spd-say`, `pw-play`, `paplay` son ejecutables del `fakebin` que anotan sus argumentos en `<HOME>/procs.log` y salen 0 (en ambos lados del gemelo); el frente los encuentra porque `opts.search_path` pone el `fakebin` primero. `/notify-popup` apunta a un servidor HTTP de la prueba que anota el cuerpo. Archivos solo bajo los HOME temporales.
**Efectos en vivo:** los del Python: `cc-notify.conf` reescrito bajo su candado, carpetas creadas dentro de `HOME`, programas lanzados en sesión nueva, un POST local a 4778, el token 0600.

- [ ] **Step 0** — Run: `grep -n 'notifyd_url\|struct NotifyPost' -r crates/comandos-server/src`. Anotar si existe la URL configurable; si no, usar la constante y en las pruebas comparar solo `popup: false` con `DESKTOP_NOTIFY=0` y el caso «4778 no responde» (`popup: false` en ambos lados, sin servidor), y probar el cuerpo enviado con una prueba unitaria de la función que lo construye contra `json.dumps` del Python.
- [ ] **Step 1: Pruebas que fallan**

```rust
//! services, T1: ajustes del tablero contra el Python (gemelo, sin tmux).
mod support;

use support::{services::seed_services, twin::Twin};

#[tokio::test]
async fn settings_routes_match_python() {
    let Some(t) = Twin::start("settings", seed_services).await else { return };
    for (method, path, body) in [
        ("GET", "/conf", ""),
        ("POST", "/conf-set", r#"{"key":"VOLUME","value":"55"}"#),
        ("POST", "/conf-set", r#"{"key":"VOLUME","value":"101"}"#),
        ("POST", "/conf-set", r#"{"key":"CC_LANG","value":"en"}"#),
        ("GET", "/fs/dirs?path=~/codebase", ""),
        ("GET", "/fs/dirs?path=~/codebase/nue", ""),
        ("GET", "/fs/dirs?path=/etc", ""),
        ("POST", "/fs/mkdir", r#"{"path":"~/codebase/nueva"}"#),
        ("POST", "/fs/mkdir", r#"{"path":"/tmp/fuera"}"#),
        ("POST", "/open-path", r#"{"path":"~/codebase"}"#),
        ("POST", "/open-path", r#"{"path":"relativa"}"#),
        ("POST", "/open-url", r#"{"url":"ftp://x"}"#),
        ("POST", "/open-url", r#"{"url":"https://example.com"}"#),
        ("POST", "/notify-popup", r#"{"title":"  ","body":"x"}"#),
        ("POST", "/test", r#"{"kind":"ruido"}"#),
        ("POST", "/test", r#"{"kind":"done"}"#),
    ] {
        t.request(method, path, body).await.assert_same();
        t.files_equal(&["cc-notify.conf", "procs.log"]).unwrap();
    }
    let token = |s: String| s.len();
    assert_eq!(token(t.get_text_a("/webterm-token").await), token(t.get_text_b("/webterm-token").await));
}

#[tokio::test]
async fn conf_set_waits_for_lock() {
    let home = support::TestHome::new("conf-lock");
    seed_services(&home);
    let conf = home.hooks().join("cc-notify.conf");
    let lock = comandos_server::dash::native::files::FileLock::acquire(&conf).unwrap();
    let legacy = support::FakeLegacy::start().await;
    let fr = support::front(&home, legacy.port, home.options()).await;
    let pending = tokio::spawn({
        let port = fr.port;
        async move { support::request_body(port, "POST", "/conf-set", "Content-Type: application/json\r\n", r#"{"key":"VOLUME","value":"40"}"#).await }
    });
    assert_eq!(support::get(fr.port, "/snippets").await.status, 200, "runtime libre mientras espera");
    std::fs::write(&conf, "OTRA=1\n").unwrap();
    drop(lock);
    assert_eq!(pending.await.unwrap().status, 200);
    assert_eq!(std::fs::read_to_string(&conf).unwrap(), "OTRA=1\nVOLUME=40\n");
    fr.stop().await;
}
```

`support::services::seed_services(home)` crea `cc-notify.conf`, `~/codebase/{a,B,.oculto}`, y el `fakebin` de la prueba con los ejecutables que anotan; `Twin::{request, get_text_a, get_text_b}` se añaden en `support/services.rs` como funciones sobre `&Twin` si `twin.rs` no las trae.

- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_settings` · Expected: FAIL.
- [ ] **Step 3: Implementar**.
- [ ] **Step 4: Fixture** — `xtask/parity/2f/services.jsonl`: `s-conf`, `s-conf-set-invalida`, `s-fs-dirs-fuera`, `s-open-url-invalida`, `s-notify-popup-invalida`, `s-test-invalido` (sin efectos).
- [ ] **Step 5: Ver que pasa** — Run: `$C test -p comandos-server --test dash_native_settings` · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-server/src/dash/native/settings.rs crates/comandos-server/tests/support/services.rs \
  crates/comandos-server/tests/dash_native_settings.rs
git add -f xtask/parity/2f/services.jsonl
git commit -m "feat(dash): ajustes del tablero nativos (conf, fs, abrir, popup, prueba de sonido, token)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Acceso remoto y terminal web (`remote.rs`)

Comportamiento portado (4843–5030):

- **`remote_state()`**: `tailscale_host` (`status --json` 8 s → `Self.DNSName` sin `.` final; si no, `status --self --json` y la expresión `[a-z0-9-]+(?:\.[a-z0-9-]+)*\.ts\.net` sobre la salida), `access_token` (T1), `tailscale serve status` 8 s (`stdout` si rc 0, si no `""`), `webterm_health` (GET a `127.0.0.1:4780/term/token` y `127.0.0.1:4779/token` con 0,4 s, `200 ≤ status < 400`; los puertos vienen de `opts` para poder apuntarlos a servidores de la prueba: si no hay campo, constantes y la prueba los deja cerrados), `remote_status_from_text` literal, `remote_urls` (`urllib.parse.quote(token, safe="")`), `qrAvailable` (`which qrencode`), `serveStatus`. Al final actualiza la caché.
- **`remote_state_cached(15)`**: caché `{at, data, refreshing}` en un `Mutex` del módulo; con foto vieja y sin refresco en curso → `tokio::spawn` del refresco (registrado en el `TaskTracker`) y responde la foto; sin foto → cálculo en línea.
- **GET `/remote-qr.png`**: URL vacía → `400 {"error": "No pude leer el host de Tailscale"}`; sin `qrencode` → 404; `qrencode -o <tmp>.png -s 8 -m 2 <url>` 5 s (archivo temporal con `tempfile`-equivalente en `std::env::temp_dir()` con nombre aleatorio, borrado al terminar) → `200 image/png` con los bytes; rc ≠ 0 → `500 {"error": stderr.strip() or "qrencode fallo"}`; excepción → `500 {"error": str(e)}`. `_bytes` del Python: comprobar sus cabeceras (`Content-Type`, `Content-Length`, `Cache-Control: no-store`) y reproducirlas.
- **POST `/remote-on`**: `remote_dashboard_on` (sin `tailscale` → «Tailscale no esta instalado»; `tailscale status` 8 s rc ≠ 0 → «Tailscale no ha iniciado sesion»; `serve_path("/", "http://127.0.0.1:4777")` con los tres intentos de 12 s); error → 400; `webterm_health`; si alguno sano → `serve_path("/term", …4780/term)` y `serve_path("/", …4779, https_port="8443")`; `200 remote_state()`.
- **POST `/remote-off`** (D9): `tailscale serve --https=443 off` 12 s, **`tailscale serve --https=8443 off`** 12 s (en lugar de `serve reset`), `webterm_off`; `200 remote_state()`.
- **POST `/remote-webterm-on`** / **`-off`**: `remote_dashboard_on` + `webterm_on` (script `~/.local/bin/cc-webterm` o `which cc-webterm` o la ruta; 20 s; rc ≠ 0 → `(stderr or stdout or "cc-webterm no arranco").strip()`; luego los dos `serve_path`); `webterm_off` (script `off` 12 s, o `pkill -f 'ttyd.*cc-webterm-attach'` 6 s).
- **Arranque** (`restore_requested_webterm`): con `Background::Front` y `WEBTERM_ENABLED_FILE` (comprobar la constante) presente → `webterm_on` en una tarea; con `legacy`, nada (lo hace el Python).
- `run_quiet`: cualquier excepción (incluido el plazo) → rc 1, `stdout ""`, `stderr = str(e)` (`TimeoutExpired` → `"Command '[...]' timed out after N seconds"`: reproducir el texto de `subprocess.TimeoutExpired.__str__` con la lista Python `repr` de los argumentos, porque puede acabar en un `error` de la respuesta).

**Files:**
- Modify: `crates/comandos-server/src/dash/native/remote.rs`
- Create: `crates/comandos-server/tests/dash_native_remote.rs`; Modify: `crates/comandos-server/tests/support/services.rs` (guion de `tailscale`), `xtask/parity/2f/services.jsonl`

**Interfaces:**
- Consumes: T1 (`access_token`), maestro T1 (`procs`, `Background`, `TaskTracker`).
- Produces: `RemoteRoute::{State, Qr, On, Off, WebtermOn, WebtermOff}`, `remote::start(&Native)` (restaurar terminal web; la tarea final del maestro lo llama).

**Confinamiento:** `tailscale`, `qrencode` (real si está instalado: es puro y local; si no, la prueba de QR compara el 404 en ambos lados), `cc-webterm` y `pkill` del `fakebin`; el `tailscale` falso responde con un guion por prueba (`status --json` con `{"Self":{"DNSName":"maquina.tail1.ts.net."}}`, `serve status` con texto que contiene o no las rutas) y anota cada llamada en `<HOME>/tailscale.log`. Ningún `tailscale` real, ningún `pkill` real.
**Efectos en vivo:** las órdenes de `tailscale serve` del Python salvo `serve reset` (D9: `--https=8443 off` en su lugar), `cc-webterm` y `pkill -f 'ttyd.*cc-webterm-attach'` (solo si no hay script), con los mismos plazos.

- [ ] **Step 1: Pruebas que fallan** — `remote_routes_match_python` (gemelo; `/remote-state` dos veces —segunda desde caché—, `/remote-qr.png`, `/remote-on` con Tailscale sin sesión → 400, con sesión → 200, `/remote-webterm-on` con `cc-webterm` que falla → 400 con su texto, `/remote-webterm-off`; comparar cuerpos —el `token` normalizado—, el PNG byte a byte y `tailscale.log`); `remote_off_never_resets`: `/remote-off` en el frente y el log contiene `serve --https=443 off` y `serve --https=8443 off` y **no** contiene `reset`; la respuesta es la de `remote_state()` posterior (comparada con la del Python **con el mismo guion**, que en el Python sí incluye la llamada a `reset`: se compara solo el cuerpo, no el log, y la diferencia del log se documenta en el cutover).
- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_remote` · Expected: FAIL.
- [ ] **Step 3: Implementar**.
- [ ] **Step 4: Fixture** — `s-remote-qr-sin-host` (el arnés enlaza `tailscale` a `/bin/false`: host vacío en ambos lados → 400).
- [ ] **Step 5: Ver que pasa** · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-server/src/dash/native/remote.rs crates/comandos-server/tests/dash_native_remote.rs \
  crates/comandos-server/tests/support/services.rs
git add -f xtask/parity/2f/services.jsonl
git commit -m "feat(dash): acceso remoto y terminal web nativos (sin tailscale serve reset, D9)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Configuración SSH (`ssh.rs`)

Comportamiento portado (7344–7542): GET `/ssh` (prefijo) = `ssh_config::parse` del maestro T3 (Parte C) sobre `ssh_config::read` (`None` → `[]`; no UTF-8 → 500). POST `/ssh-add` (`ssh_add`: `_ssh_block` con sus expresiones y su orden de validación, `makedirs(~/.ssh, 0o700)`, `flock(~/.ssh/config)` con espera, duplicado → `'<host>' ya existe en ~/.ssh/config`, `write_file_atomic` con 0600 si es nuevo), `/ssh-del` (`ssh_remove` + `_ssh_without_host`, textos «No hay ~/.ssh/config», «No existe», «Ese host comparte linea con otros; editalo a mano»), `/ssh-update` (`ssh_update`: quitar + agregar en una sola reescritura), `/ssh-key-setup` (`ssh_key_setup`: `SSH_HOST_RE` y entrada; `which ssh-copy-id`; `ssh_public_key_for_host`; `ssh-key-<host>`[:60] con `has-session`; el comando literal con `shlex.quote`; `systemd-run --user --scope --collect --quiet tmux new-session -d -s <s> -n setup <cmd>` 15 s si hay `systemd-run`; error → `stderr.strip() or "No se pudo crear la sesion"`). Todas: error → `400 {"error"}`, si no `200 {"ok": true}` (`/ssh-key-setup` con `"session"`). `data.get("host", "")` sin `str()` en `/ssh-del`: un no-cadena llega a `_ssh_without_host` y lanza → 500 (replicar).

**Files:**
- Modify: `crates/comandos-server/src/dash/native/ssh.rs`
- Create: `crates/comandos-server/tests/dash_native_ssh.rs`; Modify: `xtask/parity/2f/services.jsonl`

**Interfaces:**
- Consumes: maestro T3 Parte C (`ssh_config`), T1 (`FileLock`), `NativeOptions.scope` (2d) y `opts.tmux`.
- Produces: `SshRoute::{List, Add, Del, Update, KeySetup}`.

**Confinamiento:** `~/.ssh` del HOME temporal; `/ssh-key-setup` crea su sesión en el tmux privado (`-S`) vía `SCOPE_RUNNER` (`opts.scope` en el frente; `systemd-run` del `fakebin` en el Python) y `ssh-copy-id`/`ssh` son ejecutables del `fakebin` que solo anotan; la sesión `ssh-key-*` la mata el `Drop` de `TestHome` con su `-S`.
**Efectos en vivo:** `~/.ssh/config` reescrito atómicamente bajo su candado y una sesión tmux `ssh-key-<host>` con `systemd-run --user --scope`, exactamente como el Python.

- [ ] **Step 1: Pruebas que fallan** — `ssh_routes_match_python` (gemelo: alta válida, alta con `ProxyCommand` en `hostname` con salto de línea → «Caracteres no permitidos», alta duplicada, edición válida, baja, baja de host compartido, `/ssh-key-setup` sin llave → texto literal, con llave → sesión creada en ambos lados; comparar cuerpos, `~/.ssh/config` y `list-sessions` por `run_tmux`); `ssh_update_invalid_keeps_file`.
- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_ssh` · Expected: FAIL.
- [ ] **Step 3: Implementar**.
- [ ] **Step 4: Fixture** — `s-ssh-lista`, `s-ssh-add-invalido`, `s-ssh-del-inexistente`.
- [ ] **Step 5: Ver que pasa** · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-server/src/dash/native/ssh.rs crates/comandos-server/tests/dash_native_ssh.rs
git add -f xtask/parity/2f/services.jsonl
git commit -m "feat(dash): configuración SSH nativa (lista, alta, baja, edición, instalar llave)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Catálogos de CLIs, cadenas y modelos de OpenCode (`catalog_cli.rs`)

Comportamiento portado:

- **`lib/cli_catalog.py`** (213) y **`lib/cli_help.py`** (157) → `comandos_runtime::{cli_catalog, cli_help}`: `load_catalog`, `installed_versions` (cada `--version` con su plazo), `detected_commands` (lectura de binarios: `spawn_blocking`), `catalog_view`; `help_for` con su caché. Prueba diferencial por función.
- **`cli_catalog_payload(refresh)`** (1533): caché `_CLI_CATALOG` (catálogo, derivado por `(checkedAt, mtime_ns de providers.json, ventana de 600 s si no hay snapshot)`, `fallback` con TTL 600 s) en un `Mutex` del módulo; `snap` = `model-watch.json`. Con `refresh=1` el Python fuerza un ciclo del vigilante (`_force_model_watch_cycle`): **en esta tarea `?refresh=1` declina** (antes de cualquier efecto) y T6 lo cablea. GET `/commands/catalog`: `session[:80]`, `pane` → `agent_info_for_pane` (2d) si casa `PANE_RE`.
- **`lib/command_chains.py`** (130) → `comandos_runtime::command_chains`: `default_dir`, `list_chains`, `save_chain` (`ChainError` → 400; `OSError`/`UnicodeError` → `500 {"error": "No se pudo guardar la cadena: <Clase>"}` con el nombre de clase Python: `PermissionError`, `FileNotFoundError`, `OSError`, `UnicodeEncodeError`…, tabla de `io::ErrorKind` → nombre).
- **GET `/opencode/models`** (4711): stale-while-revalidate (900 s; en frío espera ≤ 8 s a un refresco en curso; el refresco = `_opencode_models_fetch`, comprobar su orden `opencode models` con su plazo de hasta 60 s) en una tarea registrada en el `TaskTracker`; `200 {"providers": data}` (`None` → `null`).

**Files:**
- Create: `crates/comandos-runtime/src/{cli_catalog,cli_help,command_chains}.rs`; Modify: `crates/comandos-runtime/src/lib.rs`
- Modify: `crates/comandos-server/src/dash/native/catalog_cli.rs`
- Create: `crates/comandos-runtime/tests/cli_catalog_oracle.rs`, `crates/comandos-server/tests/dash_native_catalog_cli.rs`; Modify: `xtask/parity/2f/services.jsonl`

**Interfaces:**
- Produces: `CatalogCliRoute::{Catalog, ChainsGet, ChainsPost, OpencodeModels}`; `catalog_cli::{catalog_payload, CLI_CATALOG}` (T6 usa la caché y el `refresh`).

**Confinamiento:** los CLIs (`claude`, `codex`, `grok`, `opencode`, …) son scripts del `fakebin` que responden `--version`/`--help`/`models` con texto fijo; el directorio de cadenas está en el HOME temporal. Sin tmux.
**Efectos en vivo:** ejecuta los mismos `--version`/`--help`/`opencode models` que el Python (mismos plazos) y escribe cadenas en el mismo directorio.

- [ ] **Step 1: Pruebas que fallan** — oráculo de `cli_catalog`/`cli_help`/`command_chains` por función; gemelo de las cuatro rutas (`/commands/catalog`, `?session=s&pane=%0`, GET y POST `/chains` válido/ inválido, `/opencode/models` en frío y desde caché); `catalog_refresh_declines_until_t6`.
- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-runtime --test cli_catalog_oracle && $C test -p comandos-server --test dash_native_catalog_cli` · Expected: FAIL.
- [ ] **Step 3: Implementar**.
- [ ] **Step 4: Fixture** — `s-chains-get`, `s-chains-post-invalida`, `s-commands-catalog` (con `volatile` para `versionsAt`).
- [ ] **Step 5: Ver que pasa** · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-runtime/src/cli_catalog.rs crates/comandos-runtime/src/cli_help.rs \
  crates/comandos-runtime/src/command_chains.rs crates/comandos-runtime/src/lib.rs \
  crates/comandos-runtime/tests/cli_catalog_oracle.rs \
  crates/comandos-server/src/dash/native/catalog_cli.rs crates/comandos-server/tests/dash_native_catalog_cli.rs
git add -f xtask/parity/2f/services.jsonl
git commit -m "feat(dash): catálogo de CLIs, cadenas y modelos de OpenCode nativos

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Web Push inerte, `POST /pomodoro` y planificador (`push.rs`, `background/pomodoro.rs`)

Comportamiento portado:

- **Push (D10):** `web_push.available()` es hoy `(False, "Push no disponible: falta pywebpush (ModuleNotFoundError); ver requirements-push.txt")`. Las cuatro rutas responden `503` con `{"available": false, "error": …}` (GET `/push/key`) o `{"ok": false, "error": …}` (las demás). **Step 0** verifica en esta máquina que `python3 -c 'import pywebpush'` sigue fallando; si alguien lo instaló, parar y avisar al controlador (las rutas dejarían de ser inertes). DELETE `/push/subscription`: el despachador de `do_DELETE` (security gate, `Content-Length` 0–64000 → 413 con `close`, JSON → 400 con `close`, no-objeto → 400) lo hace el residuo de la 2c/T7; esta tarea solo añade la entrada `Verb::Delete` exacta.
- **`_notices_push_loop`** (1038): con `available() == False` no hace nada; el frente no lo arranca (y lo anota en la lista de la 2g).
- **POST `/pomodoro`** (`pomodoro_post` 6674): `settings` (no-objeto → 400; `style` fuera de `POMODORO_STYLES` → 400; `set_focus_settings` por el carril de uso), `ack` (borrar `FOCUS_QUEUE`, errores ignorados), comando (`action` o `_pomodoro_legacy_command`) por `PomodoroStore::command` en el worker de app-state; `PomodoroError` → `(status, payload)`; **siempre** `wake()` del planificador del frente (el `finally`).
- **Planificador** (`pomodoro_scheduler_loop` 6549): al arrancar `pomodoro_adopt_legacy` e `import_legacy_history` (errores → línea en stderr con el texto del Python); bucle: `settle_due`, `delay = min(30, max(0.05, (deadline - now)/1000 + 0.02))`, error → 2 s; espera con `tokio::sync::Notify` (`notified()` con `timeout(delay)`). Corre con `legacy` **y** con `front` (D6). `BackgroundRunner` (`background/mod.rs`): arranca tareas según `Background`, las registra en el `TaskTracker`, las para al apagar.

**Files:**
- Modify: `crates/comandos-server/src/dash/native/push.rs`, `crates/comandos-server/src/dash/native/background/mod.rs`
- Create: `crates/comandos-server/src/dash/native/background/pomodoro.rs`
- Create: `crates/comandos-server/tests/dash_native_push_pomodoro.rs`; Modify: `xtask/parity/2f/services.jsonl`

**Interfaces:**
- Consumes: 2e `pomodoro.rs` (GET), `comandos_store::pomodoro::PomodoroStore`, maestro T1 (`Background`, `TaskTracker`).
- Produces: `PushRoute::{Key, SubscriptionPost, SubscriptionDelete, Test}`, `PomodoroPost`; `background::{BackgroundRunner, start(&Native)}`, `background::pomodoro::wake()`.

**Confinamiento:** sin tmux ni procesos; app-state y base de uso en el HOME temporal; reloj del planificador inyectado (`opts.clock_ms`).
**Efectos en vivo:** transacciones de Pomodoro (las mismas que el Python), borrado de `focus-queue.jsonl` con `ack`, y el planificador cerrando bloques vencidos (idempotente con el del Python).

- [ ] **Step 0** — Run: `python3 -c 'import pywebpush' 2>&1 | tail -1` · Expected: `ModuleNotFoundError: No module named 'pywebpush'`.
- [ ] **Step 1: Pruebas que fallan** — gemelo de las rutas push (los cuatro 503) y de `POST /pomodoro` (inicio de bloque, `expectedRevision` vieja → 409, `settings` con estilo inválido → 400, `ack`); `pomodoro_post_wakes_scheduler`: con reloj falso, un bloque de 1 s iniciado por POST se cierra en < 1,5 s de reloj real (sin esperar los 30 s); `two_schedulers_settle_once`: frente y Python (gemelo con la **misma** base) con un bloque vencido → una sola fila de cierre.
- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_push_pomodoro` · Expected: FAIL.
- [ ] **Step 3: Implementar**.
- [ ] **Step 4: Fixture** — `s-push-key`, `s-push-test`, `s-pomodoro-settings-invalido`.
- [ ] **Step 5: Ver que pasa** · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-server/src/dash/native/push.rs crates/comandos-server/src/dash/native/background/mod.rs \
  crates/comandos-server/src/dash/native/background/pomodoro.rs crates/comandos-server/tests/dash_native_push_pomodoro.rs
git add -f xtask/parity/2f/services.jsonl
git commit -m "feat(dash): POST /pomodoro y planificador nativos; Web Push inerte como hoy (D10)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: `/models/latest`, vigilante de modelos y bucle de límites (`background/models.rs`, `background/limits.rs`)

Comportamiento portado:

- **GET `/models/latest`** (prefijo): `_read_json_quiet(MODEL_WATCH_FILE) or {}` → 200.
- **`lib/model_watch.py`** (337) y **`lib/news_watch.py`** (333) → `comandos_runtime::{model_watch, news_watch}` (las funciones que usa el ciclo: `installed_versions`, `watch_models`, `watch_news`; `model_catalog.catalog_signature` ya está en `comandos_runtime::model_catalog` de la 2d o se añade aquí). `news_watch` hace red (feeds): plazos del módulo Python, en tareas de tokio.
- **Ciclo** (`_model_watch_cycle_locked` 701) bajo un `tokio::sync::Mutex` (= `_MODEL_WATCH_LOCK`) compartido con `catalog_cli` (T4): versiones baratas; escaneo completo si `force`, cambio o 6 h; calentar `CLI_CATALOG` (T4) y `cli_help`; `watch_models`; notificar novedades (`notice_emit` de la 2e + POST a 4778 con plazo 3 s); `watch_news` y su aviso; `heartbeatAt` con `write_json_file(indent=1)`; todo `except Exception: pass`.
- **Bucle** (`_model_watch_loop`): espera 90 s, ciclo, 600 s. Solo con `Background::Front`.
- **`?refresh=1` de `/commands/catalog`**: quitar el `Decline` de T4 y llamar a `force_cycle` (`_force_model_watch_cycle`: coalesce por `last_full >= started`).
- **Bucle de límites** (`_limits_snapshot_loop` 1330): cada 300 s `LimitsCache::refresh` de la 2e (`usage_provider_limits()` = refrescar la caché). Solo con `Background::Front` (D3 de la 2e: el heredado lo conserva hasta la 2g).

**Files:**
- Create: `crates/comandos-runtime/src/{model_watch,news_watch}.rs`; Modify: `crates/comandos-runtime/src/lib.rs`
- Create: `crates/comandos-server/src/dash/native/background/{models,limits}.rs`; Modify: `crates/comandos-server/src/dash/native/background/mod.rs`, `crates/comandos-server/src/dash/native/catalog_cli.rs`, `crates/comandos-server/src/dash/native/settings.rs` (entrada `/models/latest` si se agrupa allí; si no, en `models.rs`)
- Create: `crates/comandos-runtime/tests/model_watch_oracle.rs`, `crates/comandos-server/tests/dash_native_background.rs`; Modify: `xtask/parity/2f/services.jsonl`

**Interfaces:**
- Consumes: T4, T5 (`BackgroundRunner`), 2e (`LimitsCache`, `notice_emit`, `NotifyPost`).
- Produces: `ModelsRoute::Latest`; `background::models::{cycle, force_cycle, LOCK}`; `background::limits::start`.

**Confinamiento:** binarios de CLIs del `fakebin` con versiones y cadenas de modelos sintéticas; `news_watch` contra un servidor HTTP de la prueba (las URLs de feeds salen de su configuración en el HOME temporal; si están fijas en el código Python, la prueba usa un `resolver`/`base_url` inyectable en el port y el Python se compara solo en las funciones puras de parseo); el aviso a 4778 contra un servidor de la prueba; reloj del bucle inyectado. Sin tmux.
**Efectos en vivo:** con `front`: `model-watch.json` y su latido, avisos (app-state + popup), peticiones de feeds, refresco de límites cada 5 min (red a los proveedores: mismas llamadas que el Python, que deja de hacerlas en la 2g). Con `legacy`: ninguno salvo `/models/latest` (lectura) y `?refresh=1` (un ciclo forzado, como el Python).

- [ ] **Step 1: Pruebas que fallan** — oráculo de `installed_versions`/`watch_models` sobre el mismo `fakebin` y HOME gemelo (comparar `model-watch.json` normalizado); `models_latest_matches_python`; `cycle_coalesces`: dos `force_cycle` concurrentes → un escaneo; `background_legacy_starts_only_pomodoro`: con `legacy` el `BackgroundRunner` arranca solo el planificador.
- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-runtime --test model_watch_oracle && $C test -p comandos-server --test dash_native_background` · Expected: FAIL.
- [ ] **Step 3: Implementar**.
- [ ] **Step 4: Fixture** — `s-models-latest` (`setup` vacío; `model-watch.json` del arnés).
- [ ] **Step 5: Ver que pasa** · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-runtime/src/model_watch.rs crates/comandos-runtime/src/news_watch.rs crates/comandos-runtime/src/lib.rs \
  crates/comandos-runtime/tests/model_watch_oracle.rs \
  crates/comandos-server/src/dash/native/background/models.rs crates/comandos-server/src/dash/native/background/limits.rs \
  crates/comandos-server/src/dash/native/background/mod.rs crates/comandos-server/src/dash/native/catalog_cli.rs \
  crates/comandos-server/tests/dash_native_background.rs
git add -f xtask/parity/2f/services.jsonl
git commit -m "feat(dash): vigilante de modelos y bucle de límites en el frente (solo con --background=front)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

(Añadir `settings.rs` si `/models/latest` se puso allí.)

---

### Task 7: Residuo del despachador (`residue.rs`) — al final (G6)

Precondición: G1–G4 y T1–T6 fusionados; si no, las rutas aún declinadas de otros cortes aparecerían aquí como residuo.

Comportamiento portado:

- **GET `/operator*`** (8246) y **POST `/operator*`** (8654): `410 OPERATOR_RETIRED` (ya en `retired.rs` de la 2c? comprobar; si no, aquí con `Key::Prefix("/operator")`).
- **GET con prefijo de una ruta nativa** que la tabla no casa (p. ej. `/stateX`, `/confY`, `/sshZ`): el Python usa `self.path.startswith(p)` en el orden de `_do_GET`; el frente ya responde la mayoría con `Key::Prefix` de cada dominio. El residuo es lo que ninguna tabla casa: se calcula con el **orden de `_do_GET`** (lista literal de las ramas, 8246–8618, en `residue.rs` como `const GET_ORDER: &[Match]`) qué rama del Python atendería la ruta; si la rama es nativa, se llama a su `answer` (un `Key::Prefix` que faltó → bug de la tabla: la prueba `prefix_quirks_match_python` los encuentra); si ninguna, cae al estático.
- **Estático** (`SimpleHTTPRequestHandler.send_head`, `directory=DASH`): `translate_path` (quita consulta y fragmento, `unquote`, `posixpath.normpath`, descarta segmentos vacíos/`.`/`..` y con `/`), directorio sin `/` final → `301` con `Location: <ruta>/` (+ consulta si había), directorio con `index.html`/`index.htm` → ese archivo, directorio sin índice → `list_directory` (HTML literal de `http.server` 3.12: `<!DOCTYPE HTML>`, título `Directory listing for <path>`, `html.escape` y `urllib.parse.quote`, `text/html; charset=utf-8`), archivo inexistente → `send_error(404, "File not found")` (plantilla `DEFAULT_ERROR_MESSAGE` de `http.server`, `Content-Type: text/html;charset=utf-8`, `Connection: close`), archivo → `guess_type` (tabla de `mimetypes` del Python de esta máquina para las extensiones de `dash/`; la prueba `mime_table_matches_python` la compara) con `Last-Modified`, y `If-Modified-Since` → 304. Más el `end_headers` de `cc-dash` (`Cache-Control: no-store` salvo que ya se enviara) y las cabeceras propias que añada `_no_store`. HEAD: igual sin cuerpo (`do_HEAD` → `send_head`).
- **POST desconocido**: tras el preámbulo de `do_POST` (gate, cuerpo, no-objeto → 400, `/operator*` → 410, las ramas antes de 9466 que ya son nativas) viene la validación de sesión (`target::post_target`: `400 {"error": "Nombre de sesion invalido"}`) y al final `404 {"error": "Ruta desconocida"}`.
- **DELETE desconocido** (8634): gate, `Content-Length` 0–64000 (413 con `close`), JSON (400 con `close`), no-objeto (400), y `404 {"error": "No encontrado"}`.
- Tras esta tarea, `classify_with(native=true)` no devuelve `Forward` para ningún método/ruta (lo verifica la prueba `no_route_classifies_as_forward` de la tarea final del maestro).

**Files:**
- Modify: `crates/comandos-server/src/dash/native/residue.rs`
- Create: `crates/comandos-server/tests/dash_native_residue.rs`
- Modify: `xtask/parity/2f/residue.jsonl`

**Interfaces:**
- Consumes: todas las tablas de la 2f (para el orden de `_do_GET`), `router::static_path`, maestro T3 (`target::post_target`).
- Produces: `ResidueRoute::{GetFallback, HeadFallback, PostUnknown, DeleteUnknown}` con `Key::Prefix("/")` al final de `TABLES` (la tarea final del maestro fija ese orden).

**Confinamiento:** sin tmux salvo el `display-message` de lectura del preámbulo de POST (tmux privado `-S`); el directorio estático es una copia de `dash/` en el tempdir para las pruebas de listado (y el `dash/` real del repositorio, solo lectura, para la paridad).
**Efectos en vivo:** ninguno (solo lecturas).

- [ ] **Step 1: Pruebas que fallan** — `prefix_quirks_match_python` (para cada rama de `_do_GET` con `startswith`, la ruta + `X` y + `/x`; gemelo, cuerpos y estados iguales); `directory_redirect_matches_python` (`/prototypes`, `/prototypes/`, `/prototypes?x=1`); `missing_file_404_matches_python`; `mime_table_matches_python`; `post_unknown_matches_python` (`/nada` con sesión inválida y válida); `delete_unknown_matches_python` (cuerpo 64001 bytes → 413, `[]` → 400, `{}` → 404).
- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-server --test dash_native_residue` · Expected: FAIL.
- [ ] **Step 3: Implementar**.
- [ ] **Step 4: Fixture** — `xtask/parity/2f/residue.jsonl`: `r-get-prefijo-state`, `r-get-dir-sin-barra`, `r-get-inexistente`, `r-post-desconocido`, `r-delete-desconocido`, `r-operator-get`, `r-operator-post`.
- [ ] **Step 5: Ver que pasa** · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-server/src/dash/native/residue.rs crates/comandos-server/tests/dash_native_residue.rs
git add -f xtask/parity/2f/residue.jsonl
git commit -m "feat(dash): residuo del despachador nativo; la clasificación ya no reenvía

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Self-review

- **Cobertura**: las entradas `services` y `residue` del inventario del maestro (T1–T7) y los hilos `start_pomodoro_scheduler`, `_notices_push_loop`, `restore_requested_webterm`, `_model_watch_loop`, `_limits_snapshot_loop`.
- **Confinamiento y efectos en vivo** en T1–T7; `tailscale`, `pkill`, `cc-webterm`, abridores y reproductores siempre falsos en pruebas.
- **Dependencias internas**: T6 toca `catalog_cli.rs` (de T4) solo para quitar el `Decline` de `?refresh=1`; T7 depende de todo lo demás.
