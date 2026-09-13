"""Small durable ledger of browser actions; never stores prompts or credentials."""
import json
import sqlite3
import time
import uuid


def connect(root):
    root.mkdir(parents=True, exist_ok=True)
    db = sqlite3.connect(str(root / "actions.sqlite"), timeout=3)
    db.execute("PRAGMA journal_mode=WAL")
    db.execute("CREATE TABLE IF NOT EXISTS actions (id TEXT PRIMARY KEY, tool TEXT, status TEXT, detail TEXT, created REAL, updated REAL)")
    return db


def create(root, tool):
    action_id = str(uuid.uuid4())
    db = connect(root)
    try:
        with db:
            now = time.time()
            db.execute("INSERT INTO actions VALUES (?,?,?,?,?,?)", (action_id, tool, "pending", "", now, now))
            db.execute("DELETE FROM actions WHERE id IN (SELECT id FROM actions ORDER BY created DESC LIMIT -1 OFFSET 2000)")
    finally:
        db.close()
    return action_id


def acknowledge(root, data):
    action_id = str(data.get("actionId") or "")
    status = data.get("status")
    if status not in ("dispatched", "confirmed", "failed"):
        raise ValueError("Estado de acción inválido")
    db = connect(root)
    try:
        with db:
            result = db.execute("UPDATE actions SET status=?,detail=?,updated=? WHERE id=? AND status IN ('pending','dispatched')",
                                (status, str(data.get("detail") or "")[:400], time.time(), action_id))
            exists = db.execute("SELECT 1 FROM actions WHERE id=?", (action_id,)).fetchone()
            if not exists:
                raise ValueError("Acción desconocida")
        return {"ok": True}
    finally:
        db.close()


def recent(root):
    db = connect(root)
    try:
        db.row_factory = sqlite3.Row
        return [dict(row) for row in db.execute("SELECT * FROM actions ORDER BY created DESC LIMIT 40")]
    finally:
        db.close()
