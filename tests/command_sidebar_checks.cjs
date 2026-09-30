// node tests/command_sidebar_checks.cjs — desde la raíz del checkout.
// Checks de comportamiento de dash/command-sidebar.js sin navegador. El DOM es
// un stub mínimo propio (no hay jsdom offline): parser de HTML para el marcado
// que pinta el módulo y selectores simples (tag, #id, .clase, [attr], [attr="v"])
// con combinador descendiente. Los clics se emulan con delegación en la raíz.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const mod = require(process.cwd() + '/dash/command-sidebar.js');
const { createCommandSidebar } = mod;

// ---------- DOM mínimo ----------
const VOID = new Set(['input', 'br', 'img', 'hr', 'meta', 'link']);
const decode = s => s.replace(/&(amp|lt|gt|quot|#39);/g, (_, e) => ({ amp: '&', lt: '<', gt: '>', quot: '"', '#39': "'" }[e]));

function makeClassList(node) {
  const get = () => (node.attrs.class || '').split(/\s+/).filter(Boolean);
  const set = list => { node.attrs.class = list.join(' '); };
  return {
    contains: c => get().includes(c),
    add: (...cs) => set([...new Set([...get(), ...cs])]),
    remove: (...cs) => set(get().filter(x => !cs.includes(x))),
    toggle(c, force) { const on = force === undefined ? !get().includes(c) : !!force; on ? this.add(c) : this.remove(c); return on; },
  };
}

function makeEl(tag, attrs, parent) {
  const node = { nodeType: 1, tagName: tag.toUpperCase(), attrs, children: [], parentNode: parent };
  node.classList = makeClassList(node);
  Object.defineProperty(node, 'dataset', { get() {
    const d = {};
    for (const [k, v] of Object.entries(node.attrs)) if (k.startsWith('data-')) d[k.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase())] = v;
    return d;
  } });
  Object.defineProperty(node, 'textContent', { get() { return node.children.map(c => c.nodeType === 3 ? c.text : c.textContent).join(''); } });
  Object.defineProperty(node, 'value', { get() { return node.attrs.value ?? ''; }, set(v) { node.attrs.value = String(v); } });
  node.getAttribute = k => (k in node.attrs ? node.attrs[k] : null);
  node.hasAttribute = k => k in node.attrs;
  node.matches = sel => matches(node, sel);
  node.closest = sel => { for (let n = node; n && n.nodeType === 1; n = n.parentNode) if (matches(n, sel)) return n; return null; };
  node.querySelectorAll = sel => queryAll(node, sel);
  node.querySelector = sel => queryAll(node, sel)[0] || null;
  return node;
}

function parse(html, host) {
  host.children = [];
  let cur = host, i = 0;
  const tagRe = /<\/?([a-zA-Z][\w-]*)((?:\s+[\w:-]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+))?)*)\s*\/?>/y;
  while (i < html.length) {
    const lt = html.indexOf('<', i);
    const textEnd = lt === -1 ? html.length : lt;
    if (textEnd > i) { cur.children.push({ nodeType: 3, text: decode(html.slice(i, textEnd)) }); i = textEnd; continue; }
    tagRe.lastIndex = i;
    const m = tagRe.exec(html);
    if (!m) throw new Error('HTML no parseable cerca de: ' + html.slice(i, i + 60));
    i = tagRe.lastIndex;
    const name = m[1].toLowerCase();
    if (m[0][1] === '/') {
      for (let n = cur; n !== host; n = n.parentNode) if (n.tagName === name.toUpperCase()) { cur = n.parentNode; break; }
      continue;
    }
    const attrs = {};
    const aRe = /([\w:-]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?/g;
    let a; while ((a = aRe.exec(m[2]))) attrs[a[1]] = decode(a[2] ?? a[3] ?? a[4] ?? '');
    const node = makeEl(name, attrs, cur);
    cur.children.push(node);
    if (!VOID.has(name) && !m[0].endsWith('/>')) cur = node;
  }
}

