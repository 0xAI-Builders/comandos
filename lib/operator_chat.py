#!/usr/bin/env python3
"""Operador de ComandOS: chat local que dispara los mismos botones de la app.

No programa ni edita repos. Español latino (tú, no voseo).
"""
from __future__ import annotations

import json
import fcntl
import os
import re
import time
import unicodedata
import uuid
import tempfile
from pathlib import Path
from typing import Any, Callable

MAX_CONVERSATIONS = 30
MAX_MESSAGES = 80
MAX_MEMORY_CHARS = 8000

# Solo los baratos y rápidos del catálogo Claude / Codex / Grok.
# Sin effort de pensar (nada de high/xhigh). El parser NO es un modelo.
MODELS = [
    {"id": "haiku", "model": "haiku", "effort": "",
     "name": "Haiku", "intent": "Claude · barato · rápido"},
    {"id": "gpt-5.3-codex-spark", "model": "gpt-5.3-codex-spark", "effort": "",
     "name": "Codex Spark", "intent": "Codex · ultra rápido"},
    {"id": "grok-4.5", "model": "grok-4.5", "effort": "",
     "name": "Grok 4.5", "intent": "Grok · rápido, sin pensar"},
]


def model_entry(model: str, effort: str = "") -> dict:
    del effort
    model = str(model or "haiku")
    if model in ("local", "", "grok-4.6"):
        return MODELS[0]
    for item in MODELS:
        if item["model"] == model or item["id"] == model:
            return item
    return MODELS[0]

