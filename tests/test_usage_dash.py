#!/usr/bin/env python3
import importlib.machinery
import importlib.util
import json
import sys
from pathlib import Path
from types import SimpleNamespace


SRC = Path("bin/cc-dash").read_text()


def load_dash_module():
    bin_dir = str(Path("bin").resolve())
    if bin_dir not in sys.path:
        sys.path.insert(0, bin_dir)
    loader = importlib.machinery.SourceFileLoader("cc_dash_under_test", str(Path("bin/cc-dash").resolve()))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


def test_cc_dash_imports_usage_module():
    assert "import cc_usage" in SRC


def test_usage_state_endpoint_exists_and_is_authenticated():
    assert '"/usage/state"' in SRC
    api_get = SRC.split("API_GET = ", 1)[1].split("def do_GET", 1)[0]
    assert '"/usage/state"' in api_get


def test_usage_live_panes_records_pane_pwd_and_git_root():
    assert "def usage_live_panes" in SRC
    assert "cc_usage.normalize_pane_identity" in SRC
    assert "cc_usage.git_root_for_path" in SRC
    assert "cc_usage.record_pane" in SRC


def test_usage_capture_and_refresh_endpoints_exist():
    assert 'self.path == "/usage/capture"' in SRC
    assert 'self.path == "/usage/refresh"' in SRC
    assert 'self.path == "/usage/settings"' in SRC
    assert "write_usage_settings" in SRC
    assert "usage_runtime_env" in SRC
    assert "record_local_codex_threads" in SRC
    assert "record_local_claude_jsonl" in SRC
    assert "usage_credential_health" in SRC
    assert 'state["credential_health"] = usage_credential_health(env)' in SRC


def test_model_switch_endpoint_targets_requested_pane():
    assert 'self.path == "/model/switch"' in SRC
    assert "cc_usage.model_switch_text" in SRC
    assert 'tmux("send-keys", "-t", pane, "-l", "--", c)' in SRC  # cada comando (/model, /effort) va al pane pedido
    assert "claude_pane_busy(pane)" in SRC  # y nunca a mitad de turno


def test_model_switch_accepts_direct_model():
    assert 'data.get("model")' in SRC
    assert "cc_usage.model_switch_text(provider, preset, model)" in SRC


def test_usage_state_wires_exact_provider_limits():
    assert "def usage_provider_limits" in SRC
    assert "cc_usage.fetch_claude_oauth_limits" in SRC
    assert "cc_usage.read_codex_rate_limits" in SRC
    # El fetch OAuth corre en hilo aparte: /usage/state nunca se bloquea por red
    assert "threading.Thread" in SRC
    assert "limits=" in SRC


def test_codex_dropdown_drives_numbered_picker():
    # El picker /model de codex es de dos pasos numerados: modelo → reasoning
    assert "CODEX_MODEL_DIGITS" in SRC
    assert "CODEX_EFFORT_DIGITS" in SRC
    assert '"gpt-5.5": "1"' in SRC
    assert '"gpt-5.6-sol": "2"' in SRC   # picker 0.144.x reordenado
    assert '"xhigh": "4"' in SRC
    assert 'data.get("effort")' in SRC


def test_opencode_models_endpoint_and_picker_automation():
    assert '"/opencode/models"' in SRC
    assert "def opencode_models" in SRC
    assert "cc_usage.parse_opencode_models" in SRC
    assert 'provider == "opencode"' in SRC
    assert "cc_usage.opencode_picker_query" in SRC


def test_pane_models_file_for_tmux_borders():
    assert "def write_pane_models" in SRC
    assert "pane-models.txt" in SRC


def test_pane_border_uses_tmux_option_not_per_pane_subprocess():
    # Con 50 panes, un #(cc-pane-model) por pane cada status-interval causaba
    # lag al teclear. Ahora cc-dash setea la opcion @ccmodel (en un hilo) y el
    # borde la lee con #{E:@ccmodel} — cero spawns por pane.
    assert '"@ccmodel"' in SRC
    assert '"set-option", "-p"' in SRC
    assert "def _pane_model_values" in SRC
    assert "_pane_model_state" in SRC
    assert "threading.Thread" in SRC
    conf = Path("config/tmux.conf").read_text()
    assert "#{E:@ccmodel}" in conf
    assert "cc-pane-model" not in conf


def test_pane_model_values_normalize_labels_and_preserve_clears():
    dash = load_dash_module()

    values, plain = dash._pane_model_values([
        {"tmux_pane": "%1", "agent": "claude", "model": "claude-sonnet"},
        {"tmux_pane": "%2", "provider": "codex", "model": "openai/gpt-5.6"},
        {"tmux_pane": "%3", "agent": "", "model": ""},
        {"tmux_pane": "not-a-pane", "agent": "codex", "model": "ignored"},
    ])

    assert values == {
        "%1": "#[fg=colour141,bold]▸ claude#[default]#[fg=colour179,bold] · sonnet $$#[default]",
        "%2": "#[fg=colour43,bold]▸ codex#[default]#[fg=colour203,bold] · gpt-5.6 $$$#[default]",
        "%3": None,
    }
    assert plain == "%1 claude · sonnet $$\n%2 codex · gpt-5.6 $$$\n"


