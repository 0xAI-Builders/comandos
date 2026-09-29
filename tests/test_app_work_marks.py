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
         "set_work_mark", "append_work_mark_menu"]


class Widget:
    def __init__(self, label=None):
        self.label, self.children, self.active, self.sensitive = label, [], None, True
        self.handlers, self.submenu, self.shown, self.pixbuf, self.tooltip = {}, None, None, None, None

    def append(self, child): self.children.append(child)
    def set_sensitive(self, v): self.sensitive = v
    def set_active(self, v): self.active = v
    def set_draw_as_radio(self, v): pass
    def connect(self, signal, cb): self.handlers[signal] = cb
    def set_submenu(self, m): self.submenu = m
    def hide(self): self.shown = False
    def show(self): self.shown = True
    def set_from_pixbuf(self, p): self.pixbuf = p
    def set_tooltip_text(self, t): self.tooltip = t


def fake_gtk():
    return SimpleNamespace(Menu=Widget, MenuItem=lambda label=None: Widget(label),
                           CheckMenuItem=lambda label=None: Widget(label), SeparatorMenuItem=lambda: Widget("-"))


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
    }
    label._work_mark = Widget()
    tree = ast.parse(SOURCE)
    nodes = [n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name in NAMES]
    assert len(nodes) == len(NAMES)
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "<app>", "exec"), ns)
    return ns, posted, popups, label._work_mark


def test_marks_from_the_endpoint_paint_the_tab_label():
    ns, _, _, image = load([])
    ns["apply_work_marks"]({"marks": [{"scope": "session", "key": "sess", "mark": "frozen", "favorite": False,
                                       "revision": 2}], "panes": []})
    assert image.shown is True and image.pixbuf == "pix:frozen" and "Congelado" in image.tooltip
    ns["apply_work_marks"]({"marks": [], "panes": []})
    assert image.shown is False, "without a mark the tab shows no icon"


def test_set_sends_known_revision_and_adopts_a_newer_mark():
    newer = {"scope": "session", "key": "sess", "mark": "awaiting_reply", "favorite": False, "revision": 5}
    ns, posted, popups, image = load([(409, {"error": "Revisión desactualizada", "current": newer})])
    ns["WORK_MARKS"]["rows"][("session", "sess")] = {**newer, "mark": "none", "revision": 4}
    ns["set_work_mark"]("session", "sess", "resolved")
    assert posted == [{"scope": "session", "key": "sess", "value": "resolved", "expectedRevision": 4}]
    assert ns["work_mark_row"]("session", "sess")["mark"] == "awaiting_reply"
    assert image.pixbuf == "pix:awaiting_reply" and popups and "Otro dispositivo" in popups[0]


def test_set_success_updates_the_label():
    ok = {"scope": "session", "key": "sess", "mark": "resolved", "favorite": False, "revision": 1}
    ns, posted, popups, image = load([(200, {"mark": ok})])
    ns["set_work_mark"]("session", "sess", "resolved")
    assert posted[0]["expectedRevision"] == 0 and not popups and image.pixbuf == "pix:resolved"


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