# Aceptamos voseo de entrada para que "pásate/pasate/mandá" igual funcionen.
_FOCUS_RE = re.compile(
    r"^(?:por\s+favor\s+|porfa\s+)?"
    r"(?:p[aá]sate|pasa(?:te)?|ve(?:te)?|cambia(?:r)?|abre|abrir|enfoca(?:r)?|"
    r"focus|ir|vamos|pon(?:te)?|mu[eé]strate|"
    r"tab|pesta[nñ]a)"
    r"(?:\s+(?:a|hacia|en|la|el|de))?\s+(.+)$",
    re.I,
)
_CLOSE_RE = re.compile(
    r"^(?:cierra|cerrar|quita(?:r)?|saca(?:r)?)\s+(?:la\s+pesta[nñ]a\s+)?"
    r"(?:a\s+)?(.+)$",
    re.I,
)
_RENAME_RE = re.compile(
    r"^(?:renombra(?:r)?)\s+(.+?)\s+(?:a|como)\s+(.+)$",
    re.I,
)
_REMEMBER_RE = re.compile(
    r"^(?:acu[eé]rdate|acordate|recuerda(?:me)?|remember)"
    r"(?:\s*(?:que|:))?\s+(.+)$",
    re.I,
)
_LIST_RE = re.compile(
    r"^(?:lista(?:r)?|cu[aá]les(?:\s+son)?|qu[eé]\s+tabs?(?:\s+hay)?"
    r"|tabs?\s+abiertas?|pesta[nñ]as(?:\s+abiertas)?|qu[eé]\s+hay"
    r"|mostr[aá](?:me)?(?:\s+las)?(?:\s+tabs?)?)\s*\??$",
    re.I,
)
_NEW_RE = re.compile(
    r"^(?:sesi[oó]n\s+nueva|nueva\s+sesi[oó]n|abrir\s+(?:el\s+)?wizard"
    r"|crea(?:r)?\s+(?:una\s+)?sesi[oó]n|nuevo\s+proyecto)\s*[.!]?\s*$",
    re.I,
)
_BACK_RE = re.compile(
    r"^(?:manda|mand[aá]|env[ií]a(?:r)?|tira)\s+(.+?)\s+"
    r"(?:atr[aá]s|al\s+final|fuera)\s*$",
    re.I,
)
_HELP_RE = re.compile(
    r"^(?:ayuda|help|qu[eé]\s+puedes(?:\s+hacer)?|comandos)\s*\??$",
    re.I,
)
_NEWCHAT_RE = re.compile(
    r"^(?:nueva\s+conversaci[oó]n|conversaci[oó]n\s+nueva|chat\s+nuevo)$",
    re.I,
)
_OPEN = r"(?:abre\s+|abrir\s+|open\s+|muestra(?:me)?\s+)?"
_UI = (
    (re.compile(rf"^{_OPEN}(?:ajustes|settings|preferencias)$", re.I),
     "settings", "Abro Ajustes."),
    (re.compile(rf"^{_OPEN}(?:analytics|uso|consumo|estad[ií]sticas)$", re.I),
     "analytics", "Abro Analytics."),
    (re.compile(rf"^{_OPEN}(?:notificaciones|notif|campana|avisos)$", re.I),
     "notif", "Abro notificaciones."),
    (re.compile(rf"^{_OPEN}(?:pomodoro|pomo|foco|timer)$", re.I),
     "pomo", "Abro Pomodoro."),
    (re.compile(rf"^{_OPEN}(?:snippets|recortes)$", re.I),
     "snippets", "Abro snippets."),
    (re.compile(rf"^{_OPEN}(?:remoto|remote)$", re.I),
     "remote", "Abro remoto."),
    (re.compile(r"^(?:mute|silencio|silenciar|sin\s+sonido)$", re.I),
     "mute", "Silencio."),
    (re.compile(r"^(?:unmute|sonido|con\s+sonido)$", re.I),
     "unmute", "Sonido on."),
    (re.compile(rf"^{_OPEN}(?:buscador|switcher|saltar)$", re.I),
     "switcher", "Abro el buscador."),
    (re.compile(rf"^{_OPEN}(?:soberan[ií]a|sovereignty)$", re.I),
     "sov", "Abro Soberanía."),
    (re.compile(rf"^{_OPEN}(?:servidores|ssh)$", re.I),
     "servers", "Abro servidores."),
    (re.compile(rf"^{_OPEN}(?:actividad|timeline|reciente)$", re.I),
     "timeline", "Actividad reciente."),
)
_THEME_RE = re.compile(
    r"^(?:(?:cambia(?:me)?|cambia(?:r)?|pon(?:me)?|usa|usar|set(?:ea)?)\s+)?"
    r"(?:el\s+|la\s+)?"
    r"(?:tema|theme|apariencia)"
    r"(?:\s+(?:a|al|de|en))?\s*(.*)$",
    re.I,
)
_THEMES = {
    "noche": "noche", "night": "noche", "orbita": "noche",
    "oscuro": "noche", "dark": "noche",
    "dia": "dia", "día": "dia", "day": "dia", "mineral": "dia",
    "claro": "dia", "light": "dia",
    "calido": "calido", "cálido": "calido", "ambar": "calido",
    "termius": "termius",
    "bruno": "bruno", "brunio": "bruno", "grafito": "bruno",
    "brown": "bruno", "cafe": "bruno", "café": "bruno",
    "glass": "superglass", "superglass": "superglass",
    "neon": "neon", "neón": "neon",
    "contraste": "contraste",
    "ubuntu": "ubuntu",
}
_SEND_RE = re.compile(
    r"^(?:env[ií]a(?:le)?|escribe|dile)\s+(?:a\s+)?(.+?)\s*[:]\s*(.+)$",
    re.I,
)
_SPLIT_RE = re.compile(
    r"^(?:(?:abre|abrir|abr|haz|hacer|pon)\s+(?:un\s+|una\s+)?)?"
    r"(?:split|parte|divide|partir|division)"
    r"(?:\s+(?:a\s+la\s+|al\s+|a\s+|en\s+)?)?"
    r"(derecha|izquierda|abajo|arriba)"
    r"(?:\s+(?:en|de|a)\s+(.+))?$",
    re.I,
)
_SPLIT_SIDES = ("derecha", "izquierda", "abajo", "arriba")
_TOGGLE_RE = re.compile(
    r"^(?:shell|consola|alternar(?:\s+ventana)?|ventana\s*1\s*/\s*2)$",
    re.I,
)
_KILL_RE = re.compile(
    r"^(?:mata|kill|termina(?:r)?)\s+(?:la\s+sesi[oó]n\s+(?:de\s+)?)?(.+)$",
    re.I,
)
_PAUSE_RE = re.compile(
    r"^(?:pausa|pause|det[eé]n(?:te)?)\s*(?:(.+))?$",
    re.I,
)
_RESUME_RE = re.compile(
    r"^(?:sigue|continua|contin[uú]a|despausa|resume)\s*(?:(.+))?$",
    re.I,
)
_RECOVER_RE = re.compile(
    r"^(?:recupera(?:r)?)\s+(.+)$",
    re.I,
)
_NEXT_RE = re.compile(r"^(?:siguiente|next)(?:\s+(?:tab|pesta[nñ]a))?$", re.I)
_PREV_RE = re.compile(r"^(?:anterior|prev|previa)(?:\s+(?:tab|pesta[nñ]a))?$", re.I)
_EXPORT_RE = re.compile(
    r"^(?:exporta|guarda)\s+(?:la\s+)?respuesta"
    r"(?:\s+de\s+(.+?))?"
    r"(?:\s+(?:como|en))?\s+(txt|pdf)$",
    re.I,
)
_COPY_RE = re.compile(
    r"^(?:copia(?:r)?)\s+(?:la\s+)?respuesta(?:\s+de\s+(.+))?$",
    re.I,
)
_REMOTE_RE = re.compile(
    r"^(?:remoto|remote)\s+(on|off|prende|apaga|activar?|desactivar?)$",
    re.I,
)
_CLOSE_SPLIT_RE = re.compile(
    r"^(?:cierra|cerrar)\s+(?:este\s+)?split$",
    re.I,
)
_KEY_RE = re.compile(
    r"^(?:dale|manda|env[ií]a(?:le)?)\s+"
    r"(enter|escape|esc|arriba|abajo|tab|y|n)"
    r"(?:\s+(?:a|en)\s+(.+))?$",
    re.I,
)
_KEY_MAP = {
    "enter": "Enter", "escape": "Escape", "esc": "Escape",
    "arriba": "Up", "abajo": "Down", "tab": "Tab", "y": "y", "n": "n",
}


