"""Web client of the quick terminal: one requestId per click until it succeeds."""
import subprocess
from pathlib import Path

SCRIPT = r'''
const assert = require('node:assert/strict');
const { createQuickTerminal } = require(process.cwd() + '/dash/quick-terminal.js');
(async () => {
  const calls = [], opened = [], toasts = [];
  let fail = 1, ids = 0, release;
  const store = new Map();
  const storage = { getItem: k => store.get(k) ?? null, setItem: (k, v) => store.set(k, v), removeItem: k => store.delete(k) };
  const api = (path, body) => { calls.push([path, body]);
    if (fail) { fail--; return Promise.reject(new Error('tmux: no server')); }
    return new Promise(r => { release = () => r({ tabId: 'term-q1', paneKey: 'pane-q1', cwd: '/b/T-1', label: 'T-1' }); }); };
  const qt = createQuickTerminal({ api, openTerm: (s, l) => opened.push([s, l]), toast: (m, e) => toasts.push([m, e]),
                                   storage, makeId: () => 'id-' + (++ids) });
  // a failure keeps the same requestId for the retry, also across a reload
  assert.equal(await qt.open(), null);
  assert.deepEqual(toasts, [['tmux: no server', true]]);
  assert.equal(qt.pendingRequestId, 'id-1');
  const reloaded = createQuickTerminal({ api, openTerm: (s, l) => opened.push([s, l]), storage, makeId: () => 'other' });
  assert.equal(reloaded.pendingRequestId, 'id-1');
  // a double click while the request is in flight sends one request
  const a = qt.open(), b = qt.open();
  assert.equal(a, b);
  assert.deepEqual(calls.map(c => c[1].requestId), ['id-1', 'id-1']);
  assert.equal(calls.every(c => c[0] === '/terminal/quick'), true);
  release(); await a;
  assert.deepEqual(opened, [['term-q1', 'T-1']]);
  assert.equal(qt.pendingRequestId, null);
  assert.equal(store.size, 0);
  // the next click is a new terminal
  const c = qt.open(); release(); await c;
  assert.equal(calls.at(-1)[1].requestId, 'id-2');
  // no cwd is ever sent: the server picks the dated folder
  assert.equal(calls.every(([, body]) => Object.keys(body).join() === 'requestId'), true);
  console.log('ok');
})().catch(e => { console.error(e); process.exit(1); });
'''


def test_quick_terminal_client_reuses_request_until_success():
    out = subprocess.run(["node", "-e", SCRIPT], capture_output=True, text=True, timeout=20)
    assert out.returncode == 0, out.stderr
    assert out.stdout.strip() == "ok"


def test_web_tab_bar_separates_terminal_from_new_session():
    html = Path("dash/index.html").read_text()
    bar = html.split('<nav id="app-navigation"', 1)[1].split("</nav>", 1)[0]
    assert 'id="tab-terminal"' in bar and 'id="tab-new"' in bar
    init = html.split("function initTabNavigation(){", 1)[1].split("\n}\n", 1)[0]
    assert '"/tab-new"' not in init                 # "+" no longer opens a scratch shell in ~
    assert "nsOpen()" in init                         # #btn-newsess salió del panel (S2); vuelve en H1
    assert "ComandosQuickTerminal.createQuickTerminal" in init
    assert '<script src="/quick-terminal.js"></script>' in html
