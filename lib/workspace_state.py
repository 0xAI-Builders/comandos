"""Shared workspace arrangement: validated, revisioned, restored by identity.

The document is the arrangement shared by every device: ordered groups
(entries of the top strip), each a binary tree of whole tabs. Per-device
focus, drafts and reading anchors live in ``workspace_clients`` and never
alter the arrangement.
"""
import hashlib
import json
import math
import time

SCHEMA = 1
MAX_DEPTH = 64
MAX_ID = 200
MAX_CLIENT_BYTES = 256 * 1024
MAX_DRAFTS = 64
MAX_DRAFT_CHARS = 20000
MAX_ANCHORS = 128
PHASES = ("restoring", "ready", "failed")


class Conflict(Exception):
    def __init__(self, current, message="Revisión desactualizada"):
        super().__init__(message)
        self.current = current


class NotReady(Exception):
    """Automatic saves are refused until startup restoration has finished."""


class EmptyInventory(Exception):
    """A temporarily empty inventory must not replace the last arrangement."""


def _ident(value, what):
    if not isinstance(value, str) or not value or len(value) > MAX_ID:
        raise ValueError(f"{what} inválido")
    return value


def tab_ids(node, depth=0):
    if depth > MAX_DEPTH:
        raise ValueError("Distribución demasiado profunda")
    if not isinstance(node, dict):
        raise ValueError("Distribución inválida")
    if node.get("type") == "tab":
        return [_ident(node.get("tabId"), "tabId")]
    if node.get("type") != "split" or node.get("axis") not in ("x", "y"):
        raise ValueError("Distribución inválida")
    ratio = node.get("ratio")
    if isinstance(ratio, bool) or not isinstance(ratio, (int, float)) \
            or not math.isfinite(ratio) or not 0 < ratio < 1:
        raise ValueError("Proporción inválida")
    return tab_ids(node.get("first"), depth + 1) + tab_ids(node.get("second"), depth + 1)


def validate_document(document):
    if not isinstance(document, dict) or document.get("schema") != SCHEMA:
        raise ValueError("Esquema de workspace no soportado")
    groups, tabs = document.get("groups"), document.get("tabs")
    if not isinstance(groups, list) or not isinstance(tabs, dict):
        raise ValueError("Workspace incompleto")
    group_ids, ids = set(), []
    for group in groups:
        if not isinstance(group, dict):
            raise ValueError("Grupo inválido")
        gid = _ident(group.get("id"), "Grupo")
        if gid in group_ids:
            raise ValueError("Grupo duplicado")
        group_ids.add(gid)
        ids.extend(tab_ids(group.get("tree")))
    if len(ids) != len(set(ids)) or set(ids) != set(tabs):
        raise ValueError("Cada tab debe aparecer exactamente una vez")
    pane_keys = []
    for tab_id, tab in tabs.items():
        if not isinstance(tab, dict):
            raise ValueError("Tab inválida")
        _ident(tab.get("session"), "Sesión")
        keys = tab.get("paneKeys", [])
        if not isinstance(keys, list):
            raise ValueError("paneKeys inválido")
        pane_keys.extend(_ident(k, "paneKey") for k in keys)
        if "label" in tab and not isinstance(tab["label"], str):
            raise ValueError("Etiqueta inválida")
    if len(pane_keys) != len(set(pane_keys)):
        raise ValueError("Un pane pertenece a una sola tab")
    bindings = document.get("bindings", {})
    if not isinstance(bindings, dict) or not set(bindings) <= set(pane_keys):
        raise ValueError("Enlaces de pane inválidos")
    return document


