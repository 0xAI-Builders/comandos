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


def test_desktop_reopens_on_the_last_tab_unless_the_user_already_picked_one():
    """Grill 30-sep: «cuando cargo ComandOS no me pone la última sesión». The
    restore used to run only when the current tab was local, but restore_tabs
    leaves the LAST restored tab selected, so it never ran."""
    selected = []
    page = SimpleNamespace(_key='beta')           # restore_tabs left another tab selected
    ns = {'_WS': {}, '_PRESENCE': {'last': 0.0}, 'tabs': {'alpha': 1, 'beta': 1}, 'ws_select': selected.append,
          'nb': SimpleNamespace(get_nth_page=lambda i: page, get_current_page=lambda: 0)}
    load('_ws_restore_focus', ns)((200, {'activeTabId': 'alpha'}))
    assert selected == ['alpha'] and ns['_WS']['focus_ready'] is True
    ns['_WS'] = {}
    ns['_PRESENCE']['last'] = 123.0               # a real click/key already chose a tab
    load('_ws_restore_focus', ns)((200, {'activeTabId': 'alpha'}))
    assert selected == ['alpha'], "a choice made by the person is never overridden"


def test_focus_is_not_saved_until_the_restore_answered():
    posts = []
    page = SimpleNamespace(_key='beta')
    ns = {'_WS': {'focus_restored': True}, 'WS_DEVICE': 'desktop-x',
          'nb': SimpleNamespace(get_nth_page=lambda i: page, get_current_page=lambda: 0),
          '_dash_call': lambda path, payload=None, timeout=15: posts.append((path, payload)) or (200, {}),
          '_in_background': lambda work, done: done(work())}
    load('_ws_save_focus', ns)()
    assert posts == [], "the tab restore_tabs happened to leave selected must not overwrite the saved focus"


def test_desktop_saves_focus_per_device_only_when_it_changes():
    posts = []
    page = SimpleNamespace(_key='alpha')
    ns = {'_WS': {'focus_restored': True, 'focus_ready': True}, 'WS_DEVICE': 'desktop-x',
          'nb': SimpleNamespace(get_nth_page=lambda i: page, get_current_page=lambda: 0),
          '_dash_call': lambda path, payload=None, timeout=15: posts.append((path, payload)) or (200, {}),
          '_in_background': lambda work, done: done(work())}
    save = load('_ws_save_focus', ns)
    save(); save()
    assert posts == [('/workspace/client', {'deviceId': 'desktop-x', 'activeTabId': 'alpha'})]


def test_desktop_presence_marks_interaction_only_for_explicit_input_and_throttles():
    posts = []
    clock = [1000.0]
    ns = {'_PRESENCE': {'last': 0.0, 'visible': True}, 'WS_DEVICE': 'desktop-x',
          'time': SimpleNamespace(monotonic=lambda: clock[0]),
          '_dash_call': lambda path, payload=None, timeout=15: posts.append((path, payload)) or (200, {}),
          '_in_background': lambda work, done: done(work())}
    report = load('report_presence', ns)
    report(interaction=True)
    report(interaction=True)                 # throttled
    clock[0] += 6
    report(interaction=False)                # heartbeat, never an interaction
    assert posts == [('/presence', {'deviceId': 'desktop-x', 'kind': 'desktop', 'visible': True,
                                    'canPlayAudio': True, 'interaction': True}),
                     ('/presence', {'deviceId': 'desktop-x', 'kind': 'desktop', 'visible': True,
                                    'canPlayAudio': True, 'interaction': False})]


def test_hook_and_desktop_share_one_device_id():
    import re
    import subprocess
    node = 'my host.local'
    bash = subprocess.run(['bash', '-c', "printf '%s' \"$1\" | tr -c 'A-Za-z0-9_.-' '-' | cut -c1-60", '_', node],
                          capture_output=True, text=True).stdout.rstrip('\n')
    assert 'desktop-' + bash == 'desktop-' + re.sub(r"[^A-Za-z0-9_.-]", "-", node)[:60]
    assert "tr -c 'A-Za-z0-9_.-' '-' | cut -c1-60" in Path('hooks/cc-notify.sh').read_text()


