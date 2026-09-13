"""Bounded, read-only pane history for native browser text selection."""
import re


def capture(tmux, data):
    session = str(data.get("session") or "")
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,120}", session):
        raise ValueError("Sesión inválida")
    lines = data.get("lines", 2000)
    if isinstance(lines, bool) or not isinstance(lines, int) or not 1 <= lines <= 5000:
        raise ValueError("El historial admite entre 1 y 5000 líneas")
    result = tmux("list-panes", "-t", "=" + session, "-F",
                  "#{pane_id}\t#{pane_active}\t#{pane_left}\t#{pane_top}\t#{pane_width}\t#{pane_height}\t#{pane_current_command}")
    if result.returncode:
        raise ValueError("No se encuentra la sesión")
    panes = []
    for row in result.stdout.splitlines():
        fields = row.split("\t", 6)
        if len(fields) != 7:
            continue
        pane, active, left, top, width, height, title = fields
        panes.append(dict(id=pane, active=active == "1", left=int(left), top=int(top),
                          width=int(width), height=int(height), title=title[:100]))
    selected = None
    if data.get("pane"):
        selected = next((p for p in panes if p["id"] == data["pane"]), None)
        if selected is None:
            raise ValueError("El panel no pertenece a esta sesión")
    elif "col" in data or "row" in data:
        col, row = data.get("col"), data.get("row")
        if any(isinstance(v, bool) or not isinstance(v, int) or v < 0 for v in (col, row)):
            raise ValueError("Coordenadas inválidas")
        selected = next((p for p in panes if p["left"] <= col < p["left"] + p["width"]
                         and p["top"] <= row < p["top"] + p["height"]), None)
        if selected is None:
            raise ValueError("No hay un panel en esa posición")
    else:
        selected = next((p for p in panes if p["active"]), None)
    if selected is None:
        raise ValueError("No se encuentra el panel")
    result = tmux("capture-pane", "-p", "-J", "-t", selected["id"], "-S", str(-lines))
    if result.returncode:
        raise ValueError("No se pudo leer el historial del panel")
    text = result.stdout
    limit = 1_000_000
    return {"ok": True, "pane": selected["id"], "panes": panes,
            "text": text[-limit:], "lines": lines, "truncated": len(text) > limit,
            "source": "tmux", "readOnly": True}
