"""Desktop renderer for the shared workspace (GTK 3).

Each workspace group becomes one notebook page. A single-tab group is the tab
widget itself (the historical layout); a multi-tab group is a GroupPage whose
Gtk.Paned tree holds the existing tab widgets. Tab widgets are reparented,
never recreated, so their terminals and child processes keep running.

Pure helpers (prune, shape, split_paths, dock_target) need no display and are
shared with the tests; the GTK classes load only when PyGObject is present.
"""
from workspace_state import tab_ids

EDGE_FRACTION = 0.31
OUTER_PX = 16


# ---- pure helpers ------------------------------------------------------------

def prune(tree, present):
    """``tree`` restricted to ``present`` tab ids; lone children collapse."""
    if tree["type"] == "tab":
        return tree if tree["tabId"] in present else None
    first, second = prune(tree["first"], present), prune(tree["second"], present)
    if first is None:
        return second
    if second is None:
        return first
    return dict(tree, first=first, second=second)


def shape(tree):
    """Structure without ratios: equal shapes can be updated in place."""
    if tree["type"] == "tab":
        return tree["tabId"]
    return (tree["axis"], shape(tree["first"]), shape(tree["second"]))


def split_paths(tree, path=()):
    if tree["type"] == "tab":
        return []
    return [list(path)] + split_paths(tree["first"], path + ("first",)) + split_paths(tree["second"], path + ("second",))


def _inside(rect, x, y):
    rx, ry, rw, rh = rect
    return rx <= x <= rx + rw and ry <= y <= ry + rh


def _nearest(distances):
    return min(distances, key=lambda d: d[1])


def _half(rect, edge):
    x, y, w, h = rect
    if edge == "left":
        return (x, y, w / 2, h)
    if edge == "right":
        return (x + w / 2, y, w / 2, h)
    if edge == "top":
        return (x, y, w, h / 2)
    return (x, y + h / 2, w, h / 2)


def dock_target(layout, x, y, moved):
    """Drop target for a drag of ``moved`` tab ids at (x, y).

    ``layout``: strip rect, ordered strip ``entries`` [(key, rect)], viewport
    ``area`` rect, ``leaves`` {tabId: rect} of the active group, ``active``
    group id and ``activeTabs``. Mirrors dash/workspace-dock.js.
    """
    moved = set(moved)
    if _inside(layout["strip"], x, y):
        index = len(layout["entries"])
        for i, (_key, (ex, _ey, ew, _eh)) in enumerate(layout["entries"]):
            if x < ex + ew / 2:
                index = i
                break
        return {"kind": "bar", "index": index}
    area = layout["area"]
    if not _inside(area, x, y):
        return None
    ax, ay, aw, ah = area
    edge, dist = _nearest([("left", x - ax), ("right", ax + aw - x), ("top", y - ay), ("bottom", ay + ah - y)])
    if dist < OUTER_PX and layout.get("active") and not set(layout.get("activeTabs", ())) <= moved:
        return {"kind": "dock", "target": "group:" + layout["active"], "edge": edge, "rect": _half(area, edge)}
    for tab, rect in layout.get("leaves", {}).items():
        if not _inside(rect, x, y) or tab in moved:
            continue
        rx, ry, rw, rh = rect
        # Soltar en CUALQUIER punto del pane lo coloca junto a él, por el borde
        # más cercano (fix 4, 30-sep): el centro ya no es tierra de nadie.
        edge, _frac = _nearest([("left", (x - rx) / rw), ("right", (rx + rw - x) / rw),
                                ("top", (y - ry) / rh), ("bottom", (ry + rh - y) / rh)])
        return {"kind": "dock", "target": tab, "edge": edge, "rect": _half(rect, edge)}
    return None


# ---- GTK -----------------------------------------------------------------------

try:
    import gi
    gi.require_version("Gtk", "3.0")
    from gi.repository import GLib, Gtk
except (ImportError, ValueError):  # pragma: no cover - headless environments
    Gtk = None


