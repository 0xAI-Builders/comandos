#!/usr/bin/env python3
"""Cliente ACP (Agent Client Protocol) de ComandOS.

ComandOS es el CLIENTE: lanza al agente del vendor como subproceso y habla
JSON-RPC 2.0 por stdio. Cada agente corre con SU login de suscripción (nunca
API keys): claude-agent-acp (Claude Code headless), codex-acp, `grok agent
stdio`, `opencode acp`. Antigravity (agy) no habla ACP todavía: se cubre con
un transporte stream-json que expone la misma interfaz.

Eventos normalizados que emite una sesión (dicts):
  {"type": "text", "text"}            trozo de respuesta del agente
  {"type": "thought", "text"}         razonamiento visible
  {"type": "tool", "title", "status", "kind"}
  {"type": "plan", "entries"}
  {"type": "permission", "title", "options", "reply": callable}
  {"type": "end", "stopReason"}
  {"type": "error", "message"}
"""
from __future__ import annotations

import json
import os
import queue
import shlex
import shutil
import subprocess
import threading
import time
from typing import Any, Callable

PROTOCOL_VERSION = 1
_USER_BIN_DIRS = ("~/.local/bin", "~/.bun/bin", "~/.cargo/bin", "~/.npm-global/bin",
                  "~/.opencode/bin", "~/.grok/bin", "~/bin", "/usr/local/bin")


class AcpError(RuntimeError):
    pass


def which(name: str) -> str | None:
    hit = shutil.which(name)
    if hit:
        return hit
    for directory in _USER_BIN_DIRS:
        candidate = os.path.join(os.path.expanduser(directory), name)
        if os.path.isfile(candidate) and os.access(candidate, os.X_OK):
            return candidate
    return None


# ---------------------------------------------------------------- agentes
def agent_specs(registry: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return dict((registry or {}).get("acpAgents") or {})


def agent_available(spec: dict[str, Any]) -> bool:
    argv = spec.get("command") or []
    return bool(argv) and which(str(argv[0])) is not None and all(
        which(str(dep)) is not None for dep in spec.get("requires") or [])


def build_command(spec: dict[str, Any], *, model: str = "", effort: str = "",
                  danger: bool = False, extra_env: dict[str, str] | None = None) -> tuple[list[str], dict[str, str]]:
    """argv + env para lanzar el agente. Los templates del registro son
    listas de args con {model}/{effort}; nunca se pasan por un shell."""
    template = [str(a) for a in spec.get("command") or []]
    if not template:
        raise AcpError("agente sin comando")
    groups = {
        "{model_args}": [str(a).replace("{model}", model) for a in spec.get("modelArgs") or []] if model else [],
        "{effort_args}": [str(a).replace("{effort}", effort) for a in spec.get("effortArgs") or []] if effort else [],
        "{danger_args}": [str(a) for a in spec.get("dangerArgs") or []] if danger else [],
    }
    # Los grupos se insertan donde el template ponga su placeholder (grok exige
    # las opciones ANTES del subcomando `stdio`); sin placeholder van al final.
    argv: list[str] = []
    placed = set()
    for token in template:
        if token in groups:
            argv += groups[token]
            placed.add(token)
        else:
            argv.append(token)
    for key, extra in groups.items():
        if key not in placed:
            argv += extra
    resolved = which(argv[0])
    if not resolved:
        raise AcpError(f"{argv[0]} no está instalado")
    argv[0] = resolved
    env = {}
    for key, value in (spec.get("env") or {}).items():
        value = str(value)
        if value.startswith("which:"):
            hit = which(value[len("which:"):])
            if not hit:
                continue
            value = hit
        env[str(key)] = os.path.expanduser(value)
    if model and spec.get("modelEnv"):
        env[str(spec["modelEnv"])] = str(spec.get("modelEnvTemplate") or "{model}").replace("{model}", model)
    if effort and spec.get("effortEnv"):
        mapping = spec.get("effortEnvMap") or {}
        env[str(spec["effortEnv"])] = str(mapping.get(effort, effort))
    env.update(extra_env or {})
    return argv, env


def shell_command(argv: list[str], env: dict[str, str]) -> str:
    prefix = " ".join(f"{k}={shlex.quote(v)}" for k, v in env.items())
    return (prefix + " " if prefix else "") + " ".join(shlex.quote(a) for a in argv)


# ---------------------------------------------------------------- transporte
class _Proc:
    def __init__(self, argv: list[str], env: dict[str, str], cwd: str):
        full_env = dict(os.environ)
        full_env.update(env)
        self.proc = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, text=True, bufsize=1,
                                     cwd=cwd, env=full_env)
        self.lines: queue.Queue = queue.Queue()
        self.stderr: list[str] = []
        threading.Thread(target=self._pump, daemon=True).start()
        threading.Thread(target=self._pump_err, daemon=True).start()

    def _pump(self):
        try:
            for line in self.proc.stdout:
                line = line.strip()
                if not line:
                    continue
                try:
                    self.lines.put(json.loads(line))
                except ValueError:
                    self.stderr.append(line)
        finally:
            self.lines.put(None)

    def _pump_err(self):
        try:
            for line in self.proc.stderr:
                self.stderr.append(line.rstrip("\n"))
                del self.stderr[:-200]
        except Exception:
            pass

    def write(self, obj: dict[str, Any]):
        try:
            self.proc.stdin.write(json.dumps(obj) + "\n")
            self.proc.stdin.flush()
        except (BrokenPipeError, OSError) as exc:
            raise AcpError("el agente cerró la conexión: " + " | ".join(self.stderr[-3:])) from exc

    def alive(self) -> bool:
        return self.proc.poll() is None

    def close(self):
        try:
            self.proc.stdin.close()
        except Exception:
            pass
        try:
            self.proc.terminate()
            self.proc.wait(timeout=3)
        except Exception:
            try:
                self.proc.kill()
            except Exception:
                pass


