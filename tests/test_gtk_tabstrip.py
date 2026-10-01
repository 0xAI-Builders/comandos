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


@pytest.mark.skipif(not os.environ.get("DISPLAY"), reason="needs a display for GTK")
def test_rows_mode_wraps_every_tab_and_back_keeps_order():
    """1-oct: ⊞ «varias filas» envuelve las mismas pestañas (todas a la vista) y
    al volver a una fila quedan en el mismo orden, con altas y bajas por medio."""
    gi = pytest.importorskip("gi")
    gi.require_version("Gtk", "3.0")
    from gi.repository import Gtk
    import gtk_tabstrip
    nb = gtk_tabstrip.TabStripNotebook()
    col = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
    col.pack_start(nb.row, False, False, 0)
    col.pack_start(nb, True, True, 0)
    win = Gtk.OffscreenWindow(); win.add(col); win.set_default_size(500, 300); win.show_all()

    import time

    def pump(seconds=0.3):            # el FlowBox se asigna en el ciclo de layout, no en el primer pending
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            while Gtk.events_pending():
                Gtk.main_iteration()
            time.sleep(0.01)

    pages = [Gtk.Label(label=str(i)) for i in range(12)]
    labels = [Gtk.Label(label=f"Sesión larga {i}") for i in range(12)]
    for p, l in zip(pages, labels):
        nb.append_page(p, l); p.show()
    pump()
    one_row_h = nb.row.get_allocated_height()
    nb.set_rows(True)
    pump()
    cells = nb.rows_items()
    assert [c.get_child() for c in cells] == labels, "same tabs, same order"
    assert not nb.strip.get_children() and nb.rows_view.get_visible() and not nb.scroller.get_visible()
    assert nb.row.get_allocated_height() > one_row_h, "wraps into several rows"
    lines = nb.flow.get_children()
    assert len(lines) > 1
    # flex-wrap, no columnas: cada pestaña mide su contenido y van pegadas (4 px).
    first = lines[0].get_children()
    assert all(i.get_allocated_width() == i.get_preferred_width()[1] for i in cells), "natural width, never stretched"
    assert all(i.get_allocated_height() == 28 for i in cells), "28 px chips, like the remote"
    assert all(b.get_allocation().x - (a.get_allocation().x + a.get_allocated_width()) == 4 for a, b in zip(first, first[1:]))
    view_w = nb.rows_view.get_allocated_width()
    assert all(sum(i.get_allocated_width() for i in ln.get_children()) + 4 * (len(ln.get_children()) - 1) <= view_w - 12
               for ln in lines), "every row fits the strip"
    # Una etiqueta que cambia (modelo, «+1») no debe mover la altura: si bajara,
    # tmux redimensionaría todos los panes y cada TUI se redibujaría entera.
    held = nb.row.get_allocated_height()
    for l in labels:
        l.set_text("x")
    pump()
    assert nb.row.get_allocated_height() == held, "rows height only grows while the tab count is the same"
    for i, l in enumerate(labels):
        l.set_text(f"Sesión larga {i}")
    pump()
    extra, extra_lbl = Gtk.Label(label="x"), Gtk.Label(label="Nueva")
    nb.insert_page(extra, extra_lbl, 0); extra.show()
    nb.reorder_child(pages[5], 1)
    nb.remove_page(nb.page_num(pages[11]))
    pump()
    assert nb._rows_h <= held + 30, "opening/closing tabs recomputes the height"
    order = [nb.get_tab_label(nb.get_nth_page(i)) for i in range(nb.get_n_pages())]
    assert [c.get_child() for c in nb.rows_items()] == order
    nb.set_rows(False)
    pump()
    assert [i.get_child() for i in nb.strip.get_children()] == order and not nb.rows_items()
    assert nb.scroller.get_visible() and not nb.rows_view.get_visible()
    nb.set_current_page(3)
    pump()
    cur = [i for i in nb.strip.get_children() if i.get_style_context().has_class("cur")]
    assert [i.get_child() for i in cur] == [order[3]]