if Gtk is not None:

    def _detach(widget):
        parent = widget.get_parent()
        if parent is not None:
            parent.remove(widget)

    class GroupPage(Gtk.Box):
        """Notebook page of a multi-tab group; proxies the focused member."""

        def __init__(self, group_id, make_header):
            super().__init__(orientation=Gtk.Orientation.VERTICAL)
            self.group_id = group_id
            self.members = {}          # tab id -> tab widget
            self.leaves = {}           # tab id -> leaf wrapper
            self.focus = None
            self.root = None
            self._shape = None
            self._make_header = make_header
            self.get_style_context().add_class("ws-group")

        # Code written for "one page per tab" reads these on the current page.
        @property
        def _key(self):
            return self.focus

        @property
        def _term(self):
            box = self.members.get(self.focus)
            return getattr(box, "_term", None)

        @property
        def _label(self):
            box = self.members.get(self.focus)
            return getattr(box, "_label", self.focus)

        def release(self):
            """Take every member out, leaving the page empty."""
            for key, box in self.members.items():
                leaf = self.leaves.get(key)
                # A member already moved elsewhere (e.g. back to the notebook) stays there.
                if leaf is not None and box.get_parent() is leaf:
                    leaf.remove(box)
            if self.root is not None:
                self.remove(self.root)
            self.members, self.leaves, self.root, self._shape = {}, {}, None, None

        def build(self, tree, widget_for, view):
            if self._shape == shape(tree) and set(self.members) == set(tab_ids(tree)):
                self._apply_ratios(self.root, tree)
                return
            focus = self.focus
            self.release()
            self.root = self._node(tree, widget_for, view, [])
            self.pack_start(self.root, True, True, 0)
            self._shape = shape(tree)
            self.focus = focus if focus in self.members else tab_ids(tree)[0]
            self.show_all()

        def _node(self, node, widget_for, view, path):
            if node["type"] == "tab":
                key = node["tabId"]
                box = widget_for(key)
                _detach(box)
                leaf = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
                leaf.get_style_context().add_class("ws-leaf")
                leaf.pack_start(self._make_header(key, box), False, False, 0)
                leaf.pack_start(box, True, True, 0)
                leaf._tab = key
                self.members[key] = box
                self.leaves[key] = leaf
                return leaf
            paned = Gtk.Paned(orientation=Gtk.Orientation.HORIZONTAL if node["axis"] == "x"
                              else Gtk.Orientation.VERTICAL)
            paned.get_style_context().add_class("ws-paned")
            paned.set_wide_handle(True)
            paned._path, paned._ratio, paned._size, paned._group = list(path), node["ratio"], 0, self.group_id
            paned._programmatic = False
            paned.pack1(self._node(node["first"], widget_for, view, path + ["first"]), True, False)
            paned.pack2(self._node(node["second"], widget_for, view, path + ["second"]), True, False)
            paned.connect("size-allocate", view._paned_allocated)
            paned.connect("notify::position", view._paned_moved)
            return paned

        def _apply_ratios(self, widget, tree):
            if tree["type"] == "tab" or not isinstance(widget, Gtk.Paned):
                return
            if abs(widget._ratio - tree["ratio"]) > 1e-6:
                widget._ratio = tree["ratio"]
                widget._size = 0          # re-apply at the next allocation
                widget.queue_resize()
                _set_ratio(widget)
            self._apply_ratios(widget.get_child1(), tree["first"])
            self._apply_ratios(widget.get_child2(), tree["second"])

    def _paned_size(paned):
        return (paned.get_allocated_width() if paned.get_orientation() == Gtk.Orientation.HORIZONTAL
                else paned.get_allocated_height())

    def _set_ratio(paned):
        size = _paned_size(paned)
        if size <= 1:
            return
        paned._programmatic = True
        paned.set_position(int(round(size * paned._ratio)))
        paned._programmatic = False
        paned._size = size

    class WorkspaceView:
        """Keeps the notebook in the shape of the shared workspace document."""

        def __init__(self, nb, widget_for, make_tab_label, make_header=None):
            self.nb = nb
            self.widget_for = widget_for
            self.make_tab_label = make_tab_label
            self.make_header = make_header or self._default_header
            self.groups = {}            # group id -> GroupPage
            self.doc = None
            self.on_resize = None       # (group_id, path, ratio) -> None
            self._pending_resize = {}
            self._resize_timer = 0

        # -- helpers used by the app -------------------------------------------
        def page_of(self, widget):
            node = widget
            while node is not None and node.get_parent() is not self.nb:
                node = node.get_parent()
            return node

        def select(self, key):
            box = self.widget_for(key)
            if box is None:
                return False
            page = self.page_of(box)
            if page is None:
                return False
            if isinstance(page, GroupPage):
                page.focus = key
                self._mark_focus(page)
            self.nb.set_current_page(self.nb.page_num(page))
            term = getattr(box, "_term", None)
            if term is not None:
                term.grab_focus()
            return True

        def member_keys(self, page):
            """Tab ids of a page in tree order (one for a plain page)."""
            if isinstance(page, GroupPage):
                group = next((g for g in (self.doc or {}).get("groups", []) if g["id"] == page.group_id), None)
                ordered = [k for k in tab_ids(group["tree"])] if group else []
                return [k for k in ordered if k in page.members] or list(page.members)
            key = getattr(page, "_key", None)
            return [key] if isinstance(key, str) else []

        def _mark_focus(self, page):
            for key, leaf in page.leaves.items():
                ctx = leaf.get_style_context()
                (ctx.add_class if key == page.focus else ctx.remove_class)("focused")

        def _default_header(self, key, box):
            lbl = Gtk.Label(label=getattr(box, "_label", key), xalign=0)
            return lbl

        # -- document application ---------------------------------------------
        def _ensure_plain(self, key, box):
            page = self.page_of(box)
            if page is box:
                return box
            _detach(box)
            label = getattr(box, "_tab_label_widget", None)
            if label is None or label.get_parent() is not None:
                label = self.make_tab_label(key, getattr(box, "_label", key))
                box._tab_label_widget = label
            self.nb.append_page(box, label)
            box.show_all()
            return box

        def apply(self, doc):
            """Rebuild pages to match ``doc``; unknown pages stay at the end."""
            self.doc = doc
            current = self.nb.get_nth_page(self.nb.get_current_page())
            current_key = getattr(current, "_key", None)
            wanted = []            # pages in document order
            used_groups = set()
            for group in doc.get("groups", []):
                ids = [t for t in tab_ids(group["tree"]) if self.widget_for(t) is not None]
                if not ids:
                    continue
                tree = prune(group["tree"], set(ids))
                if tree["type"] == "tab":
                    wanted.append(self._ensure_plain(ids[0], self.widget_for(ids[0])))
                    continue
                page = self.groups.get(group["id"])
                if page is None:
                    page = GroupPage(group["id"], self.make_header)
                    self.groups[group["id"]] = page
                for key in ids:   # remember tab labels before their pages go away
                    box = self.widget_for(key)
                    if box.get_parent() is self.nb:
                        box._tab_label_widget = self.nb.get_tab_label(box)
                        self.nb.remove_page(self.nb.page_num(box))
                page.build(tree, self.widget_for, self)
                if page.get_parent() is not self.nb:
                    label = self.make_tab_label("group:" + group["id"], "")
                    page._tab_label_widget = label
                    self.nb.append_page(page, label)
                    page.show_all()
                self._update_group_label(page, ids)
                self._mark_focus(page)
                used_groups.add(group["id"])
                wanted.append(page)
            for gid, page in list(self.groups.items()):
                if gid in used_groups:
                    continue
                for key, box in list(page.members.items()):
                    if box.get_parent() is not None and box.get_ancestor(GroupPage) is page:
                        page.members.pop(key)
                        page.leaves.pop(key, None)
                        _detach(box)
                        self._ensure_plain(key, box)
                page.release()
                num = self.nb.page_num(page)
                if num >= 0:
                    self.nb.remove_page(num)
                del self.groups[gid]
            for index, page in enumerate(wanted):
                if self.nb.page_num(page) != index:
                    self.nb.reorder_child(page, index)
            if isinstance(current_key, str) and not self.select(current_key) and current is not None:
                num = self.nb.page_num(current)
                if num >= 0:
                    self.nb.set_current_page(num)

        def _update_group_label(self, page, ids):
            label = getattr(page, "_tab_label_widget", None)
            first = self.widget_for(ids[0])
            text = f"{getattr(first, '_label', ids[0])}  {len(ids)} tabs"
            setter = getattr(label, "_set_group_text", None)
            if setter:
                setter(text)
            elif isinstance(label, Gtk.Label):
                label.set_text(text)

        # -- paned ratios -------------------------------------------------------
        def _paned_allocated(self, paned, _alloc):
            size = _paned_size(paned)
            if size > 1 and size != paned._size:
                _set_ratio(paned)

        def _paned_moved(self, paned, _pspec):
            if paned._programmatic:
                return
            size = _paned_size(paned)
            if size <= 1 or size != paned._size:
                return   # layout pass, not a user drag
            ratio = min(0.9, max(0.1, paned.get_position() / size))
            if abs(ratio - paned._ratio) < 0.005:
                return
            paned._ratio = ratio
            self._pending_resize[(paned._group, tuple(paned._path))] = ratio
            if self._resize_timer:
                GLib.source_remove(self._resize_timer)
            self._resize_timer = GLib.timeout_add(400, self.flush_resize)

        def flush_resize(self):
            self._resize_timer = 0
            pending, self._pending_resize = self._pending_resize, {}
            for (group_id, path), ratio in pending.items():
                if self.on_resize:
                    self.on_resize(group_id, list(path), ratio)
            return False

else:  # pragma: no cover
    GroupPage = WorkspaceView = None
