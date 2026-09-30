import json, subprocess
from pathlib import Path
import pytest

ROOT = Path(__file__).resolve().parents[1]
import sys; sys.path.insert(0, str(ROOT / "lib"))
import cli_catalog

DANGER = {"claude": "--dangerously-skip-permissions", "codex": "--dangerously-bypass-approvals-and-sandbox",
          "opencode": "", "grok": "--always-approve", "agy": "--dangerously-skip-permissions"}
VERSIONS = {"claude": "2.1.268", "codex": "0.154.0", "grok": "1.0.25", "opencode": "1.17.18", "agy": "1.1.25"}

def test_catalog_file_matches_repo_danger_flags_and_map_versions():
    cat = cli_catalog.load_catalog()
    roles = json.loads((ROOT / "config" / "agent-roles.json").read_text())["dangerFlags"]
    ids = [c["id"] for c in cat["clis"]]
    assert ids == ["claude", "codex", "grok", "opencode", "agy"]
    assert set(ids) <= set(roles)
    text = (ROOT / "docs" / "tui-command-map.md").read_text()
    for cli in cat["clis"]:
        assert cli["pinnedVersion"] in text, cli["id"]
        for grp in cli["groups"]:
            for cmd in grp["commands"]:
                assert cmd["text"].strip() in text, (cli["id"], cmd["text"])
                assert "\n" not in cmd["text"] and cmd["description"]

def test_yolo_launches_come_first_and_use_danger_flag():
    cat = cli_catalog.load_catalog()
    view = cli_catalog.catalog_view(cat, danger_flags=DANGER, versions=VERSIONS, accounts={})
    claude = next(c for c in view["clis"] if c["id"] == "claude")
    assert claude["launch"]["yolo"][:2] == [
        "claude --dangerously-skip-permissions",
        "claude --dangerously-skip-permissions --resume <id>"]
    assert claude["launch"]["normal"][:2] == ["claude", "claude --resume <id>"]
    codex = next(c for c in view["clis"] if c["id"] == "codex")
    assert codex["launch"]["yolo"][1] == "codex --dangerously-bypass-approvals-and-sandbox resume <id>"
    opencode = next(c for c in view["clis"] if c["id"] == "opencode")
    assert opencode["launch"]["yolo"] == [] and "sin flag" in opencode["launch"]["yoloNote"]

def test_account_launches_use_config_dir_env():
    cat = cli_catalog.load_catalog()
    accounts = {"claude": [{"alias": "relotto", "env": {"CLAUDE_CONFIG_DIR": "/home/u/.claude-accounts/relotto"}}]}
    view = cli_catalog.catalog_view(cat, danger_flags=DANGER, versions=VERSIONS, accounts=accounts)
    claude = next(c for c in view["clis"] if c["id"] == "claude")
    assert "CLAUDE_CONFIG_DIR=/home/u/.claude-accounts/relotto claude --dangerously-skip-permissions" in claude["launch"]["normal"]

@pytest.mark.parametrize("pinned,installed,verified,expected", [
    ("2.1.268", "2.1.268", True, "ok"), ("2.1.268", "2.1.270", True, "drift"),
    ("2.1.268", None, True, "missing"), ("1.1.25", "1.1.25", False, "unverified")])
def test_version_status(pinned, installed, verified, expected):
    assert cli_catalog.version_status(pinned, installed, verified) == expected

def test_installed_versions_runs_resolved_path_and_tolerates_failures():
    cat = cli_catalog.load_catalog()
    calls = []
    def which(b): return {"claude": "/opt/claude", "codex": "/opt/codex"}.get(b)
    def run(cmd, **kw):
        calls.append(cmd)
        if cmd[0] == "/opt/codex": raise subprocess.TimeoutExpired(cmd, 1)
        return subprocess.CompletedProcess(cmd, 0, stdout="2.1.268 (Claude Code)\n", stderr="")
    versions = cli_catalog.installed_versions(cat, which=which, run=run)
    assert versions == {"claude": "2.1.268", "codex": "?", "grok": None, "opencode": None, "agy": None}
    assert calls[0] == ["/opt/claude", "--version"]

def test_view_never_contains_enter_or_newlines():
    cat = cli_catalog.load_catalog()
    view = cli_catalog.catalog_view(cat, danger_flags=DANGER, versions=VERSIONS, accounts={})
    for cli in view["clis"]:
        for text in cli["launch"]["yolo"] + cli["launch"]["normal"]:
            assert "\n" not in text and "\r" not in text
