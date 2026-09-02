#!/usr/bin/env python3
import re
from pathlib import Path


HTML = Path("dash/index.html").read_text()


def test_usage_drawer_markup_exists():
    assert 'id="btn-usage"' in HTML
    assert 'id="usage"' in HTML
    assert 'id="usage-limits"' in HTML
    assert 'id="usage-providers"' in HTML
    assert 'id="usage-projects"' in HTML
    assert 'id="usage-alerts"' in HTML


def test_usage_state_is_fetched_without_secret_rendering():
    assert 'api("/usage/state")' in HTML
    assert "renderUsage" in HTML
    forbidden = ["OPENAI_ADMIN_KEY", "ANTHROPIC_ADMIN_KEY", "x-api-key"]
    for text in forbidden:
        assert text not in HTML


def test_compare_tab_exposes_evidence_and_local_rating_controls():
    assert 'data-mtab="comparar"' in HTML
    assert 'id="compare-days"' in HTML
    assert 'id="compare-task"' in HTML
    assert 'id="compare-body"' in HTML
    assert 'api(`/usage/analytics?' in HTML
    assert 'api("/usage/interactions?limit=12")' in HTML
    assert 'api("/usage/rating",payload)' in HTML
    assert "sólo experimentos pareados pueden declarar ganador" in HTML
    assert "r.eligible" in HTML
    assert "r.successCI" in HTML
    assert "r.toolErrorRate" in HTML


def test_notifications_v2_prioritize_and_carry_action_buttons():
    # v2: el panel muestra SOLO lo importante (desbordes, sugerencias,
    # modelos, skills/MCPs) con botones de accion; los turnos van agrupados
    # y el badge cuenta unicamente las clases prioritarias.
    assert "nfPriorityItems" in HTML
    for cls in ('"desborde"', '"sugerencia"', '"modelo"', '"skill"', '"mcp"'):
        assert cls in HTML, cls
    # acciones: aplicar / guardia / ahorro + abrir(Brave)/copiar + snooze/pin/dismiss.
    # Las cards de NOTICIA traen sus botones DIRECTOS en la propia card ("leer"
    # abre en el navegador del sistema via /open-url, "copiar" copia el link);
    # la de MODELO expande su detalle in-place ("detalle"). Un solo click.
    for act in ('"aplicar"', '"guardia"', '"ahorro"', '"detalle"',
                '"leer"', '"copiar"', '"snooze"', '"pin"', '"dismiss"'):
        assert act in HTML, act
    # la card de modelo abre su detalle in-place, NUNCA el wizard de sesion nueva
    assert 'nsOpen(); return; }' not in HTML
    # se retiro el navegador MODAL interno: nada abre openUrlModal desde las cards
    assert 'data-inapp' not in HTML and 'openUrlModal:' not in HTML
    assert "cc-nf-snooze" in HTML and "cc-nf-pins" in HTML and "cc-nf-dismiss" in HTML
    # turnos rutinarios agrupados y plegados, no cards individuales
    assert "nf2-rest" in HTML and "turnos terminados" in HTML
    # badge solo prioridades
    assert "el badge cuenta SOLO lo importante" in HTML


def test_model_confirmation_action_is_sticky():
    # El sticky ya no es solo el botón de acción: TODO el footer (mp-stick =
    # acción + cambio de harness + cuenta + estado) queda fijo fuera del scroll.
    assert "#motor-pop .mp-stick{position:sticky" in HTML
    assert '<div class="mp-stick">' in HTML
    assert "min-height:44px" in HTML


def test_effort_only_switch_does_not_resend_model():
    # Cambiar SOLO el esfuerzo (mismo modelo) NUNCA debe re-teclear `/model
    # <id>`: eso dejaba el modelo en un id crudo que la API rechaza con
    # model_unavailable en el siguiente turno. Con mismo modelo -> model:"".
    assert 'model:sameModel?"":st.model' in HTML
    # el ternario roto (ambas ramas iguales) no debe volver
    assert "sameModel?st.model:st.model" not in HTML


def test_session_cards_have_usage_chip_container():
    assert 'class="usage-chip hidden"' in HTML
    assert "usageChipText" in HTML


def test_usage_ui_exposes_model_selector_with_preset_names():
    # Un solo control por concepto: menu de modelo con la intencion como etiqueta
    for preset in ("Ahorro", "Diario", "Difícil", "Máximo"):
        assert preset in HTML
    assert "MODEL_CHOICES" in HTML
    assert 'api("/model/switch"' in HTML
    assert "model: " in HTML


def test_usage_ui_labels_detected_panes_without_unattributed_noise():
    assert "function confidenceLabel" in HTML
    assert '"detected": "detectado"' in HTML
    assert "confidenceLabel(u.confidence)" in HTML


def test_usage_ui_switches_models_inline_per_pane():
    # Menu propio (no <select> nativo: el WebKitGTK viejo de la app no lo abre)
    assert "mdl-menu" in HTML
    assert "mdl-btn" in HTML
    assert "modelSelectEl" not in HTML
    assert "session: btn.dataset.session" in HTML
    assert "pane: btn.dataset.pane || undefined" in HTML
    assert 'role="listbox"' in HTML
    assert 'role="option"' in HTML


