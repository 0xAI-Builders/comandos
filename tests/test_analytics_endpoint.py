"""GET /analytics/week: el modelo de Analytics desde la base real, y los datos de ejemplo."""
import json
import time
from pathlib import Path

from test_usage_dash import load_dash_module

ROOT = Path(__file__).resolve().parents[1]


def test_week_reads_turns_spans_and_limits_per_account(tmp_path, monkeypatch):
    dash = load_dash_module()
    db = str(tmp_path / "usage.sqlite")
    monkeypatch.setattr(dash, "USAGE_DB", db)
    now = time.time()
    dash.cc_usage.record_turns(db, [{
        "id": "claude-jsonl-a", "provider": "claude", "agent": "claude", "tmux_session": "s", "tmux_pane": "",
        "pane_pwd": "/x/Relotto", "git_root": "/x/Relotto", "turn_started_at": int(now - 600), "turn_finished_at": int(now - 60),
        "total_tokens": 2_000_000, "harness_account": "relotto", "motor_account": "relotto", "source": "claude_jsonl", "confidence": "local"}])
    limits = [{"provider": "claude", "account": "relotto", "window": "7d", "scope": "", "percent": 59.0, "resets_at": int(now + 86400)}]
    monkeypatch.setattr(dash, "usage_provider_limits", lambda force=False: {"limits": limits, "health": {}})
    code, week = dash.analytics_week_query("/analytics/week?offset=0")
    assert code == 200
    assert [a["id"] for a in week["accounts"]] == ["claude:relotto"]
    assert week["accounts"][0]["week"] == 59
    # Un set: si la prueba corre justo después de medianoche, la sesión se parte en dos días.
    assert {(s["acc"], s["proj"]) for s in week["sessions"]} == {("claude:relotto", "Relotto")}
    assert dash.analytics_week_query("/analytics/week?offset=-1")[0] == 200


def test_week_rejects_other_offsets():
    dash = load_dash_module()
    assert dash.analytics_week_query("/analytics/week?offset=-2")[0] == 400
    assert dash.analytics_week_query("/analytics/week?offset=x")[0] == 400


def test_sidebar_reads_measured_opencode_and_preserves_regular_analytics(tmp_path, monkeypatch):
    dash = load_dash_module()
    db = str(tmp_path / "usage.sqlite")
    monkeypatch.setattr(dash, "USAGE_DB", db)
    now = time.time()
    dash.cc_usage.record_turns(db, [{
        "id": "opencode-db-test", "provider": "opencode", "agent": "opencode",
        "tmux_session": "source-session", "turn_started_at": int(now - 30),
        "turn_finished_at": int(now - 10), "total_tokens": 123456,
        "cost_usd": .75, "model": "opencode/model-free", "source": "opencode_db"}])
    monkeypatch.setattr(dash, "usage_provider_limits", lambda force=False: {"limits": [], "health": {}})
    assert dash.analytics_week_payload(0, now=now)["accounts"] == []
    sidebar = dash.analytics_week_payload(0, now=now, sidebar=True)
    assert sidebar["accounts"][0]["id"] == "opencode:main"
    assert sidebar["accounts"][0]["measured"]["tokens"] == 123456
    assert sidebar["accounts"][0]["measured"]["costUsd"] == .75
    assert sidebar["accounts"][0]["measured"]["sessions"] == 1
    code, result = dash.analytics_week_query("/analytics/week?offset=0&sidebar=1")
    assert code == 200 and result["accounts"][0]["id"] == "opencode:main"


def test_demo_serves_the_mockup_data_only_by_plain_name():
    dash = load_dash_module()
    code, week = dash.analytics_week_query("/analytics/week?demo=normal")
    assert code == 200
    assert week == json.loads((ROOT / "tests/fixtures/analytics/week-normal.json").read_text())
    assert dash.analytics_week_query("/analytics/week?demo=normal&offset=-1")[1]["week"]["offset"] == -1
    assert dash.analytics_week_query("/analytics/week?demo=../../etc")[0] == 404


def test_week_route_needs_the_token():
    src = Path("bin/cc-dash").read_text()
    assert '"/analytics/week"' in src[src.index("API_GET = ("):src.index("API_GET = (") + 4000]
