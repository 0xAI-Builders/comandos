/* Web renderer for the shared workspace: groups of whole tabs docked in a tree.
   Iframes are never moved in the DOM (that reloads ttyd and opens another
   tmux client); the tree draws slots and each iframe is positioned over its
   slot. Uses index.html globals at call time: openTerms, ensureFrame,
   activeTerm, showView, authToken, toast, tf, S, attrEsc. */
(function(root, factory) {
  const api = factory();
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.WorkspaceDock = api;
})(typeof self !== 'undefined' ? self : this, function() {
  const STACK_BELOW = 560;      // a horizontal split stacks when narrower than this
  const LEAF_MIN = 240;         // minimum height of a stacked tab before scrolling
  const LEAF_MIN_NARROW = 440;  // phones: term.html adds its touch toolbar and composer
  const EDGE_FRACTION = 0.31;   // how close to a tab's edge a drop docks beside it
  const OUTER_PX = 16;          // distance to the viewport edge that wraps the whole group
  const DRAG_PX = 7, HOLD_MS = 180;

  // ---- pure helpers (tested in Node) --------------------------------------
  function edgeFor(rect, x, y, fraction = EDGE_FRACTION) {
    const d = [['left', (x - rect.left) / rect.width], ['right', (rect.right - x) / rect.width],
               ['top', (y - rect.top) / rect.height], ['bottom', (rect.bottom - y) / rect.height]];
    d.sort((a, b) => a[1] - b[1]);
    return d[0][1] >= 0 && d[0][1] <= fraction ? d[0][0] : null;
  }
  function outerEdge(rect, x, y, px = OUTER_PX) {
    const d = [['left', x - rect.left], ['right', rect.right - x], ['top', y - rect.top], ['bottom', rect.bottom - y]];
    d.sort((a, b) => a[1] - b[1]);
    return d[0][1] >= 0 && d[0][1] < px ? d[0][0] : null;
  }
  // Effective axis per split at a width, and the content height it needs.
  function measure(node, width, leafMin = width < STACK_BELOW ? LEAF_MIN_NARROW : LEAF_MIN) {
    if (node.type === 'tab') return {height: leafMin};
    const axis = node.axis === 'x' && width >= STACK_BELOW ? 'x' : 'y';
    const a = measure(node.first, axis === 'x' ? width * node.ratio : width, leafMin);
    const b = measure(node.second, axis === 'x' ? width * (1 - node.ratio) : width, leafMin);
    const height = axis === 'x' ? Math.max(a.height, b.height)
      : node.axis === 'y' ? Math.max(a.height / node.ratio, b.height / (1 - node.ratio)) : a.height + b.height;
    return {axis, a, b, height};
  }
  function previewRect(rect, edge) {
    const r = {left: rect.left, top: rect.top, width: rect.width, height: rect.height};
    if (edge === 'left' || edge === 'right') { r.width /= 2; if (edge === 'right') r.left += r.width; }
    else { r.height /= 2; if (edge === 'bottom') r.top += r.height; }
    return r;
  }

  if (typeof document === 'undefined') return {edgeFor, outerEdge, measure, previewRect};

  // ---- browser state -------------------------------------------------------
  const L = () => window.WorkspaceLayout;
  const W = {doc: null, revision: 0, focus: {}, gesture: null, suppress: 0, pending: false, signature: ''};
  const groups = () => (W.doc && W.doc.groups) || [];
  const tabsOf = g => L().tabIds(g.tree);
  const groupOf = tab => groups().find(g => tabsOf(g).includes(tab));
  const sessionOf = tab => (W.doc.tabs[tab] && W.doc.tabs[tab].session) || tab;
  const label = tab => (openTerms.get(sessionOf(tab)) || {}).label || (W.doc.tabs[tab] || {}).label || tab;
  const live = tab => openTerms.has(sessionOf(tab));
  const esc = s => String(s).replace(/[&<>"']/g, c => ({'&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'}[c]));

  function adopt(state) {
    if (!state || !Array.isArray(state.groups)) return false;
    if (W.gesture || W.pending) return false;           // never yank a layout mid-gesture
    const changed = state.revision !== W.revision || !W.doc;
    W.doc = {schema: state.schema, groups: state.groups, tabs: state.tabs};
    if (state.bindings) W.doc.bindings = state.bindings;
    W.revision = state.revision;
    return changed;
  }
  function focusOf(g) {
    const ids = tabsOf(g);
    if (typeof activeTerm === 'string' && ids.includes(activeTerm)) return activeTerm;
    return ids.includes(W.focus[g.id]) ? W.focus[g.id] : ids.find(live) || ids[0];
  }
  function activeGroup() {
    return (typeof activeTerm === 'string' && groupOf(activeTerm)) || null;
  }

  // Entries for the top strip: one per group, keyed by group id.
  function stripEntries() {
    if (!W.doc) return null;
    return groups().map(g => {
      const ids = tabsOf(g), focus = focusOf(g);
      return {key: 'group:' + g.id, target: 'term:' + sessionOf(focus), group: g, tabs: ids,
              label: label(ids[0]), count: ids.length, session: sessionOf(ids[0]),
              closable: ids.length > 1 || sessionOf(ids[0]) !== 'local',
              groupId: ids.length > 1 ? g.id : null,
              source: ids.length === 1 ? ids[0] : 'group:' + g.id, live: ids.some(live)};
    });
  }
  function isSelected(entry) { return typeof activeTerm === 'string' && entry.tabs.includes(activeTerm); }

  // Tabs visible for a shown session: every tab of its group.
  function visibleTabs(shown) {
    const g = W.doc && groupOf(shown);
    return g ? tabsOf(g).map(sessionOf) : shown ? [shown] : [];
  }

  // ---- tree rendering ------------------------------------------------------
  function nodeHtml(n, path, single) {
    if (n.type === 'tab') {
      const t = n.tabId, ok = live(t);
      return `<section class="ws-leaf${single ? ' single' : ''}${ok ? '' : ' unavailable'}" data-tab="${esc(t)}">` +
        `<header class="ws-leaf-head" data-ws-source="${esc(t)}" title="${esc(tf('Arrastra para mover esta tab', 'Drag to move this tab'))}">` +
        `<span class="ws-grip" aria-hidden="true">⠿</span><b>${esc(label(t))}</b></header>` +
        `<div class="ws-body" data-body="${esc(t)}">${ok ? '' : `<p class="ws-missing">${esc(tf('Sesión no disponible. Su lugar se conserva.', 'Session unavailable. Its place is kept.'))}</p>`}</div></section>`;
    }
    const p = path.join('.');
    return `<div class="ws-split" data-path="${p}" data-axis="${n.axis}"><div class="ws-slot">${nodeHtml(n.first, path.concat('first'))}</div>` +
      `<div class="ws-divider" data-ws-divider="${p}" role="separator" tabindex="0" aria-label="${esc(tf('Ajustar división', 'Resize split'))}"></div>` +
      `<div class="ws-slot">${nodeHtml(n.second, path.concat('second'))}</div></div>`;
  }
  function treeEl() {
    const area = document.getElementById('term-area');
    let el = area.querySelector(':scope > .ws-tree');
    if (!el) { el = document.createElement('div'); el.className = 'ws-tree'; area.prepend(el); }
    return el;
  }
  function applyGrid(n, el, m) {
    if (n.type === 'tab') return;
    const axis = m.axis, r = n.ratio;
    el.dataset.axis = axis;
    el.style.gridTemplateColumns = axis === 'x' ? `minmax(0,${r}fr) 6px minmax(0,${1 - r}fr)` : 'minmax(0,1fr)';
    el.style.gridTemplateRows = axis === 'y'
      ? (n.axis === 'y' ? `minmax(0,${r}fr) 6px minmax(0,${1 - r}fr)` : `minmax(0,${m.a.height}fr) 6px minmax(0,${m.b.height}fr)`)
      : 'minmax(0,1fr)';
    el.children[1].setAttribute('aria-orientation', axis === 'x' ? 'vertical' : 'horizontal');
    // A horizontal split stacked on a narrow screen keeps the shared ratio.
    el.children[1].toggleAttribute('data-stacked', axis !== n.axis);
    applyGrid(n.first, el.children[0].firstElementChild, m.a);
    applyGrid(n.second, el.children[2].firstElementChild, m.b);
  }

  // Draw the group of ``shown`` and place its terminals over their slots.
  function render(shown) {
    const area = document.getElementById('term-area');
    const g = W.doc && groupOf(shown);
    const tree = treeEl();
    if (!g) { tree.hidden = true; tree.innerHTML = ''; W.signature = ''; placeFrames([]); return; }
    W.focus[g.id] = shown;
    const single = g.tree.type === 'tab';
    const signature = JSON.stringify([g.id, g.tree.type === 'tab' ? g.tree : stripRatios(g.tree), tabsOf(g).map(t => [label(t), live(t)])]);
    if (signature !== W.signature) { tree.innerHTML = nodeHtml(g.tree, [], single); W.signature = signature; }
    tree.hidden = false;
    tree.querySelectorAll('.ws-leaf').forEach(el => el.classList.toggle('focused', !single && el.dataset.tab === shown));
    layout(area, tree, g);
  }
  function stripRatios(n) { return n.type === 'tab' ? n.tabId : [n.axis, stripRatios(n.first), stripRatios(n.second)]; }
  function layout(area, tree, g) {
    area = area || document.getElementById('term-area');
    tree = tree || treeEl();
    g = g || (W.doc && groupOf(activeTerm));
    if (!g || tree.hidden) return;
    const m = measure(g.tree, area.clientWidth);
    tree.style.height = Math.max(area.clientHeight, m.height) + 'px';
    applyGrid(g.tree, tree.firstElementChild, m);
    placeFrames(tabsOf(g));
  }
  function placeFrames(tabs) {
    const area = document.getElementById('term-area');
    const base = area.getBoundingClientRect();
    const wanted = new Set(tabs.map(sessionOf));
    for (const [sess, o] of openTerms) {
      if (!o.frame) continue;
      if (!wanted.has(sess)) { o.frame.style.cssText = ''; continue; }
      const t = tabs.find(x => sessionOf(x) === sess);
      const body = document.querySelector(`.ws-tree [data-body="${CSS.escape(t)}"]`);
      if (!body) continue;
      const r = body.getBoundingClientRect();
      Object.assign(o.frame.style, {inset: 'auto', left: (r.left - base.left + area.scrollLeft) + 'px',
        top: (r.top - base.top + area.scrollTop) + 'px', width: r.width + 'px', height: r.height + 'px'});
    }
  }

  // ---- persistence -----------------------------------------------------------
  async function post(document_) {
    const headers = {'Content-Type': 'application/json'}, tok = authToken();
    if (tok) headers['X-Comandos-Token'] = tok;
    const requestId = (crypto.randomUUID && crypto.randomUUID()) || String(Date.now()) + Math.random();
    const r = await fetch('/workspace', {method: 'POST', headers,
      body: JSON.stringify({requestId, expectedRevision: W.revision, document: document_})});
    const body = await r.json().catch(() => ({}));
    return {status: r.status, body};
  }
  async function commit(next, focusTab) {
    const before = {doc: W.doc, revision: W.revision};
    W.doc = next; W.pending = true;
    if (focusTab) showView('term:' + sessionOf(focusTab), true); else showView(activeView);
    try {
      const {status, body} = await post(next);
      W.pending = false;
      if (status === 200) adopt(body);
      else if (status === 409 && body.current) {
        adopt(body.current);
        toast(tf('Otro dispositivo cambió la distribución; se muestra la versión actual.', 'Another device changed the layout; showing the current one.'), true);
      } else {
        W.doc = before.doc; W.revision = before.revision;
        toast(body.error || tf('No se pudo guardar la distribución', 'Could not save the layout'), true);
      }
    } catch (err) {
      W.pending = false; W.doc = before.doc; W.revision = before.revision;
      toast(tf('No se pudo guardar la distribución', 'Could not save the layout'), true);
    }
    showView(activeView);
  }
  function apply(op, ...args) {
    try { return L()[op](W.doc, ...args); } catch (_) { return null; }
  }

  // ---- gestures ---------------------------------------------------------------
  function target(x, y, source) {
    const moved = source.startsWith('group:') ? tabsOf(groups().find(g => 'group:' + g.id === source) || {tree: {type: 'tab', tabId: ''}})
      : [source];
    const strip = document.getElementById('tabbar'), sr = strip.getBoundingClientRect();
    if (x >= sr.left && x <= sr.right && y >= sr.top && y <= sr.bottom) {
      const entries = [...strip.querySelectorAll('[data-tab-key^="group:"]')];
      const hit = entries.findIndex(el => { const r = el.getBoundingClientRect(); return x < r.left + r.width / 2; });
      const index = hit < 0 ? entries.length : hit;
      const el = entries[index], r = el ? el.getBoundingClientRect() : null;
      return {kind: 'bar', index, label: tf('Separar en la barra', 'Separate into the strip'),
              rect: {left: r ? r.left - 3 : sr.right - 60, top: sr.top, width: r ? 6 : 60, height: sr.height}};
    }
    const area = document.getElementById('term-area'), ar = area.getBoundingClientRect();
    if (x < ar.left || x > ar.right || y < ar.top || y > ar.bottom) return null;
    const g = activeGroup();
    const outer = g && outerEdge(ar, x, y);
    if (outer && !tabsOf(g).every(t => moved.includes(t)))
      return {kind: 'dock', target: 'group:' + g.id, edge: outer, rect: previewRect(ar, outer),
              label: tf('Dividir todo el espacio', 'Split the whole space')};
    const leaf = document.elementFromPoint(x, y)?.closest('.ws-leaf');
    if (!leaf || moved.includes(leaf.dataset.tab)) return null;
    const rect = leaf.getBoundingClientRect(), edge = edgeFor(rect, x, y);
    if (!edge) return null;
    return {kind: 'dock', target: leaf.dataset.tab, edge, rect: previewRect(rect, edge),
            label: tf('Colocar junto a ', 'Place next to ') + label(leaf.dataset.tab)};
  }
  function activate() {
    const d = W.gesture;
    if (!d || d.active) return;
    d.active = true;
    document.body.classList.add('ws-dragging');
    const names = d.source.startsWith('group:') ? tabsOf(groups().find(g => 'group:' + g.id === d.source)).map(label) : [label(d.source)];
    d.ghost = Object.assign(document.createElement('div'), {className: 'ws-ghost', textContent: names.join(' + ')});
    d.preview = Object.assign(document.createElement('div'), {className: 'ws-preview'});
    d.preview.hidden = true;
    document.body.append(d.ghost, d.preview);
    moveGhost(d.x, d.y);
  }
  function moveGhost(x, y) { const d = W.gesture; if (d && d.ghost) { d.ghost.style.left = x + 'px'; d.ghost.style.top = y + 'px'; } }
  function start(e) {
    if (e.button !== 0 || !W.doc) return;
    const divider = e.target.closest('[data-ws-divider]');
    const handle = !divider && e.target.closest('[data-ws-source]');
    if (!divider && !handle) return;
    if (divider && divider.hasAttribute('data-stacked')) return;
    if (handle && e.target.closest('.tab-fav, .x')) return;
    W.gesture = {pointer: e.pointerId, el: divider || handle, source: handle && handle.dataset.wsSource,
                 path: divider && divider.dataset.wsDivider, x: e.clientX, y: e.clientY, type: e.pointerType,
                 active: !!divider, target: null};
    if (divider) {
      e.preventDefault();
      try { divider.setPointerCapture(e.pointerId); } catch (_) {}
      document.body.classList.add('ws-dragging');
      return;
    }
    if (e.pointerType === 'touch' || e.pointerType === 'pen')
      W.gesture.timer = setTimeout(() => { try { handle.setPointerCapture(e.pointerId); } catch (_) {} activate(); }, HOLD_MS);
  }
  function motion(e) {
    const d = W.gesture;
    if (!d || d.pointer !== e.pointerId) return;
    if (d.path != null) {
      e.preventDefault();
      const el = document.querySelector(`.ws-split[data-path="${d.path}"]`), g = activeGroup();
      if (!el || !g) return;
      const r = el.getBoundingClientRect(), axis = el.dataset.axis;
      const ratio = axis === 'x' ? (e.clientX - r.left) / r.width : (e.clientY - r.top) / r.height;
      const next = apply('resizeSplit', g.id, d.path ? d.path.split('.') : [], ratio);
      if (next) { W.doc = next; d.changed = true; layout(); }
      return;
    }
    if (!d.active) {
      const far = Math.hypot(e.clientX - d.x, e.clientY - d.y) > DRAG_PX;
      if (!far) return;
      if (d.type === 'touch' || d.type === 'pen') { clearTimeout(d.timer); W.gesture = null; return; }  // a scroll, not a hold
      try { d.el.setPointerCapture(e.pointerId); } catch (_) {}
      activate();
    }
    e.preventDefault();
    moveGhost(e.clientX, e.clientY);
    d.target = target(e.clientX, e.clientY, d.source);
    d.preview.hidden = !d.target;
    if (d.target) {
      const r = d.target.rect;
      Object.assign(d.preview.style, {left: r.left + 'px', top: r.top + 'px', width: r.width + 'px', height: r.height + 'px'});
      d.preview.textContent = d.target.label;
      d.preview.classList.toggle('bar', d.target.kind === 'bar');
    }
  }
  function finish(e, cancel = false) {
    const d = W.gesture;
    if (!d || d.pointer !== e.pointerId) return;
    clearTimeout(d.timer);
    try { d.el.releasePointerCapture(e.pointerId); } catch (_) {}
    d.ghost && d.ghost.remove(); d.preview && d.preview.remove();
    document.body.classList.remove('ws-dragging');
    W.gesture = null;
    if (!d.active) return;
    W.suppress = Date.now() + 350;
    if (d.path != null) {
      if (cancel) { W.doc = d.original || W.doc; layout(); return; }
      if (d.changed) commit(W.doc);
      return;
    }
    if (cancel || !d.target) return;
    const next = d.target.kind === 'bar' ? apply('detachTab', d.source, d.target.index)
      : apply('moveTab', d.source, d.target.target, d.target.edge);
    if (next) commit(next, d.source.startsWith('group:') ? null : d.source);
  }
  function keyResize(e) {
    const divider = e.target.closest && e.target.closest('[data-ws-divider]');
    if (!divider || divider.hasAttribute('data-stacked') || !['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(e.key)) return;
    const g = activeGroup();
    if (!g) return;
    e.preventDefault(); e.stopPropagation();
    const path = divider.dataset.wsDivider ? divider.dataset.wsDivider.split('.') : [];
    let node = g.tree;
    for (const step of path) node = node[step];
    const next = apply('resizeSplit', g.id, path, node.ratio + (['ArrowLeft', 'ArrowUp'].includes(e.key) ? -0.05 : 0.05));
    if (!next) return;
    W.doc = next; layout();
    clearTimeout(W.keyTimer);
    W.keyTimer = setTimeout(() => commit(W.doc), 400);
  }
  function init() {
    document.addEventListener('pointerdown', e => {
      if (e.target.closest && (e.target.closest('#tabbar') || e.target.closest('#term-area'))) {
        if (W.gesture && W.gesture.path != null) return;
        start(e);
        if (W.gesture && W.gesture.path != null) W.gesture.original = W.doc;
      }
    });
    document.addEventListener('pointermove', motion, {passive: false});
    document.addEventListener('pointerup', e => finish(e));
    document.addEventListener('pointercancel', e => finish(e, true));
    document.addEventListener('click', e => { if (Date.now() < W.suppress) { e.preventDefault(); e.stopImmediatePropagation(); } }, true);
    document.addEventListener('keydown', e => {
      if (e.key === 'Escape' && W.gesture) { finish({pointerId: W.gesture.pointer}, true); e.stopPropagation(); return; }
      keyResize(e);
    }, true);
    const area = document.getElementById('term-area');
    if (area && window.ResizeObserver) new ResizeObserver(() => requestAnimationFrame(() => layout())).observe(area);
    window.addEventListener('resize', () => requestAnimationFrame(() => layout()));
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init); else init();

  return {edgeFor, outerEdge, measure, previewRect, adopt, stripEntries, isSelected, visibleTabs, render, layout,
          get doc() { return W.doc; }, get revision() { return W.revision; }};
});
