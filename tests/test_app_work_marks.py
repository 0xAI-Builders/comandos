"""cc-app glue for E1 marks: same endpoint as the web, never overwrites a
newer mark, and the context menu offers tab and (identified) pane scopes."""
import ast
import io
import json
import sys
import urllib.error
from pathlib import Path
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT.joinpath("bin", "cc-app").read_text()
sys.path.insert(0, str(ROOT / "lib"))
import work_marks  # noqa: E402

NAMES = ["work_mark_row", "paint_tab_mark", "_paint_tab_sticker", "_paint_all_tab_marks", "apply_work_marks", "adopt_work_mark",
         "set_work_mark", "append_work_mark_menu", "tab_indicator_display", "tab_indicator_menu", "_mark_menu_item"]


class Widget:
    def __init__(self, label=None):
        self.label, self.children, self.active, self.sensitive = label, [], None, True
        self.handlers, self.submenu, self.shown, self.pixbuf, self.tooltip = {}, None, None, None, None
        self.animated, self.writes = [], 0

    def append(self, child): self.children.append(child)
    def set_sensitive(self, v): self.sensitive = v
    def set_active(self, v): self.active = v
    def set_draw_as_radio(self, v): pass
    def connect(self, signal, cb): self.handlers[signal] = cb
    def set_submenu(self, m): self.submenu = m
    def hide(self): self.shown = False
    def show(self): self.shown = True
    def set_from_pixbuf(self, p): self.pixbuf = p; self.writes += 1
    def set_tooltip_text(self, t): self.tooltip = t
    def set_text(self, t): self.label = t
    def get_style_context(self): return SimpleNamespace(add_class=lambda c: None, remove_class=lambda c: None)
    def add(self, child): self.children.append(child)
    def show_all(self): self.shown = True
    def popup_at_widget(self, *a): self.popped = True


def fake_gtk():
    class Box(Widget):
        def __init__(self, *a, **k): super().__init__()
        def pack_start(self, child, *a): self.children.append(child)
    return SimpleNamespace(Menu=Widget, MenuItem=lambda label=None: Widget(label),
                           CheckMenuItem=lambda label=None: Widget(label), SeparatorMenuItem=lambda: Widget("-"),
                           Box=Box, Label=lambda label=None: Widget(label),
                           Image=SimpleNamespace(new_from_pixbuf=lambda p: SimpleNamespace(pixbuf=p)),
                           Orientation=SimpleNamespace(HORIZONTAL=0))


def load(responses):
    posted, popups = [], []

    class Resp:
        def __init__(self, body): self.body = body
        def __enter__(self): return self
        def __exit__(self, *a): return False
        def read(self): return json.dumps(self.body).encode()

    def urlopen_ctx(request, timeout=None):
        posted.append(json.loads(request.data.decode()))
        status, body = responses.pop(0)
        if status != 200:
            raise urllib.error.HTTPError("u", status, "x", {}, io.BytesIO(json.dumps(body).encode()))
        return Resp(body)

    label = Widget()
    ns = {
        "json": json, "urllib": SimpleNamespace(request=SimpleNamespace(Request=lambda *a, **k: SimpleNamespace(
            data=k["data"]), urlopen=urlopen_ctx), error=urllib.error),
        "threading": SimpleNamespace(Thread=lambda target, daemon: SimpleNamespace(start=target)),
        "GLib": SimpleNamespace(idle_add=lambda fn, *a: fn(*a)),
        "notify_popup": lambda t, m: popups.append(m), "BASE_URL": "http://127.0.0.1:4777", "ES": True,
        "WORK_MARKS": {"rows": {}, "panes": [], "fetching": False}, "work_mark_state": work_marks,
        "Gtk": fake_gtk(), "tabs": {"sess": "box"}, "nb": SimpleNamespace(get_tab_label=lambda box: label), "tab_hb": lambda box: label,
        "_work_mark_pixbuf": lambda name: "pix:" + name,
        "_indicator_pixbuf": lambda icon, color, frame=0: f"pix:{icon}:{color}:{frame}",
        "_indicator_animate": lambda hb: hb._dot.animated.append(hb._ind_icon),
        "STATE_CACHE": {}, "DOT_COLORS": {"waiting": "#D08770", "working": "#81A1C1"}, "DOT_IDLE": "#4B5568",
        "TAB_FAVORITES": set(), "toggle_tab_favorite": lambda key: None,
    }
    label._work_mark = Widget()
    label._dot = label._work_mark
    label._sticker, label._suggest = Widget(), Widget()
    label.animated = []
    ns["time"] = SimpleNamespace(monotonic=lambda: 0.0)
    tree = ast.parse(SOURCE)
    nodes = [n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name in NAMES]
    assert len(nodes) == len(NAMES)
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "<app>", "exec"), ns)
    return ns, posted, popups, label._work_mark


