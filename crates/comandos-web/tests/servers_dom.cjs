// Adapted from the existing /home/someguy/codebase/0xJesus/ComandOS/.worktrees/native-extensions-ui-finish/tests/dom_stub.cjs fixture; dataset is configurable for live DOM writes.
// Stub de DOM compartido por los checks Node (no hay jsdom offline): parser de HTML
// para el marcado que pintan los módulos, selectores simples (tag, #id, .clase,
// [attr], [attr="v"]) con combinador descendiente, oyentes por nodo (sin
// burbujeo: dispatch(type, target) simula la delegación) y un document mínimo.
// ---------- DOM mínimo ----------
// Documento mínimo compartido: createElement, oyentes (keydown de Escape) y activeElement.
const doc = { activeElement: null, body: null };
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
  Object.defineProperty(node, 'dataset', { configurable: true, get() {
    const d = {};
    for (const [k, v] of Object.entries(node.attrs)) if (k.startsWith('data-')) d[k.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase())] = v;
    return d;
  } });
  Object.defineProperty(node, 'textContent', { configurable: true,
    get() { return node.children.map(c => c.nodeType === 3 ? c.text : c.textContent).join(''); },
    set(v) { node.children = [{ nodeType: 3, text: String(v) }]; } });
  Object.defineProperty(node, 'innerHTML', { configurable: true, set(v) { if (typeof v !== 'string') throw new Error('innerHTML no es string'); parse(v, node); } });
  Object.defineProperty(node, 'value', { get() { return node.attrs.value ?? ''; }, set(v) { node.attrs.value = String(v); } });
  node.getAttribute = k => (k in node.attrs ? node.attrs[k] : null);
  node.hasAttribute = k => k in node.attrs;
  node.matches = sel => matches(node, sel);
  node.closest = sel => { for (let n = node; n && n.nodeType === 1; n = n.parentNode) if (matches(n, sel)) return n; return null; };
  node.querySelectorAll = sel => queryAll(node, sel);
  node.querySelector = sel => queryAll(node, sel)[0] || null;
  // Eventos sin burbujeo: dispatch(type, target, extra) llama a los oyentes de ESTE nodo
  // con e.target = target (la delegación en la raíz hace el resto, como en el navegador).
  const listeners = {};
  node.addEventListener = (type, fn, capture=false) => { (listeners[type] ||= []).push({fn,capture}); };
  node.removeEventListener = (type, fn) => { listeners[type] = (listeners[type] || []).filter(f => f.fn !== fn); };
  node.listenerCount = type => (listeners[type] || []).length;
  node.dispatch = (type,target=node,extra={}) => {let stopped=false;const event={type,target,preventDefault(){},stopPropagation(){},stopImmediatePropagation(){stopped=true;},...extra}; for(const entry of [...(listeners[type]||[])].sort((a,b)=>Number(b.capture)-Number(a.capture))){if(stopped)break;const result=entry.fn(event);if(result&&result.catch)result.catch(()=>{});} };
  node.click = sel => {if(sel===undefined){node.dispatch('click',node);return;}const t=node.querySelector(sel);if(!t)throw Error('missing click '+sel);t.dispatch('click',t);};
  node.input = (sel, value, extra) => { const t = node.querySelector(sel); t.value = value; node.dispatch('input', t, extra); return t; };
  node.setAttribute = (k, v) => { node.attrs[k] = String(v); };
  node.removeAttribute = k => { delete node.attrs[k]; };
  node.appendChild = child => { if (child.parentNode) child.parentNode.children = child.parentNode.children.filter(c => c !== child); child.parentNode = node; node.children.push(child); return child; };
  node.remove = () => { if (node.parentNode) node.parentNode.children = node.parentNode.children.filter(c => c !== node); node.parentNode = null; };
  node.focus = () => { doc.activeElement = node; };
  node.contains = other => { for (let n = other; n; n = n.parentNode) if (n === node) return true; return false; };
  Object.defineProperty(node, 'className', { get() { return node.attrs.class || ''; }, set(v) { node.attrs.class = String(v); } });
  node.style = {setProperty(k,v){this[k]=v;}};
  Object.defineProperty(node, "id", {get(){return node.attrs.id||"";}});
  node.ownerDocument = doc;
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
  if(c.endsWith(':not(:disabled)')) return !node.disabled && !node.hasAttribute('disabled') && compound(node,c.slice(0,-15));
  if(c.endsWith(':checked')) return !!node.checked || node.hasAttribute('checked') ? compound(node,c.slice(0,-8)) : false;

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
  if(sel.includes(",") && !sel.includes("[")) return sel.split(",").some(s=>matches(node,s.trim()));
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
  if(sel.includes(",") && !sel.includes("[")) return [...new Set(sel.split(",").flatMap(s=>queryAll(scope,s.trim())))];
  const out = [];
  const walk = n => { for (const c of n.children) if (c.nodeType === 1) { if (matches(c, sel)) out.push(c); walk(c); } };
  walk(scope);
  return out;
}

function mkRoot() {
  const root = makeEl('div', { id: 'command-sidebar' }, null);
  let html = '';
  root.renders = 0;
  Object.defineProperty(root, 'innerHTML', { configurable: true, get: () => html, set(v) { if (typeof v !== 'string') throw new Error('innerHTML no es string'); html = v; root.renders++; parse(v, root); } });
  return root;
}

{
  const l = {};
  doc.addEventListener = (t, fn) => { (l[t] ||= []).push(fn); };
  doc.removeEventListener = (t, fn) => { l[t] = (l[t] || []).filter(f => f.fn !== fn); };
  doc.listenerCount = t => (l[t] || []).length;
  doc.dispatch = (t, extra = {}) => { for (const fn of [...(l[t] || [])]) fn({ type: t, target: doc.activeElement, preventDefault() {}, stopPropagation() {}, ...extra }); };
  doc.createElement = tag => makeEl(tag, {}, null);
  doc.body = makeEl('body', {}, null);
}
module.exports = { makeEl, parse, mkRoot, doc };
