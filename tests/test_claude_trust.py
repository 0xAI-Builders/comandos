#!/usr/bin/env python3
"""Claude Code 2.1+ workspace-trust dialog: ComandOS must pre-accept it.

WHY: every new session / motor switch in a folder that is its own git root
lands on "Accessing workspace: … Yes, I trust this folder". Claude 2.1.263
reads ~/.claude.json (HOME), NOT {CLAUDE_CONFIG_DIR}/.claude.json, and only
walks ancestors until the git toplevel — so a trusted parent like ~/codebase
does not cover ~/codebase/0xJesus/ServerMacMini.
"""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import claude_trust  # noqa: E402


def _read(path):
    return json.loads(Path(path).read_text())


def test_ensure_cwd_trusted_writes_home_claude_json_not_only_config_dir(tmp_path):
    """Claude 2.1.263 loads ~/.claude.json. Marking only CLAUDE_CONFIG_DIR is a no-op."""
    home = tmp_path / "home"
    cfg = tmp_path / "cfg"
    cwd = tmp_path / "proj" / "ServerMacMini"
    home.mkdir()
    cfg.mkdir()
    cwd.mkdir(parents=True)
    (home / ".claude.json").write_text(json.dumps({
        "projects": {
            str(tmp_path / "proj"): {"hasTrustDialogAccepted": True, "keep": 1},
        }
    }))
    (cfg / ".claude.json").write_text(json.dumps({"projects": {}}))

    assert claude_trust.ensure_cwd_trusted(str(cwd), config_dir=str(cfg), home=str(home)) is True

    home_doc = _read(home / ".claude.json")
    assert home_doc["projects"][str(cwd.resolve())]["hasTrustDialogAccepted"] is True
    # parent entry stays intact
    assert home_doc["projects"][str(tmp_path / "proj")]["keep"] == 1
    cfg_doc = _read(cfg / ".claude.json")
    assert cfg_doc["projects"][str(cwd.resolve())]["hasTrustDialogAccepted"] is True


def test_ensure_cwd_trusted_is_idempotent_and_preserves_other_project_keys(tmp_path):
    home = tmp_path / "home"
    home.mkdir()
    cwd = tmp_path / "repo"
    cwd.mkdir()
    (home / ".claude.json").write_text(json.dumps({
        "numStartups": 9,
        "projects": {
            str(cwd.resolve()): {
                "allowedTools": ["Bash"],
                "hasTrustDialogAccepted": False,
            }
        },
    }))
    claude_trust.ensure_cwd_trusted(str(cwd), home=str(home))
    claude_trust.ensure_cwd_trusted(str(cwd), home=str(home))
    doc = _read(home / ".claude.json")
    assert doc["numStartups"] == 9
    entry = doc["projects"][str(cwd.resolve())]
    assert entry["hasTrustDialogAccepted"] is True
    assert entry["allowedTools"] == ["Bash"]


def test_ensure_cwd_trusted_skips_home_directory_itself(tmp_path):
    """Claude treats home trust as session-only; persisting it is rejected."""
    home = tmp_path / "home"
    home.mkdir()
    (home / ".claude.json").write_text(json.dumps({"projects": {}}))
    assert claude_trust.ensure_cwd_trusted(str(home), home=str(home)) is False
    assert _read(home / ".claude.json")["projects"] == {}


def test_ensure_cwd_trusted_creates_missing_json(tmp_path):
    home = tmp_path / "home"
    home.mkdir()
    cwd = tmp_path / "fresh"
    cwd.mkdir()
    assert claude_trust.ensure_cwd_trusted(str(cwd), home=str(home)) is True
    assert _read(home / ".claude.json")["projects"][str(cwd.resolve())]["hasTrustDialogAccepted"] is True


def test_ensure_cwd_trusted_rejects_relative_or_missing_cwd(tmp_path):
    home = tmp_path / "home"
    home.mkdir()
    assert claude_trust.ensure_cwd_trusted("relative/path", home=str(home)) is False
    assert claude_trust.ensure_cwd_trusted(str(tmp_path / "nope"), home=str(home)) is False
    assert not (home / ".claude.json").exists()


def test_ensure_cwd_trusted_does_not_wipe_corrupt_claude_json(tmp_path):
    home = tmp_path / "home"
    home.mkdir()
    cwd = tmp_path / "repo"
    cwd.mkdir()
    raw = "{not-json"
    (home / ".claude.json").write_text(raw)
    assert claude_trust.ensure_cwd_trusted(str(cwd), home=str(home)) is False
    assert (home / ".claude.json").read_text() == raw


def test_session_new_and_open_with_account_pretrust_before_launching_claude():
    src = (ROOT / "bin" / "cc-dash").read_text()
    new = src.split('if self.path == "/session-new":', 1)[1].split('if self.path == "/open-with-account":', 1)[0]
    open_acc = src.split('if self.path == "/open-with-account":', 1)[1].split('if self.path == "/tab-new":', 1)[0]
    tmux_new = src.split("def tmux_new_session", 1)[1].split("\ndef ", 1)[0]
    assert "ensure_cwd_trusted" in new
    assert "ensure_cwd_trusted" in open_acc
    assert "ensure_cwd_trusted" in tmux_new
    # must run BEFORE send-keys of the claude command
    assert new.index("ensure_cwd_trusted") < new.index("threading.Thread(target=_send")
    assert open_acc.index("ensure_cwd_trusted") < open_acc.index("threading.Thread(target=_send")


def test_harness_switch_pretrusts_and_does_not_arrow_down_on_claude_trust_dialog():
    """New dialog already highlights Yes; Down selects 'No, exit'."""
    src = (ROOT / "bin" / "cc-dash").read_text()
    apply = src.split("def harness_switch_apply", 1)[1].split("def proxy_set_enabled", 1)[0]
    assert "ensure_cwd_trusted" in apply
    # no Down for claude — Yes is the default in 2.1.263
    assert 'if to != "codex":' not in apply
    assert 'tmux("send-keys", "-t", pane, "Down")' not in apply
    # still match the new copy so a leftover dialog can be Enter'd
    assert "Accessing workspace" in apply or "I trust this folder" in apply


def test_account_switch_pretrusts_destination_even_when_source_never_accepted():
    src = (ROOT / "bin" / "cc-dash").read_text()
    blk = src.split('if self.path == "/account/switch":', 1)[1][:4500]
    assert "ensure_cwd_trusted" in blk
    # old code only copied the flag if the SOURCE already had it — new folders never did
    assert 'if sp.get("hasTrustDialogAccepted"):' not in blk


def test_cc_app_resume_and_cc_acp_connect_pretrust_cwd():
    app = (ROOT / "bin" / "cc-app").read_text()
    acp_bin = (ROOT / "bin" / "cc-acp").read_text()
    assert "ensure_cwd_trusted" in app
    assert "ensure_cwd_trusted" in acp_bin
    resume = app.split("def resume_command", 1)[1].split("\ndef ", 1)[0]
    send = app.split("def _send_resume", 1)[1].split("\ndef ", 1)[0]
    connect = acp_bin.split("def connect(self)", 1)[1].split("\n    def ", 1)[0]
    assert "ensure_cwd_trusted" in resume or "ensure_cwd_trusted" in send
    assert "ensure_cwd_trusted" in connect
    assert connect.index("ensure_cwd_trusted") < connect.index("open_session")
