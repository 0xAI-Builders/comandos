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
    assert "grid-template-rows:44px minmax(0,1fr)" in panes
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
    # El modal de Servidores se acomoda por SU ancho (1-oct): en el remoto partido el modal mide
    # la columna, como en el escritorio, y @media veía toda la ventana.
    servers = re.sub(r"\s+", "", block("@container modal (max-width:640px)"))
    assert "#servers.modal-panel{padding:24px18px}" in servers
    assert ".srv-row{flex-wrap:wrap}" in servers
    assert ".srv-info{flex-basis:100%}" in servers
    assert ".srv-rowbutton{flex:110;min-height:36px}" in servers


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


def test_ai_picker_and_its_controller_are_removed():
    assert 'id="motor-pop"' not in INDEX
    assert 'openMotorFor' not in INDEX
    assert 'session-controls.js' not in INDEX
    assert not Path('dash/session-controls.js').exists()