def fold(text: str) -> str:
    raw = unicodedata.normalize("NFD", str(text or ""))
    return "".join(ch for ch in raw if unicodedata.category(ch) != "Mn").lower().strip()


def _clean_name(name: str) -> str:
    name = str(name or "").strip().strip("\"'“”«»")
    name = re.sub(r"\s+", " ", name)
    name = re.sub(r"[.?!]+$", "", name).strip()
    return name


def match_tab(query: str, tabs: list[dict]) -> dict | None:
    """Unique best match by label or session. None if empty/ambiguous/missing."""
    q = fold(_clean_name(query))
    if not q:
        return None
    scored: list[tuple[int, dict]] = []
    for tab in tabs:
        label = fold(tab.get("label") or "")
        sess = fold(tab.get("session") or "")
        if not label and not sess:
            continue
        if q == label or q == sess:
            scored.append((0, tab))
        elif label.startswith(q) or sess.startswith(q):
            scored.append((1, tab))
        elif q in label or q in sess:
            scored.append((2, tab))
    if not scored:
        return None
    scored.sort(key=lambda it: (it[0], -len(fold(it[1].get("label") or ""))))
    best_rank = scored[0][0]
    top = [t for r, t in scored if r == best_rank]
    if len(top) > 1 and best_rank != 0:
        return None
    return top[0]


def match_theme_name(name: str) -> str | None:
    q = fold(_clean_name(name))
    if not q:
        return None
    if q in _THEMES:
        return _THEMES[q]
    for key, val in _THEMES.items():
        if q.startswith(key) or key.startswith(q):
            return val
        if len(q) >= 4 and (q.startswith(key[:4]) or key.startswith(q[:4])):
            return val
    return None


def _loose_split(raw: str, tabs: list[dict], active: str | None) -> dict[str, Any] | None:
    folded = fold(re.sub(r"\s+", " ", raw))
    if "split" not in folded and "divide" not in folded and "partir" not in folded:
        return None
    side = next((s for s in _SPLIT_SIDES if s in folded), None)
    if not side:
        return None
    tab = _resolve_tab("", tabs, active)
    if not tab:
        return {"action": "unknown", "query": "tab actual"}
    return {
        "action": "split",
        "session": tab["session"],
        "label": tab.get("label") or tab["session"],
        "side": side,
    }


