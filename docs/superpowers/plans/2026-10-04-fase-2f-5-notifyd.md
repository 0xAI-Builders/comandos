# Fase 2f-5 — `comandos-notifyd` (popups propios en Rust): plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** sustituir `bin/cc-notifyd` (Python + GTK3, 1 139 líneas) por un binario Rust `comandos-notifyd` con el mismo contrato: `POST 127.0.0.1:4778/notify` con las mismas respuestas byte a byte, los mismos popups GTK3 (tema del tablero, pila de 8, uno por agente, arrastre y posición recordada, «Cerrar todas», cierre automático de los «te espera» ya atendidos) y las mismas acciones (teclas, texto y abrir, vía el tablero con caída a tmux).

**Architecture:** crate y binario nuevos `crates/comandos-notifyd` (D11 del maestro), así `comandos` sigue sin GTK. El hilo principal corre el bucle de GTK; un servidor HTTP mínimo (hyper sobre un runtime tokio `current_thread` en un hilo propio) recibe `/notify` y entrega el aviso al hilo de GTK con `glib::MainContext::channel` (o `invoke` del contexto por omisión); el barrido de «te espera» es otro hilo que consulta `GET /state` cada 3 s. La lógica sin GTK (validación, recortes, tema, Markdown a Pango, tablas, candidatos a cerrar, barrido, destino de las acciones) vive en módulos puros con pruebas diferenciales contra las funciones del Python.

**Tech Stack:** Rust 1.96, `gtk = "0.18"` (gtk3-rs; `glib`, `gdk`, `gdk-pixbuf` vía sus reexportaciones), tokio 1.53 (`rt`, `net`, `time`) + hyper 1.11 para el HTTP, `reqwest` (ya en el workspace) o `hyper` cliente para hablar con el tablero, serde_json. Sin `libnotify` (el camino nativo `NATIVE_NOTIFY=1` se trata en T3).

**Spec:** `docs/superpowers/specs/2026-10-04-comandos-rust-complete-design.md` (§8, validación de gtk3-rs) y Enmienda 4 (que este plan adelanta por D11). Plan maestro: `docs/superpowers/plans/2026-10-04-fase-2f-resto-del-tablero.md` (D11). Oráculo: `bin/cc-notifyd` (líneas de `0aa4ae1`).

**Precondición:** ninguna de la 2f: grupo **G5**, independiente (puede empezar ya). Rama `migration/rust-fase2f-notifyd`. Paquetes de desarrollo de GTK3 instalados (`pkg-config --modversion gtk+-3.0` ≥ 3.24); si faltan, el Step 0 de T1 lo detecta y la tarea se detiene con aviso al controlador (no se instala nada).

---

## Rulings que aplican

1. Respuestas HTTP idénticas en estado, cuerpo y `Content-Type`/`Content-Length`. Las cabeceras `Server` y `Date` que añade `BaseHTTPRequestHandler` (`Server: BaseHTTP/0.6 Python/3.x`, `HTTP/1.0`) **no** se imitan: ningún llamador las lee (`cc-notify.sh` usa `curl -s` sin mirar cabeceras; `cc-dash` y el frente usan `urlopen` y solo el estado). Diferencia aceptada y documentada en el cutover.
2. Mismo comportamiento visible del popup: textos (`TR` es/en), clases CSS y tokens del tema, orden de botones, tamaños (`WIDTH=320`, `MARGIN=14`, `TOP=48`), pila (`STACK_MAX=8`, «Cerrar todas» desde 2), gracia de 4 s, cierre automático, posición en `notifyd-pos.json`.
3. Nada bloqueante en el hilo de GTK: HTTP, `/state`, tmux y escrituras de posición fuera de él; vuelta al hilo de GTK por el canal.
4. Procesos y red con los plazos del Python: tmux 5 s, `wmctrl` 2 s, POST al tablero 5 s, `GET /state` 2 s, `GET /prefs` 1,5 s.

## Global Constraints

