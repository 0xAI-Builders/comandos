# Fase 2: inventario de `cc-dash`, `cc-notifyd` y la terminal web

Fecha: 2026-10-04. Entrada para el plan de implementación de la Fase 2 de la migración a Rust
(`comandos dash`, `comandos notifyd`, terminal web). Investigación de solo lectura: no se
editaron archivos versionados, no se arrancaron servidores y no se tocó `~/.local` ni systemd.

Base leída: árbol de trabajo de `main` en `af1c099`, con modificaciones sin commitear en
`bin/cc-dash` (el conjunto de rutas es idéntico al de `HEAD`: 143 ramas `if self.path…` en ambos).
Los números de línea citan el árbol de trabajo. Otra sesión edita estos archivos; antes de
implementar, volver a sacar los números de línea.

Spec de referencia: `docs/superpowers/specs/2026-10-04-comandos-rust-complete-design.md` §4.2–4.4,
§5 y §6.

## Resumen

- **Rutas reales: 161 pares método+ruta** (63 GET, 97 POST, 1 DELETE), más el servidor de
  archivos estáticos como fallback de GET (`bin/cc-dash:8744`, `super().do_GET()`) y un HEAD
  heredado. La spec dice 124 (§4.1, §4.2, §7, §8); ningún conteo del código da 124: hay 143 ramas
  `if` en `do_GET`/`do_POST` más 1 en `do_DELETE`, y 144 literales de ruta distintos dentro del
  despachador. Ver §1.13.
- `cc-dash` no son 9 897 líneas: importa 46 módulos propios (`lib/*.py` y `bin/cc_usage.py`) que
  suman **16 594 líneas** más (cálculo por cierre transitivo de imports con `ast`; lista en §3.1).
  El trabajo de la Fase 2 es ≈ 26 500 líneas de Python, no 10 000.
- **`cc-notifyd` no hace sonidos, voz ni Telegram.** Solo muestra popups GTK (o libnotify si
  `NATIVE_NOTIFY=1`). La voz y el chime ya los hace el hook Rust (`notify_http.rs`); los sonidos de
  Pomodoro y de prueba los hace `cc-dash`; Telegram está retirado (`bin/cc-dash:456-458`,
  `lib/retire-telegram.sh`). La spec §4.3 está desactualizada en esto.
- **No existe SSE hoy.** Lo único "en vivo" es el long-poll `GET /notices/watch` (hasta 25 s,
  `bin/cc-dash:8616-8638`). `/events/stream` (spec §4.2) es una ruta nueva, no una migración.
- `cc-dash` ocupa ahora **1.59 GB de RSS a las 39 h 48 min** (pico `VmHWM` 1.99 GB, 12 hilos;
  `ps`/`/proc/3529904/status`); la cgroup de la unidad marca 3.35 GB (`MemoryCurrent`, incluye
  caché de páginas). Peor que los 1.47 GB/29 h de la spec.
- `POST /remote-off` ejecuta `tailscale serve reset` (`bin/cc-dash:5115-5119`), que hoy borraría
  también los puertos 8444–8447 de otros proyectos (`tailscale serve status`). Riesgo a no portar
  tal cual.
- Es `cc-dash` quien arranca la terminal web al inicio: `restore_requested_webterm()`
  (`bin/cc-dash:5145-5149`, llamado en `main` `:9884`) lanza `cc-webterm` si existe
  `~/.claude/hooks/webterm-enabled` (existe). Retirar `cc-dash` sin sustituir esto deja la terminal
  remota apagada tras el siguiente reinicio.

## 1. Tabla de rutas de `bin/cc-dash`

### 1.1 Cómo despacha

- `Handler(http.server.SimpleHTTPRequestHandler)` en `bin/cc-dash:8173`; `directory=DASH`
  (`:8182-8183`).
- `do_GET` (`:8358-8366`): comprueba Host; si la ruta empieza por algún prefijo de `API_GET`
  (`:8338-8343`) y no es un asset público (`_public_asset`, `:8347-8356`), aplica
  `_security_gate`; después `_do_GET` (`:8368-8744`), una cadena de `if self.path.startswith(…)` /
  `==` / `urlsplit(...).path ==`. Lo que no casa va a `super().do_GET()` (estáticos, `:8744`).
- `do_POST` (`:8763-9856`): puerta de seguridad, lee el cuerpo (tope 20 000 000 bytes, `:8773`),
  exige objeto JSON (`:8778`), y compara `self.path ==` **literal, query string incluida**. A
  partir de `:9552` toda ruta exige `data["session"]` válido (`SESSION_RE`): una ruta POST
  desconocida sin `session` responde `400 {"error":"Nombre de sesion invalido"}`, no 404; con
  `session` válido llega a `404 {"error":"Ruta desconocida"}` (`:9856`). Entre `:9630` y `:9642`
  se resuelve la sesión real del proyecto (`resolve_project_session`) y el pane exacto.
- `do_DELETE` (`:8746-8761`): una ruta; tope 64 000 bytes; resto `404 {"error":"No encontrado"}`.
- `do_GET` y `do_POST` se envuelven con `_guard_request` (`:8154-8170`, aplicado en `:9861-9862`):
  `subprocess.TimeoutExpired` → 504 "Tiempo de espera agotado"; otra excepción → 500 "Error
  interno del tablero" (`_fail`, `:8192-8204`). **`do_DELETE` no está envuelto.**
- Respuesta JSON: `_json` (`:8218-8229`) usa `json.dumps(obj)` por defecto (ASCII escapado,
  separadores `", "`/`": "`), `Content-Type: application/json`, `Cache-Control: no-store`,
  `Content-Length`. Todas las rutas de esta tabla responden `application/json` salvo
  `/remote-qr.png` (`image/png`) y `/news/media/<archivo>` (`image/*`, vía `_bytes`, `:8231-8237`).

### 1.2 Convenciones de la tabla

- **Líneas**: rango de la rama en el despachador; entre paréntesis la función que hace el trabajo.
- **Estado**: `H/` = `~/.claude/hooks/`; `APP` = `~/.local/state/comandos/app-state.sqlite3`
  (`lib/app_state.py:343-345`); `USO` = `H/comandos-usage.sqlite` (`bin/cc_usage.py:20,30-33`);
  `OPS` = `H/session-operations.sqlite3` (`bin/cc-dash:2142`).
- **Llamadores**: archivos que contienen el literal de la ruta (búsqueda en `dash/*.js`,
  `dash/*.html`, `bin/`, `lib/`, `hooks/`, `adapters/`, `crates/`); los de Rust que aparecen solo en
  fixtures de tests no cuentan. "Catálogo" = solo aparece en `lib/operator_catalog.py`, que hoy solo
  usa `POST /app/command` para comandos de la app (§1.12): **sin llamador vivo**.
- **Polling**: ver §1.11; aquí solo la marca.

### 1.3 Sesiones, pestañas y paneles

