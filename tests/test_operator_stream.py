#!/usr/bin/env python3
import json
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import operator_stream as osr  # noqa: E402


def _sse(*objs):
    return [f"data: {json.dumps(o)}\n".encode() for o in objs] + [b"data: [DONE]\n"]


def test_parse_sse_yields_json_and_skips_done_and_comments():
    lines = [b": ping\n", b"event: x\n"] + _sse({"a": 1}, {"b": 2})
    assert list(osr.parse_sse(lines)) == [{"a": 1}, {"b": 2}]


def test_anthropic_stream_text_and_tool_call():
    ev = [
        {"type": "message_start"},
        {"type": "content_block_start", "index": 0, "content_block": {"type": "text"}},
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Ho"}},
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "la"}},
        {"type": "content_block_stop", "index": 0},
        {"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": "t1", "name": "focus_tab"}},
        {"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{\"tab\": \"Sig"}},
        {"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "nara\"}"}},
        {"type": "content_block_stop", "index": 1},
        {"type": "message_delta", "delta": {"stop_reason": "tool_use"}},
        {"type": "message_stop"},
    ]
    out = list(osr.stream_anthropic(iter(ev)))
    assert out[:2] == [{"t": "delta", "text": "Ho"}, {"t": "delta", "text": "la"}]
    assert {"t": "tool_call", "id": "t1", "name": "focus_tab", "input": {"tab": "Signara"}} in out
    assert out[-1] == {"t": "done", "stop": "tool_use"}


def test_openai_stream_text_and_tool_call():
    ev = [
        {"choices": [{"delta": {"content": "Ho"}}]},
        {"choices": [{"delta": {"content": "la"}}]},
        {"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "c1", "function": {"name": "focus_tab", "arguments": "{\"tab\":"}}]}}]},
        {"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": " \"Signara\"}"}}]}}]},
        {"choices": [{"delta": {}, "finish_reason": "tool_calls"}]},
    ]
    out = list(osr.stream_openai(iter(ev)))
    assert out[:2] == [{"t": "delta", "text": "Ho"}, {"t": "delta", "text": "la"}]
    assert {"t": "tool_call", "id": "c1", "name": "focus_tab", "input": {"tab": "Signara"}} in out
    assert out[-1] == {"t": "done", "stop": "tool_calls"}


def test_credentials_cache_reads_disk_once_per_ttl():
    calls = []
    c = osr.Credentials(lambda: (calls.append("c") or "tokC"), lambda: (calls.append("g") or "keyG"), ttl=0.2)
    assert c.claude() == "tokC" and c.claude() == "tokC" and calls == ["c"]
    time.sleep(0.25)
    c.claude()
    assert calls == ["c", "c"]
    c.invalidate(); c.claude()
    assert calls == ["c", "c", "c"]
