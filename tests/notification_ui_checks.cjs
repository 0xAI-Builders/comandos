#!/usr/bin/env node
// N2 notice UI contract without a browser: project grouping, one floating
// arrival at a time (auto-hide / persistent / burst), read vs. dismiss vs.
// pending, arrival that never steals focus or selection, vanished sessions,
// live-only sound claims and explicit-interaction presence. The server is a
// fake transport implementing /tmp/.../n2-contract.md; timers are manual.
'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');

const ROOT = path.resolve(__dirname, '..');
const N = require(path.join(ROOT, 'dash', 'notifications.js'));

const results = [];
async function check(name, fn) {
  try { await fn(); results.push({ name, ok: true }); } catch (e) { results.push({ name, ok: false, error: e.stack || e.message }); }
}

const DEFAULT_PREFS = {
  modes: { attention: 'sound', error: 'sound', focus: 'sound', done: 'visual', news: 'visual', usage: 'visual', info: 'visual' },
  volume: 0.6, muted: false, floatMs: 6000, burstMs: 10000,
};

let seq = 0;
function notice(over = {}) {
  seq += 1;
  const base = {
    eventId: 'ev-' + seq, sequence: seq, kind: 'turn_completed', category: 'done', needsHuman: false,
    projectKey: 'ComandOS', project: 'ComandOS', sessionKey: 'alpha', paneKey: 'pane-a', paneId: '%3',
    title: 'Terminó el turno', excerpt: 'Listo para revisar', occurredAtMs: 1000 + seq, read: false,
    float: { show: true, ms: 6000 }, group: 'ComandOS:turn_completed:' + seq,
  };
  return Object.assign(base, over);
}

function fakeServer() {
  const s = {
    notices: [], pending: [], prefs: JSON.parse(JSON.stringify(DEFAULT_PREFS)), focusActive: false,
    log: [], sound: { play: true, cue: 'permission' },
    async transport(method, url, body) {
      s.log.push([method, url, body === undefined ? undefined : JSON.parse(JSON.stringify(body))]);
      const [p, q] = url.split('?');
      if (method === 'GET' && p === '/notices') {
        const params = new URLSearchParams(q || '');
        const after = Number(params.get('after') || 0);
        const list = s.notices.filter(n => n.sequence > after).map(n => ({ ...n }));
        const nextAfter = list.reduce((m, n) => Math.max(m, n.sequence), after);
        return { notices: list, nextAfter, pending: [...s.pending], prefs: s.prefs, focusActive: s.focusActive };
      }
      if (method === 'POST' && p === '/notices/read') {
        const ids = body.all ? s.notices.filter(n => !body.project || n.project === body.project).map(n => n.eventId) : body.eventIds;
        for (const n of s.notices) if (ids.includes(n.eventId)) n.read = true;
        return { ok: true, read: ids };
      }
      if (method === 'POST' && p === '/notices/sound') return { ...s.sound };
      if (method === 'POST' && p === '/presence') return { ok: true };
      if (method === 'POST' && p === '/notices/prefs') { Object.assign(s.prefs, body); return s.prefs; }
      if (method === 'GET' && p === '/notices/prefs') return s.prefs;
      throw new Error('unexpected ' + method + ' ' + url);
    },
    calls(method, p) { return s.log.filter(([m, u]) => m === method && u.split('?')[0] === p); },
  };
  return s;
}

function fakeTimers() {
  let id = 0, now = 0;
  const pending = new Map();
  return {
    set(fn, ms) { id += 1; pending.set(id, { fn, at: now + ms }); return id; },
    clear(t) { pending.delete(t); },
    advance(ms) {
      now += ms;
      for (const [k, t] of [...pending].sort((a, b) => a[1].at - b[1].at)) if (t.at <= now) { pending.delete(k); t.fn(); }
    },
    get size() { return pending.size; },
    now: () => now,
  };
}

function fakeSounds() {
  return { ready: true, played: [], isReady() { return this.ready; }, play(cue, o) { this.played.push([cue, o]); return {}; } };
}

