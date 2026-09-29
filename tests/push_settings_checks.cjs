// dash/push-settings.js: permission only after an explicit action, denial
// handling without prompt loops, subscription sync and ?event routing.
const assert = require('node:assert/strict');
const path = require('node:path');
const PS = require(path.resolve(__dirname, '../dash/push-settings.js'));

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
const check = async (name, fn) => { await fn(); passed++; console.log('ok -', name); };

(async () => {
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

  console.log(`${passed} push settings checks passed`);
})().catch(err => { console.error(err); process.exit(1); });
