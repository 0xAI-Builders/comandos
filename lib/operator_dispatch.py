"""Ejecuta un ToolSpec del catálogo según su target. No sabe de LLMs."""
from __future__ import annotations

import json
import os
import threading
import time
import uuid
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
# cc-app lee y borra un único app-command.json: un escritor a la vez.
_APP_LOCK = threading.Lock()


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


def _summarize(data: Any, limit: int = 16000) -> str:
    try:
        text = json.dumps(data, ensure_ascii=False)
    except Exception:
        text = str(data)
    if len(text) <= limit:
        return text
    def compact(value, count, depth=0):
        if depth > 8:
            return {'omitted': True}
        if isinstance(value, dict):
            items = list(value.items())
            out = {k: compact(v, count, depth + 1) for k, v in items[:max(32, count)]}
            if len(items) > max(32, count):
                out['_omittedKeys'] = len(items) - max(32, count)
            return out
        if isinstance(value, list):
            out = [compact(v, count, depth + 1) for v in value[:count]]
            if len(value) > count:
                out.append({'_omittedItems': len(value) - count})
            return out
        if isinstance(value, str) and len(value) > 300:
            return value[:300] + ' [texto truncado]'
        return value
    for count in (24, 8, 2, 0):
        text = json.dumps({'truncated': True, 'data': compact(data, count)}, ensure_ascii=False)
        if len(text) <= limit:
            return text
    return json.dumps({'truncated': True, 'data': None, 'reason': 'Resultado demasiado grande; consulta un ámbito más concreto.'}, ensure_ascii=False)


def _fail(msg: str) -> dict:
    return {"ok": False, "reply": msg, "actions": [], "data": None}