function harness(over = {}) {
  const server = fakeServer();
  const timers = fakeTimers();
  const sounds = fakeSounds();
  const live = new Set(['alpha', 'beta']);
  const opened = [];
  const env = { visible: true };
  const controller = N.createController({
    transport: server.transport, deviceId: 'web-test-device', now: () => 5000 + timers.now(),
    setTimer: timers.set, clearTimer: timers.clear, sounds,
    isVisible: () => env.visible, isSessionLive: s => live.has(s),
    openSource: n => opened.push(['source', n.eventId, n.sessionKey, n.paneKey]),
    openNews: n => opened.push(['news', n.eventId]),
    ...over,
  });
  return { server, timers, sounds, live, opened, env, c: controller };
}

// ---- tiny DOM double: only what mount() needs, with focus/selection spies ----
function fakeDom() {
  const focusCalls = [];
  function el(tag) {
    const e = {
      tagName: tag.toUpperCase(), children: [], parent: null, id: '', className: '', dataset: {}, attrs: {},
      _html: '', listeners: {}, hidden: false,
      get innerHTML() { return this._html; }, set innerHTML(v) { this._html = String(v); this.children = []; },
      appendChild(c) { c.parent = this; this.children.push(c); return c; },
      contains(o) { for (let x = o; x; x = x.parent) if (x === this) return true; return false; },
      addEventListener(t, fn) { (this.listeners[t] ||= []).push(fn); },
      removeEventListener() {},
      setAttribute(k, v) { this.attrs[k] = String(v); }, getAttribute(k) { return this.attrs[k] ?? null; },
      querySelector() { return null; }, querySelectorAll() { return []; },
      classList: { set: new Set(), add(c) { this.set.add(c); }, remove(c) { this.set.delete(c); }, toggle(c, on) { if (on === undefined ? !this.set.has(c) : on) this.set.add(c); else this.set.delete(c); }, contains(c) { return this.set.has(c); } },
      focus() { focusCalls.push(this); doc.activeElement = this; },
      scrollIntoView() {},
    };
    return e;
  }
  const doc = {
    visibilityState: 'visible', body: el('body'), activeElement: null, listeners: {},
    createElement: el,
    getElementById(id) { return this.byId[id] || null; }, byId: {},
    addEventListener(t, fn) { (this.listeners[t] ||= []).push(fn); }, removeEventListener() {},
    fire(t, e) { for (const fn of this.listeners[t] || []) fn(e); },
  };
  doc.activeElement = doc.body;
  return { doc, el, focusCalls };
}

