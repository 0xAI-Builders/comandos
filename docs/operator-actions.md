# Acciones del operador de ComandOS

Generado desde `lib/operator_catalog.py`. Un control de la UX sin fila aquí es un defecto.


## sessions (25)

| Tool | Qué hace | Parámetros | Cómo se ejecuta |
|---|---|---|---|
| `list_tabs` 👁 | Lista las pestañas abiertas (mismas que el escritorio). | — | GET /tabs |
| `list_sessions` 👁 | Estado de todas las sesiones/agentes: status, proyecto, último mensaje. | — | GET /state |
| `session_status` 👁 | Estado del panel exacto; scope=session incluye sus paneles; solo filas vivas salvo historical=true. | session, pane, scope, historical | GET /state, filtro por identidad |
| `get_active_tab` 👁 | Qué pestaña está activa en la app y su pane vivo. | — | GET /active-tab |
| `session_brain` 👁 | Configuración de skills/MCPs de un panel vivo, sin inferir uso. | session, pane; cwd debe coincidir si se indica | GET /state y GET /session-brain |
| `events_log` 👁 | Últimos 80 eventos de actividad (working/waiting/done). | — | GET /events |
| `dedication_stats` 👁 | Tiempo dedicado por proyecto (hoy y semana). | — | GET /dedication |
| `tab_history` 👁 | Pestañas cerradas recientemente (recuperables). | — | GET /tab-history |
| `focus_tab` | Trae al frente una pestaña/sesión. | tab | local:focus |
| `next_tab` | Pasa a la pestaña siguiente. | — | local:step_next |
| `prev_tab` | Pasa a la pestaña anterior. | — | local:step_prev |
| `close_tab` | Cierra la pestaña (la sesión tmux sigue viva). | tab | local:close |
| `rename_tab` | Renombra una pestaña. | tab, name | local:rename |
| `send_tab_back` | Manda la pestaña al final de la barra. | tab | local:send_back |
| `new_terminal_tab` | Nueva terminal (shell) como pestaña en ambos lados. | label | POST /tab-new |
| `recover_tab` | Recupera una pestaña cerrada por nombre. | name | local:recover |
| `new_ai_session` | Crea una sesión de IA nueva en una carpeta (wizard +): harness, motor, modelo, esfuerzo, cuenta, modo sin aprobaciones. | cwd, agent, model, effort, routeId, harnessAccount, motorAccount, danger | POST /session-new |
| `open_project` | Abre/crea la sesión de un proyecto por nombre (busca en ~/codebase). | session, agent | POST /new |
| `open_with_account` | Nueva sesión Claude en una carpeta con una cuenta concreta. | cwd, account, danger | POST /open-with-account |
| `session_open` | Revive si hace falta y enfoca la ventana de una sesión. | session, cwd, agent | POST /up |
| `session_ensure` | Asegura que la sesión y su ventana existen, sin robar el foco. | session, cwd, win, agent | POST /ensure |
| `session_shell` | Abre/enfoca la ventana shell de la sesión. | session, cwd | POST /shell |
| `toggle_shell` | Alterna entre la ventana de la IA y el shell (Ctrl-b l). | — | local:toggle_shell |
| `kill_session` ⚠ confirm | MATA la sesión tmux (irreversible). | tab, confirm | local:kill |
| `export_reply` | Exporta la última respuesta de la IA a txt o pdf y la abre. | session, format | POST /export |
| `favorite_toggle` | Marca/desmarca una sesión como favorita (★). | session, on | ui:call opFavorite |

## panes (12)

| Tool | Qué hace | Parámetros | Cómo se ejecuta |
|---|---|---|---|
| `send_text` | Escribe texto en el pane y pulsa Enter (responder a la IA). | tab, text, pane | local:send |
| `paste_text` | Pega texto (bracketed paste) SIN Enter. | session, text, pane | POST /paste |
| `send_key` | Envía una tecla: Enter, Escape, Up, Down, Tab, y, n, 1-9. | key, tab, pane | local:key |
| `choose_option` | Elige la opción N de un menú numerado de la IA (manda el número y Enter). | tab, option | local:choose_option |
| `interrupt_turn` | Para el turno actual de la IA (cancela cambio en cola y manda Escape). | tab, pane | local:interrupt |
| `pause_agent` | Pausa (SIGSTOP) el proceso de la IA. | tab | local:pause_on |
| `resume_agent` | Reanuda (SIGCONT) el proceso de la IA. | tab | local:pause_off |
| `split_pane` | Divide el pane: derecha, izquierda, abajo o arriba. | side, tab | local:split |
| `close_split` ⚠ confirm | Cierra el split activo (kill-pane). | tab, confirm | local:close_split |
| `tmux_mouse_get` 👁 | Lee si el modo ratón de tmux está activo en la sesión. | session | GET /tmux-mouse |
| `tmux_mouse_set` | Activa/desactiva el modo ratón (seleccionar texto vs interactuar). | session, enabled | POST /tmux-mouse |
| `tmux_scroll` | Desplaza el historial del pane N líneas (negativo=arriba). | session, delta | POST /tmux-scroll |

