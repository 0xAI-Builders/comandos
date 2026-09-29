#!/usr/bin/env node
// Pomodoro UI contract without a browser: sound adapter lifecycle (fake audio
// engine, no device), art catalog integrity against the vendored manifest and
// the view's command discipline (fake transport, fixed clock).
'use strict';
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const ROOT = path.resolve(__dirname, '..');
const SOUNDS = require(path.join(ROOT, 'dash', 'ui-sounds.js'));
const P = require(path.join(ROOT, 'dash', 'pomodoro.js'));
const MIN = 60000;
const results = [];
async function check(name, fn) {
  try { await fn(); results.push({ name, ok: true }); } catch (e) { results.push({ name, ok: false, error: e.message }); }
}

function memoryStorage() {
  const m = new Map();
  return { getItem: k => (m.has(k) ? m.get(k) : null), setItem: (k, v) => m.set(k, String(v)), map: m };
}

function fakeEngine(log) {
  return {
    cueNames: ['open', 'close', 'complete', 'success', 'level-up', 'notification', 'error', 'warning', 'loading'],
    createUISFX(opts) {
      log.push(['create', opts]);
      return {
        unlock: async () => { log.push(['unlock']); return true; },
        play: (cue, o) => { log.push(['play', cue, o]); return { stop() {} }; },
        stopAll: () => log.push(['stopAll']),
        setVolume: v => log.push(['volume', v]),
        destroy: async () => log.push(['destroy']),
      };
    },
  };
}

function fakeDoc() {
  const handlers = {};
  return {
    visibilityState: 'visible',
    addEventListener: (t, fn) => { (handlers[t] ||= []).push(fn); },
    removeEventListener() {},
    fire(t, e) { for (const fn of handlers[t] || []) fn(e); },
  };
}

async function readySounds(storage = memoryStorage(), log = []) {
  const doc = fakeDoc();
  const s = SOUNDS.createUISounds({ loadEngine: async () => fakeEngine(log), doc, win: { navigator: {} }, storage });
  await new Promise(r => setImmediate(r));
  return { s, doc, log, storage };
}

