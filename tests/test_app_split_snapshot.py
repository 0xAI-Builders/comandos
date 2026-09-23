"""Load the app adapters without importing GTK or touching real terminals."""
import ast
import json
from pathlib import Path
from types import SimpleNamespace

SOURCE = Path('bin/cc-app').read_text()

def load(name, ns):
    tree=ast.parse(SOURCE)
    nodes=[n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name==name]
    assert nodes, f'missing {name}'
    exec(compile(ast.Module(body=nodes,type_ignores=[]),'<app-test>','exec'),ns)
    return ns[name]


def test_layout_snapshot_does_not_run_before_startup_restore_finishes():
    ns={'_LAYOUT_RESTORE_READY':False}
    assert load('snapshot_layouts',ns)() is False


def test_strict_resume_never_substitutes_last_conversation():
    import shlex
    ns={'resume_command':lambda p:'codex resume --last','shlex':shlex}
    fn=load('exact_resume_command',ns)
    assert fn({'agent':'codex','resume_id':'missing'}) is None
    ns['resume_command']=lambda p:'codex resume exact-id'
    assert fn({'agent':'codex','resume_id':'exact-id'})=='codex resume exact-id'
    assert fn({'agent':'codex'}) is None


def test_complete_layout_restores_before_legacy_single_pane_fallback():
    calls=[]
    ns={'layout_snapshot':SimpleNamespace(read_snapshot=lambda p:{'sessions':{'test':{'windows':[{}]}}},
        restore_session=lambda *a:calls.append(a)), 'LAYOUT_SNAPSHOT_FILE':'unused',
        'tmuxc':object(),'exact_resume_command':object()}
    assert load('restore_saved_layout',ns)('test') is True
    assert calls[0][1:3]==('test',{'windows':[{}]})
    assert ns['restore_saved_layout']('absent') is False


def test_app_captures_layout_on_normal_exit_and_restores_local_before_spawn():
    assert SOURCE.index('restore_saved_layout("local")') < SOURCE.index('hub = make_term(')
    assert 'finally:\n    snapshot_layouts(wait=True)' in SOURCE
    restore=next(n for n in ast.parse(SOURCE).body if isinstance(n,ast.FunctionDef) and n.name=='restore_tabs')
    text=ast.get_source_segment(SOURCE,restore)
    assert 'restore_saved_layout(sess)' in text


def _snapshot_ns(previous, captures, live):
    written = {}
    def capture(tmux, key, inspector):
        if isinstance(captures.get(key), Exception):
            raise captures[key]
        return captures[key]
    import threading
    ns = {'_LAYOUT_RESTORE_READY': True, '_LAYOUT_LOCK': threading.Lock(),
          'tabs': {k: None for k in captures if k != 'local'}, 'PaneInspector': lambda: None,
          'LAYOUT_SNAPSHOT_FILE': 'unused', 'sys': __import__('sys'), 'print': lambda *a, **k: None,
          'tmuxc': lambda *a: SimpleNamespace(returncode=0 if live is not None else 1,
                                               stdout='\n'.join(live or [])),
          'layout_snapshot': SimpleNamespace(
              read_snapshot=lambda p: {'sessions': previous},
              capture_session=capture,
              carry_resume_ids=lambda cap, prev: dict(cap, carried=prev is not None),
              write_snapshot=lambda p, data: written.update(data))}
    load('snapshot_layouts', ns)(wait=True)
    return written


def test_snapshot_agent_without_id_does_not_freeze_the_whole_session():
    fresh = {'windows': [{'panes': [{'id': '%1', 'agent': 'grok'}]}]}
    written = _snapshot_ns({'term-a': {'old': True}}, {'local': {'windows': []}, 'term-a': fresh}, ['local', 'term-a'])
    assert written['term-a']['windows'] == fresh['windows'] and written['term-a']['carried'] is True


def test_snapshot_prunes_closed_tabs_and_dead_sessions_but_survives_tmux_down():
    previous = {'term-closed': {'x': 1}, 'term-dead': {'x': 2}, 'local': {'x': 3}}
    fail = RuntimeError('no session')
    written = _snapshot_ns(previous, {'local': fail, 'term-dead': fail}, ['local'])
    assert written == {'local': {'x': 3}}
    written = _snapshot_ns(previous, {'local': fail, 'term-dead': fail}, None)
    assert written == {'local': {'x': 3}, 'term-dead': {'x': 2}}