## models (21)

| Tool | Qué hace | Parámetros | Cómo se ejecuta |
|---|---|---|---|
| `proxy_state` 👁 | Estado del gateway: motor y modelo global, cuentas, logins. | — | GET /proxy |
| `list_providers` 👁 | Registro de proveedores, harnesses, motores y rutas disponibles. | — | GET /providers |
| `list_model_tiers` 👁 | Tabla de tiers de modelos (barato/rápido/potente). | — | GET /model-tiers |
| `list_opencode_models` 👁 | Catálogo de modelos de OpenCode. | — | GET /opencode/models |
| `sovereignty_report` 👁 | Reporte de soberanía (qué corre local vs nube). | — | GET /sovereignty |
| `model_switch` | Cambia modelo/motor/esfuerzo del pane de una sesión en vivo. | session, pane, routeId, provider, model, effort, motor, harnessAccount, motorAccount, interrupt | POST /model/switch |
| `model_switch_cancel` | Cancela un cambio de modelo en cola. | session, pane | POST /model/switch-cancel |
| `model_switch_status` 👁 | Progreso de un cambio de modelo/motor en curso. | operationKey | GET /model/status |
| `harness_switch` | Cambia el CLI (Claude Code, Codex, Grok Build, OpenCode, ACP) de un pane con traspaso de contexto. | session, pane, toHarness, model, effort, account, motor, danger, interrupt | POST /harness/switch |
| `set_global_motor` | Fija motor y modelo GLOBAL del gateway. | motor, model | POST /proxy |
| `gateway_enable` | Enciende/apaga el gateway (proxy) de modelos. | enable | POST /proxy |
| `account_add` | Añade una cuenta (alias) de un proveedor y abre su login. | provider, alias, cwd | POST /account/add |
| `account_switch` | Mueve la conversación viva a otra cuenta Claude. | session, pane, alias, interrupt | POST /account/switch |
| `skill_toggle` | Activa/desactiva una skill (aplica al reciclar la sesión). | path, on | POST /skill-toggle |
| `mcp_toggle` | Activa/desactiva un MCP del proyecto. | cwd, name, on | POST /mcp-toggle |
| `optimization_plans` 👁 | Perfiles de optimización (ahorro, equilibrio, potencia). | — | GET /optimization/plans |
| `optimization_set_default` | Perfil de optimización por defecto. | profile | POST /optimization/default |
| `optimization_apply` | Aplica un perfil a varias sesiones (un model_switch por sesión). | profile, sessions | local:optimization_apply |
| `undo_guard_switch` | Deshace el último cambio de modelo hecho por la guardia (vuelve al anterior). | session, model | POST /model/switch |
| `open_motor_picker` | Abre el selector de motor/modelo/cuenta de un pane en el tablero. | session, pane | ui:call openMotorFor |
| `open_global_motor_picker` | Abre el selector de motor GLOBAL. | — | ui:click #motor-global |

## usage (18)