| Método | Ruta | Líneas (función) | Estado / efectos | Llamadores, polling | Procesos, caché |
|---|---|---|---|---|---|
| GET | `/state` (prefijo) | 8388-8389 (`read_states_cached` 7315, `read_states` 7159) | Lee `H/state/*.json` (476 archivos hoy) y transcripts; **escribe** `H/app-tab-models.json` (`write_app_tab_models` 6838-6886, en un GET). Lista de tarjetas por pane | `index.html` tick 2 s (15 s oculto); `cc-app` cada 3 s; `cc-notifyd` cada 3 s si hay popups "waiting"; `lib/operator_*` | `tmux list-panes`, `/proc`, `ps`; caché 1.2 s con un solo cómputo en vuelo (`:7311-7351`) |
| GET | `/tabs` | 8673-8683 | Lee `H/app-tabs.json` (`tab_labels` 6428), `H/prefs.json` (favoritos); lista `[{session,label,closable?}]` | `index.html` | `tmux list-sessions` |
| GET | `/tab-history` | 8684-8693 (`read_tab_history` 5318) | Lee `H/app-tabs-history.json`; máx. 40 | `index.html` | `tmux` |
| GET | `/tab-models` | 8415-8426 | Lee `H/app-tab-models.json`; `{session,panes}` | `term.html` cada 2 s por iframe | — |
| GET | `/active-tab` | 8427-8441 | Lee `H/app-tab-active.json`; añade `pane` desde tmux | `index.html` cada 1 s dentro de la app | `tmux has-session`, `display-message` |
| GET | `/workspace` | 8614-8615 (`workspace_sync` 6533, `workspace_payload` 6566) | APP (`workspace_*`), importa `H/app-tabs.json` | `index.html`, `term.html`, `workspace-dock.js`; `cc-app` cada 2 s | conexión SQLite por hilo (`:6444-6460`) |
| GET | `/workspace/close-group` | 8657-8665 | APP; vista previa de cierre | `index.html` | — |
| GET | `/workspace/client` | 8666-8672 | APP `workspace_clients` por `deviceId` | `index.html`, `term.html`, `cc-app` | — |
| POST | `/workspace` | 8792-8810 | APP: commit con `expectedRevision`; 409 con `current` | `index.html`, `term.html`, `workspace-dock.js`, `cc-app` | — |
| POST | `/workspace/sort` | 8789-8791 (`workspace_sort` 6497) | APP | `index.html`, `cc-app` | — |
| POST | `/workspace/close-group` | 8843-8866 | APP; cierra tabs (`close_app_tab`); escribe `H/app-tab-close.json` | `index.html`, `cc-app` | `tmux` |
| POST | `/workspace/client` | 8878-8884 | APP `workspace_clients` | `index.html`, `term.html`, `cc-app` | — |
| POST | `/tab-new` | 9523-9537 | Crea `term-r<n>`; `register_app_tab` (5439) escribe `H/app-tabs.json`, `H/app-tab-open.json` | Catálogo | `tmux new-session` |
| POST | `/tab-register` | 9571-9583 | `register_app_tab` | `index.html` | `tmux has-session` |
| POST | `/tab-metadata` | 9584-9592 (`write_tab_metadata` 5392) | `H/app-tabs-meta.json` | `cc-app-mac` | — |
| POST | `/tab-metadata-remove` | 9593-9596 (5407) | `H/app-tabs-meta.json` | `cc-app-mac` | — |
| POST | `/tab-close` | 9597-9607 (`close_app_tab` 5468) | `H/app-tabs.json`, `H/app-tab-close.json`, historial | `index.html` | `tmux display-message` |
| POST | `/recover-tab` | 9608-9629 | `H/app-tabs.json`, `H/app-tabs-meta.json`, historial | `index.html` | `tmux new-session` + lanzamiento de agente (`tmux_new_session` 5559) |
| POST | `/session-new` | 9333-9484 | Perfiles (USO), `H/profile-launches/`, `register_app_tab`, `record_runtime_config` (USO) | `index.html` | `tmux new-session`, `send-keys` del comando del agente en un hilo tras 1.5 s |
| POST | `/open-with-account` | 9485-9519 | `register_app_tab` | Catálogo | `tmux new-session` + `send-keys claude` |
| POST | `/terminal/quick` | 9520-9522 (`quick_terminal_request` 5631) | APP `quick_terminal_requests` | `quick-terminal.js`, `cc-app` | `tmux` |
| POST | `/session/recover` | 8885-8888 (`session_recover` 3505) | OPS | **ninguno** | hilo de recuperación (`:3543`) |
| POST | `/terminal-history` | 8889-8897 (`lib/terminal_history.py`) | — | `term.html` | `tmux capture-pane`; 503 si falla |
| POST | `/terminal-panes` | 8898-8907 (`lib/terminal_panes.py`) | Snapshot de pane cerrado (`save_closed_pane_snapshot` 5802) | `index.html`, `term.html` **cada 2 s por iframe** (`action:list`), `cc-app` | `tmux` |
| POST | `/pane/type` | 9667-9701 | Caché de respuestas por `requestId` (256, `:1395`) | `command-sidebar.js` | `tmux send-keys -l`; lock por pane |
| POST | `/send` | 9702-9713 | — | `index.html`, `cc-notifyd` | `tmux send-keys -l` + `Enter` |
| POST | `/paste` | 9714-9726 | — | `index.html` | `tmux load-buffer/paste-buffer` (`snippet_paste_to_pane` 5842) |
| POST | `/key` | 9727-9745 | — | `index.html`, `cc-notifyd` | `tmux send-keys` (`ALLOWED_KEYS` 5706) |
| POST | `/export` | 9746-9760 (`export_response` 8028) | Escribe un .txt/.pdf | `cc-app` | `xdg-open` (`_open_url` 34) |
| POST | `/kill` | 9761-9772 | — | `index.html` | `tmux kill-session` |
| POST | `/focus` | 9773-9780 (`focus_session` 5651) | Escribe `H/app-focus.json` | `index.html`, `cc-notifyd`, `cc-next` | `tmux switch-client`, `wmctrl` |
| POST | `/ensure` | 9781-9804 | `H/app-tabs-meta.json` | `index.html`, `cc-app` | `tmux new-session` + agente |
| POST | `/new` | 9805-9825 | `H/app-tabs-meta.json` | `index.html`, `cc-app` | `systemd-run --user --scope tmux new-session` (`scope_cmd` 5538) |
| POST | `/shell` | 9826-9841 | — | `index.html` | `tmux new-window/select-window` |
| POST | `/up` | 9842-9856 | — | `index.html`, `cc-next` | `tmux new-session` + agente |
| POST | `/pause` | 9005-9018 | — | **ninguno** | `kill -STOP/-CONT` al pid del agente |
| GET | `/tmux-mouse` | 8604-8613 | — | `index.html` | `tmux show-options` |
| POST | `/tmux-mouse` | 9556-9563 | — | `index.html` | `tmux set-option mouse` |
| POST | `/tmux-scroll` | 9564-9570 (`tmux_scroll` 5968) | — | `term.html`, `cc-app` | `tmux copy-mode/send-keys` |

### 1.4 Configuración de sesión, cuentas, modelos y motor

| Método | Ruta | Líneas (función) | Estado / efectos | Llamadores, polling | Procesos, caché |
|---|---|---|---|---|---|
| GET | `/accounts?harness=` | 8390-8403 | `config/providers.json`, `lib/accounts.py`; con `usage≠0` añade límites (red) | `term.html`, `cc-app` | límites con caché 60 s (`usage_provider_limits` 1309) |
| POST | `/account/add` | 9538-9540 (`account_add_request` 2749) | Crea dir de cuenta | `term.html`, `cc-app` | `tmux new-session` + `send-keys` login |
| POST | `/account/switch` | 9643-9654 (`account_switch_configuration` 2821, `session_configure` 3454) | OPS | `index.html`, `term.html`, `cc-app` | operación en hilo (`:3501`), `tmux` |
| POST | `/session/configure` | 9643-9654 | OPS | `index.html` | igual |
| POST | `/harness/switch` | 9643-9654 | OPS | Catálogo | igual |
| POST | `/model/switch` | 9663-9666 (`model_switch_apply` 8066) | OPS | `index.html` | igual |
| POST | `/model/switch-cancel` | 9655-9662 (`model_switch_cancel` 3602) | `H/motor-queue.json` | Catálogo | — |
| GET | `/model/status?operationKey&operationId` | 8452-8459 (`session_operation_status` 6335) | OPS | `index.html` (intervalo `:2587`, solo con operaciones pendientes), `term.html`, `cc-app` | — |
| GET | `/session-config-history?session&pane` | 8374-8381 (2137) | OPS | **ninguno** | `tmux display-message` |
| GET | `/session-brain` | 8495-8501 (6370) | providers.json, identidad del pane | Catálogo | `tmux`, `/proc` |
| GET | `/providers` | 8404-8408 (`provider_public_state` 1722) | `config/providers.json` (caché por mtime, `:1412-1420`) | `index.html`, `workspace.js` | — |
| GET | `/model-tiers` | 8413-8414 (3832) | `config/model-tiers.json` (caché por mtime) | `index.html` | — |
| GET | `/opencode/models` | 8411-8412 (4829) | `~/.opencode/bin/opencode` | `index.html` | proceso `opencode` en hilo; caché (`:4798-4846`) |
| GET | `/optimization/plans` | 8409-8410 (1463) | `config/optimization-plans.json`, `H/optimization-default.json` | `index.html` | — |
| POST | `/optimization/default` | 8923-8928 (1486) | escribe `H/optimization-default.json` | Catálogo | — |
| GET | `/proxy` | 8460-8472 | `config/proxy.json`, `H/session-effort.json`, cuentas, `~/.codex/auth.json`, `~/.grok` | Catálogo | conexión TCP a 127.0.0.1:18765 (`proxy_alive` 3815-3821); `which` |
| POST | `/proxy` | 8929-8941 (`motor_set_global` 3800, `proxy_set_enabled` 3557) | `config`/`H/` del motor | Catálogo | `systemctl --user start/stop cc-proxy`; `time.sleep(1.0)` en el hilo de la petición |
| GET | `/session-profiles` | 8475-8486 | USO (`session_profiles`) | `workspace.js` | — |
| POST | `/session-profiles` | 8956-8966 | USO | `workspace.js` | — |
| POST | `/session-profile-apply` | 8967-8980 | USO, `H/profile-launches/` | `workspace.js` | — |
| GET | `/extension-usage` | 8487-8494 | USO | `workspace.js` | — |
| GET | `/project-profiles` | 8590-8592 (2642) | USO | **ninguno** | — |
| POST | `/project-profile` | 8920-8922 (2661) | USO | **ninguno** | — |

### 1.5 Extensiones y MCP

| Método | Ruta | Líneas (función) | Estado / efectos | Llamadores | Procesos |
|---|---|---|---|---|---|
| GET | `/pane-extensions` (prefijo) | 8382-8385 (`pane_extensions_state` 2202) | Transcripts, inventario de extensiones, OPS | `extensions.js` | `/proc` |
| POST | `/pane-extensions` | 8786-8788 (`pane_extensions_write` 2245) | OPS, borradores | `extensions.js` | — |
| POST | `/pane-extensions/apply` | 8786-8788 | OPS; relanza agente | `extensions.js` (sufijo concatenado) | `tmux`, agente |
| POST | `/pane-extensions/template` | 8786-8788 | plantillas | `extensions.js` | — |
| POST | `/pane-extensions/cancel` | 8786-8788 | OPS | `extensions.js` | — |
| POST | `/pane-extensions/recover` | 8786-8788 | OPS | `extensions.js` | — |
| POST | `/skill-toggle` | 8981-9004 | USO (perfil) | Catálogo | — |
| POST | `/mcp-toggle` | 8981-9004 | USO (perfil) | Catálogo | — |
| GET | `/commands/catalog` | 8731-8739 (`cli_catalog_payload` 1533) | Catálogo de CLIs | `command-sidebar.js`, `index.html` | `--help` de CLIs; caché + fallback 600 s (`:1506-1508`) |
| GET | `/chains` | 8742-8743 | Directorio de cadenas (`lib/command_chains.py`) | `chain-builder.js`, `command-sidebar.js` | — |
| POST | `/chains` | 9268-9277 | igual | `chain-builder.js` | — |
| POST | `/chains/delete` | 9278-9286 | igual | **ninguno** | — |
| GET | `/snippets` | 8740-8741 (`read_snippets` 5264) | `H/snippets.json` | `index.html` | — |
| POST | `/snippets` | 9287-9300 | `H/snippets.json` con `file_lock` | `index.html` | — |
| POST | `/snippets/update` | 9301-9319 | igual | `index.html` | — |
| POST | `/snippets/delete` | 9320-9332 | igual | `index.html` | — |
| POST | `/app/command` | 9541-9551 | Escribe `H/app-command.json` y **bloquea el hilo** esperando que `cc-app` lo borre (`lib/operator_dispatch.py:238-260`) | ninguno vivo (comentario en `bin/cc-app:8980`) | — |

