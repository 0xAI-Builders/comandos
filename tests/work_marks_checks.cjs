// Pure behaviour of the web work-marks indicator (no browser).
const assert = require('node:assert/strict');
const path = require('node:path');
const wm = require(path.join(__dirname, '..', 'dash/work-marks.js'));

// The human mark wins over activity; finishing a turn shows the neutral icon.
assert.deepEqual(wm.display('frozen', 'working'), {icon: 'frozen', label: 'Congelado', animated: true, mark: 'frozen'});
assert.equal(wm.display('none', 'working').icon, 'working');
assert.equal(wm.display('none', 'working').label, 'Trabajando');
for (const finished of ['completed', 'cancelled', 'failed', null, undefined]) {
  const d = wm.display('none', finished);
  assert.equal(d.icon, 'none', `finished=${finished} must stay neutral`);
  assert.equal(d.animated, false);
}
assert.equal(wm.display('bogus', null).icon, 'none', 'unknown marks fall back to neutral');

// Every state has an SVG with a fixed box and no emoji.
const emoji = /[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}]/u;
for (const name of [...wm.MARKS, 'working', 'favorite']) {
  const svg = wm.iconSvg(name);
  assert.match(svg, /^<svg[^>]* width="16" height="16" viewBox="0 0 24 24"/);
  assert.match(svg, /aria-hidden="true"/);
  assert.ok(!emoji.test(svg), `${name} icon must not use emoji`);
}
assert.notEqual(wm.iconSvg('resolved'), wm.iconSvg('frozen'), 'states have distinct shapes');

// Rows resolve to the pane scope only when the W1 binding is unambiguous.
const panes = [{paneKey: 'pk1', session: 's', paneId: '%1'}, {paneKey: 'pk2', session: 's', paneId: '%2'},
  {paneKey: 'dupA', session: 't', paneId: '%9'}, {paneKey: 'dupB', session: 't', paneId: '%9'}];
assert.deepEqual(wm.targetForRow('s|%2', panes), {scope: 'pane', key: 'pk2', session: 's', paneId: '%2'});
assert.deepEqual(wm.targetForRow('s', panes), {scope: 'session', key: 's', session: 's', paneId: null});
assert.equal(wm.targetForRow('t|%9', panes).scope, 'session', 'ambiguous pane ids never guess a paneKey');
assert.equal(wm.targetForRow('', panes), null);

// Activity: exact pane, then tmux id; a tab is working if any pane is.
const activity = {'pane:pk1': {state: 'working', session: 's'}, 'tmux:s:%2': {state: 'completed', session: 's'}};
assert.equal(wm.activityFor({scope: 'pane', key: 'pk1', session: 's', paneId: '%1'}, activity), 'working');
assert.equal(wm.activityFor({scope: 'pane', key: 'pk2', session: 's', paneId: '%2'}, activity), 'completed');
assert.equal(wm.activityFor({scope: 'session', key: 's'}, activity), 'working');
assert.equal(wm.activityFor({scope: 'session', key: 'other'}, activity), null);

// Menu: four radio marks + an independent favorite toggle per scope.
const row = {mark: 'awaiting_reply', favorite: true};
const pane = wm.menuItems('pane', row, false);
assert.deepEqual(pane.map(i => [i.kind, i.value, i.checked]), [
  ['mark', 'none', false], ['mark', 'resolved', false], ['mark', 'frozen', false],
  ['mark', 'awaiting_reply', true], ['favorite', false, true]]);
const session = wm.menuItems('session', row, false);
assert.deepEqual(session.at(-1), {kind: 'favorite', value: true, label: 'Favorito', checked: false},
  'a tab uses its existing session favorite, not the pane flag');

// Keyboard navigation wraps.
assert.equal(wm.nextIndex('ArrowDown', 4, 5), 0);
assert.equal(wm.nextIndex('ArrowUp', 0, 5), 4);
assert.equal(wm.nextIndex('End', 1, 5), 4);
assert.equal(wm.nextIndex('Home', 3, 5), 0);
assert.equal(wm.nextIndex('x', 2, 5), 2);

