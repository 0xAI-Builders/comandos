"""Lo que Jesús hace con una noticia de los Resúmenes: guardarla, anotarla,
chatear con ella y traducir sus fuentes. Todo queda en SQLite (app_state).

Chat y traducción llaman a un agente, que puede tardar: `start_*` deja el
trabajo registrado como pendiente y `run_*` lo completa (cc-dash lo corre en
un hilo). Si el servicio se reinicia a medias, `recover` marca lo pendiente
como fallido en vez de dejarlo colgado para siempre.

Las fuentes capturadas son DATO: van al agente entre marcas <fuente> y el
agente tiene prohibidas las herramientas.
"""
from __future__ import annotations

import json
import re
import time

import news_editions as ne

MAX_NOTE = 8000
MAX_MESSAGE = 4000
CHAT_TURNS = 12


def _now():
    return int(time.time() * 1000)


def _clip(text, limit):
    text = str(text or "").strip()
    return text if len(text) <= limit else text[:limit - 1] + "…"


def story_row(conn, story_id):
    row = conn.execute("SELECT id, edition_id, title, summary_md, body_md FROM news_stories WHERE id = ?",
                       (story_id,)).fetchone()
    return {"id": row[0], "editionId": row[1], "title": row[2], "summary": row[3], "body": row[4]} if row else None


def recover(conn, now=None):
    """Al arrancar: nada queda «pensando» para siempre."""
    now = now or _now()
    with ne._tx(conn):
        conn.execute("UPDATE news_chat SET state = 'failed', text = 'Se interrumpió (el servicio se reinició). "
                     "Vuelve a preguntar.' WHERE state = 'pending'")
        conn.execute("UPDATE news_translations SET state = 'failed', error = 'interrumpida', updated_at_ms = ? "
                     "WHERE state = 'running'", (now,))


# ---------------------------------------------------------------- guardadas y conteos

def saved_ids(conn):
    return [r[0] for r in conn.execute("SELECT story_id FROM news_saved ORDER BY saved_at_ms DESC")]


def set_saved(conn, story_id, on, now=None):
    story = story_row(conn, story_id)
    if not story:
        return None
    with ne._tx(conn):
        if on:
            conn.execute("INSERT OR IGNORE INTO news_saved (story_id, edition_id, saved_at_ms) VALUES (?, ?, ?)",
                         (story_id, story["editionId"], now or _now()))
        else:
            conn.execute("DELETE FROM news_saved WHERE story_id = ?", (story_id,))
    return bool(on)


def saved_stories(conn):
    """Las guardadas con lo necesario para listarlas y volver a abrirlas."""
    rows = conn.execute("SELECT st.id, st.edition_id, st.title, st.summary_md, st.meta, sv.saved_at_ms "
                        "FROM news_saved sv JOIN news_stories st ON st.id = sv.story_id "
                        "ORDER BY sv.saved_at_ms DESC LIMIT 300").fetchall()
    return [{"id": r[0], "editionId": r[1], "title": r[2], "summary": r[3], "meta": ne._json_obj(r[4]),
             "savedAt": r[5]} for r in rows]


def story_counts(conn, story_ids):
    """Por noticia: notas, mensajes de chat y si está guardada (para la lista)."""
    ids = [int(i) for i in story_ids if isinstance(i, int)]
    if not ids:
        return {}
    marks = ",".join("?" * len(ids))
    out = {i: {"notes": 0, "chat": 0, "saved": False} for i in ids}
    for sid, n in conn.execute(f"SELECT story_id, COUNT(*) FROM news_notes WHERE story_id IN ({marks}) "
                               "GROUP BY story_id", ids):
        out[sid]["notes"] = n
    for sid, n in conn.execute(f"SELECT story_id, COUNT(*) FROM news_chat WHERE story_id IN ({marks}) "
                               "GROUP BY story_id", ids):
        out[sid]["chat"] = n
    for (sid,) in conn.execute(f"SELECT story_id FROM news_saved WHERE story_id IN ({marks})", ids):
        out[sid]["saved"] = True
    return out


# ---------------------------------------------------------------- notas

