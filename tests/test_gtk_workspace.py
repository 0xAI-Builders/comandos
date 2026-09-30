"""Desktop docking: pure geometry/tree helpers, plus an offscreen GTK bench.

The GTK part never opens a visible window and never touches the user's tmux:
terminals are real Vte widgets running `sleep` in a temporary directory.
"""
import os
from pathlib import Path
import sys

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
# The QA venv has no PyGObject: borrow the system binding (same CPython 3.10),
# appended so it never shadows the venv's own packages.
if "/usr/lib/python3/dist-packages" not in sys.path:
    sys.path.append("/usr/lib/python3/dist-packages")
import gtk_workspace as gw  # noqa: E402


def leaf(t):
    return {"type": "tab", "tabId": t}


def split(a, b, axis="x", ratio=0.5):
    return {"type": "split", "axis": axis, "ratio": ratio, "first": a, "second": b}


# ---- pure helpers -----------------------------------------------------------

def test_prune_keeps_only_present_tabs_and_collapses():
    tree = split(leaf("a"), split(leaf("b"), leaf("c"), "y", 0.3), "x", 0.6)
    assert gw.prune(tree, {"a", "c"}) == split(leaf("a"), leaf("c"), "x", 0.6)
    assert gw.prune(tree, {"b"}) == leaf("b")
    assert gw.prune(tree, set()) is None


def test_shape_ignores_ratios():
    a = split(leaf("a"), leaf("b"), "x", 0.3)
    assert gw.shape(a) == gw.shape(split(leaf("a"), leaf("b"), "x", 0.7))
    assert gw.shape(a) != gw.shape(split(leaf("a"), leaf("b"), "y", 0.3))


def test_leaf_paths():
    tree = split(leaf("a"), split(leaf("b"), leaf("c"), "y"), "x")
    assert gw.split_paths(tree) == [[], ["second"]]


R = lambda x, y, w, h: (x, y, w, h)  # noqa: E731


def layout():
    return {
        "strip": R(0, 0, 1000, 40),
        "entries": [("group:g1", R(0, 0, 120, 40)), ("group:g2", R(120, 0, 120, 40)), ("group:g3", R(240, 0, 120, 40))],
        "area": R(0, 40, 1000, 600),
        "leaves": {"a": R(0, 40, 500, 600), "b": R(500, 40, 500, 600)},
        "active": "g1",
        "activeTabs": ["a", "b"],
    }


def test_drop_on_strip_before_an_entry():
    t = gw.dock_target(layout(), 125, 20, ["c"])
    assert t["kind"] == "bar" and t["index"] == 1


def test_drop_on_strip_after_last_entry():
    t = gw.dock_target(layout(), 900, 20, ["c"])
    assert t["kind"] == "bar" and t["index"] == 3


def test_drop_near_a_leaf_edge_docks_beside_it():
    t = gw.dock_target(layout(), 960, 300, ["c"])
    assert t == {"kind": "dock", "target": "b", "edge": "right", "rect": (750, 40, 250, 600)}


def test_drop_in_leaf_centre_docks_beside_its_nearest_edge():
    """Fix 4 (30-sep): dropping anywhere on a pane places the tab next to it."""
    t = gw.dock_target(layout(), 750, 330, ["c"])
    assert t["kind"] == "dock" and t["target"] == "b" and t["edge"] == "top"
    assert gw.dock_target(layout(), 700, 340, ["c"])["edge"] == "left"


def test_outer_edge_wraps_the_active_group():
    t = gw.dock_target(layout(), 500, 635, ["c"])
    assert t["kind"] == "dock" and t["target"] == "group:g1" and t["edge"] == "bottom"


def test_dropping_on_itself_is_not_a_target():
    assert gw.dock_target(layout(), 960, 300, ["b"]) is None
    # A whole group cannot wrap itself.
    assert gw.dock_target(layout(), 500, 635, ["a", "b"]) is None


# ---- offscreen GTK bench ----------------------------------------------------

gi = pytest.importorskip("gi")


@pytest.fixture(scope="module")
def gtk():
    if not os.environ.get("DISPLAY") and not os.environ.get("WAYLAND_DISPLAY"):
        pytest.skip("GTK needs a display connection (no window is shown)")
    gi.require_version("Gtk", "3.0")
    gi.require_version("Vte", "2.91")
    from gi.repository import Gtk, GLib, Vte
    return Gtk, GLib, Vte


def pump(Gtk, seconds=0.4):
    """Run the main loop long enough for deferred size allocation."""
    import time
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        while Gtk.events_pending():
            Gtk.main_iteration_do(False)
        time.sleep(0.01)


@pytest.fixture
def bench(gtk, tmp_path):
    Gtk, GLib, Vte = gtk
    win = Gtk.OffscreenWindow()
    win.set_default_size(1200, 700)
    nb = Gtk.Notebook()
    win.add(nb)
    widgets, pids = {}, {}
    for key in ("a", "b", "c", "d"):
        box = Gtk.Box()
        term = Vte.Terminal()
        box.pack_start(term, True, True, 0)
        box._key, box._term, box._label = key, term, key.upper()
        ok, pid = term.spawn_sync(Vte.PtyFlags.DEFAULT, str(tmp_path), ["/bin/sleep", "60"], [], GLib.SpawnFlags.DEFAULT, None, None)
        pids[key] = pid
        widgets[key] = box
        nb.append_page(box, Gtk.Label(label=key))
    view = gw.WorkspaceView(nb, widgets.get, lambda key, text: Gtk.Label(label=text))
    win.show_all()
    pump(Gtk)
    yield Gtk, nb, view, widgets, pids, win
    for pid in pids.values():
        try:
            os.kill(pid, 9)
        except OSError:
            pass
    win.destroy()


