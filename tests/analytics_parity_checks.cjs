// Paridad pixel perfect: dash/analytics-render.js debe pintar EXACTAMENTE el HTML del mockup
// aprobado para los mismos datos. Las referencias salen de tools/analytics_fixture.cjs.
const assert = require('assert');
const fs = require('fs');
const path = require('path');
const { create } = require('../dash/analytics-render.js');
const dir = path.join(__dirname, 'fixtures', 'analytics');
const ref = JSON.parse(fs.readFileSync(path.join(dir, 'reference.json'), 'utf8'));
let n = 0;
for (const [key, want] of Object.entries(ref)) {
  const [week, tab, mode] = key.split('|');
  const model = JSON.parse(fs.readFileSync(path.join(dir, week + '.json'), 'utf8'));
  const got = create(model, { phone: mode === 'phone' }).html(tab);
  if (got !== want) {
    const i = [...got].findIndex((c, k) => c !== want[k]);
    assert.fail(`${key}: difiere en el carácter ${i}\n  mockup: ${want.slice(Math.max(0, i - 80), i + 80)}\n  tablero: ${got.slice(Math.max(0, i - 80), i + 80)}`);
  }
  n++;
}
assert.ok(n >= 21, `solo ${n} casos`);
console.log(`analytics parity: ${n} casos idénticos al mockup`);
