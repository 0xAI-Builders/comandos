#!/usr/bin/env python3
import re
from pathlib import Path


INDEX = Path("dash/index.html").read_text()
CSS = INDEX


def rule(selector: str) -> str:
    pattern = re.compile(re.escape(selector) + r"\{([^}]*)\}", re.S)
    match = pattern.search(CSS)
    assert match, f"missing CSS rule: {selector}"
    return re.sub(r"\s+", " ", match.group(1)).strip()


def block(selector: str) -> str:
    marker = selector + "{"
    start = CSS.index(marker) + len(marker)
    depth = 1
    for index in range(start, len(CSS)):
        if CSS[index] == "{":
            depth += 1
        elif CSS[index] == "}":
            depth -= 1
            if depth == 0:
                return CSS[start:index]
    raise AssertionError(f"unclosed CSS block: {selector}")


def test_split_left_panel_is_the_scroll_container():
    panel = rule("body.app.split #view-panel")
    assert "min-height:0" in panel
    assert "overflow-y:auto" in panel
    assert "overflow-x:hidden" in panel

    content = rule("body.app.split #view-panel #content")
    assert "overflow:visible" in content
    assert "flex:none" in content


def test_desktop_panel_has_a_real_content_scroller():
    panes = rule("#panes")
    assert "display:flex" in panes
    assert "flex-direction:column" in panes

    panel = rule("#view-panel")
    assert "display:flex" in panel
    assert "flex-direction:column" in panel
    assert "min-height:0" in panel


def test_remote_shell_uses_dynamic_viewport_grid_without_fixed_tab_offset():
    assert "interactive-widget=resizes-content" in CSS
    app = rule("body.app")
    assert "var(--app-height,100dvh)" in app
    panes = rule("body.app #panes")
    assert "grid-template-rows:auto minmax(0,1fr)" in panes
    assert "safe-area-inset-top" in panes
    assert "safe-area-inset-bottom" in panes
    narrow = rule("body.app #view-panel,body.app #term-area")
    assert "top:44px" not in narrow
    assert "position:absolute" not in narrow


def test_remote_touch_targets_have_stable_minimums():
    tab = rule(".apptab")
    assert "min-height:44px" in tab
    splitter = rule("body.app.split #splitter::before")
    assert "inset:0 -12px" in splitter


def test_phone_dashboard_keeps_servers_compact_and_session_actions_visible():
    mobile = block("@media (max-width:640px)")
    compact = re.sub(r"\s+", "", mobile)

    assert "#ssh-bar{flex-wrap:wrap;overflow:visible}" in compact, "servidores se envuelven, sin scroll horizontal"
    # el PATH ya no se oculta en móvil: es el identificador humano de la fila
    # (display:block bajo el nombre, con wrap — decisión del rediseño de rows)
    assert ".row.rpath{display:block" in compact
    assert ".row.name{flex-basis:140px}" in compact
    assert "#servers.modal-panel{padding:24px18px}" in compact
    assert ".srv-row{flex-wrap:wrap}" in compact
    assert ".srv-info{flex-basis:100%}" in compact
    assert ".srv-rowbutton{flex:110;min-height:36px}" in compact


def test_tablet_dashboard_wraps_session_rows_before_they_overflow():
    tablet = block("@media (max-width:900px)")
    compact = re.sub(r"\s+", "", tablet)

    assert '"actsactsactsacts"' in compact, "las acciones bajan a su propia fila"
    assert ".row.acts{display:none}" in compact, "las tarjetas no llevan botones: click = seleccionar"


def test_ssh_connection_list_is_an_independent_touch_scroller():
    saved = rule("#srv-list")

    assert "max-height:min(42dvh,420px)" in saved
    assert "overflow-y:auto" in saved
    assert "overflow-x:hidden" in saved
    assert "overscroll-behavior-y:contain" in saved
    assert "-webkit-overflow-scrolling:touch" in saved
    assert "touch-action:pan-y" in saved
    assert "scrollbar-gutter:stable" in saved
    assert "scrollbar-width:auto" in saved
    assert "width:10px" in rule("#srv-list::-webkit-scrollbar")
    assert (
        '<div id="srv-list" role="region" '
        'aria-label="Conexiones guardadas" tabindex="0"></div>'
    ) in CSS


def test_gtk_sidebar_keeps_split_cards_and_hides_global_inventory():
    # La app GTK necesita las cards de panes; ocultar sessions-wrap deja sidebar vacío.
    assert "html.gtkapp #sessions-wrap{display:none}" not in INDEX
    assert 'id="sessions-title"' in INDEX
    assert "appPanes.length >= 2 && it.session === appSess" in INDEX
    assert '$("#sessions-wrap").classList.toggle("hidden", nPanes < 2)' in INDEX
    assert "CENTRO_VIEW.sessionId === rowKey(it)" in INDEX
    assert 'const rk = rowKey(it)' in INDEX


def test_active_tmux_pane_drives_the_sidebar_control_card():
    assert '(d.pane || "") !== (ACTIVE_TAB.pane || "")' in INDEX
    assert 'S.sel = key; S.selTs = Date.now()' in INDEX
    assert 'render(S.list || [])' in INDEX
    assert 'r.dataset.rk === key' in INDEX


def test_gtk_sidebar_reuses_compact_analytics_instead_of_all_sessions():
    assert 'id="sidebar-insights"' in INDEX
    assert "async function renderSidebarInsights()" in INDEX
    assert 'api("/dedication")' in INDEX
    assert 'api("/usage/state")' in INDEX


def test_motor_picker_is_inline_and_contained_in_gtk_sidebar():
    assert '$("#centro-wrap").appendChild(pop)' in INDEX
    assert 'pop.classList.add("inline")' in INDEX
    assert 'html.gtkapp #motor-pop.inline' in INDEX
    assert 'box-sizing:border-box' in rule("#motor-pop")
    assert 'minmax(260px,1fr)' in rule("#motor-pop .mp-grid")
    assert 'box-sizing:border-box' in rule(".mtile")
    assert 'min-width:0' in rule(".mtile")


def test_selecting_card_does_not_focus_or_change_terminal_session():
    handler = INDEX.split('el.addEventListener("click", e=>{', 1)[1].split('});', 1)[0]
    assert "S.sel = rowKey(it)" in handler
    assert "openSession" not in handler
    assert 'botón explícito "Abrir"' in handler


def test_sidebar_today_projects_are_a_card_with_real_tracks_and_share():
    # Hoy: pista de 7px visible, filas de 44px mínimo separadas, % del día y
    # progressbar accesible. Sin porcentajes (solo horas). Nada de hilos de 3px.
    assert ".side-proj .bar{grid-column:1/-1;height:7px" in INDEX
    assert ".side-proj{display:grid;grid-template-columns:minmax(0,1fr) auto;" in INDEX
    assert "min-height:44px" in INDEX.split(".side-proj{",1)[1].split("}",1)[0]
    assert '<span class="pct"' not in INDEX and 'role="progressbar"' in INDEX
