"""cc-app workspace integration, loaded without GTK (same loader as the split tests)."""
import ast
from pathlib import Path
from types import SimpleNamespace

SOURCE = Path('bin/cc-app').read_text()


def load(name, ns):
    tree = ast.parse(SOURCE)
    nodes = [n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == name]
    assert nodes, f'missing {name}'
    exec(compile(ast.Module(body=nodes, type_ignores=[]), '<app-test>', 'exec'), ns)
    return ns[name]


def test_registry_keeps_every_member_of_a_docked_group():
    group, plain = object(), object()
    view = SimpleNamespace(member_keys=lambda page: ['b', 'a'] if page is group else ['c'])
    ns = {'notebook_pages': lambda: [group, plain], 'tabs': {'a': 1, 'b': 1, 'c': 1}, 'WS_VIEW': view}
    assert load('current_tab_order', ns)() == ['b', 'a', 'c']


def test_shared_document_owns_the_strip_order():
    calls = []
    ns = {'_TAB_REORDERING': False, '_WS': {'doc': {'groups': []}},
          'notebook_pages': lambda: calls.append('reordered') or []}
    load('enforce_tab_order', ns)()
    assert calls == []


def test_closing_a_docked_tab_never_removes_another_page():
    # remove_page(-1) would drop the LAST page; a docked tab has page_num -1.
    text = ast.get_source_segment(SOURCE, next(n for n in ast.parse(SOURCE).body
                                              if isinstance(n, ast.FunctionDef) and n.name == 'close_tab'))
    assert 'nb.remove_page(nb.page_num(box))' not in text
    assert 'ws_forget(key, box)' in text


def test_mosaic_button_is_not_packed_but_shortcut_remains():
    assert '_headerbar.pack_end(_mosaic_btn)' not in SOURCE
    assert '_mosaic_toggle' in SOURCE


def test_dashboard_url_is_configurable_for_isolated_candidates():
    assert SOURCE.count('127.0.0.1:4777') == 1
    assert 'os.environ.get("COMANDOS_DASH_URL")' in SOURCE


def _sync_ns(calls, answers, confirm=True):
    """Run background work inline; record dashboard calls and popups."""
    def dash(path, payload=None, timeout=15):
        calls.append((path, payload))
        return answers.pop(0)
    return {'ES': True, '_dash_call': dash, '_in_background': lambda work, done: done(work()),
            '_confirm_must_answer': lambda **k: calls.append(('confirm', k['primary'])) or confirm,
            'notify_popup': lambda t, b: calls.append(('popup', b)), 'secrets': SimpleNamespace(token_hex=lambda n: 'rid'),
            'quote': lambda s, safe='': s}


def test_split_close_confirms_the_listed_identity_then_uses_the_guarded_route():
    calls = []
    answers = [(200, {'panes': [{'id': '%3', 'title': 'claude', 'identity': 'abc'}]}), (200, {'ok': True})]
    load('close_split_guarded', _sync_ns(calls, answers))('alpha', '%3')
    assert calls[0] == ('/terminal-panes', {'session': 'alpha', 'action': 'list'})
    assert calls[1][0] == 'confirm'
    assert calls[2] == ('/terminal-panes', {'session': 'alpha', 'action': 'close', 'pane': '%3', 'identity': 'abc'})


def test_split_close_cancelled_sends_nothing_destructive():
    calls = []
    answers = [(200, {'panes': [{'id': '%3', 'identity': 'abc'}]})]
    load('close_split_guarded', _sync_ns(calls, answers, confirm=False))('alpha', '%3')
    assert [c for c in calls if c[0] == '/terminal-panes' and c[1].get('action') == 'close'] == []


def test_group_close_posts_the_exact_confirmed_members_and_reports_partial_results():
    calls = []
    members = [{'tabId': 'a', 'session': 'a', 'label': 'A', 'sessionId': '$1', 'kept': False},
               {'tabId': 'local', 'session': 'local', 'label': 'local', 'sessionId': 'local', 'kept': True}]
    answers = [(200, {'revision': 7, 'members': members}),
               (200, {'ok': False, 'closed': [], 'remaining': ['a'], 'error': 'tmux falló'})]
    load('close_group_guarded', _sync_ns(calls, answers))('g1')
    post = next(c for c in calls if c[0] == '/workspace/close-group')
    assert post[1] == {'requestId': 'rid', 'groupId': 'g1', 'expectedRevision': 7, 'members': members}
    assert any(c[0] == 'popup' and 'tmux falló' in c[1] for c in calls)


def test_close_signal_with_a_list_closes_every_listed_tab(tmp_path):
    closed = []
    f = tmp_path / 'close.json'
    f.write_text('{"session": "b", "sessions": ["a", "b", "zz"]}')
    ns = {'Gio': SimpleNamespace(FileMonitorEvent=SimpleNamespace(CHANGES_DONE_HINT=1)), 'CLOSE_FILE': str(f),
          'json': __import__('json'), 'tabs': {'a': 1, 'b': 1}, 'close_tab': lambda k, confirm: closed.append((k, confirm))}
    load('on_tab_close_request', ns)(None, None, None, 1)
    assert closed == [('a', False), ('b', False)]


def test_desktop_restores_its_own_focus_once_without_overriding_a_choice():
    selected = []
    page = SimpleNamespace(_key='local')
    ns = {'_WS': {}, 'tabs': {'alpha': 1}, 'ws_select': selected.append,
          'nb': SimpleNamespace(get_nth_page=lambda i: page, get_current_page=lambda: 0)}
    load('_ws_restore_focus', ns)((200, {'activeTabId': 'alpha'}))
    assert selected == ['alpha']
    page._key = 'beta'
    load('_ws_restore_focus', ns)((200, {'activeTabId': 'alpha'}))
    assert selected == ['alpha']


def test_desktop_saves_focus_per_device_only_when_it_changes():
    posts = []
    page = SimpleNamespace(_key='alpha')
    ns = {'_WS': {'focus_restored': True}, 'WS_DEVICE': 'desktop-x',
          'nb': SimpleNamespace(get_nth_page=lambda i: page, get_current_page=lambda: 0),
          '_dash_call': lambda path, payload=None, timeout=15: posts.append((path, payload)) or (200, {}),
          '_in_background': lambda work, done: done(work())}
    save = load('_ws_save_focus', ns)
    save(); save()
    assert posts == [('/workspace/client', {'deviceId': 'desktop-x', 'activeTabId': 'alpha'})]
