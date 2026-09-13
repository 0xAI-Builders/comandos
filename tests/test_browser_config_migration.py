import json
from pathlib import Path
import stat
import subprocess
import sys

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'lib'))


def module():
    assert (ROOT / 'lib/browser_config_migration.py').exists(), 'migration implementation missing'
    import browser_config_migration
    return browser_config_migration


def test_json_consolidates_global_and_project_servers_preserves_extension():
    m = module()
    original = {'secret': 'do-not-print', 'mcpServers': {
        'chrome-bg': {'command': 'old'}, 'chrome-current': {'command': 'old2'},
        'chrome-devtools': {'command': 'old3'}, 'claude-in-chrome': {'command': 'personal'},
        'chrome-devtools-current': {'command': 'old4'},
        'other': {'env': {'TOKEN': 'private'}}},
        'projects': {'/project': {'mcpServers': {'chrome-current': {'command': 'old'}}, 'keep': True}}}
    out = json.loads(m.transform_json(json.dumps(original), '/client'))
    assert out['mcpServers']['chrome-bg'] == {'type': 'stdio', 'command': '/client', 'args': []}
    assert set(out['mcpServers']) == {'chrome-bg', 'claude-in-chrome', 'other'}
    assert out['projects']['/project']['mcpServers']['chrome-bg']['command'] == '/client'
    assert out['secret'] == original['secret']
    assert out['mcpServers']['other'] == original['mcpServers']['other']
    assert m.transform_json(json.dumps({'keep': True}), '/client') == json.dumps({'keep': True})


def test_settings_only_disable_exact_plugin():
    m = module()
    original = {'enabledPlugins': {'chrome-devtools-mcp@chrome-devtools-plugins': True,
                                   'claude-in-chrome@official': True}}
    out = json.loads(m.transform_json(json.dumps(original), '/client'))
    assert out['enabledPlugins']['chrome-devtools-mcp@chrome-devtools-plugins'] is False
    assert out['enabledPlugins']['claude-in-chrome@official'] is True


def test_toml_nested_env_quoted_headers_and_unrelated_text():
    m = module()
    prefix = '# retained comment\napi_key = "private"\n\n'
    suffix = '[mcp_servers.other]\ncommand = "keep" # keep formatting\n[mcp_servers.other.env]\nTOKEN = "private"\n'
    original = prefix + '[mcp_servers."chrome-bg"]\ncommand="old"\n[mcp_servers."chrome-bg".env]\nTOKEN="old"\n' + suffix
    out = m.transform_toml(original, '/client')
    assert out.startswith(prefix)
    assert suffix in out
    assert 'TOKEN="old"' not in out
    assert m.tomllib.loads(out)['mcp_servers']['chrome-bg'] == {'command': '/client', 'args': []}
    assert m.transform_toml(out, '/client') == out


def test_toml_multiline_strings_are_preserved():
    m = module()
    original = 'description = """\n[mcp_servers.chrome-bg]\ncommand="text"\n"""\n[mcp_servers.chrome-current]\ncommand="old"\n'
    out = m.transform_toml(original, '/client')
    assert m.tomllib.loads(out)['description'] == m.tomllib.loads(original)['description']
    assert set(m.tomllib.loads(out)['mcp_servers']) == {'chrome-bg'}


def test_apply_requires_reviewed_hash_and_private_backup(tmp_path):
    m = module()
    path = tmp_path / '.claude.json'
    original = b'{"mcpServers":{"chrome-current":{"command":"old"}},"secret":"private"}'
    path.write_bytes(original)
    plan = m.build_plan([path], '/client')
    assert path.read_bytes() == original
    backups = m.apply_plan(plan, tmp_path / 'backups')
    assert len(backups) == 1
    assert Path(backups[0]).read_bytes() == original
    assert stat.S_IMODE(Path(backups[0]).stat().st_mode) == 0o600
    assert stat.S_IMODE((tmp_path / 'backups').stat().st_mode) == 0o700
    assert json.loads(path.read_text())['mcpServers']['chrome-bg']['command'] == '/client'


def test_changed_file_rejects_whole_plan_before_writes(tmp_path):
    m = module()
    paths = [tmp_path / 'one.json', tmp_path / 'two.json']
    original = '{"mcpServers":{"chrome-current":{"command":"old"}}}'
    for path in paths:
        path.write_text(original)
    plan = m.build_plan(paths, '/client')
    paths[1].write_text('{"concurrent":"private"}')
    with pytest.raises(m.MigrationError, match='changed'):
        m.apply_plan(plan, tmp_path / 'backups')
    assert paths[0].read_text() == original


