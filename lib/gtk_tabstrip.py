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

Dos acomodos (1-oct-2026, pref ``tabs_layout``): «una fila» (``set_rows(False)``,
la de siempre, con desplazamiento) y «varias filas» (``set_rows(True)``): las
mismas pestañas repartidas en renglones según su ancho natural (como el
``flex-wrap`` del remoto; GtkFlowBox no sirve: alinea en columnas de igual ancho).
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
        # Varias filas: renglones (Box horizontales) dentro de una columna; se
        # intercambia con el scroller en la fila según el acomodo elegido. Va en
        # su propio ScrolledWindow (sin barras) para que un renglón ancho nunca
        # empuje el ancho mínimo de la ventana.
        self.flow = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        self.flow.get_style_context().add_class("tabstrip-tabs")
        self.flow.get_style_context().add_class("tabstrip-rows")
        for side in ("start", "end", "top", "bottom"):
            getattr(self.flow, "set_margin_" + side)(6)
        self.rows_view = Gtk.ScrolledWindow()
        self.rows_view.set_policy(Gtk.PolicyType.EXTERNAL, Gtk.PolicyType.NEVER)
        self.rows_view.set_propagate_natural_height(True)
        self.rows_view.add(self.flow)
        self.rows_view.set_hexpand(True)
        self.rows_view.set_valign(Gtk.Align.START)
        self.rows_view.set_no_show_all(True)
        self.rows_view.get_style_context().add_class("tabstrip-rows")
        self._wrap = []          # pestañas en orden mientras está en varias filas
        self._wrap_sig = None    # (ancho, anchos naturales) del último reparto
        self._wrap_idle = None
        # La altura de las filas solo crece (1-oct): si un cambio de etiqueta
        # (modelo, «+1», sticker) reacomodara las filas, la zona de terminales
        # cambiaría de alto, tmux redimensionaría todos los panes y cada TUI se
        # redibujaría entera. Se recalcula solo al alternar o al abrir/cerrar pestañas.
        self._rows_h = 0
        self.rows_view.connect("size-allocate", self._on_rows_allocate)
        self.rows = False
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
        self.row.pack_start(self.rows_view, True, True, 0)
        self.row.pack_start(self._end, False, False, 0)
        self.row.show_all()
        self.rows_view.hide()
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
            self._inset(tab_label)
            item.add(tab_label)
            tab_label.show()

    def set_action_widget(self, widget, pack_type):
        box = self._start if pack_type == Gtk.PackType.START else self._end
        for old in box.get_children():
            box.remove(old)
        box.pack_start(widget, False, False, 0)
        widget.show()

    # ---- acomodo: una fila ⇄ varias filas ------------------------------------
    def set_rows(self, rows):
        """``True``: todas las pestañas a la vista en filas; ``False``: una fila con desplazamiento."""
        rows = bool(rows)
        if rows == self.rows:
            return
        self.rows = rows
        self._release_rows_height()
        order = [self._items[p] for p in self._pages_in_order() if p in self._items]
        for item in order:
            self._detach_item(item)
        for i, item in enumerate(order):
            self._attach_item(item, i)
        ctx = self.row.get_style_context()
        (ctx.add_class if rows else ctx.remove_class)("rows")
        # ☐ y >_ + ⇅ ⊞ se quedan en la banda de 44 px de arriba (como el remoto,
        # align-self:flex-start); si llenaran el alto de las filas se verían gigantes.
        for box in (self._start, self._end):
            box.set_valign(Gtk.Align.START if rows else Gtk.Align.FILL)
            box.set_size_request(-1, 44 if rows else -1)
        if rows:
            self.scroller.hide()
            self.rows_view.show()
            self.flow.show()
        else:
            self.rows_view.hide()
            self.scroller.show()
        self._paint_current()

    def _on_rows_allocate(self, _view, alloc):
        if not self.rows:
            return
        if alloc.height > self._rows_h:
            self._rows_h = alloc.height
            self.rows_view.set_size_request(-1, alloc.height)
        if self._wrap_signature(alloc.width) != self._wrap_sig and self._wrap_idle is None:
            self._wrap_idle = GLib.idle_add(self._reflow)

    def _release_rows_height(self):
        self._rows_h = 0
        self.rows_view.set_size_request(-1, -1)

    def _wrap_signature(self, width):
        return (width, tuple(i.get_preferred_width()[1] for i in self._wrap))

    def _reflow(self):
        """Reparte las pestañas en renglones por su ancho natural (flex-wrap)."""
        self._wrap_idle = None
        width = self.rows_view.get_allocated_width() - 12
        if self.rows and width <= 1:
            return False
        for line in self.flow.get_children():
            for item in line.get_children():
                line.remove(item)
            self.flow.remove(line)
            line.destroy()
        if not self.rows:
            return False
        self._wrap_sig = self._wrap_signature(width + 12)
        line, used = None, 0
        for item in self._wrap:
            w = item.get_preferred_width()[1]
            if line is None or (used and used + 4 + w > width):
                line = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
                self.flow.pack_start(line, False, False, 0)
                line.show()
                used = 0
            line.pack_start(item, False, False, 0)
            item.show()
            used += (4 if used else 0) + w
        return False

    def _reflow_soon(self):
        self._wrap_sig = None
        if self._wrap_idle is None:
            self._wrap_idle = GLib.idle_add(self._reflow)

    def rows_items(self):
        """Pestañas en el orden en que se ven en varias filas (renglón a renglón)."""
        return [i for line in self.flow.get_children() for i in line.get_children()]

    def _pages_in_order(self):
        return [self.get_nth_page(i) for i in range(self.get_n_pages())]

    def _attach_item(self, item, position):
        # Chip de varias filas = remoto: 28 px de alto y 10 px de aire. GTK3: el
        # EventBox ignora min-height/padding de CSS, así que va por código.
        item.set_size_request(-1, 28 if self.rows else -1)
        label = item.get_child()
        if label is not None:
            label.set_margin_start(10 if self.rows else 12)
            label.set_margin_end(10 if self.rows else 12)
        if self.rows:
            self._wrap.insert(max(0, min(position, len(self._wrap))) if position >= 0 else len(self._wrap), item)
            self._reflow_soon()
        else:
            self.strip.pack_start(item, False, False, 0)
            self.strip.reorder_child(item, position)
        item.show()

    def _detach_item(self, item):
        if item in self._wrap:
            self._wrap.remove(item)
            self._reflow_soon()
        parent = item.get_parent()
        if parent is not None:
            parent.remove(item)

    # ---- sincronía con las páginas ----------------------------------------
    @staticmethod
    def _inset(label):
        # GTK3: un EventBox no aplica «padding» de CSS; el aire de 12 px del
        # prototipo va como margen de la etiqueta.
        label.set_margin_start(12)
        label.set_margin_end(12)

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
        self._inset(label)
        item.add(label)
        self._labels[page] = label
        self._items[page] = item
        if self.rows:
            self._release_rows_height()
        self._attach_item(item, num)
        label.show()
        self._paint_current()

    def _on_removed(self, _nb, page, _num):
        item = self._items.pop(page, None)
        label = self._labels.get(page)
        if item is not None:
            if label is not None and label.get_parent() is item:
                item.remove(label)
            self._detach_item(item)
            item.destroy()
            if self.rows:
                self._release_rows_height()
        # la etiqueta se conserva: gtk_workspace la reutiliza al volver la página

    def _on_reordered(self, _nb, page, num):
        item = self._items.get(page)
        if item is None:
            return
        if self.rows:
            self._detach_item(item)
            self._attach_item(item, num)
        else:
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
        if item is not None and not self.rows:
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
        if self.rows:
            return
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