def test_session_cards_have_model_selector():
    assert "modelMenuEl" in HTML
    assert "function wireModelMenu" in HTML
    assert "mdl-cur" in HTML


def test_no_agent_badges_in_header():
    # Los badges de agente se eliminaron a peticion del usuario
    assert 'id="agent-pick"' not in HTML


def test_header_has_no_search_nor_open_project():
    # The old inline search was removed; Ctrl+K opens the session switcher and
    # the plus button sits beside the Sessions heading.
    assert 'id="new-form"' not in HTML
    assert 'id="new-name"' not in HTML
    assert 'id="q"' not in HTML
    switch_button = re.search(
        r'<button\b(?=[^>]*\bid="btn-switch")'
        r'(?=[^>]*\btitle="[^"]*Ctrl\+K)[^>]*>', HTML
    )
    assert switch_button
    sessions_label = re.search(
        r'<div class="sec-label"><span class="chip" id="sessions-title">Sesiones</span>(?P<body>.*?)</div>', HTML, re.S
    )
    assert sessions_label
    assert 'id="btn-newsess"' in sessions_label.group("body")


def test_recent_closed_sessions_are_recoverable():
    assert 'id="recent-wrap"' in HTML
    assert 'api("/tab-history")' in HTML
    assert '"/recover-tab"' in HTML.replace("'", '"')
    assert "renderRecent" in HTML


def test_switcher_closes_on_outside_click():
    assert 'if(e.target === $("#sw-ov")) swClose();' in HTML


def test_terminal_tab_close_asks_with_modal_and_syncs_desktop():
    # La × abre un modal de confirmación; al aceptar cierra local Y en el
    # escritorio via /tab-close (sin esto el poll /tabs resucitaba la tab)
    assert 'id="tabclose"' in HTML
    assert "function askCloseTab" in HTML
    assert 'api("/tab-close"' in HTML
    # el prefijo "term:" se recorta antes de cerrar (bug: closeTerm no matcheaba)
    assert 'key.replace(/^term:/, "")' in HTML


def test_remote_tab_mirror_prunes_closed_desktop_tabs():
    # Tabs espejadas que ya no están en /tabs se PODAN (antes quedaban
    # huérfanas "solo en remoto")
    assert "o.mirrored && !want.has(s)" in HTML
    assert "addTermTab(t.session, t.label, true)" in HTML


def test_per_target_alert_rules_with_bells():
    assert "uv-bell" in HTML
    assert "openRuleMenu" in HTML
    assert "renderAlertRules" in HTML
    assert 'api("/usage/alert-rule"' in HTML
    assert 'id="alert-rules"' in HTML
    assert "RULE_BUDGETS" in HTML


def test_alert_button_is_explicit_and_limit_windows_configurable():
    # El boton dice "🔔 alerta" (no un icono mudo) y muestra la regla activa
    assert 'tf("alerta", "alert")' in HTML
    # Reglas por proveedor+lapso: cada ventana del plan tiene su boton con %
    assert "RULE_PERCENTS" in HTML
    assert 'data-scope="limit"' in HTML


def test_alert_thresholds_are_configurable_from_drawer():
    assert 'id="alert-config"' in HTML
    assert 'data-th="85"' in HTML
    assert "COMANDOS_ALERT_THRESHOLDS" in HTML
    assert "renderAlertConfig" in HTML


def test_project_panes_are_clickable_to_open_session():
    assert "function openPane" in HTML
    assert 'uv-pane[data-session]' in HTML
    assert ".uv-pane:hover" in HTML


def test_codex_dropdown_offers_models_with_reasoning():
    # Modelos vigentes de codex-cli 0.144.x (gpt-5.6 sol/terra/luna)
    assert "gpt-5.6-sol" in HTML
    assert "gpt-5.6-luna" in HTML
    assert "gpt-5.3-codex-spark" in HTML
    assert "data-effort" in HTML
    assert "effort: effort || undefined" in HTML


def test_opencode_menu_offers_providers_and_models():
    # OpenCode es el UNICO agente con seleccion de provider desde la UI
    assert 'api("/opencode/models")' in HTML
    assert "opencodeMenuHtml" in HTML
    assert "mdl-head" in HTML
    assert "model_name" in HTML


def test_card_usage_chip_is_full_width_line():
    # Texto COMPLETO siempre: el chip vive en su propia linea, sin ellipsis
    assert ".card .usage-chip" in HTML
    assert 'white-space:normal' in HTML


def test_usage_drawer_collapses_historic_sessions():
    # Las sesiones historicas (transcripts) no se listan una por una:
    # se ven los panes vivos + una linea de historial (adios duplicados)
    assert "historial" in HTML
    assert 'startsWith("%")' in HTML