def test_discovery_scoped_roots_and_allowlisted_environment(tmp_path):
    m = module()
    home = tmp_path / 'home'
    (home / '.codex/accounts/work').mkdir(parents=True)
    (home / '.codex/accounts/work/config.toml').write_text('')
    (home / '.claude').mkdir()
    (home / '.claude/settings.json').write_text('{}')
    (home / 'unrelated').mkdir()
    (home / 'unrelated/config.toml').write_text('')
    external = tmp_path / 'external'
    external.mkdir()
    (external / 'config.toml').write_text('')
    paths = m.discover_configs(home, {'CODEX_HOME': str(external), 'OTHER_SECRET': str(home / 'unrelated')}, proc_root=None)
    assert external / 'config.toml' in paths
    assert home / '.codex/accounts/work/config.toml' in paths
    assert home / 'unrelated/config.toml' not in paths


def test_cli_plan_contains_no_secrets_and_apply_uses_same_plan(tmp_path):
    m = module()
    cfg = tmp_path / 'config.toml'
    cfg.write_text('[mcp_servers.chrome-bg]\ncommand="old"\n[mcp_servers.other.env]\nTOKEN="do-not-print"\n')
    plan = tmp_path / 'plan.json'
    result = subprocess.run([sys.executable, str(ROOT / 'lib/browser_config_migration.py'), 'dry-run', '--config', str(cfg), '--wrapper', '/client', '--plan', str(plan)], capture_output=True, text=True)
    assert result.returncode == 0, result.stderr
    assert 'do-not-print' not in result.stdout + result.stderr + plan.read_text()
    assert stat.S_IMODE(plan.stat().st_mode) == 0o600
    result = subprocess.run([sys.executable, str(ROOT / 'lib/browser_config_migration.py'), 'apply', '--plan', str(plan), '--backup-dir', str(tmp_path / 'backups')], capture_output=True, text=True)
    assert result.returncode == 0, result.stderr
    assert 'do-not-print' not in result.stdout + result.stderr
    assert m.tomllib.loads(cfg.read_text())['mcp_servers']['chrome-bg']['command'] == '/client'


def test_wrapper_executes_only_direct_ssh_forwarding(tmp_path):
    wrapper = ROOT / 'bin/cc-browser-remote'
    assert wrapper.exists(), 'lightweight client missing'
    fake = tmp_path / 'ssh'
    fake.write_text('#!/bin/sh\nprintf "%s\\n" "$@"\n')
    fake.chmod(0o755)
    import os
    result = subprocess.run([str(wrapper)], env={**os.environ, 'PATH': str(tmp_path)}, capture_output=True, text=True)
    assert result.returncode == 0
    assert result.stdout.splitlines() == ['-T', '-o', 'BatchMode=yes', '-o', 'ExitOnForwardFailure=yes', '-o', 'ConnectTimeout=8', '-W', '127.0.0.1:19441', 'macmini']
    for prohibited in ('node', 'npm', 'npx', 'chrome-headless', 'fallback'):
        assert prohibited not in wrapper.read_text()


def test_duplicate_json_keys_are_refused_without_exposing_values():
    m = module()
    with pytest.raises(m.MigrationError, match='Duplicate JSON key') as error:
        m.transform_json('{"secret":"one","secret":"do-not-print","mcpServers":{"chrome-bg":{}}}', '/client')
    assert 'do-not-print' not in str(error.value)


def test_unsupported_inline_toml_refuses_rewrite():
    m = module()
    with pytest.raises(m.MigrationError, match='Unsupported TOML layout'):
        m.transform_toml('mcp_servers = {chrome-bg = {command="old"}, other = {command="keep"}}\n', '/client')


def test_process_environment_discovery_only_retains_allowed_roots(tmp_path):
    m = module()
    proc = tmp_path / 'proc/123'
    proc.mkdir(parents=True)
    account = tmp_path / 'account'
    account.mkdir()
    (account / 'config.toml').write_text('')
    unrelated = tmp_path / 'unrelated'
    unrelated.mkdir()
    (unrelated / 'config.toml').write_text('')
    (proc / 'environ').write_bytes(b'CODEX_HOME=' + str(account).encode() + b'\0TOKEN=do-not-print\0OTHER=' + str(unrelated).encode())
    found = m.discover_configs(tmp_path / 'home', {}, proc_root=proc.parent)
    assert found == [account / 'config.toml']


def test_symlink_config_refused_and_same_contents_replaced_file_rejected(tmp_path):
    m = module()
    original = '{"mcpServers":{"chrome-bg":{"command":"old"}}}'
    target = tmp_path / 'original.json'
    target.write_text(original)
    link = tmp_path / 'link.json'
    link.symlink_to(target)
    with pytest.raises(m.MigrationError):
        m.build_plan([link], '/client')
    plan = m.build_plan([target], '/client')
    replacement = tmp_path / 'new.json'
    replacement.write_text(original)
    replacement.replace(target)
    with pytest.raises(m.MigrationError, match='changed'):
        m.apply_plan(plan, tmp_path / 'backups')
