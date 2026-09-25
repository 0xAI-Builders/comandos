"""Exercise real desktop callbacks with widgets, HTTP and tmux isolated."""
import ast
from collections import Counter
import io
import json
from pathlib import Path
import sys
import threading
from types import SimpleNamespace as NS

SOURCE = Path('bin/cc-app').read_text()


def load(names, ns):
    nodes = [n for n in ast.parse(SOURCE).body
             if isinstance(n, ast.FunctionDef) and n.name in names]
    exec(compile(ast.Module(body=nodes, type_ignores=[]), '<cc-app>', 'exec'), ns)
    return ns


class Widget:
    def __init__(self, **kwargs):
        self.calls = Counter()
        self.children = []
        self.signals = {}
        self.text = ''
        self.width, self.height = 9, 20
        self.margin_start, self.margin_top = 8, 8

    def __getattr__(self, name):
        if name.startswith(('set_', 'show', 'hide', 'add_class', 'remove_class')):
            def call(*args):
                self.calls[name] += 1
            return call
        raise AttributeError(name)

    def set_text(self, text):
        self.text = text
        self.calls['set_text'] += 1

    def get_style_context(self): return self
    def get_pixbuf(self): return object()
    def get_char_width(self): return self.width
    def get_char_height(self): return self.height
    def get_margin_start(self): return self.margin_start
    def get_margin_top(self): return self.margin_top
    def get_allocated_width(self): return 1200
    def get_children(self): return self.children
    def connect(self, signal, callback): self.signals[signal] = callback
    def add(self, widget): self.children.append(widget)
    def pack_start(self, widget, *args): self.children.append(widget)
    def reorder_child(self, *args): pass
    def remove(self, widget):
        self.children.remove(widget)
        self.calls['remove'] += 1
    def add_overlay(self, widget):
        self.children.append(widget)
        self.calls['add_overlay'] += 1


def model_ui():
    counts = Counter()
    stamp = [1]
    tabs = {f'term-{i}': Widget() for i in range(19)}
    labels = {id(box): NS(_model=Widget(), _micon=Widget()) for box in tabs.values()}
    hub = Widget()
    labels[id(hub)] = NS(_micon=Widget())
    active = [next(iter(tabs.values()))]
    panes = [{'pane': '%0', 'harness': 'codex', 'motor': 'codex',
              'model': 'gpt-5.6', 'state': 'verified'}]
    models = {key: {'panes': [dict(p) for p in panes]} for key in tabs}
    geometry = {'%0': (0, 1, 120)}

    def svg(*args):
        counts['svg'] += 1
        return Widget()

    def get_geometry(session):
        counts['geometry'] += 1
        return dict(geometry)

    ns = {'os': NS(path=NS(getmtime=lambda _: stamp[0])),
          'open': lambda *a, **k: io.StringIO(json.dumps(models)),
          'APP_TAB_MODELS_FILE': 'unused', '_tab_models_mtime': 0,
          'json': json, 'tabs': tabs, 'hub': hub,
          'nb': NS(get_current_page=lambda: 0, get_nth_page=lambda _: active[0],
                   get_tab_label=lambda b: labels[id(b)]),
          '_svg_image': svg, '_pane_geometry': get_geometry,
          '_PANE_CMD': {'%0': 'codex'}, '_SHELLS': {'zsh'},
          'Gtk': NS(Box=Widget, Overlay=Widget, Label=Widget, Button=Widget,
                    Orientation=NS(HORIZONTAL=1), Align=NS(START=1), ReliefStyle=NS(NONE=1)),
          'THEME': {'fg': '#eee', 'dim': '#aaa', 'brand': '#abc'},
          '_PV_ICON': {'codex': 'openai'}, '_PV_HEX': {'codex': '#aaa'},
          '_STATE_UI': {'verified': ('green', 'v'), 'detecting': ('gray', '?'),
                        'changing': ('yellow', '>')}, '_esc': str, 'ES': False}
    load({'_refresh_tab_models', '_place_pills', '_pane_pill', '_shell_pill',
          '_pill_row_y', '_attach_model_bar', '_extension_pill'}, ns)
    for key, box in tabs.items():
        box._term = Widget()
        box.children = [box._term]
        ns['_attach_model_bar'](box, key)
    return NS(ns=ns, counts=counts, stamp=stamp, models=models, tabs=tabs,
              labels=labels, current=active[0], active=active, geometry=geometry,
              refresh=ns['_refresh_tab_models'])


def test_unchanged_models_and_identical_allocations_do_not_redraw():
    ui = model_ui()
    ui.refresh()
    assert ui.counts['svg'] == 21  # 19 tab icons and two icons in the active pill.
    for _ in range(9):
        ui.stamp[0] += 1  # cc-dash replaces the file even when its payload is unchanged.
        callback = ui.current._term.signals.get('size-allocate')
        if callback:
            callback(ui.current._term, NS(width=1200, height=800))
        ui.refresh()
    assert ui.counts['svg'] == 21
    assert ui.current._pill_overlay.calls['add_overlay'] == 1
    assert sum(label._model.calls['set_text'] for label in ui.labels.values()
               if hasattr(label, '_model')) == 19
    assert ui.counts['geometry'] == 10  # Keep querying real geometry, once per tick.


