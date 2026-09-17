#!/usr/bin/env python3
"""Explicit legacy trust helper, trust inheritance, and safe automated launch boundaries.

Configuration operations must never invent provider trust acceptance for a cwd.
Trust may be INHERITED — copied via inherit_cwd_trust() from an account whose
~/.claude.json or CLAUDE_CONFIG_DIR/.claude.json already has
hasTrustDialogAccepted for that cwd, to a destination account's config — but
never granted where no account had already accepted it. The helper tests
cover explicit calls only; launch tests never invoke it.
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


def _write_trust(path, cwd):
    path.parent.mkdir(parents=True, exist_ok=True)
    data = json.loads(path.read_text()) if path.exists() else {}
    data.setdefault("projects", {})[cwd] = {"hasTrustDialogAccepted": True}
    path.write_text(json.dumps(data))


def test_inherit_copies_trust_only_when_source_had_it(tmp_path):
    home = tmp_path / "home"
    src = tmp_path / "home/.claude-accounts/main"
    dst = tmp_path / "home/.claude-accounts/relotto"
    cwd = str(tmp_path / "repo")
    (tmp_path / "repo").mkdir()
    for d in (home, src, dst):
        d.mkdir(parents=True, exist_ok=True)
    assert claude_trust.inherit_cwd_trust(
        cwd, source_config_dir=str(src), dest_config_dir=str(dst), home=str(home)
    ) is False
    assert not (dst / ".claude.json").exists()
    _write_trust(src / ".claude.json", cwd)
    assert claude_trust.inherit_cwd_trust(
        cwd, source_config_dir=str(src), dest_config_dir=str(dst), home=str(home)
    ) is True
    assert json.loads((dst / ".claude.json").read_text())["projects"][cwd]["hasTrustDialogAccepted"] is True
    assert json.loads((home / ".claude.json").read_text())["projects"][cwd]["hasTrustDialogAccepted"] is True


def test_inherit_accepts_home_trust_as_source(tmp_path):
    home = tmp_path / "home"
    dst = tmp_path / "home/.claude-accounts/relotto"
    cwd = str(tmp_path / "repo")
    (tmp_path / "repo").mkdir()
    dst.mkdir(parents=True)
    _write_trust(home / ".claude.json", cwd)
    assert claude_trust.inherit_cwd_trust(
        cwd, source_config_dir=None, dest_config_dir=str(dst), home=str(home)
    ) is True


def test_inherit_never_for_home_itself(tmp_path):
    home = tmp_path / "home"
    home.mkdir()
    _write_trust(home / ".claude.json", str(home))
    assert claude_trust.inherit_cwd_trust(
        str(home), source_config_dir=None, dest_config_dir=str(home / "x"), home=str(home)
    ) is False


def test_ancestor_walk_stops_at_git_directory_toplevel(tmp_path):
    """A normal clone's .git is a directory; trust above the toplevel must not leak in."""
    repo = tmp_path / "repo"
    (repo / ".git").mkdir(parents=True)
    deep = repo / "sub" / "deep"
    deep.mkdir(parents=True)
    home = tmp_path / "home"
    home.mkdir()
    _write_trust(home / ".claude.json", str(repo))
    assert claude_trust.cwd_trusted_in(str(deep), config_dir=None, home=str(home)) is True

    home2 = tmp_path / "home2"
    home2.mkdir()
    _write_trust(home2 / ".claude.json", str(tmp_path))
    assert claude_trust.cwd_trusted_in(str(deep), config_dir=None, home=str(home2)) is False


def test_ancestor_walk_stops_at_git_file_toplevel(tmp_path):
    """A linked worktree's .git is a plain file (gitdir pointer). os.path.isdir would miss
    it and walk past the toplevel; this is the regression guard for that line."""
    repo = tmp_path / "repo"
    repo.mkdir(parents=True)
    (repo / ".git").write_text("gitdir: /somewhere\n")
    deep = repo / "sub" / "deep"
    deep.mkdir(parents=True)
    home = tmp_path / "home"
    home.mkdir()
    _write_trust(home / ".claude.json", str(repo))
    assert claude_trust.cwd_trusted_in(str(deep), config_dir=None, home=str(home)) is True

    home2 = tmp_path / "home2"
    home2.mkdir()
    _write_trust(home2 / ".claude.json", str(tmp_path))
    assert claude_trust.cwd_trusted_in(str(deep), config_dir=None, home=str(home2)) is False


