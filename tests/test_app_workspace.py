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