def _note(row):
    return {"id": row[0], "storyId": row[1], "editionId": row[2], "storyTitle": row[3], "kind": row[4],
            "chatId": row[5], "quote": row[6], "cite": row[7], "text": row[8], "createdAt": row[9],
            "updatedAt": row[10]}


_NOTE_COLS = "id, story_id, edition_id, story_title, kind, chat_id, quote, cite, text, created_at_ms, updated_at_ms"


def list_notes(conn, story_id=None, query=None, limit=500):
    sql, args = f"SELECT {_NOTE_COLS} FROM news_notes", []
    where = []
    if story_id is not None:
        where.append("story_id = ?")
        args.append(story_id)
    q = str(query or "").strip().lower()
    if q:
        where.append("(lower(text) LIKE ? OR lower(COALESCE(quote,'')) LIKE ? OR lower(story_title) LIKE ?)")
        like = "%" + q.replace("%", "").replace("_", "") + "%"
        args += [like, like, like]
    if where:
        sql += " WHERE " + " AND ".join(where)
    sql += " ORDER BY created_at_ms DESC LIMIT ?"
    args.append(max(1, min(int(limit), 1000)))
    notes = [_note(r) for r in conn.execute(sql, args).fetchall()]
    total = conn.execute("SELECT COUNT(*) FROM news_notes").fetchone()[0]
    return {"notes": notes, "total": total}


def add_note(conn, story_id, text, *, kind="libre", chat_id=None, quote=None, cite=None, now=None):
    story = story_row(conn, story_id)
    if not story:
        raise LookupError("noticia no encontrada")
    text = _clip(text, MAX_NOTE)
    if not text and not quote:
        raise ValueError("la nota está vacía")
    now = now or _now()
    with ne._tx(conn):
        cur = conn.execute("INSERT INTO news_notes (story_id, edition_id, story_title, kind, chat_id, quote, cite, "
                           "text, created_at_ms, updated_at_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                           (story_id, story["editionId"], story["title"], "chat" if kind == "chat" else "libre",
                            chat_id, _clip(quote, MAX_NOTE) or None, _clip(cite, 200) or None, text, now, now))
    return _note(conn.execute(f"SELECT {_NOTE_COLS} FROM news_notes WHERE id = ?", (cur.lastrowid,)).fetchone())


def update_note(conn, note_id, text, now=None):
    text = _clip(text, MAX_NOTE)
    with ne._tx(conn):
        cur = conn.execute("UPDATE news_notes SET text = ?, updated_at_ms = ? WHERE id = ?", (text, now or _now(), note_id))
    if not cur.rowcount:
        raise LookupError("nota no encontrada")
    return _note(conn.execute(f"SELECT {_NOTE_COLS} FROM news_notes WHERE id = ?", (note_id,)).fetchone())


def delete_note(conn, note_id):
    with ne._tx(conn):
        return conn.execute("DELETE FROM news_notes WHERE id = ?", (note_id,)).rowcount > 0


def toggle_chat_note(conn, chat_id, now=None):
    """☆ en una burbuja: la guarda como nota con su cita; si ya estaba, la quita."""
    msg = conn.execute("SELECT id, story_id, role, state, text, cite FROM news_chat WHERE id = ?", (chat_id,)).fetchone()
    if not msg or msg[3] != "done":
        raise LookupError("mensaje no encontrado")
    existing = conn.execute("SELECT id FROM news_notes WHERE chat_id = ?", (chat_id,)).fetchone()
    if existing:
        delete_note(conn, existing[0])
        return {"noted": False, "noteId": existing[0]}
    who = "Mi pregunta" if msg[2] == "user" else "Respuesta de la IA"
    note = add_note(conn, msg[1], "", kind="chat", chat_id=chat_id, quote=msg[4], cite=msg[5],
                    now=now)
    conn.execute("UPDATE news_notes SET text = ? WHERE id = ?", (f"{who}, guardada desde el chat.", note["id"]))
    note["text"] = f"{who}, guardada desde el chat."
    return {"noted": True, "note": note}


# ---------------------------------------------------------------- chat