### 1.6 Uso, cuotas y analítica

| Método | Ruta | Líneas (función) | Estado / efectos | Llamadores, polling | Procesos, caché |
|---|---|---|---|---|---|
| GET | `/usage/state` | 8568-8589 | USO (166 MB hoy); importa transcripts en hilo cada ≥60 s (`refresh_local_usage` 235); **escribe** bordes de pane (`write_pane_models` 4759, `H/pane-models.txt`) | `index.html` como máximo cada 10 s (`tickUsage` `:2973-2978`) | `build_usage_state` memoizado (`_usage_state_cache` 264-285); límites caché 60 s con red (`api.anthropic.com/api/oauth/usage`, `bin/cc_usage.py:1034`) |
| POST | `/usage/refresh` | 9155-9177 | USO | Catálogo | red síncrona OpenAI/Anthropic admin (`bin/cc_usage.py:3075-3116`) |
| POST | `/usage/settings` | 9236-9247 | USO | Catálogo | recalcula estado completo |
| POST | `/usage/quota` | 9214-9224 (590) | `H/provider-quotas.json` | Catálogo | — |
| POST | `/usage/subscription` | 9225-9235 (564) | `H/provider-subs.json` | Catálogo | — |
| GET | `/usage/guard` | 8502-8503 (1429) | USO | Catálogo | — |
| GET | `/usage/changes` | 8504-8505 | USO | Catálogo | — |
| GET | `/usage/provider-compare` | 8540-8550 | USO, `config/api-prices.json` | Catálogo | — |
| GET | `/usage/experiments` | 8551-8552 | USO | Catálogo | — |
| GET | `/usage/analytics` | 8553-8560 | USO | Catálogo | — |
| GET | `/usage/interactions` | 8561-8567 | USO | Catálogo | — |
| POST | `/usage/experiment` | 9109-9133 | USO | Catálogo | — |
| POST | `/usage/rating` | 9134-9147 | USO | Catálogo | — |
| POST | `/usage/capture` | 9148-9154 | USO (`record_turn`) | **ninguno** (los hooks escriben directo) | — |
| GET | `/analytics/week?offset&sidebar` | 8444-8445 (`analytics_week_query` 6734) | USO, límites | `analytics.js` cada 60 s con el panel abierto (`index.html:5840`); barra lateral cada 60 s (`:4259-4268`) | — |
| GET | `/dedication` | 8442-8443 (4484) | USO | Catálogo | caché 60 s |
| GET | `/sovereignty` | 8450-8451 (4393) | `H/cc-notify.conf`, USO, `H/webterm-enabled`, `dash-token` (solo metadatos) | `index.html` | — |
| GET | `/ui-log/summary` | 8473-8474 (98) | `H/ui-events.jsonl` (437 KB) | Catálogo | lee el archivo entero |
| POST | `/ui-log` | 8942-8943 (`ui_log_append` 64) | `H/ui-events.jsonl` con rotación 2 MB | `index.html` cada 5 s si hay eventos (`:4448`) | — |

### 1.7 Notificaciones, eventos, marcas y Pomodoro

| Método | Ruta | Líneas (función) | Estado / efectos | Llamadores, polling | Procesos |
|---|---|---|---|---|---|
| GET | `/notices?after&limit&deviceId` | 8639-8656 | APP (`events`, `notice_reads`, `client_presence`, `notice_prefs`) | `notifications.js` cada 3 s (15 s oculto, `:796-798`); `pomodoro.js` | `tmux list-panes` (`notices_is_live` 980) |
| GET | `/notices/prefs` | 8639-8656 | APP | `notifications.js` | — |
| GET | `/notices/watch?rev&wait` | 8616-8638 | APP; **long-poll** hasta 25 s, sondeo cada 0.2 s (`:8628-8630`) | `notifications.js` bucle continuo (`:810-815`); `cc-app` (`bin/cc-app:3231-3245`, timeout 35 s) | `tmux list-panes` |
| GET | `/notifs/count` | 8506-8515 | APP | `notifications.js` tras marcar | — |
| POST | `/presence` | 8818-8842 | APP `client_presence` | `notifications.js` cada 30 s + gestos (`:575`); `cc-app` cada 30 s (`bin/cc-app:9001`) | — |
| POST | `/notices/read` | 8818-8842 | APP | `notifications.js` | — |
| POST | `/notices/sound` | 8818-8842 | APP (`claim_sound`) | `notifications.js`, `pomodoro.js` | — |
| POST | `/notices/prefs` | 8818-8842 | APP | `notifications.js` | — |
| POST | `/notify-popup` | 8811-8817 (`desktop_popup` 475) | Lee `H/cc-notify.conf` | `index.html` | POST a `127.0.0.1:4778/notify` |
| POST | `/event` | 9019-9041 | — | ninguno vivo (el adaptador de opencode ya no lo usa, `crates/comandos-runtime/src/hooks/opencode.rs:156-173`) | `Popen H/cc-notify.sh --agent …` (hoy symlink al binario Rust) |
| POST | `/test` | 9042-9050 (`play_test` 7411) | `H/cc-notify.conf` | `index.html` | `piper` por `/bin/sh -c`, `spd-say`, `pw-play`/`paplay` |
| GET | `/events` (prefijo) | 8719-8730 | `H/events.jsonl` (350 KB): `readlines()` completo, últimos 80 | ninguno vivo en `dash/` (solo `sw.js` y catálogo) | — |
| GET | `/events/v2?after&limit&turns` | 8714-8718 (8081) | APP; importa `events.jsonl` una vez | ninguno vivo | — |
| POST | `/events/v2` | 8870-8877 (8142) | APP; **solo productor interno** (`_internal_producer` 8322-8333) | ninguno vivo (los hooks escriben SQLite directo) | — |
| GET | `/work-marks` | 8712-8713 (8117) | APP; recalcula turnos de los últimos 2000 eventos | `work-marks.js` cada 5 s (`:338`); `cc-app` cada 5 s (`bin/cc-app:9011`) | — |
| POST | `/work-marks` | 8867-8869 (8130) | APP (`work_marks`) | `work-marks.js`, `cc-app` | — |
| GET | `/pomodoro` (exacto) | 8448-8449 (6646) | APP `pomodoro_*` | `pomodoro.js` 15 s / 1 s si vence (`:610-619`); `cc-app` (`_hourglass_poll` 6522) | — |
| GET | `/pomodoro/report` | 8446-8447 (6756) | APP | **ninguno** | — |
| POST | `/pomodoro` | 8944-8945 (6798) | APP; despierta el scheduler | `pomodoro.js`, `index.html` | sonido de fin: `pw-play` (`_play_local_sound` 1007) |
| GET | `/push/key` | 8518-8519 (1073) | VAPID en disco (`lib/web_push.py:83-90`) | `push-settings.js` | `pywebpush` **no está instalado** en el Python del sistema: responde error |
| POST | `/push/subscription` | 9178-9179 (1081) | APP `push_subscriptions` | `push-settings.js` | — |
| DELETE | `/push/subscription` | 8759-8760 | APP | `push-settings.js` | — |
| POST | `/push/test` | 9180-9182 (1098) | APP; antirrebote `_PUSH_TEST_LAST` | `push-settings.js` | — |

### 1.8 Noticias y modelos

| Método | Ruta | Líneas (función) | Estado / efectos | Llamadores | Procesos |
|---|---|---|---|---|---|
| GET | `/news/latest` | 8516-8517 | `H/news-watch.json` | `index.html` | — |
| GET | `/news/editions` | 8520-8521 (794) | APP `news_*` | `news-reader.js` | — |
| GET | `/news/edition?id` | 8528-8533 (812) | APP | `news-reader.js` | — |
| GET | `/news/media/<32hex>.<ext>` | 8522-8524 (894) | `~/.local/state/comandos/news-media/` | `news-reader.js` | — |
| GET | `/news/source`, `/news/chat`, `/news/notes`, `/news/saved` | 8525-8527 (`news_get` 870) | APP | `news-reader.js` | — |
| POST | `/news/saved`, `/news/notes`, `/news/chat`, `/news/chat/note`, `/news/translate` | 9183-9185 (`news_post` 906) | APP; chat/traducción en hilo con agente | `news-reader.js` | agente ACP o API HTTP (`lib/news_editions.py:993-1014`), semáforo 2 (`:837`) |
| POST | `/news/refresh` | 9186-9193 | `H/news-watch.json` | Catálogo | red |
| GET | `/models/latest` | 8534-8536 | `H/model-watch.json` | `index.html` | — |
| POST | `/models/refresh` | 9202-9206 (`_model_watch_cycle` 696) | `H/model-watch.json`; escaneo de binarios de CLIs | Catálogo | lectura de binarios (cientos de MB, `:710`) |
| POST | `/open-url` | 9194-9201 | — | `index.html` | `xdg-open` |

### 1.9 Acceso remoto, SSH, preferencias y archivos

