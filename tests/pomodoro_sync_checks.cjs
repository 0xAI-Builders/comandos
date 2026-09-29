#!/usr/bin/env node
// Two Pomodoro clients against the REAL lib/pomodoro.py authority (temporary
// database, fixed clock, in-process bridge). No browser, live API, timer,
// notification service or user data is touched. Exit 0 = all checks passed.
'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');
const readline = require('node:readline');

const ROOT = path.resolve(__dirname, '..');
const P = require(path.join(ROOT, 'dash', 'pomodoro.js'));
const MIN = 60000;
const T0 = 2000000000000;
const PY = fs.existsSync('/tmp/comandos-v1-qa/bin/python') ? '/tmp/comandos-v1-qa/bin/python' : 'python3';

const BRIDGE = `
import json, sys
sys.path.insert(0, ${JSON.stringify(path.join(ROOT, 'lib'))})
import app_state, pomodoro
now = [${T0}]
events = []
conn = app_state.connect(sys.argv[1]); app_state.migrate(conn)
store = pomodoro.PomodoroStore(conn, lambda: now[0], emit=lambda c, e: events.append(e))
for line in sys.stdin:
    msg = json.loads(line)
    op = msg["op"]
    try:
        if op == "clock": now[0] = msg["now"]; out = {"status": 200, "body": {}}
        elif op == "settle": out = {"status": 200, "body": {"done": store.settle_due()}}
        elif op == "events": out = {"status": 200, "body": {"events": events}}
        elif op == "GET": out = {"status": 200, "body": store.snapshot()}
        elif op == "POST": out = {"status": 200, "body": store.command(msg["body"])}
    except pomodoro.PomodoroError as exc:
        out = {"status": exc.status, "body": exc.payload()}
    sys.stdout.write(json.dumps(out) + "\\n"); sys.stdout.flush()
`;

function startBridge() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pomodoro-sync-'));
  const child = spawn(PY, ['-c', BRIDGE, path.join(dir, 'state.sqlite3')], { stdio: ['pipe', 'pipe', 'inherit'] });
  const rl = readline.createInterface({ input: child.stdout });
  const waiting = [];
  rl.on('line', line => waiting.shift()(JSON.parse(line)));
  const call = msg => new Promise(resolve => { waiting.push(resolve); child.stdin.write(JSON.stringify(msg) + '\n'); });
  return { call, close: () => { child.stdin.end(); fs.rmSync(dir, { recursive: true, force: true }); } };
}

const results = [];
async function check(name, fn) {
  try { await fn(); results.push({ name, ok: true }); }
  catch (e) { results.push({ name, ok: false, error: e.message }); }
}