def test_marks_from_the_endpoint_paint_the_tab_indicator():
    """Two channels (grill 30-sep): the indicator is the AI pixel semáforo; the
    human mark is a sticker after the name; finished-and-unmarked suggests Hecho."""
    ns, _, _, image = load([])
    ns["STATE_CACHE"]["sess"] = "working"
    ns["apply_work_marks"]({"marks": [{"scope": "session", "key": "sess", "mark": "frozen", "favorite": False,
                                       "revision": 2}], "panes": []})
    assert image.pixbuf == "pix:ai:work:None:0" and "Trabajando" in image.tooltip
    assert image.animated == ["ai:work"], "working loops continuously"
    hb = ns["tab_hb"](None)
    assert hb._sticker.label == "Aparcado" and hb._sticker.shown is True
    assert hb._suggest.shown is False
    ns["STATE_CACHE"]["sess"] = "done"
    ns["apply_work_marks"]({"marks": [], "panes": []})
    assert image.pixbuf == "pix:ai:done:None:0"
    assert hb._sticker.shown is False and hb._suggest.shown is True


def test_indicator_shows_what_the_ai_knows():
    display = load([])[0]["tab_indicator_display"]
    assert display("frozen", "working") == ("ai:work", None, True), "the human mark no longer hides the AI state"
    assert display("none", "waiting") == ("ai:need", None, True)
    assert display("none", "done") == ("ai:done", None, True)
    assert display("none", "") == ("ai:idle", None, False)


def test_activity_changes_repaint_the_same_indicator_without_a_mark():
    ns, _, _, image = load([])
    ns["STATE_CACHE"]["sess"] = "working"
    ns["_paint_all_tab_marks"]()
    assert image.pixbuf == "pix:ai:work:None:0" and "Trabajando" in image.tooltip
    ns["STATE_CACHE"]["sess"] = "waiting"
    ns["_paint_all_tab_marks"]()
    assert image.pixbuf == "pix:ai:need:None:0" and "Te necesita" in image.tooltip


def test_set_sends_known_revision_and_adopts_a_newer_mark():
    newer = {"scope": "session", "key": "sess", "mark": "awaiting_reply", "favorite": False, "revision": 5}
    ns, posted, popups, image = load([(409, {"error": "Revisión desactualizada", "current": newer})])
    ns["WORK_MARKS"]["rows"][("session", "sess")] = {**newer, "mark": "none", "revision": 4}
    ns["set_work_mark"]("session", "sess", "resolved")
    assert posted == [{"scope": "session", "key": "sess", "value": "resolved", "expectedRevision": 4}]
    assert ns["work_mark_row"]("session", "sess")["mark"] == "awaiting_reply"
    assert ns["tab_hb"](None)._sticker.label == "Esperando" and popups and "Otro dispositivo" in popups[0]


def test_set_success_updates_the_label():
    ok = {"scope": "session", "key": "sess", "mark": "resolved", "favorite": False, "revision": 1}
    ns, posted, popups, image = load([(200, {"mark": ok})])
    ns["set_work_mark"]("session", "sess", "resolved")
    assert posted[0]["expectedRevision"] == 0 and not popups and ns["tab_hb"](None)._sticker.label == "Hecho"
    writes = image.writes
    ns["_paint_all_tab_marks"]()
    assert image.writes == writes, "an unchanged indicator is not rewritten"


