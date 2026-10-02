"""Cuota de agy (Antigravity) y de Grok para las botellas de Analytics (2-oct, Jesús:
«no se ve uso de agy y grok y codex en cuentas»)."""
import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "bin"))
sys.path.insert(0, str(ROOT / "lib"))
import cc_usage  # noqa: E402
import analytics_week  # noqa: E402

BILLING = {"ts": "2026-10-02T07:49:43.764Z", "msg": "billing: fetched credits config",
           "ctx": {"config": {"creditUsagePercent": 9.0, "currentPeriod": {
               "type": "USAGE_PERIOD_TYPE_WEEKLY",
               "start": "2026-09-26T20:54:01+00:00", "end": "2026-10-03T20:54:01+00:00"}},
               "subscriptionTier": "SuperGrok"}}

# Lo que agy le pasa a su comando de barra de estado (capturado del agy 1.2.14 real).
AGY_PAYLOAD = {
    "cwd": "/home/x", "session_id": "", "email": "alguien@example.com",
    "transcript_path": "/home/x/.gemini/antigravity/brain/t.jsonl",
    "model": {"id": "Gemini 3.1 Pro (High)", "display_name": "Gemini 3.1 Pro (High)", "effort": "high"},
    "product": "antigravity", "plan_tier": "Google AI Pro",
    "quota": {
        "3p-5h": {"remaining_fraction": 1, "reset_time": "2026-10-02T21:35:27Z", "reset_in_seconds": 17999},
        "3p-weekly": {"remaining_fraction": 0.8, "reset_time": "2026-10-09T16:35:27Z", "reset_in_seconds": 604799},
        "gemini-5h": {"remaining_fraction": 0.9042365, "reset_time": "2026-10-02T18:48:28Z", "reset_in_seconds": 7980},
        "gemini-weekly": {"remaining_fraction": 0.7441311, "reset_time": "2026-10-07T17:21:23Z", "reset_in_seconds": 434755},
    },
}
NOW = 1790958900  # 2026-10-02 16:35 UTC: todas las ventanas de arriba siguen abiertas


def test_grok_limit_is_found_even_far_from_the_end_of_a_busy_log(tmp_path):
    # El log real de grok mide MB: la última línea de cobro quedó a 778 KB del final.
    home = tmp_path / "grok"
    (home / "logs").mkdir(parents=True)
    filler = json.dumps({"ts": "2026-10-02T09:00:00.000Z", "msg": "tool call", "ctx": {"x": "y" * 200}})
    with open(home / "logs" / "unified.jsonl", "w") as fh:
        fh.write(json.dumps(BILLING) + "\n")
        for _ in range(4000):
            fh.write(filler + "\n")
    assert os.path.getsize(home / "logs" / "unified.jsonl") > 800_000
    got = cc_usage.read_grok_credit_limits([home], now=NOW)
    assert got and got["percent"] == 9.0 and got["tier"] == "SuperGrok"


def test_agy_statusline_capture_keeps_only_the_quota(tmp_path):
    env = {**os.environ, "HOME": str(tmp_path)}
    run = subprocess.run([sys.executable, str(ROOT / "adapters" / "agy-statusline.py")],
                         input=json.dumps(AGY_PAYLOAD), text=True, capture_output=True, env=env, timeout=10)
    assert run.returncode == 0, run.stderr
    assert run.stdout == ""  # agy ya pinta su barra; la nuestra no repite nada
    saved = json.loads((tmp_path / ".claude" / "hooks" / "agy-quota.json").read_text())
    assert saved["plan_tier"] == "Google AI Pro"
    assert saved["quota"]["gemini-weekly"] == {"remaining_fraction": 0.7441311, "reset_time": "2026-10-07T17:21:23Z"}
    text = json.dumps(saved)
    for private in ("alguien@example.com", "/home/x", "transcript", "Gemini 3.1 Pro"):
        assert private not in text
    assert oct(os.stat(tmp_path / ".claude" / "hooks" / "agy-quota.json").st_mode & 0o777) == "0o600"


def test_agy_statusline_capture_never_fails_agy(tmp_path):
    env = {**os.environ, "HOME": str(tmp_path)}
    for junk in ("", "no es json", json.dumps({"quota": "x"}), json.dumps([1, 2])):
        run = subprocess.run([sys.executable, str(ROOT / "adapters" / "agy-statusline.py")],
                             input=junk, text=True, capture_output=True, env=env, timeout=10)
        assert run.returncode == 0 and run.stdout == ""
    assert not (tmp_path / ".claude" / "hooks" / "agy-quota.json").exists()


def _agy_file(tmp_path, payload=AGY_PAYLOAD, captured=NOW - 60):
    path = tmp_path / "agy-quota.json"
    path.write_text(json.dumps({"captured_at": captured, "plan_tier": payload["plan_tier"],
                                "quota": {k: {"remaining_fraction": v["remaining_fraction"], "reset_time": v["reset_time"]}
                                          for k, v in payload["quota"].items()}}))
    return path


def test_agy_quota_becomes_week_model_and_5h_rows(tmp_path):
    rows = cc_usage.read_agy_quota(_agy_file(tmp_path), now=NOW)
    by = {(r["window"], r["scope"]): r for r in rows}
    assert set(by) == {("7d", ""), ("5h", ""), ("7d", "Claude·GPT")}
    assert all(r["provider"] == "agy" and r["account"] == "main" for r in rows)
    assert by[("7d", "")]["percent"] == 25.6 and by[("7d", "")]["resets_at"] == 1791393683
    assert by[("5h", "")]["percent"] == 9.6
    assert by[("7d", "Claude·GPT")]["percent"] == 20.0
    assert by[("7d", "")]["plan_type"] == "Google AI Pro"