(async () => {
  const bridge = startBridge();
  let serverNow = T0;
  const setServer = async ms => { serverNow = ms; await bridge.call({ op: 'clock', now: ms }); };
  const posts = [];
  function makeClient(name, skewMs = 0) {
    let offline = false;
    const localClock = { now: () => serverNow + skewMs };
    const transport = async (method, p, body) => {
      if (offline) throw new Error('offline');
      if (method === 'POST') posts.push({ client: name, body: structuredClone(body) });
      return bridge.call(method === 'GET' ? { op: 'GET' } : { op: 'POST', body });
    };
    const client = P.createClient({ transport, now: localClock.now });
    return { client, setOffline: v => { offline = v; } };
  }
  const desktop = makeClient('desktop');
  const remote = makeClient('remote', -93000);   // phone clock 93 s behind

  await check('start on one client appears with the same deadline on the other', async () => {
    const res = await desktop.client.send('start', { mode: 'focus', targetMs: 25 * MIN, project: 'Diagnostic', sessionKey: 'diagnostic-session', paneKey: '%999' });
    assert.equal(res.ok, true);
    await remote.client.refresh();
    const a = desktop.client.view(), b = remote.client.view();
    assert.equal(a.block.blockId, b.block.blockId);
    assert.equal(a.remainingMs, 25 * MIN);
    assert.equal(b.remainingMs, 25 * MIN, 'remote corrects its clock skew with serverNowMs');
  });

  await check('confirmed extension is shared by both clients and the API', async () => {
    await setServer(T0 + 2 * MIN);
    const before = posts.length;
    const res = await desktop.client.send('extend', { deltaMs: 5 * MIN });
    assert.equal(res.ok, true);
    assert.equal(posts.length - before, 1, 'extension is one API request');
    await remote.client.refresh();
    const api = (await bridge.call({ op: 'GET' })).body.block;
    assert.equal(api.deadlineMs, T0 + 30 * MIN);
    assert.equal(desktop.client.view().remainingMs, 28 * MIN);
    assert.equal(remote.client.view().remainingMs, 28 * MIN);
  });

  await check('pause and resume on the remote are reflected on the desktop', async () => {
    await setServer(T0 + 4 * MIN);
    await remote.client.refresh();
    assert.equal((await remote.client.send('pause')).ok, true);
    await setServer(T0 + 64 * MIN);   // a long pause
    await desktop.client.refresh();
    assert.equal(desktop.client.view().status, 'paused');
    assert.equal(desktop.client.view().remainingMs, 26 * MIN, 'paused time does not count');
    assert.equal((await desktop.client.send('resume')).ok, true);
    await remote.client.refresh();
    assert.equal(remote.client.view().status, 'running');
  });

  await check('a stale command is rejected and shows the confirmed server state', async () => {
    await setServer(T0 + 65 * MIN);
    await desktop.client.send('pause');                  // remote has not refreshed
    const res = await remote.client.send('extend', { deltaMs: MIN });
    assert.equal(res.ok, false);
    assert.equal(res.error.code, 'stale_revision');
    assert.equal(remote.client.view().status, 'paused', 'conflict body carries the confirmed state');
    assert.equal((await remote.client.send('resume')).ok, true);
  });

  await check('network failure keeps the last confirmed state and retries the same request', async () => {
    await setServer(T0 + 66 * MIN);
    await desktop.client.refresh();
    desktop.setOffline(true);
    await desktop.client.refresh();
    const before = desktop.client.view();
    const res = await desktop.client.send('extend', { deltaMs: 2 * MIN });
    assert.equal(res.ok, false);
    assert.equal(res.error.retryable, true);
    assert.equal(desktop.client.view().block.targetMs, before.block.targetMs, 'no local-only success');
    assert.equal(desktop.client.view().pending, 'extend');
    const busy = await desktop.client.send('cancel');
    assert.equal(busy.busy, true, 'a second command waits for the pending one');
    desktop.setOffline(false);
    const retried = await desktop.client.retry();
    assert.equal(retried.ok, true);
    assert.equal(retried.request.requestId, res.request.requestId, 'retry keeps requestId');
    assert.equal(desktop.client.view().block.targetMs, before.block.targetMs + 2 * MIN);
  });

  await check('an out-of-order older snapshot never overwrites a newer one', async () => {
    const snap = desktop.client.snapshot();
    const older = Object.assign({}, snap, { revision: snap.revision - 1, block: Object.assign({}, snap.block, { status: 'paused' }) });
    assert.equal(desktop.client.accept(older, serverNow), false);
    assert.equal(desktop.client.view().status, 'running');
  });

  await check('clients never finish a block; the backend completes it once for both', async () => {
    const block = desktop.client.view().block;
    await setServer(block.deadlineMs + 3000);         // pages suspended past the deadline
    assert.equal(desktop.client.view().due, true);
    assert.equal(desktop.client.view().remainingMs, 0);
    assert.ok(!posts.some(p => ['finish', 'complete'].includes(p.body.action) || p.body.stop), 'no client finish request');
    const settled = await bridge.call({ op: 'settle' });
    assert.deepEqual(settled.body.done, [block.blockId]);
    await desktop.client.refresh();
    await remote.client.refresh();
    assert.equal(desktop.client.view().status, 'completed');
    assert.equal(remote.client.view().status, 'completed');
    const again = await bridge.call({ op: 'settle' });
    assert.deepEqual(again.body.done, []);
    const events = (await bridge.call({ op: 'events' })).body.events;
    assert.equal(events.length, 1);
    assert.equal(events[0].kind, 'focus_completed');
  });

  await check('cancellation on one client stops the other', async () => {
    await setServer(serverNow + MIN);
    assert.equal((await remote.client.send('start', { mode: 'focus', targetMs: 25 * MIN })).ok, true);
    await desktop.client.refresh();
    assert.equal(desktop.client.view().status, 'running');
    await setServer(serverNow + 3 * MIN);
    assert.equal((await desktop.client.send('cancel')).ok, true);
    await remote.client.refresh();
    assert.equal(remote.client.view().live, false);
    assert.equal(remote.client.view().status, 'cancelled');
    assert.equal(remote.client.view().block.activeMs, 3 * MIN);
  });

  await check('ruler delta turns the remaining time into the chosen minutes', async () => {
    const block = { status: 'running', targetMs: 25 * MIN, activeMs: 0, resumedAtMs: 0 };
    assert.equal(P.deltaForRemaining(block, 10 * MIN, 30), 15 * MIN);
    assert.equal(P.deltaForRemaining(block, 10 * MIN, 5), -10 * MIN);
    assert.equal(P.deltaForRemaining(block, 10 * MIN, 500), 180 * MIN - 25 * MIN);
  });

  bridge.close();
  console.log(JSON.stringify(results, null, 2));
  const failed = results.filter(r => !r.ok).length;
  console.log(`${results.length - failed}/${results.length} pomodoro sync checks passed`);
  process.exitCode = failed ? 1 : 0;
})().catch(e => { console.error(e); process.exitCode = 2; });