- `$C` = `CARGO_TARGET_DIR=/home/someguy/codebase/0xJesus/ComandOS/.build/target nice -n 10 cargo <cmd> -j 6`. Antes de cada commit: `$C fmt --all -- --check`, `$C clippy --workspace --all-targets -j 6 -- -D warnings`, `$C test -p comandos-notifyd`.
- `git add <rutas>`; mensajes en español con `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Comentarios en español, identificadores en inglés; sin `unsafe`/`unwrap`/`expect`/indexado fuera de pruebas (gtk3-rs no exige `unsafe` para nada de lo que se usa; si alguna API lo exigiera, se busca la alternativa segura o se para y se consulta); cero Python o bash nuevos.
- No se toca `systemd/cc-notifyd.service`, `~/.local/bin` ni el servicio vivo: el cambio de `ExecStart` lo hace la sección «2f» del cutover (tarea final del maestro), con el usuario.
- **Regla tmux (vinculante, `CLAUDE.md`; el 4 de octubre a las 21:59 un subagente mató las ~20 sesiones vivas del usuario con `TMUX_TMPDIR=<dir borrado> tmux kill-server`):**
  - Todo tmux de prueba con socket explícito `-S <dir>/tmux-<uid>/default` (o `-L <etiqueta propia>`). En este crate: `comandos-notifyd --tmux-socket <ruta>` hace que la caída a tmux use `tmux -S <ruta>`; las pruebas siempre lo pasan y crean su servidor con `-S` (ayudante `PrivateTmux` del crate, mismo patrón que `Tmux::private`). **Nunca `TMUX_TMPDIR` solo.**
  - Limpieza: `kill-server` con el `-S` propio y después borrar el directorio.
  - Prohibido `kill-server`, `kill-session`, `pkill tmux` o `kill -9 -1` sin `-S`/`-L` propio; prohibido ejecutar tmux a mano.
  - En producción (sin `--tmux-socket`) la caída usa `tmux` sin socket, como el Python: solo `send-keys`, `display-message`, `has-session` y `switch-client` sobre la sesión/pane del aviso.
  - Cada tarea mutadora tiene sus apartados **Confinamiento** y **Efectos en vivo**.
- **Pantalla:** las pruebas de GTK solo corren con `COMANDOS_GTK_TESTS=1` y una pantalla ya existente (`DISPLAY` o `WAYLAND_DISPLAY`); sin ellas se omiten con aviso. Nunca se arranca Xvfb ni otro servidor gráfico (regla de esta máquina). Ningún popup de prueba se queda abierto: cada prueba cierra sus ventanas.
- **Puerto 4778:** las pruebas nunca lo usan (`--port 0` y el puerto elegido se lee de stdout); el binario vivo sigue siendo el Python hasta el cutover.

## Decisiones de este sub-plan

- **N1 — Línea de arranque.** El Python imprime `cc-notifyd listo en 127.0.0.1:4778 (popups propios v3)`; el Rust imprime `comandos-notifyd listo en 127.0.0.1:<puerto> (popups propios v3)` (las pruebas leen el puerto de ahí). Ningún consumidor depende del texto.
- **N2 — `NATIVE_NOTIFY=1`.** Hoy desactivado por omisión (`_conf_value("NATIVE_NOTIFY", "0")`). Sin `libnotify` en Rust, el binario trata `NATIVE_NOTIFY=1` como el Python cuando `HAVE_NOTIFY` es falso: popup propio. Diferencia aceptada (solo si alguien activa la opción); anotada en el cutover.
- **N3 — Configuración por flags.** `--port` (por omisión 4778), `--dash-url` (por omisión `http://127.0.0.1:4777`), `--tmux-socket` (por omisión ninguno), `--hooks-dir` (por omisión `~/.claude/hooks`), `--repo-root` (por omisión el del binario instalado: `<exe>/../..` resuelto como el Python con `realpath(__file__)`; en el despliegue por enlace simbólico de `~/.local/bin`, el mismo cálculo).

## Estructura de archivos

```
crates/comandos-notifyd/Cargo.toml                 (bin comandos-notifyd; gtk 0.18, tokio, hyper, serde_json)   T1
crates/comandos-notifyd/src/main.rs                (flags, arranque, bucle GTK)                                 T1, T2
crates/comandos-notifyd/src/http.rs                (POST /notify)                                               T1
crates/comandos-notifyd/src/notice.rs              (Notice: recortes, conf, lang)                               T1
crates/comandos-notifyd/src/theme.rs, markup.rs, stack.rs, popup.rs, position.rs                                T2
crates/comandos-notifyd/src/actions.rs, sweep.rs                                                                T3
crates/comandos-notifyd/tests/{http,logic,actions,gtk}.rs, tests/support/mod.rs                                  T1–T3
Cargo.toml (workspace: miembro nuevo)                                                                           T1
```

