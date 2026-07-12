#!/usr/bin/env python3
import importlib.machinery
import importlib.util
import sys
from pathlib import Path


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
    assert 'tmux("send-keys", "-t", pane, "-l", "--", switch_text)' in SRC


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


def test_tab_close_endpoint_syncs_mirror_and_history():
    assert '"/tab-close"' in SRC
    assert "app-tab-close.json" in SRC
    body = SRC.split('"/tab-close"', 1)[1].split('self.path ==', 1)[0]
    assert "remember_tab" in body and "TABS_FILE" in body


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
    test_usage_state_is_cached_and_refresh_is_backgrounded()
    test_tab_close_endpoint_syncs_mirror_and_history()
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