def doc(*groups):
    tabs = {}
    for g in groups:
        for t in gw.tab_ids(g["tree"]):
            tabs[t] = {"session": t, "label": t.upper(), "paneKeys": []}
    return {"schema": 1, "groups": list(groups), "tabs": tabs}


def pages(nb):
    return [nb.get_nth_page(i) for i in range(nb.get_n_pages())]


def test_group_becomes_one_page_with_live_terminals(bench):
    Gtk, nb, view, widgets, pids, _ = bench
    view.apply(doc({"id": "g1", "tree": split(leaf("a"), split(leaf("b"), leaf("c"), "y"), "x", 0.6)},
                   {"id": "g2", "tree": leaf("d")}))
    pump(Gtk)
    ps = pages(nb)
    assert len(ps) == 2
    assert isinstance(ps[0], gw.GroupPage) and ps[1] is widgets["d"]
    assert set(ps[0].members) == {"a", "b", "c"}
    # Terminals are reparented, never recreated: same widget, same child process.
    for key in "abc":
        assert widgets[key].get_ancestor(gw.GroupPage) is ps[0]
        os.kill(pids[key], 0)
    assert ps[0]._key in {"a", "b", "c"} and ps[0]._term is widgets[ps[0]._key]._term
    root = ps[0].root
    assert isinstance(root, Gtk.Paned) and root.get_orientation() == Gtk.Orientation.HORIZONTAL
    assert abs(root.get_position() / root.get_allocated_width() - 0.6) < 0.05


def test_ungrouping_restores_plain_pages_in_document_order(bench):
    Gtk, nb, view, widgets, pids, _ = bench
    view.apply(doc({"id": "g1", "tree": split(leaf("a"), leaf("b"))}, {"id": "g2", "tree": leaf("c")}, {"id": "g3", "tree": leaf("d")}))
    pump(Gtk)
    view.apply(doc({"id": "g2", "tree": leaf("c")}, {"id": "g-b", "tree": leaf("b")}, {"id": "g1", "tree": leaf("a")}, {"id": "g3", "tree": leaf("d")}))
    pump(Gtk)
    assert pages(nb) == [widgets["c"], widgets["b"], widgets["a"], widgets["d"]]
    assert nb.get_tab_label(widgets["b"]) is not None
    for key in "abcd":
        os.kill(pids[key], 0)


def test_unknown_tabs_are_kept_as_trailing_pages(bench):
    Gtk, nb, view, widgets, pids, _ = bench
    view.apply(doc({"id": "g1", "tree": split(leaf("a"), leaf("zz"))}, {"id": "g2", "tree": leaf("b")}))
    pump(Gtk)
    ps = pages(nb)
    # "zz" is not open on this desktop: the group degrades to its present tab.
    assert ps[:2] == [widgets["a"], widgets["b"]]
    assert set(ps[2:]) == {widgets["c"], widgets["d"]}


def test_same_shape_updates_ratio_without_rebuilding(bench):
    Gtk, nb, view, widgets, pids, _ = bench
    view.apply(doc({"id": "g1", "tree": split(leaf("a"), leaf("b"), "x", 0.3)}, {"id": "g2", "tree": leaf("c")}, {"id": "g3", "tree": leaf("d")}))
    pump(Gtk)
    page = pages(nb)[0]
    root = page.root
    view.apply(doc({"id": "g1", "tree": split(leaf("a"), leaf("b"), "x", 0.7)}, {"id": "g2", "tree": leaf("c")}, {"id": "g3", "tree": leaf("d")}))
    pump(Gtk)
    assert page.root is root
    assert abs(root.get_position() / root.get_allocated_width() - 0.7) < 0.05


def test_user_resize_reports_the_split_path(bench):
    Gtk, nb, view, widgets, pids, _ = bench
    seen = []
    view.on_resize = lambda gid, path, ratio: seen.append((gid, path, round(ratio, 2)))
    view.apply(doc({"id": "g1", "tree": split(leaf("a"), split(leaf("b"), leaf("c"), "y"), "x")}, {"id": "g2", "tree": leaf("d")}))
    pump(Gtk)
    inner = pages(nb)[0].root.get_child2()
    height = inner.get_allocated_height()
    inner.set_position(int(height * 0.25))
    pump(Gtk)
    view.flush_resize()
    assert seen and seen[-1][0] == "g1" and seen[-1][1] == ["second"] and abs(seen[-1][2] - 0.25) < 0.05


def test_select_reveals_the_group_and_focus_member(bench):
    Gtk, nb, view, widgets, pids, _ = bench
    view.apply(doc({"id": "g2", "tree": leaf("d")}, {"id": "g1", "tree": split(leaf("a"), leaf("b"))}, {"id": "g3", "tree": leaf("c")}))
    pump(Gtk)
    view.select("b")
    pump(Gtk)
    page = nb.get_nth_page(nb.get_current_page())
    assert isinstance(page, gw.GroupPage) and page._key == "b"
    assert view.page_of(widgets["b"]) is page
