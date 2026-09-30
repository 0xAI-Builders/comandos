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

NAMES = ["work_mark_row", "paint_tab_mark", "_paint_all_tab_marks", "apply_work_marks", "adopt_work_mark",
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
    }
    label._work_mark = Widget()
    label._dot = label._work_mark
    label.animated = []
    ns["time"] = SimpleNamespace(monotonic=lambda: 0.0)
    tree = ast.parse(SOURCE)
    nodes = [n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name in NAMES]
    assert len(nodes) == len(NAMES)
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "<app>", "exec"), ns)
    return ns, posted, popups, label._work_mark


def test_marks_from_the_endpoint_paint_the_tab_indicator():
    ns, _, _, image = load([])
    ns["apply_work_marks"]({"marks": [{"scope": "session", "key": "sess", "mark": "frozen", "favorite": False,
                                       "revision": 2}], "panes": []})
    assert image.pixbuf == "pix:frozen:#7CC4FF:0" and "Congelado" in image.tooltip
    assert image.animated == ["frozen"], "a visible mark loops continuously"
    ns["apply_work_marks"]({"marks": [], "panes": []})
    assert image.pixbuf == "pix:none:#4B5568:0", "without a mark the tab shows the neutral ready icon"
    assert image.shown is not False


def test_indicator_precedence_matches_the_web():
    display = load([])[0]["tab_indicator_display"]
    # A human mark wins over activity; activity shows the working ring; otherwise
    # the neutral ring keeps the status colour (waiting stays visible).
    assert display("frozen", "working") == ("frozen", "#7CC4FF", True)
    assert display("none", "working") == ("working", "#7AA5FF", True)
    assert display("none", "waiting") == ("none", "#D08770", False)
    assert display("none", "") == ("none", "#4B5568", False)
    assert display("resolved", "") == ("resolved", "#2EE59D", True)


def test_activity_changes_repaint_the_same_indicator_without_a_mark():
    ns, _, _, image = load([])
    ns["STATE_CACHE"]["sess"] = "working"
    ns["_paint_all_tab_marks"]()
    assert image.pixbuf == "pix:working:#7AA5FF:0" and "Trabajando" in image.tooltip
    ns["STATE_CACHE"]["sess"] = "waiting"
    ns["_paint_all_tab_marks"]()
    assert image.pixbuf == "pix:none:#D08770:0"


def test_set_sends_known_revision_and_adopts_a_newer_mark():
    newer = {"scope": "session", "key": "sess", "mark": "awaiting_reply", "favorite": False, "revision": 5}
    ns, posted, popups, image = load([(409, {"error": "Revisión desactualizada", "current": newer})])
    ns["WORK_MARKS"]["rows"][("session", "sess")] = {**newer, "mark": "none", "revision": 4}
    ns["set_work_mark"]("session", "sess", "resolved")
    assert posted == [{"scope": "session", "key": "sess", "value": "resolved", "expectedRevision": 4}]
    assert ns["work_mark_row"]("session", "sess")["mark"] == "awaiting_reply"
    assert image.pixbuf == "pix:awaiting_reply:#FFAE1A:0" and popups and "Otro dispositivo" in popups[0]


def test_set_success_updates_the_label():
    ok = {"scope": "session", "key": "sess", "mark": "resolved", "favorite": False, "revision": 1}
    ns, posted, popups, image = load([(200, {"mark": ok})])
    ns["set_work_mark"]("session", "sess", "resolved")
    assert posted[0]["expectedRevision"] == 0 and not popups and image.pixbuf == "pix:resolved:#2EE59D:0"
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


def test_header_hourglass_uses_the_fifteen_original_frames():
    """P2: the desktop header shows Zoedoz's animated hourglass (15 × 42 px), not a flat icon."""
    ns = {"CC_REPO": str(ROOT), "os": __import__("os"), "GdkPixbuf": None, "HOURGLASS_FRAME_PX": 42}
    tree = ast.parse(SOURCE)
    nodes = [n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name in ("hourglass_sheet_frames",)]
    assert nodes, "cc-app defines hourglass_sheet_frames"
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "<app>", "exec"), ns)
    frames = ns["hourglass_sheet_frames"](630, 42)
    assert len(frames) == 15 and frames[0] == (0, 0, 42, 42) and frames[-1] == (588, 0, 42, 42)
    assert ns["hourglass_sheet_frames"](630, 42, scale=0.5)[1] == (42, 0, 42, 42), "source rects are native pixels"
    path = ROOT / "assets" / "pomodoro" / "zoedoz" / "hourglass.png"
    assert path.is_file(), "the production asset ships with the app"


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