def _resolve_tab(name: str, tabs: list[dict], active: str | None = None) -> dict | None:
    if not name or fold(name) in ("esta", "esta tab", "aqui", "aquí", "actual"):
        if active:
            return next((t for t in tabs if t.get("session") == active), None)
        return tabs[0] if tabs else None
    return match_tab(name, tabs)


def parse_intent(text: str, tabs: list[dict], active: str | None = None) -> dict[str, Any]:
    raw = str(text or "").strip()
    if not raw:
        return {"action": "empty"}
    if _HELP_RE.match(raw):
        return {"action": "help"}
    if _LIST_RE.match(raw):
        return {"action": "list"}
    if _NEW_RE.match(raw):
        return {"action": "new_session"}
    if _NEWCHAT_RE.match(raw):
        return {"action": "new_chat"}
    if _NEXT_RE.match(raw):
        return {"action": "step", "delta": 1}
    if _PREV_RE.match(raw):
        return {"action": "step", "delta": -1}
    if _TOGGLE_RE.match(raw):
        return {"action": "toggle_shell"}
    if _CLOSE_SPLIT_RE.match(raw):
        return {"action": "close_split"}
    m = _COPY_RE.match(raw)
    if m:
        if m.group(1):
            tab = _resolve_tab(m.group(1), tabs, active)
            if not tab:
                return {"action": "unknown", "query": _clean_name(m.group(1))}
            return {"action": "copy_session", "session": tab["session"],
                    "label": tab.get("label") or tab["session"]}
        return {"action": "copy_reply"}
    for rx, target, reply in _UI:
        if rx.match(raw):
            return {"action": "ui", "target": target, "reply": reply}

    m = _REMOTE_RE.match(raw)
    if m:
        on = fold(m.group(1)) in ("on", "prende", "activa", "activar")
        return {"action": "remote", "on": on}

    m = _THEME_RE.match(raw)
    if m:
        theme = match_theme_name(m.group(1) or "")
        if not theme:
            return {"action": "unknown", "query": _clean_name(m.group(1) or "tema")}
        return {"action": "ui", "target": "theme", "value": theme,
                "reply": f"Tema: {theme}."}

    m = _REMEMBER_RE.match(raw)
    if m:
        return {"action": "remember", "fact": _clean_name(m.group(1))}

    m = _EXPORT_RE.match(raw)
    if m:
        fmt = fold(m.group(2))
        if m.group(1):
            tab = _resolve_tab(m.group(1), tabs, active)
            if not tab:
                return {"action": "unknown", "query": _clean_name(m.group(1))}
            return {"action": "export", "fmt": fmt, "session": tab["session"],
                    "label": tab.get("label") or tab["session"]}
        return {"action": "export", "fmt": fmt}

    m = _SEND_RE.match(raw)
    if m:
        tab = _resolve_tab(m.group(1), tabs, active)
        if not tab:
            return {"action": "unknown", "query": _clean_name(m.group(1))}
        return {"action": "send", "session": tab["session"],
                "label": tab.get("label") or tab["session"],
                "text": m.group(2).strip()}

    m = _SPLIT_RE.match(raw)
    if m:
        tab = _resolve_tab(m.group(2) or "", tabs, active)
        if not tab:
            return {"action": "unknown", "query": "tab actual"}
        return {"action": "split", "session": tab["session"],
                "label": tab.get("label") or tab["session"],
                "side": fold(m.group(1))}
    loose = _loose_split(raw, tabs, active)
    if loose:
        return loose

    m = _RENAME_RE.match(raw)
    if m:
        tab = match_tab(m.group(1), tabs)
        new = _clean_name(m.group(2))
        if not tab:
            return {"action": "unknown", "query": _clean_name(m.group(1))}
        return {"action": "rename", "session": tab["session"],
                "label": tab.get("label") or tab["session"], "new_label": new}

    m = _BACK_RE.match(raw)
    if m:
        tab = match_tab(m.group(1), tabs)
        if not tab:
            return {"action": "unknown", "query": _clean_name(m.group(1))}
        return {"action": "send_back", "session": tab["session"],
                "label": tab.get("label") or tab["session"]}

    m = _KILL_RE.match(raw)
    if m:
        tab = _resolve_tab(m.group(1), tabs, active)
        if not tab:
            return {"action": "unknown", "query": _clean_name(m.group(1))}
        return {"action": "kill", "session": tab["session"],
                "label": tab.get("label") or tab["session"]}

    m = _RESUME_RE.match(raw)
    if m and (m.group(1) or active):
        tab = _resolve_tab(m.group(1) or "", tabs, active)
        if tab:
            return {"action": "pause", "session": tab["session"], "on": False,
                    "label": tab.get("label") or tab["session"]}

    m = _PAUSE_RE.match(raw)
    if m and (m.group(1) or active):
        tab = _resolve_tab(m.group(1) or "", tabs, active)
        if tab:
            return {"action": "pause", "session": tab["session"], "on": True,
                    "label": tab.get("label") or tab["session"]}

    m = _RECOVER_RE.match(raw)
    if m:
        return {"action": "recover", "query": _clean_name(m.group(1))}

    m = _KEY_RE.match(raw)
    if m:
        key = _KEY_MAP.get(fold(m.group(1)))
        tab = _resolve_tab(m.group(2) or "", tabs, active)
        if not key:
            return {"action": "unknown", "query": m.group(1)}
        if not tab:
            return {"action": "unknown", "query": _clean_name(m.group(2) or "tab actual")}
        return {"action": "key", "session": tab["session"],
                "label": tab.get("label") or tab["session"], "key": key}

    m = _CLOSE_RE.match(raw)
    if m:
        tab = _resolve_tab(m.group(1), tabs, active)
        if not tab:
            return {"action": "unknown", "query": _clean_name(m.group(1))}
        return {"action": "close", "session": tab["session"],
                "label": tab.get("label") or tab["session"]}

    m = _FOCUS_RE.match(raw)
    target = _clean_name(m.group(1) if m else raw)
    tab = match_tab(target, tabs)
    if tab:
        return {"action": "focus", "session": tab["session"],
                "label": tab.get("label") or tab["session"]}
    return {"action": "unknown", "query": target}