| Tool | Qué hace | Parámetros | Cómo se ejecuta |
|---|---|---|---|
| `usage_state` 👁 | Uso y cuotas por proveedor/cuenta, alertas, salud de credenciales. | — | GET /usage/state |
| `session_usage` 👁 | Contadores asociados al panel; scope=session incluye sus paneles sin sumar contadores compartidos. Sin datos significa desconocido. | session, pane, scope | GET /usage/state, filtro por identidad |
| `usage_guard` 👁 | Guardia anti-desborde con pronóstico por proyecto. | — | GET /usage/guard |
| `usage_changes` 👁 | Ledger de cambios de modelo hechos por la guardia. | — | GET /usage/changes |
| `usage_provider_compare` 👁 | Comparativa de costo/uso entre proveedores. | days | GET /usage/provider-compare |
| `usage_analytics` 👁 | Analytics de experimentos A/B por tipo de tarea. | days, taskType | GET /usage/analytics |
| `usage_interactions` 👁 | Últimas interacciones registradas. | limit | GET /usage/interactions |
| `usage_experiments` 👁 | Lista de experimentos A/B. | — | GET /usage/experiments |
| `usage_experiment_create` | Crea un experimento A/B. | label, taskType, variants, minPairs | POST /usage/experiment |
| `usage_experiment_pair` | Registra un par de comparación en un experimento. | experimentId, projectId | POST /usage/experiment |
| `usage_rate` | Califica una interacción: Mal, Parcial o Resuelto. | interactionId, outcome, rating, note, taskType | POST /usage/rating |
| `usage_refresh` | Vuelve a bajar uso y costos de todos los proveedores. | — | POST /usage/refresh |
| `usage_set_quota` | Declara la cuota de 7 días de un proveedor (0 la borra). | provider, tokens7d | POST /usage/quota |
| `usage_set_subscription` | Declara el costo mensual/moneda de un proveedor para el ROI. | provider, monthly, currency, display | POST /usage/subscription |
| `usage_alert_rule_set` | Crea/actualiza una alerta de presupuesto (proyecto, pane o proveedor). | id, scope, target, label, threshold | POST /usage/alert-rule |
| `usage_alert_rule_delete` | Borra una alerta de presupuesto. | id | POST /usage/alert-rule |
| `usage_set_thresholds` | Umbrales globales de alerta (p. ej. 70,85,95). | thresholds | POST /usage/settings |
| `usage_settings_set` | Escribe ajustes de uso arbitrarios. | settings | POST /usage/settings |
| `ui_log_summary` 👁 | Resumen de telemetría de la UI. | — | GET /ui-log/summary |

## pomodoro (8)

| Tool | Qué hace | Parámetros | Cómo se ejecuta |
|---|---|---|---|
| `pomodoro_state` 👁 | Bloque de foco actual, cola y analytics de 7 días. | — | GET /pomodoro |
| `pomodoro_start` | Inicia un bloque de foco/descanso. | mins, mode, project, session, cycleIndex, cycleTotal | POST /pomodoro |
| `pomodoro_stop` | Termina/salta el bloque actual. | status | POST /pomodoro |
| `pomodoro_ack` | Vacía la cola de notificaciones del pomodoro. | — | POST /pomodoro |
| `pomodoro_settings` | Duración, descanso, ciclos y auto-descanso. | mins, breakMins, cycles, auto | POST /pomodoro |
| `pomodoro_extend` | Añade 5 minutos al bloque en marcha. | — | ui:click #pp-extend |
| `pomodoro_skip` | Salta el bloque en marcha. | — | ui:click #pp-skip |
| `open_pomodoro` | Abre el panel de pomodoro. | — | ui:click #btn-pomo |

## prefs (20)

| Tool | Qué hace | Parámetros | Cómo se ejecuta |
|---|---|---|---|
| `get_conf` 👁 | Configuración (volumen, notificaciones, idioma, worktrees…). | — | GET /conf |
| `get_prefs` 👁 | Preferencias (tema, fuente, cursor, favoritos) y fuentes instaladas. | — | GET /prefs |
| `set_pref` | Switch de configuración on/off. | key, on | local:pref |
| `set_voice` | Voz que anuncia el proyecto (SPEAK_DONE + SPEAK_ATTENTION). | on | local:voice |
| `set_volume` | Volumen de voz y chime 0-100. | percent | POST /conf-set |
| `set_language` | Idioma del tablero: auto, es, en. | lang | local:lang |
| `set_theme` | Tema visual del tablero y terminales. | theme | local:theme |
| `set_notify_corner` | Esquina de los avisos: tl, tr, bl, br o free. | corner | local:notify_pos |
| `set_terminal_font` | Familia y/o tamaño de fuente del terminal. | family, size | POST /prefs-set |
| `set_cursor` | Forma y parpadeo del cursor. | shape, blink | POST /prefs-set |
| `set_ligatures` | Ligaduras tipográficas en el terminal. | on | POST /prefs-set |
| `set_terminal_padding` | Margen interno del terminal. | px | POST /prefs-set |
| `set_terminal_opacity` | Opacidad del fondo del terminal 0-100. | percent | POST /prefs-set |
| `set_poll_seconds` | Segundos de refresco del tablero remoto. | seconds | ui:call setPollSeconds |
| `set_browser_notifications` | Notificaciones del navegador on/off. | on | ui:call setBrowserNotifications |
| `test_notification` | Prueba un aviso: voz, chime o terminado. | kind | POST /test |
| `open_settings` | Abre Ajustes en una tab: Apariencia, Notificaciones, Terminal o Tablero. | tab | local:ui_settings |
| `set_limit_style` | Estilo de la barra de límites en Analytics. | style | ui:call setLimitStyle |
| `notif_dismiss_ids` | Marca como leídas notificaciones por id (persistente). | ids | POST /prefs-set |
| `notif_snooze_ids` | Pospone notificaciones por id hasta una marca de tiempo. | snooze | POST /prefs-set |

