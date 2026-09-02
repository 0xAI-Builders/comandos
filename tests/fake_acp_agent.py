#!/usr/bin/env python3
"""Agente ACP falso para tests: JSON-RPC por stdio, sin red.

Responde initialize / session/new / session/load / session/prompt /
session/set_model / session/cancel. En el prompt: pide permiso (para probar
el flujo request_permission), emite un tool_call y devuelve el texto de vuelta
en agent_message_chunk. Registra lo recibido en FAKE_ACP_LOG si esta seteado.
"""
import json
import os
import sys


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def log(entry):
    path = os.environ.get("FAKE_ACP_LOG")
    if path:
        with open(path, "a") as handle:
            handle.write(json.dumps(entry) + "\n")


pending_permission = None
model = os.environ.get("FAKE_ACP_MODEL", "fake-1")
for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    msg = json.loads(raw)
    log({"in": msg, "env": {k: v for k, v in os.environ.items() if k.startswith(("FAKE_", "MAX_", "CLAUDE_", "CODEX_", "GROK_", "OPENCODE_"))},
         "argv": sys.argv[1:]})
    method, rid, params = msg.get("method"), msg.get("id"), msg.get("params") or {}
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": rid, "result": {
            "protocolVersion": 1, "agentCapabilities": {"loadSession": True}, "authMethods": []}})
    elif method in ("session/new", "session/load"):
        sid = params.get("sessionId") or "sess-fake-1"
        send({"jsonrpc": "2.0", "id": rid, "result": {
            "sessionId": sid,
            "models": {"currentModelId": model, "availableModels": [{"modelId": model}, {"modelId": "fake-2"}]},
            "modes": {"currentModeId": "default", "availableModes": [{"id": "default"}, {"id": "bypassPermissions"}]}}})
    elif method == "session/set_model":
        model = params.get("modelId")
        send({"jsonrpc": "2.0", "id": rid, "result": {}})
    elif method == "session/set_mode":
        send({"jsonrpc": "2.0", "id": rid, "result": {}})
    elif method == "session/prompt":
        sid = params.get("sessionId")
        text = "".join(p.get("text", "") for p in params.get("prompt") or [])
        send({"jsonrpc": "2.0", "id": 900, "method": "session/request_permission", "params": {
            "sessionId": sid, "toolCall": {"toolCallId": "t1", "title": "escribir archivo", "kind": "edit"},
            "options": [{"optionId": "allow-once", "name": "Allow", "kind": "allow_once"},
                        {"optionId": "reject", "name": "Reject", "kind": "reject_once"}]}})
        pending_permission = (rid, sid, text)
    elif rid == 900 and pending_permission:
        prid, sid, text = pending_permission
        pending_permission = None
        outcome = ((msg.get("result") or {}).get("outcome") or {}).get("optionId", "")
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
            "sessionUpdate": "tool_call", "toolCallId": "t1", "title": "escribir archivo", "status": "completed", "kind": "edit"}}})
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
            "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": f"eco[{model}|{outcome}]: {text}"}}}})
        send({"jsonrpc": "2.0", "id": prid, "result": {"stopReason": "end_turn"}})
    elif method == "session/cancel":
        pass
    elif rid is not None:
        send({"jsonrpc": "2.0", "id": rid, "error": {"code": -32601, "message": f"{method} no soportado"}})
