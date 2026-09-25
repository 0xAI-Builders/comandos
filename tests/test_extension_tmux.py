"""One real coordinator/launcher seam; private tmux and a non-networking CLI."""
from pathlib import Path

from test_session_tmux import SID, live  # noqa: F401 — reuse bounded private fixture
from session_operations import run_operation


def test_real_extension_apply_verifies_receipt_and_preserves_conversation_and_sibling(live):
    dash = live.dash
    query = {'session': 'audit', 'pane': live.pane}
    config = live.tmp_path / '.codex/config.toml'
    config.write_text('[mcp_servers.fixture_docs]\ncommand="/bin/false"\n')
    original_config = config.read_bytes()
    sibling = live.checked('display-message', '-p', '-t', live.other,
                           '#{pane_pid}\t#{pane_current_command}')
    layout = live.checked('display-message', '-p', '-t', live.pane, '#{window_layout}')
    original_pid = dash.agent_info_for_pane(live.pane)['pid']

    initial = dash.pane_extensions_state(query)
    assert initial['conversationId'] == SID and initial['loaded'] is None
    desired = {kind: dict(rows) for kind, rows in initial['desired'].items()}
    desired['mcps']['fixture_docs'] = False
    status, saved = dash.pane_extensions_write('', {
        **query, 'expectedIdentity': initial['identity'], 'expectedConversationId': SID,
        'revision': initial['revision'], 'desired': desired,
    })
    assert status == 200 and saved['loaded'] is None
    assert saved['desired']['mcps']['fixture_docs'] is False

    target = dash._extension_target(query)
    request = {
        **query, 'harness': 'codex', 'requestId': 'real-extension-tmux',
        'expectedIdentity': saved['identity'], 'expectedConversationId': SID,
        'revision': saved['revision'], 'extensionDraftKey': target['draft']['key'],
        'extensionsOnly': True, 'interrupt': True,
    }
    identity = dash._pane_identity('audit', live.pane)
    adapter = dash.PaneExtensionConfiguration(request, identity)
    adapter.store = live.store
    assert live.store.claim(request['requestId'], saved['identity'], request)
    dash._extension_store().require_revision(target['draft']['key'], saved['revision'])
    result = run_operation(live.store, request['requestId'], adapter)
    assert result['ok'], result
    assert result['observed']['conversationId'] == SID
    assert result['observed']['extensionsConfirmed'] is True

    operation = live.store.get(request['requestId'])
    assert operation['state'] == 'confirmed'
    launch = operation['snapshot']['destination']['extensionLaunch']
    pid = dash.agent_info_for_pane(live.pane)['pid']
    assert pid != original_pid
    assert dash.extension_launch.verify_launch(pid, launch)
    assert dash.extension_launch.launch_from_pid(pid) == launch
    assert Path(launch['manifest']).stat().st_mode & 0o777 == 0o600
    argv = dash._proc_cmdline(pid)
    assert argv[argv.index('resume') + 1] == SID
    assert 'mcp_servers.fixture_docs.enabled=false' in argv
    assert argv[argv.index('--sandbox') + 1] == 'read-only'
    assert argv[argv.index('--ask-for-approval') + 1] == 'untrusted'

    current = dash.pane_extensions_state(query)
    assert current['conversationId'] == SID
    assert current['revision'] == saved['revision']
    assert current['desired'] == current['loaded'] == desired
    assert current['operation']['state'] == 'confirmed'
    assert config.read_bytes() == original_config
    assert live.checked('display-message', '-p', '-t', live.other,
                        '#{pane_pid}\t#{pane_current_command}') == sibling
    assert live.checked('display-message', '-p', '-t', live.pane, '#{window_layout}') == layout
