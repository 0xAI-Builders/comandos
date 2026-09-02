#!/usr/bin/env python3
import base64
import importlib.util
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT / "lib") not in sys.path:
    sys.path.insert(0, str(ROOT / "lib"))
import accounts
import capabilities


def registry(tmp_path):
    return {"harnesses": {
        provider: {
            "defaultHome": str(tmp_path / provider),
            "accountsRoot": str(tmp_path / f"{provider}-accounts"),
            "authFile": ".credentials.json" if provider == "claude" else "auth.json",
            "accountEnv": {"claude": "CLAUDE_CONFIG_DIR", "codex": "CODEX_HOME", "grok": "GROK_HOME"}[provider],
            "capabilities": {"accounts": True},
        } for provider in ("claude", "codex", "grok")
    }}


def _jwt(payload):
    body = base64.urlsafe_b64encode(json.dumps(payload).encode()).decode().rstrip("=")
    return f"x.{body}.y"


def test_discovers_subscription_accounts_for_all_harnesses(tmp_path):
    reg = registry(tmp_path)
    for provider in ("claude", "codex", "grok"):
        (tmp_path / provider).mkdir()
        (tmp_path / f"{provider}-accounts" / "work").mkdir(parents=True)
    (tmp_path / "claude" / ".credentials.json").write_text(json.dumps({"claudeAiOauth": {"accessToken": "secret"}}))
    (tmp_path / "claude-accounts" / "work" / ".credentials.json").write_text(json.dumps({"claudeAiOauth": {"refreshToken": "secret"}}))
    (tmp_path / "codex" / "auth.json").write_text(json.dumps({"tokens": {"access_token": "secret", "id_token": _jwt({"email": "codex@example.test"})}}))
    (tmp_path / "grok" / "auth.json").write_text(json.dumps({"issuer": {"refresh_token": "secret", "email": "grok@example.test"}}))

    found = accounts.public_accounts(reg)
    assert [a["alias"] for a in found["claude"]] == ["main", "work"]
    assert found["codex"][0]["identity"] == "codex@example.test"
    assert found["grok"][0]["identity"] == "grok@example.test"
    assert found["claude"][1]["selectable"] is True
    assert found["codex"][0]["selectable"] is True
    encoded = json.dumps(found)
    assert "secret" not in encoded
    assert str(tmp_path) not in encoded
    assert "access_token" not in encoded


def test_api_key_only_and_pending_accounts_are_not_selectable(tmp_path):
    reg = registry(tmp_path)
    (tmp_path / "codex").mkdir()
    (tmp_path / "codex" / "auth.json").write_text(json.dumps({"OPENAI_API_KEY": "secret"}))
    (tmp_path / "codex-accounts" / "pending").mkdir(parents=True)

    found = accounts.list_accounts(reg, "codex")
    assert [(a["alias"], a["state"], a["selectable"]) for a in found] == [
        ("main", "unsupported_auth", False),
        ("pending", "login_required", False),
    ]


def test_alias_path_cannot_escape_or_follow_symlink(tmp_path):
    reg = registry(tmp_path)
    root = tmp_path / "grok-accounts"
    root.mkdir()
    (root / "escape").symlink_to(tmp_path)
    for alias in ("../escape", ".hidden", "-flag"):
        try:
            accounts.account_home(reg, "grok", alias)
        except accounts.AccountError:
            pass
        else:
            raise AssertionError(alias)
    try:
        accounts.account_home(reg, "grok", "escape")
    except accounts.AccountError:
        pass
    else:
        raise AssertionError("symlink alias accepted")


def test_codex_mcp_inventory_uses_account_and_project_toml_without_values(tmp_path):
    reg = registry(tmp_path)
    home = tmp_path / "codex"
    home.mkdir()
    (home / "config.toml").write_text('''
[mcp_servers.github]
command = "secret-command"
[mcp_servers.disabled]
enabled = false
env = { TOKEN = "secret-token" }
''')
    project = tmp_path / "repo"
    (project / ".git").mkdir(parents=True)
    (project / ".codex").mkdir()
    (project / ".codex" / "config.toml").write_text('[mcp_servers.linear]\nurl = "https://private.test"\n')

    result = capabilities.session_capabilities(reg, "codex", "main", str(project))
    assert [(m["name"], m["enabled"], m["source"]) for m in result["mcps"]] == [
        ("disabled", False, "codex-user"),
        ("github", True, "codex-user"),
        ("linear", True, "codex-project"),
    ]
    encoded = json.dumps(result)
    assert "secret-command" not in encoded
    assert "secret-token" not in encoded
    assert "private.test" not in encoded