def _canonical(document):
    return json.dumps(document, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def _digest(document):
    return hashlib.sha256(_canonical(document).encode()).hexdigest()


def is_empty(document):
    return not document.get("groups") and not document.get("tabs")


class WorkspaceStore:
    """Workspace persistence over an ``app_state`` connection."""

    def __init__(self, conn):
        self.conn = conn
        self.phase = "restoring"

    def set_phase(self, phase):
        if phase not in PHASES:
            raise ValueError("Fase desconocida")
        self.phase = phase

    def _read(self, table):
        row = self.conn.execute(f"SELECT revision, document FROM {table} WHERE id = 1").fetchone()
        if not row:
            return None
        try:
            document = validate_document(json.loads(row[1]))
        except (ValueError, TypeError):
            return None
        return {"revision": row[0], "document": document}

    def current(self):
        """Latest valid revision; a corrupt current row yields the previous one."""
        state = self._read("workspace_current")
        if state is not None:
            return state
        previous = self._read("workspace_previous")
        if previous is not None:
            previous["recovered"] = True
        return previous

    def _current_revision(self):
        row = self.conn.execute("SELECT revision FROM workspace_current WHERE id = 1").fetchone()
        return row[0] if row else 0

    def _write_current(self, revision, encoded, now):
        self.conn.execute("INSERT OR REPLACE INTO workspace_previous "
                          "SELECT 1, revision, document, updated_at FROM workspace_current WHERE id = 1")
        self.conn.execute("INSERT OR REPLACE INTO workspace_current VALUES (1, ?, ?, ?)",
                          (revision, encoded, now))

    def commit(self, expected_revision, document, request_id, reason="user"):
        """Persist ``document`` as the next revision, or raise.

        ``reason='auto'`` marks saves produced by inventory polling: they need
        the ready phase and can never replace a workspace with an empty one.
        """
        _ident(request_id, "requestId")
        if reason not in ("auto", "user"):
            raise ValueError("Motivo inválido")
        if reason == "auto" and self.phase != "ready":
            raise NotReady(self.phase)
        validate_document(document)
        digest, encoded, now = _digest(document), _canonical(document), time.time()
        self.conn.execute("BEGIN IMMEDIATE")
        try:
            seen = self.conn.execute("SELECT digest, revision FROM workspace_requests WHERE request_id = ?",
                                     (request_id,)).fetchone()
            if seen:
                self.conn.execute("ROLLBACK")
                if seen[0] != digest:
                    raise Conflict(self.current(), "requestId reutilizado con otro contenido")
                return {"revision": seen[1], "document": json.loads(encoded)}
            revision = self._current_revision()
            if expected_revision != revision:
                self.conn.execute("ROLLBACK")
                raise Conflict(self.current())
            if reason == "auto" and is_empty(document) and revision:
                previous = self._read("workspace_current")
                if previous and not is_empty(previous["document"]):
                    self.conn.execute("ROLLBACK")
                    raise EmptyInventory()
            self._write_current(revision + 1, encoded, now)
            self.conn.execute("INSERT INTO workspace_requests VALUES (?, ?, ?, ?)",
                              (request_id, digest, revision + 1, now))
            # Keep idempotency records bounded.
            self.conn.execute("DELETE FROM workspace_requests WHERE created_at < ?", (now - 86400,))
            self.conn.execute("COMMIT")
        except (Conflict, EmptyInventory):
            raise
        except BaseException:
            if self.conn.in_transaction:
                self.conn.execute("ROLLBACK")
            raise
        return {"revision": revision + 1, "document": json.loads(encoded)}

    # ---- per-device state ----------------------------------------------
    def save_client(self, device_id, state):
        """Merge one device's state. ``draftsPatch``/``anchorsPatch`` change
        single keys (None removes one) so frequent small saves never race a
        whole-dict rewrite from another tab of the same device."""
        _ident(device_id, "deviceId")
        if not isinstance(state, dict):
            raise ValueError("Estado de cliente inválido")
        self.conn.execute("BEGIN IMMEDIATE")
        try:
            # Fields a request omits keep their saved value: saving focus must
            # never erase that device's drafts or reading anchors.
            previous = self.client(device_id) or {}
            clean = {key: state[key] if key in state else previous.get(key, default)
                     for key, default in (("activeTabId", None), ("activePaneKey", None),
                                          ("drafts", {}), ("readingAnchors", {}))}
            clean["drafts"] = dict(clean["drafts"] or {})
            clean["readingAnchors"] = dict(clean["readingAnchors"] or {})
            for key in ("activeTabId", "activePaneKey"):
                if clean[key] is not None:
                    _ident(clean[key], key)
            if not isinstance(clean["drafts"], dict) or not isinstance(clean["readingAnchors"], dict):
                raise ValueError("Estado de cliente inválido")
            if "draftsPatch" in state:
                _patch(clean["drafts"], state["draftsPatch"], _draft_entry, MAX_DRAFTS)
            if "anchorsPatch" in state:
                _patch(clean["readingAnchors"], state["anchorsPatch"], _anchor_entry, MAX_ANCHORS)
            encoded = _canonical(clean)
            if len(encoded.encode()) > MAX_CLIENT_BYTES:
                raise ValueError("Estado de cliente demasiado grande")
            self.conn.execute("INSERT OR REPLACE INTO workspace_clients VALUES (?, ?, ?)",
                              (device_id, encoded, time.time()))
            self.conn.execute("COMMIT")
        except BaseException:
            self.conn.execute("ROLLBACK")
            raise
        return clean

    def client(self, device_id):
        row = self.conn.execute("SELECT state FROM workspace_clients WHERE device_id = ?",
                                (device_id,)).fetchone()
        return json.loads(row[0]) if row else None

    def meta(self, key):
        row = self.conn.execute("SELECT value FROM workspace_meta WHERE key = ?", (key,)).fetchone()
        return row[0] if row else None

    def set_meta(self, key, value):
        self.conn.execute("BEGIN IMMEDIATE")
        try:
            self.conn.execute("INSERT OR REPLACE INTO workspace_meta VALUES (?, ?)", (key, value))
            self.conn.execute("COMMIT")
        except BaseException:
            self.conn.execute("ROLLBACK")
            raise


# ---- per-device drafts and reading anchors ----------------------------------

def _stamp(value):
    at = value.get("updatedAt")
    if isinstance(at, bool) or not isinstance(at, (int, float)) or not math.isfinite(at):
        return time.time() * 1000
    return at


def _draft_entry(value):
    text = value.get("text")
    if not isinstance(text, str) or len(text) > MAX_DRAFT_CHARS:
        raise ValueError("Borrador inválido o demasiado largo")
    entry = {"text": text, "updatedAt": _stamp(value)}
    for key in ("selStart", "selEnd"):
        pos = value.get(key)
        if isinstance(pos, int) and not isinstance(pos, bool) and 0 <= pos <= len(text):
            entry[key] = pos
    return entry


def _anchor_entry(value):
    text, ratio = value.get("text"), value.get("ratio")
    if not isinstance(text, str) or len(text) > 300:
        raise ValueError("Ancla de lectura inválida")
    entry = {"text": text, "updatedAt": _stamp(value)}
    if isinstance(ratio, (int, float)) and not isinstance(ratio, bool) and 0 <= ratio <= 1:
        entry["ratio"] = float(ratio)
    return entry


def _patch(target, patch, make, limit):
    if not isinstance(patch, dict):
        raise ValueError("Cambio inválido")
    for key, value in patch.items():
        _ident(key, "clave")
        if value is None:
            target.pop(key, None)
        elif isinstance(value, dict):
            target[key] = make(value)
        else:
            raise ValueError("Cambio inválido")
    # Bounded: the least recently updated entries go first.
    for key in sorted(target, key=lambda k: target[k].get("updatedAt", 0))[:max(0, len(target) - limit)]:
        target.pop(key)


# ---- closing a whole group -------------------------------------------------

PROTECTED_SESSIONS = {"local"}


def _group(document, group_id):
    for group in document.get("groups", []):
        if group.get("id") == group_id:
            return group
    raise ValueError("El grupo ya no existe")


def close_group_preview(document, group_id, session_identity):
    """The fixed list a person confirms: every member and its live identity."""
    members = []
    for tab_id in tab_ids(_group(document, group_id)["tree"]):
        tab = document["tabs"].get(tab_id) or {}
        session = tab.get("session") or tab_id
        members.append({"tabId": tab_id, "session": session, "label": tab.get("label") or session,
                        "sessionId": session_identity(session), "kept": session in PROTECTED_SESSIONS})
    return {"groupId": group_id, "members": members}


def close_group(store, group_id, expected_revision, identities, request_id, session_identity, close_tab):
    """Close exactly the confirmed members of a group, one by one.

    Nothing closes unless the revision, member set and every live session
    identity still match what was confirmed. A failure after a close stops
    there and reports what really closed; nothing is undone or compensated.
    ``close_tab(session)`` returns an error string or None.
    """
    _ident(request_id, "requestId")
    key = "close-group:" + request_id
    seen = store.meta(key)
    if seen:
        return json.loads(seen)
    state = store.current()
    if not state or state["revision"] != expected_revision:
        raise Conflict(state)
    document = state["document"]
    confirmed = {m.get("tabId"): m for m in identities if isinstance(m, dict)}
    order = tab_ids(_group(document, group_id)["tree"])
    if set(order) != set(confirmed) or len(confirmed) != len(identities):
        raise ValueError("Las pestañas del grupo cambiaron. Revisa la lista antes de cerrar")
    for tab_id in order:
        member = confirmed[tab_id]
        session = (document["tabs"].get(tab_id) or {}).get("session") or tab_id
        live = session_identity(session)
        if member.get("session") != session or not live or live != member.get("sessionId"):
            raise ValueError(f"«{member.get('label') or session}» cambió. No se ha cerrado nada")
    result = {"ok": True, "closed": [], "remaining": [], "kept": [], "error": None}
    pending = [confirmed[t]["session"] for t in order]
    while pending:
        session = pending.pop(0)
        if session in PROTECTED_SESSIONS:
            result["kept"].append(session)
            continue
        try:
            error = close_tab(session)
        except Exception as exc:  # report, never compensate
            error = str(exc) or "Error al cerrar"
        if error:
            result.update(ok=False, error=error,
                          remaining=[session] + [s for s in pending if s not in PROTECTED_SESSIONS])
            result["kept"] += [s for s in pending if s in PROTECTED_SESSIONS]
            break
        result["closed"].append(session)
    store.set_meta(key, json.dumps(result))
    return result


# ---- restore by identity ---------------------------------------------------

def _same_process(binding, live):
    return bool(live) and live.get("pid") == binding.get("pid") \
        and live.get("startTime") == binding.get("startTime")


def restore_workspace(document, inspect, resume_exact):
    """Reattach live panes, resume exact conversations, report the rest.

    ``inspect(binding)`` returns the live process at the binding's pane (pid and
    start time) or None. ``resume_exact(binding)`` resumes that exact
    conversation and returns the new binding, or None. A pane without a
    conversation id is never resumed with "the latest" conversation.
    """
    validate_document(document)
    bindings = document.get("bindings", {})
    out = {"attached": [], "resumed": {}, "unavailable": []}
    for tab in document["tabs"].values():
        for key in tab.get("paneKeys", []):
            binding = bindings.get(key)
            if not binding:
                out["unavailable"].append(key)
                continue
            if _same_process(binding, inspect(binding)):
                out["attached"].append(key)
                continue
            conversation = binding.get("conversation") or {}
            if not conversation.get("id"):
                out["unavailable"].append(key)
                continue
            try:
                resumed = resume_exact(binding)
            except Exception:
                resumed = None
            if resumed:
                out["resumed"][key] = resumed
            else:
                out["unavailable"].append(key)
    return out


# ---- inventory reconciliation ---------------------------------------------

def _remove_leaf(node, tab_id):
    """Tree without ``tab_id``; a split left with one child collapses."""
    if node["type"] == "tab":
        return None if node["tabId"] == tab_id else node
    first, second = _remove_leaf(node["first"], tab_id), _remove_leaf(node["second"], tab_id)
    if first is None:
        return second
    if second is None:
        return first
    return dict(node, first=first, second=second)


def pane_bindings(snapshot_session):
    """[(paneKey, binding)] for one tmux session from the layout snapshot."""
    out = []
    for window in (snapshot_session or {}).get("windows", []):
        for pane in window.get("panes", []):
            key = pane.get("key")
            if not isinstance(key, str) or not key:
                continue
            conversation = None
            if pane.get("resume_id"):
                conversation = {"agent": pane.get("agent"), "id": pane["resume_id"]}
            elif (pane.get("acp") or {}).get("sessionId"):
                conversation = {"agent": "acp", "id": pane["acp"]["sessionId"]}
            out.append((key, {"paneId": pane.get("id"), "pid": pane.get("pid"),
                              "startTime": pane.get("start"), "conversation": conversation}))
    return out


def reconcile(document, live_tabs, panes=None):
    """Fit the arrangement to the open tabs, keeping groups, order and ratios.

    ``live_tabs`` is the ordered list of ``(tab_id, label)``. Missing tabs
    leave their groups; new tabs become single-tab groups at the end.
    """
    live = dict(live_tabs)
    groups = []
    for group in document.get("groups", []):
        tree = group["tree"]
        for tab_id in tab_ids(tree):
            if tab_id not in live:
                tree = _remove_leaf(tree, tab_id) if tree else None
        if tree:
            groups.append(dict(group, tree=tree))
    tabs = {k: dict(v) for k, v in document.get("tabs", {}).items() if k in live}
    known = {t for g in groups for t in tab_ids(g["tree"])}
    used = {g["id"] for g in groups}
    for tab_id, label in live_tabs:
        entry = tabs.setdefault(tab_id, {"session": tab_id, "paneKeys": []})
        if label:
            entry["label"] = label
        if tab_id not in known:
            gid = "group-" + tab_id
            n = 1
            while gid in used:
                n += 1
                gid = f"group-{tab_id}-{n}"
            used.add(gid)
            groups.append({"id": gid, "tree": {"type": "tab", "tabId": tab_id}})
            known.add(tab_id)
    bindings = dict(document.get("bindings", {}))
    for tab_id, found in (panes or {}).items():
        if tab_id not in tabs or not found:
            continue  # without a fresh capture keep the last known panes
        tabs[tab_id]["paneKeys"] = [k for k, _ in found]
        for key, binding in found:
            bindings[key] = dict(binding, session=tabs[tab_id]["session"])
    keys = {k for t in tabs.values() for k in t.get("paneKeys", [])}
    out = dict(document, schema=SCHEMA, groups=groups, tabs=tabs)
    if "bindings" in document or bindings:
        out["bindings"] = {k: v for k, v in bindings.items() if k in keys}
    return validate_document(out)


def empty_document():
    return {"schema": SCHEMA, "groups": [], "tabs": {}}
