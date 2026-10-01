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
    assert ".side-tab.cur { background-color: mix(mix(@BAR@, @TEXT@, 0.06), @BRAND@, 0.16); border-color: mix(@BAR@, @BRAND@, 0.6); }" in APP


def test_side_tabs_are_pills_in_a_track_with_round_arrows():
    # píldoras con flechas (grill ronda 2): ‹ pista › + «+» redondo, rueda = desplazamiento
    head = APP[APP.index("_side_head.pack_start(_side_grip"):]
    head = head[:head.index("_side_head_ev.connect(")]
    order = ["_side_grip", "_side_prev", "_side_tabs_sw", "_side_next", "_side_plus"]
    assert [head.index(f"pack_start({n},") for n in order] == sorted(head.index(f"pack_start({n},") for n in order)
    sync = APP[APP.index("def _side_sync_arrows("):APP.index("def _side_tabs_wheel(")]
    assert "set_visible(over)" in sync and "set_sensitive" in sync
    assert '_side_tabs_sw.connect("scroll-event", _side_tabs_wheel)' in APP
    assert ".side-tab { background-color: mix(@BAR@, @TEXT@, 0.06); border: 1px solid @LINE@; border-radius: 999px; }" in APP


def test_side_tabs_close_with_confirmation_and_the_toggle_is_a_persiana():
    tab = APP[APP.index("def _side_tab_widget("):APP.index("def _side_head_render(")]
    assert '_side_term_action("close", k)' in tab and '"¿Cerrar?" if closing else "✕"' in tab
    render = APP[APP.index("def _side_head_render("):APP.index("def _side_tabs_from_web(")]
    assert '"chevron-up" if hidden else "chevron-down"' in render and '("closed")' in render
    assert "repeating-linear-gradient" not in APP[APP.index(".side-head {"):APP.index(".tabplus {")]   # sin rayas
    kill = HTML[HTML.index("async function sidebarKillTerm("):HTML.index("function activePaneTarget(")]
    assert "isQuickTermSession(sess)" in kill and 'api("/kill", {session: sess})' in kill
    assert "killTerm: sidebarKillTerm" in HTML


def test_left_panel_hides_completely_not_down_to_its_minimum_width():
    """Grill 1-oct: contraído = «oculto del todo», como el remoto. El panel va con
    shrink=False; set_position(0) lo dejaba en su ancho mínimo (~98 px)."""
    import ast
    import os
    import time
    import pytest
    if not os.environ.get("DISPLAY"):
        pytest.skip("needs a display for GTK")
    import gi
    gi.require_version("Gtk", "3.0")
    from gi.repository import Gtk

    def pump(seconds=0.3):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            while Gtk.events_pending():
                Gtk.main_iteration()
            time.sleep(0.01)

    paned = Gtk.Paned(orientation=Gtk.Orientation.HORIZONTAL)
    side, terms = Gtk.Box(), Gtk.Box()
    side.set_size_request(98, -1)
    paned.pack1(side, True, False)
    paned.pack2(terms, True, False)
    win = Gtk.OffscreenWindow(); win.add(paned); paned.set_size_request(1200, 300); win.show_all()
    paned.set_position(500); pump()
    paned.set_position(0); pump()
    assert side.get_allocated_width() >= 98, "the old way: clamped to the minimum, never hidden"
    paned.set_position(500); pump()

    calls = []
    ns = {"paned": paned, "_side_paned": side, "_SIDE": {"leftPos": 98}, "_left_btn_paint": lambda: calls.append("paint"),
          "_layout_save": lambda: calls.append("save"), "_dash_js_quiet": lambda js: calls.append(js),
          "_LEFT_FX": {"snap": None}, "_left_fx_snapshot": lambda: None,
          "_left_fx_run": lambda hiding, snap, width: calls.append(("fx", hiding, width))}
    for name in ("_left_panel_set", "_left_panel_toggle"):
        node = next(n for n in ast.parse(APP).body if isinstance(n, ast.FunctionDef) and n.name == name)
        exec(compile(ast.Module(body=[node], type_ignores=[]), "<app>", "exec"), ns)
    ns["_left_panel_toggle"](); pump()
    assert not side.get_visible() and ns["_SIDE"]["leftHidden"] is True
    assert terms.get_allocated_width() == paned.get_allocated_width(), "terminals take the whole width"
    assert ns["_SIDE"]["leftPos"] == 500 and "window.appLeftPanel&&appLeftPanel(true)" in calls
    ns["_left_panel_toggle"](); pump()
    assert side.get_visible() and ns["_SIDE"]["leftHidden"] is False and abs(paned.get_position() - 500) <= 1
    assert ("fx", False, 500) in calls, "showing slides the panel in"
    ns["_SIDE"]["leftPos"] = 98                       # una posición «plegada» vieja no reabre una tira
    ns["_left_panel_set"](True); ns["_left_panel_set"](False); pump()
    assert paned.get_position() >= 280
    assert 'GLib.idle_add(_left_panel_set, {"toggle": not _SIDE.get("leftHidden")' in APP
    assert "paned.get_position() >= 60" not in APP
    assert "paned.get_position() < 420" not in APP, "showing dashboard content never widens a panel you sized"
    save = APP[APP.index("def _save_pane_position("):]
    save = save[:save.index("\n\n\n")]
    assert 'or _SIDE.get("leftHidden") or not _side_paned.get_visible():' in save, "a hidden panel never saves its width"
    assert '_side_paned.set_no_show_all(True)' in APP