def test_inherit_cwd_trust_respects_git_toplevel_boundary(tmp_path):
    """inherit_cwd_trust must honor the same worktree-safe toplevel boundary as cwd_trusted_in."""
    repo = tmp_path / "repo"
    repo.mkdir(parents=True)
    (repo / ".git").write_text("gitdir: /somewhere\n")
    deep = repo / "sub" / "deep"
    deep.mkdir(parents=True)

    home = tmp_path / "home"
    home.mkdir()
    src = tmp_path / "home/.claude-accounts/main"
    dst = tmp_path / "home/.claude-accounts/relotto"
    src.mkdir(parents=True)
    dst.mkdir(parents=True)
    _write_trust(src / ".claude.json", str(repo))
    assert claude_trust.inherit_cwd_trust(
        str(deep), source_config_dir=str(src), dest_config_dir=str(dst), home=str(home)
    ) is True
    assert json.loads((dst / ".claude.json").read_text())["projects"][str(deep)]["hasTrustDialogAccepted"] is True

    home2 = tmp_path / "home2"
    home2.mkdir()
    src2 = tmp_path / "src2"
    dst2 = tmp_path / "dst2"
    src2.mkdir()
    dst2.mkdir()
    _write_trust(src2 / ".claude.json", str(tmp_path))
    assert claude_trust.inherit_cwd_trust(
        str(deep), source_config_dir=str(src2), dest_config_dir=str(dst2), home=str(home2)
    ) is False
    assert not (dst2 / ".claude.json").exists()


def test_launch_commands_do_not_persist_workspace_trust(tmp_path, monkeypatch):
    from test_session_route_matrix import Boundary
    boundary = Boundary(tmp_path, monkeypatch)
    home_file = tmp_path / '.claude.json'
    home_file.write_text('{"projects": {}}')
    for account in ('main', 'work'):
        config = tmp_path / 'claude' / account / '.claude.json'
        config.write_text('{"projects": {}}')
        command = boundary.dash._configuration_command('claude', 'claude', 'claude-sonnet-5', 'high', account)
        assert '--model claude-sonnet-5' in command
        assert 'skip-permissions' not in command
        assert _read(config) == {'projects': {}}
    assert _read(home_file) == {'projects': {}}


def test_harness_switch_leaves_trust_dialog_unconfirmed_without_enter(tmp_path, monkeypatch):
    from test_session_route_matrix import Boundary
    boundary = Boundary(tmp_path, monkeypatch)
    boundary.configure_source()
    boundary.screen = 'Accessing workspace: /tmp\n> Yes, I trust this folder'
    def configure(data):
        data = {k: v for k, v in data.items() if k not in ('session', 'pane', 'requestId')}
        return 200, boundary.run(**data)[0]
    monkeypatch.setattr(boundary.dash, 'session_configure', configure)
    _, result = boundary.dash.harness_switch_apply({'sess': 'audit', 'pane': '%1', 'to': 'claude',
        'model': boundary.state['model'], 'effort': boundary.state['effort'], 'account': 'main'})
    assert result['ok'] is False
    assert not boundary.events


def test_account_snapshot_copies_history_without_accepting_destination_trust(tmp_path, monkeypatch):
    from test_session_route_matrix import Boundary
    boundary = Boundary(tmp_path, monkeypatch)
    boundary.configure_source()
    home_file = tmp_path / '.claude.json'
    home_file.write_text('{"projects": {}}')
    config = tmp_path / 'claude/work/.claude.json'
    config.write_text('{"projects": {}}')
    adapter = boundary.adapter(harnessAccount='work', motorAccount='work')
    snapshot = adapter.snapshot(adapter.prepare())
    assert snapshot['origin']['resume_id'] == 'original-session'
    assert boundary.transcript('claude', 'work', 'original-session').read_text() == 'original history\n'
    assert _read(config) == _read(home_file) == {'projects': {}}
    assert not boundary.events


def test_acp_connect_does_not_accept_workspace_trust(tmp_path, monkeypatch):
    import importlib.machinery
    import importlib.util
    from types import SimpleNamespace
    from test_session_route_matrix import Boundary
    boundary = Boundary(tmp_path, monkeypatch)
    loader = importlib.machinery.SourceFileLoader('acp_trust_test', str(ROOT / 'bin/cc-acp'))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    monkeypatch.setattr(module.provider_registry, 'load_registry', lambda: boundary.registry)
    monkeypatch.setattr(module.acp, 'agent_available', lambda spec: True)
    opened = []
    session = SimpleNamespace(initialize=lambda: None, new_session=lambda: opened.append('new'),
                              models=[{'modelId': 'claude-sonnet-5'}], modes=[], current_model='claude-sonnet-5', close=lambda: None)
    monkeypatch.setattr(module.acp, 'open_session', lambda *args, **kwargs: session)
    cwd = tmp_path / 'workspace'; cwd.mkdir()
    pane = module.Pane(SimpleNamespace(cwd=str(cwd), agent='claude', model='claude-sonnet-5',
                                      effort='high', account='work', danger=False, resume=''))
    monkeypatch.setattr(pane, 'publish', lambda: None)
    pane.connect()
    assert opened == ['new']
    assert not (tmp_path / '.claude.json').exists()
    assert not (tmp_path / 'claude/work/.claude.json').exists()