class Dispatcher:
    def __init__(self, *, base_url: str, token: str, hooks_dir: str,
                 local_handlers: dict[str, Callable[[dict], dict]],
                 http_post=None, http_get=None, receipt_root=None,
                 default_session=None, default_pane=None, readonly=False):
        self.base_url = base_url.rstrip("/")
        self.token = token
        self.hooks_dir = hooks_dir
        self.local_handlers = local_handlers
        self._post = http_post or self._default_post
        self._get = http_get or self._default_get
        self.receipt_root = receipt_root
        self.default_session = default_session
        self.default_pane = default_pane
        self.readonly = readonly
        self.context = {'session': default_session, 'pane': default_pane}
        self.app_wait = 2.0      # lo que la app tiene para recoger su comando
        self.app_stale = 30.0    # un comando sin recoger más viejo que esto se da por abandonado

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
        if args is not None and not isinstance(args, dict):
            return _fail('Los argumentos de la herramienta deben ser un objeto.')
        args = dict(args or {})
        spec = cat.by_name(name)
        if spec and self.readonly and not spec.readonly:
            return _fail('Esta consulta es de análisis. Presenta la recomendación; aplicarla requiere una petición explícita posterior.')
        all_scope = name == 'extension_usage' and args.get('scope') == 'all'
        session_scope = name in ('extension_usage', 'session_status', 'session_usage') and args.get('scope') == 'session'
        if session_scope:
            args.pop('pane', None)
        if all_scope:
            args.pop('session', None)
            args.pop('pane', None)
        if spec and self.default_session and not all_scope:
            if "session" in spec.params and not args.get("session"):
                args["session"] = self.default_session
            target = args.get('session') or args.get('tab') or self.default_session
            if "pane" in spec.params and self.default_pane and not args.get("pane") and target == self.default_session and not session_scope:
                args["pane"] = self.default_pane
        receipt_id = None
        if self.receipt_root and spec and not spec.readonly:
            import operator_receipts
            receipt_id = operator_receipts.create(self.receipt_root, name)
        result = self._execute(name, args)
        if receipt_id:
            status = "failed" if not result.get("ok") or result.get("error") else "dispatched" if result.get("pending") or result.get("actions") or spec.target["kind"] == "app" else "confirmed"
            data = result.get("data") or {}
            reference = {k: data[k] for k in ("operationId", "operationKey") if isinstance(data, dict) and k in data}
            operator_receipts.acknowledge(self.receipt_root, {"actionId": receipt_id, "status": status,
                "detail": json.dumps({"session": args.get("session", self.default_session), "pane": args.get("pane"), **reference})})
            result["receiptId"] = receipt_id
        return result

    def _execute(self, name: str, args: dict | None) -> dict:
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
        if spec.name in ('session_brain', 'list_session_profiles'):
            rows = self._get('/state', {})
            rows = self._scoped_rows(rows, args, live=True)
            if len(rows) != 1:
                return _fail('Selecciona un panel vivo exacto para consultar su configuración.')
            row = rows[0]
            if args.get('cwd') and args['cwd'] != row.get('cwd'):
                return _fail('La carpeta solicitada no coincide con el panel seleccionado.')
            profile = spec.name == 'list_session_profiles'
            args = {**args, 'pane': row.get('pane'), 'cwd': row.get('cwd'),
                    'harness': (args.get('harness') if profile else None) or row.get('agent'),
                    'account': (args.get('account') if profile else None) or row.get('harnessAccount') or row.get('account')}
        if t["method"] == "GET":
            data = self._get(t["path"], substitute(t.get("query") or {}, args))
        else:
            data = self._post(t["path"], substitute(t.get("body") or {}, args))
        err = data.get("error") if isinstance(data, dict) else None
        if isinstance(data, dict) and data.get("ok") is False and not err:
            err = data.get("message") or "La operación no se completó."
        if not err and spec.name == 'session_status':
            data = {'scope': {'session': args['session'], 'pane': args.get('pane')},
                    'source': '/state', 'sessions': self._scoped_rows(data, args, live=not args.get('historical'))}
        elif not err and spec.name == 'session_usage':
            rows = self._scoped_rows(data.get('panes') or [], args)
            data = {'scope': {'session': args['session'], 'pane': args.get('pane')},
                    'source': '/usage/state', 'generated_at': data.get('generated_at'), 'panes': rows,
                    'attribution': 'reported' if rows else 'unavailable',
                    'coverage': 'Conserva confidence de cada fila. Uso compartido/local por carpeta no equivale a uso exacto de este panel; no sumes contadores compartidos entre paneles. Sin fila no significa cero.'}
        pending = bool(isinstance(data, dict) and
                       (data.get("pending") or data.get("queued") or data.get("operationKey")))
        reply = _summarize(data) if spec.readonly else (str(err) if err else
                "Cambio solicitado; consulta su estado para confirmar el resultado." if pending else "Hecho.")
        if not spec.readonly and isinstance(data, dict) and (data.get('operationKey') or data.get('operationId')):
            reply = _summarize({'ok': not bool(err), 'pending': pending and not bool(err), 'message': reply,
                                'data': {k: data[k] for k in ('operationKey', 'operationId', 'state', 'error') if k in data}})
        return {"ok": not err, "pending": pending and not bool(err), "reply": reply, "actions": [], "data": data}

    @staticmethod
    def _scoped_rows(rows, args, live=False):
        return [row for row in rows if isinstance(row, dict)
                and (row.get('session') or row.get('tmux_session')) == args.get('session')
                and (not args.get('pane') or (row.get('pane') or row.get('tmux_pane')) == args['pane'])
                and (not live or row.get('alive') is True)]

    def _run_ui(self, spec, args):
        t = spec.target
        if t["op"] == "click":
            action = {"type": "ui", "op": "click", "selector": t["selector"]}
        elif t["op"] == "call":
            vals = [substitute(x, args) for x in t.get("args", [])]
            action = {"type": "ui", "op": "call", "fn": t["fn"], "args": [v if v is not _MISSING else None for v in vals]}
        else:
            action = {"type": "ui", "op": "term", "term": {"type": t["type"], **{k: v for k, v in args.items() if k != "confirm"}}}
        return {"ok": True, "pending": True, "reply": "Acción enviada al tablero; falta confirmar su ejecución.", "actions": [action], "data": None}

    def _wait_gone(self, path):
        end = time.monotonic() + self.app_wait
        while os.path.exists(path) and time.monotonic() < end:
            time.sleep(0.02)
        return not os.path.exists(path)

    def _run_app(self, spec, args):
        # Contrato con cc-app (on_app_command): un solo archivo que la app lee y
        # borra. Solo es éxito si la app lo recoge; si no, se retira y es fallo.
        path = os.path.join(self.hooks_dir, "app-command.json")
        payload = {"command": spec.target["command"], "args": {k: v for k, v in args.items() if k != "confirm"}, "ts": time.time()}
        with _APP_LOCK:
            if not self._wait_gone(path):
                try:
                    abandoned = time.time() - os.path.getmtime(path) > self.app_stale
                except OSError:
                    abandoned = False
                if not abandoned:
                    return _fail("La app de escritorio aún no recogió el comando anterior; no lo piso. ¿Está abierta?")
                try:
                    os.remove(path)
                except FileNotFoundError:
                    pass
            tmp = f"{path}.{uuid.uuid4().hex}.tmp"
            with open(tmp, "w", encoding="utf-8") as f:
                json.dump(payload, f)
            os.replace(tmp, path)
            if not self._wait_gone(path):
                # Borrar arbitra con la app: solo uno de los dos os.remove gana, y si
                # gana este la app no lo ejecuta (on_app_command sale al fallar su remove).
                try:
                    os.remove(path)
                    return _fail("La app de escritorio no recogió el comando; ¿está abierta? No se ejecutó.")
                except FileNotFoundError:
                    pass
        return {"ok": True, "reply": "La app de escritorio recogió el comando.", "actions": [], "data": None}

    def _run_local(self, spec, args):
        fn = self.local_handlers.get(spec.target["intent"])
        if fn is None:
            return _fail(f"Acción local '{spec.target['intent']}' no disponible.")
        out = dict(fn(args) or {})
        # Un handler que devuelve error (con o sin ok) no puede acabar en "Hecho."
        if out.get("error"):
            out["ok"] = False
            out.setdefault("reply", str(out["error"]))
        out.setdefault("ok", True)
        out.setdefault("reply", "Hecho." if out["ok"] else "La acción no se completó.")
        out.setdefault("actions", [])
        out.setdefault("data", None)
        if out.get("actions"):
            out["pending"] = True
            out["reply"] = "Acción enviada al tablero; falta confirmar su ejecución."
        return out