def chat_history(conn, story_id):
    noted = {r[0] for r in conn.execute("SELECT chat_id FROM news_notes WHERE story_id = ? AND chat_id IS NOT NULL",
                                        (story_id,))}
    return [{"id": r[0], "role": r[1], "state": r[2], "text": r[3], "cite": r[4], "model": r[5],
             "createdAt": r[6], "noted": r[0] in noted}
            for r in conn.execute("SELECT id, role, state, text, cite, model, created_at_ms FROM news_chat "
                                  "WHERE story_id = ? ORDER BY id", (story_id,)).fetchall()]


def start_chat(conn, story_id, message, now=None):
    story = story_row(conn, story_id)
    if not story:
        raise LookupError("noticia no encontrada")
    text = _clip(message, MAX_MESSAGE)
    if not text:
        raise ValueError("el mensaje está vacío")
    busy = conn.execute("SELECT 1 FROM news_chat WHERE story_id = ? AND state = 'pending'", (story_id,)).fetchone()
    if busy:
        raise RuntimeError("todavía estoy respondiendo la pregunta anterior")
    now = now or _now()
    with ne._tx(conn):
        conn.execute("INSERT INTO news_chat (story_id, edition_id, role, state, text, created_at_ms) "
                     "VALUES (?, ?, 'user', 'done', ?, ?)", (story_id, story["editionId"], text, now))
        cur = conn.execute("INSERT INTO news_chat (story_id, edition_id, role, state, text, created_at_ms) "
                           "VALUES (?, ?, 'assistant', 'pending', '', ?)", (story_id, story["editionId"], now + 1))
    return cur.lastrowid


def _sources_payload(conn, story_id, max_chars=36000):
    parts, used = [], 0
    rows = conn.execute("SELECT s.id FROM news_story_sources l JOIN news_sources s ON s.id = l.source_id "
                        "WHERE l.story_id = ? ORDER BY s.id", (story_id,)).fetchall()
    for n, (sid,) in enumerate(rows, 1):
        src = ne.get_source(conn, sid)
        if not src:
            continue
        cap = src.get("capture") or {}
        text = _blocks_text(cap.get("blocks") or [], numbered=True) or src["title"]
        text = text[:max(0, max_chars - used)]
        used += len(text)
        host = re.sub(r"^www\.", "", re.sub(r"^https?://([^/]+).*", r"\1", src["url"]))
        official = ' oficial="sí"' if src["official"] else ""
        parts.append(f"<fuente id=\"s{n}\" host=\"{host}\" origen=\"{src['origin']}\"{official}>\n"
                     f"{src['title']}\n{text}\n</fuente>")
        if used >= max_chars:
            break
    return "\n\n".join(parts)


def _blocks_text(blocks, numbered=False):
    out, n = [], 0
    for b in blocks:
        if b.get("type") == "img":
            continue
        n += 1
        prefix = f"[párrafo {n}] " if numbered else ""
        mark = {"h": "## ", "li": "- ", "quote": "> "}.get(b.get("type"), "")
        out.append(prefix + mark + str(b.get("text") or ""))
    return "\n".join(out)


def chat_prompt(conn, story_id, pending_id):
    story = story_row(conn, story_id)
    turns = conn.execute("SELECT role, text FROM news_chat WHERE story_id = ? AND id < ? AND state = 'done' "
                         "ORDER BY id DESC LIMIT ?", (story_id, pending_id, CHAT_TURNS)).fetchall()[::-1]
    history = "\n".join(("Jesús: " if role == "user" else "Asistente: ") + text for role, text in turns[:-1])
    question = turns[-1][1] if turns and turns[-1][0] == "user" else ""
    return (f"Noticia: {story['title']}\n\nResumen del boletín:\n{story['summary']}\n\n{story['body']}\n\n"
            f"Fuentes capturadas:\n{_sources_payload(conn, story_id)}\n\n"
            + (f"Conversación previa:\n{history}\n\n" if history else "")
            + f"Pregunta de Jesús: {question}")