(async () => {
  await check('groups by project; news stays in its own group, never an invented project', () => {
    const list = [
      notice({ project: 'ComandOS', projectKey: 'ComandOS' }),
      notice({ project: 'Lola', projectKey: 'Lola', kind: 'permission_requested', category: 'attention', needsHuman: true }),
      notice({ project: null, projectKey: null, kind: 'news_edition', category: 'news', sessionKey: null, paneKey: null, paneId: null }),
      notice({ project: 'ComandOS', projectKey: 'ComandOS', kind: 'news_edition', category: 'news' }),
      notice({ project: 'ComandOS', projectKey: 'ComandOS', kind: 'turn_failed', category: 'error' }),
    ];
    const groups = N.groupNotices(list, { filter: 'all', pending: new Set([list[1].eventId]) });
    const byKey = Object.fromEntries(groups.map(g => [g.key, g]));
    assert.deepEqual(Object.keys(byKey).sort(), ['ComandOS', 'Lola', N.GROUP_NEWS].sort());
    assert.equal(byKey[N.GROUP_NEWS].label, 'Noticias');
    assert.equal(byKey[N.GROUP_NEWS].notices.length, 2, 'both news notices, even one tagged with a project');
    assert.equal(byKey.ComandOS.notices.length, 2);
    assert.ok(byKey.ComandOS.notices.every(n => n.category !== 'news'));
    assert.equal(byKey.Lola.pendingCount, 1);
    assert.equal(groups[groups.length - 1].key, N.GROUP_NEWS, 'news after the projects');
    const html = N.renderStrip({ notices: list, pending: new Set([list[1].eventId]), filter: 'all', collapsed: false, now: 9999 });
    assert.match(html, /data-nt-group="ComandOS"/);
    assert.match(html, /data-nt-group="__news__"/);
    assert.ok(!/data-nt-group="null"/.test(html));
  });

  await check('type filters narrow each group; pending filter uses server pending list', () => {
    const a = notice({ kind: 'permission_requested', category: 'attention', needsHuman: true });
    const b = notice({ kind: 'turn_failed', category: 'error' });
    const c = notice({ category: 'done' });
    const pending = new Set([a.eventId]);
    assert.deepEqual(N.groupNotices([a, b, c], { filter: 'pending', pending }).flatMap(g => g.notices).map(n => n.eventId), [a.eventId]);
    assert.deepEqual(N.groupNotices([a, b, c], { filter: 'error', pending }).flatMap(g => g.notices).map(n => n.eventId), [b.eventId]);
    c.read = true;
    assert.deepEqual(N.groupNotices([a, b, c], { filter: 'unread', pending }).flatMap(g => g.notices).map(n => n.eventId).sort(), [a.eventId, b.eventId].sort());
  });

  await check('history on first load never floats and never claims sound', async () => {
    const h = harness();
    h.server.notices.push(notice({ category: 'attention', kind: 'permission_requested', needsHuman: true, float: { show: true, ms: null } }), notice());
    await h.c.poll();
    assert.equal(h.c.state.notices.length, 2);
    assert.equal(h.c.state.float, null);
    assert.equal(h.server.calls('POST', '/notices/sound').length, 0);
    assert.equal(h.sounds.played.length, 0);
  });

  await check('one float at a time: newest replaces a transient one; both stay in the strip', async () => {
    const h = harness();
    await h.c.poll();
    h.server.notices.push(notice({ sessionKey: 'alpha', group: 'g1' }));
    await h.c.poll();
    const first = h.c.state.float.eventIds[0];
    h.server.notices.push(notice({ project: 'Lola', projectKey: 'Lola', sessionKey: 'beta', group: 'g2' }));
    await h.c.poll();
    assert.equal(h.c.state.float.eventIds.length, 1);
    assert.notEqual(h.c.state.float.eventIds[0], first);
    assert.equal(h.c.state.notices.length, 2);
    const html = N.renderFloat(h.c.view());
    assert.equal((html.match(/class="nt-float /g) || []).length, 1);
    assert.match(html, /Lola/);
  });

  await check('numeric float.ms hides itself; ms:null persists until opened or closed', async () => {
    const h = harness();
    await h.c.poll();
    h.server.notices.push(notice({ float: { show: true, ms: 6000 } }));
    await h.c.poll();
    assert.ok(h.c.state.float);
    h.timers.advance(5999);
    assert.ok(h.c.state.float, 'still visible before ms');
    h.timers.advance(1);
    assert.equal(h.c.state.float, null, 'hidden after ms');
    assert.equal(h.c.state.notices[0].read, false, 'auto-hide does not read');

    h.server.notices.push(notice({ category: 'attention', kind: 'permission_requested', needsHuman: true, float: { show: true, ms: null } }));
    await h.c.poll();
    h.timers.advance(10 * 60 * 1000);
    assert.ok(h.c.state.float, 'persistent float survives any time');
    h.server.notices.push(notice({ float: { show: true, ms: 6000 }, group: 'other' }));
    await h.c.poll();
    assert.equal(h.c.state.float.persistent, true, 'a transient arrival does not bury a pending request');
    h.c.dismissFloat();
    assert.equal(h.c.state.float, null);
  });

  await check('a catch-up (full page after reconnection) is history: no float, no sound', async () => {
    const h = harness();
    await h.c.poll();
    for (let i = 0; i < 60; i++) h.server.notices.push(notice({ category: 'attention', kind: 'permission_requested', needsHuman: true, float: { show: true, ms: null } }));
    await h.c.poll();
    assert.equal(h.c.state.notices.length, 60, 'all kept in the strip');
    assert.equal(h.c.state.float, null);
    assert.equal(h.server.calls('POST', '/notices/sound').length, 0);
    const q = new URLSearchParams(h.server.calls('GET', '/notices')[0][1].split('?')[1]);
    assert.equal(q.get('deviceId'), 'web-test-device');
    assert.equal(q.get('after'), '0');
  });

  await check('float.show=false or already-read arrivals do not float', async () => {
    const h = harness();
    await h.c.poll();
    h.server.notices.push(notice({ float: { show: false, ms: null } }), notice({ read: true }));
    await h.c.poll();
    assert.equal(h.c.state.float, null);
    assert.equal(h.c.state.notices.length, 2);
  });

  await check('bursts sharing a group combine into one compact float; each event stays in the strip', async () => {
    const h = harness();
    await h.c.poll();
    const g = 'ComandOS:turn_completed:bucket-7';
    h.server.notices.push(notice({ group: g }), notice({ group: g }));
    await h.c.poll();
    h.server.notices.push(notice({ group: g }));
    await h.c.poll();
    assert.equal(h.c.state.float.eventIds.length, 3);
    const html = N.renderFloat(h.c.view());
    assert.match(html, /3 turnos terminados en ComandOS/);
    assert.equal(N.groupNotices(h.c.state.notices, { filter: 'all', pending: new Set() })[0].notices.length, 3);
    h.timers.advance(6000);
    assert.equal(h.c.state.float, null, 'combined transient float still auto-hides');
  });

  await check('closing the float is not reading it', async () => {
    const h = harness();
    await h.c.poll();
    h.server.notices.push(notice());
    await h.c.poll();
    h.c.dismissFloat();
    assert.equal(h.c.state.float, null);
    assert.equal(h.c.state.notices[0].read, false);
    assert.equal(h.server.calls('POST', '/notices/read').length, 0);
    assert.equal(h.c.unreadCount(), 1);
  });

  await check('mark all read leaves pending requests pending (answered in the terminal)', async () => {
    const h = harness();
    const req = notice({ kind: 'permission_requested', category: 'attention', needsHuman: true });
    h.server.notices.push(req, notice());
    h.server.pending = [req.eventId];
    await h.c.poll();
    await h.c.markAllRead();
    const call = h.server.calls('POST', '/notices/read')[0];
    assert.deepEqual(call[2], { all: true, project: null });
    assert.equal(h.c.unreadCount(), 0);
    assert.equal(h.c.pendingCount(), 1);
    assert.ok(h.c.state.pending.has(req.eventId));
    const html = N.renderStrip(h.c.view());
    assert.match(html, /Pendiente/);
    assert.ok(!/Autorizar|Permitir|Allow/.test(html), 'no authorize buttons in notices');
    await h.c.poll();
    assert.equal(h.c.pendingCount(), 1, 'server still reports it pending');
  });

  await check('arrival never changes tab, focus, draft or text selection', async () => {
    const dom = fakeDom();
    const input = dom.el('textarea');
    input.value = 'borrador sin enviar'; input.selectionStart = 3; input.selectionEnd = 9;
    dom.doc.body.appendChild(input);
    dom.doc.activeElement = input;
    const host = dom.el('div');
    dom.doc.body.appendChild(host);
    const server = fakeServer();
    const timers = fakeTimers();
    const opened = [];
    const shown = [];
    const inst = N.mount({
      doc: dom.doc, host, transport: server.transport, deviceId: 'web-dom', sync: true,
      setTimer: timers.set, clearTimer: timers.clear, setInterval: () => 0, clearInterval() {},
      sounds: fakeSounds(), isVisible: () => true, isSessionLive: () => true,
      openSource: n => opened.push(n.eventId), openNews: () => shown.push('news'), storage: null, presence: false,
    });
    await inst.controller.poll();
    server.notices.push(notice({ category: 'attention', kind: 'permission_requested', needsHuman: true, float: { show: true, ms: null } }));
    for (let i = 0; i < 5; i++) server.notices.push(notice({ group: 'burst' }));
    await inst.controller.poll();
    assert.ok(inst.controller.state.float, 'float rendered');
    assert.match(inst.floatEl.innerHTML, /nt-float/);
    assert.match(inst.stripEl.innerHTML, /nt-row/);
    assert.equal(dom.doc.activeElement, input, 'focus kept');
    assert.equal(input.value, 'borrador sin enviar');
    assert.equal(input.selectionStart, 3); assert.equal(input.selectionEnd, 9);
    assert.equal(dom.focusCalls.length, 0, 'no focus() calls');
    assert.deepEqual(opened, [], 'no navigation on arrival');
    assert.deepEqual(shown, []);
  });

  await check('open goes to the exact origin; a vanished session shows the notice without creating a terminal', async () => {
    const h = harness();
    const here = notice({ sessionKey: 'alpha', paneKey: 'pane-7', paneId: '%7' });
    const gone = notice({ sessionKey: 'zeta', paneKey: 'pane-9', paneId: '%9' });
    h.server.notices.push(here, gone);
    await h.c.poll();
    await h.c.open(here.eventId);
    assert.deepEqual(h.opened, [['source', here.eventId, 'alpha', 'pane-7']]);
    assert.equal(h.c.state.unavailable, null);
    await h.c.open(gone.eventId);
    assert.equal(h.opened.length, 1, 'no terminal created for a vanished session');
    assert.equal(h.c.state.unavailable, gone.eventId);
    const html = N.renderStrip(h.c.view());
    assert.match(html, /El panel ya no existe/);
    assert.ok(h.c.state.notices.some(n => n.eventId === gone.eventId), 'event kept');
    const floatHtml = N.renderFloat({ ...h.c.view(), float: { eventIds: [gone.eventId], persistent: true } });
    assert.match(floatHtml, /El panel ya no existe/);
  });

  await check('news notices open the reader, not a terminal', async () => {
    const h = harness();
    const n = notice({ kind: 'news_edition', category: 'news', project: null, projectKey: null, sessionKey: null, paneKey: null, paneId: null });
    h.server.notices.push(n);
    await h.c.poll();
    await h.c.open(n.eventId);
    assert.deepEqual(h.opened, [['news', n.eventId]]);
    assert.equal(h.c.state.unavailable, null);
  });

  await check('open-event routing (push) opens a known notice once loaded', async () => {
    const h = harness();
    const n = notice({ sessionKey: 'beta' });
    h.server.notices.push(n);
    h.c.requestOpen(n.eventId);
    assert.equal(h.opened.length, 0);
    await h.c.poll();
    assert.deepEqual(h.opened[0], ['source', n.eventId, 'beta', n.paneKey]);
  });

  await check('sound: only live arrivals, only when the server says play, one claim per batch', async () => {
    const h = harness();
    h.server.notices.push(notice({ category: 'attention', kind: 'permission_requested', needsHuman: true }));
    await h.c.poll();
    assert.equal(h.server.calls('POST', '/notices/sound').length, 0, 'history');

    const live1 = notice({ category: 'attention', kind: 'permission_requested', needsHuman: true });
    h.server.notices.push(live1);
    h.server.sound = { play: true, cue: 'permission' };
    await h.c.poll();
    const claims = h.server.calls('POST', '/notices/sound');
    assert.equal(claims.length, 1);
    assert.deepEqual(claims[0][2], { eventId: live1.eventId, deviceId: 'web-test-device' });
    assert.deepEqual(h.sounds.played, [['permission', { eventId: live1.eventId, volume: 0.6 }]]);

    h.server.sound = { play: false, reason: 'other-device' };
    h.server.notices.push(notice({ category: 'error', kind: 'turn_failed' }));
    await h.c.poll();
    assert.equal(h.sounds.played.length, 1, 'play:false stays silent');

    const before = h.server.calls('POST', '/notices/sound').length;
    h.server.notices.push(notice({ category: 'done' }));
    await h.c.poll();
    assert.equal(h.server.calls('POST', '/notices/sound').length, before, 'visual-only type: no claim');

    h.server.sound = { play: true, cue: 'error' };
    h.server.notices.push(notice({ category: 'error', kind: 'turn_failed' }), notice({ category: 'attention', kind: 'input_requested', needsHuman: true }), notice({ category: 'error', kind: 'turn_failed' }));
    await h.c.poll();
    const batch = h.server.calls('POST', '/notices/sound').slice(before);
    assert.equal(batch.length, 1, 'no overlapping cues in a burst');
    assert.equal(h.sounds.played.length, 2);
  });

  await check('sound: hidden page, muted prefs or locked audio never claim the receipt', async () => {
    for (const setup of [h => { h.env.visible = false; }, h => { h.server.prefs.muted = true; }, h => { h.sounds.ready = false; }]) {
      const h = harness();
      await h.c.poll();
      setup(h);
      await h.c.poll();
      h.server.notices.push(notice({ category: 'attention', kind: 'permission_requested', needsHuman: true }));
      await h.c.poll();
      assert.equal(h.server.calls('POST', '/notices/sound').length, 0);
      assert.equal(h.sounds.played.length, 0);
    }
  });

  await check('presence: interaction only from explicit trusted gestures, throttled; timers never claim it', async () => {
    const server = fakeServer();
    const dom = fakeDom();
    let now = 0;
    const intervals = [];
    const p = N.createPresence({
      transport: server.transport, deviceId: 'web-p', doc: dom.doc, win: dom.doc, now: () => now,
      setInterval: (fn, ms) => { intervals.push([fn, ms]); return intervals.length; }, clearInterval() {},
      canPlayAudio: () => true,
    });
    p.start();
    await Promise.resolve();
    const sent = () => server.calls('POST', '/presence').map(c => c[2]);
    assert.deepEqual(sent()[0], { deviceId: 'web-p', visible: true, canPlayAudio: true, interaction: false });
    assert.equal(intervals[0][1], 30000);
    intervals[0][0]();
    assert.equal(sent()[1].interaction, false, 'heartbeat is not interaction');
    dom.doc.fire('pointerdown', { isTrusted: false });
    assert.equal(sent().length, 2, 'synthetic event ignored');
    dom.doc.fire('pointerdown', { isTrusted: true });
    assert.equal(sent()[2].interaction, true);
    now += 4000;
    dom.doc.fire('keydown', { isTrusted: true });
    assert.equal(sent().length, 3, 'throttled within 5 s');
    now += 1000;
    dom.doc.fire('keydown', { isTrusted: true });
    assert.equal(sent()[3].interaction, true);
    dom.doc.visibilityState = 'hidden';
    dom.doc.fire('visibilitychange', {});
    assert.deepEqual(sent()[4], { deviceId: 'web-p', visible: false, canPlayAudio: true, interaction: false });
    intervals[0][0]();
    assert.equal(sent().length, 5, 'no heartbeat while hidden');

    const h = harness();
    await h.c.poll();
    h.server.notices.push(notice());
    await h.c.poll();
    assert.equal(h.server.calls('POST', '/presence').length, 0, 'polling never reports presence');
  });

  await check('prefs: per-type Visual / Visual + sonido, volume and mute go to /notices/prefs', async () => {
    const h = harness();
    await h.c.poll();
    await h.c.setPrefs({ modes: { ...h.c.state.prefs.modes, done: 'sound' } });
    await h.c.setPrefs({ volume: 0.3, muted: true });
    const posts = h.server.calls('POST', '/notices/prefs').map(c => c[2]);
    assert.equal(posts[0].modes.done, 'sound');
    assert.deepEqual(posts[1], { volume: 0.3, muted: true });
    assert.equal(h.c.state.prefs.muted, true);
    const html = N.renderSettings({ prefs: h.c.state.prefs, localSound: false });
    assert.match(html, /Visual \+ sonido/);
    assert.match(html, /data-nt-preview="permission"/);
    assert.match(html, /data-nt-mode="usage"/, 'usage type configurable');
  });

  await check('announcement goes to Noticias; usage_alert has its own filter', () => {
    const ann = notice({ kind: 'announcement', category: 'news', project: null, projectKey: null, sessionKey: null, paneKey: null, paneId: null });
    const use = notice({ kind: 'usage_alert', category: 'usage', project: null, projectKey: null, sessionKey: null, paneKey: null, paneId: null });
    const other = notice();
    const groups = N.groupNotices([ann, use, other], { filter: 'all', pending: new Set() });
    assert.deepEqual(groups.find(g => g.key === N.GROUP_NEWS).notices.map(n => n.eventId), [ann.eventId]);
    assert.ok(!groups.some(g => g.notices.includes(use) && g.key === N.GROUP_NEWS));
    assert.deepEqual(N.groupNotices([ann, use, other], { filter: 'usage', pending: new Set() }).flatMap(g => g.notices).map(n => n.eventId), [use.eventId]);
    assert.match(N.renderStrip({ notices: [use], pending: new Set(), filter: 'all', collapsed: false, now: 1 }), /data-nt-filter="usage"/);
  });

  console.log(JSON.stringify(results.filter(r => !r.ok), null, 2));
  const failed = results.filter(r => !r.ok).length;
  console.log(`${results.length - failed}/${results.length} notification UI checks passed`);
  process.exitCode = failed ? 1 : 0;
})().catch(e => { console.error(e); process.exitCode = 2; });