def test_alert_rules_endpoint_and_evaluation():
    assert '"/usage/alert-rule"' in SRC
    assert "cc_usage.set_alert_rule" in SRC
    assert "cc_usage.delete_alert_rule" in SRC
    assert "cc_usage.rule_current_values" in SRC
    assert "cc_usage.rule_alerts" in SRC
    assert 'state["alert_rules"]' in SRC


def test_limit_alerts_notify_by_desktop_and_telegram():
    assert "def usage_alert_send" in SRC
    # Popups PROPIOS de ComandOS (cc-notifyd), jamas notify-send
    assert "127.0.0.1:4778/notify" in SRC
    assert "notify-send" not in SRC
    assert "TELEGRAM_ENABLED" in SRC
    assert "cc_usage.limit_threshold_alerts" in SRC
    assert "cc_usage.record_alert_once" in SRC
    assert 'state["alerts"] = cc_usage.list_alerts(USAGE_DB)' in SRC


def test_new_sessions_load_provider_keys_env():
    # Las sesiones nuevas cargan ~/.claude/hooks/providers.env (keys de
    # groq/cerebras/sambanova/cloudflare para opencode y amigos)
    assert "providers.env" in SRC
    assert 'data.get("agent")' in SRC


def test_tab_close_endpoint_delegates_with_explicit_ephemeral_flag():
    assert '"/tab-close"' in SRC
    assert "app-tab-close.json" in SRC
    body = SRC.split('"/tab-close"', 1)[1].split('self.path ==', 1)[0]
    assert 'data.get("ephemeral") is True' in body
    assert "close_app_tab(sess, ephemeral=ephemeral)" in body


def test_ephemeral_tab_close_skips_history_and_rejects_non_e2e_names(tmp_path, monkeypatch):
    dash = load_dash_module()
    tabs_file = tmp_path / "app-tabs.json"
    history_file = tmp_path / "app-tabs-history.json"
    tabs_file.write_text(json.dumps({
        "comandos-e2e-4242": "e2e",
        "term-user": "user",
    }))
    history_file.write_text(json.dumps([{"session": "existing", "ts": 1}]))
    monkeypatch.setattr(dash, "HOOKS", str(tmp_path))
    monkeypatch.setattr(dash, "TABS_FILE", str(tabs_file))
    monkeypatch.setattr(dash, "TAB_HISTORY_FILE", str(history_file))
    monkeypatch.setattr(dash, "tmux", lambda *_args, **_kwargs: (_ for _ in ()).throw(
        AssertionError("ephemeral close must not inspect tmux")
    ))

    assert dash.close_app_tab("comandos-e2e-4242", ephemeral=True) is None
    assert json.loads(tabs_file.read_text()) == {"term-user": "user"}
    assert json.loads(history_file.read_text()) == [{"session": "existing", "ts": 1}]
    assert json.loads((tmp_path / "app-tab-close.json").read_text())["session"] == "comandos-e2e-4242"

    before_tabs = tabs_file.read_text()
    before_history = history_file.read_text()
    assert dash.close_app_tab("term-user", ephemeral=True) == "ephemeral requiere comandos-e2e-"
    assert tabs_file.read_text() == before_tabs
    assert history_file.read_text() == before_history


def test_normal_tab_close_still_records_recents(tmp_path, monkeypatch):
    dash = load_dash_module()
    tabs_file = tmp_path / "app-tabs.json"
    history_file = tmp_path / "app-tabs-history.json"
    tabs_file.write_text(json.dumps({"term-user": "User tab"}))
    history_file.write_text("[]")
    monkeypatch.setattr(dash, "HOOKS", str(tmp_path))
    monkeypatch.setattr(dash, "TABS_FILE", str(tabs_file))
    monkeypatch.setattr(dash, "TAB_HISTORY_FILE", str(history_file))
    monkeypatch.setattr(dash, "session_labels", lambda: {"term-user": "User tab"})
    monkeypatch.setattr(dash, "state_agent", lambda _session: "codex")
    monkeypatch.setattr(dash, "tmux", lambda *_args, **_kwargs: SimpleNamespace(
        returncode=0, stdout="/tmp/project\n", stderr=""
    ))

    assert dash.close_app_tab("term-user", ephemeral=False) is None
    history = json.loads(history_file.read_text())
    assert isinstance(history[0]["ts"], int)
    assert history[0] == {
        "session": "term-user",
        "label": "User tab",
        "cwd": "/tmp/project",
        "agent": "codex",
        "reason": "closed",
        "ts": history[0]["ts"],
    }
    assert json.loads(tabs_file.read_text()) == {}


