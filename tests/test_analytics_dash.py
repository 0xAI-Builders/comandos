"""Cableado de Analytics en cc-dash: fotos de cuota, sin avisos, sin Reparto con Aplicar."""
from pathlib import Path

from test_usage_dash import load_dash_module

SRC = Path("bin/cc-dash").read_text()
HTML = Path("dash/index.html").read_text()


def test_every_limits_read_saves_a_quota_snapshot():
    assert "cc_usage.record_quota_snapshots(USAGE_DB, claude_rows + codex_rows + grok_rows)" in SRC
    assert "threading.Thread(target=_limits_snapshot_loop, daemon=True).start()" in SRC


def test_quota_alerts_are_gone():
    for gone in ("_check_limit_alerts", "rule_alerts(", '"/usage/alert-rule"', "COMANDOS_ALERT_THRESHOLDS"):
        assert gone not in SRC, gone


def _limit(limit_id, provider, window, percent, resets_at):
    return {"id": limit_id, "provider": provider, "account": "main", "kind": "x", "label": "x", "scope": "",
            "percent": percent, "resets_at": resets_at, "window": window, "captured_at": 500}


def _limits_read_without_network(dash, monkeypatch, tmp_path, claude=(), codex=(), grok=None, groq=(), accounts=("main",)):
    """Una lectura de límites sin red ni archivos del usuario: cada fuente devuelve lo que se le da."""
    monkeypatch.setattr(dash, "USAGE_DB", str(tmp_path / "u.sqlite"))
    monkeypatch.setattr(dash, "_claude_account_creds", lambda: [(a, f"/x/{a}/.credentials.json") for a in accounts])
    monkeypatch.setattr(dash, "account_email_for_dir", lambda *a, **k: "")
    monkeypatch.setattr(dash, "user_quotas", lambda: {})
    monkeypatch.setattr(dash.cc_usage, "fetch_claude_oauth_limits", lambda **k: ([dict(r) for r in claude], {"status": "ok"}))
    monkeypatch.setattr(dash.cc_usage, "read_codex_rate_limits", lambda *a, **k: [dict(r) for r in codex])
    monkeypatch.setattr(dash.cc_usage, "grok_measured_usage", lambda *a, **k: None)
    monkeypatch.setattr(dash.cc_usage, "read_grok_credit_limits", lambda *a, **k: grok)
    monkeypatch.setattr(dash.grok_state, "account_homes", lambda: [])
    monkeypatch.setattr(dash, "_groq_limit_rows", lambda: [dict(r) for r in groq])
    dash._limits_refreshing = True
    return dash.USAGE_DB


def test_a_limits_read_saves_one_snapshot_per_account_and_limit_but_not_groq(tmp_path, monkeypatch):
    dash = load_dash_module()
    db = _limits_read_without_network(
        dash, monkeypatch, tmp_path, accounts=("main", "relotto"),
        claude=[_limit("claude_weekly", "claude", "7d", 33.0, 1791014400), _limit("claude_session", "claude", "5h", 10.0, 1790899200)],
        codex=[_limit("codex_weekly", "codex", "7d", 61.5, 1791100800)],
        grok={"percent": 12.0, "resets_at": 1791201600, "captured_at": 700, "tier": "", "stale_period": False},
        groq=[_limit("groq_measured", "groq", "7d", 99.0, 1791300000)])
    dash._refresh_provider_limits()
    assert sorted((s["provider"], s["account"], s["window"], s["percent"]) for s in dash.cc_usage.quota_snapshots(db)) == [
        ("claude", "main", "5h", 10.0), ("claude", "main", "7d", 33.0),
        ("claude", "relotto", "5h", 10.0), ("claude", "relotto", "7d", 33.0),
        ("codex", "main", "7d", 61.5), ("grok", "main", "7d", 12.0)]


def test_a_failing_snapshot_write_does_not_stop_the_limits_read(tmp_path, monkeypatch):
    dash = load_dash_module()
    _limits_read_without_network(dash, monkeypatch, tmp_path, claude=[_limit("claude_weekly", "claude", "7d", 33.0, 1791014400)])

    def disk_full(*args, **kwargs):
        raise RuntimeError("disk full")

    monkeypatch.setattr(dash.cc_usage, "record_quota_snapshots", disk_full)
    dash._refresh_provider_limits()
    assert [r["id"] for r in dash._limits_cache["limits"]] == ["claude_weekly"]
    assert dash._limits_refreshing is False


def test_usage_modal_is_the_approved_analytics():
    start = HTML.index('<div id="usage"')
    block = HTML[start:start + 300]
    assert '<div class="an" id="an-root"></div>' in block
    assert 'href="analytics.css' in HTML
    assert HTML.index('src="analytics-render.js') < HTML.index('src="analytics.js')
    assert "window.openAnalyticsTab = tab => openAnalytics(tab);" in HTML
    assert "/analytics/week?offset=" in HTML


def test_old_analytics_is_gone():
    for gone in ("data-mpane=\"resumen\"", "data-mpane=\"guardia\"", "data-mpane=\"alertas\"", "data-mpane=\"reparto\"",
                 "renderQuotaHero", "renderUsageLimits", "limitCardKit", "wireQuotaEditors", "loadGuard", "guardPoll",
                 "renderLedger", "loadCompare", "renderCompare", "loadProvCompare", "renderProvCompare", "pcWire",
                 "renderAlertConfig", "openRuleMenu", "renderAlertRules", "renderDedication", "compareSetDays",
                 "setLimitStyle", "usage-refresh", "cc-guard-alert-key", "reparto.js", "reparto.css",
                 "/usage/guard", "/usage/provider-compare", "/usage/analytics", "/usage/alert-rule", "/allocation/"):
        assert gone not in HTML, gone
    pomo = Path("dash/pomodoro.js").read_text()
    assert "loadAnalytics" not in pomo and "/pomodoro/report" not in pomo
    assert not Path("dash/reparto.js").exists() and not Path("dash/reparto.css").exists()


def test_install_links_the_new_files():
    install = Path("install.sh").read_text()
    for name in ("analytics.js", "analytics-render.js", "analytics.css"):
        assert f" {name}" in install
    assert "reparto.js" not in install and "reparto.css" not in install


def test_nothing_proposes_or_applies_account_changes():
    for gone in ('"/allocation/propose"', '"/allocation/preview"', '"/allocation/apply"', '"/allocation/status"',
                 '"/allocation/retry"', '"/allocation/revert"', "import allocation_batch", "ALLOCATION_PLANS"):
        assert gone not in SRC, gone
    assert not Path("lib/allocation_batch.py").exists()
    catalog = Path("lib/operator_catalog.py").read_text()
    assert '("cuentas", "comparar", "pomodoro")' in catalog
    assert "setLimitStyle" not in catalog and "compareSetDays" not in catalog