class AcpSession:
    """Una conversación con un agente ACP por stdio."""

    def __init__(self, argv: list[str], env: dict[str, str], cwd: str,
                 permission_handler: Callable[[dict[str, Any]], str | None] | None = None):
        self.cwd = cwd
        self.argv, self.env = argv, env
        self.permission_handler = permission_handler
        self.proc = _Proc(argv, env, cwd)
        self._id = 0
        self.session_id = ""
        self.agent_capabilities: dict[str, Any] = {}
        self.auth_methods: list[dict[str, Any]] = []
        self.models: list[dict[str, Any]] = []
        self.current_model = ""
        self.modes: list[dict[str, Any]] = []
        self.current_mode = ""
        self.commands: list[dict[str, Any]] = []

    # -- rpc
    def _request(self, method: str, params: dict[str, Any]) -> int:
        self._id += 1
        self.proc.write({"jsonrpc": "2.0", "id": self._id, "method": method, "params": params})
        return self._id

    def _respond(self, id_: Any, result: Any = None, error: dict[str, Any] | None = None):
        msg: dict[str, Any] = {"jsonrpc": "2.0", "id": id_}
        if error:
            msg["error"] = error
        else:
            msg["result"] = result if result is not None else {}
        self.proc.write(msg)

    def _notify(self, method: str, params: dict[str, Any]):
        self.proc.write({"jsonrpc": "2.0", "method": method, "params": params})

    def _pump_until(self, want_id: int | None, on_event: Callable[[dict[str, Any]], None] | None,
                    timeout: float) -> dict[str, Any]:
        deadline = time.time() + timeout
        while time.time() < deadline:
            try:
                msg = self.proc.lines.get(timeout=0.25)
            except queue.Empty:
                if not self.proc.alive():
                    raise AcpError("el agente murió: " + " | ".join(self.proc.stderr[-3:]))
                continue
            if msg is None:
                raise AcpError("el agente cerró stdout: " + " | ".join(self.proc.stderr[-3:]))
            if "method" in msg:
                self._handle_incoming(msg, on_event)
                continue
            if msg.get("id") == want_id:
                if "error" in msg:
                    err = msg["error"] or {}
                    detail = err.get("data")
                    text = str(err.get("message") or "error del agente")
                    if isinstance(detail, dict) and detail.get("message"):
                        text += ": " + str(detail["message"])[:400]
                    raise AcpError(text)
                return msg.get("result") or {}
        raise AcpError(f"timeout esperando respuesta ({timeout:.0f}s)")

    def _handle_incoming(self, msg: dict[str, Any], on_event):
        method, params, rid = msg.get("method"), msg.get("params") or {}, msg.get("id")
        if method == "session/update":
            self._session_update(params.get("update") or {}, on_event)
        elif method == "session/request_permission":
            self._permission(params, rid, on_event)
        elif method == "fs/read_text_file":
            self._fs_read(params, rid)
        elif method == "fs/write_text_file":
            self._fs_write(params, rid)
        elif rid is not None:
            # capacidades que no implementamos (terminal/*): declararlo honesto
            self._respond(rid, error={"code": -32601, "message": f"{method} no soportado por cc-acp"})

    def _fs_read(self, params, rid):
        try:
            with open(params.get("path") or "", encoding="utf-8", errors="replace") as handle:
                lines = handle.read().splitlines(True)
            start = max(int(params.get("line") or 1) - 1, 0)
            limit = params.get("limit")
            chunk = lines[start:start + int(limit)] if limit else lines[start:]
            self._respond(rid, {"content": "".join(chunk)})
        except Exception as exc:
            self._respond(rid, error={"code": -32603, "message": str(exc)})

    def _fs_write(self, params, rid):
        try:
            path = params.get("path") or ""
            os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
            with open(path, "w", encoding="utf-8") as handle:
                handle.write(params.get("content") or "")
            self._respond(rid, {})
        except Exception as exc:
            self._respond(rid, error={"code": -32603, "message": str(exc)})

    def _permission(self, params, rid, on_event):
        options = params.get("options") or []
        call = params.get("toolCall") or {}
        decision = None
        if self.permission_handler:
            decision = self.permission_handler({"title": call.get("title") or "", "kind": call.get("kind") or "",
                                                "options": options, "raw": params})
        if decision is None:
            allow = next((o for o in options if "allow" in str(o.get("kind") or "")), None)
            decision = (allow or (options[0] if options else {})).get("optionId")
        if decision:
            self._respond(rid, {"outcome": {"outcome": "selected", "optionId": decision}})
        else:
            self._respond(rid, {"outcome": {"outcome": "cancelled"}})
        if on_event:
            on_event({"type": "permission", "title": call.get("title") or "", "decision": decision})

    def _session_update(self, update: dict[str, Any], on_event):
        kind = update.get("sessionUpdate")
        if not on_event:
            return
        if kind in ("agent_message_chunk", "agent_thought_chunk"):
            content = update.get("content") or {}
            if content.get("type") == "text":
                on_event({"type": "text" if kind == "agent_message_chunk" else "thought", "text": content.get("text", "")})
        elif kind in ("tool_call", "tool_call_update"):
            on_event({"type": "tool", "title": update.get("title") or "", "status": update.get("status") or "",
                      "kind": update.get("kind") or "", "id": update.get("toolCallId") or ""})
        elif kind == "plan":
            on_event({"type": "plan", "entries": update.get("entries") or []})
        elif kind == "available_commands_update":
            self.commands = update.get("availableCommands") or []
        elif kind == "current_mode_update":
            self.current_mode = update.get("currentModeId") or self.current_mode

    # -- ciclo de vida
    def initialize(self, timeout: float = 60.0) -> dict[str, Any]:
        result = self._pump_until(self._request("initialize", {
            "protocolVersion": PROTOCOL_VERSION,
            "clientCapabilities": {"fs": {"readTextFile": True, "writeTextFile": True}, "terminal": False},
            "clientInfo": {"name": "comandos", "title": "ComandOS", "version": "1.0"},
        }), None, timeout)
        self.agent_capabilities = result.get("agentCapabilities") or {}
        self.auth_methods = result.get("authMethods") or []
        return result

    def _absorb_session(self, result: dict[str, Any]):
        models = result.get("models") or {}
        self.models = models.get("availableModels") or []
        self.current_model = models.get("currentModelId") or self.current_model
        modes = result.get("modes") or {}
        self.modes = modes.get("availableModes") or []
        self.current_mode = modes.get("currentModeId") or self.current_mode

    def new_session(self, timeout: float = 90.0) -> str:
        result = self._pump_until(self._request("session/new", {"cwd": self.cwd, "mcpServers": []}), None, timeout)
        self.session_id = result.get("sessionId") or ""
        if not self.session_id:
            raise AcpError("session/new sin sessionId")
        self._absorb_session(result)
        return self.session_id

    def supports_load(self) -> bool:
        return bool(self.agent_capabilities.get("loadSession"))

    def load_session(self, session_id: str, on_event=None, timeout: float = 120.0) -> str:
        result = self._pump_until(self._request("session/load", {"sessionId": session_id, "cwd": self.cwd,
                                                                 "mcpServers": []}), on_event, timeout)
        self.session_id = session_id
        self._absorb_session(result or {})
        return self.session_id

    def prompt(self, text: str, on_event=None, timeout: float = 3600.0) -> str:
        result = self._pump_until(self._request("session/prompt", {
            "sessionId": self.session_id, "prompt": [{"type": "text", "text": text}]}), on_event, timeout)
        stop = result.get("stopReason") or "end_turn"
        if on_event:
            on_event({"type": "end", "stopReason": stop})
        return stop

    def cancel(self):
        try:
            self._notify("session/cancel", {"sessionId": self.session_id})
        except AcpError:
            pass

    def set_model(self, model_id: str, timeout: float = 30.0):
        self._pump_until(self._request("session/set_model", {"sessionId": self.session_id, "modelId": model_id}), None, timeout)
        self.current_model = model_id

    def set_mode(self, mode_id: str, timeout: float = 30.0):
        self._pump_until(self._request("session/set_mode", {"sessionId": self.session_id, "modeId": mode_id}), None, timeout)
        self.current_mode = mode_id

    def close(self):
        self.proc.close()


