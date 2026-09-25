"""Durable extension drafts share the coordinator's pane lock; no live tmux."""
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))


def store(tmp_path):
    import pane_extensions
    return pane_extensions.ExtensionStore(tmp_path / 'operations.sqlite')


def selection(on=True):
    return {'mcps': {'mcp:docs': on}, 'skills': {'skill:tdd': on}}


def test_drafts_survive_reopen_but_never_cross_pane_conversation_or_generation(tmp_path):
    s = store(tmp_path)
    a = s.state('generation-1|%1', 'thread-1', 'codex', selection())
    changed = s.save(a['key'], a['revision'], selection(False))
    assert changed['revision'] == a['revision'] + 1
    assert store(tmp_path).state('generation-1|%1', 'thread-1', 'codex', selection())['desired'] == selection(False)
    for identity, conversation in [('generation-1|%2', 'thread-1'),
                                    ('generation-1|%1', 'thread-2'),
                                    ('generation-2|%1', 'thread-1')]:
        assert s.state(identity, conversation, 'codex', selection())['desired'] == selection()


def test_stale_revision_cannot_overwrite_another_clients_draft(tmp_path):
    import pane_extensions
    s = store(tmp_path)
    a = s.state('server|%1', 'thread', 'claude', selection())
    s.save(a['key'], a['revision'], selection(False))
    with pytest.raises(pane_extensions.ExtensionConflict, match='cambió'):
        s.save(a['key'], a['revision'], selection())


def test_active_configuration_operation_blocks_draft_edits_on_same_pane(tmp_path):
    import pane_extensions
    from session_operations import OperationStore
    s = store(tmp_path)
    a = s.state('server|%1', 'thread', 'claude', selection())
    other = s.state('server|%2', 'thread', 'claude', selection())
    ops = OperationStore(s.path)
    ops.claim('operation-123', 'server|%1', {'session': 's', 'pane': '%1'})
    with pytest.raises(pane_extensions.ExtensionConflict, match='operación'):
        s.save(a['key'], a['revision'], selection(False))
    assert s.save(other['key'], other['revision'], selection(False))['desired'] == selection(False)
    ops.stage('operation-123', 'failed', result={'ok': False})
    assert s.save(a['key'], a['revision'], selection(False))['desired'] == selection(False)


def test_apply_revision_check_rejects_draft_changed_before_operation_claim(tmp_path):
    import pane_extensions
    s = store(tmp_path)
    a = s.state('server|%1', 'thread', 'codex', selection())
    s.save(a['key'], a['revision'], selection(False))
    with pytest.raises(pane_extensions.ExtensionConflict):
        s.require_revision(a['key'], a['revision'])


def test_named_templates_are_shared_without_project_or_harness_defaults(tmp_path):
    s = store(tmp_path)
    t = s.save_template('Código + docs', selection(False))
    assert store(tmp_path).templates() == [t]
    assert t['selection'] == selection(False)
    assert 'project' not in t and 'harness' not in t
    assert s.state('server|%2', 'new-thread', 'grok', selection())['desired'] == selection()


@pytest.mark.parametrize('invalid', [
    {'mcps': {'mcp:docs': 'false'}, 'skills': {}},
    {'mcps': {}, 'skills': {}, 'command': 'private'},
    {'mcps': {'mcp:bad\nname': False}, 'skills': {}},
    {'mcps': [], 'skills': {}},
])
def test_selection_rejects_unknown_fields_and_non_boolean_or_malformed_ids(tmp_path, invalid):
    s = store(tmp_path)
    with pytest.raises(ValueError):
        s.state('server|%1', 'thread', 'codex', invalid)


def test_template_names_are_bounded_and_do_not_accept_empty_or_control_text(tmp_path):
    s = store(tmp_path)
    for name in ('', '   ', 'x' * 81, 'hello\nworld'):
        with pytest.raises(ValueError):
            s.save_template(name, selection())