function splitSel(sel) {
  const parts = []; let buf = '', inBr = false, q = '';
  for (const ch of sel.trim()) {
    if (q) { buf += ch; if (ch === q) q = ''; continue; }
    if (ch === '"' || ch === "'") { q = ch; buf += ch; continue; }
    if (ch === '[') inBr = true; if (ch === ']') inBr = false;
    if (/\s/.test(ch) && !inBr) { if (buf) parts.push(buf); buf = ''; continue; }
    buf += ch;
  }
  if (buf) parts.push(buf);
  return parts;
}
function compound(node, c) {
  const re = /([a-zA-Z][\w-]*)|#([\w-]+)|\.([\w-]+)|\[([\w-]+)(?:=(?:"([^"]*)"|'([^']*)'|([^\]]+)))?\]/g;
  let m, n = 0;
  while ((m = re.exec(c))) {
    n += m[0].length;
    if (m[1] && node.tagName !== m[1].toUpperCase()) return false;
    if (m[2] && node.attrs.id !== m[2]) return false;
    if (m[3] && !node.classList.contains(m[3])) return false;
    if (m[4]) { if (!(m[4] in node.attrs)) return false; const v = m[5] ?? m[6] ?? m[7]; if (v !== undefined && node.attrs[m[4]] !== v) return false; }
  }
  if (n !== c.length) throw new Error('selector no soportado: ' + c);
  return true;
}
function matches(node, sel) {
  const parts = splitSel(sel);
  if (!compound(node, parts.at(-1))) return false;
  let n = node.parentNode;
  for (let k = parts.length - 2; k >= 0; k--) {
    while (n && n.nodeType === 1 && !compound(n, parts[k])) n = n.parentNode;
    if (!n || n.nodeType !== 1) return false;
    n = n.parentNode;
  }
  return true;
}
function queryAll(scope, sel) {
  const out = [];
  const walk = n => { for (const c of n.children) if (c.nodeType === 1) { if (matches(c, sel)) out.push(c); walk(c); } };
  walk(scope);
  return out;
}

function mkRoot() {
  const root = makeEl('div', { id: 'command-sidebar' }, null);
  const listeners = {};
  let html = '';
  root.renders = 0;
  Object.defineProperty(root, 'innerHTML', { get: () => html, set(v) { if (typeof v !== 'string') throw new Error('innerHTML no es string'); html = v; root.renders++; parse(v, root); } });
  root.addEventListener = (type, fn) => { (listeners[type] ||= []).push(fn); };
  const dispatch = (type, target) => { for (const fn of listeners[type] || []) fn({ type, target, preventDefault() {}, stopPropagation() {} }); };
  root.click = sel => { const t = root.querySelector(sel); if (!t) throw new Error('sin elemento para clic: ' + sel); dispatch('click', t); };
  root.input = (sel, value) => { const t = root.querySelector(sel); t.value = value; dispatch('input', t); };
  return root;
}

