"""E1 work marks: an organizational state chosen by the human, per scope.

Scopes are a tmux session (tab) and a pane (W1 ``paneKey``). Each scope has
its own mark and an independent ``favorite`` flag; marking a session never
touches its panes and vice versa. A new pane key starts without a mark.

Decision D1 (Jesús, 2026-09-29): Congelado and Esperando respuesta only
change by a human action; only Resuelto reopens, and only with a confirmed
accepted prompt. Finishing a turn never assigns a mark (neutral icon).

Writes use optimistic revisions per scope/key. Helpers join a caller's
transaction when one is open and otherwise run their own short one.
"""
import time

MARKS = ("none", "resolved", "frozen", "awaiting_reply")
LABELS = {"none": ("Sin marca", "No mark"), "resolved": ("Resuelto", "Resolved"),
          "frozen": ("Congelado", "Frozen"), "awaiting_reply": ("Esperando respuesta", "Awaiting reply"),
          "favorite": ("Favorito", "Favorite"), "working": ("Trabajando", "Working")}
# Same shapes as dash/work-marks.js (parity is tested). The web animates them
# with CSS; the desktop bakes the same loops into per-phase SVG frames.
ICONS = {
    "none": '<circle class="wm-ring" cx="12" cy="12" r="6.5"/>',
    "resolved": '<circle cx="12" cy="12" r="9"/><path class="wm-draw" pathLength="1" d="m7.5 12.4 3 3 6-6.6"/>',
    "frozen": '<g class="wm-spin"><path d="M12 3v18M4.2 7.5l15.6 9M4.2 16.5l15.6-9"/>'
              '<path d="m9.5 4.5 2.5 2 2.5-2M9.5 19.5l2.5-2 2.5 2"/></g>',
    "awaiting_reply": '<path d="M4 5.5h16v10H10l-4.5 3.8V15.5H4z"/>'
                      '<circle class="wm-dot wm-d1" cx="8.5" cy="10.5" r="1.1"/><circle class="wm-dot wm-d2" cx="12" cy="10.5" r="1.1"/>'
                      '<circle class="wm-dot wm-d3" cx="15.5" cy="10.5" r="1.1"/>',
    "working": '<circle class="wm-track" cx="12" cy="12" r="8"/><path class="wm-spin" d="M12 4a8 8 0 0 1 8 8"/>',
    "favorite": '<path class="wm-twinkle" d="m12 3.6 2.6 5.3 5.8.8-4.2 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.2-4.1 5.8-.8z"/>',
}
ICON_COLORS = {"none": "#5E6980", "resolved": "#2EE59D", "frozen": "#7CC4FF",
               "awaiting_reply": "#FFAE1A", "working": "#7AA5FF", "favorite": "#FFAE1A"}


# Loop lengths of dash/workspace.css (wm-spin 1.4 s, frozen 9 s, wm-draw 2.6 s,
# wm-dot 1.5 s, wm-twinkle 2.4 s). "none" is the neutral ready indicator: static.
CYCLE_S = {"working": 1.4, "frozen": 9.0, "resolved": 2.6, "awaiting_reply": 1.5, "favorite": 2.4}
ANIMATED = tuple(CYCLE_S)
FRAME_S = 0.08                      # one desktop tick; frames are cached per icon
MAX_FRAMES = 48


def frame_count(name):
    cycle = CYCLE_S.get(name)
    return 1 if not cycle else max(8, min(MAX_FRAMES, round(cycle / FRAME_S)))


def frame_index(name, seconds):
    """Frame to show ``seconds`` into the loop (wraps every CYCLE_S)."""
    cycle = CYCLE_S.get(name)
    if not cycle:
        return 0
    n = frame_count(name)
    return int(((seconds % cycle) / cycle) * n) % n


def _ease(t):
    return t * t * (3 - 2 * t)   # ease-in-out, like the CSS keyframes


def _phase_body(name, body, phase):
    """Bake one moment of the CSS loop into the static markup."""
    p = phase % 1.0
    if name in ("working", "frozen"):
        return body.replace('class="wm-spin"', f'transform="rotate({p * 360:.1f} 12 12)"', 1)
    if name == "resolved":                       # 0→1, 40–80 % held at 0, →-1
        off = 1 - _ease(p / 0.4) if p < 0.4 else (0 if p < 0.8 else -_ease((p - 0.8) / 0.2))
        return body.replace('class="wm-draw"', f'stroke-dasharray="1" stroke-dashoffset="{off:.3f}"', 1)
    if name == "awaiting_reply":                # three dots, 0.2 s apart on a 1.5 s loop
        out = body
        for i, cls in enumerate(("wm-d1", "wm-d2", "wm-d3")):
            q = (p - i * 0.2 / CYCLE_S[name]) % 1.0
            alpha = 0.25 + 0.75 * (_ease(q / 0.4) if q < 0.4 else 1 - _ease((q - 0.4) / 0.6))
            out = out.replace(f'class="wm-dot {cls}"', f'opacity="{alpha:.3f}"', 1)
        return out
    if name == "favorite":                      # 1 → .82 → 1, opacity 1 → .7 → 1
        k = _ease(p * 2 if p < 0.5 else 2 - p * 2)
        scale, alpha = 1 - 0.18 * k, 1 - 0.3 * k
        return body.replace('class="wm-twinkle"', f'transform="translate(12 12) scale({scale:.3f}) translate(-12 -12)" opacity="{alpha:.3f}"', 1)
    return body