def run_chat(conn, story_id, pending_id, ask):
    """Completa la respuesta pendiente. Nunca lanza: deja el error escrito."""
    try:
        prompt = chat_prompt(conn, story_id, pending_id)

        def parse(obj):
            reply = obj.get("reply") if isinstance(obj, dict) else None
            if not isinstance(reply, str) or not reply.strip():
                raise ValueError("respuesta vacía")
            cite = obj.get("cite") if isinstance(obj.get("cite"), str) else None
            return reply.strip(), cite
        (reply, cite), model = ask(ne.CHAT_INSTRUCTIONS, prompt, parse, timeout=240)
        conn.execute("UPDATE news_chat SET state = 'done', text = ?, cite = ?, model = ? WHERE id = ?",
                     (_clip(reply, 20000), _clip(cite, 200) or None, model, pending_id))
    except Exception as exc:
        conn.execute("UPDATE news_chat SET state = 'failed', text = ? WHERE id = ?",
                     (f"No pude responder: {_clip(str(exc), 300)}", pending_id))


# ---------------------------------------------------------------- traducción

def translation(conn, source_id, lang="es"):
    row = conn.execute("SELECT state, title, blocks, model, error, updated_at_ms FROM news_translations "
                       "WHERE source_id = ? AND lang = ?", (source_id, lang)).fetchone()
    if not row:
        return None
    try:
        blocks = json.loads(row[2] or "[]")
    except ValueError:
        blocks = []
    return {"state": row[0], "title": row[1], "blocks": blocks, "model": row[3], "error": row[4], "updatedAt": row[5]}


def start_translation(conn, source_id, lang="es", now=None):
    """→ (arrancar, estado). Una traducción hecha se reutiliza; una fallida se reintenta."""
    src = ne.get_source(conn, source_id)
    if not src or not src.get("capture"):
        raise LookupError("esta fuente no tiene texto capturado")
    current = translation(conn, source_id, lang)
    if current and current["state"] in ("done", "running"):
        return False, current
    now = now or _now()
    with ne._tx(conn):
        conn.execute("INSERT INTO news_translations (source_id, lang, state, created_at_ms, updated_at_ms) "
                     "VALUES (?, ?, 'running', ?, ?) ON CONFLICT(source_id, lang) DO UPDATE SET state = 'running', "
                     "error = NULL, updated_at_ms = excluded.updated_at_ms", (source_id, lang, now, now))
    return True, translation(conn, source_id, lang)


def run_translation(conn, source_id, ask, lang="es", chunk=40):
    """Traduce título y bloques de texto por tandas; las imágenes se quedan."""
    try:
        src = ne.get_source(conn, source_id)
        cap = src["capture"]
        blocks = cap["blocks"]
        def translatable(b):
            return b.get("type") != "img" or bool(b.get("alt"))
        texts = [cap.get("title") or src["title"]] + [b.get("alt") if b.get("type") == "img" else b["text"]
                                                     for b in blocks if translatable(b)]
        out, model = [], None
        for start in range(0, len(texts), chunk):
            part = texts[start:start + chunk]

            def parse(obj, n=len(part)):
                got = obj.get("texts") if isinstance(obj, dict) else None
                if not isinstance(got, list) or len(got) != n:
                    raise ValueError("la traducción no trae los mismos textos")
                return [str(t) for t in got]
            got, model = ask(ne.TRANSLATE_INSTRUCTIONS, json.dumps(part, ensure_ascii=False), parse, timeout=300)
            out += got
        title, rest = out[0], iter(out[1:])
        translated = [b if not translatable(b) else dict(b, alt=next(rest)) if b.get("type") == "img"
                      else dict(b, text=next(rest)) for b in blocks]
        conn.execute("UPDATE news_translations SET state = 'done', title = ?, blocks = ?, model = ?, error = NULL, "
                     "updated_at_ms = ? WHERE source_id = ? AND lang = ?",
                     (_clip(title, 300), json.dumps(translated, ensure_ascii=False), model, _now(), source_id, lang))
    except Exception as exc:
        conn.execute("UPDATE news_translations SET state = 'failed', error = ?, updated_at_ms = ? "
                     "WHERE source_id = ? AND lang = ?", (_clip(str(exc), 300), _now(), source_id, lang))
