# CommandOS 1.0 — registro de aceptación

Base: main `c0c68cf`. Rama: `implementation/comandos-v1`. Entorno de pruebas: `/tmp/comandos-v1-qa` (Python 3.10, pytest 9.1.1; dependencias en `/tmp/comandos-v1-qa-requirements.txt`).

## Decisiones de Jesús (2026-09-29, respuestas literales a propuestas concretas)

| ID | Respuesta | Alcance aplicado |
| --- | --- | --- |
| D1 | "Se conservan" | Congelado y Esperando respuesta solo cambian por acción humana. Solo Resuelto se reabre con un prompt aceptado y confirmado. Tras terminar un turno el pane queda sin marca (icono neutral). |
| D2 | "La del mockup" | Política `v1`: 10 XP por minuto de foco activo, nivel cada 1.000 XP, objetivo diario 100 min. Cancelados cuentan solo su tiempo activo; descansos no dan XP. Sin puntos retroactivos sobre historial previo. |
| D3 | "Global, ciclos manuales" | Un estilo de arte para toda la app (Alquimia inicial). Al terminar un foco no arranca el descanso automáticamente. |
| D4 | "09:00 · 15:00 · 21:00" | America/Mexico_City. Máx. 25 fuentes y tope ≈ US$0,25 por edición con un modelo económico. Una edición perdida por caída se marca como no publicada, sin recuperarla después. |
| D5 | "Sonido solo si te necesita" | Sonido en permiso/entrada pedida, error y fin de Pomodoro; turno terminado solo visual. Float 6 s; ráfagas del mismo proyecto se agrupan 10 s. Durante un foco solo suenan permisos. Push al Android si ningún cliente está visible desde hace 2 min. |
| D6 | "Proyecto + título" | Pantalla bloqueada: proyecto y título breve, sin extracto. Si el último cliente no puede sonar, suena el siguiente cliente visible. |

## Evidencia por bloque

Leyenda: **Impl** = implementado y en commit; **Auto** = pruebas automáticas ejecutadas en esta rama;
**Nav** = comprobado en Chrome de la Mac mini (chrome-bg) sobre el candidato aislado; **Esc** = comprobado en el
cc-app candidato (GTK sobre Broadway, HOME/tmux/estado aislados); **Jesús** = aceptación humana (pendiente = R2).

| Bloque | Commits | Impl | Auto | Nav / Esc | Jesús |
| --- | --- | --- | --- | --- | --- |
| W1 Documento y restauración | c0a810b | Sí | test_workspace_state, test_workspace_endpoints, test_tmux_snapshot | Esc: arreglo compartido visible | pendiente |
| W2 Acoplar, desacoplar, redimensionar | 52cbff5, 3ef0b8a, 8474636, 7731619 | Sí | workspace_layout (py+node), workspace_dock_checks, test_gtk_workspace, test_app_workspace | Nav: 1440×1000, 820×1180, 390×844, 320×568, 667×375, táctil (hold 180 ms vs swipe), Escape; iframes sin recarga y PIDs de pane constantes. Esc: acoplar (rev 9), desacoplar, redimensionar (rev 11) | pendiente |
| W3 Foco por dispositivo y cierres | da2ec26, c2e7509 | Sí | test_workspace_clients (2 PTY reales en tmux privado), test_workspace_close, test_terminal_panes, test_app_workspace | Nav: abrir aviso seleccionó `%1` solo en ese navegador (tecleo cayó en `%1` con `%5` activo compartido); diálogo de cierre de grupo lista miembros y LOCAL se queda; Cancelar conserva todo. Esc: confirmación de grupo GTK con la misma lista | pendiente |
| W4 Borrador y lectura | 6060a8e | Sí | test_workspace_reopen, device_drafts_checks 6/6 | pendiente (cerrar/reabrir con Jesús) | pendiente |
| E1 Marcas | f36044c | Sí | test_work_marks*, work_marks_checks | pendiente | pendiente (icono neutral y transición) |
| E2 Terminal rápida | 733aa14 | Sí | test_quick_terminal* (incl. tmux privado real) | botón visible | pendiente |
| E3/E4 Extensiones y chat | ccb53e9, 9500ce5 | Sí | test_extension_batches/loading, operator/session suites | pendiente | pendiente |
| P1–P4 Pomodoro | c23b2e5 … 141926a | Sí | test_pomodoro*, test_focus_progress, pomodoro_sync 9/9, pomodoro_ui 15/15 | reloj visible en cabecera | pendiente (escucha real de sonidos) |
| N1 Eventos | fd0d52f | Sí | test_event_store, test_turn_state, test_events_endpoints | — | — |
| N2 Avisos | 2b6bdc1, 3084b8d, 95b35f5 | Sí | test_notification_delivery 31, test_notice_endpoints 7, notification_ui_checks 19/19 | Nav: float + franja por proyecto; llegada no cambió foco ni selección de un borrador (3–8); float oculto a los 6 s; ráfaga "2 turnos terminados en Relotto" en 390×844. Esc: presencia `desktop-<host>` con interacción explícita | pendiente (sonido real, D5 en uso) |
| N3 Telegram retirado | 2a37444 | Sí | test_telegram_retirement (systemctl simulado) | — | activación en R3 |
| N4 Push Android | 0aa1665 | Sí | test_web_push, test_push_endpoints, sw_push_checks, push_settings_checks | — | **pendiente en Android físico** |
| N5 Ediciones | 5bafe51, 9259fc1, 243cfbd, df7b1cb | Sí | test_news_editions, test_news_endpoints, news_reader_checks | pendiente | pendiente (sin configurar: no inventa noticias) |
| R1 Integración | c2e7509, 61db013 | — | suite completa tras revisión final: **2240 passed, 22 failed (los 22 preexistentes de main: browser_config_migration 12, extension_auth 9, extension_proxy 1), 7 skipped**; test_js_parses OK; migraciones 6/6 | — | — |

