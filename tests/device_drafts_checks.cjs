// W4 client checks: drafts return to the composer only, reading anchors
// report a missing line instead of jumping elsewhere. No browser.
'use strict';
const assert = require('assert');
const path = require('path');
const D = require(path.join(__dirname, '..', 'dash', 'device-drafts.js'));

let passed = 0;
async function check(name, fn) {
  try { await fn(); passed++; } catch (e) { console.error('FAIL', name, '\n', e); process.exitCode = 1; }
}

function timers() {
  const q = new Map();
  let n = 0;
  return { schedule: (fn) => { q.set(++n, fn); return n; }, cancel: id => { q.delete(id); },
           run: () => { for (const [id, fn] of [...q]) { q.delete(id); fn(); } } };
}

function composer(initial = '') {
  const c = { value: initial, sel: null, ptyBytes: [] };
  return c;
}

(async () => {
  await check('restore writes the saved text into the composer and sends nothing to the PTY', async () => {
    const c = composer();
    const d = D.createDrafts({ key: 'tab:alpha', read: () => c.value, write: t => { c.value = t; },
      select: (a, b) => { c.sel = [a, b]; },
      load: async () => ({ drafts: { 'tab:alpha': { text: 'git commit -m "wip', selStart: 3, selEnd: 3 } } }),
      save: async () => {} });
    assert.equal(await d.restore(), 'restored');
    assert.equal(c.value, 'git commit -m "wip');
    assert.deepEqual(c.sel, [3, 3]);
    assert.deepEqual(c.ptyBytes, [], 'no byte reaches the terminal');
    assert.equal(typeof d.send, 'undefined', 'the module has no way to type into the terminal');
  });

  await check('text typed on this device before the answer is never overwritten', async () => {
    let release;
    const c = composer();
    const d = D.createDrafts({ key: 'k', read: () => c.value, write: t => { c.value = t; },
      load: () => new Promise(r => { release = r; }), save: async () => {} });
    const p = d.restore();
    c.value = 'nuevo';
    release({ drafts: { k: { text: 'viejo' } } });
    assert.equal(await p, 'none');
    assert.equal(c.value, 'nuevo');
  });

  await check('changes save as debounced patches; the last one wins; empty removes', async () => {
    const t = timers(), saves = [];
    const d = D.createDrafts({ key: 'k', read: () => '', write() {}, load: async () => null,
      save: async p => saves.push(p), schedule: t.schedule, cancel: t.cancel, now: () => 7 });
    d.changed('h', 1, 1); d.changed('ho', 2, 2); d.changed('hola', 4, 4);
    t.run();
    await new Promise(r => setImmediate(r));
    assert.deepEqual(saves, [{ draftsPatch: { k: { text: 'hola', selStart: 4, selEnd: 4, updatedAt: 7 } } }]);
    d.changed('', 0, 0);
    t.run();
    await new Promise(r => setImmediate(r));
    assert.deepEqual(saves[1], { draftsPatch: { k: null } });
  });

  await check('a failed save retries on the next change', async () => {
    const t = timers(), saves = [];
    let fail = true;
    const d = D.createDrafts({ key: 'k', read: () => '', write() {}, load: async () => null,
      save: async p => { saves.push(p); if (fail) throw new Error('offline'); }, schedule: t.schedule, cancel: t.cancel });
    d.changed('hola'); t.run(); await new Promise(r => setImmediate(r));
    fail = false;
    d.changed('hola'); t.run(); await new Promise(r => setImmediate(r));
    assert.equal(saves.length, 2);
  });

  await check('reading anchor is found by its line text, or reported missing', async () => {
    const history = 'uno\ndos\ntres\ncuatro\n';
    const found = await D.createAnchor({ key: 'k', load: async () => ({ readingAnchors: { k: { text: 'tres' } } }), save() {} }).find(history);
    assert.deepEqual(found, { state: 'found', index: history.indexOf('tres') });
    const missing = await D.createAnchor({ key: 'k', load: async () => ({ readingAnchors: { k: { text: 'borrado' } } }), save() {} }).find(history);
    assert.equal(missing.state, 'missing');
    const none = await D.createAnchor({ key: 'k', load: async () => ({}), save() {} }).find(history);
    assert.equal(none.state, 'none');
  });

  await check('remembering the top line saves one debounced anchor patch', async () => {
    const t = timers(), saves = [];
    const a = D.createAnchor({ key: 'k', load: async () => null, save: async p => saves.push(p),
      schedule: t.schedule, cancel: t.cancel, now: () => 9 });
    const text = 'uno\ndos\ntres\n';
    a.remember(text, text.indexOf('dos') + 1, 0.4);
    a.remember(text, text.indexOf('tres'), 0.6);
    t.run();
    assert.deepEqual(saves, [{ anchorsPatch: { k: { text: 'tres', ratio: 0.6, updatedAt: 9 } } }]);
  });

  console.log(`${passed}/6 device draft checks passed`);
})();
