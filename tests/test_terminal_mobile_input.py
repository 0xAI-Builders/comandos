"""Byte and event tests for native mobile drafting; no live panes or services."""
import json
import subprocess
from pathlib import Path

from test_remote_ui import extract_js_function


SOURCE = Path('dash/term.html').read_text()


def run_js(names, body):
    code = '\n'.join(extract_js_function(SOURCE, name) for name in names)
    return json.loads(subprocess.check_output(['node', '-e', code + '\n' + body], text=True))


def test_native_ime_changes_have_one_explicit_producer_and_preserve_repetition():
    result = run_js(['createMobileDraftController', 'terminalPastePayload'], r'''
let value = '', online = true;
const bytes = [], saved = [];
const draft = createMobileDraftController({
  read: () => value, write: text => {value = text;},
  changed: text => saved.push(text),
  send: (text, enter) => {
    if (!online) return false;
    bytes.push(Buffer.from(terminalPastePayload(text, true, enter)).toString('hex'));
    return true;
  },
});
// Samsung/SwiftKey-style replacement, composition-end then final input.
draft.compositionStart();
value = 'hol'; draft.input();
value = 'hola'; draft.input();
const composingSubmit = draft.submit(true);
draft.compositionEnd();
value = 'hola hola 😃\n終'; draft.input();
const beforeSend = [...bytes];
const sent = draft.submit(true);
// Same committed text again is intentional; never deduplicate by content.
value = 'hola hola 😃\n終'; draft.input(); draft.submit(true);
// Browser-owned selection deletion, autocorrection and dictation.
value = 'teh teh'; draft.input();
value = 'the the'; draft.input();
value = 'the'; draft.input();
value = 'the dictation'; draft.input();
online = false;
const offline = draft.submit(false), retained = value;
online = true;
const retry = draft.submit(false);
console.log(JSON.stringify({composingSubmit,beforeSend,sent,bytes,offline,retained,retry,value,saved}));
''')
    payload = '\x1b[200~hola hola 😃\r終\x1b[201~\r'.encode().hex()
    assert result['composingSubmit'] is False
    assert result['beforeSend'] == []
    assert result['bytes'] == [payload, payload, '\x1b[200~the dictation\x1b[201~'.encode().hex()]
    assert result['offline'] is False
    assert result['retained'] == 'the dictation'
    assert result['sent'] and result['retry']
    assert result['value'] == ''


def test_paste_keeps_terminal_line_endings_and_blocks_bracketed_paste_escape():
    result = run_js(['terminalPastePayload'], r'''
console.log(JSON.stringify([
 terminalPastePayload('one\r\ntwo\n\n', false, false),
 terminalPastePayload('a\x1b[201~b', true, false),
 terminalPastePayload('', false, true),
]));
''')
    assert result == ['one\rtwo\r\r', '\x1b[200~a[201~b\x1b[201~', '\r']


def test_history_snapshot_discards_late_pane_response_and_freezes_until_refresh():
    result = run_js(['createTerminalHistoryController'], r'''
(async () => {
const pending = [], rendered = [], statuses = [];
const history = createTerminalHistoryController({
  fetchSnapshot: request => new Promise(resolve => pending.push({request, resolve})),
  render: snapshot => rendered.push(snapshot),
  status: value => statuses.push(value),
});
const a = history.open({pane:'%1'});
const b = history.open({pane:'%2'});
pending[1].resolve({pane:'%2',text:'stable select me'}); await b;
pending[0].resolve({pane:'%1',text:'wrong pane'}); await a;
history.output(); history.output();
const stable = [...rendered];
const c = history.refresh();
history.close();
pending[2].resolve({pane:'%2',text:'after close'}); await c;
console.log(JSON.stringify({stable,rendered,statuses,request:pending[2].request,active:history.isOpen()}));
})();
''')
    assert result['stable'] == [{'pane': '%2', 'text': 'stable select me'}]
    assert result['rendered'] == result['stable']
    assert result['request'] == {'pane': '%2'}
    assert result['active'] is False
    assert result['statuses'].count('Hay salida nueva. Actualiza cuando termines de seleccionar.') == 1