Excluido de la suite completa: `tests/test_extension_catalog.py` (no colecciona en Python 3.10, sin `tomllib`) y los E2E
reales de `tests/test_acp_client.py` (solo con `COMANDOS_E2E=1`, usan suscripciones reales). Ninguna prueba contacta
los servicios reales (4777–4780): las referencias a esos puertos son cadenas o `urlopen` sustituido.

## Medidas y hallazgos de integración

- **Tamaño de ventana tmux (W3):** con `window-size latest` (config actual), al conectarse/reconectarse el móvil
  (60×30) la ventana compartida baja a 60×29 hasta que el escritorio (200×50) escribe; con `ignore-size` el
  escritorio conserva 200×49 pero el móvil ve un recorte. Se mantiene `latest` en v1; **decisión para Jesús en R2**.
- **Panel web del cc-app bajo Broadway:** la columna del panel se ve estrecha en el candidato aislado, igual con el
  dashboard de `main` servido al mismo cc-app: artefacto previo del WebKit bajo Broadway, no de esta rama.
- **Proxy de ensayo:** el proxy del candidato enviaba `/terminal-panes` a ttyd por prefijo `/term`; corregido en el
  arnés (no afecta al producto: la ruta ya existía en producción).

## Revisión final (revisor en contexto nuevo, opus)

Veredicto inicial "arreglar primero": 6 hallazgos importantes (+1 reclasificado), todos corregidos en `61db013`
con una prueba que falló antes del arreglo:

| Hallazgo | Arreglo | Prueba |
| --- | --- | --- |
| Pruebas escribían la base real | aislamiento por prueba + guarda de sesión; sin hilos al importar cc-dash; temporizador del sonido Pomodoro daemon y ligado a su base | conftest, test_importing_cc_dash_starts_no_background_loop, test_focus_end_sound_is_a_daemon_bound_to_the_emitting_database |
| Primer arranque en paralelo fallaba al migrar | relectura de versiones dentro de `BEGIN IMMEDIATE` | test_parallel_first_runs_all_succeed |
| Caracteres de control en el destino Pomodoro bloqueaban el fin | rechazo al iniciar; saneado al completar | test_pomodoro (2 nuevas) |
| `idle_prompt` de Claude sonaba y quedaba pendiente | no genera aviso (tampoco `auth_success`) | test_claude_idle_and_auth_notices_are_not_requests |
| Push de eventos ya vistos en pantalla | solo eventos posteriores al último momento visible | test_push_only_for_events_after_the_last_visible_moment |
| Ventana de escritorio abierta bloqueaba el push | visible = interacción explícita en los últimos 2 min | test_an_open_but_unattended_window_does_not_block_the_push |
| `app-tabs.json` ilegible colapsaba el arreglo (reclasificado) | se conserva el arreglo guardado | test_an_unreadable_registry_never_collapses_the_arrangement |

Menores aplazados (sin cambio en v1): fuga de temporal `NOTICE_OUT` en el hook, DELETE sin `_guard_request`,
`bindings` escribibles por el cliente en `POST /workspace`, mensaje de cierre de grupo tras error de red, base de
estado 0644, crecimiento sin poda de lecturas/entregas/eventos, 4778 fijo en cc-app/model-watch, ventana de
DNS-rebinding en noticias.

## Pendiente antes de activar (R2/R3)

- Recorrido humano con Jesús (tabla de R2 del plan 05), incluidos Android físico (permiso, pantalla bloqueada,
  toque al origen) y escucha real de sonidos.
- Decisión sobre el tamaño de ventana tmux entre móvil y escritorio.
- Activación según `docs/verification/commandos-v1-activation.md`.
