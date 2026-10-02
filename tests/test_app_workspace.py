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
    assert '_card_button("ia-gear", "IA"' in keys and '_extension_pill(sess' in keys
    place = SOURCE.split('def _place_pills(box, panes, geo=None):')[1].split('\ndef ')[0]
    assert 'ov.set_overlay_pass_through(pill, True)' in place, 'the card never blocks dragging the tmux border'
    assert 'add_class("pane-card")' in src
    ext = SOURCE.split('def _extension_pill(sess, pane, harness=""):')[1].split('\ndef ')[0]
    assert '_card_button("ia-spark", "MCPs · Skills"' in ext


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


def test_focusing_a_group_member_republishes_the_command_bar_target():
    # Un clic dentro de otra terminal del grupo mueve GroupPage.focus; la app debe
    # volver a publicar la pestaña activa (destino de la barra), guardar el foco y
    # repintar los marcos, igual que al cambiar de pestaña.
    body = SOURCE[SOURCE.index("def _ws_member_focused("):SOURCE.index("WS_VIEW.on_focus = _ws_member_focused")]
    assert "_report_active_tab" in body and "_ws_save_focus" in body and "_refresh_tab_models" in body
    assert "WS_VIEW.on_focus = _ws_member_focused" in SOURCE


def test_stacked_panes_breathe_and_the_resize_handle_lights_up_on_the_gutter():
    """1-oct: 5 px between a pane's frame and the header below it; hovering or dragging
    the gutter between panes paints a handle (tmux's own border line is hidden)."""
    frames_src = SOURCE.split('def _pane_frames(')[1].split('\ndef ')[0]
    assert "rail + 3" in frames_src and "bottom - 2" in frames_src
    assert 'gutters.append(("v"' in frames_src and 'gutters.append(("h"' in frames_src
    ns = {'_PANE_GUTTERS': {}}
    at = load('_gutter_at', ns)
    term = object()
    box = SimpleNamespace(_term=term)
    ns['_PANE_GUTTERS'][id(term)] = [("v", 100, 10, 8, 200), ("h", 0, 300, 400, 5)]
    assert at(box, 104, 50) == 0 and at(box, 50, 302) == 1 and at(box, 50, 50) is None
    draw = SOURCE.split('def _draw_pane_frames(box, cr):')[1].split('\ndef ')[0]
    assert '_hot_gutter' in draw and '1.0 if dragging else 0.55' in draw and '(44, 8) if dragging else (34, 6)' in draw
    assert '_grip_cursor(w, "grabbing")' in SOURCE and '_grip_cursor(w, "grab")' in SOURCE


def test_dragging_the_gutter_is_done_by_comandos_with_half_on_double_click():
    """Grill 1-oct: the handle is ComandOS's own (wide zone, not tmux's 1-cell border);
    the size follows the pointer by cells, each side keeps 2, double click halves."""
    ns = {}
    for name in ('_gutter_neighbor', '_gutter_target', '_gutter_half'):
        load(name, ns)
    # izquierda %1 (0..59) | derecha arriba %2 (61..119, 0..19) / derecha abajo %3 (61.., 21..39)
    geo = {'%1': (0, 0, 60), '%2': (61, 0, 59), '%3': (61, 21, 59)}
    heights = {'%1': 40, '%2': 20, '%3': 19}
    assert ns['_gutter_neighbor'](geo, heights, '%1', 'v') == '%2'      # el que más se traslapa
    assert ns['_gutter_neighbor'](geo, heights, '%2', 'h') == '%3'
    assert ns['_gutter_neighbor'](geo, heights, '%3', 'h') is None
    assert ns['_gutter_target'](geo, heights, '%1', 'v', 80, '%2') == 80
    assert ns['_gutter_target'](geo, heights, '%1', 'v', 500, '%2') == 117   # el otro lado conserva 2
    assert ns['_gutter_target'](geo, heights, '%1', 'v', -4, '%2') == 2
    assert ns['_gutter_target'](geo, heights, '%2', 'h', 10, '%3') == 10
    assert ns['_gutter_half'](geo, heights, '%1', 'v', '%2') == 59
    assert ns['_gutter_half'](geo, heights, '%2', 'h', '%3') == 19


