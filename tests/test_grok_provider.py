#!/usr/bin/env python3
import json
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import grok_state

DASH = (ROOT / "bin/cc-dash").read_text()
APP = (ROOT / "bin/cc-app").read_text()
CCX = (ROOT / "bin/ccx").read_text()


def test_models_cache_exposes_per_model_efforts_without_auth_data(tmp_path):
    (tmp_path / "models_cache.json").write_text(json.dumps({"models": {
        "grok-4.6": {"info": {"id": "grok-4.6", "name": "Grok 4.6", "context_window": 500000,
                                    "reasoning_effort": "high", "reasoning_efforts": [
                                        {"id": "xhigh"}, {"id": "high"}, {"id": "low"}]}},
        "hidden": {"info": {"id": "hidden", "hidden": True}},
    }}))
    assert grok_state.models(tmp_path) == [{
        "id": "grok-4.6", "name": "Grok 4.6", "contextWindow": 500000,
        "efforts": ["xhigh", "high", "low"], "defaultEffort": "high", "hidden": False,
    }]


def test_auth_identity_returns_public_fields_only(tmp_path):
    (tmp_path / "auth.json").write_text(json.dumps({"issuer": {
        "refresh_token": "never-return-me", "first_name": "0xJesus", "coding_data_retention_opt_out": True,
    }}))
    public = grok_state.auth_identity(tmp_path)
    assert public == {"authenticated": True, "name": "0xJesus", "email": "", "retentionOptOut": True}
    assert "token" not in json.dumps(public).lower()


def test_active_pid_resolves_exact_summary(tmp_path):
    sid = "01a04934-f975-7141-b9ad-a85aea96b4e0"
    (tmp_path / "active_sessions.json").write_text(json.dumps([{"session_id": sid, "pid": os.getpid(), "cwd": "/work"}]))
    folder = tmp_path / "sessions" / "work" / sid
    folder.mkdir(parents=True)
    (folder / "summary.json").write_text(json.dumps({
        "info": {"id": sid, "cwd": "/work"}, "current_model_id": "grok-4.6",
        "reasoning_effort": "xhigh", "generated_title": "Exact pane",
    }))
    info = grok_state.session_for_pid(os.getpid(), tmp_path)
    assert info["sessionId"] == sid
    assert (info["model"], info["effort"], info["title"]) == ("grok-4.6", "xhigh", "Exact pane")


def test_backend_has_native_grok_launch_account_and_confirmed_switch_driver():
    assert '"grok":     "grok --continue 2>/dev/null || grok"' in DASH
    assert 'elif agent == "grok"' in DASH
    assert 'provider == "grok"' in DASH
    assert 'def grok_motor_apply' in DASH
    assert 'grok_state.models(home)' in DASH
    assert 'summary.json' in DASH
    assert 'operationKey' in DASH
    assert 'account_registry.account_environment(load_provider_registry(), agent, alias)' in DASH
    assert '"GROK_HOME"' in (ROOT / "config/providers.json").read_text()


def test_desktop_and_ccx_resume_grok_exactly():
    assert "grok --continue 2>/dev/null || grok" in CCX
    assert 'ent["agent"] == "grok"' in APP
    assert 'grok --resume ' in APP
    assert 'grok --continue' in APP
