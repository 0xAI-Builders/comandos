from pathlib import Path

import pytest

from test_agent_launch import load_dash_module
from test_provider_accounts import registry


@pytest.mark.parametrize('source_trust,destination_trust,expected', [
    ('trusted', '', True), ('untrusted', '', False), ('trusted', 'untrusted', False),
])
def test_codex_switch_inherits_only_existing_trust_and_preserves_target_config(
        tmp_path, monkeypatch, source_trust, destination_trust, expected):
    dash = load_dash_module()
    monkeypatch.setattr(dash, 'load_provider_registry', lambda: registry(tmp_path))
    cwd = tmp_path / 'project'; cwd.mkdir()
    main = tmp_path / 'codex'; main.mkdir()
    target = tmp_path / 'codex-accounts' / 'work'; target.mkdir(parents=True)
    (main / 'config.toml').write_text(f'[projects."{cwd}"]\ntrust_level = "{source_trust}"\n')
    original = 'cli_auth_credentials_store = "file"\n# Keep my settings\n'
    if destination_trust:
        original += f'[projects."{cwd}"]\ntrust_level = "{destination_trust}"\n'
    config = target / 'config.toml'; config.write_text(original)
    assert dash.inherit_trust_for_switch(str(cwd), 'main', 'work', harness='codex') is expected
    assert config.read_text().startswith(original)
    if expected:
        assert dash.extension_launch._read(config)['projects'][str(cwd)]['trust_level'] == 'trusted'
    else:
        assert config.read_text() == original


def test_corrupt_destination_config_is_not_changed_by_codex_switch(tmp_path, monkeypatch):
    dash = load_dash_module()
    monkeypatch.setattr(dash, 'load_provider_registry', lambda: registry(tmp_path))
    cwd = tmp_path / 'project'; cwd.mkdir()
    main = tmp_path / 'codex'; main.mkdir()
    target = tmp_path / 'codex-accounts' / 'work'; target.mkdir(parents=True)
    (main / 'config.toml').write_text(f'[projects."{cwd}"]\ntrust_level = "trusted"\n')
    config = target / 'config.toml'; config.write_text('not valid [')
    assert dash.inherit_trust_for_switch(str(cwd), 'main', 'work', harness='codex') is False
    assert config.read_text() == 'not valid ['