(async () => {
  // A stale revision adopts the server's current value instead of overwriting it.
  const calls = [];
  let reply = {status: 200, body: {marks: [{scope: 'pane', key: 'pk1', mark: 'frozen', favorite: false, revision: 3}],
    panes, activity: {}}};
  global.fetch = async (url, opt) => {
    calls.push({url, body: opt && opt.body ? JSON.parse(opt.body) : null});
    return {status: reply.status, json: async () => reply.body};
  };
  await wm.load();
  reply = {status: 409, body: {error: 'Revisión desactualizada',
    current: {scope: 'pane', key: 'pk1', mark: 'resolved', favorite: true, revision: 4}}};
  await assert.rejects(wm.setMark('pane', 'pk1', 'none'));
  assert.deepEqual(calls.at(-1).body, {scope: 'pane', key: 'pk1', value: 'none', expectedRevision: 3});
  reply = {status: 200, body: {mark: {scope: 'pane', key: 'pk1', mark: 'none', favorite: true, revision: 5}}};
  const saved = await wm.setMark('pane', 'pk1', 'none');
  assert.equal(saved.revision, 5);
  assert.equal(calls.at(-1).body.expectedRevision, 4, 'retry uses the adopted revision');
  reply = {status: 200, body: {mark: {scope: 'session', key: 'new', mark: 'frozen', favorite: false, revision: 1}}};
  await wm.setMark('session', 'new', 'frozen');
  assert.equal(calls.at(-1).body.expectedRevision, 0, 'a new scope starts at revision 0');
  console.log('work marks checks passed');
})().catch(err => { console.error(err); process.exit(1); });

// ---- Dos canales (grill 30-sep): la IA pone hechos (semáforo pixel), tú pones marcas (sticker).
assert.deepEqual(wm.channels('none', 'working'), {ai: 'work', sticker: null, mark: 'none', suggest: false});
assert.deepEqual(wm.channels('frozen', 'working'), {ai: 'work', sticker: 'Aparcado', mark: 'frozen', suggest: false});
assert.deepEqual(wm.channels('none', 'completed'), {ai: 'done', sticker: null, mark: 'none', suggest: true});
assert.equal(wm.channels('resolved', 'completed').suggest, false, 'already marked Hecho: no suggestion');
assert.equal(wm.channels('none', 'awaiting_permission').ai, 'need');
assert.equal(wm.channels('none', 'failed').ai, 'error');
assert.equal(wm.channels('none', null).ai, 'idle');
// 1-oct: sin monitos. El estado es un punto de color (mismos hex que lib/work_marks.py); solo «need» late.
for (const name of ['work', 'need', 'done', 'error', 'idle']) {
  const html = wm.aiIconSvg(name);
  assert.match(html, new RegExp(`class="ai-icon ai-dot ai-${name}"`));
  assert.ok(html.includes(`--ai-c:${wm.AI_COLORS[name]}`), `${name}: colour comes from AI_COLORS`);
  assert.ok(html.includes('width:12px;height:12px'), `${name}: 12 px box by default`);
  assert.ok(!html.includes('.png') && !emoji.test(html), `${name}: no sprite, no emoji`);
}
assert.ok(wm.aiIconSvg('need', 16).includes('width:16px;height:16px'), 'size is configurable (pane header uses 16)');
assert.equal(wm.aiCycle('idle'), 0, 'idle is still'); assert.equal(wm.aiCycle('work'), 0, 'working is still');
assert.equal(wm.aiCycle('need'), 1.4, 'needs-you pulses');
assert.equal(wm.AI_COLORS.done, '#60a5fa', 'finished is blue, distinct from working green');
// A session shows its most urgent pane: need > error > work > done > idle.
assert.equal(wm.activityFor({scope: 'session', key: 's'}, {a: {session: 's', state: 'completed'}, b: {session: 's', state: 'awaiting_input'}}), 'awaiting_input');
console.log('two-channel checks ok');
// Paridad con escritorio: clic derecho en la pestaña remota abre el menú de estado.
{
  const src = require('node:fs').readFileSync(require('node:path').join(__dirname, '..', 'dash/work-marks.js'), 'utf8');
  assert.match(src, /addEventListener\('contextmenu'/, 'right click on a remote tab opens the state menu');
  console.log('remote right-click check ok');
}