def test_usage_state_is_cached_and_refresh_is_backgrounded():
    assert "def cached_usage_state" in SRC
    assert "def read_states_cached" in SRC
    assert "_usage_state_cache" in SRC
    assert "threading.Thread(target=_bg" in SRC   # refresh local no bloquea
    assert "prune_old_turns" in SRC


def test_agent_pane_maps_keeps_one_agent_per_tmux_pane():
    dash = load_dash_module()

    class R:
        returncode = 0
        stdout = "term-1|%1|100\n"

    dash.tmux = lambda *args, **kwargs: R()
    parents = {300: 200, 200: 100, 400: 100}
    dash.parent_pid = lambda pid: parents.get(pid, 0)

    _by_session, by_cwd = dash.agent_pane_maps([
        (300, "/repo", "claude"),
        (400, "/repo", "codex"),
    ])

    assert len(by_cwd["/repo"]) == 1
    assert by_cwd["/repo"][0]["agent"] == "codex"
    assert by_cwd["/repo"][0]["pane"] == "%1"


if __name__ == "__main__":
    test_pane_border_uses_tmux_option_not_per_pane_subprocess()
    test_usage_state_is_cached_and_refresh_is_backgrounded()
    test_tab_close_endpoint_delegates_with_explicit_ephemeral_flag()
    test_alert_rules_endpoint_and_evaluation()
    test_codex_dropdown_drives_numbered_picker()
    test_limit_alerts_notify_by_desktop_and_telegram()
    test_cc_dash_imports_usage_module()
    test_usage_state_endpoint_exists_and_is_authenticated()
    test_usage_live_panes_records_pane_pwd_and_git_root()
    test_usage_capture_and_refresh_endpoints_exist()
    test_model_switch_endpoint_targets_requested_pane()
    test_model_switch_accepts_direct_model()
    test_usage_state_wires_exact_provider_limits()
    test_opencode_models_endpoint_and_picker_automation()
    test_pane_models_file_for_tmux_borders()
    test_new_sessions_load_provider_keys_env()
    test_agent_pane_maps_keeps_one_agent_per_tmux_pane()


def test_active_tab_endpoint_enriches_session_with_active_tmux_pane():
    assert 'if self.path.startswith("/active-tab")' in SRC
    assert '"#{pane_id}"' in SRC
    assert 'active["pane"] = pane_a' in SRC


def test_state_emits_one_control_card_per_live_claude_split():
    assert "def _claude_all_panes" in SRC
    assert '"pane": pane_l' in SRC
    assert '"split": True' in SRC
    assert "by_sess_count" in SRC


def test_providers_endpoint_is_security_gated_and_uses_sanitized_registry():
    assert '"/providers"' in SRC.split("API_GET =", 1)[1].split("def do_GET", 1)[0]
    assert 'provider_registry.public_state(load_provider_registry())' in SRC


def test_model_confirmation_handles_cross_provider_dialog_and_requires_new_marker(monkeypatch):
    dash = load_dash_module()
    captures = iter([
        "old\n❯ /model grok-4.6\nSwitch model?\n1. Yes, switch to grok-4.6\n2. No",
        "old\n❯ /model grok-4.6\nSet model to grok-4.6 and saved as your default",
    ])
    sent = []

    def fake_tmux(*args, **_kwargs):
        if args[0] == "capture-pane":
            return SimpleNamespace(returncode=0, stdout=next(captures), stderr="")
        sent.append(args)
        return SimpleNamespace(returncode=0, stdout="", stderr="")

    monkeypatch.setattr(dash, "tmux", fake_tmux)
    monkeypatch.setattr(dash.time, "sleep", lambda _n: None)
    ok, error = dash.claude_command_confirm("%1", "/model grok-4.6", "old", 2)
    assert (ok, error) == (True, "")
    assert ("send-keys", "-t", "%1", "1") in sent
    assert ("send-keys", "-t", "%1", "Enter") in sent


def test_model_confirmation_does_not_accept_stale_marker(monkeypatch):
    dash = load_dash_module()
    stale = "Set model to gpt-5.6-sol\n❯ /model grok-4.6\nSwitching..."
    monkeypatch.setattr(
        dash, "tmux",
        lambda *_args, **_kwargs: SimpleNamespace(returncode=0, stdout=stale, stderr=""),
    )
    monkeypatch.setattr(dash.time, "sleep", lambda _n: None)
    assert dash.claude_command_confirm("%1", "/model grok-4.6", stale, 1) == (False, "")


def test_native_codex_switch_reports_pane_keyed_confirmed_result():
    assert "def confirm_codex_native" in SRC
    assert "def codex_pane_model" in SRC
    assert 'operationKey": opkey' in SRC
    assert 'confirmando modelo y esfuerzo en Codex CLI' in SRC
