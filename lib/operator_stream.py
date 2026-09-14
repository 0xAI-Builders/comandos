"""Streaming SSE hacia Anthropic (/v1/messages) y OpenAI-compatible (/v1/chat/completions)."""
from __future__ import annotations

import json
import time
import urllib.request
from typing import Callable, Iterable, Iterator

RETRYABLE = (401, 403, 429, 500, 502, 503, 504)


class Credentials:
    def __init__(self, claude_token: Callable[[], str], grok_key: Callable[[], str], ttl: float = 60.0):
        self._c, self._g, self._ttl = claude_token, grok_key, ttl
        self._cache: dict[str, tuple[float, str]] = {}

    def _get(self, key, fn):
        now = time.monotonic()
        hit = self._cache.get(key)
        if hit and now - hit[0] < self._ttl:
            return hit[1]
        val = str(fn() or "")
        self._cache[key] = (now, val)
        return val

    def claude(self):
        return self._get("claude", self._c)

    def grok(self):
        return self._get("grok", self._g)

    def invalidate(self):
        self._cache.clear()


def parse_sse(lines: Iterable[bytes]) -> Iterator[dict]:
    for raw in lines:
        line = raw.decode("utf-8", "replace").rstrip("\r\n")
        if not line.startswith("data:"):
            continue
        body = line[5:].strip()
        if not body or body == "[DONE]":
            continue
        try:
            yield json.loads(body)
        except json.JSONDecodeError:
            continue


def open_stream(url: str, payload: dict, headers: dict, timeout: float = 30.0) -> Iterator[bytes]:
    req = urllib.request.Request(url, data=json.dumps(payload).encode(), headers=headers, method="POST")
    resp = urllib.request.urlopen(req, timeout=timeout)   # HTTPError sube al llamador
    try:
        for line in resp:
            yield line
    finally:
        resp.close()


def stream_anthropic(events: Iterator[dict]) -> Iterator[dict]:
    blocks: dict[int, dict] = {}
    stop = None
    for ev in events:
        t = ev.get("type")
        if t == "content_block_start":
            blocks[ev["index"]] = {"block": dict(ev.get("content_block") or {}), "json": ""}
        elif t == "content_block_delta":
            d = ev.get("delta") or {}
            if d.get("type") == "text_delta" and d.get("text"):
                yield {"t": "delta", "text": d["text"]}
            elif d.get("type") == "input_json_delta":
                blocks.setdefault(ev["index"], {"block": {}, "json": ""})["json"] += d.get("partial_json") or ""
        elif t == "content_block_stop":
            b = blocks.pop(ev["index"], None)
            if b and b["block"].get("type") == "tool_use":
                try:
                    inp = json.loads(b['json']) if b['json'] else b['block'].get('input', {})
                    if not isinstance(inp, dict):
                        raise ValueError('tool input must be an object')
                except ValueError:
                    yield {'t': 'error', 'text': 'Argumentos de herramienta incompletos o inválidos.'}
                    continue
                yield {"t": "tool_call", "id": b["block"].get("id"), "name": b["block"].get("name"), "input": inp}
        elif t == "message_delta":
            stop = (ev.get("delta") or {}).get("stop_reason") or stop
        elif t == "error":
            yield {"t": "error", "text": str((ev.get("error") or {}).get("message") or "error")}
    if stop not in ('end_turn', 'tool_use', 'stop_sequence') or blocks:
        yield {'t': 'error', 'text': 'El proveedor terminó sin una respuesta completa.'}
    yield {"t": "done", "stop": stop}


def stream_openai(events: Iterator[dict]) -> Iterator[dict]:
    calls: dict[int, dict] = {}
    stop = None
    for ev in events:
        if ev.get('error'):
            yield {'t': 'error', 'text': 'El proveedor devolvió un error.'}
        for ch in ev.get("choices") or []:
            d = ch.get("delta") or {}
            if d.get("content"):
                yield {"t": "delta", "text": d["content"]}
            for tc in d.get("tool_calls") or []:
                slot = calls.setdefault(tc.get("index", 0), {"id": None, "name": None, "args": ""})
                slot["id"] = tc.get("id") or slot["id"]
                fn = tc.get("function") or {}
                slot["name"] = fn.get("name") or slot["name"]
                slot["args"] += fn.get("arguments") or ""
            if ch.get("finish_reason"):
                stop = ch["finish_reason"]
    if stop not in ('stop', 'tool_calls', 'function_call'):
        yield {'t': 'error', 'text': 'El proveedor terminó sin una respuesta completa.'}
        yield {'t': 'done', 'stop': stop}
        return
    for slot in calls.values():
        try:
            inp = json.loads(slot["args"] or "{}")
            if not isinstance(inp, dict):
                raise ValueError('tool input must be an object')
        except ValueError:
            yield {'t': 'error', 'text': 'Argumentos de herramienta incompletos o inválidos.'}
            continue
        yield {"t": "tool_call", "id": slot["id"], "name": slot["name"], "input": inp}
    yield {"t": "done", "stop": stop}
