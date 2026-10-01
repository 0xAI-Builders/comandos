"""La terminal de la barra es una terminal nativa de sesión en el escritorio, y el
panel izquierdo y la parte de terminales se pueden esconder."""
from pathlib import Path

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
    assert "if(inApp()){" in mount and "sidebarTerm: {session: sess" in mount
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