def test_operator_tab_reorder_commits_the_shared_arrangement():
    commits = []
    doc = {"groups": [{"id": "g-local", "tree": {"type": "tab", "tabId": "local"}},
                      {"id": "g-a", "tree": {"type": "tab", "tabId": "a"}},
                      {"id": "g-b", "tree": {"type": "tab", "tabId": "b"}}]}
    import sys; sys.path.insert(0, "lib"); import workspace_layout
    ns = {'_WS': {'doc': doc}, 'workspace_layout': workspace_layout,
          'ws_commit': lambda d, focus=None: commits.append(([g["id"] for g in d["groups"]], focus)),
          '_page_for': lambda s: (-1, None), 'nb': None, 'save_tabs': lambda: None}
    assert load('tab_reorder', ns)("b", 1) is True
    assert commits == [(["g-local", "g-b", "g-a"], "b")]


def test_desktop_trays_appear_below_the_strip_and_win_over_docking():
    """Grill 30-sep: arrastrar una pestaña hacia abajo muestra 4 bandejas de estado."""
    ns = {}
    rects = load('ws_tray_rects', ns)(1200, 800)
    assert [m for m, _r in rects] == ['frozen', 'awaiting_reply', 'resolved', 'none']
    xs = [r[0] for _m, r in rects]
    assert xs == sorted(xs) and all(r[1] + r[3] < 800 for _m, r in rects), "centred row near the bottom"
    hit = load('ws_tray_at', ns)
    m, (x, y, w, h) = rects[2]
    assert hit(rects, x + w / 2, y + h / 2, strip_bottom=40) == 'resolved'
    assert hit(rects, x + w / 2, 20, strip_bottom=40) is None, "not while still on the strip"


def test_desktop_drag_scrolls_the_strip_at_its_edges():
    """Paridad con remoto (grill 30-sep): al arrastrar cerca del borde de la
    barra, las pestañas corren solas."""
    step = load('ws_strip_edge_step', {})
    assert step(10, 1000) == -1 and step(990, 1000) == 1 and step(500, 1000) == 0


def test_desktop_pane_card_has_no_yes_no_and_shows_semaforo_session_model_and_buttons():
    """Fix 1 (30-sep): Sí/No leave the desktop card (they live in the remote touch bar);
    the card is semáforo + session | logo + model + ✓ | «IA» and «MCPs · Skills»."""
    assert 'def pane_answer' not in SOURCE and '_answer_button' not in SOURCE and 'pill-yes' not in SOURCE
    src = SOURCE.split('def _pane_pill(sess, p, keys=True):')[1].split('\ndef ')[0]
    assert '_pane_card_ai(sess)' in src and 'getattr(box, "_label", None) or sess' in src, 'semáforo + tab name (not the tmux key) first'
    keys = SOURCE.split('def _pane_card_keys(sess, p, harness):')[1].split('\ndef ')[0]
    assert '_card_button("settings", "IA"' in keys and '_extension_pill(sess' in keys
    place = SOURCE.split('def _place_pills(box, panes, geo=None):')[1].split('\ndef ')[0]
    assert 'ov.set_overlay_pass_through(pill, True)' in place, 'the card never blocks dragging the tmux border'
    assert 'add_class("pane-card")' in src
    ext = SOURCE.split('def _extension_pill(sess, pane, harness=""):')[1].split('\ndef ')[0]
    assert '_card_button("sparkles", "MCPs · Skills"' in ext


class _Notebook:
    def __init__(self, pages):
        self.pages, self.current = list(pages), 0
    def get_n_pages(self): return len(self.pages)
    def get_nth_page(self, i): return self.pages[i]
    def get_current_page(self): return self.current
    def set_current_page(self, i): self.current = i
    def page_num(self, page): return self.pages.index(page)
    def prev_page(self): self.current = max(0, self.current - 1)
    def next_page(self): self.current = min(len(self.pages) - 1, self.current + 1)
    def get_tab_label(self, page): return SimpleNamespace(page=page)


def _drag_ns(pages, current):
    nb = _Notebook(pages)
    nb.current = current
    layer = SimpleNamespace(show=lambda: None, hide=lambda: None, queue_draw=lambda: None,
                            get_allocated_width=lambda: 1000, get_allocated_height=lambda: 600)
    entries = [(p, (i * 100, 0, 100, 30)) for i, p in enumerate(pages)]
    ns = {'nb': nb, '_WS': {'doc': {'groups': []}, 'drag': None, 'target': None, 'press': None},
          '_ws_layer': layer, 'GLib': SimpleNamespace(timeout_add=lambda ms, fn: 1),
          'notebook_pages': lambda: list(pages), 'gtk_workspace': SimpleNamespace(GroupPage=type('G', (), {})),
          '_ws_layout': lambda: {'strip': (0, 0, 1000, 30), 'entries': entries},
          '_ws_target': lambda x, y: {'kind': 'bar'}, 'ws_strip_edge_step': lambda x, w, zone=56: 0,
          '_ws_grab': lambda on: None, '_ws_button_still_down': lambda: True, '_ws_finish': lambda t: None}
    for name in ('_ws_source_page', '_ws_drag_begin', '_ws_drag_end', '_ws_edge_tick'):
        load(name, ns)
    return ns, nb


