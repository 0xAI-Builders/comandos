"""Barra de pestañas propia para el escritorio (fase 1, 30-sep-2026).

GtkNotebook, cuando hay más pestañas de las que caben, reparte el ancho sobrante
entre las visibles y no hay propiedad que lo apague: las pestañas salían del
mismo ancho y con un hueco vacío, lejos del prototipo aprobado. Este notebook
oculta su cabecera nativa y pinta la suya: cada pestaña mide exactamente su
contenido, ☆/✕ aparecen al pasar el cursor, y se desplaza con la rueda.

La API del resto de la app no cambia: ``append_page``/``insert_page`` reciben la
etiqueta, ``get_tab_label``/``set_tab_label`` la devuelven y la sustituyen, y las
señales de GtkNotebook (switch-page, page-added/removed/reordered) mantienen la
barra al día aunque las páginas se muevan desde gtk_workspace.
"""
import gi

gi.require_version("Gtk", "3.0")
from gi.repository import Gdk, GLib, Gtk  # noqa: E402


class TabStripNotebook(Gtk.Notebook):
    def __init__(self):
        super().__init__()
        self.set_show_tabs(False)
        self.set_show_border(False)
        self._labels = {}        # página -> widget etiqueta
        self._items = {}         # página -> EventBox de la barra
        self._pending = {}       # página -> etiqueta mientras GTK la añade
        self.strip = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        self.strip.get_style_context().add_class("tabstrip-tabs")
        self.scroller = Gtk.ScrolledWindow()
        self.scroller.set_policy(Gtk.PolicyType.EXTERNAL, Gtk.PolicyType.NEVER)
        self.scroller.set_overlay_scrolling(True)
        self.scroller.add(self.strip)
        self.scroller.set_hexpand(True)
        self.scroller.add_events(Gdk.EventMask.SCROLL_MASK | Gdk.EventMask.SMOOTH_SCROLL_MASK)
        self.scroller.connect("scroll-event", self._on_wheel)
        self.row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        self.row.get_style_context().add_class("tabstrip")
        self._start = Gtk.Box()
        self._end = Gtk.Box()
        self.row.pack_start(self._start, False, False, 0)
        self.row.pack_start(self.scroller, True, True, 0)
        self.row.pack_start(self._end, False, False, 0)
        self.row.show_all()
        self.connect("page-added", self._on_added)
        self.connect("page-removed", self._on_removed)
        self.connect("page-reordered", self._on_reordered)
        self.connect("switch-page", self._on_switch)

    # ---- API de GtkNotebook que usa la app -------------------------------
    def append_page(self, child, tab_label=None):
        self._pending[child] = tab_label
        return Gtk.Notebook.append_page(self, child, None)

    def insert_page(self, child, tab_label=None, position=-1):
        self._pending[child] = tab_label
        return Gtk.Notebook.insert_page(self, child, None, position)

    def get_tab_label(self, child):
        return self._labels.get(child)

    def set_tab_label(self, child, tab_label=None):
        item = self._items.get(child)
        old = self._labels.get(child)
        if item is not None and old is not None and old.get_parent() is item:
            item.remove(old)
        self._labels[child] = tab_label
        if item is not None and tab_label is not None:
            item.add(tab_label)
            tab_label.show()

    def set_action_widget(self, widget, pack_type):
        box = self._start if pack_type == Gtk.PackType.START else self._end
        for old in box.get_children():
            box.remove(old)
        box.pack_start(widget, False, False, 0)
        widget.show()

    # ---- sincronía con las páginas ----------------------------------------
    def _make_item(self, page):
        item = Gtk.EventBox()
        item.set_visible_window(True)
        item.get_style_context().add_class("strip-tab")
        item.add_events(Gdk.EventMask.ENTER_NOTIFY_MASK | Gdk.EventMask.LEAVE_NOTIFY_MASK
                        | Gdk.EventMask.BUTTON_PRESS_MASK)
        item.connect("enter-notify-event", lambda w, e: (w.set_state_flags(Gtk.StateFlags.PRELIGHT, False), False)[1])
        item.connect("leave-notify-event", self._on_leave)
        item.connect("button-press-event", self._on_press, page)
        return item

    def _on_leave(self, item, event):
        if event.detail != Gdk.NotifyType.INFERIOR:
            item.unset_state_flags(Gtk.StateFlags.PRELIGHT)
        return False

    def _on_press(self, _item, event, page):
        if event.button == 1 and event.type == Gdk.EventType.BUTTON_PRESS:
            num = self.page_num(page)
            if num >= 0 and num != self.get_current_page():
                self.set_current_page(num)
        return False             # la etiqueta (arrastre, renombrar, menú) sigue recibiendo el clic

    def _on_added(self, _nb, page, num):
        label = self._pending.pop(page, None)
        if label is None:
            label = self._labels.get(page) or Gtk.Label(label="")
        item = self._make_item(page)
        parent = label.get_parent()
        if parent is not None:
            parent.remove(label)
        item.add(label)
        self._labels[page] = label
        self._items[page] = item
        self.strip.pack_start(item, False, False, 0)
        self.strip.reorder_child(item, num)
        item.show()
        label.show()
        self._paint_current()

    def _on_removed(self, _nb, page, _num):
        item = self._items.pop(page, None)
        label = self._labels.get(page)
        if item is not None:
            if label is not None and label.get_parent() is item:
                item.remove(label)
            self.strip.remove(item)
            item.destroy()
        # la etiqueta se conserva: gtk_workspace la reutiliza al volver la página

    def _on_reordered(self, _nb, page, num):
        item = self._items.get(page)
        if item is not None:
            self.strip.reorder_child(item, num)

    def _on_switch(self, _nb, page, _num):
        GLib.idle_add(self._paint_current, page)

    def _paint_current(self, page=None):
        if page is None:
            n = self.get_current_page()
            page = self.get_nth_page(n) if n >= 0 else None
        for p, item in self._items.items():
            ctx = item.get_style_context()
            (ctx.add_class if p is page else ctx.remove_class)("cur")
        item = self._items.get(page)
        if item is not None:
            self._scroll_to(item)
        return False

    def _scroll_to(self, item):
        adj = self.scroller.get_hadjustment()
        alloc = item.get_allocation()
        if alloc.width <= 1:
            GLib.timeout_add(60, lambda: (self._scroll_to(item), False)[1])
            return
        lo, hi = adj.get_value(), adj.get_value() + adj.get_page_size()
        if alloc.x < lo:
            adj.set_value(alloc.x)
        elif alloc.x + alloc.width > hi:
            adj.set_value(alloc.x + alloc.width - adj.get_page_size())

    def scroll_by(self, dx):
        adj = self.scroller.get_hadjustment()
        adj.set_value(max(adj.get_lower(), min(adj.get_upper() - adj.get_page_size(), adj.get_value() + dx)))

    def _on_wheel(self, _w, event):
        ok, dx, dy = event.get_scroll_deltas()
        if ok:
            self.scroll_by((dx or dy) * 40)
        elif event.direction in (Gdk.ScrollDirection.UP, Gdk.ScrollDirection.LEFT):
            self.scroll_by(-80)
        elif event.direction in (Gdk.ScrollDirection.DOWN, Gdk.ScrollDirection.RIGHT):
            self.scroll_by(80)
        return True