def icon_svg(name, color=None, size=16, phase=None):
    """Standalone SVG document for one state (desktop tab labels and menus).

    ``phase`` in [0, 1) renders that moment of the icon's loop; None is the
    resting frame. Static icons ignore it."""
    body = ICONS.get(name, ICONS["none"])
    if phase is not None and name in CYCLE_S:
        body = _phase_body(name, body, phase)
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 24 24" '
            f'fill="none" stroke="{color or ICON_COLORS.get(name, ICON_COLORS["none"])}" stroke-width="1.8" '
            f'stroke-linecap="round" stroke-linejoin="round">{body}</svg>')


def pane_key_for(panes, session, pane_id):
    """W1 paneKey of tmux pane ``pane_id`` in ``session``; None when ambiguous."""
    hits = [p.get("paneKey") for p in panes or []
            if isinstance(p, dict) and p.get("session") == session and p.get("paneId") == pane_id]
    return hits[0] if len(hits) == 1 and hits[0] else None
SCOPES = ("session", "pane")
MAX_KEY = 200


class Conflict(Exception):
    def __init__(self, current):
        super().__init__("Revisión desactualizada")
        self.current = current


def mark_after_event(mark, event):
    if event['kind'] == 'prompt_accepted' and event['evidence'] == 'confirmed':
        return 'none' if mark == 'resolved' else mark
    return mark


def _check(scope, key):
    if scope not in SCOPES:
        raise ValueError("Ámbito inválido")
    if not isinstance(key, str) or not key or len(key) > MAX_KEY or any(ord(c) < 32 for c in key):
        raise ValueError("Clave inválida")


def _row(scope, key, row):
    if row is None:
        return {"scope": scope, "key": key, "mark": "none", "favorite": False, "revision": 0,
                "updatedAtMs": None, "updatedBy": None}
    return {"scope": scope, "key": key, "mark": row[0], "favorite": bool(row[1]),
            "revision": row[2], "updatedAtMs": row[3], "updatedBy": row[4]}


def get_mark(conn, scope, key):
    _check(scope, key)
    row = conn.execute("SELECT mark, favorite, revision, updated_at_ms, updated_by FROM work_marks "
                       "WHERE scope = ? AND key = ?", (scope, key)).fetchone()
    return _row(scope, key, row)


def list_marks(conn):
    rows = conn.execute("SELECT scope, key, mark, favorite, revision, updated_at_ms, updated_by "
                        "FROM work_marks ORDER BY scope, key").fetchall()
    return [_row(r[0], r[1], r[2:]) for r in rows]


def _parse_value(value):
    if isinstance(value, str):
        value = {"mark": value}
    if not isinstance(value, dict) or not value or set(value) - {"mark", "favorite"}:
        raise ValueError("Valor inválido")
    if "mark" in value and value["mark"] not in MARKS:
        raise ValueError("Marca inválida")
    if "favorite" in value and not isinstance(value["favorite"], bool):
        raise ValueError("Favorito inválido")
    return value


def _write(conn, current, mark, favorite, by):
    revision = current["revision"] + 1
    conn.execute("INSERT OR REPLACE INTO work_marks VALUES (?, ?, ?, ?, ?, ?, ?)",
                 (current["scope"], current["key"], mark, int(favorite), revision,
                  int(time.time() * 1000), by))
    return get_mark(conn, current["scope"], current["key"])


class _Tx:
    def __init__(self, conn):
        self.conn, self.own = conn, not conn.in_transaction

    def __enter__(self):
        if self.own:
            self.conn.execute("BEGIN IMMEDIATE")
        return self.conn

    def __exit__(self, kind, *_):
        if self.own and self.conn.in_transaction:
            self.conn.execute("COMMIT" if kind is None else "ROLLBACK")


def set_mark(conn, scope, key, value, expected_revision):
    """Set the mark and/or favorite of one scope; raises Conflict on a stale revision."""
    _check(scope, key)
    change = _parse_value(value)
    if isinstance(expected_revision, bool) or not isinstance(expected_revision, int) \
            or expected_revision < 0:
        raise ValueError("expectedRevision inválido")
    with _Tx(conn):
        current = get_mark(conn, scope, key)
        if current["revision"] != expected_revision:
            raise Conflict(current)
        return _write(conn, current, change.get("mark", current["mark"]),
                      change.get("favorite", current["favorite"]), "user")


def apply_turn_event(conn, event):
    """Apply a lifecycle event once per scope; returns the rows it changed."""
    event_id = event.get("eventId")
    targets = [(scope, event.get(field)) for scope, field in (("pane", "paneKey"), ("session", "sessionKey"))
               if event.get(field)]
    if not event_id or not targets or event.get("kind") not in ("prompt_accepted",):
        return []
    changed = []
    with _Tx(conn):
        for scope, key in targets:
            _check(scope, key)
            fresh = conn.execute("INSERT OR IGNORE INTO work_mark_applied VALUES (?, ?, ?)",
                                 (event_id, scope, key)).rowcount
            if not fresh:
                continue
            current = get_mark(conn, scope, key)
            after = mark_after_event(current["mark"], {"kind": event["kind"],
                                                       "evidence": event.get("evidence")})
            if after != current["mark"]:
                changed.append(_write(conn, current, after, current["favorite"], "event:" + event_id))
    return changed
