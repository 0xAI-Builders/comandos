// node tests/chain_builder_wasm_checks.cjs ABS_WASM_BINDGEN_NODE_MODULE [--edges]
// Exercises actual Rust/WASM with the existing JS sidebar and DOM stub; no browser.
const assert = require('node:assert/strict');
const path = require('node:path');
const Module = require('node:module');
const native = require(path.resolve(process.argv[2]));
const source = path.resolve('dash/chain-builder.js');
if (process.argv[3] !== '--edges') {
  const load = Module._load;
  Module._load = function (request, parent, main) {
    if (request === source) return { createChainBuilder: native.createChainBuilder };
    return load.apply(this, arguments);
  };
  require('./chain_builder_checks.cjs');
} else {
  require(path.resolve('dash/command-sidebar.js'));
  const legacy = require(source);
  const { doc } = require('./dom_stub.cjs');
  const tick = () => new Promise(r => setImmediate(r));
  async function exercise(create) {
    const requests = [], notices = [], refreshed = [];
    let finish, closeCount = 0;
    const chain = { slug: 'surrogate', name: 'A\ud800B\u{1f980}', steps: [{ kind: 'shell', text: '\udfff<&"  ' }, { kind: 'alien', text: null }] };
    const b = create({ root: doc.body, doc, chains: () => [chain],
      catalog: () => { throw Error('unavailable'); }, here: () => { throw Error('unavailable'); },
      target: () => 'pane<&\udfff', hydrate: () => { throw Error('ignored'); },
      onClose: () => closeCount++, toast: (...v) => notices.push(v),
      api: (p, body) => { requests.push([p, body]); return new Promise(r => { finish = r; }); },
      onSaved: (c, o) => { refreshed.push([c, o]); throw Error('refresh failed'); } });
    const bd = () => doc.body.querySelector('.backdrop');
    const state = b.state;
    assert.deepEqual(Object.keys(b), ['open', 'close', 'state']);
    const descriptor = Object.getOwnPropertyDescriptor(b, 'state');
    assert.equal(descriptor.enumerable, true);
    assert.equal(descriptor.configurable, true);
    assert.equal(descriptor.set, undefined);
    assert.equal(b.open('surrogate'), state);
    assert.notEqual(state.steps, chain.steps);
    assert.equal(state.steps[0].text, chain.steps[0].text);
    assert.equal(state.steps[1].text, 'null');
    assert.equal(state.steps[1].kind, 'pane');
    const markup = bd().innerHTML;
    const current = bd();
    assert.equal(b.open('missing'), null);
    assert.equal(bd(), current);
    state.q = '\ud800'; // Public state remains writable and search persists across opens.
    bd().dispatch('click', bd().querySelector('[data-run]'));
    assert.equal(requests.length, 1, 'API is called synchronously before first await');
    assert.equal(b.close(), true);
    assert.equal(closeCount, 0);
    b.open('surrogate');
    assert.equal(state.q, '\ud800');
    assert.equal(bd().querySelector('.m-q').value, '');
    bd().dispatch('click', bd().querySelector('[data-run]'));
    assert.equal(requests.length, 1, 'inflight gate survives reopening');
    finish({ chain }); await tick();
    assert.ok(bd(), 'old save does not close replacement');
    assert.equal(refreshed[0][1].run, false);
    assert.deepEqual(notices.at(-1), ['refresh failed', true]);
    doc.dispatch('keydown', { key: 'Escape' });
    assert.equal(closeCount, 1);
    assert.equal(b.close(), false);
    assert.equal(doc.listenerCount('keydown'), 0);
    return { markup, requests, notices, refreshed, closeCount };
  }
  (async () => {
    const expected = await exercise(legacy.createChainBuilder);
    const actual = await exercise(native.createChainBuilder);
    assert.deepEqual(actual, expected);
    console.log('chain-builder WASM edge/markup/UTF16 oracle checks ok');
  })().catch(e => { console.error(e); process.exitCode = 1; });
}