| Método | Ruta | Líneas (función) | Estado / efectos | Llamadores | Procesos, caché |
|---|---|---|---|---|---|
| GET | `/webterm-token` | 8386-8387 | `H/dash-token` | `index.html`, `cc-app`, `cc-webterm` | — |
| GET | `/remote-state` | 8694-8695 (`remote_state_cached` 5056) | token | `index.html` | `tailscale status --json`, `tailscale serve status` (hasta 24 s); caché 15 s con refresco en segundo plano |
| GET | `/remote-qr.png` | 8696-8711 | — | `index.html` | `qrencode`; `image/png` |
| POST | `/remote-on` | 9051-9060 | — | `index.html` | `tailscale serve` ×3 (`serve_path` 5093), sondeo 4779/4780 |
| POST | `/remote-off` | 9061-9066 (5114) | — | `index.html` | `tailscale serve --https=443 off` **y `tailscale serve reset`** + `cc-webterm off` |
| POST | `/remote-webterm-on` | 9067-9075 (`webterm_on` 5122) | `H/webterm-enabled` (lo crea `cc-webterm`) | `index.html` | `cc-webterm` (timeout 20 s) + `tailscale serve` |
| POST | `/remote-webterm-off` | 9076-9081 | borra `H/webterm-enabled` | `index.html` | `cc-webterm off` / `pkill -f ttyd.*cc-webterm-attach` |
| GET | `/ssh` | 8593-8594 (`parse_ssh_config` 7468) | `~/.ssh/config` | `index.html` | — |
| POST | `/ssh-add`, `/ssh-del`, `/ssh-update` | 9082-9099 | `~/.ssh/config` | `index.html` | — |
| POST | `/ssh-connect` | 9248-9254 (7868) | — | `index.html`, `cc-app` | `tmux`, `ssh` |
| POST | `/ssh-new-tab` | 9255-9261 (7919) | `H/app-tabs.json` | `index.html`, `cc-app` | `tmux`, `ssh` |
| POST | `/ssh-key-setup` | 9262-9267 (7634) | `~/.ssh` | `index.html` | `systemd-run`, `ssh-copy-id`, `tmux` |
| GET | `/conf` | 8595-8599 (`read_conf` 4937) | `H/cc-notify.conf` | `index.html`, `cc-app` | — |
| POST | `/conf-set` | 8946-8955 (`write_conf_key` 7446) | `H/cc-notify.conf` | `index.html` | — |
| GET | `/prefs` | 8600-8603 (`read_prefs` 7754) | `H/prefs.json` + fuentes instaladas | `index.html` cada ≥5 s (`:3032-3041`), `extensions.js`, `cc-app` cada 3 s, `cc-notifyd` cada ≥5 s | `fc-list` (`installed_terminal_fonts` 7745) |
| POST | `/prefs-set` | 9100-9108 (`update_prefs` 7768) | `H/prefs.json` | `index.html`, `cc-app` | — |
| GET | `/fs/dirs?path` | 8537-8539 (620) | sistema de archivos | `index.html` | — |
| POST | `/fs/mkdir` | 9207-9213 (658) | crea directorio | `index.html` | — |
| POST | `/open-path` | 8908-8919 | — | `index.html` | `xdg-open` |

### 1.10 Retiradas y estáticos

| Método | Ruta | Líneas | Comportamiento |
|---|---|---|---|
| GET | `/operator*` | 8370-8373 | `410 {"error":"El chat de CommandOS se retiró…","code":"retired"}` (`:5751`). **Sin puerta de seguridad**: `/operator` no está en `API_GET` |
| POST | `/operator*` | 8781-8785 | 410, tras la puerta |
| GET | resto | 8744 | `SimpleHTTPRequestHandler` sobre `DASH` (§3.4) |
| HEAD | cualquiera | heredado | `do_HEAD` de la librería: estáticos **sin comprobar Host ni token** (no hay `do_HEAD` propio) |
| PUT/PATCH/… | cualquiera | — | 501 HTML de `BaseHTTPRequestHandler` |

### 1.11 Polling: carga en reposo

Con el tablero abierto en la app de escritorio y una terminal remota abierta, las peticiones
periódicas son:

| Origen | Ruta | Periodo | Cita |
|---|---|---|---|
| `index.html` | `GET /state` | 2 s (`S.cfg.poll`), 15 s oculto | `dash/index.html:4513-4527` |
| `index.html` | `GET /usage/state` | ≥10 s | `dash/index.html:2973-2978` |
| `index.html` | `GET /prefs` | ≥5 s | `dash/index.html:3032-3041` |
| `index.html` (en la app) | `GET /active-tab` | 1 s | `dash/index.html:2434-2449` |
| `index.html` | `GET /model/status` | mientras haya cambios pendientes | `dash/index.html:2587` |
| `index.html` | `GET /analytics/week` | 60 s | `dash/index.html:4265-4268`, `:5840` |
| `index.html` | `POST /ui-log` | 5 s si hay eventos | `dash/index.html:4448` |
| `notifications.js` | `GET /notices` | 3 s visible, 15 s oculto | `dash/notifications.js:796-798` |
| `notifications.js` | `GET /notices/watch` | long-poll continuo de 25 s | `dash/notifications.js:810-815` |
| `notifications.js` | `POST /presence` | 30 s | `dash/notifications.js:575` |
| `work-marks.js` | `GET /work-marks` | 5 s | `dash/work-marks.js:338` |
| `pomodoro.js` | `GET /pomodoro` | 15 s (1 s al vencer, 30 s oculto) | `dash/pomodoro.js:610-619` |
| `term.html` (por iframe) | `POST /terminal-panes` + `GET /tab-models` | 2 s | `dash/term.html:2306-2334` |
| `cc-app` | `GET /state` + `GET /prefs` | 3 s | `bin/cc-app:3246-3281` |
| `cc-app` | `GET /workspace` | 2 s | `bin/cc-app:4228-4238` |
| `cc-app` | `GET /work-marks` | 5 s | `bin/cc-app:9011` |
| `cc-app` | `POST /presence` | 30 s | `bin/cc-app:9001` |
| `cc-app` | `GET /notices/watch` | long-poll | `bin/cc-app:3231-3245` |
| `cc-notifyd` | `GET /state` | 3 s si hay popups "waiting" | `bin/cc-notifyd:622-634` |

Cada `/state` recorre los 476 `H/state/*.json`, `tmux list-panes` y `/proc`, con caché de 1.2 s.
Con dos tableros (app + móvil) y varias terminales remotas, el servidor atiende varias peticiones
por segundo de forma continua.

### 1.12 Rutas sin llamador vivo

Sin ningún llamador: `GET /pomodoro/report`, `GET /session-config-history`,
`GET /project-profiles`, `POST /project-profile`, `POST /session/recover`, `POST /pause`,
`POST /usage/capture`, `POST /chains/delete`, `POST /app/command` (solo un comentario en
`bin/cc-app:8980`), `GET /events/v2`, `POST /events/v2`, `POST /event`, `GET /events` (solo en
listas de `sw.js` y del catálogo).

Solo en `lib/operator_catalog.py` (el chat que las llamaba por HTTP está retirado, `:5751`):
`GET /dedication`, `/proxy`, `/ui-log/summary`, `/session-brain`, `/usage/guard`,
`/usage/changes`, `/usage/provider-compare`, `/usage/experiments`, `/usage/analytics`,
`/usage/interactions`; `POST /proxy`, `/optimization/default`, `/skill-toggle`, `/mcp-toggle`,
`/usage/experiment`, `/usage/rating`, `/usage/refresh`, `/usage/quota`, `/usage/subscription`,
`/usage/settings`, `/news/refresh`, `/models/refresh`, `/open-with-account`, `/tab-new`,
`/harness/switch`, `/model/switch-cancel`.

En total, 39 de las 161 rutas no tienen un llamador vivo. Es una decisión del plan (§9) si se
portan, se portan como 410 o se retiran.

### 1.13 Conteo y discrepancia con la spec

| Conteo | Número | Cómo |
|---|---|---|
| Pares método+ruta (esta tabla) | **161** | 63 GET + 97 POST + 1 DELETE |
| Ramas `if` del despachador | 143 + 1 | `grep '^        if self.path\|…urlsplit…'` entre `:8370` y `:9842`; más `:8759` |
| Literales de ruta distintos en el despachador | 144 | `sed -n 8358,9862p \| grep -oE '"/[a-z]…' \| sort -u` |
| Spec | 124 | §4.1, §4.2, §7, §8 |

La diferencia no se explica con ningún criterio razonable; probablemente es un conteo de una
versión anterior. El plan debe trabajar con 161 (o con 122 si se retiran las 39 sin llamador).

## 2. Puerta de seguridad

### 2.1 Reglas (`bin/cc-dash`)

- **Host allowlist** (`HOST_OK_RE`, `:4876-4879`): `*.localhost`, `127.0.0.1`, `[::1]`,
  `[::ffff:127.0.0.1]` o `*.ts.net`, con puerto opcional y etiquetas RFC (`:4865-4870`), hostname
  ≤ 253 (`_authority_matches`, `:4882-4889`). Exactamente una cabecera Host. Fallo:
  `403 {"error":"Host no permitido"}` (`:8289-8294`; en `do_GET` también `:8359-8361`).
- **Origin** (`_origin_matches_host`, `:4892-4908`): si hay Origin, debe ser exactamente el mismo
  origen que Host (`http`/`https`, sin ruta/query/userinfo, mismo hostname, mismo puerto con el
  puerto por defecto normalizado). Más de un Origin o Origin distinto:
  `403 {"error":"Origen no permitido"}` (`:8295-8302`). Sin Origin (curl, `cc-app`, `cc-notifyd`,
  hooks) pasa.
- **Local frente a remoto** (`:8303-8312`):
  - *Local directo*: peer loopback (`_ip_is_loopback`, `:4911-4919`, acepta IPv4 mapeado), sin
    ninguna `X-Forwarded-For` y Host que casa `DIRECT_LOCAL_OK_RE` (`:4873-4875`). No necesita
    token.
  - *Proxy de desarrollo*: peer loopback, `X-Forwarded-For` presente, Host `*.localhost`
    (`LOCALHOST_OK_RE`, `:4871-4872`) y **toda** la cadena XFF loopback (`:4922-4925`). No necesita
    token (devhost/nginx).
  - *Remoto*: cualquier otra cosa. `tailscale serve` llega desde loopback pero con
    `X-Forwarded-For` de la IP del tailnet y Host `*.ts.net`, así que cae aquí. Exige token.
