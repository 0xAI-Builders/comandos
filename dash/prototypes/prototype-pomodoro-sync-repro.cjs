// Isolated diagnostic: actual dashboard JS, two pages, simulated API and clock.
// No browser, live API, timer, notification service, or user data is touched.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const file = process.argv[2] || path.resolve(__dirname, '../index.html');
const html = fs.readFileSync(file, 'utf8');
const start = html.indexOf('const POMO =');
const end = html.indexOf('function fmtMin(', start);
if (start < 0 || end < 0) throw Error('Pomodoro source block missing');
const source = html.slice(start, end);
const now = 2000000000000;
const backend = {focus: null, posts: []};
class Clock extends Date { constructor(...args) { super(...(args.length ? args : [now])); } static now() { return now; } }
const settle = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
function page(name) {
  const nodes = new Map();
  function node(selector) {
    if (!nodes.has(selector)) {
      const classes = new Set();
      nodes.set(selector, {
        innerHTML: '', style: {}, handlers: {},
        classList: {add: k => classes.add(k), remove: k => classes.delete(k), contains: k => classes.has(k), toggle(k, v) { v = v === undefined ? !classes.has(k) : v; v ? classes.add(k) : classes.delete(k); }},
        setAttribute() {}, addEventListener(k, fn) { this.handlers[k] = fn; },
        querySelector: node, querySelectorAll: () => [],
        getBoundingClientRect: () => ({height: 30, bottom: 40, left: 20}),
      });
    }
    return nodes.get(selector);
  }
  let interval = 0;
  const c = vm.createContext({
    Date: Clock, innerWidth: 1440, console,
    $: node, document: {addEventListener() {}},
    S: {list: [{session:'diagnostic-session', project:'Diagnostic', pane:'%999'}]},
    pickSel: a => a[0], tf: es => es, svg: () => '', mdEsc: s => String(s), attrEsc: s => String(s),
    localStorage: {getItem: () => null, setItem() {}},
    toast() {}, setInterval: () => ++interval, clearInterval() {}, setTimeout() {},
    fetch: async () => ({}), openSession: async () => '',
    api: async (path, data) => {
      if (path !== '/pomodoro') throw Error('Unexpected API: ' + path);
      if (data) {
        backend.posts.push({page: name, data: structuredClone(data)});
        if (data.stop) backend.focus = null;
        else if (data.mode) backend.focus = {
          ...data, until: now / 1000 + data.mins * 60, startedAt: now / 1000,
        };
      }
      return {focus: structuredClone(backend.focus), queue: []};
    },
  });
  vm.runInContext(source, c, {filename: file});
  return {run: s => vm.runInContext(s, c), nodes};
}
(async () => {
  const a = page('desktop');
  const b = page('remote');
  await settle();
  a.run('pomoStart(25, "focus")');
  await settle();
  await b.run('pomoSync()');
  const initial = a.run('POMO.end') === b.run('POMO.end');
  const postsBefore = backend.posts.length;
  a.nodes.get('#pp-extend').handlers.click();
  await settle();
  await b.run('pomoSync()');
  const extension = {
    expected: 'Both clients and API deadline extend by five minutes',
    sameDeadline: a.run('POMO.end') === b.run('POMO.end'),
    desktopRemainingMinutes: (a.run('POMO.end') - now) / 60000,
    remoteRemainingMinutes: (b.run('POMO.end') - now) / 60000,
    apiRemainingMinutes: (backend.focus.until * 1000 - now) / 60000,
    extensionPosts: backend.posts.length - postsBefore,
  };
  a.run('pomoFinish(false)');
  await settle();
  await b.run('pomoSync()');
  const stopped = {expected: 'Both clients stop after cancellation', backendStopped: backend.focus === null, remoteStillRunning: b.run('POMO.t !== null')};
  console.log(JSON.stringify({source: file, initialSyncPassed: initial, extension, stopped}, null, 2));
  process.exitCode = initial && extension.sameDeadline && !stopped.remoteStillRunning ? 0 : 1;
})().catch(e => { console.error(e); process.exitCode = 2; });