## notifs (8)

| Tool | Qué hace | Parámetros | Cómo se ejecuta |
|---|---|---|---|
| `notifs_count` 👁 | Cuántas notificaciones vivas hay. | — | GET /notifs/count |
| `open_notifications` | Abre la campana de notificaciones. | — | ui:click #btn-notif |
| `notif_clear_all` | Limpia todas las notificaciones. | — | ui:click #nf-clearall |
| `notif_dismiss` | Quita una notificación por id. | id | ui:call nfDismiss |
| `notif_pin` | Guarda (fija) una notificación. | id | ui:call nfPin |
| `notif_unpin` | Quita una notificación guardada. | id | ui:call nfUnpin |
| `notif_snooze_1h` | Recordar una notificación en 1 hora. | id | ui:call nfSnooze |
| `show_timeline` | Muestra/oculta la actividad reciente. | on | ui:call setTimeline |

## remote (14)

| Tool | Qué hace | Parámetros | Cómo se ejecuta |
|---|---|---|---|
| `remote_state` 👁 | Estado del acceso remoto (Tailscale) y URLs. | — | GET /remote-state |
| `remote_on` | Prende el tablero remoto en el tailnet. | — | POST /remote-on |
| `remote_off` ⚠ confirm | Apaga el tablero remoto. | confirm | POST /remote-off |
| `webterm_on` | Prende el terminal web remoto. | — | POST /remote-webterm-on |
| `webterm_off` ⚠ confirm | Apaga el terminal web remoto. | confirm | POST /remote-webterm-off |
| `open_remote_panel` | Abre el panel Remoto (QR y URLs). | — | ui:click #btn-remote |
| `ssh_list` 👁 | Servidores de ~/.ssh/config. | — | GET /ssh |
| `ssh_add` | Añade un servidor SSH. | host, hostname, user, port, identity | POST /ssh-add |
| `ssh_update` | Edita/renombra un servidor SSH. | orig, host, hostname, user, port, identity | POST /ssh-update |
| `ssh_delete` ⚠ confirm | Borra un servidor SSH. | host, confirm | POST /ssh-del |
| `ssh_connect` | Conecta a un servidor en la sesión actual. | host | POST /ssh-connect |
| `ssh_new_tab` | Abre una pestaña nueva conectada a un servidor. | host | POST /ssh-new-tab |
| `ssh_key_setup` | Instala tu llave en el servidor (acceso sin contraseña). | host | POST /ssh-key-setup |
| `open_servers_panel` | Abre el gestor de servidores SSH. | — | ui:click #ssh-manage |

## snippets (6)

| Tool | Qué hace | Parámetros | Cómo se ejecuta |
|---|---|---|---|
| `snippets_list` 👁 | Lista los snippets guardados. | — | GET /snippets |
| `snippet_create` | Crea un snippet. | name, body, tags | POST /snippets |
| `snippet_update` | Edita un snippet. | id, name, body, tags | POST /snippets/update |
| `snippet_delete` ⚠ confirm | Borra un snippet. | id, confirm | POST /snippets/delete |
| `snippet_send` | Pega un snippet en una sesión (sin ejecutar). | id, session | local:snippet_send |
| `open_snippets` | Abre el panel de snippets. | — | ui:click #btn-snippets |

## fs (4)

