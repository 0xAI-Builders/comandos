// Runs the shared docking fixtures against dash/workspace-layout.js (no browser).
const assert = require('node:assert/strict');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const layout = require(path.join(root, 'dash/workspace-layout.js'));
const cases = require(path.join(root, 'tests/fixtures/workspace_layout.json'));
const ops = {move: layout.moveTab, detach: layout.detachTab, resize: layout.resizeSplit};
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
  passed++;
}
console.log(`${passed}/${cases.length} workspace layout fixtures passed`);