def test_left_panel_slide_is_an_overlay_and_never_resizes_the_terminals_per_frame():
    """1-oct: «ponle un mejor efecto de transición». Se anima una imagen del panel
    encima; las terminales cambian de tamaño una sola vez (tmux + Claude Code)."""
    import ast
    import os
    import time
    import pytest
    if not os.environ.get("DISPLAY"):
        pytest.skip("needs a display for GTK")
    import cairo
    import gi
    gi.require_version("Gtk", "3.0")
    gi.require_version("Gdk", "3.0")
    from gi.repository import Gdk, GdkPixbuf, Gtk

    def pump(seconds):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            while Gtk.events_pending():
                Gtk.main_iteration()
            time.sleep(0.01)

    overlay, paned = Gtk.Overlay(), Gtk.Paned()
    paned.pack1(Gtk.Box(), True, False); paned.pack2(Gtk.Box(), True, False)
    overlay.add(paned)
    win = Gtk.Window(); win.add(overlay); win.set_default_size(800, 300); win.show_all()
    pump(0.2)
    ns = {"Gtk": Gtk, "Gdk": Gdk, "cairo": cairo, "modal_overlay": overlay, "paned": paned,
          "THEME": {"bar": "#0a121b"}, "_animations_enabled": lambda: True}
    for name in ("_left_fx_clear", "_left_fx_run", "_tween"):
        node = next(n for n in ast.parse(APP).body if isinstance(n, ast.FunctionDef) and n.name == name)
        exec(compile(ast.Module(body=[node], type_ignores=[]), "<app>", "exec"), ns)
    ns["_LEFT_FX"] = {"snap": None, "da": None}
    ns["LEFT_FX_MS"] = 150
    snap = (GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB, False, 8, 300, 200), 0, 30, 300, 200)
    for hiding in (True, False):
        ns["_left_fx_run"](hiding, snap if hiding else None, 300)
        da = ns["_LEFT_FX"]["da"]
        assert da is not None and da.get_parent() is overlay and overlay.get_overlay_pass_through(da), "clicks pass through"
        pump(0.5)
        assert ns["_LEFT_FX"]["da"] is None and da.get_parent() is None, "the overlay goes away when the slide ends"
    ns["_animations_enabled"] = lambda: False                 # movimiento reducido: sin animación
    ns["_left_fx_run"](True, snap, 300)
    assert ns["_LEFT_FX"]["da"] is None
    body = APP[APP.index("def _left_panel_set("):APP.index("def _left_panel_toggle(")]
    assert "_tween(" not in body and "set_position(" in body, "the paned moves once; only the overlay animates"


def test_folded_commands_blind_fits_the_webview_to_its_header():
    """Persiana de Comandos plegada (grill 1-oct): el WebView mide hasta la cabecera de
    Comandos (alto que manda la web, por el zoom) y la terminal nativa toma el resto."""
    msg = APP[APP.index('if isinstance(d.get("sidebarTerm"), dict):'):]
    msg = msg[:msg.index("return")]
    assert '_SIDE["cmds_closed"] = not bool(c.get("open"))' in msg and '_SIDE["cmds_h"]' in msg
    mount = HTML[HTML.index("function sidebarTermMount("):HTML.index("function activePaneTarget(")]
    assert "cmds: o.cmds || null" in mount
    assert "html[data-native-side-term] #command-sidebar.cmds-closed .sec-cmds{flex:0 0 auto!important}" in HTML
    src = APP[APP.index("def _side_pin_pos("):APP.index("def _side_apply_pin(")]

    class Host:
        visible = True
        def get_visible(self): return self.visible

    class Paned:
        def get_property(self, k): return {"min-position": 0, "max-position": 900}[k]

    class View:
        def get_zoom_level(self): return 1.25

    ns = {"_SIDE": {"cmds_closed": True, "cmds_h": 200, "collapsed": False},
          "_side_term_host": Host(), "_side_paned": Paned(), "wv": View()}
    exec(src, ns)
    pin = ns["_side_pin_pos"]
    assert pin() == 250                                   # 200 px CSS × zoom 1.25
    ns["_SIDE"]["cmds_h"] = 5000; assert pin() == 900     # nunca pasa del máximo del paned
    ns["_SIDE"]["cmds_h"] = 10; assert pin() is None      # sin alto real no fija nada
    ns["_SIDE"].update(cmds_h=200, cmds_closed=False); assert pin() is None   # desplegada: reparto del usuario
    ns["_SIDE"].update(cmds_closed=True, collapsed=True); assert pin() is None  # sin terminal: plegada manda
    # mientras está fijada no se arrastra ni se guarda el reparto como si fuera del usuario
    drag = APP[APP.index("def _side_head_press("):APP.index("def _side_head_motion(")]
    assert "_side_pin_pos() is not None" in drag
    save = APP[APP.index("def _side_term_save_share("):APP.index("def _side_paned_moved(")]
    assert "_side_pin_pos() is not None" in save
    assert '_side_paned.connect("size-allocate", lambda *_: _side_keep_layout())' in APP