---

### Task 1: Crate, `POST /notify` y el aviso normalizado

Comportamiento portado (`Handler.do_POST` 1075):

- Peer fuera de `{127.0.0.1, ::1, ::ffff:127.0.0.1}` → `403` sin cuerpo. `Origin` presente que no casa `^https?://(localhost|127\.0\.0\.1)(:\d+)?$` → `403`. Ruta ≠ `/notify` → `404`. `Content-Length` > 200 000 → `413`; `int()` inválido o JSON inválido → `400` (todos sin cuerpo, como `send_response` + `end_headers`). Otros métodos: `BaseHTTPRequestHandler` responde `501` con su HTML de error (`Unsupported method ('GET')`): reproducir estado, `Content-Type: text/html;charset=utf-8` y cuerpo con la plantilla `DEFAULT_ERROR_MESSAGE`.
- `POPUPS != "1"` (leído en cada petición de `cc-notify.conf`, `_conf_value`: primera línea que empieza por `POPUPS=`, valor sin espacios ni comillas) → `200` `{"ok": true, "popup": false}` con `Content-Type: application/json` y `Content-Length`.
- Si no: `Notice { title: str(title or "Claude Code")[:200], body[:400], session, kind (por omisión "done"), project[:80], options[:600], full[:60000], pane[:10] }` (recortes en **caracteres**; `str()` de Python sobre no-cadenas: `py_str` = `True`→`True`, números como Python, listas/objetos con `repr` de Python —`str(d.get(...))` de una lista da `['a']`—; se replica con una función `py_str(&Value)` probada contra el Python) → al hilo de GTK → `200 {"ok": true}`.
- `UI_LANG` (`_ui_lang`, una vez al arrancar): `CC_LANG` de la conf (`es`/`en`), si no `LANG` empieza por `es` → `es`, si no `en`.

**Files:**
- Create: `crates/comandos-notifyd/Cargo.toml`, `crates/comandos-notifyd/src/{main,http,notice}.rs`, `crates/comandos-notifyd/tests/http.rs`, `crates/comandos-notifyd/tests/support/mod.rs`
- Modify: `Cargo.toml` (miembro del workspace)

**Interfaces:**
- Produces: `notice::{Notice, py_str, conf_value(&Path, &str, &str) -> String, ui_lang(&Path, Option<&str>) -> Lang}`; `http::{serve(listener, sink: Sender<Notice>, hooks: PathBuf)}`; binario `comandos-notifyd` con `--headless` (solo para pruebas: sin GTK, los avisos aceptados se escriben como una línea JSON en stdout; permite probar el HTTP sin pantalla).

**Confinamiento:** el binario corre con `--headless --port 0 --hooks-dir <tempdir>`; las peticiones van al puerto elegido; el Python de comparación (`bin/cc-notifyd`) no se ejecuta como servidor (necesitaría GTK y el 4778): la prueba diferencial importa su `Handler` con `SourceFileLoader` y un `gi` falso (`sys.modules['gi']` sustituido por un módulo de la prueba con `require_version` y `repository` mínimos) y lo sirve en un `HTTPServer` de puerto efímero dentro del `python3 -c`, con `GLib.idle_add` capturando los argumentos. Sin tmux.
**Efectos en vivo:** ninguno salvo leer `cc-notify.conf` y entregar el aviso al hilo de GTK (igual que el Python).

- [ ] **Step 0** — Run: `pkg-config --modversion gtk+-3.0` · Expected: `3.24.x`. Si falla, parar y avisar al controlador (no instalar).
- [ ] **Step 1: Pruebas que fallan**

