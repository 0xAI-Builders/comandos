// La capa del tablero sobre el mockup: datos por semana, pestañas, flechas, celular y +N.
const assert = require('assert');
const fs = require('fs');
const path = require('path');
const { mkRoot } = require('./dom_stub.cjs');
const render = require('../dash/analytics-render.js');
const { create, tabName } = require('../dash/analytics.js');

const fx = name => JSON.parse(fs.readFileSync(path.join(__dirname, 'fixtures', 'analytics', name + '.json'), 'utf8'));
const ref = JSON.parse(fs.readFileSync(path.join(__dirname, 'fixtures', 'analytics', 'reference.json'), 'utf8'));
const tail = '<div class="tip" hidden></div><div class="pop" hidden></div>';
globalThis.innerWidth = 1300; globalThis.innerHeight = 900;

(async () => {
  assert.strictEqual(tabName('resumen'), 'cuentas');
  assert.strictEqual(tabName('reparto'), 'cuentas');
  assert.strictEqual(tabName('proveedores'), 'comparar');
  assert.strictEqual(tabName('pomodoro'), 'pomodoro');
  assert.strictEqual(tabName('nada'), 'cuentas');

  const asked = [];
  let width = 1100;
  const el = mkRoot();
  const an = create(el, { render, width: () => width, fetchWeek: async off => { asked.push(off); return fx(off ? 'week-normal-prev' : 'week-normal'); } });

  await an.open('comparar');
  assert.deepStrictEqual(asked, [0]);
  assert.strictEqual(el.innerHTML, ref['week-normal|comparar|desk'] + tail, 'Comparar = mockup');

  el.click('[data-tab="pomodoro"]');
  assert.strictEqual(el.innerHTML, ref['week-normal|pomodoro|desk'] + tail, 'Pomodoro = mockup');
  assert.deepStrictEqual(asked, [0], 'cambiar de pestaña no vuelve a pedir datos');

  el.click('[data-tab="cuentas"]');
  el.click('[data-w="-1"]');
  await an.state.loading;
  assert.deepStrictEqual(asked, [0, -1]);
  assert.strictEqual(el.innerHTML, ref['week-normal-prev|cuentas|desk'] + tail, 'semana anterior = mockup');
  el.click('[data-w="-1"]');
  assert.deepStrictEqual(asked, [0, -1], 'no hay semana más vieja que -1');

  el.click('[data-w="1"]');
  await an.state.loading;
  width = 390;
  an.paint();
  assert.ok(el.classList.contains('phone'), 'angosto = celular');
  assert.strictEqual(el.innerHTML, ref['week-normal|cuentas|phone'] + tail, 'celular = mockup');
  el.click('[data-pd="-1"]');
  assert.ok(el.innerHTML.includes('lun 28 · mar 29 · mié 30'), 'la flecha del calendario muestra días anteriores');

  // Un error de red no borra lo que ya se ve.
  const before = el.innerHTML;
  const bad = create(mkRoot(), { render, width: () => 1100, fetchWeek: async () => { throw new Error('sin red'); } });
  await bad.open();
  assert.ok(bad.state.error.includes('sin red'));
  assert.strictEqual(el.innerHTML, before);

  console.log('analytics ui: ok');
})().catch(e => { console.error(e); process.exit(1); });
