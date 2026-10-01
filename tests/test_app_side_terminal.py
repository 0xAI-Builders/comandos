"""La terminal de la barra es una terminal nativa de sesión en el escritorio, y el
panel izquierdo y la parte de terminales se pueden esconder."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
APP = (ROOT / "bin" / "cc-app").read_text()
HTML = (ROOT / "dash" / "index.html").read_text()


def test_left_column_stacks_the_dashboard_over_a_native_session_terminal():
    assert "_side_paned.pack1(wv, True, False)" in APP and "_side_paned.pack2(_side_term_host, True, False)" in APP
    import re
    assert re.search(r"^paned\.pack1\(_side_paned, True, False\)$", APP, re.M)
    assert not re.search(r"^paned\.pack1\(wv,", APP, re.M)
    box = APP[APP.index("def _side_term_box("):APP.index("def _side_term_apply_share(")]
    # la misma receta que una pestaña: VTE con tmux attach + cabecera de pane y marcos
    assert "make_term(" in box and "tmux attach -t" in box and "_attach_model_bar(box, sess)" in box


def test_web_tells_the_app_which_terminal_and_the_app_reports_focus_back():
    mount = HTML[HTML.index("function sidebarTermMount("):HTML.index("function activePaneTarget(")]
    assert "if(inApp()){" in mount and "sidebarTerm: {session: o.session" in mount and "tabs: o.tabs" in mount
    assert "window.sidebarTermAction = function(kind, id)" in HTML and "commandSidebar.termAction(kind, id)" in HTML
    assert 'if isinstance(d.get("sidebarTerm"), dict):' in APP and "_side_term_show" in APP
    assert 'term.connect("focus-in-event", lambda t, _e: (globals().get("_side_term_focused")' in APP
    focus = APP[APP.index("def _side_term_focused("):APP.index("def _left_panel_set(")]
    assert "sidebarTermFocused" in focus and "_dash_js_quiet" in focus
    assert "window.sidebarTermFocused = function(sess)" in HTML


def test_side_terminal_gets_frames_and_its_own_focus():
    vis = APP[APP.index("def _visible_boxes("):APP.index("def _box_focused(")]
    assert '_SIDE.get("box")' in vis
    foc = APP[APP.index("def _box_focused("):APP.index("def _pane_frames(")]
    assert 'getattr(box, "_side", False)' in foc and "has_focus()" in foc


def test_left_panel_toggle_lives_outside_the_panel_and_is_remembered():
    assert '_icon_btn("panel-left"' in APP and "_strip_start.pack_start(_left_btn" in APP
    assert "nb.set_action_widget(_strip_start, Gtk.PackType.START)" in APP
    assert (ROOT / "dash" / "icons" / "panel-left.svg").exists()
    assert 'if _SIDE.get("leftHidden"):' in APP and "app-layout.json" in APP
    assert 'd.get("leftPanel") in ("toggle", "hide", "show")' in APP


def test_side_terminals_have_one_native_header_that_is_also_the_drag_handle():
    # la cabecera GTK = la .cs-terms del remoto: agarre, pestañas, «+ Terminal», ▾
    assert '_side_term_host.pack_start(_side_head_ev, False, False, 0)' in APP
    assert '_side_paned.set_wide_handle(False)' in APP and 'add_class("cc-shelf-paned")\n_side_paned' not in APP
    for piece in ('_side_grip', '_side_plus', '_side_tog', '"row-resize"'):
        assert piece in APP
    msg = APP[APP.index('if isinstance(d.get("sidebarTerm"), dict):'):]
    msg = msg[:msg.index("return")]
    assert '_SIDE["tabs"] = _side_tabs_from_web(st.get("tabs"))' in msg
    act = APP[APP.index("def _side_term_action("):APP.index("_side_plus = ")]
    assert "sidebarTermAction" in act and "_dash_js_quiet" in act
    drag = APP[APP.index("def _side_head_press("):APP.index('_side_head_ev.connect("button-press-event"')]
    assert '_SIDE.get("collapsed")' in drag and "set_position" in drag and "_side_term_save_share()" in drag
    show = APP[APP.index("def _side_term_show("):APP.index("def _side_term_save_share(")]
    assert '_SIDE["collapsed"] = box, sess, box is None' in show and "_side_term_keep_collapsed()" in show
    # la web, en modo nativo, no pinta su propia cabecera
    assert "html[data-native-side-term] #command-sidebar .sec-terms{display:none}" in HTML


def test_side_tabs_from_web_are_sanitized():
    import types
    ns = {"SESSION_RE": re.compile(r"^[A-Za-z0-9_.:-]{1,120}$")}
    src = APP[APP.index("def _side_tabs_from_web("):APP.index("def _side_term_show(")]
    exec(src, ns)
    out = ns["_side_tabs_from_web"]([{"id": "T-1", "label": "14:56", "on": 1}, {"id": "bad id!"}, "x"])
    assert out == [{"id": "T-1", "label": "14:56", "title": "T-1", "on": True, "sel": False, "closing": False}]
    assert ns["_side_tabs_from_web"](None) == []


def test_side_tabs_keep_their_own_look_under_every_button_style():
    # el estilo 3D global de botones (prioridad mayor) pisaba el borde de la pestaña activa
    gen = APP[APP.index('    gen = "button:not(.cc-key)'):]
    assert ":not(.side-btn)" in gen[:gen.index("\n")]
    assert ".side-tab.cur { background-color: mix(@BAR@, @LINE2@, 0.35); border-bottom-color: @BRAND@; }" in APP


def test_side_tabs_close_with_confirmation_and_the_toggle_is_a_persiana():
    tab = APP[APP.index("def _side_tab_widget("):APP.index("def _side_head_render(")]
    assert '_side_term_action("close", k)' in tab and '"¿Cerrar?" if closing else "✕"' in tab
    render = APP[APP.index("def _side_head_render("):APP.index("def _side_tabs_from_web(")]
    assert '"chevron-up" if hidden else "chevron-down"' in render and '("closed")' in render
    assert ".side-head.closed { background-image: repeating-linear-gradient(" in APP
    kill = HTML[HTML.index("async function sidebarKillTerm("):HTML.index("function activePaneTarget(")]
    assert "isQuickTermSession(sess)" in kill and 'api("/kill", {session: sess})' in kill
    assert "killTerm: sidebarKillTerm" in HTML
