import ast
import io
import json
import sys
import threading
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from types import SimpleNamespace

import pytest

from test_dashboard_security import dash
from test_remote_ui import HTML, extract_js_function, run_node_json


def post(dash, data):
    handler = object.__new__(dash.Handler)
    body = json.dumps(data).encode()
    handler.path = '/prefs-set'
    handler._security_gate = lambda: None
    handler.headers = {'Content-Length': str(len(body))}
    handler.rfile = io.BytesIO(body)
    handler._json = lambda code, value, **kwargs: (code, value)
    return handler.do_POST()


def test_favorites_update_only_one_session_and_survive_reload(dash, monkeypatch, tmp_path):
    prefs = tmp_path / 'prefs.json'
    prefs.write_text(json.dumps({'favorites':['alpha'], 'theme':'dia', 'font_size':16}))
    monkeypatch.setattr(dash, 'PREFS_PATH', str(prefs))
    assert post(dash, {'favorite':{'session':'beta','enabled':True}})[0] == 200
    assert set(json.loads(prefs.read_text())['favorites']) == {'alpha','beta'}
    post(dash, {'favorite':{'session':'alpha','enabled':False}})
    post(dash, {'favorite':{'session':'beta','enabled':True}})
    assert dash.read_prefs()['favorites'] == ['beta']
    assert dash.read_prefs()['theme'] == 'dia'
    assert dash.read_prefs()['font_size'] == 16


@pytest.mark.parametrize('patch', [None, {}, {'session':'local','enabled':True},
    {'session':'alpha','enabled':'false'}, {'session':'','enabled':True}])
def test_invalid_favorite_does_not_write_prefs(dash, monkeypatch, tmp_path, patch):
    prefs = tmp_path / 'prefs.json'
    prefs.write_text('{"favorites":["alpha"]}')
    monkeypatch.setattr(dash, 'PREFS_PATH', str(prefs))
    assert post(dash, {'favorite':patch})[0] == 400
    assert prefs.read_text() == '{"favorites":["alpha"]}'


def test_simultaneous_clients_preserve_both_favorites_and_other_settings(dash, monkeypatch, tmp_path):
    prefs = tmp_path / 'prefs.json'
    prefs.write_text('{"favorites":[]}')
    monkeypatch.setattr(dash, 'PREFS_PATH', str(prefs))
    barrier = threading.Barrier(3)
    def update(data):
        barrier.wait(timeout=3)
        return post(dash, data)[0]
    patches = [{'favorite':{'session':s,'enabled':True}} for s in ['alpha','beta']]
    with ThreadPoolExecutor(max_workers=3) as pool:
        assert list(pool.map(update, patches + [{'font_size':19}])) == [200,200,200]
    assert set(dash.read_prefs()['favorites']) == {'alpha','beta'}
    assert dash.read_prefs()['font_size'] == 19


def test_failed_atomic_save_leaves_existing_preferences_readable(dash, monkeypatch, tmp_path):
    prefs = tmp_path / 'prefs.json'
    prefs.write_text('{"favorites":["alpha"],"theme":"dia"}')
    monkeypatch.setattr(dash, 'PREFS_PATH', str(prefs))
    def fail(*args): raise OSError('disk unavailable')
    monkeypatch.setattr(dash.os, 'replace', fail)
    assert post(dash, {'favorite':{'session':'beta','enabled':True}})[0] == 503
    assert json.loads(prefs.read_text()) == {'favorites':['alpha'],'theme':'dia'}
    assert list(tmp_path.iterdir()) == [prefs]


def test_api_groups_favorites_stably_and_keeps_home_first(dash, monkeypatch):
    monkeypatch.setattr(dash, 'read_prefs', lambda: {'favorites':['gamma','alpha','local','closed']})
    monkeypatch.setattr(dash, 'tmux_sessions', lambda: {'local','alpha','beta','gamma','delta','closed'})
    monkeypatch.setattr(dash, 'tab_labels', lambda: dict.fromkeys(['beta','alpha','delta','gamma'], 'Tab'))
    handler = object.__new__(dash.Handler)
    handler.path = '/tabs'
    handler._json = lambda code, value: value
    assert [t['session'] for t in handler._do_GET()] == ['local','alpha','gamma','beta','delta']


