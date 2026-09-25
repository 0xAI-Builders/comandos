"""Native callbacks are exercised without starting a local browser or GTK runtime."""
import ast
import re
from pathlib import Path
from types import SimpleNamespace as NS
from urllib.parse import quote, urlparse

SOURCE = Path('bin/cc-app').read_text()


def fixture():
    calls = []
    view = NS(hide=lambda: calls.append('hide'), load_uri=lambda uri: calls.append(uri),
              show=lambda: calls.append('show'), set_size_request=lambda *args: None)
    ns = {'_EXTENSION_SHELF': {'view': view, 'target': None}, 're': re, 'quote': quote,
          'urlparse': urlparse, 'BASE_URL': 'http://127.0.0.1:4777', '_DASH_V': 'test',
          '_pane_geometry': lambda sess: {'%4': ()} if sess == 'term-alpha' else {},
          'tabs': {}, 'win': NS(get_size=lambda: (1400,900))}
    names={'_open_extension_shelf','_close_extension_shelf','_extension_message'}
    nodes=[n for n in ast.parse(SOURCE).body if isinstance(n,ast.FunctionDef) and n.name in names]
    exec(compile(ast.Module(body=nodes,type_ignores=[]),'<cc-app>','exec'),ns)
    return ns, calls, view


def test_shelf_rejects_missing_or_foreign_pane_and_encodes_exact_target():
    ns,calls,_=fixture()
    for session,pane in [('term-alpha',''),('term-other','%4'),('term-alpha','%5'),('term-alpha','=other:')]:
        ns['_open_extension_shelf'](session,pane)
    assert calls == []
    ns['_open_extension_shelf']('term-alpha','%4','codex')
    assert calls == ['http://127.0.0.1:4777/extensions.html?session=term-alpha&pane=%254&harness=codex&v=test','show']


def test_close_stops_page_polling_and_does_not_destroy_the_terminal():
    ns,calls,_=fixture()
    ns['_EXTENSION_SHELF']['target']=('term-alpha','%4','codex')
    ns['_close_extension_shelf']()
    assert calls == ['hide','about:blank']
    assert ns['_EXTENSION_SHELF']['target'] is None


def test_only_trusted_shelf_document_can_send_close():
    ns,calls,view=fixture()
    message=NS(get_js_value=lambda: NS(to_string=lambda:'close'))
    for uri in ['https://example.org/extensions.html','http://127.0.0.1:4777/','about:blank']:
        view.get_uri=lambda:uri
        ns['_extension_message'](None,message)
    assert calls == []
    view.get_uri=lambda:'http://127.0.0.1:4777/extensions.html?pane=%254'
    ns['_extension_message'](None,message)
    assert calls == ['hide','about:blank']