def test_grok_inventory_merges_native_and_compatible_mcp_sources(tmp_path):
    reg = registry(tmp_path)
    home = tmp_path / "grok"
    home.mkdir()
    (home / "config.toml").write_text('[mcp_servers.context7]\ncommand="native"\n')
    project = tmp_path / "repo"
    (project / ".git").mkdir(parents=True)
    (project / ".mcp.json").write_text(json.dumps({"mcpServers": {"context7": {"command": "secret"}, "jira": {}}}))

    result = capabilities.session_capabilities(reg, "grok", "main", str(project))
    by_name = {m["name"]: m for m in result["mcps"]}
    assert set(by_name) == {"context7", "jira"}
    assert by_name["context7"]["sources"] == ["grok-user", "mcp-json"]
    assert "secret" not in json.dumps(result)


def test_effort_switch_accepts_cli_reported_model_variants():
    # Bug real: "cambiar esfuerzo" moria en model_unavailable porque el CLI
    # reporta variantes del id (claude-fable-5, fable-5[1m], fable) distintas
    # al id canonico del registry (claude-fable-5[1m]).
    import json
    import providers
    registry = json.load(open("config/providers.json"))
    for variant in ("claude-fable-5[1m]", "claude-fable-5", "fable-5[1m]",
                    "claude-fable-5-20260901"):
        spec = providers.model_spec(registry, "claude", variant, section="motors")
        assert spec and spec["id"] == "claude-fable-5[1m]", variant
        assert "high" in spec["efforts"]
    # alias de familia resuelve al MAS NUEVO (como el CLI): fable -> 5.1
    spec = providers.model_spec(registry, "claude", "fable", section="motors")
    assert spec and spec["id"] == "claude-fable-5-1"
    # familia ambigua o inexistente: jamas adivinar
    assert providers.model_spec(registry, "claude", "gpt", section="motors") is None
    assert providers.model_spec(registry, "claude", "nope-9", section="motors") is None


def test_model_watch_reports_only_true_news_and_never_edits_registry(tmp_path, monkeypatch):
    import model_watch as MW
    reg = tmp_path / "providers.json"
    reg.write_text(json.dumps({"motors": {"codex": {"models": [
        {"id": "gpt-5.6-sol[1m]"}, {"id": "gpt-5.6-luna"}]}}}))
    monkeypatch.setattr(MW, "installed_versions", lambda: {"codex": "0.150.0"})
    monkeypatch.setattr(MW, "discover_models", lambda gh=None: {
        "codex": ["gpt-5.6-sol", "gpt-5.6-luna", "gpt-5.7-nova"]})
    r1 = MW.watch_models(tmp_path, str(reg), now=1000)
    # sol/luna ya estan (match normalizado con [1m]); solo la nova es noticia
    assert r1["news"] == {"codex": ["gpt-5.7-nova"]}
    assert r1["snapshot"]["newSince"]["codex"]["models"] == ["gpt-5.7-nova"]
    # segunda vuelta sin cambios: cero re-notificacion, novedad persiste
    r2 = MW.watch_models(tmp_path, str(reg), now=2000)
    assert r2["news"] == {}
    assert "codex" in r2["snapshot"]["newSince"]
    # al AGREGARLA al registry (decision humana), deja de ser novedad
    reg.write_text(json.dumps({"motors": {"codex": {"models": [
        {"id": "gpt-5.6-sol[1m]"}, {"id": "gpt-5.6-luna"}, {"id": "gpt-5.7-nova"}]}}}))
    r3 = MW.watch_models(tmp_path, str(reg), now=3000)
    assert r3["snapshot"]["newSince"] == {}
    # el registry JAMAS se toca solo
    assert "gpt-5.7-nova" in reg.read_text()
