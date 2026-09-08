"""Ejecuta un ToolSpec del catálogo según su target. No sabe de LLMs."""
from __future__ import annotations

import json
import os
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Callable

import operator_catalog as cat

LOCAL_INTENTS = (
    "focus", "step_next", "step_prev", "close", "rename", "send_back", "recover",
    "toggle_shell", "kill", "send", "key", "choose_option", "interrupt", "pause_on",
    "pause_off", "split", "close_split", "optimization_apply", "pref", "voice", "lang",
    "theme", "notify_pos", "ui_settings", "ui_panel", "snippet_send", "remember",
    "memory_read", "memory_replace", "copy_reply", "copy_session",
)

_MISSING = object()


def substitute(template: Any, args: dict) -> Any:
    if isinstance(template, str) and template.startswith("$"):
        return args.get(template[1:], _MISSING)
    if isinstance(template, dict):
        out = {}
        for k, v in template.items():
            val = substitute(v, args)
            if val is not _MISSING and val is not None:
                out[k] = val
        return out
    if isinstance(template, list):
        return [x for x in (substitute(v, args) for v in template) if x is not _MISSING]
    return template


def _summarize(data: Any, limit: int = 1800) -> str:
    try:
        text = json.dumps(data, ensure_ascii=False)
    except Exception:
        text = str(data)
    return text if len(text) <= limit else text[:limit] + "…"


def _fail(msg: str) -> dict:
    return {"ok": False, "reply": msg, "actions": [], "data": None}


class Dispatcher:
    def __init__(self, *, base_url: str, token: str, hooks_dir: str,
                 local_handlers: dict[str, Callable[[dict], dict]],
                 http_post=None, http_get=None):
        self.base_url = base_url.rstrip("/")
        self.token = token
        self.hooks_dir = hooks_dir
        self.local_handlers = local_handlers
        self._post = http_post or self._default_post
        self._get = http_get or self._default_get

    def _default_post(self, path, body):
        req = urllib.request.Request(self.base_url + path, data=json.dumps(body).encode(), method="POST",
                                     headers={"Content-Type": "application/json", "X-Comandos-Token": self.token})
        with urllib.request.urlopen(req, timeout=20) as r:
            return json.loads(r.read().decode("utf-8", "replace") or "{}")

    def _default_get(self, path, query):
        url = self.base_url + path
        clean = {k: v for k, v in (query or {}).items() if v not in (None, "")}
        if clean:
            url += "?" + urllib.parse.urlencode(clean)
        req = urllib.request.Request(url, headers={"X-Comandos-Token": self.token})
        with urllib.request.urlopen(req, timeout=20) as r:
            return json.loads(r.read().decode("utf-8", "replace") or "{}")

    def run(self, name: str, args: dict | None) -> dict:
        args = dict(args or {})
        spec = cat.by_name(name)
        if spec is None:
            return _fail(f"No existe la acción '{name}'.")
        if spec.destructive and args.get("confirm") is not True:
            return _fail(f"'{name}' es destructiva: pide confirmación al usuario y vuelve a llamar con confirm=true.")
        missing = [r for r in spec.required if args.get(r) in (None, "")]
        if missing:
            return _fail(f"Faltan parámetros: {', '.join(missing)}.")
        kind = spec.target["kind"]
        try:
            if kind == "api":
                return self._run_api(spec, args)
            if kind == "ui":
                return self._run_ui(spec, args)
            if kind == "app":
                return self._run_app(spec, args)
            return self._run_local(spec, args)
        except urllib.error.HTTPError as e:
            try:
                detail = json.loads(e.read().decode("utf-8", "replace")).get("error") or str(e)
            except Exception:
                detail = str(e)
            return _fail(f"No pude ({name}): {detail}")
        except Exception as e:
            return _fail(f"No pude ({name}): {str(e)[:200]}")

    def _run_api(self, spec, args):
        t = spec.target
        if t["method"] == "GET":
            data = self._get(t["path"], substitute(t.get("query") or {}, args))
        else:
            data = self._post(t["path"], substitute(t.get("body") or {}, args))
        err = data.get("error") if isinstance(data, dict) else None
        reply = _summarize(data) if spec.readonly else (str(err) if err else "Hecho.")
        return {"ok": not err, "reply": reply, "actions": [], "data": data}

    def _run_ui(self, spec, args):
        t = spec.target
        if t["op"] == "click":
            action = {"type": "ui", "op": "click", "selector": t["selector"]}
        elif t["op"] == "call":
            vals = [substitute(x, args) for x in t.get("args", [])]
            action = {"type": "ui", "op": "call", "fn": t["fn"], "args": [v if v is not _MISSING else None for v in vals]}
        else:
            action = {"type": "ui", "op": "term", "term": {"type": t["type"], **{k: v for k, v in args.items() if k != "confirm"}}}
        return {"ok": True, "reply": "Hecho en el tablero.", "actions": [action], "data": None}

    def _run_app(self, spec, args):
        path = os.path.join(self.hooks_dir, "app-command.json")
        payload = {"command": spec.target["command"], "args": {k: v for k, v in args.items() if k != "confirm"}, "ts": time.time()}
        tmp = path + ".tmp"
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump(payload, f)
        os.replace(tmp, path)
        return {"ok": True, "reply": "Enviado a la app de escritorio.", "actions": [], "data": None}

    def _run_local(self, spec, args):
        fn = self.local_handlers.get(spec.target["intent"])
        if fn is None:
            return _fail(f"Acción local '{spec.target['intent']}' no disponible.")
        out = dict(fn(args) or {})
        out.setdefault("ok", True)
        out.setdefault("reply", "Hecho.")
        out.setdefault("actions", [])
        out.setdefault("data", None)
        return out