(async () => {
  await check('sound is opt-in: muted by default and silent without a trusted gesture', async () => {
    const { s, doc, log } = await readySounds();
    assert.equal(s.isEnabled(), false);
    assert.equal(s.play('focus-complete'), null);
    s.setEnabled(true);
    assert.equal(s.play('focus-complete'), null, 'no gesture yet');
    doc.fire('pointerdown', { isTrusted: false });
    await new Promise(r => setImmediate(r));
    assert.equal(s.play('focus-complete'), null, 'synthetic events never unlock audio');
    doc.fire('pointerdown', { isTrusted: true });
    await new Promise(r => setImmediate(r));
    assert.ok(s.play('focus-complete'));
    assert.deepEqual(log.filter(x => x[0] === 'play').map(x => x[1]), ['complete']);
  });

  await check('one playback per eventId, shared by tabs of the same browser', async () => {
    const storage = memoryStorage();
    const a = await readySounds(storage);
    const b = await readySounds(storage, a.log);
    for (const x of [a, b]) { x.s.setEnabled(true); x.doc.fire('keydown', { isTrusted: true }); }
    await new Promise(r => setImmediate(r));
    assert.ok(a.s.play('focus-complete', { eventId: 'pomodoro:blk:completed' }));
    assert.equal(a.s.play('focus-complete', { eventId: 'pomodoro:blk:completed' }), null);
    assert.equal(b.s.play('focus-complete', { eventId: 'pomodoro:blk:completed' }), null, 'second tab stays silent');
    assert.ok(b.s.play('focus-complete', { eventId: 'pomodoro:other:completed' }));
  });

  await check('no loops: loop cues are rejected and every play is one-shot', async () => {
    const { s, doc, log } = await readySounds();
    s.setEnabled(true); doc.fire('pointerdown', { isTrusted: true });
    await new Promise(r => setImmediate(r));
    assert.equal(s.play('loading'), null);
    assert.equal(s.register('spinner', 'processing'), false);
    assert.equal(s.register('ping', 'notification'), true);
    assert.ok(s.play('ping'));
    for (const [kind, , opts] of log) if (kind === 'play') assert.equal(opts.loop, false);
  });

  await check('hidden pages are silent and stop sound; volume is clamped', async () => {
    const { s, doc, log } = await readySounds();
    s.setEnabled(true); doc.fire('pointerdown', { isTrusted: true });
    await new Promise(r => setImmediate(r));
    assert.equal(s.setVolume(7), 1);
    s.play('attention', { volume: 0.5 });
    const last = log.filter(x => x[0] === 'play').pop();
    assert.ok(Math.abs(last[2].volume - 0.5 * SOUNDS.MAX_VOLUME) < 1e-9, 'per-call volume scales the user volume');
    doc.visibilityState = 'hidden'; doc.fire('visibilitychange');
    assert.ok(log.some(x => x[0] === 'stopAll'));
    assert.equal(s.play('attention'), null);
  });

  await check('preview works while muted only from a human gesture', async () => {
    const log = [];
    const doc = fakeDoc();
    const win = { navigator: { userActivation: { isActive: false } } };
    const s = SOUNDS.createUISounds({ loadEngine: async () => fakeEngine(log), doc, win, storage: memoryStorage() });
    await new Promise(r => setImmediate(r));
    assert.equal(s.preview('level-up'), null, 'no user activation');
    win.navigator.userActivation.isActive = true;
    assert.ok(s.preview('level-up'));
    assert.equal(s.isEnabled(), false, 'preview does not change the preference');
    s.stop();
    assert.ok(log.some(x => x[0] === 'stopAll'));
  });

  await check('six styles, Alquimia first; each role maps to a vendored, hash-verified original', async () => {
    assert.deepEqual(P.STYLE_ORDER, ['alchemy', 'arcade', 'shikashi', 'soul', 'garden', 'crystals']);
    assert.equal(P.DEFAULT_STYLE, 'alchemy');
    assert.equal(P.styleOf('nope'), 'alchemy');
    const manifest = JSON.parse(fs.readFileSync(path.join(ROOT, 'assets/pomodoro/manifest.json'), 'utf8'));
    const hashes = Object.fromEntries(manifest.files.map(f => [f.file, f.sha256]));
    for (const file of P.artFiles()) {
      const buf = fs.readFileSync(path.join(ROOT, 'assets/pomodoro', file));
      assert.equal(crypto.createHash('sha256').update(buf).digest('hex'), hashes[file], file);
    }
    for (const id of P.STYLE_ORDER) {
      const roles = Object.keys(P.STYLES[id].assets).sort();
      assert.deepEqual(roles, ['clock', 'crystal', 'first', 'hundred', 'level', 'streak'], id);
      assert.equal(P.STYLES[id].assets.clock.file, 'zoedoz/hourglass.png');
      assert.equal(P.STYLES[id].assets.clock.frames, 15);
    }
    assert.ok(fs.readFileSync(path.join(ROOT, 'assets/pomodoro/CREDITS.md'), 'utf8').includes('Zoedoz'));
  });

  await check('animated sheets play their real frame count (PNG width = frames x frame width)', async () => {
    for (const id of P.STYLE_ORDER) for (const a of Object.values(P.STYLES[id].assets)) {
      if (!a.width) continue;
      const png = fs.readFileSync(path.join(ROOT, 'assets/pomodoro', a.file));
      assert.equal(png.readUInt32BE(16), a.frames * a.width, a.file);
    }
  });

  // ---- view discipline ---------------------------------------------------
  function loadView(snapshot, played) {
    const posts = [];
    let now = snapshot.serverNowMs;
    const timers = [];
    const mk = () => {
      const classes = new Set();
      return {
        innerHTML: '', style: {}, dataset: {}, classList: { add: c => classes.add(c), remove: c => classes.delete(c), contains: c => classes.has(c), toggle: (c, v) => (v === undefined ? !classes.has(c) : v) ? classes.add(c) : classes.delete(c) },
        setAttribute() {}, addEventListener() {}, querySelectorAll: () => [], contains: () => false,
        children: {}, querySelector(sel) { return sel === '.pomo-mini' ? (this.children[sel] ||= { textContent: '' }) : null; },
        getBoundingClientRect: () => ({ height: 30, bottom: 40, left: 10 }),
      };
    };
    const panel = mk(); const button = mk(); const analytics = mk();
    const gets = [];
    const document = {
      readyState: 'complete', hidden: false, documentElement: mk(), activeElement: null,
      getElementById: id => (id === 'pomo-panel' ? panel : id === 'btn-pomo' ? button : id === 'pomodoro-analytics' ? analytics : null),
      addEventListener() {},
    };
    const state = { snapshot };
    const context = {
      console, document, URLSearchParams, Date: class extends Date { static now() { return now; } },
      setTimeout: (fn, ms) => { timers.push({ fn, ms }); return timers.length; }, clearTimeout() {}, setInterval() { return 0; },
      fetch: async (url, opt) => {
        const body = opt && opt.body ? JSON.parse(opt.body) : null;
        if (body) posts.push(body);
        if (body && body.action) {
          const res = { ok: true, revision: state.snapshot.revision + 1, serverNowMs: now, replayed: false, block: Object.assign({}, state.snapshot.block, { targetMs: state.snapshot.block.targetMs + (body.deltaMs || 0) }) };
          state.snapshot = res;
          return { status: 200, json: async () => res };
        }
        if (body && body.settings) return { status: 200, json: async () => ({ ok: true, settings: body.settings }) };
        if (!body) gets.push(url);
        if (String(url).startsWith('/pomodoro/report')) return { status: 200, json: async () => state.report };
        return { status: 200, json: async () => state.snapshot };
      },
      crypto: { randomUUID: () => 'req-' + posts.length },
      uiSounds: { play: (cue, o) => { played.push([cue, o && o.eventId]); return {}; }, isEnabled: () => false, getVolume: () => 0.6, setEnabled() {}, unlock() {}, preview() {}, setVolume() {} },
    };
    context.window = context; context.self = context;
    vm.createContext(context);
    vm.runInContext(fs.readFileSync(path.join(ROOT, 'dash', 'pomodoro.js'), 'utf8'), context);
    return { ui: context.ComandosPomodoro.ui, P: context.ComandosPomodoro, posts, gets, panel, button, analytics, timers, setNow: ms => { now = ms; }, state };
  }
  const settle = () => new Promise(r => setImmediate(r));
  const T0 = 2000000000000;
  const running = { revision: 3, serverNowMs: T0 + 5 * MIN, settings: { focusMinutes: 25, style: 'crystals' }, block: { blockId: 'b1', mode: 'focus', status: 'running', targetMs: 25 * MIN, activeMs: 0, resumedAtMs: T0, deadlineMs: T0 + 25 * MIN, project: 'ComandOS' } };

  await check('dragging the ruler while idle never starts a block', async () => {
    const played = [];
    const v = loadView({ revision: 0, serverNowMs: T0, block: null, settings: { focusMinutes: 25 } }, played);
    await settle();
    v.ui.setMinutes(40, false); v.ui.setMinutes(42, false); v.ui.setMinutes(45, true);
    for (const t of v.timers.splice(0)) t.fn();
    await settle();
    assert.equal(v.posts.filter(p => p.action).length, 0);
    assert.deepEqual(v.posts.filter(p => p.settings).pop(), { settings: { focusMinutes: 45, shortBreakMinutes: 5 } });
    assert.equal(v.ui.state.draft.focus, 45);
  });

  await check('ruler release while running sends one confirmed extend with the resulting remaining time', async () => {
    const v = loadView(JSON.parse(JSON.stringify(running)), []);
    await settle();
    v.ui.setMinutes(30, false);
    assert.equal(v.posts.filter(p => p.action).length, 0, 'preview does not send');
    v.ui.setMinutes(30, true);
    await settle();
    const cmd = v.posts.filter(p => p.action);
    assert.equal(cmd.length, 1);
    assert.equal(cmd[0].action, 'extend');
    assert.equal(cmd[0].deltaMs, 10 * MIN, '5 min spent + 30 remaining = 35 min target');
    assert.equal(cmd[0].expectedRevision, 3);
    assert.equal(v.ui.client.snapshot().settings.style, 'crystals', 'a command answer keeps settings/progress from the last GET');
  });

  await check('style comes from the global setting and renders the shared hourglass', async () => {
    const v = loadView(JSON.parse(JSON.stringify(running)), []);
    await settle();
    v.ui.render();
    assert.equal(v.ui.state.style, 'crystals');
    assert.ok(v.button.innerHTML.includes('zoedoz/hourglass.png'));
    assert.equal(v.button.querySelector('.pomo-mini').textContent, '20:00', 'header shows the server-based remaining time');
    const built = v.button.innerHTML;
    v.setNow(T0 + 6 * MIN); v.ui.render();
    assert.equal(v.button.querySelector('.pomo-mini').textContent, '19:00');
    assert.equal(v.button.innerHTML, built, 'the animated sprite is not rebuilt every tick');
  });

  await check('a completed block plays its cue once, with the server event id', async () => {
    const played = [];
    const done = { revision: 4, serverNowMs: T0 + 25 * MIN + 2000, settings: {}, block: Object.assign({}, running.block, { status: 'completed', activeMs: 25 * MIN, endedAtMs: T0 + 25 * MIN, deadlineMs: null, resumedAtMs: null }) };
    const v = loadView(done, played);
    await settle();
    v.ui.render(); v.ui.render();
    assert.deepEqual(played, [['focus-complete', 'pomodoro:b1:completed']]);
    assert.equal(v.ui.state.mode, 'break', 'suggests the break without starting it (manual cycles)');
    assert.equal(v.posts.filter(p => p.action).length, 0);
  });

  await check('an old completion after reload is not replayed', async () => {
    const played = [];
    const old = { revision: 4, serverNowMs: T0 + 5 * 3600000, settings: {}, block: Object.assign({}, running.block, { status: 'completed', endedAtMs: T0 + 25 * MIN }) };
    const v = loadView(old, played);
    await settle();
    v.ui.render();
    assert.deepEqual(played, []);
  });

  await check('a level-up completion plays one level-up cue instead of the completion cue', async () => {
    const played = [];
    const endedAtMs = T0 + 25 * MIN;
    const snap = { revision: 9, serverNowMs: endedAtMs + 3000, settings: {},
      progress: { policyVersion: 'v1', xpPerMinute: 10, xpPerLevel: 1000, xp: 1000, level: 2, levelPct: 0, xpToNextLevel: 1000, todayMinutes: 100, dailyGoalMinutes: 100, achievements: [], lastLevelUp: { blockId: 'b1', level: 2, awardedAtMs: endedAtMs } },
      block: Object.assign({}, running.block, { status: 'completed', activeMs: 25 * MIN, endedAtMs, deadlineMs: null, resumedAtMs: null }) };
    const v = loadView(snap, played);
    await settle();
    v.panel.classList.remove('hidden');
    v.ui.render(); v.ui.render();
    assert.deepEqual(played, [['level-up', 'pomodoro:level:v1:2']]);
    assert.ok(v.ui.state.banner.includes('+250 XP') && v.ui.state.banner.includes('Nivel 2'));
    assert.ok(v.panel.innerHTML.includes('100 / 100 min'), 'daily goal from the server progress');
  });

  await check('analytics filters query the report and totals come from its records', async () => {
    const v = loadView(JSON.parse(JSON.stringify(running)), []);
    await settle();
    v.state.report = { range: {}, projects: ['ComandOS', 'Lola'], byProject: [], history: [], byDay: [{ date: '2033-05-18', activeMs: 37 * MIN, completed: 1, cancelled: 1, legacyPlannedMs: 0 }],
      measured: { completed: { blocks: 1, activeMs: 25 * MIN, plannedMs: 25 * MIN }, cancelled: { blocks: 1, activeMs: 12 * MIN, plannedMs: 50 * MIN }, activeMs: 37 * MIN, plannedMs: 75 * MIN, completionRate: 0.5 },
      legacy: { blocks: 0, completedBlocks: 0, plannedMs: 0, note: '' }, breaks: { blocks: 0, activeMs: 0 } };
    v.P.analytics.project = 'Lola';
    v.P.analytics.period = '1';
    await v.P.loadAnalytics();
    const url = v.gets.filter(u => u.startsWith('/pomodoro/report')).pop();
    const q = new URLSearchParams(url.split('?')[1]);
    assert.equal(q.get('project'), 'Lola');
    assert.equal(q.get('fromDate'), q.get('toDate'), 'Hoy = one Mexico City day');
    assert.match(q.get('fromDate'), /^\d{4}-\d{2}-\d{2}$/);
    assert.ok(v.analytics.innerHTML.includes('37′'), 'measured minutes from the report');
    assert.ok(v.analytics.innerHTML.includes('75′'), 'planned minutes shown separately');
    assert.ok(!v.analytics.innerHTML.includes('20:00'), 'not the visual clock');
  });

  console.log(JSON.stringify(results, null, 2));
  const failed = results.filter(r => !r.ok).length;
  console.log(`${results.length - failed}/${results.length} pomodoro UI checks passed`);
  process.exitCode = failed ? 1 : 0;
})().catch(e => { console.error(e); process.exitCode = 2; });