- **Token** (`access_token`, `:4848-4862`): `H/dash-token`, 0600, `secrets.token_urlsafe(32)` si
  no existe. Se presenta por `Authorization: Bearer …`, `X-Comandos-Token`, cookie `cc_token` o
  `?token=` (`_presented_token`, `:8254-8263`), en ese orden; comparación con
  `hmac.compare_digest` sobre bytes (`:8313-8319`). Fallo:
  `401 {"error":"No autorizado (token requerido para acceso remoto)"}` (`:8320`). Nadie emite la
  cookie (`Set-Cookie` no aparece en el código); el cliente guarda el token de `?token=` en
  `localStorage["cc_token"]` y lo manda como `X-Comandos-Token` (`dash/index.html:1944-1968`).
- **Productor interno** (`_internal_producer`, `:8322-8333`): loopback, sin XFF, sin Origin y
  `X-Comandos-Token` correcto. Solo lo exige `POST /events/v2` (`:8870-8872`, 403 "Solo
  productores internos de este equipo").
- **Qué rutas pasan por la puerta**: GET solo si empieza por un prefijo de `API_GET`
  (`:8338-8343`, 55 prefijos) y no es asset público (`PUBLIC_ASSET_RE` `^/[A-Za-z0-9_-]+(/…)*\.(css|js)$`
  y el archivo existe en `DASH`, `:8345-8356`). Todo POST y DELETE pasa por la puerta. GET de
  `/operator`, de estáticos (`/`, `/index.html`, `/term.html`, `/assets/…`, `/vendor/…`) y HEAD no.
- **Cierre de conexión**: los rechazos de POST/DELETE salen con `Connection: close`
  (`do_POST` `:8763-8766`, `_json(close=True)` `:8221-8224`); los de GET no.
- **CORS**: no hay. Ninguna respuesta lleva `Access-Control-Allow-*`. La web no puede hablar con
  `:4778`; por eso existe `POST /notify-popup` (`:475-477`).
- **Cuerpos**: `Content-Length` negativo → 400 "JSON invalido"; > 20 MB (POST) o > 64 000 (DELETE)
  → 413 "Payload demasiado grande", ambos con cierre; JSON no objeto → 400 "El cuerpo debe ser un
  objeto JSON" sin cierre (`:8768-8780`, `:8750-8758`).

### 2.2 Equivalente Rust ya hecho

`crates/comandos-core/src/dashboard_access.rs` (664 líneas) porta todo lo anterior: `API_GET`
(`:78-134`, los mismos 55 prefijos), `authority_matches` (`:190`), `origin_matches_host`
(`:401`), `presented_token` (`:499`), `security_gate` (`:528`), `internal_producer` (`:563`),
`public_asset` (`:573`), límites de `Content-Length` (`:597-622`) y `request_admission`
(`:624-652`). Tiene fixture de paridad (`crates/comandos-core/tests/dashboard_access.rs`,
`dashboard_access_fixture.json`). Diferencias de transporte en §7.

### 2.3 Terminal web y `cc-notifyd`

- ttyd (4779 y 4780) **no tiene autenticación propia** (sin `-c`, `bin/cc-webterm:128-131`). La
  única barrera es `cc-webterm-attach`, que compara su primer argumento con `H/dash-token`
  recortado y sale con "Acceso denegado" si no coincide (`bin/cc-webterm-attach:19-25`). Ambos
  escuchan solo en 127.0.0.1 (`-i 127.0.0.1`, `bin/cc-webterm:142`).
- `cc-notifyd` acepta solo peers `127.0.0.1`/`::1`/`::ffff:127.0.0.1` y Origin
  `http(s)://localhost|127.0.0.1[:puerto]`; rechaza con `403` sin cuerpo
  (`bin/cc-notifyd:1078-1083`). Sin token.

## 3. Servidor

### 3.1 Tamaño real

`bin/cc-dash` (9 897 líneas) importa, directa o transitivamente, 46 módulos propios:
`accounts, acp, allocation, analytics_week, app_state, capabilities, cc_usage, claude_trust,
cli_catalog, cli_help, command_chains, event_intake, event_store, extension_launch,
extension_metadata, extension_observations, focus_progress, grok_state, mcp_descriptions,
model_catalog, model_watch, news_editions, news_radar, news_reading, news_watch,
notification_delivery, operator_catalog, operator_dispatch, operator_receipts, pane_extensions,
pane_snapshot, pane_typing, pomodoro, providers, quick_terminal, session_operations,
session_profiles, terminal_history, terminal_panes, tmux_snapshot, tui_state, turn_state,
web_push, work_marks, workspace_layout, workspace_state` = **16 594 líneas** (los más grandes:
`cc_usage` 3 361, `news_editions` 1 208, `news_radar` 937, `extension_launch` 607). Importa `lib/`
por `sys.path.insert` a partir de `realpath(__file__)` (`:1370-1373`), por lo que el symlink de
`~/.local/bin/cc-dash` resuelve al repo; `cc_usage` se importa desde `bin/` (`:31`).

### 3.2 Hilos

- `socketserver.ThreadingTCPServer(("127.0.0.1", port), Handler)` (`:9880`): **un hilo por
  conexión**, no daemon; Python 3.10.12 reaprovecha la lista de hilos (`_Threads`), así que la
  lista no crece. `allow_reuse_address = True` (`:9874`). Solo IPv4 aunque el allowlist acepte
  `[::1]`.
- Keep-alive HTTP/1.1 (`protocol_version`, `:8177`) con `timeout = 30` s por socket (`:8180`).
- Hilos de fondo arrancados en `main` (`:9876-9879`, `:9881`):
  `_model_watch_loop` (espera 90 s, luego cada 600 s, `:766-770`), `_news_editions_loop` (120 s,
  luego cada 60 s, `:1127-1159`), `_notices_push_loop` (30 s, luego cada 15 s, `:1038-1058`),
  `_limits_snapshot_loop` (cada 300 s, `:1330-1338`), `pomodoro_scheduler_loop` (≤ 30 s o el
  próximo vencimiento, `:6668-6690`).
- Hilos efímeros por petición: importación de uso (`:249-253`), límites (`:1324`), recuperación
  de sesiones (`:3543`), operaciones de sesión (`:3501`), `send-keys` diferido (`:2817`, `:9515`),
  refresco de remote-state (`:5066`), modelos de opencode (`:4840`), agentes de noticias (`:867`),
  costumes (`:4474`), reconciliación de bordes (`:4783`), alertas de tier (`:4582`).
- SQLite: una conexión por hilo con `threading.local` y `app_state.migrate()` en el primer uso de
  cada hilo (`notices_conn` `:965-971`, `workspace_store` `:6447-6460`, `push_conn` `:1059-1066`,
  noticias `:785-791`, quick terminal `:5598-5604`, Pomodoro `:6620-6624`). Como cada conexión TCP
  es un hilo nuevo, cada conexión abre y migra su propia conexión SQLite.

### 3.3 Lectura de estado y cachés

| Caché | Cita | Política |
|---|---|---|
| `/state` | `:7308-7351` | TTL 1.2 s, una sola ejecución en vuelo |
| uso (memo de `build_usage_state`) | `:264-285` | por generación de importación + firma de panes vivos; guarda el estado completo |
| importación de transcripts | `:232-262`, `_IMPORT_SEEN` `:176` | cada ≥ 60 s; dict `ruta → mtime` **sin límite** |
| límites de proveedor | `:417-419`, `:1309-1329` | TTL 60 s (180 s tras error) |
| remote-state | `:5040-5072` | 15 s, refresco en segundo plano |
| registro de proveedores, tiers, costos, detectores | `:1412`, `:3822`, `:3948`, `:4179` | por mtime |
| catálogo de CLIs | `:1506-1508` | fallback 600 s |
| `dedication` | `:4481-4487` | 60 s |
| cuentas por pid / email | `:3875-3942` | se vacían enteras al pasar de 4096 |
| respuestas de `/pane/type` | `:1395` | 256 entradas |
| transcripts y metadatos de agentes | `:1397-1400` (`lib/tui_state.py:77-148`, `lib/pane_snapshot.py:205-262`) | 128 entradas; guardan resultados pequeños, pero cada fallo lee hasta 2 MiB de cola del transcript |

Ningún JSON de `H/` se cachea por mtime salvo los de `config/`: `H/app-tabs.json`, `prefs.json`,
`app-tab-models.json`, `cc-notify.conf`, `snippets.json` y los 476 de `H/state/` se leen en cada
petición que los usa.

### 3.4 Estáticos

- `SimpleHTTPRequestHandler(directory=DASH)` (`:8182-8183`). `DASH` = `H/dash`, o
  `COMANDOS_DASH_DIR` si existe y es legible (`:150-160`). `H/dash` es un directorio de symlinks
  uno a uno hacia `dash/` del repo, más `assets → <repo>/assets` y `vendor`, `icons`; hoy
  contiene un symlink colgante (`session-controls.js`, borrado en el árbol de trabajo) y un
  prototipo (`prototype-usage-cards.html`).
- Tipos MIME: `mimetypes`/`extensions_map` de Python (dependen de `/etc/mime.types`); hoy
  `.webmanifest → application/manifest+json`, `.js → text/javascript`, `.wasm → application/wasm`.
- Cabeceras: `Cache-Control: no-store` en **toda** respuesta, estáticos incluidos (`end_headers`,
  `:8210-8216`); además `Last-Modified` y 304 por `If-Modified-Since` de la librería.
- Directorios sin `index.html` (`/assets/`, `/vendor/`, `/icons/`) devuelven un listado HTML sin
  autenticación; un directorio sin barra final da 301. Una ruta inexistente da 404 HTML de la
  librería, no JSON.
- `term.html` se sirve aquí como `/term.html` y además lo usa ttyd como índice de `/term/` (§5).
  `term.html` carga `../assets/xterm/*.js`; en remoto eso resuelve contra el origen del tablero, es
  decir, lo sirve `cc-dash` por el symlink `assets`.

### 3.5 Arranque, parada y registros

- Argumentos: `cc-dash [puerto] [--no-open]` (`:9866-9872`); por defecto 4777 y abre el navegador
  (`webbrowser.open`, `:9885-9889`). La unidad usa `cc-dash 4777 --no-open`
  (`systemd/cc-dash.service:6`).
- Entorno: `HOME` (todo `~`), `COMANDOS_DASH_DIR`, `COMANDOS_STATE_DB`/`XDG_STATE_HOME`
  (`lib/app_state.py:343-349`), `COMANDOS_USAGE_DB` (`bin/cc_usage.py:31-32`), variables
  `COMANDOS_USAGE_*` y `COMANDOS_CLAUDE_PROJECTS_DIR` (`:359-385`).
- En el arranque: `motor_queue_resume()` (`:9868`, reanuda `H/motor-queue.json`),
  `start_pomodoro_scheduler()` (`:9881`), `restore_requested_webterm()` (`:9884`).
- Sin archivo PID. Parada: solo `KeyboardInterrupt` (`:9890-9893`); SIGTERM de systemd usa la
  acción por defecto de Python (termina sin limpieza).
- Registros: `log_message` silenciado (`:8185-8186`); errores con traza a stderr sin query string
  (`_fail`, `:8192-8199`); una línea en stdout al arrancar (`:9883`). Todo va al journal.

### 3.6 Memoria

Medido hoy: RSS 1 589 408 kB, `VmHWM` 1 991 600 kB, 12 hilos, 56 descriptores, tras 1 d 15 h 48 m
(pid 3529904). Sospechosos, de más a menos probable (los tres primeros son inferencias, no
mediciones):

1. **Arenas de malloc por hilo.** Un hilo por conexión (`:9880`) con glibc crea arenas por hilo
   (por defecto hasta 8 × núcleos); la memoria liberada por objetos Python grandes queda
   fragmentada en esas arenas y no vuelve al sistema.
2. **Churn de asignaciones en los caminos calientes.** `/state` (cada ~1 s entre todos los
   clientes) parsea 476 JSON y, en cada fallo de caché, lee hasta 2 MiB de cola de transcript y
   hace `json.loads` línea a línea (`lib/tui_state.py:94-130`); `/work-marks` (dos clientes cada
   5 s) relee 2000 eventos (`events_v2_turns`, `:8121`); `/events` hace `readlines()` del archivo
   entero (`:8723`); `/ui-log/summary` carga el log entero (`:98-134`).
3. **Memo de uso.** `_usage_state_cache["state"]` guarda la salida de `build_usage_state` sobre una
   base de 166 MB y se recalcula cada importación (≥ 60 s); mientras se construye el nuevo conviven
   dos copias.
4. **Diccionarios sin tope**: `_IMPORT_SEEN` (`:176`, una entrada por transcript visto, crece con
   el tiempo), `MOTOR_GEN` (`:3592`), `_TIER_LAST`/`_TIER_ALERTED` (`:4553-4554`),
   `_PUSH_TEST_LAST` (`:957`). Pequeños por entrada.
5. **Conexiones SQLite por hilo** con su caché de páginas, abiertas y migradas en cada conexión
   TCP (§3.2).

Para el plan: medir en Rust con `xtask` el RSS tras un arnés de polling realista (§1.11), no en
reposo.

## 4. `cc-notifyd`

### 4.1 API HTTP

- `ThreadingTCPServer(("127.0.0.1", 4778))` en un hilo; GTK en el principal
  (`bin/cc-notifyd:1123-1135`).
- Una ruta: `POST /notify` (`:1071-1120`). Puerta: §2.3. Otra ruta → `404` sin cuerpo; cuerpo
  > 200 000 bytes → `413`; JSON inválido → `400` (`:1084-1092`).
- Si `POPUPS≠1` en `H/cc-notify.conf` responde `{"ok": true, "popup": false}` sin mostrar nada
  (`:1096-1103`). Hoy `POPUPS=1` y `DESKTOP_NOTIFY=1`.
- Si muestra: responde `{"ok": true}` y programa en GTK `native_notify` con
  `title[:200]`, `body[:400]`, `session`, `kind` (por defecto `"done"`), `project[:80]`,
  `options[:600]` (separador `\x1f`), `full[:60000]`, `pane[:10]` (`:1104-1120`).

### 4.2 Popups

- Ventanas `Gtk.Window` propias, tema desde `config/themes.json` (`:71-149`), pila de máx. 8
  (`STACK_MAX`, `:584`) con desalojo del más viejo que no espera respuesta (`:587-593`); un popup
  por `sesión|pane` (`:670-675`); posición desde `GET /prefs` `notif_pos` (caché 5 s, `:451-462`) o
  ancla arrastrada en `H/notifyd-pos.json` (`:210-232`); botón "Cerrar todas" con ≥ 2 (`:498-580`);
  cierre automático a los 10 s de los que no son "waiting" salvo que estén expandidos (`:1015-1022`).
- Barrido cada 3 s: con popups "waiting", lee `GET /state` y cierra los ya atendidos tras 4 s de
  gracia (`:583-634`).
- Acciones del popup: teclas, opción numerada, texto y abrir → `POST /key`, `/send`, `/focus` de
  `cc-dash`, con caída a `tmux send-keys`/`switch-client`/`wmctrl` si el tablero no responde
  (`:400-448`).
- Con `NATIVE_NOTIFY=1` usa libnotify (`gi Notify 0.7`, `:23-28`, `:1029-1068`) con
  `suppress-sound` y acciones "abrir"/"ver todo"; por defecto no.

### 4.3 Sonidos, voz y Telegram

- `cc-notifyd` **no reproduce nada**. Los sonidos y la voz los hace el proceso de entrega del hook
  Rust: `piper` → `say` → `spd-say` para la frase y `pw-play --volume` → `paplay` → `afplay` para el
  chime (`crates/comandos-runtime/src/hooks/notify_http.rs:157-221`).
- `cc-dash` reproduce: fin de Pomodoro (`assets/sounds/pomodoro-complete.wav`) y sonido de aviso
  (`/usr/share/sounds/freedesktop/stereo/complete.oga`) con reclamo por dispositivo en APP
  (`bin/cc-dash:997-1035`); la prueba `POST /test` (`:7411-7445`).
- Telegram está retirado: comentario en `bin/cc-dash:456-458`, script `lib/retire-telegram.sh`;
  `cc-telegram.service` aparece como `not-found` en `systemctl --user list-units`, pero el
  symlink `~/.config/systemd/user/cc-telegram.service` sigue ahí, colgante.

### 4.4 Cola, deduplicación y estado

- Sin cola ni rate-limit propios: cada POST va a `GLib.idle_add`. La deduplicación es "un popup por
  `sesión|pane`" y el tope de 8.
- Estado en disco: `H/notifyd-pos.json` (ancla); lee `H/cc-notify.conf` en cada petición
  (`_conf_value`, `:58-67`).
- La política de avisos (no leídos, sonido por dispositivo, push) vive en APP y en `cc-dash`
  (`lib/notification_delivery.py`), no en `cc-notifyd`.

### 4.5 Quién le habla

- Hook Rust: `POST <COMANDOS_NOTIFYD_URL|http://127.0.0.1:4778>/notify`, timeout 2 s, cuerpo
  compacto con campos en este orden: `title`, `body`, `session`, `kind`, `project`, `options`,
  `full` y `pane` solo si existe (`crates/comandos-runtime/src/hooks/claude.rs:401-436`;
  envío en `notify_http.rs:128-140`, `:223-259`). Si falla y hay `osascript`, notificación de
  macOS. Solo se envía si `DESKTOP_NOTIFY` está activo (`claude.rs:409-411`).
- `cc-dash`: `/notify-popup` (`:475-489`), alertas de uso (`:455-472`), modelos y noticias nuevas
  (`:684-691`, `:733-740`).
- `cc-app`: `notify_popup` con `kind:"done"`, `session:"local"` (`bin/cc-app:1058-1072`).
- `hooks/cc-notify.sh:448` (versión bash; hoy `H/cc-notify.sh` es symlink al binario Rust).

## 5. Terminal web

### 5.1 Cableado actual

- `cc-webterm` (`bin/cc-webterm`) arranca dos ttyd como unidades transitorias de systemd
  (`systemd-run --user --collect --unit=cc-webterm|cc-webterm-path`, `:173-178`; no hay archivo
  `.service` en `systemd/`):
  - `cc-webterm`: `ttyd -p 4779 -i 127.0.0.1 --url-arg -t … cc-webterm-attach` (raíz; UI
    embebida de ttyd).
  - `cc-webterm-path`: `ttyd -p 4780 -b /term -I dash/term.html …` (índice propio).
  - Opciones `-t` comunes: tema, `fontSize=11`, `fontFamily`, `rendererType=canvas`,
    `scrollback=10000`, `cursorBlink`, `disableLeaveAlert`, `disableResizeOverlay`
    (`:141-148`). ttyd instalado: 1.6.3 (escribible por defecto, sin `-W`).
- `tailscale serve` (lo configura `cc-dash`, `bin/cc-dash:5093-5131`): `https://<host>/` → 4777,
  `https://<host>/term` → `127.0.0.1:4780/term`, `https://<host>:8443/` → 4779. Así está hoy
  (`tailscale serve status`).
- `cc-dash` decide si la terminal está sana sondeando `http://127.0.0.1:4780/term/token` y
  `http://127.0.0.1:4779/token` (timeout 0.4 s, `:4981-4986`).
- `H/webterm-enabled` (0600) lo crea `cc-webterm` al quedar activo y lo usa `cc-dash` para
  relanzarlo al arrancar (§3.5).

### 5.2 URLs que usa el tablero

- Remoto (HTTPS no loopback): `TERM_BASE = ${origin}/term` (`dash/index.html:3048-3050`); iframe
  `${TERM_BASE}/?auth=<token>&arg=<sesión>&theme=<tema>` (`:3863-3866`, `:4140-4143`); si `/term` no
  responde tras 3 intentos, cae a `https://<host>:8443/?arg=<token>&arg=<sesión>` (`:3276-3287`).
- Tablero dentro de `cc-app` (loopback): los terminales de la barra lateral usan
  `http://127.0.0.1:4779/?arg=<token>&arg=<sesión>`, es decir, **la UI propia de ttyd, no
  `term.html`** (`dash/index.html:4144-4148`).
- Pestaña experimental de `cc-app`: `file://…/dash/term.html?auth=<token>&arg=<sesión>&theme=…`
  contra `ws://127.0.0.1:4779/ws` (`bin/cc-app:3512-3567`); las pestañas normales son VTE.
- `cc-app-mac` usa `ws://127.0.0.1:4779/ws` y `http://127.0.0.1:4779/token`
  (`bin/cc-app-mac:94-95`).
- El token sale de `GET /webterm-token` (`dash/index.html:1958-1964`).

### 5.3 Protocolo y tamaño antes del attach

- `term.html` abre `new WebSocket(ws_url, ['tty'])` con
  `ws_url = <base>/ws?arg=<token>&arg=<sesión>` (`dash/term.html:313-325`, `:826`).
- Primer mensaje: JSON crudo sin prefijo `{"AuthToken":"","columns":C,"rows":R}`
  (`:839-845`). ttyd espera este mensaje para lanzar el proceso con ese tamaño, y entonces ejecuta
  `cc-webterm-attach <token> <sesión>`, que valida el token y hace
  `exec tmux attach -f active-pane -t =<sesión>` (tmux ≥ 3.2; aquí 3.2a)
  (`bin/cc-webterm-attach:27-31`, `:58-63`). Así el PTY nace con el tamaño del cliente y la sesión
  no se encoge. Sin sesión válida muestra un selector numerado y, si no, un `$SHELL`.
- Mensajes (comentario `dash/term.html:638-643`): cliente → servidor `'0'+bytes` entrada,
  `'1'+JSON` resize, `'2'` pausa, `'3'` reanudar; servidor → cliente `'0'+bytes` salida, `'1'+texto`
  título, `'2'+JSON` preferencias (fuente). Binario (`arraybuffer`).
- Reconexión con backoff 250 ms × 2ⁿ hasta 4 s (`:644-672`); cola de entrada pendiente de 4 096
  bytes / 15 s (`:679`); un cambio de tamaño puede reabrir el socket (`resizeReconnectSocket`,
  `:859-868`).
- xterm.js 5.5.0 con `addon-fit` 0.10.0, `addon-web-links` 0.11.0, `addon-canvas` 0.7.0 y
  `addon-ligatures-web` (más `opentype.min.js`), vendorizados en `assets/xterm/`
  (`assets/xterm/README.md:9-16`; `dash/term.html:280-286`).
- Además de la terminal, `term.html` llama a `cc-dash`: `/terminal-panes`, `/tab-models` (2 s),
  `/terminal-history`, `/tmux-scroll`, `/accounts`, `/account/switch`, `/account/add`,
  `/model/status`, `/workspace`, `/workspace/client`.

### 5.4 Diferencias con la spec

La spec (§4.4) propone `/term/ws?session=…` en `comandos dash`. El spike usa otro protocolo
(binario = teclado, texto `R <cols> <rows>` = resize;
`docs/research/2026-10-04-spike-terminal-rust.md:28-30`). Mientras `term.html`/xterm.js siga en
uso (hasta la Fase 3), el servidor Rust tiene que hablar el protocolo de ttyd (subprotocolo `tty`,
mensaje inicial JSON, prefijos `'0'..'3'`, token y sesión como `arg` repetidos) o hay que cambiar
`term.html` a la vez. Además, la barra lateral del tablero en `cc-app` usa la UI embebida de ttyd en
4779, que no existe en Rust.

## 6. Unidades systemd, symlinks y quién habla con 4777/4778

### 6.1 Unidades

| Unidad | Origen | Exec | Estado hoy |
|---|---|---|---|
| `cc-dash.service` | `~/.config/systemd/user/cc-dash.service` → `<repo>/systemd/cc-dash.service` | `%h/.local/bin/cc-dash 4777 --no-open`, `Restart=on-failure`, `RestartSec=3` | activa, pid 3529904 |
| `cc-notifyd.service` | → `<repo>/systemd/cc-notifyd.service` | `%h/.local/bin/cc-notifyd`, `After=graphical-session.target` | activa, 30 MB |
| `cc-webterm.service` | transitoria (`/run/user/1000/systemd/transient/`) | `ttyd -p 4779 …` | activa |
| `cc-webterm-path.service` | transitoria | `ttyd -p 4780 -b /term -I …/dash/term.html …` | activa |
| `tmux.service` | → `<repo>/systemd/tmux.service` | `tmux start-server` + `exit-empty off` | no se toca |
| `comandos-broker.service` | → `<repo>/systemd/comandos-broker.service` | binario Rust | Fase 1 |

Ninguna unidad del repo tiene `MemoryMax` salvo el broker; `cc-dash.service` no tiene límites ni
`ManagedOOMPreference`.

### 6.2 Symlinks (leídos con `readlink`)

- `~/.local/bin/cc-dash` → `<repo>/bin/cc-dash`
- `~/.local/bin/cc-notifyd` → `<repo>/bin/cc-notifyd`
- `~/.local/bin/cc-webterm` → `<repo>/bin/cc-webterm`
- `~/.local/bin/cc-webterm-attach` → `<repo>/bin/cc-webterm-attach`
- `~/.local/bin/cc_usage.py` → `<repo>/bin/cc_usage.py`
- Ya en Rust: `~/.local/bin/cc-extensions`, `H/cc-notify.sh`, `H/cc-status.sh`,
  `H/cc-usage-tool.sh` → `~/.local/share/comandos/bin/comandos`.
- `H/dash/*` → `<repo>/dash/*` (§3.4).

`crates/comandos-cli/src/dispatch.rs:15-24` aún no tiene entradas para `cc-dash`, `cc-notifyd`
ni `cc-webterm`, ni subcomandos `dash`/`notifyd` (`:36-42`); `comandos-cli` no depende de
`comandos-server` (`crates/comandos-cli/Cargo.toml`).

### 6.3 Clientes de 4777

`cc-app` (`BASE_URL`, `bin/cc-app:86`; rutas en §1), `cc-app-mac` (`bin/cc-app-mac:85`, usa
además `/tab-metadata*`), `cc-notifyd` (`/key`, `/send`, `/focus`, `/prefs`, `/state`,
`bin/cc-notifyd:246-259`), `cc-next` (`/focus`, `/up`, `bin/cc-next:15-49`), `cc-webterm`
(`/webterm-token`, `bin/cc-webterm:39`), `cc-doctor` (`/prefs` como sonda, `bin/cc-doctor:196`),
`cc-mobile` (puerto para `tailscale serve`), `lib/operator_dispatch.py` (el propio `cc-dash` a sí
mismo, `bin/cc-dash:5762-5765`) y los navegadores. Ni el broker Rust ni el hook Rust llaman a 4777
(búsqueda en `crates/*/src`).

### 6.4 Clientes de 4778

Hook Rust (`notify_http.rs:23`), `cc-dash` (4 sitios, §4.5), `cc-app` (`bin/cc-app:1064`),
`hooks/cc-notify.sh:448` (no instalado).

## 7. Estado de `comandos-server`

### 7.1 Qué hace

- `lib.rs` (434 líneas): transporte hyper 1.11 HTTP/1.1 sobre tokio con keep-alive, límites de
  conexiones, cabeceras, cuerpo agregado, timeouts de cabecera/cuerpo/handler/escritura, cierre
  ordenado (`Limits`, `:87-102`; `serve`, `:348-434`). Admisión con `dashboard_access`
  (`dispatch`, `:209-346`). Respuestas `Bytes` o `Stream` (canal acotado, `:51-55`), siempre con
  `Cache-Control: no-store` y framing calculado (`response`, `:160-183`). Errores con los mismos
  textos que Python (`reject`, `:184-190`). No conoce rutas: recibe un `Handler`.
- `blocking.rs` (154 líneas): `BlockingWorker`, un hilo dueño de un backend síncrono (SQLite) con
  cola acotada, separado del runtime de red.
- `events_routes.rs` (294 líneas): 4 rutas — `GET /events/v2`, `GET /work-marks`,
  `POST /events/v2`, `POST /work-marks` — sobre `comandos-store`, con import del timeline legado.
- `write_timeout.rs` (97 líneas): timeout de escritura por socket.
- Tests: `transport.rs` (769 líneas, 19 casos: bytes JSON, keep-alive, seguridad, límites, stream,
  pánicos, latin-1), `events_routes.rs` (539), `events_routes_socket.rs` (319), `blocking.rs`
  (278).

### 7.2 Cobertura frente a la tabla

4 de 161 rutas (2.5 %), y son de las que no tienen llamador vivo salvo `/work-marks`. Piezas de
dominio ya portadas a otros crates que sirven para rutas de la tabla (librerías, no rutas):

| Crate | Módulo | Rutas que alimenta |
|---|---|---|
| `comandos-core` | `analytics_week.rs` (885), `allocation.rs` (391) | `/analytics/week` |
| `comandos-core` + `comandos-store` | `pomodoro.rs` (981 + 628), `focus.rs` | `/pomodoro`, `/pomodoro/report` |
| `comandos-core` + `comandos-store` | `notifications.rs` (251 + 288) | `/notices*`, `/presence`, `/notifs/count` |
| `comandos-core` + `comandos-store` | `workspace.rs` (542 + 355 layout + 341) | `/workspace*` |
| `comandos-runtime` | `quick_terminal.rs`, `terminal_history.rs`, `terminal_panes.rs`, `pane_typing.rs` | `/terminal/quick`, `/terminal-history`, `/terminal-panes`, `/pane/type` |
| `comandos-runtime` | `session_operations.rs` (587) | `/session/configure`, `/account/switch`, `/model/*` (journal; el efecto en tmux falta) |
| `comandos-runtime` | `accounts.rs` (548), `model_catalog.rs` (295) | `/accounts`, `/providers` (parcial) |
| `comandos-store` | `usage.rs` (1 089) | solo las ramas de hook de `cc_usage`; **no** `build_usage_state` |

El esquema SQLite de APP está a la par (migraciones 1–11 copiadas,
`crates/comandos-store/src/state_db/migrations.rs` frente a `lib/app_state.py:15-340`).

### 7.3 Qué falta

- **Rutas**: 157 de 161, incluidos los dominios pesados sin portar: `/state` (`read_states` y la
  inspección de procesos/transcripts), `/usage/state` (`build_usage_state`, 3 361 líneas de
  `cc_usage`), noticias (2 805 líneas de `news_*`), extensiones (`extension_launch`, 607),
  perfiles, SSH, remoto/tailscale, snippets, cadenas, catálogo de CLIs.
- **Estáticos**: no hay servidor de archivos (la spec quiere `include_bytes!`); falta decidir
  listados de directorio, 404 HTML, `Last-Modified`/304 y tipos MIME.
- **HEAD**: el transporte responde 501 a todo método que no sea GET/POST/DELETE (`lib.rs:220-225`);
  Python sirve HEAD de estáticos.
- **WebSocket**: `http1::Builder` sin `with_upgrades()` (`lib.rs:403-407`): no hay upgrade para
  `/term/ws`.
- **Long-poll**: `/notices/watch` necesita hasta 25 s; el `handler_timeout` aplica a producir la
  respuesta, así que o se configura por encima de 25 s o la ruta se implementa como `Stream`
  (el test `review_quiet_event_stream_can_outlive_write_and_handler_timeouts` muestra que el stream
  sí puede exceder el timeout).
- **SSE** `/events/stream`: no existe en ningún lado (ver Resumen).
- **Binario**: no hay `comandos dash`/`notifyd` en `comandos-cli`; tampoco hilos de fondo
  (watch de modelos, noticias, push, límites, Pomodoro), `motor_queue_resume`, ni el relanzamiento
  de la terminal web.
- **Arnés de paridad**: `xtask` solo tiene `rss.rs`; no existe `xtask parity` (spec §5.2).

## 8. Pruebas existentes (oráculo)

- 169 archivos `tests/test_*.py` con 1 878 funciones `def test_`; 48 archivos mencionan
  `cc-dash`, 6 mencionan `cc-notifyd`/4778. La spec habla de 173 tests.
- **Patrón A, caja negra (portable a Rust)**: `tests/dash_harness.py` arranca
  `python3 bin/cc-dash <puerto> --no-open` como subproceso con `HOME=tmp_path`, `TMUX_TMPDIR`
  privado, sin `$TMUX`, `H/app-tabs.json = {}` y token fijo `dash-harness-test-token`; todas las
  peticiones llevan `X-Forwarded-For: 100.64.0.9`, así que ejercitan la puerta remota
  (`tests/dash_harness.py:88-135`). Lo usan `test_chains_endpoints.py` (6),
  `test_commands_catalog_endpoint.py` (7), `test_pane_type_endpoint.py` (7),
  `test_operator_retired.py` (6). Cambiar la línea del `Popen` por el binario Rust los convierte en
  pruebas de paridad.
- **Patrón B, en proceso (no portable)**: cargan el script con `SourceFileLoader` y montan
  `http.server.ThreadingHTTPServer(("127.0.0.1", 0), dash.Handler)` con `monkeypatch` sobre
  funciones del módulo (p. ej. `tests/test_dashboard_security.py:22-31`, `:50-58`):
  `test_dashboard_security.py` (19), `test_dash_hardening.py` (20), `test_events_endpoints.py`
  (9), `test_notice_endpoints.py` (14), `test_workspace_endpoints.py` (11),
  `test_news_endpoints.py` (8), `test_push_endpoints.py` (7), `test_quick_terminal.py` (24).
  Otros importan el módulo y llaman funciones sin servidor: `test_usage_dash.py` (53),
  `test_pomodoro_endpoint.py` (13), `test_session_profiles.py` (14), `test_dash_concurrency.py`
  (17), `test_agent_launch.py` (14), `test_session_config_history.py` (12). Sirven como
  especificación, no como oráculo ejecutable contra Rust.
- **Interfaz**: `test_remote_ui.py` (84), `test_remote_controls.py` (34),
  `test_remote_workspace.py` (4) inspeccionan HTML/JS; los `*.cjs` de `tests/` prueban los módulos
  JS con `dom_stub.cjs`; `e2e_*.js|cjs` necesitan navegador.
- **Notificaciones**: `test_notifyd_robust.py` (4) importa `bin/cc-notifyd` con GTK y prueba
  `evict_candidate`/`stale_waiting`; `test_notify_hook.py` cubre el hook.
- **Terminal**: `test_webterm_auth.py` (9) prueba `cc-webterm-attach` (token, recorte, vacío) y
  extrae funciones JS de `term.html` (reconexión, orden token/sesión en la URL del WS).
- **Rust**: `crates/comandos-core/tests/dashboard_access.rs` (fixture de paridad de la puerta),
  `crates/comandos-server/tests/*` (§7.1) y oráculos Python en `crates/*/tests/*_oracle.py`.

## 9. Riesgos y decisiones para el plan

1. **Alcance real.** 161 rutas sobre ≈ 26 500 líneas de Python, no 124 sobre 10 000. Decidir antes
   del plan qué hacer con las 39 rutas sin llamador vivo (§1.12): portarlas, devolver 410 como
   `/operator`, o retirarlas en Python primero. Es el mayor recorte posible de trabajo.
2. **Orden por dominio, no por ruta.** El cutover de `cc-dash` es de una sola unidad: no se puede
   pasar media API. Opción: `comandos dash` como proxy inverso que atiende en Rust lo portado y
   reenvía el resto a Python en otro puerto, para cortar por dominios con reversión inmediata.
   Hay que decidirlo pronto porque cambia el arnés y la puerta de seguridad (el proxy añade
   `X-Forwarded-For`; Python lo trataría como remoto).
3. **Paridad byte a byte.** `json.dumps` con `ensure_ascii` y separadores de Python, orden de
   inserción, 404 HTML de la librería, listados de directorio, 501 HTML, HEAD sin puerta y
   `do_DELETE` sin `_guard_request`. Decidir cuáles se replican y cuáles se corrigen (y se
   documentan como diferencia aceptada en el arnés).
4. **Terminal web.** Mientras viva `term.html` + xterm.js, Rust debe hablar el protocolo de ttyd
   (subprotocolo `tty`, JSON inicial con tamaño, prefijos `'0'..'3'`, `arg` repetidos) en vez del
   del spike, y servir algo en 4779/4780 o actualizar `index.html`, `cc-app` (barra lateral y
   pestaña xterm) y `cc-app-mac` a la vez. Además hace falta upgrade WebSocket en el transporte.
5. **Relanzamiento de la terminal y `tailscale serve`.** `cc-dash` relanza `cc-webterm` al arrancar
   y configura las rutas de Tailscale. Si `comandos dash` sirve la terminal, las rutas
   `/term` → 4780 y `:8443` → 4779 cambian; y `POST /remote-off` no debe ejecutar
   `tailscale serve reset`, que hoy borraría los servicios de otros proyectos (8444–8447).
6. **Long-poll y timeouts.** `/notices/watch` (25 s), `/app/command` (espera a `cc-app`),
   `/proxy` (1 s de `sleep`), `/remote-*` (hasta ≈ 30 s de `tailscale`/`cc-webterm`),
   `/usage/refresh` y noticias (red) no caben en un `handler_timeout` único corto ni en un
   `BlockingWorker` serial. Definir clases de rutas: SQLite serial, procesos con su propio
   timeout, red en tareas, long-poll como `Stream`.
7. **Efectos en GET.** `/state` escribe `H/app-tab-models.json` y `/usage/state` escribe bordes de
   tmux y `H/pane-models.txt`. En la sombra de solo lectura (spec §5.2) esos GET no son inocuos:
   el arnés debe correrlos contra una copia de `~/.claude/hooks` o desactivar esas escrituras.
8. **Memoria.** No basta con "cachés acotadas": el crecimiento probable viene de un hilo por
   conexión, del churn de `/state`/`/work-marks` y del memo de uso. Fijar en el plan un criterio
   medible (RSS tras N horas de polling sintético según §1.11) y plantear SSE para `/state`,
   `/work-marks`, `/notices` y `/pomodoro`, que hoy suman varias peticiones por segundo.
9. **`notifyd` es solo popups GTK.** La Fase 2 de `notifyd` es pequeña (una ruta, 1 139 líneas casi
   todas de GTK), pero trae GTK3 a `comandos` (spec §4.1 dice binario headless). Decidir si va en
   `comandos-app`, en un binario aparte o con gtk-rs en `comandos notifyd`, y corregir la spec:
   sonidos y voz ya están en el hook, Telegram está retirado. Web Push no funciona hoy
   (`pywebpush` no instalado): decidir si se porta con un crate VAPID o se deja como
   "no disponible".
10. **Oráculo.** Solo 4 archivos de tests son de caja negra (`dash_harness`). Los demás monkeypatchean
    el módulo. Antes de portar dominios, convertir los tests de endpoints del patrón B al patrón A
    (o a fixtures de entrada/salida) para tener un oráculo ejecutable contra ambos servidores.