```rust
//! POST /notify contra el Handler del Python (gi sustituido), ambos en puertos efímeros.
mod support;

use support::{python_notifyd, rust_notifyd, Wire};

#[tokio::test]
async fn notify_responses_match_python() {
    let hooks = tempfile::tempdir().unwrap();
    std::fs::write(hooks.path().join("cc-notify.conf"), "POPUPS=1\n").unwrap();
    let Some(py) = python_notifyd(hooks.path()) else { return };
    let rs = rust_notifyd(hooks.path());
    let cases: &[(&str, &str, &[(&str, &str)], &str)] = &[
        ("POST", "/notify", &[("Content-Type", "application/json")], r#"{"title":"Hola ñ","body":"b","session":"s","pane":"%12345678901"}"#),
        ("POST", "/notify", &[("Origin", "https://evil.example")], "{}"),
        ("POST", "/notify", &[("Origin", "http://localhost:4777")], "{}"),
        ("POST", "/otra", &[], "{}"),
        ("POST", "/notify", &[], "no es json"),
        ("POST", "/notify", &[("Content-Length", "200001")], ""),
        ("GET", "/notify", &[], ""),
    ];
    for (method, path, headers, body) in cases {
        let a: Wire = rs.request(method, path, headers, body).await;
        let b: Wire = py.request(method, path, headers, body).await;
        assert_eq!((a.status, a.content_type(), a.body.clone()), (b.status, b.content_type(), b.body.clone()), "{method} {path} {headers:?}");
    }
    assert_eq!(rs.accepted(), py.accepted(), "mismos avisos entregados (recortes incluidos)");
}

#[tokio::test]
async fn popups_off_answers_popup_false() {
    let hooks = tempfile::tempdir().unwrap();
    std::fs::write(hooks.path().join("cc-notify.conf"), "POPUPS=0\n").unwrap();
    let rs = rust_notifyd(hooks.path());
    let wire = rs.request("POST", "/notify", &[], r#"{"title":"x"}"#).await;
    assert_eq!(wire.body, br#"{"ok": true, "popup": false}"#);
    assert!(rs.accepted().is_empty());
}
```

`support::{python_notifyd, rust_notifyd}`: el primero arranca el `python3 -c` descrito en Confinamiento (sin `python3` o sin `bin/cc-notifyd`, `None`) y captura los argumentos de `idle_add` como líneas JSON; el segundo arranca `comandos-notifyd --headless --port 0 --hooks-dir …` (ruta del binario: `env!("CARGO_BIN_EXE_comandos-notifyd")`). `accepted()` devuelve las listas de avisos normalizadas a `[title, body, session, kind, project, options, full, pane]`.

- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-notifyd --test http` · Expected: FAIL (crate sin código).
- [ ] **Step 3: Implementar** el crate, el HTTP y `Notice`.
- [ ] **Step 4: Ver que pasa** — misma orden · Expected: PASS.
- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/comandos-notifyd/Cargo.toml crates/comandos-notifyd/src/main.rs \
  crates/comandos-notifyd/src/http.rs crates/comandos-notifyd/src/notice.rs \
  crates/comandos-notifyd/tests/http.rs crates/comandos-notifyd/tests/support/mod.rs
git commit -m "feat(notifyd): crate comandos-notifyd con POST /notify igual al de cc-notifyd

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

(Añadir `Cargo.lock` si cambió.)

---

### Task 2: Popups GTK3

Comportamiento portado (líneas de `bin/cc-notifyd`):

- **Tema** (`theme_tokens` 75, `build_css` 117, `CSS` 172, `apply_theme_css` 649): `config/themes.json` + `GET <dash>/prefs` (`theme`, 1,5 s) con caché de 30 s; tokens base literales; CSS generado byte a byte igual (prueba diferencial de `build_css` con los tokens de cada tema de `themes.json`).
- **Iconos** (`_icon_image` 104): SVG de `dash/` (`_NOTIF_ICONS`) recoloreado y cargado con `gdk_pixbuf::Pixbuf::from_stream_at_scale` (o `PixbufLoader`), mismo tamaño.
- **Markdown a Pango** (`md_to_pango` 282, `_fmt_table` 261, `build_full_widget` 318): conversión literal con prueba diferencial de las funciones de texto.
- **`make_popup`** (661–1024): pila (`STACK_MAX`, `evict_candidate`), un popup por `session|pane`, ventana `TOPLEVEL` sin decoración, `keep_above`, sin foco al mapear, `NOTIFICATION`, visual RGBA, cabecera tintada por `kind`, `PERM_RE` para los botones numerados (opciones partidas por `\x1f`, máximo 4), `[Enter][Esc]` en esperas, `[Abrir]`, entrada de texto, «Ver todo» (`expanded`), arrastre (`_press`, `_motion`, `_release`) que guarda el ancla, teclas (`_keys`), plegado (`_set_open`, `_toggle`), desvanecimiento (`_fade`) y cierre automático de los «done» (10 s; comprobar el valor en el código). `reposition` (465) con `_notif_pos` (`GET /prefs` `notif_pos`, caché 5 s; modos `tl`, `tr`, `bl`, `br`, libre con ancla), `«Cerrar todas»` (502–580).
- **Posición** (`_anchor_load`/`_anchor_save` 213–230): `notifyd-pos.json` con `{"x", "y"}` enteros, escritura por `.tmp` + `rename` fuera del hilo de GTK.

**Files:**
- Create: `crates/comandos-notifyd/src/{theme,markup,stack,popup,position}.rs`; Modify: `crates/comandos-notifyd/src/main.rs`
- Create: `crates/comandos-notifyd/tests/logic.rs`, `crates/comandos-notifyd/tests/gtk.rs`

**Interfaces:**
- Consumes: T1 (`Notice`).
- Produces: `theme::{tokens, build_css}`, `markup::{md_to_pango, fmt_table}`, `stack::{evict_candidate, PopupMeta { kind, session, pane, born }}`, `popup::{make_popup, close_popup, close_all}`, `position::{load_anchor, save_anchor}`.

**Confinamiento:** `logic.rs` sin pantalla (funciones puras contra el Python con el `gi` falso); `gtk.rs` solo con `COMANDOS_GTK_TESTS=1` y pantalla existente: crea popups en el proceso de prueba, comprueba geometría, clases CSS y botones, y los cierra; `--hooks-dir` temporal (la posición se guarda ahí); el tablero (`/prefs`) es un servidor HTTP de la prueba. Sin tmux.
**Efectos en vivo:** ventanas en el escritorio del usuario (las mismas que hoy) y `notifyd-pos.json` al arrastrar.

- [ ] **Step 1: Pruebas que fallan** — `logic.rs`: `build_css` para cada tema, `md_to_pango` con veinte textos (tablas, código, negritas, `<` y `&`), `fmt_table`, `evict_candidate` (pila llena de «waiting», mezcla), `_notif_pos`/posición calculada por `reposition` para los cinco modos con una geometría fija (la parte aritmética extraída a `position::layout(geometry, mode, anchor, heights) -> Vec<(i32, i32)>` y comparada con la misma aritmética del Python, que se ejecuta con alturas fijas); `gtk.rs`: `popup_layout_and_buttons` (aviso de permiso con tres opciones → botones `1`, `2`, `3`, `Abrir`; aviso «done» sin opciones → sin botones numerados), `stack_evicts_oldest_calm`, `clear_all_appears_from_two`.
- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-notifyd --test logic` · Expected: FAIL.
- [ ] **Step 3: Implementar**.
- [ ] **Step 4: Ver que pasa** — Run: `$C test -p comandos-notifyd --test logic` (y, con pantalla, `COMANDOS_GTK_TESTS=1 $C test -p comandos-notifyd --test gtk`) · Expected: PASS.
- [ ] **Step 5: Revisión visual (controlador)** — con `COMANDOS_GTK_TESTS=1` en la sesión gráfica del usuario, `comandos-notifyd --port 0 --hooks-dir <tmp>` y un aviso de cada tipo enviado con `curl` al puerto impreso; comparar a ojo con el mismo aviso del `cc-notifyd` vivo. Captura en `docs/verification/` si el controlador lo pide.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-notifyd/src/theme.rs crates/comandos-notifyd/src/markup.rs crates/comandos-notifyd/src/stack.rs \
  crates/comandos-notifyd/src/popup.rs crates/comandos-notifyd/src/position.rs crates/comandos-notifyd/src/main.rs \
  crates/comandos-notifyd/tests/logic.rs crates/comandos-notifyd/tests/gtk.rs
git commit -m "feat(notifyd): popups GTK3 con tema del tablero, pila, arrastre y Cerrar todas

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Acciones y barrido de «te espera»

Comportamiento portado:

- **Acciones** (`send_key` 400, `choose` 413, `send_text` 419, `open_session` 434): validación (`ALLOWED_KEYS = {Enter, Escape, 1, 2, 3}`, `SESSION_RE`, `PANE_RE`); `POST <dash>/key|/send|/focus` con `_target_payload` (5 s, en un hilo; el resultado vuelve al hilo de GTK solo si hace falta); si el POST lanza (conexión rechazada, plazo, HTTP ≥ 400 —`urlopen` lanza en ≥ 400—), caída a tmux: `_fallback_target` (con pane: solo ese pane si `display-message -p -t <pane> '#{pane_id}'` devuelve exactamente el pane; sin pane: `=<s>:` si `has-session`), `send-keys` (tecla, o `-l -- <texto>` + `Enter`), y para abrir `switch-client -t =<s>` + `wmctrl -a <s>` (2 s). `choose`: dígito y `Enter` 250 ms después (`glib::timeout_add_local_once`).
- **Barrido** (`waiting_sweep_loop` 622, `stale_waiting` 596): hilo que cada 3 s, si hay algún popup «waiting» (consultado por el canal), hace `GET <dash>/state` (2 s); si es lista, `stale_waiting(popups, state, now)` en el hilo de GTK con `WAITING_GRACE = 4.0`.
- **`native_notify`** con `NATIVE_NOTIFY=1` → popup propio (N2).