def test_slow_scroll_transport_has_one_request_and_one_bounded_accumulator():
    result = run_js([], r'''
(async () => {
const arg = 'fixture', accessTokenParam = 'fixture-token';
let tmuxScrollLines = 0, tmuxScrollFrame = 0, tmuxScrollPoint = null;
let tmuxScrollChain = Promise.resolve();
const frames = [], pending = [], requests = [];
function requestAnimationFrame(callback) { frames.push(callback); return frames.length; }
const cellFromTouch = () => ({col:15,row:9});
const fetch = (url, options) => new Promise(resolve => {
 requests.push(JSON.parse(options.body)); pending.push(resolve);
});
// Evaluated functions close over these transport fixtures.
__SCROLL__
queueTmuxScroll(280, {}); frames.shift()(); await Promise.resolve();
for (let i = 0; i < 100; i++) {queueTmuxScroll(280, {}); if (frames.length) frames.shift()();}
const duringSlow = {requests:requests.length,accumulated:tmuxScrollLines};
pending.shift()({ok:true}); await tmuxScrollChain;
frames.shift()(); await Promise.resolve();
pending.shift()({ok:true}); await tmuxScrollChain;
console.log(JSON.stringify({duringSlow,requests,frames:frames.length}));
})();
'''.replace('__SCROLL__', '\n'.join(extract_js_function(SOURCE, name) for name in ['queueTmuxScroll', 'flushTmuxScroll'])))
    assert result['duringSlow'] == {'requests': 1, 'accumulated': 24}
    assert result['requests'] == [
        {'session': 'fixture', 'delta': 8, 'col': 14, 'row': 8},
        {'session': 'fixture', 'delta': 24, 'col': 14, 'row': 8},
    ]
    assert result['frames'] == 0


def test_keystrokes_typed_while_reconnecting_are_queued_bounded_and_flushed_on_open():
    result = run_js(['createInputQueue', 'inputFrame', 'sendInput', 'flushPendingInput'], r'''
const enc = new TextEncoder();
const errors = [];
function showErr(message) { errors.push(message); }
let clock = 1000;
const pendingInput = createInputQueue(8, 15000, () => clock);
let ws = {readyState: 3, sent: [], send(frame) { this.sent.push(Buffer.from(frame).toString()); }};
function socketIsOpen() { return ws.readyState === 1; }
const queued = sendInput('ab', {queue: true});
const mouse = sendInput('\x1b[<0;1;1M');           // clicks/pegados no se encolan
const overflow = sendInput('0123456789', {queue: true});
ws.readyState = 1;
flushPendingInput(ws);
const flushed = [...ws.sent];
ws.readyState = 3;
sendInput('old', {queue: true});
clock += 20000;
ws.readyState = 1;
flushPendingInput(ws);
console.log(JSON.stringify({queued, mouse, overflow, flushed, after: ws.sent.length, errors}));
''')
    assert result['queued'] is True
    assert result['mouse'] is False and result['overflow'] is False
    assert result['flushed'] == ['0ab']
    assert result['after'] == 1                   # lo viejo se descarta, no se envía
    assert len(result['errors']) >= 3


def test_ctrl_stays_armed_when_the_key_could_not_be_sent():
    result = run_js(['controlByte', 'setCtrlArmed', 'sendTerminalData'], r'''
let ctrlArmed = false, pasting = false, accept = false;
const sent = [];
const document = {querySelector() { return null; }};
function sendInput(data) { if (!accept) return false; sent.push(data); return true; }
setCtrlArmed(true);
sendTerminalData('c');
const stillArmed = ctrlArmed;
accept = true;
sendTerminalData('c');
console.log(JSON.stringify({stillArmed, armedAfter: ctrlArmed, sent}));
''')
    assert result == {'stillArmed': True, 'armedAfter': False, 'sent': ['\x03']}


def test_composer_keeps_draft_until_terminal_output_acknowledges_it():
    result = run_js(['createAckTracker', 'createMobileDraftController'], r'''
const timers = [];
const acks = createAckTracker(cb => { timers.push(cb); return timers.length; }, id => { timers[id - 1] = null; });
let value = '', failures = 0;
const socket = {};
const draft = createMobileDraftController({
  read: () => value, write: text => { value = text; }, changed: () => {},
  send: () => acks.wait(socket, 4000),
  failed: () => { failures += 1; },
});
(async () => {
  value = 'ls -la';
  draft.submit(true);
  const pendingValue = value;
  const doubleSubmit = draft.submit(true);
  acks.output(socket);
  await new Promise(r => setTimeout(r, 0));
  const ackedValue = value;
  value = 'rm -rf build';
  draft.submit(true);
  timers.filter(Boolean).forEach(cb => cb());      // sin salida: half-open
  await new Promise(r => setTimeout(r, 0));
  const timedOut = value;
  draft.submit(true);
  acks.closed(socket);
  await new Promise(r => setTimeout(r, 0));
  console.log(JSON.stringify({pendingValue, doubleSubmit, ackedValue, timedOut, closed: value, failures}));
})();
''')
    assert result['pendingValue'] == 'ls -la'
    assert result['doubleSubmit'] is False
    assert result['ackedValue'] == ''
    assert result['timedOut'] == 'rm -rf build' and result['closed'] == 'rm -rf build'
    assert result['failures'] == 2
