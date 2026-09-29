// Pure geometry of the web docking renderer (no browser).
const assert = require('node:assert/strict');
const path = require('node:path');
const dock = require(path.join(__dirname, '..', 'dash/workspace-dock.js'));
const rect = {left: 0, top: 0, right: 200, bottom: 100, width: 200, height: 100};
assert.equal(dock.edgeFor(rect, 10, 50), 'left');
assert.equal(dock.edgeFor(rect, 195, 50), 'right');
assert.equal(dock.edgeFor(rect, 100, 5), 'top');
assert.equal(dock.edgeFor(rect, 100, 50), null, 'centre is not a drop target');
assert.equal(dock.edgeFor(rect, 300, 50), null, 'outside the rect');
assert.equal(dock.outerEdge(rect, 3, 50), 'left');
assert.equal(dock.outerEdge(rect, 100, 50), null);
assert.deepEqual(dock.previewRect(rect, 'right'), {left: 100, top: 0, width: 100, height: 100});
assert.deepEqual(dock.previewRect(rect, 'top'), {left: 0, top: 0, width: 200, height: 50});
const leaf = t => ({type: 'tab', tabId: t});
const tree = {type: 'split', axis: 'x', ratio: 0.5, first: leaf('a'),
  second: {type: 'split', axis: 'y', ratio: 0.5, first: leaf('b'), second: leaf('c')}};
const wide = dock.measure(tree, 1400);
assert.equal(wide.axis, 'x');
assert.equal(wide.b.axis, 'y');
const narrow = dock.measure(tree, 390);
assert.equal(narrow.axis, 'y', 'horizontal splits stack on phones');
assert.ok(narrow.height >= 3 * 440, 'phone-stacked tabs leave room for the touch toolbar and scroll');
assert.equal(narrow.a.height, 440);
assert.equal(narrow.b.height, 880, 'rows are shared by measured height, not halves');
assert.equal(dock.measure({type: 'tab', tabId: 'a'}, 1400).height, 240);
console.log('workspace dock geometry checks passed');