| Tool | Qué hace | Parámetros | Cómo se ejecuta |
|---|---|---|---|
| `fs_list_dirs` 👁 | Lista subcarpetas de una ruta (selector de carpeta). | path | GET /fs/dirs |
| `fs_mkdir` | Crea una carpeta. | path | POST /fs/mkdir |
| `open_path` | Abre una ruta local con la app por defecto. | path | POST /open-path |
| `open_url` | Abre una URL http(s) en el navegador del sistema. | url | POST /open-url |

## news (4)

| Tool | Qué hace | Parámetros | Cómo se ejecuta |
|---|---|---|---|
| `news_latest` 👁 | Últimas noticias de IA vigiladas. | — | GET /news/latest |
| `models_latest` 👁 | Modelos nuevos detectados por el vigilante. | — | GET /models/latest |
| `news_refresh` | Fuerza un ciclo del vigilante de noticias. | — | POST /news/refresh |
| `models_refresh` | Fuerza un ciclo del vigilante de modelos. | — | POST /models/refresh |

## nav (16)

| Tool | Qué hace | Parámetros | Cómo se ejecuta |
|---|---|---|---|
| `open_panel` | Abre un panel del tablero: analytics (con tab), sov, switcher, timeline, wizard, centro. | panel, tab | local:ui_panel |
| `close_panels` | Cierra todos los paneles/modales abiertos. | — | ui:call closeAllPanels |
| `show_view` | Muestra el panel o una terminal: 'panel' o 'term:<sesión>'. | view | ui:call showView |
| `switcher_search` | Abre el conmutador con un texto de búsqueda. | query | ui:call swOpenWith |
| `wizard_prefill` | Abre el wizard de nueva sesión precargado. | cwd, harness, motor, model, effort, danger | ui:call nsOpenPrefilled |
| `select_session_card` | Selecciona la fila de una sesión en el panel (centro de control). | session | ui:call selectSessionCard |
| `expand_reply` | Expande/colapsa la respuesta de una sesión en el panel. | session, on | ui:call expandReply |
| `toggle_ssh_chips` | Expande/colapsa la fila de servidores. | — | ui:click #ssh-toggle |
| `open_analytics_tab` | Abre Analytics en una tab concreta. | tab | ui:call openAnalyticsTab |
| `compare_set_window` | Ventana de días del comparador. | days | ui:call compareSetDays |
| `set_split_left` | Ancho del panel izquierdo en layout ancho (px). | px | ui:call setSplitLeft |
| `set_chat_height` | Alto del chat del operador (px). | px | ui:call setOpChatHeight |
| `copy_text` | Copia un texto al portapapeles del navegador. | text | ui:call copyText |
| `reload_dashboard` | Recarga el tablero. | — | ui:call reloadDashboard |
| `dashboard_toast` | Muestra un aviso breve en el tablero. | text, error | ui:call toast |
| `close_tab_modal` | Abre el modal de cerrar pestaña de una terminal remota. | session | ui:call askCloseTab |

## term (5)

| Tool | Qué hace | Parámetros | Cómo se ejecuta |
|---|---|---|---|
| `term_key` | Pulsa una tecla de la toolbar del terminal web: escape, enter, tab, left, up, down, right. | key, session | ui:term toolbar |
| `term_paste` | Pega texto en el terminal web visible. | text, session | ui:term paste |
| `term_selection_mode` | Modo seleccionar texto (true) o interactuar (false) en el terminal web. | selecting, session | ui:term mode |
| `term_ctrl_arm` | Arma Ctrl para la siguiente tecla en el terminal web. | session | ui:term ctrl |
| `term_focus` | Da foco al terminal web visible (abre teclado en móvil). | session | ui:call focusVisibleTerm |

## app (30)