**Files:**
- Create: `crates/comandos-notifyd/src/actions.rs`, `crates/comandos-notifyd/src/sweep.rs`; Modify: `crates/comandos-notifyd/src/popup.rs`, `crates/comandos-notifyd/src/main.rs`
- Create: `crates/comandos-notifyd/tests/actions.rs`

**Interfaces:**
- Consumes: T1, T2.
- Produces: `actions::{send_key, choose, send_text, open_session, Target}`, `sweep::{stale_waiting, run}`.

**Confinamiento:** el tablero es un servidor HTTP de la prueba (responde 200, o 500, o no escucha) en `--dash-url`; la caída a tmux usa `--tmux-socket <tempdir>/tmux-<uid>/default` con un servidor privado creado por la prueba (`PrivateTmux`, `-S`) con una sesión `s` de dos panes `cat`; lo tecleado se lee con `capture-pane` por el mismo `-S`; `wmctrl` es un ejecutable de un `PATH` de prueba que anota. Ninguna orden tmux sin `-S`.
**Efectos en vivo:** POST al tablero (el mismo que hoy) y, si no responde, `send-keys`/`switch-client` sobre la sesión o el pane exacto del aviso y `wmctrl -a`, como el Python.

- [ ] **Step 1: Pruebas que fallan** — `actions.rs`: `key_goes_through_dash` (el servidor de prueba recibe `POST /key {"session":"s","key":"1","pane":"%1"}`), `key_falls_back_to_exact_pane` (tablero caído: la tecla llega solo al pane `%1`, nunca al activo), `dead_pane_types_nothing` (pane inexistente y tablero caído: ningún pane cambia), `text_fallback_is_literal` (texto con `;` y `$(…)` llega literal), `invalid_key_does_nothing`, `stale_waiting_matches_python` (diferencial de la función con veinte estados y relojes, contra el Python con el `gi` falso).
- [ ] **Step 2: Ver el fallo** — Run: `$C test -p comandos-notifyd --test actions` · Expected: FAIL.
- [ ] **Step 3: Implementar**.
- [ ] **Step 4: Ver que pasa** · Expected: PASS.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-notifyd/src/actions.rs crates/comandos-notifyd/src/sweep.rs crates/comandos-notifyd/src/popup.rs \
  crates/comandos-notifyd/src/main.rs crates/comandos-notifyd/tests/actions.rs
git commit -m "feat(notifyd): acciones de los popups vía el tablero con caída a tmux y barrido de esperas

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Cutover (lo ejecuta la sección «2f» del maestro con el usuario)

Instalar `comandos-notifyd` en `~/.local/bin` (enlace al binario de `.build/target/release`), cambiar `ExecStart` de `systemd/cc-notifyd.service` a `%h/.local/bin/comandos-notifyd`, `systemctl --user daemon-reload && systemctl --user restart cc-notifyd`, comprobar con `curl -s -XPOST 127.0.0.1:4778/notify -d '{"title":"prueba","kind":"done"}'` y un popup visible. Reversión: `ExecStart` de vuelta a `%h/.local/bin/cc-notifyd` y reinicio. Diferencias aceptadas: cabeceras `Server`/`Date` (ruling 1), línea de arranque (N1), `NATIVE_NOTIFY=1` sin libnotify (N2).

## Self-review

- **Cobertura**: `POST /notify` (T1), popups y tema (T2), acciones, caída a tmux y barrido (T3); `bin/cc-notifyd` entero salvo libnotify (N2).
- **Confinamiento**: tmux solo con `--tmux-socket` y `-S` en pruebas; pantalla solo si existe y con opt-in; puerto 4778 nunca en pruebas.
- **Independencia**: el grupo no depende de nada de la 2f; T1 → T2 → T3 en serie.
