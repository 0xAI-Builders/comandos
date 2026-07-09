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
    test_cc_dash_imports_usage_module()
    test_usage_state_endpoint_exists_and_is_authenticated()
    test_usage_live_panes_records_pane_pwd_and_git_root()
    test_usage_capture_and_refresh_endpoints_exist()
    test_model_switch_endpoint_targets_requested_pane()
    test_agent_pane_maps_keeps_one_agent_per_tmux_pane()