def test_the_drag_measure_reads_cells_on_both_sides():
    term = object()
    ns = {'_PANE_GEO': {id(term): {'%1': (0, 0, 100), '%2': (101, 0, 80)}}, '_PANE_BOX': {'%1': (30, True), '%2': (30, False)}}
    load('_gutter_neighbor', ns)
    measure = load('_gutter_measure', ns)
    box = SimpleNamespace(_term=term)
    assert measure(box, ("v", 0, 0, 8, 100, '%1')) == "100 | 80 col"
    assert measure(box, ("v", 0, 0, 8, 100, '%9')) == ""


def test_a_click_moves_the_frame_at_once_without_asking_tmux():
    term = SimpleNamespace(get_char_width=lambda: 10, get_char_height=lambda: 20)
    redrawn = []
    ns = {'_PANE_GEO': {id(term): {'%1': (0, 1, 60), '%2': (61, 1, 59)}},
          '_PANE_BOX': {'%1': (39, True), '%2': (39, False)},
          '_reposition_pills': lambda box, geo: redrawn.append(dict(ns['_PANE_BOX'])),
          'GLib': SimpleNamespace(idle_add=lambda fn: None)}
    load('_box_cell', ns)
    focus = load('_focus_frame_now', ns)
    box = SimpleNamespace(_term=term)
    focus(box, 700, 100)          # columna 70, fila 5: el pane de la derecha
    assert ns['_PANE_BOX'] == {'%1': (39, False), '%2': (39, True)} and len(redrawn) == 1
    focus(box, 700, 100)          # ya activo: no repinta
    assert len(redrawn) == 1


def test_the_active_frame_and_handle_take_the_theme_brand_and_win_over_vte():
    draw = SOURCE.split('def _draw_pane_frames(box, cr):')[1].split('\ndef ')[0]
    assert '#4ade80' not in draw and 'THEME.get("brand")' in draw
    attach = SOURCE.split('def _attach_model_bar(')[1].split('\ndef ')[0]
    # La VTE ya no ve los canales: ni pelea de cursores ni arrastre de tmux (1-oct).
    assert '_gutter_cursor' not in SOURCE and 'connect_after("motion-notify-event"' not in attach
    assert '_term_click(b, e)' in attach


def test_each_gutter_gets_its_own_input_strip_with_slack():
    """Parpadeo mano/flecha (1-oct): cada canal tiene su franja con cursor propio,
    GRIP_SLACK px a cada lado; si no cambió, no se recoloca."""
    ns = {'GRIP_SLACK': 8}
    rect = load('_grip_rect', ns)
    assert rect(("v", 100, 10, 8, 200, '%1')) == (92, 10, 24, 200)
    assert rect(("h", 0, 300, 400, 5, '%1')) == (0, 292, 400, 21)
    sync = SOURCE.split('def _sync_grips(box):')[1].split('\ndef ')[0]
    assert 'set_visible_window(False)' in sync and 'eb._rect != rect' in sync
    assert SOURCE.count('_sync_grips(box)') >= 2


def test_pane_header_matches_the_remote_and_prototype_b():
    """1-oct (celular/tablet = escritorio): la cabecera de pane del escritorio usaba el
    lila de _PV_HEX con un icono, un separador invisible (7 px en el color de línea) y
    los iconos «settings/sparkles». Ahora es la del prototipo B y la de term.html."""
    import re
    src = SOURCE
    term = Path("dash/term.html").read_text()
    logo_js = {k: (l, c) for k, l, c in re.findall(r"(\w+): \['([A-Z]+)', '(#[0-9a-f]{6})'\]", term)}
    ns = {}
    exec(src[src.index("MOTOR_LOGO = {"):src.index("\n\n\ndef _motor_badge")], ns)
    assert ns["MOTOR_LOGO"] == logo_js, "same letters and colours on desktop and remote"
    assert ns["MOTOR_LOGO"]["claude"] == ("A", "#d97757")
    for name, js in (("ia-gear", "gear"), ("ia-spark", "spark")):
        path = re.search(r'<path d="([^"]+)"', Path(f"dash/icons/{name}.svg").read_text()).group(1)
        assert path in term.split(f"{js}: '", 1)[1].split("'", 1)[0], f"{name}.svg = term.html ICON.{js}"
    assert "min-width:1px;min-height:14px;margin:0;" in src and "sep.set_valign(Gtk.Align.CENTER)" in src
    assert 'b"box.pane-card > separator,box.pane-card > separator:backdrop{' in src, "beats paned separator:backdrop"