// ---------- checks ----------
const catalog = JSON.parse(fs.readFileSync(process.cwd() + '/tests/fixtures/command-catalog.json'));
(async () => {
  const calls = [], toasts = [];
  const store = new Map(), storage = { getItem: k => store.get(k) ?? null, setItem: (k, v) => store.set(k, v), removeItem: k => store.delete(k) };
  let target = { session: 'demo', pane: '%2', kind: 'pane', title: 'demo %2' }, ids = 0, typeStatus = 200;
  const api = async (path, body) => { calls.push([path, body]);
    if (path.startsWith('/commands/catalog')) return { cliInPane: 'codex', target: { session: 'demo', pane: '%2' }, catalog, versionsAt: 1 };
    if (path === '/chains') return { chains: [{ slug: 'yolo', name: 'Codex yolo', steps: [{ kind: 'shell', text: 'codex --dangerously-bypass-approvals-and-sandbox' }, { kind: 'pane', text: '/model' }] }, { slug: 'rota', name: 'rota', error: 'Paso inválido: - foo: bar' }] };
    if (path === '/pane/type') { if (typeStatus !== 200) { const e = new Error('El pane %2 ya no existe'); e.code = 'pane_gone'; throw e; } return { ok: true, typed: body.text.length, requestId: body.requestId }; }
    throw new Error('ruta inesperada ' + path); };
  const root = mkRoot();
  const sb = createCommandSidebar({ api, root, storage, makeId: () => 'id-' + (++ids), getTarget: () => target, focusTarget: () => {}, openBuilder: () => {}, toast: (m, e) => toasts.push([m, e]), terminals: () => [] });
  await sb.refresh();
  assert.equal(calls[0][0], '/commands/catalog?session=demo&pane=%252');
  // 1) el CLI del pane abre solo, marcado, con yolo primero y todo plegable
  assert.equal(root.querySelector('.cs-cli.here').dataset.cli, 'codex');
  assert.equal(root.querySelector('.cs-cli.here .launch').classList.contains('yolo'), true);
  assert.equal(root.querySelector('.cs-cli.here .launch.yolo .cmd').dataset.cmd, 'codex --dangerously-bypass-approvals-and-sandbox');
  root.click('.cs-cli.here .cli-h'); assert.equal(root.querySelector('.cs-cli.here').classList.contains('open'), false);
  root.click('.cs-cli.here .cli-h'); assert.equal(root.querySelector('.cs-cli.here').classList.contains('open'), true);
  assert.equal(JSON.parse(store.get('comandos.commands.open')).includes('codex'), true);
  // R4: todo CLI, bloque de arranque y grupo está en el DOM aunque esté plegado
  assert.equal(root.querySelectorAll('.cs-cli').length, 5);
  assert.equal(root.querySelectorAll('.cs-cli[data-cli="grok"] .grp').length, 4);
  assert.equal(root.querySelector('.cs-cli[data-cli="grok"]').classList.contains('open'), false);
  assert.equal(root.querySelector('.cs-cli.here .launch.normal').classList.contains('open'), false);
  assert.equal(root.querySelector('.cs-cli[data-cli="opencode"] .launch.yolo').textContent.includes('sin flag'), true);
  // 2) un clic teclea sin Enter en el pane capturado, con requestId
  root.click('.cs-cli.here .grp .cmd');
  await sb.state.typing;
  const typed = calls.filter(c => c[0] === '/pane/type');
  assert.deepEqual(typed[0][1], { session: 'demo', pane: '%2', text: '/model', requestId: 'id-1' });
  assert.equal(calls.some(c => c[0] === '/send'), false);
  // segundo clic con un tecleo en vuelo: no llama y avisa
  root.click('.cs-cli.here .grp .cmd'); const inflight = calls.length;
  root.click('.cs-cli.here .grp .cmd');
  assert.equal(calls.length, inflight); assert.equal(toasts.at(-1)[0], 'Espera a que termine de escribir');
  await sb.state.typing;
  // 3) chips de argumento
  root.click('.cs-cli[data-cli="claude"] .cli-h'); root.click('.cs-cli[data-cli="claude"] .opts button');
  await sb.state.typing; assert.equal(calls.at(-1)[1].text, '/model claude-fable-5-1');
  assert.equal(root.querySelector('.cs-cli[data-cli="claude"] .grp .cmd code').textContent, '/model …');
  // 4) cadena guardada: correr fija el destino; Siguiente avanza solo con 200; pane cerrado no salta de paso
  root.click('.cs-saved-item[data-run="yolo"] button');
  assert.equal(root.querySelector('.cs-runner .r-h').textContent.includes('paso 1 de 2'), true);
  target = { session: 'demo', pane: '%9', kind: 'pane', title: 'otro' };            // el usuario cambió de pane
  root.click('.cs-next'); await sb.state.typing;
  assert.equal(calls.at(-1)[1].pane, '%2');                                       // destino fijado al arrancar
  assert.equal(sb.state.run.step, 1);
  typeStatus = 404; root.click('.cs-next'); await sb.state.typing.catch(() => {});
  assert.equal(sb.state.run.step, 1); assert.equal(toasts.at(-1)[0], 'El pane %2 ya no existe');
  assert.equal(root.querySelector('.cs-runner').textContent.includes('El pane %2 ya no existe'), true);
  typeStatus = 200; root.click('.cs-next'); await sb.state.typing;
  assert.equal(root.querySelector('.cs-runner').classList.contains('done'), true);
  assert.equal(root.querySelector('.cs-runner').textContent.includes('completa'), true);
  root.click('.cs-runner [data-run-stop]'); assert.equal(sb.state.run, null); assert.equal(root.querySelector('.cs-runner'), null);
  // 5) cadena rota se lista con error y sin botón Correr
  assert.equal(root.querySelector('.cs-saved-item[data-run="rota"]'), null);
  assert.equal(root.querySelector('.cs-saved-item.error').textContent.includes('Paso inválido'), true);
  assert.equal(root.querySelector('.cs-saved-item.error button'), null);
  // 6) terminal rápida sin CLI: comandos /… atenuados, arranques activos; al arrancar se recuerda el CLI
  target = { session: 'term-q1', pane: '%7', kind: 'term', title: 'Terminal 14:32' };
  sb.render(); const before = calls.length; root.click('.cs-cli[data-cli="claude"] .grp .cmd'); assert.equal(calls.length, before);   // atenuado: no teclea
  assert.equal(root.querySelector('.cs-cli[data-cli="claude"] .grp .cmd').classList.contains('dis'), true);
  root.click('.cs-cli[data-cli="claude"] .launch.yolo .cmd'); await sb.state.typing;
  assert.equal(store.get('comandos.commands.preferred.%7'), 'claude');
  // 7) el CLI sale del pane: la barra deja de marcar "en este pane"
  target = { session: 'demo', pane: '%2', kind: 'pane', title: 'demo %2' };
  sb.applyCatalog({ cliInPane: '', target: { session: 'demo', pane: '%2' }, catalog, versionsAt: 2 });
  assert.equal(root.querySelector('.cs-cli.here'), null);
  assert.equal(root.querySelector('.cs-cli[data-cli="codex"] .grp .cmd').classList.contains('dis'), true);
  // 8) drift y missing
  const drift = JSON.parse(JSON.stringify(catalog)); drift.clis[1].version.status = 'drift'; drift.clis[4].version.status = 'missing';
  sb.applyCatalog({ cliInPane: 'codex', target: { session: 'demo', pane: '%2' }, catalog: drift, versionsAt: 3 });
  assert.equal(root.querySelector('.cs-cli[data-cli="codex"]').classList.contains('drift'), true);
  assert.equal(root.querySelector('.cs-cli[data-cli="codex"]').textContent.includes('el CLI confirma'), true);
  assert.equal(root.querySelector('.cs-cli[data-cli="agy"] .launch .cmd').classList.contains('dis'), true);

  // extra) la búsqueda filtra filas sin tocar state.open
  const openBefore = [...sb.state.open].sort().join();
  root.input('.cs-search', 'compact');
  const visible = root.querySelectorAll('.cmd').filter(r => !r.hasAttribute('hidden'));
  assert.ok(visible.length >= 3 && visible.every(r => /compact/i.test(r.textContent)));
  assert.equal(root.querySelector('.cs-cli[data-cli="codex"] .launch.yolo').hasAttribute('hidden'), true);
  assert.equal([...sb.state.open].sort().join(), openBefore);
  root.input('.cs-search', '');
  // extra) terminales rápidas: foco y nueva
  const focused = [], root2 = mkRoot();
  const sb2 = createCommandSidebar({ api, root: root2, storage, makeId: () => 'x', getTarget: () => ({ session: 'term-q1', pane: '%7', paneKey: 'term-q1:%7', kind: 'term', title: 'T' }),
    focusTarget: t => focused.push(t), openBuilder: () => focused.push('builder'), toast: () => {}, newTerm: () => focused.push('new'),
    terminals: () => [{ tabId: 'q1', paneKey: 'term-q1:%7', session: 'term-q1', pane: '%7', label: 'Terminal <14:32>', cwd: '/tmp' }] });
  sb2.render();
  assert.equal(root2.querySelector('.cs-terms .t[data-focus-term="q1"]').textContent.includes('Terminal <14:32>'), true);
  assert.equal(root2.querySelector('.cs-terms .t[data-focus-term="q1"]').classList.contains('cur'), true);
  root2.click('.t[data-focus-term="q1"]'); root2.click('.t.plus[data-new-term]'); root2.click('.cs-chains[data-open-builder]');
  assert.equal(focused[0].kind, 'term'); assert.equal(focused[0].pane, '%7'); assert.deepEqual(focused.slice(1), ['new', 'builder']);
  // helpers puros para el constructor de cadenas (S3)
  const claude = catalog.clis[0];
  const build = mod.cliHTML(claude, { mode: 'build', open: new Set() });
  assert.equal(build.includes('data-add="/model claude-fable-5-1"'), true);
  assert.equal(build.includes('data-cmd='), false);
  assert.equal(mod.rowHTML({ text: '<x> "y"', description: 'd&d' }, {}).includes('&lt;x&gt; &quot;y&quot;'), true);
  console.log('command-sidebar checks ok');
})().catch(e => { console.error(e); process.exit(1); });
