"""Own tab strip (30-sep): tabs as wide as their content, same notebook API."""
import os
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))


def test_app_uses_the_own_strip_and_the_prototype_css():
    src = (ROOT / "bin" / "cc-app").read_text()
    assert "nb = gtk_tabstrip.TabStripNotebook()" in src
    assert "_ws_column.pack_start(nb.row, False, False, 0)" in src
    css = src.split('APP_CSS = """', 1)[1].split('"""', 1)[0]
    assert ".strip-tab.cur { color: @TEXT@;" in css and "box-shadow: inset 0 -2px 0 0 @BRAND@" in css
    assert ".strip-tab:hover button, .strip-tab button.favorite" in css


@pytest.mark.skipif(not os.environ.get("DISPLAY"), reason="needs a display for GTK")
def test_strip_tracks_pages_and_measures_content():
    gi = pytest.importorskip("gi")
    gi.require_version("Gtk", "3.0")
    from gi.repository import Gtk
    import gtk_tabstrip
    nb = gtk_tabstrip.TabStripNotebook()
    col = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
    col.pack_start(nb.row, False, False, 0)
    col.pack_start(nb, True, True, 0)
    win = Gtk.OffscreenWindow(); win.add(col); win.set_default_size(600, 200); win.show_all()
    pages = [Gtk.Label(label=str(i)) for i in range(10)]
    labels = [Gtk.Label(label=f"Tab {i}") for i in range(10)]
    for p, l in zip(pages, labels):
        nb.append_page(p, l); p.show()
    while Gtk.events_pending():
        Gtk.main_iteration()
    items = nb.strip.get_children()
    assert [i.get_child() for i in items] == labels
    assert all(i.get_allocation().width == i.get_preferred_width()[1] for i in items), "no stretched tabs"
    nb.reorder_child(pages[0], 4)
    assert nb.strip.get_children()[4].get_child() is labels[0]
    lab = nb.get_tab_label(pages[2]); nb.remove_page(nb.page_num(pages[2]))
    assert lab.get_parent() is None and len(nb.strip.get_children()) == 9
    nb.set_current_page(nb.page_num(pages[9]))
    while Gtk.events_pending():
        Gtk.main_iteration()
    assert [i.get_child() for i in nb.strip.get_children() if i.get_style_context().has_class("cur")] == [labels[9]]