def format_tab_list(tabs: list[dict]) -> str:
    names = [str(t.get("label") or t.get("session") or "") for t in tabs]
    names = [n for n in names if n]
    if not names:
        return "No hay pestañas abiertas."
    return "A la vista: " + " · ".join(names)


def help_text() -> str:
    return (
        "Parser local, sin modelo, instantáneo. Opero los botones de ComandOS:\n"
        "tabs: pásate a X · lista · siguiente · anterior · cierra X · "
        "renombra X a Y · manda X atrás · recupera X · buscador\n"
        "sesión: sesión nueva · envía a X: texto · dale enter/escape · "
        "split derecha · cierra split · shell · pausa · sigue · mata X\n"
        "app: ajustes · analytics · notificaciones · pomodoro · snippets · "
        "soberanía · servidores · actividad · silencio · "
        "remoto on/off · tema ubuntu\n"
        "memoria: acuérdate: … · copia respuesta · copia respuesta de X · "
        "exporta respuesta pdf · nueva conversación\n"
        "No programo: eso va al pane de la IA."
    )


def apply_intent(
    intent: dict,
    *,
    tabs: list[dict],
    memory: str,
    active: str | None = None,
    focus: Callable[[str], str | None] | None = None,
    close: Callable[[str], str | None] | None = None,
    rename: Callable[[str, str], str | None] | None = None,
    send_back: Callable[[str], str | None] | None = None,
    remember: Callable[[str], None] | None = None,
    kill: Callable[[str], str | None] | None = None,
    pause: Callable[[str], str | None] | None = None,
    send: Callable[[str, str], str | None] | None = None,
    split: Callable[[str, str], str | None] | None = None,
    recover: Callable[[str], str | None] | None = None,
    remote: Callable[[bool], str | None] | None = None,
    toggle_shell: Callable[[str], str | None] | None = None,
    close_split: Callable[[str], str | None] | None = None,
    key: Callable[[str, str], str | None] | None = None,
) -> dict[str, Any]:
    action = intent.get("action")
    if action == "empty":
        return {"reply": "Dime qué hacer: pestaña, lista, sesión nueva…", "actions": []}
    if action == "help":
        return {"reply": help_text(), "actions": []}
    if action == "list":
        extra = ("\nMemoria: " + memory.strip().splitlines()[0]) if memory.strip() else ""
        return {"reply": format_tab_list(tabs) + extra, "actions": [{"type": "list"}]}
    if action == "new_session":
        return {
            "reply": "Abro el asistente de sesión nueva.",
            "actions": [{"type": "open_wizard"}],
        }
    if action == "new_chat":
        return {
            "reply": "Nueva conversación.",
            "actions": [{"type": "new_chat"}],
        }
    if action == "ui":
        act = {"type": "ui", "target": intent.get("target"),
               "value": intent.get("value")}
        if intent.get("tab"):
            act["tab"] = intent.get("tab")
        return {"reply": intent.get("reply") or "Listo.", "actions": [act]}
    if action == "pref":
        key = str(intent.get("key") or "")
        on = bool(intent.get("on"))
        return {"reply": f"{key}: {'on' if on else 'off'}.",
                "actions": [{"type": "pref", "key": key, "on": on}]}
    if action == "volume":
        val = max(0, min(100, int(intent.get("value") or 0)))
        return {"reply": f"Volumen {val}%.",
                "actions": [{"type": "volume", "value": val}]}
    if action == "lang":
        return {"reply": f"Idioma: {intent.get('value')}.",
                "actions": [{"type": "lang", "value": intent.get("value")}]}
    if action == "notify_pos":
        return {"reply": f"Avisos: {intent.get('value')}.",
                "actions": [{"type": "notify_pos", "value": intent.get("value")}]}
    if action == "remember":
        fact = str(intent.get("fact") or "").strip()
        if not fact:
            return {"reply": "¿Qué quieres que recuerde?", "actions": []}
        if remember:
            remember(fact)
        return {"reply": "Guardado en memoria.", "actions": [{"type": "remember", "fact": fact}]}
    if action == "unknown":
        q = intent.get("query") or ""
        return {
            "reply": (
                f"No encontré «{q}». " + format_tab_list(tabs)
            ),
            "actions": [],
        }
    if action == "step":
        delta = int(intent.get("delta") or 1)
        ids = [t.get("session") for t in tabs if t.get("session")]
        if not ids:
            return {"reply": "No hay pestañas.", "actions": []}
        cur = active if active in ids else ids[0]
        nxt = ids[(ids.index(cur) + delta) % len(ids)]
        tab = next(t for t in tabs if t.get("session") == nxt)
        err = focus(nxt) if focus else None
        if err:
            return {"reply": err, "actions": []}
        return {"reply": f"Listo. {tab.get('label') or nxt}.",
                "actions": [{"type": "focus", "session": nxt}]}
    if action == "toggle_shell":
        sess = active or (tabs[0].get("session") if tabs else "")
        err = toggle_shell(str(sess)) if toggle_shell and sess else None
        if err:
            return {"reply": err, "actions": []}
        return {"reply": "Alterno agente / shell.",
                "actions": [{"type": "toggle_shell", "session": sess}]}
    if action == "close_split":
        sess = active or (tabs[0].get("session") if tabs else "")
        err = close_split(str(sess)) if close_split and sess else None
        if err:
            return {"reply": err, "actions": []}
        return {"reply": "Cierro este split.",
                "actions": [{"type": "close_split", "session": sess}]}
    if action == "copy_reply":
        return {"reply": "Copio la última respuesta.",
                "actions": [{"type": "copy_reply"}]}
    if action == "copy_session":
        sess = str(intent.get("session") or "")
        label = str(intent.get("label") or sess)
        return {"reply": f"Copio la respuesta de {label}.",
                "actions": [{"type": "copy_session", "session": sess}]}
    if action == "export":
        fmt = intent.get("fmt") or "txt"
        sess = intent.get("session") or active
        if not sess:
            return {"reply": "¿De cuál pestaña exporto la respuesta?", "actions": []}
        return {"reply": f"Exporto la respuesta como {fmt}.",
                "actions": [{"type": "export", "fmt": fmt, "session": sess}]}
    if action == "remote":
        on = bool(intent.get("on"))
        err = remote(on) if remote else None
        if err:
            return {"reply": err, "actions": []}
        return {"reply": "Remoto " + ("on." if on else "off."),
                "actions": [{"type": "remote", "on": on}]}
    if action == "recover":
        q = str(intent.get("query") or "")
        err = recover(q) if recover else None
        if err:
            return {"reply": err, "actions": []}
        return {"reply": f"Recupero {q}.", "actions": [{"type": "recover", "query": q}]}

    sess = str(intent.get("session") or "")
    label = str(intent.get("label") or sess)
    if action == "focus":
        err = focus(sess) if focus else None
        if err:
            return {"reply": err, "actions": []}
        return {"reply": f"Listo. {label}.", "actions": [{"type": "focus", "session": sess}]}
    if action == "close":
        err = close(sess) if close else None
        if err:
            return {"reply": err, "actions": []}
        return {"reply": f"Cerré la pestaña {label}.", "actions": [{"type": "close", "session": sess}]}
    if action == "rename":
        new = str(intent.get("new_label") or "").strip()
        err = rename(sess, new) if rename else None
        if err:
            return {"reply": err, "actions": []}
        return {
            "reply": f"Renombré {label} a {new}.",
            "actions": [{"type": "rename", "session": sess, "label": new}],
        }
    if action == "send_back":
        err = send_back(sess) if send_back else None
        if err:
            return {"reply": err, "actions": []}
        return {
            "reply": f"{label} atrás. {format_tab_list(tabs)}",
            "actions": [{"type": "send_back", "session": sess}],
        }
    if action == "kill":
        err = kill(sess) if kill else None
        if err:
            return {"reply": err, "actions": []}
        return {"reply": f"Maté la sesión {label}.",
                "actions": [{"type": "kill", "session": sess}]}
    if action == "pause":
        on = bool(intent.get("on", True))
        err = pause(sess, on) if pause else None
        if err:
            return {"reply": err, "actions": []}
        verb = "Pausé" if on else "Seguí"
        return {"reply": f"{verb} {label}.",
                "actions": [{"type": "pause", "session": sess, "on": on}]}
    if action == "key":
        k = str(intent.get("key") or "")
        err = key(sess, k) if key else None
        if err:
            return {"reply": err, "actions": []}
        return {"reply": f"{k} → {label}.",
                "actions": [{"type": "key", "session": sess, "key": k}]}
    if action == "send":
        body = str(intent.get("text") or "")
        err = send(sess, body) if send else None
        if err:
            return {"reply": err, "actions": []}
        return {"reply": f"Enviado a {label}.",
                "actions": [{"type": "send", "session": sess}]}
    if action == "split":
        side = str(intent.get("side") or "right")
        err = split(sess, side) if split else None
        if err:
            return {"reply": err, "actions": []}
        return {"reply": f"Split {side} en {label}.",
                "actions": [{"type": "split", "session": sess, "side": side}]}
    return {"reply": help_text(), "actions": []}


