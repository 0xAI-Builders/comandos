# lib/operator_catalog.py
"""Catálogo ÚNICO de acciones de ComandOS accesibles desde el operador.

Cada control de la UX (tablero remoto, terminal web, app GTK) tiene aquí un
ToolSpec. Los esquemas para Anthropic/OpenAI se generan de esta lista. Un
test cruza cada target con el código fuente para que no haya deriva.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

GROUPS = ("sessions", "panes", "models", "usage", "pomodoro", "prefs", "notifs",
          "remote", "snippets", "fs", "news", "nav", "term", "app", "chat")


@dataclass(frozen=True)
class ToolSpec:
    name: str
    group: str
    description: str
    params: dict = field(default_factory=dict)
    required: tuple[str, ...] = ()
    target: dict = field(default_factory=dict)
    destructive: bool = False
    readonly: bool = False


def P(**kw: Any) -> dict:
    out = {}
    for k, v in kw.items():
        if isinstance(v, str):
            out[k] = {"type": "string", "description": v}
        elif isinstance(v, tuple) and len(v) == 2:
            out[k] = {"type": v[0], "description": v[1]}
        elif isinstance(v, tuple) and len(v) == 3:
            out[k] = {"type": v[0], "description": v[1], "enum": list(v[2])}
        else:
            out[k] = dict(v)
    return out


CONFIRM = {"confirm": {"type": "boolean",
                       "description": "Debe ser true; pide confirmación al usuario en el chat antes de llamar."}}
TAB = "Nombre o etiqueta de la pestaña/sesión (se resuelve difuso)."
PANE = "Id de pane tmux (%N). Opcional: por defecto el pane vivo de la sesión."
ARR = lambda desc: {"type": "array", "items": {"type": "string"}, "description": desc}


def api(method, path, body=None, query=None):
    return {"kind": "api", "method": method, "path": path, "body": body or {}, "query": query or {}}


def ui_click(selector):
    return {"kind": "ui", "op": "click", "selector": selector}


def ui_call(fn, *args):
    return {"kind": "ui", "op": "call", "fn": fn, "args": list(args)}


def ui_term(type_):
    return {"kind": "ui", "op": "term", "type": type_}


def app(command):
    return {"kind": "app", "command": command}


def local(intent):
    return {"kind": "local", "intent": intent}


def T(name, group, description, params=None, required=(), target=None, destructive=False, readonly=False):
    params = dict(params or {})
    required = tuple(required)
    if destructive:
        params.update(CONFIRM)
        required = tuple(dict.fromkeys(required + ("confirm",)))
    return ToolSpec(name, group, description, params, required, target or {}, destructive, readonly)


EFFORT = ("string", "Esfuerzo", ("low", "medium", "high", "xhigh", "max", "ultra"))

CATALOG: list[ToolSpec] = [
    T("configure_session", "models", "Solicita un único cambio recuperable de CLI, motor, modelo, esfuerzo y cuentas en el panel exacto. Consulta model_switch_status para confirmar.", P(session="Sesión", pane=PANE, toHarness="CLI destino", motor="Motor destino", model="Modelo", effort=EFFORT, harnessAccount="Cuenta del CLI", motorAccount="Cuenta del motor", interrupt=("boolean", "Interrumpir el turno"), requestId="Identificador único para reintentos"), ("session", "pane", "toHarness"), api("POST", "/session/configure", {"session":"$session", "pane":"$pane", "toHarness":"$toHarness", "motor":"$motor", "model":"$model", "effort":"$effort", "harnessAccount":"$harnessAccount", "motorAccount":"$motorAccount", "interrupt":"$interrupt", "requestId":"$requestId"})),
    T("list_session_profiles", "sessions", "Lista perfiles de inicio y capacidades de skills/MCPs para el CLI y proyecto.", P(cwd="Carpeta", harness="CLI", account="Cuenta"), (), api("GET", "/session-profiles", query={"cwd":"$cwd", "harness":"$harness", "account":"$account"}), readonly=True),
    T("extension_usage", "usage", "Uso observado de skills y MCPs; no atribuye tokens o costes cuando faltan datos.", P(session="Sesión opcional", pane=PANE, days=("integer", "Días, máximo 90")), (), api("GET", "/extension-usage", query={"session":"$session", "pane":"$pane", "days":"$days"}), readonly=True),
    T("show_chat", "chat", "Muestra u oculta el chat conservando su borrador e historial.", P(visible=("boolean", "Mostrar chat")), ("visible",), ui_call("setChatVisible", "$visible")),
    T("open_session_profiles", "sessions", "Abre el editor de perfiles de inicio; las preferencias se aplican a sesiones nuevas.", target=ui_call("openSessionProfiles")),
    T("operator_action_results", "chat", "Consulta el registro durable de acciones del chat, incluidas acciones enviadas y fallos.", target=api("GET", "/operator/action-results"), readonly=True),
    # ───────────── sessions / tabs ─────────────
    T("list_tabs", "sessions", "Lista las pestañas abiertas (mismas que el escritorio).", target=api("GET", "/tabs"), readonly=True),
    T("list_sessions", "sessions", "Estado de todas las sesiones/agentes: status, proyecto, último mensaje.", target=api("GET", "/state"), readonly=True),
    T("get_active_tab", "sessions", "Qué pestaña está activa en la app y su pane vivo.", target=api("GET", "/active-tab"), readonly=True),
    T("session_brain", "sessions", "MCPs, skills, cuentas y CLAUDE.md de un proyecto.", P(cwd="Carpeta absoluta del proyecto", pane=PANE, harness="claude|codex|grok|opencode"), ("cwd",), api("GET", "/session-brain", query={"cwd": "$cwd", "pane": "$pane", "harness": "$harness"}), readonly=True),
    T("events_log", "sessions", "Últimos 80 eventos de actividad (working/waiting/done).", target=api("GET", "/events"), readonly=True),
    T("dedication_stats", "sessions", "Tiempo dedicado por proyecto (hoy y semana).", target=api("GET", "/dedication"), readonly=True),
    T("tab_history", "sessions", "Pestañas cerradas recientemente (recuperables).", target=api("GET", "/tab-history"), readonly=True),
    T("focus_tab", "sessions", "Trae al frente una pestaña/sesión.", P(tab=TAB), ("tab",), local("focus")),
    T("next_tab", "sessions", "Pasa a la pestaña siguiente.", target=local("step_next")),
    T("prev_tab", "sessions", "Pasa a la pestaña anterior.", target=local("step_prev")),
    T("close_tab", "sessions", "Cierra la pestaña (la sesión tmux sigue viva).", P(tab=TAB), ("tab",), local("close")),
    T("rename_tab", "sessions", "Renombra una pestaña.", P(tab=TAB, name="Nuevo nombre"), ("tab", "name"), local("rename")),
    T("send_tab_back", "sessions", "Manda la pestaña al final de la barra.", P(tab=TAB), ("tab",), local("send_back")),
    T("new_terminal_tab", "sessions", "Nueva terminal (shell) como pestaña en ambos lados.", P(label="Etiqueta opcional"), (), api("POST", "/tab-new", {"label": "$label"})),
    T("recover_tab", "sessions", "Recupera una pestaña cerrada por nombre.", P(name="Nombre de la pestaña cerrada"), ("name",), local("recover")),
    T("new_ai_session", "sessions", "Crea una sesión de IA nueva en una carpeta (wizard +): harness, motor, modelo, esfuerzo, cuenta, modo sin aprobaciones.", P(cwd="Carpeta absoluta", agent=("string", "Harness", ("claude", "codex", "grok", "opencode", "agy", "acp")), model="Modelo", effort=EFFORT, routeId="Ruta de la matriz de capacidades", harnessAccount="Alias de cuenta del harness", motorAccount="Alias de cuenta del motor", danger=("boolean", "Sin aprobaciones (dangerously)")), ("cwd",), api("POST", "/session-new", {"cwd": "$cwd", "agent": "$agent", "model": "$model", "effort": "$effort", "routeId": "$routeId", "harnessAccount": "$harnessAccount", "motorAccount": "$motorAccount", "danger": "$danger"})),
    T("open_project", "sessions", "Abre/crea la sesión de un proyecto por nombre (busca en ~/codebase).", P(session="Nombre del proyecto", agent="Harness opcional"), ("session",), api("POST", "/new", {"session": "$session", "agent": "$agent"})),
    T("open_with_account", "sessions", "Nueva sesión Claude en una carpeta con una cuenta concreta.", P(cwd="Carpeta", account="Alias de cuenta", danger=("boolean", "Sin aprobaciones")), ("cwd", "account"), api("POST", "/open-with-account", {"cwd": "$cwd", "account": "$account", "danger": "$danger"})),
    T("session_open", "sessions", "Revive si hace falta y enfoca la ventana de una sesión.", P(session="Sesión tmux", cwd="Carpeta", agent="Harness"), ("session",), api("POST", "/up", {"session": "$session", "cwd": "$cwd", "agent": "$agent"})),
    T("session_ensure", "sessions", "Asegura que la sesión y su ventana existen, sin robar el foco.", P(session="Sesión", cwd="Carpeta", win=("string", "Ventana", ("claude", "shell")), agent="Harness"), ("session",), api("POST", "/ensure", {"session": "$session", "cwd": "$cwd", "win": "$win", "agent": "$agent"})),
    T("session_shell", "sessions", "Abre/enfoca la ventana shell de la sesión.", P(session="Sesión", cwd="Carpeta"), ("session",), api("POST", "/shell", {"session": "$session", "cwd": "$cwd"})),
    T("toggle_shell", "sessions", "Alterna entre la ventana de la IA y el shell (Ctrl-b l).", target=local("toggle_shell")),
    T("kill_session", "sessions", "MATA la sesión tmux (irreversible).", P(tab=TAB), ("tab",), local("kill"), destructive=True),
    T("export_reply", "sessions", "Exporta la última respuesta de la IA a txt o pdf y la abre.", P(session="Sesión", format=("string", "Formato", ("txt", "pdf"))), ("session", "format"), api("POST", "/export", {"session": "$session", "format": "$format"})),
    T("favorite_toggle", "sessions", "Marca/desmarca una sesión como favorita (★).", P(session="Sesión", on=("boolean", "true=favorita")), ("session", "on"), ui_call("opFavorite", "$session", "$on")),
    # ───────────── panes / input ─────────────
    T("send_text", "panes", "Escribe texto en el pane y pulsa Enter (responder a la IA).", P(tab=TAB, text="Texto a enviar", pane=PANE), ("tab", "text"), local("send")),
    T("paste_text", "panes", "Pega texto (bracketed paste) SIN Enter.", P(session="Sesión", text="Texto", pane=PANE), ("session", "text"), api("POST", "/paste", {"session": "$session", "text": "$text", "pane": "$pane"})),
    T("send_key", "panes", "Envía una tecla: Enter, Escape, Up, Down, Tab, y, n, 1-9.", P(key="Tecla", tab=TAB, pane=PANE), ("key",), local("key")),
    T("choose_option", "panes", "Elige la opción N de un menú numerado de la IA (manda el número y Enter).", P(tab=TAB, option=("integer", "1-9")), ("tab", "option"), local("choose_option")),
    T("interrupt_turn", "panes", "Para el turno actual de la IA (cancela cambio en cola y manda Escape).", P(tab=TAB, pane=PANE), ("tab",), local("interrupt")),
    T("pause_agent", "panes", "Pausa (SIGSTOP) el proceso de la IA.", P(tab=TAB), ("tab",), local("pause_on")),
    T("resume_agent", "panes", "Reanuda (SIGCONT) el proceso de la IA.", P(tab=TAB), ("tab",), local("pause_off")),
    T("split_pane", "panes", "Divide el pane: derecha, izquierda, abajo o arriba.", P(side=("string", "Lado", ("derecha", "izquierda", "abajo", "arriba")), tab=TAB), ("side",), local("split")),
    T("close_split", "panes", "Cierra el split activo (kill-pane).", P(tab=TAB), (), local("close_split"), destructive=True),
    T("tmux_mouse_get", "panes", "Lee si el modo ratón de tmux está activo en la sesión.", P(session="Sesión"), ("session",), api("GET", "/tmux-mouse", query={"session": "$session"}), readonly=True),
    T("tmux_mouse_set", "panes", "Activa/desactiva el modo ratón (seleccionar texto vs interactuar).", P(session="Sesión", enabled=("boolean", "true=interactuar")), ("session", "enabled"), api("POST", "/tmux-mouse", {"session": "$session", "enabled": "$enabled"})),
    T("tmux_scroll", "panes", "Desplaza el historial del pane N líneas (negativo=arriba).", P(session="Sesión", delta=("integer", "Líneas")), ("session", "delta"), api("POST", "/tmux-scroll", {"session": "$session", "delta": "$delta"})),
    # ───────────── models / harness / accounts ─────────────
    T("proxy_state", "models", "Estado del gateway: motor y modelo global, cuentas, logins.", target=api("GET", "/proxy"), readonly=True),
    T("list_providers", "models", "Registro de proveedores, harnesses, motores y rutas disponibles.", target=api("GET", "/providers"), readonly=True),
    T("list_model_tiers", "models", "Tabla de tiers de modelos (barato/rápido/potente).", target=api("GET", "/model-tiers"), readonly=True),
    T("list_opencode_models", "models", "Catálogo de modelos de OpenCode.", target=api("GET", "/opencode/models"), readonly=True),
    T("sovereignty_report", "models", "Reporte de soberanía (qué corre local vs nube).", target=api("GET", "/sovereignty"), readonly=True),
    T("model_switch", "models", "Cambia modelo/motor/esfuerzo del pane de una sesión en vivo.", P(session="Sesión", pane=PANE, routeId="Ruta de la matriz", provider="claude|codex|grok|opencode", model="Modelo", effort=EFFORT, motor="Motor", harnessAccount="Cuenta harness", motorAccount="Cuenta motor", interrupt=("boolean", "Detener y cambiar ya")), ("session",), api("POST", "/model/switch", {"session": "$session", "pane": "$pane", "routeId": "$routeId", "provider": "$provider", "model": "$model", "effort": "$effort", "motor": "$motor", "harnessAccount": "$harnessAccount", "motorAccount": "$motorAccount", "interrupt": "$interrupt"})),
    T("model_switch_cancel", "models", "Cancela un cambio de modelo en cola.", P(session="Sesión", pane=PANE), ("session",), api("POST", "/model/switch-cancel", {"session": "$session", "pane": "$pane"})),
    T("model_switch_status", "models", "Progreso de un cambio de modelo/motor en curso.", P(operationKey="Clave devuelta por model_switch"), ("operationKey",), api("GET", "/model/status", query={"operationKey": "$operationKey"}), readonly=True),
    T("harness_switch", "models", "Cambia el CLI (Claude Code, Codex, Grok Build, OpenCode, ACP) de un pane con traspaso de contexto.", P(session="Sesión", pane=PANE, toHarness="Harness destino", model="Modelo", effort=EFFORT, account="Cuenta", motor="Motor", danger=("boolean", "Sin aprobaciones"), interrupt=("boolean", "Detener y cambiar ya")), ("session", "toHarness"), api("POST", "/harness/switch", {"session": "$session", "pane": "$pane", "toHarness": "$toHarness", "model": "$model", "effort": "$effort", "account": "$account", "motor": "$motor", "danger": "$danger", "interrupt": "$interrupt"})),
    T("set_global_motor", "models", "Fija motor y modelo GLOBAL del gateway.", P(motor=("string", "Motor", ("claude", "codex", "grok")), model="Modelo"), ("motor",), api("POST", "/proxy", {"motor": "$motor", "model": "$model"})),
    T("gateway_enable", "models", "Enciende/apaga el gateway (proxy) de modelos.", P(enable=("boolean", "true=encender")), ("enable",), api("POST", "/proxy", {"enable": "$enable"})),
    T("account_add", "models", "Añade una cuenta (alias) de un proveedor y abre su login.", P(provider=("string", "Proveedor", ("claude", "codex", "grok")), alias="Alias", cwd="Carpeta"), ("provider", "alias"), api("POST", "/account/add", {"provider": "$provider", "alias": "$alias", "cwd": "$cwd"})),
    T("account_switch", "models", "Mueve la conversación viva a otra cuenta Claude.", P(session="Sesión", pane=PANE, alias="Alias", interrupt=("boolean", "Detener y cambiar ya")), ("session", "alias"), api("POST", "/account/switch", {"session": "$session", "pane": "$pane", "alias": "$alias", "interrupt": "$interrupt"})),
    T("skill_toggle", "models", "Activa/desactiva una skill (aplica al reciclar la sesión).", P(path="Ruta de la skill", on=("boolean", "true=activar")), ("path", "on"), api("POST", "/skill-toggle", {"path": "$path", "on": "$on"})),
    T("mcp_toggle", "models", "Activa/desactiva un MCP del proyecto.", P(cwd="Carpeta", name="Nombre del MCP", on=("boolean", "true=activar")), ("cwd", "name", "on"), api("POST", "/mcp-toggle", {"cwd": "$cwd", "name": "$name", "on": "$on"})),
    T("optimization_plans", "models", "Perfiles de optimización (ahorro, equilibrio, potencia).", target=api("GET", "/optimization/plans"), readonly=True),
    T("optimization_set_default", "models", "Perfil de optimización por defecto.", P(profile="Perfil"), ("profile",), api("POST", "/optimization/default", {"profile": "$profile"})),
    T("optimization_apply", "models", "Aplica un perfil a varias sesiones (un model_switch por sesión).", P(profile="Perfil", sessions=ARR("Sesiones")), ("profile", "sessions"), local("optimization_apply")),
    T("undo_guard_switch", "models", "Deshace el último cambio de modelo hecho por la guardia (vuelve al anterior).", P(session="Sesión", model="Modelo previo"), ("session", "model"), api("POST", "/model/switch", {"session": "$session", "model": "$model"})),
    T("open_motor_picker", "models", "Abre el selector de motor/modelo/cuenta de un pane en el tablero.", P(session="Sesión", pane=PANE), ("session",), ui_call("openMotorFor", "$session", "$pane")),
    T("open_global_motor_picker", "models", "Abre el selector de motor GLOBAL.", target=ui_click("#motor-global")),
    # ───────────── usage / guard / analytics ─────────────
    T("usage_state", "usage", "Uso y cuotas por proveedor/cuenta, alertas, salud de credenciales.", target=api("GET", "/usage/state"), readonly=True),
    T("usage_guard", "usage", "Guardia anti-desborde con pronóstico por proyecto.", target=api("GET", "/usage/guard"), readonly=True),
    T("usage_changes", "usage", "Ledger de cambios de modelo hechos por la guardia.", target=api("GET", "/usage/changes"), readonly=True),
    T("usage_provider_compare", "usage", "Comparativa de costo/uso entre proveedores.", P(days=("integer", "Ventana en días (14)")), (), api("GET", "/usage/provider-compare", query={"days": "$days"}), readonly=True),
    T("usage_analytics", "usage", "Analytics de experimentos A/B por tipo de tarea.", P(days=("integer", "Días"), taskType="Tipo de tarea"), (), api("GET", "/usage/analytics", query={"days": "$days", "taskType": "$taskType"}), readonly=True),
    T("usage_interactions", "usage", "Últimas interacciones registradas.", P(limit=("integer", "Máximo (20)")), (), api("GET", "/usage/interactions", query={"limit": "$limit"}), readonly=True),
    T("usage_experiments", "usage", "Lista de experimentos A/B.", target=api("GET", "/usage/experiments"), readonly=True),
    T("usage_experiment_create", "usage", "Crea un experimento A/B.", P(label="Nombre", taskType="Tipo de tarea", variants=ARR("Variantes"), minPairs=("integer", "Pares mínimos")), ("label", "taskType", "variants"), api("POST", "/usage/experiment", {"action": "create", "label": "$label", "taskType": "$taskType", "variants": "$variants", "minPairs": "$minPairs"})),
    T("usage_experiment_pair", "usage", "Registra un par de comparación en un experimento.", P(experimentId="Id", projectId="Proyecto"), ("experimentId",), api("POST", "/usage/experiment", {"action": "pair", "experimentId": "$experimentId", "projectId": "$projectId"})),
    T("usage_rate", "usage", "Califica una interacción: Mal, Parcial o Resuelto.", P(interactionId="Id", outcome=("string", "Resultado", ("bad", "partial", "solved")), rating=("integer", "1-5"), note="Nota", taskType="Tipo"), ("interactionId", "outcome"), api("POST", "/usage/rating", {"interactionId": "$interactionId", "outcome": "$outcome", "rating": "$rating", "note": "$note", "taskType": "$taskType"})),
    T("usage_refresh", "usage", "Vuelve a bajar uso y costos de todos los proveedores.", target=api("POST", "/usage/refresh")),
    T("usage_set_quota", "usage", "Declara la cuota de 7 días de un proveedor (0 la borra).", P(provider="Proveedor", tokens7d=("integer", "Tokens/7d")), ("provider", "tokens7d"), api("POST", "/usage/quota", {"provider": "$provider", "tokens7d": "$tokens7d"})),
    T("usage_set_subscription", "usage", "Declara el costo mensual/moneda de un proveedor para el ROI.", P(provider="Proveedor", monthly=("number", "Costo mensual"), currency="Moneda", display="Moneda de display"), ("provider",), api("POST", "/usage/subscription", {"provider": "$provider", "monthly": "$monthly", "currency": "$currency", "display": "$display"})),
    T("usage_alert_rule_set", "usage", "Crea/actualiza una alerta de presupuesto (proyecto, pane o proveedor).", P(id="Id opcional", scope=("string", "Ámbito", ("project", "pane", "provider")), target="Objetivo", label="Etiqueta", threshold=("number", "Umbral")), ("scope", "target", "threshold"), api("POST", "/usage/alert-rule", {"action": "set", "id": "$id", "scope": "$scope", "target": "$target", "label": "$label", "threshold": "$threshold"})),
    T("usage_alert_rule_delete", "usage", "Borra una alerta de presupuesto.", P(id="Id de la regla"), ("id",), api("POST", "/usage/alert-rule", {"action": "delete", "id": "$id"})),
    T("usage_set_thresholds", "usage", "Umbrales globales de alerta (p. ej. 70,85,95).", P(thresholds={"type": "array", "items": {"type": "integer"}, "description": "Porcentajes"}), ("thresholds",), api("POST", "/usage/settings", {"settings": {"COMANDOS_ALERT_THRESHOLDS": "$thresholds"}})),
    T("usage_settings_set", "usage", "Escribe ajustes de uso arbitrarios.", P(settings={"type": "object", "description": "Ajustes"}), ("settings",), api("POST", "/usage/settings", {"settings": "$settings"})),
    T("ui_log_summary", "usage", "Resumen de telemetría de la UI.", target=api("GET", "/ui-log/summary"), readonly=True),
    # ───────────── pomodoro ─────────────
    T("pomodoro_state", "pomodoro", "Bloque de foco actual, cola y analytics de 7 días.", target=api("GET", "/pomodoro"), readonly=True),
    T("pomodoro_start", "pomodoro", "Inicia un bloque de foco/descanso.", P(mins=("integer", "Minutos"), mode=("string", "Modo", ("focus", "break")), project="Proyecto", session="Sesión", cycleIndex=("integer", "Ciclo"), cycleTotal=("integer", "Total")), ("mins", "mode"), api("POST", "/pomodoro", {"mins": "$mins", "mode": "$mode", "project": "$project", "session": "$session", "cycleIndex": "$cycleIndex", "cycleTotal": "$cycleTotal"})),
    T("pomodoro_stop", "pomodoro", "Termina/salta el bloque actual.", P(status=("string", "Estado", ("done", "skipped", "cancelled"))), ("status",), api("POST", "/pomodoro", {"stop": True, "status": "$status"})),
    T("pomodoro_ack", "pomodoro", "Vacía la cola de notificaciones del pomodoro.", target=api("POST", "/pomodoro", {"ack": True})),
    T("pomodoro_settings", "pomodoro", "Duración, descanso, ciclos y auto-descanso.", P(mins=("integer", "Foco"), breakMins=("integer", "Descanso"), cycles=("integer", "Ciclos"), auto=("boolean", "Auto-descanso")), (), api("POST", "/pomodoro", {"settings": {"mins": "$mins", "breakMins": "$breakMins", "cycles": "$cycles", "auto": "$auto"}})),
    T("pomodoro_extend", "pomodoro", "Añade 5 minutos al bloque en marcha.", target=ui_click("#pp-extend")),
    T("pomodoro_skip", "pomodoro", "Salta el bloque en marcha.", target=ui_click("#pp-skip")),
    T("open_pomodoro", "pomodoro", "Abre el panel de pomodoro.", target=ui_click("#btn-pomo")),
    # ───────────── prefs / settings ─────────────
    T("get_conf", "prefs", "Configuración (volumen, notificaciones, idioma, worktrees…).", target=api("GET", "/conf"), readonly=True),
    T("get_prefs", "prefs", "Preferencias (tema, fuente, cursor, favoritos) y fuentes instaladas.", target=api("GET", "/prefs"), readonly=True),
    T("set_pref", "prefs", "Switch de configuración on/off.", P(key=("string", "Clave", ("AUTO_WORKTREE", "NOTIFY_ON_DONE", "NOTIFY_ON_ATTENTION", "SOUND_ENABLED", "DESKTOP_NOTIFY", "TELEGRAM_ENABLED", "SPEAK_DONE", "SPEAK_ATTENTION")), on=("boolean", "true=on")), ("key", "on"), local("pref")),
    T("set_voice", "prefs", "Voz que anuncia el proyecto (SPEAK_DONE + SPEAK_ATTENTION).", P(on=("boolean", "true=on")), ("on",), local("voice")),
    T("set_volume", "prefs", "Volumen de voz y chime 0-100.", P(percent=("integer", "0-100")), ("percent",), api("POST", "/conf-set", {"key": "VOLUME", "value": "$percent"})),
    T("set_language", "prefs", "Idioma del tablero: auto, es, en.", P(lang=("string", "Idioma", ("auto", "es", "en"))), ("lang",), local("lang")),
    T("set_theme", "prefs", "Tema visual del tablero y terminales.", P(theme="Nombre del tema"), ("theme",), local("theme")),
    T("set_notify_corner", "prefs", "Esquina de los avisos: tl, tr, bl, br o free.", P(corner=("string", "Esquina", ("tl", "tr", "bl", "br", "free"))), ("corner",), local("notify_pos")),
    T("set_terminal_font", "prefs", "Familia y/o tamaño de fuente del terminal.", P(family="Familia instalada", size=("integer", "Tamaño px")), (), api("POST", "/prefs-set", {"font_family": "$family", "font_size": "$size"})),
    T("set_cursor", "prefs", "Forma y parpadeo del cursor.", P(shape=("string", "Forma", ("block", "bar", "underline")), blink=("boolean", "Parpadeo")), (), api("POST", "/prefs-set", {"cursor_shape": "$shape", "cursor_blink": "$blink"})),
    T("set_ligatures", "prefs", "Ligaduras tipográficas en el terminal.", P(on=("boolean", "true=on")), ("on",), api("POST", "/prefs-set", {"ligatures": "$on"})),
    T("set_terminal_padding", "prefs", "Margen interno del terminal.", P(px=("integer", "Píxeles")), ("px",), api("POST", "/prefs-set", {"terminal_padding": "$px"})),
    T("set_terminal_opacity", "prefs", "Opacidad del fondo del terminal 0-100.", P(percent=("integer", "0-100")), ("percent",), api("POST", "/prefs-set", {"terminal_opacity": "$percent"})),
    T("set_poll_seconds", "prefs", "Segundos de refresco del tablero remoto.", P(seconds=("integer", "Segundos")), ("seconds",), ui_call("setPollSeconds", "$seconds")),
    T("set_browser_notifications", "prefs", "Notificaciones del navegador on/off.", P(on=("boolean", "true=on")), ("on",), ui_call("setBrowserNotifications", "$on")),
    T("test_notification", "prefs", "Prueba un aviso: voz, chime o terminado.", P(kind=("string", "Tipo", ("voice", "chime", "done"))), ("kind",), api("POST", "/test", {"kind": "$kind"})),
    T("open_settings", "prefs", "Abre Ajustes en una tab: Apariencia, Notificaciones, Terminal o Tablero.", P(tab=("string", "Tab", ("appearance", "notif", "term", "dash"))), (), local("ui_settings")),
    T("set_limit_style", "prefs", "Estilo de la barra de límites en Analytics.", P(style="Estilo"), ("style",), ui_call("setLimitStyle", "$style")),
    T("notif_dismiss_ids", "prefs", "Marca como leídas notificaciones por id (persistente).", P(ids=ARR("Ids")), ("ids",), api("POST", "/prefs-set", {"nfDismiss": "$ids"})),
    T("notif_snooze_ids", "prefs", "Pospone notificaciones por id hasta una marca de tiempo.", P(snooze={"type": "object", "description": "{id: epoch_ms}"}), ("snooze",), api("POST", "/prefs-set", {"nfSnooze": "$snooze"})),
    # ───────────── notifications ─────────────
    T("notifs_count", "notifs", "Cuántas notificaciones vivas hay.", target=api("GET", "/notifs/count"), readonly=True),
    T("open_notifications", "notifs", "Abre la campana de notificaciones.", target=ui_click("#btn-notif")),
    T("notif_clear_all", "notifs", "Limpia todas las notificaciones.", target=ui_click("#nf-clearall")),
    T("notif_dismiss", "notifs", "Quita una notificación por id.", P(id="Id"), ("id",), ui_call("nfDismiss", "$id")),
    T("notif_pin", "notifs", "Guarda (fija) una notificación.", P(id="Id"), ("id",), ui_call("nfPin", "$id")),
    T("notif_unpin", "notifs", "Quita una notificación guardada.", P(id="Id"), ("id",), ui_call("nfUnpin", "$id")),
    T("notif_snooze_1h", "notifs", "Recordar una notificación en 1 hora.", P(id="Id"), ("id",), ui_call("nfSnooze", "$id")),
    T("show_timeline", "notifs", "Muestra/oculta la actividad reciente.", P(on=("boolean", "true=mostrar")), ("on",), ui_call("setTimeline", "$on")),
    # ───────────── remote / ssh ─────────────
    T("remote_state", "remote", "Estado del acceso remoto (Tailscale) y URLs.", target=api("GET", "/remote-state"), readonly=True),
    T("remote_on", "remote", "Prende el tablero remoto en el tailnet.", target=api("POST", "/remote-on")),
    T("remote_off", "remote", "Apaga el tablero remoto.", target=api("POST", "/remote-off"), destructive=True),
    T("webterm_on", "remote", "Prende el terminal web remoto.", target=api("POST", "/remote-webterm-on")),
    T("webterm_off", "remote", "Apaga el terminal web remoto.", target=api("POST", "/remote-webterm-off"), destructive=True),
    T("open_remote_panel", "remote", "Abre el panel Remoto (QR y URLs).", target=ui_click("#btn-remote")),
    T("ssh_list", "remote", "Servidores de ~/.ssh/config.", target=api("GET", "/ssh"), readonly=True),
    T("ssh_add", "remote", "Añade un servidor SSH.", P(host="Alias", hostname="Host/IP", user="Usuario", port=("integer", "Puerto"), identity="Ruta de llave"), ("host", "hostname"), api("POST", "/ssh-add", {"host": "$host", "hostname": "$hostname", "user": "$user", "port": "$port", "identity": "$identity"})),
    T("ssh_update", "remote", "Edita/renombra un servidor SSH.", P(orig="Alias actual", host="Alias nuevo", hostname="Host/IP", user="Usuario", port=("integer", "Puerto"), identity="Llave"), ("orig",), api("POST", "/ssh-update", {"orig": "$orig", "host": "$host", "hostname": "$hostname", "user": "$user", "port": "$port", "identity": "$identity"})),
    T("ssh_delete", "remote", "Borra un servidor SSH.", P(host="Alias"), ("host",), api("POST", "/ssh-del", {"host": "$host"}), destructive=True),
    T("ssh_connect", "remote", "Conecta a un servidor en la sesión actual.", P(host="Alias"), ("host",), api("POST", "/ssh-connect", {"host": "$host"})),
    T("ssh_new_tab", "remote", "Abre una pestaña nueva conectada a un servidor.", P(host="Alias"), ("host",), api("POST", "/ssh-new-tab", {"host": "$host"})),
    T("ssh_key_setup", "remote", "Instala tu llave en el servidor (acceso sin contraseña).", P(host="Alias"), ("host",), api("POST", "/ssh-key-setup", {"host": "$host"})),
    T("open_servers_panel", "remote", "Abre el gestor de servidores SSH.", target=ui_click("#ssh-manage")),
    # ───────────── snippets ─────────────
    T("snippets_list", "snippets", "Lista los snippets guardados.", target=api("GET", "/snippets"), readonly=True),
    T("snippet_create", "snippets", "Crea un snippet.", P(name="Nombre", body="Contenido", tags=ARR("Etiquetas")), ("name", "body"), api("POST", "/snippets", {"name": "$name", "body": "$body", "tags": "$tags"})),
    T("snippet_update", "snippets", "Edita un snippet.", P(id="Id", name="Nombre", body="Contenido", tags=ARR("Etiquetas")), ("id",), api("POST", "/snippets/update", {"id": "$id", "name": "$name", "body": "$body", "tags": "$tags"})),
    T("snippet_delete", "snippets", "Borra un snippet.", P(id="Id"), ("id",), api("POST", "/snippets/delete", {"id": "$id"}), destructive=True),
    T("snippet_send", "snippets", "Pega un snippet en una sesión (sin ejecutar).", P(id="Id del snippet", session="Sesión"), ("id", "session"), local("snippet_send")),
    T("open_snippets", "snippets", "Abre el panel de snippets.", target=ui_click("#btn-snippets")),
    # ───────────── fs / links ─────────────
    T("fs_list_dirs", "fs", "Lista subcarpetas de una ruta (selector de carpeta).", P(path="Ruta (~ por defecto)"), (), api("GET", "/fs/dirs", query={"path": "$path"}), readonly=True),
    T("fs_mkdir", "fs", "Crea una carpeta.", P(path="Ruta"), ("path",), api("POST", "/fs/mkdir", {"path": "$path"})),
    T("open_path", "fs", "Abre una ruta local con la app por defecto.", P(path="Ruta absoluta o ~"), ("path",), api("POST", "/open-path", {"path": "$path"})),
    T("open_url", "fs", "Abre una URL http(s) en el navegador del sistema.", P(url="URL"), ("url",), api("POST", "/open-url", {"url": "$url"})),
    # ───────────── news / model watch ─────────────
    T("news_latest", "news", "Últimas noticias de IA vigiladas.", target=api("GET", "/news/latest"), readonly=True),
    T("models_latest", "news", "Modelos nuevos detectados por el vigilante.", target=api("GET", "/models/latest"), readonly=True),
    T("news_refresh", "news", "Fuerza un ciclo del vigilante de noticias.", target=api("POST", "/news/refresh")),
    T("models_refresh", "news", "Fuerza un ciclo del vigilante de modelos.", target=api("POST", "/models/refresh")),
    # ───────────── nav (tablero) ─────────────
    T("open_panel", "nav", "Abre un panel del tablero: analytics (con tab), sov, switcher, timeline, wizard, centro.", P(panel=("string", "Panel", ("analytics", "sov", "switcher", "timeline", "wizard", "centro")), tab=("string", "Tab de analytics", ("resumen", "comparar", "optimizar"))), ("panel",), local("ui_panel")),
    T("close_panels", "nav", "Cierra todos los paneles/modales abiertos.", target=ui_call("closeAllPanels")),
    T("show_view", "nav", "Muestra el panel o una terminal: 'panel' o 'term:<sesión>'.", P(view="panel | term:<sesión>"), ("view",), ui_call("showView", "$view")),
    T("switcher_search", "nav", "Abre el conmutador con un texto de búsqueda.", P(query="Texto"), (), ui_call("swOpenWith", "$query")),
    T("wizard_prefill", "nav", "Abre el wizard de nueva sesión precargado.", P(cwd="Carpeta", harness="Harness", motor="Motor", model="Modelo", effort="Esfuerzo", danger=("boolean", "Sin aprobaciones")), (), ui_call("nsOpenPrefilled", "$cwd", "$harness", "$motor", "$model", "$effort", "$danger")),
    T("select_session_card", "nav", "Selecciona la fila de una sesión en el panel (centro de control).", P(session="Sesión"), ("session",), ui_call("selectSessionCard", "$session")),
    T("expand_reply", "nav", "Expande/colapsa la respuesta de una sesión en el panel.", P(session="Sesión", on=("boolean", "true=expandir")), ("session",), ui_call("expandReply", "$session", "$on")),
    T("toggle_ssh_chips", "nav", "Expande/colapsa la fila de servidores.", target=ui_click("#ssh-toggle")),
    T("open_analytics_tab", "nav", "Abre Analytics en una tab concreta.", P(tab=("string", "Tab", ("resumen", "comparar", "optimizar", "proveedores", "alertas", "guardia"))), ("tab",), ui_call("openAnalyticsTab", "$tab")),
    T("compare_set_window", "nav", "Ventana de días del comparador.", P(days=("integer", "Días")), ("days",), ui_call("compareSetDays", "$days")),
    T("set_split_left", "nav", "Ancho del panel izquierdo en layout ancho (px).", P(px=("integer", "Píxeles")), ("px",), ui_call("setSplitLeft", "$px")),
    T("set_chat_height", "nav", "Alto del chat del operador (px).", P(px=("integer", "Píxeles")), ("px",), ui_call("setOpChatHeight", "$px")),
    T("copy_text", "nav", "Copia un texto al portapapeles del navegador.", P(text="Texto"), ("text",), ui_call("copyText", "$text")),
    T("reload_dashboard", "nav", "Recarga el tablero.", target=ui_call("reloadDashboard")),
    T("dashboard_toast", "nav", "Muestra un aviso breve en el tablero.", P(text="Texto", error=("boolean", "Estilo error")), ("text",), ui_call("toast", "$text", "$error")),
    T("close_tab_modal", "nav", "Abre el modal de cerrar pestaña de una terminal remota.", P(session="Sesión"), ("session",), ui_call("askCloseTab", "$session")),
    # ───────────── term (toolbar del terminal web) ─────────────
    T("term_key", "term", "Pulsa una tecla de la toolbar del terminal web: escape, enter, tab, left, up, down, right.", P(key=("string", "Tecla", ("escape", "enter", "tab", "left", "up", "down", "right")), session="Sesión (terminal visible por defecto)"), ("key",), ui_term("toolbar")),
    T("term_paste", "term", "Pega texto en el terminal web visible.", P(text="Texto", session="Sesión"), ("text",), ui_term("paste")),
    T("term_selection_mode", "term", "Modo seleccionar texto (true) o interactuar (false) en el terminal web.", P(selecting=("boolean", "true=seleccionar"), session="Sesión"), ("selecting",), ui_term("mode")),
    T("term_ctrl_arm", "term", "Arma Ctrl para la siguiente tecla en el terminal web.", P(session="Sesión"), (), ui_term("ctrl")),
    T("term_focus", "term", "Da foco al terminal web visible (abre teclado en móvil).", P(session="Sesión"), (), ui_call("focusVisibleTerm")),
    # ───────────── app (escritorio GTK) ─────────────
    T("app_split", "app", "Split del pane actual en la app: right, left, down, up.", P(side=("string", "Lado", ("right", "left", "down", "up")), session="Sesión"), ("side",), app("split")),
    T("app_split_ssh", "app", "Split con otra conexión SSH al mismo servidor.", P(side=("string", "Lado", ("right", "left", "down", "up")), session="Sesión"), ("side",), app("split_ssh")),
    T("app_kill_pane", "app", "Cierra ESTE split en la app.", P(session="Sesión"), (), app("kill_pane"), destructive=True),
    T("app_toggle_window", "app", "Alterna ventana 1/2 (Ctrl-b l) en la app.", P(session="Sesión"), (), app("toggle_window")),
    T("app_select_pane", "app", "Selecciona un pane por id en la app.", P(pane="Id %N"), ("pane",), app("select_pane")),
    T("app_next_tab", "app", "Pestaña siguiente (Ctrl+PgDn).", target=app("next_tab")),
    T("app_prev_tab", "app", "Pestaña anterior (Ctrl+PgUp).", target=app("prev_tab")),
    T("app_mru_toggle", "app", "Alterna con la última pestaña usada (Ctrl+Tab).", target=app("mru_toggle")),
    T("app_focus_page", "app", "Enfoca una pestaña GTK ya abierta sin re-attachear.", P(session="Sesión"), ("session",), app("focus_page")),
    T("app_tab_reorder", "app", "Mueve una pestaña a una posición (0 = primera).", P(session="Sesión", index=("integer", "Posición")), ("session", "index"), app("tab_reorder")),
    T("app_mosaic", "app", "Mosaico de todas las sesiones: on, off o toggle (Ctrl+G).", P(state=("string", "Estado", ("on", "off", "toggle"))), ("state",), app("mosaic")),
    T("app_mosaic_zoom", "app", "En mosaico, enfoca en grande una sesión.", P(session="Sesión"), ("session",), app("mosaic_zoom")),
    T("app_side_panel", "app", "Muestra/oculta el tablero lateral (en mosaico).", P(on=("boolean", "true=mostrar")), ("on",), app("side_panel")),
    T("app_terminals_visible", "app", "Muestra/oculta las terminales (F12).", P(on=("boolean", "true=mostrar")), ("on",), app("terminals_visible")),
    T("app_reload_dashboard", "app", "Recarga el tablero embebido (F5).", target=app("reload_dashboard")),
    T("app_font_scale", "app", "Zoom de fuente del terminal: +0.1, -0.1 o reset (0).", P(delta=("number", "Delta; 0 = reset")), ("delta",), app("font_scale")),
    T("app_open_switcher", "app", "Abre el conmutador nativo (Ctrl+K).", target=app("open_switcher")),
    T("app_tabs_overview", "app", "Abre la vista de todas las pestañas.", target=app("tabs_overview")),
    T("app_help", "app", "Abre la ayuda de atajos (F1).", target=app("help")),
    T("app_snippets", "app", "Abre el panel nativo de snippets (Ctrl+Shift+K).", target=app("snippets")),
    T("app_new_local_tab", "app", "Nueva terminal VTE aquí (Ctrl+T).", target=app("new_local_tab")),
    T("app_open_xterm_tab", "app", "Abre una sesión en el terminal xterm.js (Ctrl+Shift+T).", P(session="Sesión (local por defecto)"), (), app("open_xterm_tab")),
    T("app_open_wizard", "app", "Abre el wizard de nueva sesión desde la app (+).", target=app("open_wizard")),
    T("app_start_ai_here", "app", "Inicia IA en el pane actual (Ctrl+Shift+A).", P(session="Sesión", pane=PANE), (), app("start_ai_here")),
    T("app_copy_selection", "app", "Copia la selección del terminal al portapapeles del sistema.", target=app("copy_selection")),
    T("app_paste_clipboard", "app", "Pega el portapapeles del sistema en el terminal (Ctrl+V).", target=app("paste_clipboard")),
    T("app_copy_reply", "app", "Copia la última respuesta de la IA de la pestaña actual.", target=app("copy_reply")),
    T("app_window", "app", "Ventana de la app: minimize, maximize, restore, raise.", P(action=("string", "Acción", ("minimize", "maximize", "restore", "raise"))), ("action",), app("window")),
    T("app_side_dashboard_width", "app", "Posición del divisor tablero/terminales (px).", P(px=("integer", "Píxeles")), ("px",), app("paned_position")),
    T("app_quit", "app", "Cierra la app de escritorio (Ctrl+Q).", target=app("quit"), destructive=True),
    # ───────────── chat / operador ─────────────
    T("remember", "chat", "Guarda un hecho en la memoria del operador.", P(fact="Hecho"), ("fact",), local("remember")),
    T("memory_read", "chat", "Lee la memoria completa del operador.", target=local("memory_read"), readonly=True),
    T("memory_replace", "chat", "Reescribe la memoria completa (para borrar o corregir).", P(text="Texto completo"), ("text",), local("memory_replace"), destructive=True),
    T("new_conversation", "chat", "Empieza una conversación nueva del operador.", target=ui_click("#op-new")),
    T("set_chat_model", "chat", "Modelo del chat: haiku, gpt-5.3-codex-spark, grok-4.5.", P(model=("string", "Modelo", ("haiku", "gpt-5.3-codex-spark", "grok-4.5"))), ("model",), api("POST", "/operator/model", {"model": "$model"})),
    T("copy_last_reply", "chat", "Copia la última respuesta del chat.", target=local("copy_reply")),
    T("copy_session_reply", "chat", "Copia la última respuesta de la IA de una sesión.", P(tab=TAB), ("tab",), local("copy_session")),
]

DESTRUCTIVE = frozenset(t.name for t in CATALOG if t.destructive)
READONLY = frozenset(t.name for t in CATALOG if t.readonly)
APP_TOOL_BY_COMMAND = {t.target["command"]: t.name for t in CATALOG if t.target["kind"] == "app"}
APP_COMMAND_NAMES = frozenset(APP_TOOL_BY_COMMAND)
_BY_NAME = {t.name: t for t in CATALOG}


def by_name(name):
    return _BY_NAME.get(str(name or ""))


def _schema(t):
    return {"type": "object", "properties": t.params, "required": list(t.required)}


def anthropic_tools():
    return [{"name": t.name, "description": t.description, "input_schema": _schema(t)} for t in CATALOG]


def openai_tools():
    return [{"type": "function", "function": {"name": t.name, "description": t.description, "parameters": _schema(t)}} for t in CATALOG]


def groups_summary():
    groups = {}
    for t in CATALOG:
        groups.setdefault(t.group, []).append(t.name)
    return "\n".join(f"- {g}: " + ", ".join(names) for g, names in groups.items())