class AgyStreamSession:
    """Antigravity (agy) por stream-json: misma interfaz que AcpSession.
    Un proceso `agy -p '' --input-format stream-json` vive toda la sesión;
    cada prompt es una línea {"event":"user"} y la respuesta llega como
    step_update(text_delta) ... result."""

    def __init__(self, argv: list[str], env: dict[str, str], cwd: str, permission_handler=None):
        self.cwd, self.argv, self.env = cwd, argv, env
        self.proc = _Proc(argv, env, cwd)
        self.session_id = ""
        self.agent_capabilities = {"loadSession": False}
        self.auth_methods, self.models, self.modes, self.commands = [], [], [], []
        self.current_model, self.current_mode = "", ""

    def initialize(self, timeout: float = 60.0):
        return {"agentCapabilities": self.agent_capabilities}

    def new_session(self, timeout: float = 90.0) -> str:
        # agy emite init al primer turno; la sesión "existe" en cuanto arranca
        self.session_id = "agy-pending"
        return self.session_id

    def supports_load(self) -> bool:
        return False

    def load_session(self, session_id, on_event=None, timeout=120.0):
        raise AcpError("agy no soporta reanudar por ACP")

    def prompt(self, text: str, on_event=None, timeout: float = 3600.0) -> str:
        self.proc.write({"event": "user", "message": {"role": "user", "content": text}})
        deadline = time.time() + timeout
        while time.time() < deadline:
            try:
                msg = self.proc.lines.get(timeout=0.25)
            except queue.Empty:
                if not self.proc.alive():
                    raise AcpError("agy murió: " + " | ".join(self.proc.stderr[-3:]))
                continue
            if msg is None:
                raise AcpError("agy cerró stdout: " + " | ".join(self.proc.stderr[-3:]))
            event = msg.get("event")
            if event == "init":
                init = msg.get("init") or {}
                self.current_model = init.get("model") or self.current_model
                self.session_id = msg.get("conversation_id") or self.session_id
            elif event == "step_update":
                step = msg.get("step_update") or {}
                if step.get("text_delta") and on_event:
                    on_event({"type": "text", "text": step["text_delta"]})
                elif step.get("step_type") not in ("user_input", "agent_response") and on_event:
                    on_event({"type": "tool", "title": str(step.get("step_type") or ""), "status": str(step.get("state") or "").lower(), "kind": "", "id": str(step.get("step_index", ""))})
            elif event == "result":
                result = msg.get("result") or {}
                self.session_id = result.get("conversation_id") or self.session_id
                if result.get("status") != "SUCCESS":
                    raise AcpError(str(result.get("error") or result.get("status") or "agy error"))
                if on_event:
                    on_event({"type": "end", "stopReason": "end_turn"})
                return "end_turn"
        raise AcpError(f"timeout esperando a agy ({timeout:.0f}s)")

    def cancel(self):
        pass

    def set_model(self, model_id: str, timeout: float = 30.0):
        raise AcpError("agy cambia de modelo relanzando (usa /model y se reinicia el proceso)")

    def set_mode(self, mode_id: str, timeout: float = 30.0):
        raise AcpError("agy no expone modos por stream-json")

    def close(self):
        self.proc.close()


def open_session(spec: dict[str, Any], cwd: str, *, model: str = "", effort: str = "", danger: bool = False,
                 extra_env: dict[str, str] | None = None, permission_handler=None):
    argv, env = build_command(spec, model=model, effort=effort, danger=danger, extra_env=extra_env)
    if spec.get("transport") == "agy-stream":
        return AgyStreamSession(argv, env, cwd, permission_handler)
    return AcpSession(argv, env, cwd, permission_handler)