def default_root(home: str | None = None) -> Path:
    base = Path(home or os.path.expanduser("~"))
    return base / ".claude" / "hooks" / "operator"


def _read_json(path: Path, fallback):
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        return data
    except Exception:
        return fallback


def load_store(root: Path) -> dict:
    data = _read_json(root / "conversations.json", None)
    if not isinstance(data, dict):
        data = _read_json(root / "conversations.json.bak", {})
    if not isinstance(data, dict):
        data = {}
    convos = data.get("conversations")
    if not isinstance(convos, list):
        convos = []
    current = str(data.get("current") or "")
    if not current and convos:
        current = str(convos[0].get("id") or "")
    model = str(data.get("model") or "haiku")
    effort = str(data.get("effort") or "")
    entry = model_entry(model, effort)
    return {
        "current": current,
        "conversations": convos,
        "model": entry["model"],
        "effort": entry["effort"],
    }


def set_model(store: dict, model: str, effort: str = "") -> dict:
    entry = model_entry(model, effort)
    store["model"] = entry["model"]
    store["effort"] = entry["effort"]
    return entry


def llm_system_prompt() -> str:
    return (
        "Eres el chat de ComandOS. Español latino (tú, no voseo). "
        "Responde corto y útil. No menciones HTTP ni 401. "
        "No te pongas a razonar en voz alta."
    )