def test_live_model_status_geometry_padding_and_theme_still_update():
    ui = model_ui()
    ui.refresh()
    pill = ui.current._pills[0]
    ui.models['term-0']['panes'][0].update(model='new-model', state='changing', target='next')
    ui.refresh()
    assert ui.labels[id(ui.current)]._model.text == 'new-model'
    assert ui.current._pills[0] is not pill
    for change in (lambda: ui.geometry.update({'%0': (0, 20, 120)}),
                   lambda: setattr(ui.current._term, 'height', 24),
                   lambda: setattr(ui.current._term, 'margin_top', 16),
                   lambda: ui.ns['THEME'].update(fg='#fff', dim='#999'),
                   lambda: ui.ns['_PANE_CMD'].update({'%0': 'zsh'})):
        pill = ui.current._pills[0]
        change()
        ui.refresh()
        assert ui.current._pills[0] is not pill
    ui.active[0] = ui.tabs['term-1']
    ui.refresh()
    assert ui.active[0]._pills  # Newly visible tabs still get their overlays.


def test_dim_theme_change_updates_tab_icons_without_rebuilding_fixed_color_pills():
    ui = model_ui()
    ui.refresh()
    pill = ui.current._pills[0]
    ui.ns['THEME']['dim'] = '#777'
    ui.refresh()
    assert ui.counts['svg'] == 40  # 21 initially, then the 19 dim-colored tab icons.
    assert ui.current._pills[0] is pill  # _pane_pill uses a fixed #8A8F98 for dim.
    ui.refresh()
    assert ui.counts['svg'] == 40


def poll_ui(iterations=20, fail_notifications=False):
    pending, network, state_updates, favorites, themes, terminal_prefs = [], [], [], [], [], []
    ticks = [0]
    class StopPoll(BaseException): pass

    def sleep(_seconds):
        ticks[0] += 1
        if ticks[0] >= iterations:
            raise StopPoll()

    def urlopen(url, **kwargs):
        network.append((url, threading.get_ident()))
        if url.endswith('/state'):
            data = [{'session': 's', 'status': 'working', 'ts': ticks[0]}]
        elif url.endswith('/notifs/count'):
            if fail_notifications:
                raise TimeoutError('notification service unavailable')
            data = {'count': ticks[0]}
        else:
            data = {'favorites': [str(ticks[0])], 'theme': f'theme-{ticks[0]}', 'font_size': 12 + ticks[0]}
        return io.BytesIO(json.dumps(data).encode())

    ns = {'urllib': NS(request=NS(urlopen=urlopen)), 'json': json,
          'GLib': NS(idle_add=lambda *args: pending.append(args)),
          '_FAVORITE_GENERATION': 0, '_LIVE_PREF_KEYS': ('theme', 'font_size'),
          'update_dots': lambda *args: state_updates.append(args),
          'apply_tab_favorites': lambda *args: favorites.append(args),
          'apply_theme': lambda *args: themes.append(args),
          'apply_terminal_prefs': lambda *args: terminal_prefs.append(args),
          'time': NS(sleep=sleep), 'threading': threading, 'sys': sys,
          '_POLL_UI_LOCK': threading.Lock(), '_POLL_UI_PENDING': {}, '_POLL_UI_QUEUED': False}
    load({'poll_state_loop', '_queue_poll_update', '_drain_poll_updates'}, ns)

    def run():
        try: ns['poll_state_loop']()
        except StopPoll: pass

    worker = threading.Thread(target=run)
    worker.start()
    worker.join(timeout=5)
    assert not worker.is_alive()
    return NS(ns=ns, pending=pending, network=network, states=state_updates,
              favorites=favorites, themes=themes, terminal_prefs=terminal_prefs)


def test_poll_fetches_notifications_off_ui_thread_and_coalesces_latest_values():
    ui = poll_ui()
    assert len(ui.pending) == 1
    assert any(url.endswith('/notifs/count') for url, _ in ui.network)
    assert all(thread != threading.get_ident() for _, thread in ui.network)
    callback, *args = ui.pending.pop()
    assert callback(*args) is False
    assert ui.states == [({'s': 'working'}, [{'session': 's', 'status': 'working', 'ts': 19}], 19)]
    assert ui.favorites == [(['19'], 0)]
    assert ui.themes == [('theme-19',)]
    assert ui.terminal_prefs[0][0]['font_size'] == 31
    assert not ui.ns['_POLL_UI_PENDING']


