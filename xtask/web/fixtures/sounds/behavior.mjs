// Original assertion cases, adapted only for Node/browser module loading.
const equal=(a,b,msg='')=>{if(a!==b)throw new Error(msg+': '+String(a)+' !== '+String(b));};
const stable=v=>JSON.stringify(v,(_,v)=>v&&typeof v==='object'&&!Array.isArray(v)?Object.fromEntries(Object.keys(v).sort().map(k=>[k,v[k]])):v);
const deep=(a,b,msg='')=>equal(stable(a),stable(b),msg);
const assert={equal,deepEqual:deep,deepStrictEqual:deep,ok:(v,m='')=>{if(!v)throw new Error(m||'expected truthy');},match:(v,re)=>{if(!re.test(v))throw new Error('regex mismatch');},throws:(fn,cls,msg)=>{let caught;try{fn();}catch(e){caught=e;}if(!caught||!(caught instanceof cls))throw new Error(msg||'expected throw');}};
const setImmediate=fn=>setTimeout(fn,0);
export async function runBehavior(){const output={};
{const D=globalThis.ComandosDeviceDrafts;const rows=[];
// W4 client checks: drafts return to the composer only, reading anchors
// report a missing line instead of jumping elsewhere. No browser.
'use strict';




let passed = 0;
async function check(name, fn) {
  try { await fn(); passed++; rows.push({name,ok:true}); } catch (e) { console.error('FAIL', name, '\n', e); throw e; }
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

await (async () => {
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

  })();

await check('anchor keeps a lone surrogate at the 300 UTF16 unit boundary', async () => {
  const source='a'.repeat(299)+'😀 tail', t=timers();let patch;
  const a=D.createAnchor({key:'edge',load:async()=>({readingAnchors:{edge:patch.anchorsPatch.edge}}),save:value=>{patch=value;return Promise.resolve();},now:()=>1,schedule:t.schedule,cancel:t.cancel});
  a.remember(source,0,0);t.run();
  assert.equal(patch.anchorsPatch.edge.text.length,300);
  assert.equal(patch.anchorsPatch.edge.text.charCodeAt(299),55357);
  assert.deepEqual(await a.find(source),{state:'found',index:0});
});
output.device_drafts=rows;}
{const PS=globalThis.PushSettings;const rows=[];
// dash/push-settings.js: permission only after an explicit action, denial
// handling without prompt loops, subscription sync and ?event routing.




const KEY = 'BOrKqD0jdhZ4Ta4Q2S7XqVy9YkKxzwP3xqg7r5RkVY8H8Yb3x8o5JH3fSxPp9QmW1dYkGiJ0rJ6nq3M0Qm2a1sQ';

function env({ permission = 'default', answer = 'granted', existing = null, secure = true, keyAvailable = true } = {}) {
  const calls = { prompts: 0, subscribe: [], requests: [] };
  const storage = new Map();
  let sub = existing;
  const makeSub = () => ({ endpoint: 'https://fcm.googleapis.com/fcm/send/abc', toJSON() { return { endpoint: this.endpoint, keys: { p256dh: 'p', auth: 'a' } }; },
    unsubscribe: async () => { sub = null; return true; } });
  const Notification = { permission, requestPermission: async () => { calls.prompts++; Notification.permission = answer; return answer; } };
  return {
    calls,
    env: {
      isSecureContext: secure, PushManager: function () {}, Notification, crypto: { randomUUID: () => 'uuid-1' },
      navigator: { userAgent: 'Android Chrome', serviceWorker: { ready: Promise.resolve({ pushManager: {
        getSubscription: async () => sub,
        subscribe: async (opts) => { calls.subscribe.push(opts); sub = makeSub(); return sub; },
      } }) } },
      storage: { getItem: k => (storage.has(k) ? storage.get(k) : null), setItem: (k, v) => storage.set(k, String(v)), removeItem: k => storage.delete(k) },
      fetchJson: async (method, url, body) => {
        calls.requests.push({ method, url, body });
        if (url === '/push/key') return keyAvailable ? { available: true, publicKey: KEY } : { available: false, error: 'Push no disponible' };
        if (url === '/push/test') return { ok: true, status: 201, removed: false };
        return { ok: true };
      },
    },
  };
}

let passed = 0;
const check = async (name, fn) => { await fn(); passed++; rows.push({name,ok:true}); };

await (async () => {
  await check('reading the state never prompts for permission', async () => {
    const t = env();
    const c = PS.createController(t.env);
    const r = await c.current();
    assert.equal(r.state, 'disabled');
    assert.equal(await c.resync(), false);
    assert.equal(t.calls.prompts, 0);
    assert.equal(t.calls.requests.length, 0);
  });

  await check('enable prompts once, subscribes with the VAPID key and registers the device', async () => {
    const t = env();
    const r = await PS.createController(t.env).enable();
    assert.equal(r.state, 'enabled');
    assert.equal(t.calls.prompts, 1);
    assert.equal(t.calls.subscribe[0].userVisibleOnly, true);
    assert.equal(t.calls.subscribe[0].applicationServerKey.length, 65);
    const post = t.calls.requests.find(q => q.url === '/push/subscription');
    assert.equal(post.method, 'POST');
    assert.equal(post.body.deviceId, 'uuid-1');
    assert.equal(post.body.subscription.endpoint, 'https://fcm.googleapis.com/fcm/send/abc');
  });

  await check('a denied permission explains how to fix it and never re-prompts', async () => {
    const t = env({ answer: 'denied' });
    const c = PS.createController(t.env);
    const first = await c.enable();
    assert.equal(first.state, 'denied');
    assert.match(first.message, /bloqueado/);
    const second = await c.enable();
    assert.equal(second.state, 'denied');
    assert.equal(t.calls.prompts, 1, 'no prompt loop after denial');
    assert.equal(t.calls.subscribe.length, 0);
  });

  await check('a dismissed prompt stays disabled without subscribing', async () => {
    const t = env({ answer: 'default' });
    const r = await PS.createController(t.env).enable();
    assert.equal(r.state, 'disabled');
    assert.equal(t.calls.subscribe.length, 0);
  });

  await check('insecure origins and missing support are explained, not prompted', async () => {
    const t = env({ secure: false });
    const r = await PS.createController(t.env).enable();
    assert.equal(r.state, 'unsupported');
    assert.match(r.message, /HTTPS/);
    assert.equal(t.calls.prompts, 0);
  });

  await check('server without push reports an error and does not subscribe', async () => {
    const t = env({ keyAvailable: false });
    const r = await PS.createController(t.env).enable();
    assert.equal(r.state, 'error');
    assert.equal(t.calls.subscribe.length, 0);
  });

  await check('test and disable are explicit actions against the stored endpoint', async () => {
    const t = env();
    const c = PS.createController(t.env);
    await c.enable();
    assert.equal((await c.test()).state, 'enabled');
    assert.deepEqual(t.calls.requests.find(q => q.url === '/push/test').body, { endpoint: 'https://fcm.googleapis.com/fcm/send/abc' });
    assert.equal((await c.disable()).state, 'disabled');
    assert.equal(t.calls.requests.find(q => q.method === 'DELETE').url, '/push/subscription');
    assert.equal((await c.current()).state, 'disabled');
  });

  await check('resync re-posts an existing subscription only after an explicit enable', async () => {
    const t = env();
    const c = PS.createController(t.env);
    await c.enable();
    t.calls.requests.length = 0;
    assert.equal(await c.resync(), true);
    assert.deepEqual(t.calls.requests.map(q => q.url), ['/push/subscription']);
    assert.equal(t.calls.prompts, 1);
  });

  await check('?event=<id> is routed once and removed from the address bar', async () => {
    const dispatched = [];
    let replaced = null;
    const win = {
      location: { search: '?event=ev%2042&x=1', pathname: '/', hash: '' },
      history: { state: null, replaceState: (s, t, url) => { replaced = url; } },
      setTimeout: fn => fn(),
      CustomEvent: class { constructor(type, init) { this.type = type; this.detail = init.detail; } },
      dispatchEvent: e => dispatched.push(e),
      navigator: {},
    };
    PS.installEventRouting(win);
    assert.equal(replaced, '/?x=1');
    assert.deepEqual(dispatched.map(e => [e.type, e.detail.eventId]), [['comandos:open-event', 'ev 42']]);
  });

  })();

output.push_settings=rows;}
{const SOUNDS=globalThis.ComandosUISounds;
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

await (async () => {
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

})();
output.ui_sounds=results;}
{const layout=globalThis.WorkspaceLayout;const cases=await(await fetch('/tests/fixtures/workspace_layout.json')).json();const rows=[];const ops = {move: layout.moveTab, detach: layout.detachTab, resize: layout.resizeSplit};
const tabs = d => d.groups.flatMap(g => layout.tabIds(g.tree)).sort();
let passed = 0;
for (const c of cases) {
  const before = JSON.stringify(c.doc);
  if (c.error) assert.throws(() => ops[c.op](c.doc, ...c.args), Error, c.name);
  else {
    const out = ops[c.op](c.doc, ...c.args);
    assert.deepStrictEqual(out, c.expect, c.name);
    assert.deepStrictEqual(tabs(out), tabs(c.doc), c.name);
  }
  assert.equal(JSON.stringify(c.doc), before, c.name + ': input mutated');
  passed++;rows.push({name:c.name,ok:true});
}
output.workspace_layout=rows;}
{const {createQuickTerminal}=globalThis.ComandosQuickTerminal;await (async () => {
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
  output.quick_terminal=[{name:'same request until success, reload, double-click, next id and exact body',ok:true}];
})();
}
output.session_config=[];
{const S=globalThis.SessionConfig;const capture=result=>output.session_config.push({name:'test_combined_cli_account_effort_uses_one_consistent_draft',result});const registry={harnesses:{claude:{accounts:[{alias:'main',selectable:true}]},codex:{accounts:[{alias:'work',selectable:true}]}},motors:{claude:{models:[{id:'opus',efforts:['high'],defaultEffort:'high'}]},codex:{models:[{id:'gpt-6-astra',efforts:['high','ultra'],defaultEffort:'high'}]}},matrix:[{harness:'claude',motor:'claude',selectable:true},{harness:'codex',motor:'codex',selectable:true}]};
      const before=S.draft({agent:'claude',model:'opus',effort:'high',account:'main'});
      let after=S.update(registry,before,'toHarness','codex');
      const invalid=S.validate(registry,after);
      after=S.update(registry,after,'harnessAccount','work');
      after=S.update(registry,after,'effort','ultra');
      capture({before,after,invalid,valid:S.validate(registry,after),changed:S.changed(before,after)});
    }
{const S=globalThis.SessionConfig;const capture=result=>output.session_config.push({name:'test_invalid_effort_cannot_submit_and_unchanged_draft_is_not_change',result});const registry={harnesses:{claude:{accounts:[{alias:'main',selectable:true}]},codex:{accounts:[{alias:'work',selectable:true}]}},motors:{claude:{models:[{id:'opus',efforts:['high'],defaultEffort:'high'}]},codex:{models:[{id:'gpt-6-astra',efforts:['high','ultra'],defaultEffort:'high'}]}},matrix:[{harness:'claude',motor:'claude',selectable:true},{harness:'codex',motor:'codex',selectable:true}]};
      const before=S.draft({agent:'codex',model:'gpt-6-astra',effort:'high',account:'work'});
      capture({same:S.changed(before,{...before,interrupt:true}),error:S.validate(registry,{...before,effort:'nonsense'})});
    }
{const S=globalThis.SessionConfig;const capture=result=>output.session_config.push({name:'test_acp_keeps_named_motor_account_separate_from_local_harness',result});const registry={harnesses:{claude:{accounts:[{alias:'main',selectable:true}]},codex:{accounts:[{alias:'work',selectable:true}]}},motors:{claude:{models:[{id:'opus',efforts:['high'],defaultEffort:'high'}]},codex:{models:[{id:'gpt-6-astra',efforts:['high','ultra'],defaultEffort:'high'}]}},matrix:[{harness:'claude',motor:'claude',selectable:true},{harness:'codex',motor:'codex',selectable:true}]};
      registry.harnesses.acp={};registry.matrix.push({harness:'acp',motor:'codex',selectable:true});
      let d=S.draft({agent:'codex',motor:'codex',model:'gpt-6-astra',effort:'high',account:'work'});
      d=S.update(registry,d,'toHarness','acp');
      d=S.update(registry,d,'motorAccount','work');
      capture({d,error:S.validate(registry,d)});
    }
{const S=globalThis.SessionConfig;const capture=result=>output.session_config.push({name:'test_direct_options_skip_unavailable_models_and_accounts',result});const registry={harnesses:{claude:{accounts:[{alias:'main',selectable:true}]},codex:{accounts:[{alias:'work',selectable:true}]}},motors:{claude:{models:[{id:'opus',efforts:['high'],defaultEffort:'high'}]},codex:{models:[{id:'gpt-6-astra',efforts:['high','ultra'],defaultEffort:'high'}]}},matrix:[{harness:'claude',motor:'claude',selectable:true},{harness:'codex',motor:'codex',selectable:true}]};
      registry.motors.codex.models.push({id:'future',soon:true,efforts:['high']});
      registry.harnesses.codex.accounts.push({alias:'logged-out',selectable:false});
      const d=S.draft({agent:'codex',model:'gpt-6-astra',effort:'high',account:'work'});
      capture({models:S.fieldOptions(registry,d,'model'),
        next:S.cycle(registry,d,'model',1),accounts:S.fieldOptions(registry,d,'harnessAccount'),
        invalid:S.validate(registry,{...d,model:'future'})});
    }
{const S=globalThis.SessionConfig;const capture=result=>output.session_config.push({name:'test_recommendations_keep_live_identity_and_filter_incompatible_configurations',result});const registry={harnesses:{claude:{accounts:[{alias:'main',selectable:true}]},codex:{accounts:[{alias:'work',selectable:true}]}},motors:{claude:{models:[{id:'opus',efforts:['high'],defaultEffort:'high'}]},codex:{models:[{id:'gpt-6-astra',efforts:['high','ultra'],defaultEffort:'high'}]}},matrix:[{harness:'claude',motor:'claude',selectable:true},{harness:'codex',motor:'codex',selectable:true}]};
      const d=S.draft({agent:'codex',model:'gpt-6-astra',effort:'high',account:'work',
        observedConfig:{identity:{pid:99},conversationId:'live-conversation'}});
      const config={...d,effort:'ultra',expectedConversationId:'old',interrupt:true};
      const rows=[{config,count:3,lastUsed:3},{config,count:1,lastUsed:1},
        {config:{...config,harnessAccount:'logged-out'},count:10,lastUsed:5},
        {config:d,count:20,lastUsed:4}];
      capture(S.recommendations(registry,d,rows,{agent:'codex'}));
    }
{const S=globalThis.SessionConfig;const capture=result=>output.session_config.push({name:'test_direct_changes_require_confirmation_only_for_route_or_explicit_interruption',result});const registry={harnesses:{claude:{accounts:[{alias:'main',selectable:true}]},codex:{accounts:[{alias:'work',selectable:true}]}},motors:{claude:{models:[{id:'opus',efforts:['high'],defaultEffort:'high'}]},codex:{models:[{id:'gpt-6-astra',efforts:['high','ultra'],defaultEffort:'high'}]}},matrix:[{harness:'claude',motor:'claude',selectable:true},{harness:'codex',motor:'codex',selectable:true}]};
      const d=S.draft({agent:'codex',model:'gpt-6-astra',effort:'high',account:'work'});
      capture({effort:S.requiresConfirmation(d,{...d,effort:'ultra'}),
        account:S.requiresConfirmation(d,{...d,harnessAccount:'other'}),
        cli:S.requiresConfirmation(d,{...d,toHarness:'claude'}),
        motor:S.requiresConfirmation(d,{...d,motor:'claude'}),
        interrupt:S.requiresConfirmation(d,{...d,interrupt:true})});
    }
return output;}