def save_store(root: Path, store: dict) -> None:
    root.mkdir(parents=True, exist_ok=True)
    path = root / "conversations.json"
    with (root / "conversations.lock").open("a") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        existing = _read_json(path, {})
        if not isinstance(existing, dict):
            existing = _read_json(root / "conversations.json.bak", {})
        existing = existing if isinstance(existing, dict) else {}
        conversations = {c["id"]: c for c in existing.get("conversations", []) if isinstance(c, dict) and c.get("id")}
        for convo in store.get("conversations", []):
            if not isinstance(convo, dict) or not convo.get("id"):
                continue
            old = conversations.get(convo["id"], {})
            messages = {}
            for message in list(old.get("messages", [])) + list(convo.get("messages", [])):
                key = message.get("id") or json.dumps([message.get("ts"), message.get("role"), message.get("text")])
                messages[key] = message
            newer = convo if convo.get("updated", 0) >= old.get("updated", 0) else old
            conversations[convo["id"]] = {**newer, "messages": sorted(messages.values(), key=lambda m: m.get("ts", 0))[-MAX_MESSAGES:]}
        merged = {**existing, **store, "conversations": sorted(conversations.values(), key=lambda c: c.get("updated", 0), reverse=True)[:MAX_CONVERSATIONS]}
        def atomic(target, data):
            fd, name = tempfile.mkstemp(dir=root, prefix=".chat-")
            try:
                with os.fdopen(fd, "w") as output:
                    json.dump(data, output, ensure_ascii=False)
                    output.flush()
                    os.fsync(output.fileno())
                os.replace(name, target)
            finally:
                if os.path.exists(name):
                    os.unlink(name)
        if existing.get("conversations"):
            atomic(root / "conversations.json.bak", existing)
        atomic(path, merged)