def test_ui_updates_keep_fresh_state_without_network_or_unchanged_widget_writes():
    dot, badge = Widget(), Widget()
    box = Widget()
    network = []
    ns = {'STATE_CACHE': {}, 'STATE_ITEMS': {},
          'urllib': NS(request=NS(urlopen=lambda *a, **k: network.append(a))),
          'json': json, 'tabs': {'s': box}, 'nb': NS(get_tab_label=lambda _: NS(_dot=dot)),
          'DOT_COLORS': {'working': 'green', 'waiting': 'yellow'}, 'DOT_IDLE': 'gray',
          '_notif_badge': badge}
    load({'update_dots', 'set_notif_badge'}, ns)
    ns['update_dots']({'s': 'working'}, [{'session': 's', 'ts': 1}])
    ns['update_dots']({'s': 'working'}, [{'session': 's', 'ts': 2}])
    assert network == []
    assert ns['STATE_ITEMS']['s']['ts'] == 2
    assert dot.calls['set_markup'] == 1
    ns['update_dots']({'s': 'waiting'}, [])
    assert dot.calls['set_markup'] == 2
    assert ns['STATE_ITEMS'] == {}
    ns['set_notif_badge'](3)
    ns['set_notif_badge'](3)
    assert badge.calls['set_text'] == 1
    ns['set_notif_badge'](0)
    assert badge.calls['hide'] == 1


def test_coalesced_theme_runs_before_latest_terminal_preferences():
    ui = poll_ui(iterations=1)
    ui.pending.pop()[0]()
    applied = []
    theme = lambda value: applied.append(('theme', value))
    prefs = lambda value: applied.append(('prefs', value))
    # A font-only poll precedes a theme poll while GTK is busy.
    ui.ns['_queue_poll_update'](prefs, {'font_size': 15})
    ui.ns['_queue_poll_update'](theme, 'dia')
    ui.ns['_queue_poll_update'](prefs, {'font_size': 15, 'theme': 'dia', 'terminal_opacity': 50})
    assert len(ui.pending) == 1
    ui.pending.pop()[0]()
    assert [kind for kind, _ in applied] == ['theme', 'prefs']


def test_poll_updates_queued_during_dispatch_run_on_next_idle():
    ui = poll_ui(iterations=1)
    ui.pending.pop()[0]()
    applied = []
    def update(value):
        applied.append(value)
        if value == 1:
            ui.ns['_queue_poll_update'](update, 2)
            ui.ns['_queue_poll_update'](update, 3)
    ui.ns['_queue_poll_update'](update, 1)
    assert ui.pending.pop()[0]() is False
    assert applied == [1]
    assert len(ui.pending) == 1
    assert ui.pending.pop()[0]() is False
    assert applied == [1, 3]


def test_notification_failure_preserves_latest_state_and_preferences():
    ui = poll_ui(iterations=2, fail_notifications=True)
    assert len(ui.pending) == 1
    ui.pending.pop()[0]()
    assert ui.states[0][1][0]['ts'] == 1
    assert ui.states[0][2] == 0
    assert ui.favorites == [(['1'], 0)]
    assert ui.terminal_prefs[0][0]['font_size'] == 13


def test_failed_ui_callback_does_not_discard_other_updates_or_wedge_queue(capsys):
    ui = poll_ui(iterations=1)
    ui.pending.pop()[0]()
    applied = []
    def broken(_value):
        raise RuntimeError('widget destroyed')
    ui.ns['_queue_poll_update'](broken, 1)
    ui.ns['_queue_poll_update'](applied.append, 2)
    ui.pending.pop()[0]()
    assert applied == [2]
    assert 'widget destroyed' in capsys.readouterr().err
    ui.ns['_queue_poll_update'](applied.append, 3)
    assert len(ui.pending) == 1
    ui.pending.pop()[0]()
    assert applied == [2, 3]


def test_unchanged_favorites_skip_widget_updates_but_pending_changes_still_apply():
    button = Widget()
    ns = {'tabs': {'s': Widget()}, 'nb': NS(get_tab_label=lambda _: NS(_favorite=button)),
          'TAB_FAVORITES': {'s'}, '_FAVORITE_PENDING': {}, '_FAVORITE_GENERATION': 0,
          'ES': False, 'save_tabs': lambda: None}
    load({'refresh_favorite_buttons', 'apply_tab_favorites'}, ns)
    ns['apply_tab_favorites'](['s'], 0)
    initial = dict(button.calls)
    ns['apply_tab_favorites'](['s'], 0)
    assert dict(button.calls) == initial
    ns['_FAVORITE_PENDING']['s'] = False
    ns['apply_tab_favorites'](['s'], 0)
    assert ns['TAB_FAVORITES'] == set()
    assert button.calls['set_sensitive'] == 2
    ns['_FAVORITE_PENDING'].clear()
    ns['apply_tab_favorites']([], 0)
    assert button.calls['set_sensitive'] == 3
    ns['apply_tab_favorites'](['s'], -1)
    assert ns['TAB_FAVORITES'] == set()  # Stale favorite generations remain ignored.
