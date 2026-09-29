// dash/sw.js push + notificationclick contract in a fake ServiceWorker scope.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.resolve(__dirname, '../dash/sw.js'), 'utf8');

function load({ windows = [] } = {}) {
  const handlers = {};
  const shown = [];
  const opened = [];
  const self = {
    location: { origin: 'https://comandos.example.ts.net:8444' },
    addEventListener: (type, fn) => { handlers[type] = fn; },
    skipWaiting() {},
    registration: { showNotification: async (title, options) => { shown.push({ title, options }); } },
    clients: {
      claim() {},
      matchAll: async () => windows,
      openWindow: async (url) => { opened.push(url); return {}; },
    },
  };
  const ctx = { self, URL, caches: { open: async () => ({ addAll: async () => {} }), keys: async () => [] },
    fetch: async () => ({}), console };
  vm.runInNewContext(source, ctx);
  return { handlers, shown, opened };
}

function event(extra) {
  const waits = [];
  return { ...extra, waitUntil: (p) => waits.push(p), waits };
}

let passed = 0;
const check = async (name, fn) => { await fn(); passed++; console.log('ok -', name); };

(async () => {
  await check('push shows project + brief title with a stable tag, offline-capable', async () => {
    const sw = load();
    const e = event({ data: { json: () => ({ eventId: 'ev-1', title: 'comandos', body: 'Terminó el turno',
      tag: 'comandos-event-ev-1', url: 'https://evil.example/?token=x' }) } });
    sw.handlers.push(e);
    await Promise.all(e.waits);
    assert.equal(sw.shown.length, 1);
    const { title, options } = sw.shown[0];
    assert.equal(title, 'comandos');
    assert.equal(options.body, 'Terminó el turno');
    assert.equal(options.tag, 'comandos-event-ev-1');
    assert.deepEqual(JSON.parse(JSON.stringify(options.data)), { eventId: 'ev-1' });
    assert.ok(!JSON.stringify(options).includes('evil.example'), 'the payload URL is never trusted');
  });

  await check('a retried push reuses the same tag', async () => {
    const sw = load();
    for (let i = 0; i < 2; i++) {
      const e = event({ data: { json: () => ({ eventId: 'ev-2', title: 'p', body: 'b' }) } });
      sw.handlers.push(e);
      await Promise.all(e.waits);
    }
    assert.equal(sw.shown[0].options.tag, sw.shown[1].options.tag);
    assert.equal(sw.shown[0].options.tag, 'comandos-event-ev-2');
  });

  await check('an unreadable payload still shows a generic notice', async () => {
    const sw = load();
    const e = event({ data: { json: () => { throw new Error('bad'); } } });
    sw.handlers.push(e);
    await Promise.all(e.waits);
    assert.equal(sw.shown[0].title, 'CommandOS');
  });

  await check('click opens ?event=<id> on the app origin without tokens', async () => {
    const sw = load();
    let closed = false;
    const e = event({ notification: { close: () => { closed = true; }, data: { eventId: 'a b&c' } } });
    sw.handlers.notificationclick(e);
    await Promise.all(e.waits);
    assert.ok(closed);
    assert.deepEqual(sw.opened, ['https://comandos.example.ts.net:8444/?event=a%20b%26c']);
    assert.ok(!sw.opened[0].includes('token'));
  });

  await check('click focuses an open window and asks it to open the event (no reload)', async () => {
    const messages = [];
    let focused = false;
    const win = { url: 'https://comandos.example.ts.net:8444/?x=1', focus: async () => { focused = true; },
      navigate: async () => { throw new Error('must not reload the app'); },
      postMessage: (m) => messages.push(m) };
    const other = { url: 'https://other.example/', focus: async () => { throw new Error('foreign'); } };
    const sw = load({ windows: [other, win] });
    const e = event({ notification: { close() {}, data: { eventId: 'ev-3' } } });
    sw.handlers.notificationclick(e);
    await Promise.all(e.waits);
    assert.ok(focused);
    assert.deepEqual(JSON.parse(JSON.stringify(messages)), [{ type: 'comandos:open-event', eventId: 'ev-3' }]);
    assert.equal(sw.opened.length, 0);
  });

  await check('the fetch handler is still registered', () => {
    const sw = load();
    assert.equal(typeof sw.handlers.fetch, 'function');
    assert.equal(typeof sw.handlers.install, 'function');
  });

  console.log(`${passed} service worker push checks passed`);
})().catch((err) => { console.error(err); process.exit(1); });