def test_usage_ui_renders_exact_limit_bars():
    # Porcentajes exactos del proveedor (OAuth Claude / rollouts Codex),
    # sin inputs manuales de limites de tokens.
    assert 'id="usage-limits"' in HTML
    assert "renderUsageLimits" in HTML
    assert "resetea" in HTML
    assert 'id="limit-codex-daily"' not in HTML
    assert 'id="usage-limits-save"' not in HTML


def test_header_shows_global_limit_percentages():
    # Los porcentajes globales viven SIEMPRE visibles en el header,
    # no solo dentro del drawer
    assert 'id="limits-strip"' in HTML
    assert "renderLimitsStrip" in HTML


def test_refactored_limit_cards_show_remaining_reset_and_daily_curve():
    # La card responde las tres preguntas: cuanto llevo, CUANTO ME QUEDA,
    # y cuando resetea; Grok ademas dibuja su curva diaria real (14d)
    assert "uw-remain" in HTML and '"queda", "left"' in HTML
    assert "uw-reset" in HTML and "uw-track" in HTML
    assert "uw-daily" in HTML and "l.daily" in HTML
    # Umbrales de alerta visibles en el track (70% y 90%)
    assert 'left:70%' in HTML and 'left:90%' in HTML
    # Cuota declarada por el usuario para providers sin API de limites
    assert "/usage/quota" in HTML and "data-quota-provider" in HTML
    assert "definir límite" in HTML
    # El editor es inline (input + guardar), no un prompt del navegador
    assert "uw-quota-edit" in HTML and "window.prompt" not in HTML


def test_limit_cards_have_style_switcher_and_grok_measured_support():
    # Switcher persistido (barras / anillos / compacta) con logos por card
    assert "cc-limit-style" in HTML
    for style in ("bars", "rings", "compact"):
        assert f'data-style="{style}"' in HTML or f'"{style}"' in HTML
    assert "limitRingSvg" in HTML and "uv-style" in HTML
    # Grok sin API de limites: card medida-local honesta, sin barra falsa
    assert "medido local" in HTML
    assert "limitMetricHtml" in HTML and "tokens_today" in HTML
    # La campana de alertas no se ofrece en cards medidas (no hay % que alertar)
    assert 'l.kind === "measured"' in HTML


if __name__ == "__main__":
    test_terminal_tab_close_asks_with_modal_and_syncs_desktop()
    test_remote_tab_mirror_prunes_closed_desktop_tabs()
    test_alert_button_is_explicit_and_limit_windows_configurable()
    test_per_target_alert_rules_with_bells()
    test_project_panes_are_clickable_to_open_session()
    test_alert_thresholds_are_configurable_from_drawer()
    test_codex_dropdown_offers_models_with_reasoning()
    test_usage_drawer_markup_exists()
    test_usage_state_is_fetched_without_secret_rendering()
    test_session_cards_have_usage_chip_container()
    test_usage_ui_exposes_model_selector_with_preset_names()
    test_usage_ui_labels_detected_panes_without_unattributed_noise()
    test_usage_ui_switches_models_inline_per_pane()
    test_session_cards_have_model_selector()
    test_usage_ui_renders_exact_limit_bars()
    test_header_shows_global_limit_percentages()
    test_no_agent_badges_in_header()
    test_header_has_no_search_nor_open_project()
    test_recent_closed_sessions_are_recoverable()
    test_switcher_closes_on_outside_click()
    test_opencode_menu_offers_providers_and_models()
    test_card_usage_chip_is_full_width_line()
    test_usage_drawer_collapses_historic_sessions()


def test_every_metric_surface_carries_an_insight_with_recommendation():
    # Regla de producto: ninguna métrica "pelona" — cada bloque interpreta
    # su número y recomienda algo concreto, calculado determinístico.
    assert "limitInsight" in HTML          # cards de límites (ritmo vs reloj)
    assert "pcDailyInsight" in HTML        # serie diaria (tendencia 3d)
    assert "pc-insight" in HTML and "uw-insight" in HTML
    # composición (salud de caché), tabla de modelos ($/turno), estimado, ROI
    for marker in ("reutiliza", "necesita el caro", "absorbieron", "ROI"):
        assert marker in HTML, marker
    # dedicación y proyectos también interpretan
    assert "Día enfocado" in HTML and "se come el" in HTML
    # proyección honesta: usa la ventana real del límite, no números mágicos
    assert "LIMIT_WINDOW_SEC" in HTML
    # los insights traen BOTONES de acción reales (Guardia/Optimizar/CSV)
    assert "pc-act" in HTML and 'act: "guardia"' in HTML
    assert 'act: "optimizar"' in HTML and 'act: "csv"' in HTML
    # gráficos nuevos: heatmap día×hora, dispersión, tabla ordenable, $ por día
    assert "pcHeatmap" in HTML and "pcScatterChart" in HTML
    assert "pcModelTable" in HTML and "pcEstDailyChart" in HTML
    assert 'data-sort=' in HTML and "pcModelsCsv" in HTML
    # ROI: costo de suscripción declarado por el usuario + moneda
    assert "pcRoiBlock" in HTML and "data-sub-provider" in HTML
    assert "/usage/subscription" in HTML and "SUBS_CURRENCIES" in HTML