def test_drag_begin_returns_to_the_view_you_were_looking_at_and_lifts_the_tab():
    """Fix 4 (30-sep): the click on tab B already switched the notebook to B; the
    drag must show A again so B can be dropped INTO A's area (group them)."""
    a, b = SimpleNamespace(_key='A'), SimpleNamespace(_key='B')
    ns, nb = _drag_ns([a, b], current=1)          # notebook switched to B on press
    ns['_ws_drag_begin']('B', press_page=0)
    assert nb.current == 0 and ns['_WS']['drag_page'] == 0
    assert ns['_WS']['source_label'].page is b, 'the lifted tab is covered on the strip and drawn as a ghost'
    ns['_ws_drag_end']()
    assert ns['_WS']['source_label'] is None and ns['_WS']['drag'] is None


def test_hovering_another_tab_while_dragging_reveals_it_after_two_ticks():
    a, b, c = (SimpleNamespace(_key=k) for k in 'ABC')
    ns, nb = _drag_ns([a, b, c], current=0)
    ns['_ws_drag_begin']('B', press_page=0)
    ns['_WS']['pointer'] = (250, 10)             # over C
    assert ns['_ws_edge_tick']() is True and nb.current == 0, 'first tick only arms the dwell'
    ns['_ws_edge_tick']()
    assert nb.current == 2, 'second tick over the same tab switches the view: now B can be docked into C'
    ns['_WS']['pointer'] = (150, 10)             # over the dragged tab itself: never switches
    ns['_ws_edge_tick'](); ns['_ws_edge_tick']()
    assert nb.current == 2
    ns['_WS']['pointer'] = (250, 300)            # below the strip: no dwell
    assert ns['_ws_edge_tick']() is True and ns['_WS']['dwell'] is None


def test_bar_target_is_a_tab_sized_slot_and_the_layer_draws_ghost_and_cover():
    src = SOURCE
    assert 'target["slot"] = True' in src and 'def _ws_draw_ghost' in src and 'def _ws_draw_lifted' in src
    draw = src.split('def _ws_draw(widget, cr):')[1].split('\n\n\n')[0]
    assert '_ws_draw_lifted(cr)' in draw and '_ws_draw_ghost(cr)' in draw and 'cr.set_dash([5, 4])' in draw


def test_a_lost_release_ends_the_drag_instead_of_leaving_it_stuck():
    """The real bug of 30-sep: dragging a docked leaf's header and switching page unmapped
    the pressed widget, the release never arrived and _WS['drag'] stayed set, so the
    workspace stopped syncing. The tick now finishes the drag when button 1 is up."""
    a, b = SimpleNamespace(_key='A'), SimpleNamespace(_key='B')
    ns, nb = _drag_ns([a, b], current=0)
    finished = []
    ns['_ws_finish'] = lambda t: finished.append(t) or ns['_ws_drag_end']()
    ns['_ws_drag_begin']('B', press_page=0)
    ns['_WS']['pointer'] = (50, 10)
    ns['_ws_button_still_down'] = lambda: False
    assert ns['_ws_edge_tick']() is False
    assert finished == [{'kind': 'bar'}] and ns['_WS']['drag'] is None
    src = SOURCE
    assert 'win.connect("button-release-event", _ws_window_release)' in src and 'seat.grab(win.get_window()' in src


def test_group_members_get_their_own_card_and_frame_and_only_the_focused_one_is_green():
    """30-sep: in a docked group every terminal carries the card and frame like a single
    tab, and only the focused member's active pane is drawn green."""
    refresh = SOURCE.split('def _refresh_tab_models():')[1].split('\ndef ')[0]
    assert 'visible = _visible_boxes()' in refresh and 'box not in visible' in refresh
    group = type('GroupPage', (), {})
    a, b = object(), object()
    page = group(); page.members = {'A': a, 'B': b}; page.focus = 'B'
    ns = {'nb': SimpleNamespace(get_n_pages=lambda: 1, get_current_page=lambda: 0, get_nth_page=lambda _: page),
          'gtk_workspace': SimpleNamespace(GroupPage=group)}
    assert load('_visible_boxes', ns)() == [a, b]
    focused = load('_box_focused', ns)
    assert focused(b) and not focused(a)
