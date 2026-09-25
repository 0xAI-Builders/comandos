import pytest

from test_dashboard_security import dash
from test_remote_ui import HTML, extract_js_function, run_node_json


def test_remote_tabs_include_desktop_home_without_resurrecting_closed_tabs(dash, monkeypatch):
    monkeypatch.setattr(dash, 'tmux_sessions', lambda: {'local', 'alpha', 'beta', 'closed'})
    monkeypatch.setattr(dash, 'tab_labels', lambda: {'beta':'Beta', 'alpha':'Alpha', 'dead':'Dead'})
    handler = object.__new__(dash.Handler)
    handler.path = '/tabs'
    handler._json = lambda code, body: (code, body)
    code, tabs = handler._do_GET()
    assert code == 200
    assert tabs == [
        {'session':'local', 'label':'⌂ local', 'closable':False},
        {'session':'beta', 'label':'Beta'},
        {'session':'alpha', 'label':'Alpha'},
    ]


def test_pinch_zoom_fits_the_app_to_the_visible_area():
    # Antes el zoom mantenia el layout completo y habia que desplazarse; en
    # tablet no se podia volver arriba y las pestanas quedaban fuera de vista.
    fn = extract_js_function(HTML, 'currentViewportHeight')
    result = run_node_json(f'''
const window={{innerHeight:844,visualViewport:{{height:422,scale:2}}}};
const document={{documentElement:{{clientHeight:844}}}};
{fn}
const zoom=currentViewportHeight();
window.visualViewport={{height:490,scale:1}};
console.log(JSON.stringify({{zoom,keyboard:currentViewportHeight()}}));
''')
    assert result == {'zoom':422, 'keyboard':490}


def test_home_close_is_rejected_before_any_state_mutation(dash, monkeypatch):
    def unexpected(*args, **kwargs):
        pytest.fail("Closing LOCAL must not read or mutate tab/session state")
    for name in ('session_labels', 'tmux', 'remember_tab', 'remove_tab_metadata'):
        monkeypatch.setattr(dash, name, unexpected)
    assert dash.close_app_tab('local') == 'La pestaña local permanece abierta'


def test_opening_home_never_requests_a_duplicate_desktop_tab(dash, monkeypatch):
    def unexpected(*args, **kwargs):
        pytest.fail("LOCAL already exists as the pinned desktop tab")
    monkeypatch.setattr(dash, 'load_json_file', unexpected)
    monkeypatch.setattr(dash, 'write_json_file', unexpected)
    dash.register_app_tab('local', '⌂ local')