def test_web_order_keeps_home_first_without_mutating_registry():
    fn = extract_js_function(HTML, 'orderedTermTabs')
    result = run_node_json('''
const S={favs:new Set(['gamma','alpha','local'])};
const openTerms=new Map(['beta','alpha','local','delta','gamma'].map(k=>[k,{label:k}]));
''' + fn + '''
console.log(JSON.stringify({order:orderedTermTabs().map(([k])=>k), original:[...openTerms.keys()]}));
''')
    assert result['order'] == ['local','alpha','gamma','beta','delta']
    assert result['original'] == ['beta','alpha','local','delta','gamma']


def test_web_pending_changes_survive_old_polls_and_failed_writes():
    funcs = '\n'.join(extract_js_function(HTML,n) for n in
                      ['applyFavorites','setSessionFavorite','refreshFavorites'])
    result = run_node_json('''
const S={favs:new Set(),list:[]},favoritePending=new Map();
let favoriteQueue=Promise.resolve(),favoriteVersion=0,favoriteReadAt=0,favoriteReadPending=false;
let saved=[],readDone,fail=false,calls=[];
function render(){}
async function api(path,data){
  if(path==='/prefs')return new Promise(resolve=>readDone=resolve);
  calls.push(data.favorite);
  if(fail)throw new Error('offline');
  const {session,enabled}=data.favorite;
  saved=saved.filter(s=>s!==session);if(enabled)saved.push(session);
  return {favorites:saved.slice()};
}
''' + funcs + '''
(async()=>{
  const stale=refreshFavorites();
  await Promise.all([setSessionFavorite('alpha',true),setSessionFavorite('beta',true)]);
  readDone({favorites:[]});await stale;
  const afterStale=[...S.favs].sort();
  await setSessionFavorite('local',true);
  fail=true;let error='';
  try{await setSessionFavorite('alpha',false);}catch(e){error=e.message;}
  console.log(JSON.stringify({afterStale,afterFailure:[...S.favs].sort(),pending:favoritePending.size,
    error,calls}));
})();
''')
    assert result['afterStale'] == ['alpha','beta']
    assert result['afterFailure'] == ['alpha','beta']
    assert result['pending'] == 0
    assert result['error'] == 'offline'
    assert result['calls'] == [{'session':'alpha','enabled':True},
                               {'session':'beta','enabled':True},{'session':'alpha','enabled':False}]


def test_desktop_reorder_preserves_active_page_and_home_position():
    sys.path.insert(0, str(Path('lib').resolve()))
    from session_tabs import ordered_tab_keys
    source = Path('bin/cc-app').read_text()
    funcs = {n.name:ast.get_source_segment(source,n) for n in ast.parse(source).body
             if isinstance(n,ast.FunctionDef) and n.name in {'notebook_pages','enforce_tab_order'}}
    class Notebook:
        def __init__(self):
            self.pages = [SimpleNamespace(_key=k) for k in ['beta','gamma','local','alpha','delta']]
            self.active = self.pages[0]
            self.moves = 0
        def get_n_pages(self): return len(self.pages)
        def get_nth_page(self, i): return self.pages[i]
        def get_current_page(self): return self.pages.index(self.active)
        def page_num(self, p): return self.pages.index(p)
        def set_current_page(self, i): self.active = self.pages[i]
        def reorder_child(self, p, i):
            self.moves += 1
            self.pages.remove(p)
            self.pages.insert(i,p)
            # GTK emits page-reordered synchronously: exercise the recursion guard.
            ns['enforce_tab_order']()
    nb = Notebook()
    ns = {'nb':nb,'ordered_tab_keys':ordered_tab_keys,'TAB_FAVORITES':{'alpha','gamma'},
          '_TAB_REORDERING':False}
    exec('\n\n'.join(funcs.values()), ns)
    ns['enforce_tab_order']()
    assert [p._key for p in nb.pages] == ['local','gamma','alpha','beta','delta']
    assert nb.active._key == 'beta'
    moves = nb.moves
    ns['enforce_tab_order']()
    assert nb.moves == moves
