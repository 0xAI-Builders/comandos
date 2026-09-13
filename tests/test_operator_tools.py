#!/usr/bin/env python3
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import operator_catalog as cat  # noqa: E402
import operator_tools as OT  # noqa: E402

TABS = [
    {"session": "term-1", "label": "Relotto"},
    {"session": "term-2", "label": "SAVA"},
]


def test_tools_come_from_catalog_with_json_schema():
    names = [t["name"] for t in OT.TOOLS]
    assert OT.TOOLS == OT.anthropic_tools()
    assert len(names) == len(set(names))
    assert names == [t.name for t in cat.CATALOG]
    for t in OT.TOOLS:
        assert t["name"]
        assert t["description"]
        schema = t["input_schema"]
        assert schema["type"] == "object"
        assert "properties" in schema
    for need in (
        "list_tabs", "focus_tab", "close_tab", "rename_tab", "send_tab_back",
        "next_tab", "prev_tab", "recover_tab", "toggle_shell",
        "split_pane", "close_split", "kill_session", "pause_agent",
        "resume_agent", "send_text", "send_key", "open_panel",
        "set_theme", "set_volume", "set_pref", "set_language",
        "set_notify_corner", "remember", "copy_last_reply",
        "copy_session_reply",
    ):
        assert need in names, need


def test_openai_tools_wrap_functions():
    wrapped = OT.openai_tools()
    assert all(tool["type"] == "function" for tool in wrapped)
    assert [tool["function"]["name"] for tool in wrapped] == [tool.name for tool in cat.CATALOG]
    assert "list_tabs" in {tool["function"]["name"] for tool in wrapped}
    assert wrapped == cat.openai_tools()


def test_local_tools_map_to_catalog_intents():
    assert cat.by_name("focus_tab").target == {"kind": "local", "intent": "focus"}
    assert cat.by_name("next_tab").target == {"kind": "local", "intent": "step_next"}
    assert cat.by_name("prev_tab").target == {"kind": "local", "intent": "step_prev"}
    assert cat.by_name("close_tab").target == {"kind": "local", "intent": "close"}
    assert cat.by_name("rename_tab").target == {"kind": "local", "intent": "rename"}
    assert cat.by_name("send_tab_back").target == {"kind": "local", "intent": "send_back"}
    assert cat.by_name("set_theme").target == {"kind": "local", "intent": "theme"}
    assert cat.by_name("split_pane").target == {"kind": "local", "intent": "split"}


def test_set_volume_points_to_conf_set():
    t = cat.by_name("set_volume")
    assert t.target["kind"] == "api"
    assert t.target["path"] == "/conf-set"
    assert t.target["body"] == {"key": "VOLUME", "value": "$percent"}


def test_dispatch_tool_requires_dispatcher():
    with pytest.raises(RuntimeError, match="dispatcher"):
        OT.dispatch_tool("focus_tab", {"tab": "Relotto"})


def test_dispatch_tool_delegates_to_dispatcher():
    seen = []

    class D:
        def run(self, name, args):
            seen.append((name, args))
            return {"ok": True, "reply": "Foco en Relotto", "actions": [], "data": None}

    out = OT.dispatch_tool("focus_tab", {"tab": "Relotto"}, dispatcher=D())
    assert seen == [("focus_tab", {"tab": "Relotto"})]
    assert out["reply"] == "Foco en Relotto"


def test_agent_prompt_mentions_open_tabs():
    prompt = OT.agent_system_prompt(TABS, "máximo 4", "term-1")
    assert "Relotto" in prompt
    assert "tool" in prompt.lower()
    assert "bruno" in prompt
    assert cat.groups_summary() in prompt
