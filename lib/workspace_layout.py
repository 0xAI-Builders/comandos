"""Pure docking operations on a workspace document (see workspace_state).

Every operation works on a copy, keeps the exact set of tab ids and raises
ValueError instead of producing a no-op or an invalid arrangement. The
browser mirrors these functions in dash/workspace-layout.js; both are
driven by tests/fixtures/workspace_layout.json.
"""
import copy

from workspace_state import tab_ids, validate_document

EDGES = {"left": ("x", True), "right": ("x", False), "top": ("y", True), "bottom": ("y", False)}
MIN_RATIO, MAX_RATIO = 0.1, 0.9


def _remove(node, ids):
    if node["type"] == "tab":
        return None if node["tabId"] in ids else node
    first, second = _remove(node["first"], ids), _remove(node["second"], ids)
    if first is None:
        return second
    if second is None:
        return first
    return dict(node, first=first, second=second)


def _replace_leaf(node, tab_id, replacement):
    if node["type"] == "tab":
        return replacement if node["tabId"] == tab_id else node
    return dict(node, first=_replace_leaf(node["first"], tab_id, replacement),
                second=_replace_leaf(node["second"], tab_id, replacement))


def _find_leaf(node, tab_id):
    if node["type"] == "tab":
        return node if node["tabId"] == tab_id else None
    return _find_leaf(node["first"], tab_id) or _find_leaf(node["second"], tab_id)


def _source(document, source):
    """Subtree and tab ids named by ``tabId`` or ``group:<id>``."""
    if source.startswith("group:"):
        for group in document["groups"]:
            if group["id"] == source[6:]:
                return group["tree"]
        raise ValueError("Grupo de origen inexistente")
    for group in document["groups"]:
        leaf = _find_leaf(group["tree"], source)
        if leaf:
            return leaf
    raise ValueError("Tab de origen inexistente")


def _without(document, ids):
    groups = []
    for group in document["groups"]:
        tree = _remove(group["tree"], ids)
        if tree is not None:
            groups.append(dict(group, tree=tree))
    return groups


def move_tab(document, source, target_id, edge):
    """Dock ``source`` (tab or ``group:<id>``) at ``edge`` of a tab or whole group."""
    if edge not in EDGES:
        raise ValueError("Borde inválido")
    document = copy.deepcopy(document)
    moved = _source(document, source)
    ids = set(tab_ids(moved))
    groups = _without(document, ids)
    axis, before = EDGES[edge]
    for index, group in enumerate(groups):
        if target_id.startswith("group:"):
            if group["id"] != target_id[6:]:
                continue
            kept = group["tree"]
        else:
            kept = _find_leaf(group["tree"], target_id)
            if kept is None:
                continue
        split = {"type": "split", "axis": axis, "ratio": 0.5,
                 "first": moved if before else kept, "second": kept if before else moved}
        tree = split if target_id.startswith("group:") else _replace_leaf(group["tree"], target_id, split)
        groups[index] = dict(group, tree=tree)
        out = dict(document, groups=groups)
        if out == document:
            raise ValueError("Sin cambios")
        return validate_document(out)
    raise ValueError("Destino inexistente o dentro de lo que se mueve")


def _new_group_id(groups, tab_id):
    used = {g["id"] for g in groups}
    gid, n = "group-" + tab_id, 1
    while gid in used:
        n += 1
        gid = f"group-{tab_id}-{n}"
    return gid


def detach_tab(document, source, index):
    """Put ``source`` in the strip before the entry currently at ``index``."""
    if isinstance(index, bool) or not isinstance(index, int) or index < 0:
        raise ValueError("Posición inválida")
    document = copy.deepcopy(document)
    before = document["groups"][index]["id"] if index < len(document["groups"]) else None
    if source.startswith("group:"):
        entry = next((g for g in document["groups"] if g["id"] == source[6:]), None)
        if entry is None:
            raise ValueError("Grupo de origen inexistente")
        groups = [g for g in document["groups"] if g is not entry]
    else:
        moved = _source(document, source)
        owner = next(g for g in document["groups"] if _find_leaf(g["tree"], source))
        if owner["tree"] is moved:
            entry = owner
            groups = [g for g in document["groups"] if g is not owner]
        else:
            groups = _without(document, {source})
            entry = {"id": _new_group_id(document["groups"], source), "tree": moved}
    at = next((i for i, g in enumerate(groups) if g["id"] == before), len(groups))
    groups.insert(at, entry)
    out = dict(document, groups=groups)
    if out == document:
        raise ValueError("Sin cambios")
    return validate_document(out)


def resize_split(document, group_id, path, ratio):
    """Set the ratio of the split at ``path`` ("first"/"second" steps) of a group."""
    if isinstance(ratio, bool) or not isinstance(ratio, (int, float)):
        raise ValueError("Proporción inválida")
    document = copy.deepcopy(document)
    group = next((g for g in document["groups"] if g["id"] == group_id), None)
    if group is None:
        raise ValueError("Grupo inexistente")
    node = group["tree"]
    for step in path:
        if step not in ("first", "second") or node["type"] != "split":
            raise ValueError("Ruta inválida")
        node = node[step]
    if node["type"] != "split":
        raise ValueError("La ruta no es una división")
    node["ratio"] = min(MAX_RATIO, max(MIN_RATIO, float(ratio)))
    return validate_document(document)


def move_tab_group(document, tab_id, index):
    """New document with the group holding tab_id moved to position index
    (clamped). None when the tab is not in the arrangement."""
    groups = list(document.get("groups", []))
    at = next((i for i, g in enumerate(groups) if tab_id in tab_ids(g.get("tree") or {})), None)
    if at is None:
        return None
    group = groups.pop(at)
    groups.insert(max(0, min(int(index), len(groups))), group)
    return {**copy.deepcopy(document), "groups": copy.deepcopy(groups)}


SORTS = ("fav", "recent", "need", "alpha")


def sort_groups(document, by, info):
    """«Ordenar una vez» (grill 30-sep): reordena los grupos UNA vez según
    ``by``; ``local`` siempre primero. ``info[tabId]`` = label, fav, activeAt,
    need. Estable: los empates conservan el orden actual."""
    if by not in SORTS:
        raise ValueError("Orden desconocido")
    groups = list(document.get("groups", []))

    def facts(group):
        ids = tab_ids(group.get("tree") or {})
        rows = [info.get(t) or {} for t in ids]
        return {"local": "local" in ids, "fav": any(r.get("fav") for r in rows),
                "need": any(r.get("need") for r in rows),
                "activeAt": max([r.get("activeAt") or 0 for r in rows] or [0]),
                "label": min([(r.get("label") or t).lower() for r, t in zip(rows, ids)] or [""])}
    key = {"fav": lambda f: (not f["fav"],), "recent": lambda f: (-f["activeAt"],),
           "need": lambda f: (not f["need"], not f["fav"]), "alpha": lambda f: (f["label"],)}[by]
    ordered = sorted(groups, key=lambda g: (not facts(g)["local"],) + key(facts(g)))
    return {**copy.deepcopy(document), "groups": copy.deepcopy(ordered)}


def restore_order(document, group_ids):
    """Deshacer: vuelve al orden de ``group_ids`` (grupos nuevos al final)."""
    pos = {g: i for i, g in enumerate(group_ids)}
    ordered = sorted(document.get("groups", []), key=lambda g: pos.get(g["id"], len(pos)))
    return {**copy.deepcopy(document), "groups": copy.deepcopy(ordered)}