def test_context_menu_offers_tab_and_identified_pane_only():
    ns, posted, _, _ = load([(200, {"mark": {"scope": "pane", "key": "pk1", "mark": "frozen",
                                             "favorite": False, "revision": 1}})])
    ns["WORK_MARKS"]["panes"] = [{"paneKey": "pk1", "session": "sess", "paneId": "%3"}]
    menu = Widget()
    ns["append_work_mark_menu"](menu, "sess", "%3")
    root = menu.children[0]
    labels = [c.label for c in root.submenu.children]
    assert labels == ["Esta pestaña", "Sin marca", "Resuelto", "Congelado", "Esperando respuesta", "-",
                      "Este pane", "Sin marca", "Resuelto", "Congelado", "Esperando respuesta", "Favorito"]
    frozen_pane = root.submenu.children[9]
    frozen_pane.handlers["activate"](frozen_pane)
    assert posted == [{"scope": "pane", "key": "pk1", "value": "frozen", "expectedRevision": 0}]
    other = Widget()
    ns["append_work_mark_menu"](other, "sess", "%8")  # unknown pane: tab scope only
    assert "Este pane" not in [c.label for c in other.children[0].submenu.children]
    none = Widget()
    ns["append_work_mark_menu"](none, "local", None)
    assert none.children == []


def test_header_hourglass_is_the_own_strip_and_follows_the_block():
    """Grill 30-sep: our own 27-frame 32 px strip; the sand is the real time (C)."""
    ns = {"CC_REPO": str(ROOT), "os": __import__("os"), "GdkPixbuf": None, "HOURGLASS_FRAME_PX": 32}
    tree = ast.parse(SOURCE)
    nodes = [n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name in ("hourglass_sheet_frames", "hourglass_frame")]
    assert len(nodes) == 2, "cc-app defines hourglass_sheet_frames and hourglass_frame"
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "<app>", "exec"), ns)
    frames = ns["hourglass_sheet_frames"](864, 32)
    assert len(frames) == 27 and frames[-1] == (832, 0, 32, 32)
    f = ns["hourglass_frame"]
    block = lambda status, active: {"status": status, "targetMs": 25 * 60000, "activeMs": active, "resumedAtMs": 0}
    assert f(None, 0, None) == 0
    assert f(block("paused", 12.5 * 60000), 0, None) == 10
    assert sorted(f(block("running", 5 * 60000), t, None) for t in (0, 450, 900)) == [4, 4, 5]
    assert [f(None, 1000 + d, 1000) for d in (0, 110, 550)] == [21, 22, 26]
    assert (ROOT / "assets" / "pomodoro" / "comandos" / "hourglass.png").is_file()


def test_clicking_the_tab_indicator_opens_the_state_menu_with_icons_and_text():
    """Grill: the indicator is the control; tapping it opens the state menu (icons + labels, no emoji)."""
    ns, posted, _, _ = load([(200, {"mark": {"scope": "session", "key": "sess", "mark": "frozen", "favorite": False, "revision": 1}})])
    menu = ns["tab_indicator_menu"]("sess", None)
    rows = [c for c in menu.children if c.children]          # icon + text rows
    texts = [c.children[0].children[1].label for c in rows]
    assert texts[:4] == ["Sin marca", "Resuelto", "Congelado", "Esperando respuesta"]
    icons = [c.children[0].children[0].pixbuf for c in rows]
    assert icons[:4] == ["pix:none:None:0", "pix:resolved:None:0", "pix:frozen:None:0", "pix:awaiting_reply:None:0"]
    assert rows[0].active is True, "the current state is the checked one"
    rows[2].handlers["activate"](rows[2])
    assert posted == [{"scope": "session", "key": "sess", "value": "frozen", "expectedRevision": 0}]
    assert menu.shown is True


def test_tab_menu_offers_the_tab_favorite_and_opens_with_right_click_on_the_tab():
    """Grill 30-sep: el estado se cambia fácil (clic derecho en toda la pestaña)
    y el menú de la pestaña también marca la favorita (antes solo la del pane)."""
    ns, posted, _, _ = load([])
    toggled = []
    ns["TAB_FAVORITES"] = {"sess"}
    ns["toggle_tab_favorite"] = toggled.append
    menu = ns["tab_indicator_menu"]("sess", None)
    rows = [c for c in menu.children if c.children]
    fav = [r for r in rows if r.children[0].children[1].label == "Favorita"]
    assert fav and fav[0].active is True, "the tab favourite is in the tab menu and shows its state"
    fav[0].handlers["activate"](fav[0])
    assert toggled == ["sess"] and posted == []
    assert 'if ev.button == 3 and key and key != "local":' in SOURCE, "right click anywhere on the tab name opens it"