def test_agy_window_already_reset_counts_as_unused(tmp_path):
    later = NOW + 3 * 3600  # la ventana de 5 h de Gemini ya reinició y agy no ha vuelto a correr
    rows = cc_usage.read_agy_quota(_agy_file(tmp_path), now=later)
    h5 = next(r for r in rows if r["window"] == "5h")
    assert h5["percent"] == 0.0 and h5["resets_at"] == 0
    assert cc_usage.read_agy_quota(tmp_path / "no-existe.json", now=NOW) == []
    (tmp_path / "roto.json").write_text("{")
    assert cc_usage.read_agy_quota(tmp_path / "roto.json", now=NOW) == []


def test_agy_is_an_account_in_analytics_even_without_sessions(tmp_path):
    week = analytics_week.build_week(now=NOW, offset=0, limits=cc_usage.read_agy_quota(_agy_file(tmp_path), now=NOW),
                                     turns=[], spans=[], snapshots=[], records=[])
    agy = next(a for a in week["accounts"] if a["id"] == "agy:main")
    assert agy["cli"] == "Antigravity" and agy["color"]
    assert (agy["week"], agy["h5"], agy["model"]["n"], agy["model"]["v"]) == (26, 10, "Claude·GPT", 20)
    assert agy["left"] and agy["h5Left"]
    assert agy["hoy"] == {"h": 0, "tok": 0, "ses": 0}


def test_grok_week_that_already_ended_shows_the_new_week_unused(tmp_path, monkeypatch):
    # Si grok no ha corrido desde que venció su semana, el % viejo no aplica: la semana nueva va en 0 %
    # y su reset es el fin viejo + 7 días (los periodos de grok son semanas seguidas).
    from test_analytics_dash import _limits_read_without_network, load_dash_module
    dash = load_dash_module()
    end = 1791060841  # 2026-10-03T20:54:01Z
    _limits_read_without_network(dash, monkeypatch, tmp_path,
                                 grok={"percent": 40.0, "resets_at": end, "captured_at": end - 3600,
                                       "tier": "SuperGrok", "stale_period": True})
    monkeypatch.setattr(dash.time, "time", lambda: end + 86400)
    dash._refresh_provider_limits()
    grok = [r for r in dash._limits_cache["limits"] if r["provider"] == "grok"]
    assert [(r["percent"], r["resets_at"], r["window"]) for r in grok] == [(0.0, end + 7 * 86400, "7d")]


def test_a_limits_read_includes_agy(tmp_path, monkeypatch):
    from test_analytics_dash import _limits_read_without_network, load_dash_module
    dash = load_dash_module()
    _limits_read_without_network(dash, monkeypatch, tmp_path)
    monkeypatch.setattr(dash, "AGY_QUOTA_FILE", str(_agy_file(tmp_path)))
    monkeypatch.setattr(dash.time, "time", lambda: NOW)
    dash._refresh_provider_limits()
    agy = sorted((r["window"], r["scope"], r["percent"]) for r in dash._limits_cache["limits"] if r["provider"] == "agy")
    assert agy == [("5h", "", 9.6), ("7d", "", 25.6), ("7d", "Claude·GPT", 20.0)]
    snaps = {(s["provider"], s["window"], s["scope"]) for s in dash.cc_usage.quota_snapshots(dash.USAGE_DB)}
    assert ("agy", "7d", "") in snaps


def test_cc_agents_wires_the_agy_quota_capture_without_replacing_a_users_statusline():
    src = (ROOT / "bin" / "cc-agents").read_text()
    assert "agy-statusline.py" in src
    assert "antigravity-cli/settings.json" in src
    assert '.statusLine == null' in src or 'has("statusLine")' in src


def _setup_agy(home, tmp_path):
    bin_dir = tmp_path / "fakebin"
    bin_dir.mkdir(exist_ok=True)
    (bin_dir / "agy").write_text("#!/bin/sh\nexit 0\n")
    (bin_dir / "agy").chmod(0o755)
    env = {"HOME": str(home), "PATH": f"{bin_dir}:/usr/bin:/bin", "LANG": "C.UTF-8"}
    return subprocess.run(["bash", str(ROOT / "bin" / "cc-agents"), "setup"], env=env,
                          capture_output=True, text=True, timeout=30)


def test_cc_agents_setup_connects_the_agy_quota_once_and_respects_the_users_statusline(tmp_path):
    home = tmp_path / "home"
    st = home / ".gemini" / "antigravity-cli" / "settings.json"
    st.parent.mkdir(parents=True)
    st.write_text(json.dumps({"model": "Gemini 3.1 Pro (High)"}))
    st.chmod(0o600)
    assert _setup_agy(home, tmp_path).returncode == 0
    got = json.loads(st.read_text())
    assert got["model"] == "Gemini 3.1 Pro (High)"
    assert got["statusLine"] == {"type": "command", "command": str(ROOT / "adapters" / "agy-statusline.py"),
                                 "stack_with_default": True}
    assert oct(st.stat().st_mode & 0o777) == "0o600"
    again = _setup_agy(home, tmp_path)
    assert "ya conectada" in again.stdout and json.loads(st.read_text()) == got
    mine = {"statusLine": {"type": "command", "command": "/mio/barra.sh"}}
    st.write_text(json.dumps(mine))
    out = _setup_agy(home, tmp_path)
    assert json.loads(st.read_text()) == mine and "otra statusLine" in out.stdout