| Tool | Qué hace | Parámetros | Cómo se ejecuta |
|---|---|---|---|
| `app_split` | Split del pane actual en la app: right, left, down, up. | side, session | app:split |
| `app_split_ssh` | Split con otra conexión SSH al mismo servidor. | side, session | app:split_ssh |
| `app_kill_pane` ⚠ confirm | Cierra ESTE split en la app. | session, confirm | app:kill_pane |
| `app_toggle_window` | Alterna ventana 1/2 (Ctrl-b l) en la app. | session | app:toggle_window |
| `app_select_pane` | Selecciona un pane por id en la app. | pane | app:select_pane |
| `app_next_tab` | Pestaña siguiente (Ctrl+PgDn). | — | app:next_tab |
| `app_prev_tab` | Pestaña anterior (Ctrl+PgUp). | — | app:prev_tab |
| `app_mru_toggle` | Alterna con la última pestaña usada (Ctrl+Tab). | — | app:mru_toggle |
| `app_focus_page` | Enfoca una pestaña GTK ya abierta sin re-attachear. | session | app:focus_page |
| `app_tab_reorder` | Mueve una pestaña a una posición (0 = primera). | session, index | app:tab_reorder |
| `app_mosaic` | Mosaico de todas las sesiones: on, off o toggle (Ctrl+G). | state | app:mosaic |
| `app_mosaic_zoom` | En mosaico, enfoca en grande una sesión. | session | app:mosaic_zoom |
| `app_side_panel` | Muestra/oculta el tablero lateral (en mosaico). | on | app:side_panel |
| `app_terminals_visible` | Muestra/oculta las terminales (F12). | on | app:terminals_visible |
| `app_reload_dashboard` | Recarga el tablero embebido (F5). | — | app:reload_dashboard |
| `app_font_scale` | Zoom de fuente del terminal: +0.1, -0.1 o reset (0). | delta | app:font_scale |
| `app_open_switcher` | Abre el conmutador nativo (Ctrl+K). | — | app:open_switcher |
| `app_tabs_overview` | Abre la vista de todas las pestañas. | — | app:tabs_overview |
| `app_help` | Abre la ayuda de atajos (F1). | — | app:help |
| `app_snippets` | Abre el panel nativo de snippets (Ctrl+Shift+K). | — | app:snippets |
| `app_new_local_tab` | Nueva terminal VTE aquí (Ctrl+T). | — | app:new_local_tab |
| `app_open_xterm_tab` | Abre una sesión en el terminal xterm.js (Ctrl+Shift+T). | session | app:open_xterm_tab |
| `app_open_wizard` | Abre el wizard de nueva sesión desde la app (+). | — | app:open_wizard |
| `app_start_ai_here` | Inicia IA en el pane actual (Ctrl+Shift+A). | session, pane | app:start_ai_here |
| `app_copy_selection` | Copia la selección del terminal al portapapeles del sistema. | — | app:copy_selection |
| `app_paste_clipboard` | Pega el portapapeles del sistema en el terminal (Ctrl+V). | — | app:paste_clipboard |
| `app_copy_reply` | Copia la última respuesta de la IA de la pestaña actual. | — | app:copy_reply |
| `app_window` | Ventana de la app: minimize, maximize, restore, raise. | action | app:window |
| `app_side_dashboard_width` | Posición del divisor tablero/terminales (px). | px | app:paned_position |
| `app_quit` ⚠ confirm | Cierra la app de escritorio (Ctrl+Q). | confirm | app:quit |

## chat (7)

| Tool | Qué hace | Parámetros | Cómo se ejecuta |
|---|---|---|---|
| `remember` | Guarda un hecho en la memoria del operador. | fact | local:remember |
| `memory_read` 👁 | Lee la memoria completa del operador. | — | local:memory_read |
| `memory_replace` ⚠ confirm | Reescribe la memoria completa (para borrar o corregir). | text, confirm | local:memory_replace |
| `new_conversation` | Empieza una conversación nueva del operador. | — | ui:click #op-new |
| `set_chat_model` | Modelo del chat: haiku, gpt-5.3-codex-spark, grok-4.5. | model | POST /operator/model |
| `copy_last_reply` | Copia la última respuesta del chat. | — | local:copy_reply |
| `copy_session_reply` | Copia la última respuesta de la IA de una sesión. | tab | local:copy_session |


## Perfiles y operaciones recuperables

| Tool | Qué hace | Cómo se ejecuta |
|---|---|---|
| `configure_session` | Un cambio de CLI, motor, modelo y cuentas por panel; requiere verificar resultado. | POST /session/configure |
| `list_session_profiles` | Perfiles y capacidades de extensiones para el panel seleccionado; admite otro harness/account como destino del perfil. | GET /state y GET /session-profiles |
| `extension_usage` | Llamadas observadas del panel; scope=session incluye sus paneles y scope=all todas las sesiones. | GET /extension-usage |
| `show_chat` | Oculta o muestra el chat conservando el borrador. | UI setChatVisible |
| `open_session_profiles` | Abre el editor de perfiles de inicio. | UI openSessionProfiles |
| `operator_action_results` | Consulta acciones enviadas, confirmadas y fallidas. | GET /operator/action-results |