def save_conversation(root: Path, store: dict, convo: dict) -> None:
    save_store(root, {**store, "conversations": [convo]})


def load_memory(root: Path) -> str:
    path = root / "memory.md"
    try:
        text = path.read_text(encoding="utf-8")
    except Exception:
        return ""
    return text[:MAX_MEMORY_CHARS]


def save_memory(root: Path, text: str) -> None:
    root.mkdir(parents=True, exist_ok=True)
    (root / "memory.md").write_text(str(text)[:MAX_MEMORY_CHARS], encoding="utf-8")


def append_memory(root: Path, fact: str) -> None:
    fact = _clean_name(fact)
    if not fact:
        return
    cur = load_memory(root).rstrip()
    line = f"- {fact}"
    if line.lower() in fold(cur):
        return
    nxt = (cur + "\n" + line).strip() + "\n"
    save_memory(root, nxt)


def new_conversation(store: dict) -> dict:
    cid = str(uuid.uuid4())
    now = time.time()
    item = {
        "id": cid,
        "title": "Nueva conversación",
        "created": now,
        "updated": now,
        "messages": [],
    }
    convos = [item] + [c for c in store.get("conversations") or [] if isinstance(c, dict)]
    store["conversations"] = convos[:MAX_CONVERSATIONS]
    store["current"] = cid
    return item


def get_conversation(store: dict, cid: str | None = None) -> dict | None:
    want = cid or store.get("current")
    for item in store.get("conversations") or []:
        if isinstance(item, dict) and item.get("id") == want:
            return item
    return None


def append_message(convo: dict, role: str, text: str, actions=None) -> None:
    msgs = list(convo.get("messages") or [])
    msgs.append({
        "id": str(uuid.uuid4()),
        "role": role,
        "text": str(text or "")[:4000],
        "ts": time.time(),
        "actions": actions or [],
    })
    convo["messages"] = msgs[-MAX_MESSAGES:]
    convo["updated"] = time.time()
    if role == "user" and (convo.get("title") in ("", "Nueva conversación")):
        title = str(text or "").strip().replace("\n", " ")
        convo["title"] = (title[:48] + "…") if len(title) > 48 else (title or convo["title"])


def public_store(store: dict) -> dict:
    convos = []
    for item in store.get("conversations") or []:
        if not isinstance(item, dict):
            continue
        convos.append({
            "id": item.get("id"),
            "title": item.get("title") or "Conversación",
            "updated": item.get("updated") or 0,
            "n": len(item.get("messages") or []),
        })
    entry = model_entry(store.get("model") or "local", store.get("effort") or "")
    return {
        "current": store.get("current"),
        "conversations": convos,
        "model": entry["model"],
        "effort": entry["effort"],
        "modelName": entry["name"],
        "models": MODELS,
    }
