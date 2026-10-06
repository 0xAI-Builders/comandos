/* Pure docking operations; mirror of lib/workspace_layout.py.
   Driven by tests/fixtures/workspace_layout.json in both languages. */
(function(root, factory) {
  const api = factory();
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.WorkspaceLayout = api;
})(typeof self !== 'undefined' ? self : this, function() {
  const EDGES = {left: ['x', true], right: ['x', false], top: ['y', true], bottom: ['y', false]};
  const MIN_RATIO = 0.1, MAX_RATIO = 0.9;
  const clone = value => JSON.parse(JSON.stringify(value));
  const fail = message => { throw new Error(message); };

  function tabIds(node) {
    return node.type === 'tab' ? [node.tabId] : [...tabIds(node.first), ...tabIds(node.second)];
  }
  function remove(node, ids) {
    if (node.type === 'tab') return ids.has(node.tabId) ? null : node;
    const first = remove(node.first, ids), second = remove(node.second, ids);
    if (!first) return second;
    if (!second) return first;
    return {...node, first, second};
  }
  function replaceLeaf(node, tabId, next) {
    if (node.type === 'tab') return node.tabId === tabId ? next : node;
    return {...node, first: replaceLeaf(node.first, tabId, next), second: replaceLeaf(node.second, tabId, next)};
  }
  function findLeaf(node, tabId) {
    if (node.type === 'tab') return node.tabId === tabId ? node : null;
    return findLeaf(node.first, tabId) || findLeaf(node.second, tabId);
  }
  function source(doc, src) {
    if (src.startsWith('group:')) {
      const g = doc.groups.find(x => x.id === src.slice(6));
      return g ? g.tree : fail('Grupo de origen inexistente');
    }
    for (const g of doc.groups) { const leaf = findLeaf(g.tree, src); if (leaf) return leaf; }
    return fail('Tab de origen inexistente');
  }
  function without(doc, ids) {
    return doc.groups.map(g => ({...g, tree: remove(g.tree, ids)})).filter(g => g.tree);
  }
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

  function moveTab(document, src, targetId, edge) {
    if (!EDGES[edge]) fail('Borde inválido');
    const doc = clone(document), moved = source(doc, src), ids = new Set(tabIds(moved));
    const groups = without(doc, ids), [axis, before] = EDGES[edge];
    const outer = targetId.startsWith('group:');
    for (let i = 0; i < groups.length; i++) {
      const g = groups[i];
      const kept = outer ? (g.id === targetId.slice(6) ? g.tree : null) : findLeaf(g.tree, targetId);
      if (!kept) continue;
      const split = {type: 'split', axis, ratio: 0.5, first: before ? moved : kept, second: before ? kept : moved};
      groups[i] = {...g, tree: outer ? split : replaceLeaf(g.tree, targetId, split)};
      const out = {...doc, groups};
      if (same(out, document)) fail('Sin cambios');
      return out;
    }
    return fail('Destino inexistente o dentro de lo que se mueve');
  }

  function newGroupId(groups, tabId) {
    const used = new Set(groups.map(g => g.id));
    let id = 'group-' + tabId, n = 1;
    while (used.has(id)) id = `group-${tabId}-${++n}`;
    return id;
  }

  function detachTab(document, src, index) {
    if (!Number.isInteger(index) || index < 0) fail('Posición inválida');
    const doc = clone(document);
    const before = index < doc.groups.length ? doc.groups[index].id : null;
    let entry, groups;
    if (src.startsWith('group:')) {
      entry = doc.groups.find(g => g.id === src.slice(6)) || fail('Grupo de origen inexistente');
      groups = doc.groups.filter(g => g !== entry);
    } else {
      const moved = source(doc, src), owner = doc.groups.find(g => findLeaf(g.tree, src));
      if (owner.tree === moved) { entry = owner; groups = doc.groups.filter(g => g !== owner); }
      else { groups = without(doc, new Set([src])); entry = {id: newGroupId(doc.groups, src), tree: moved}; }
    }
    let at = groups.findIndex(g => g.id === before);
    if (at < 0) at = groups.length;
    groups.splice(at, 0, entry);
    const out = {...doc, groups};
    if (same(out, document)) fail('Sin cambios');
    return out;
  }

  function resizeSplit(document, groupId, path, ratio) {
    if (typeof ratio !== 'number' || !Number.isFinite(ratio)) fail('Proporción inválida');
    const doc = clone(document), g = doc.groups.find(x => x.id === groupId) || fail('Grupo inexistente');
    let node = g.tree;
    for (const step of path) {
      if (!['first', 'second'].includes(step) || node.type !== 'split') fail('Ruta inválida');
      node = node[step];
    }
    if (node.type !== 'split') fail('La ruta no es una división');
    node.ratio = Math.min(MAX_RATIO, Math.max(MIN_RATIO, ratio));
    return doc;
  }

  return {tabIds, moveTab, detachTab, resizeSplit, EDGES, MIN_RATIO, MAX_RATIO};
});
