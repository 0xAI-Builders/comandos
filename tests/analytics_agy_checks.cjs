// 2-oct: agy (Antigravity) es una cuenta más en Analytics. Sin logo propio el dibujo tronaba
// (LOGO[provider] indefinido) y Analytics se quedaba en «Leyendo el uso…».
const assert = require('assert');
const fs = require('fs');
const path = require('path');
const render = require('../dash/analytics-render.js');
globalThis.innerWidth = 1300; globalThis.innerHeight = 900;

const week = JSON.parse(fs.readFileSync(path.join(__dirname, 'fixtures', 'analytics', 'week-normal.json'), 'utf8'));
week.accounts.push({
  id: 'agy:main', provider: 'agy', cli: 'Antigravity', alias: 'main', color: '#5BD6A0',
  week: 26, model: { n: 'Claude·GPT', v: 20, reset: 'vie 9 oct, 10:35', left: '6d 23h' },
  h5: 10, reset: 'mié 7 oct, 11:21', left: '5d 0h', h5Reset: 'vie 2 oct, 12:48', h5Left: '2h 13m',
  hoy: { h: 0, tok: 0, ses: 0 }, sem: { h: 0, tok: 0, ses: 0 }, weekUsed: 26,
});
for (const phone of [false, true]) {
  const R = render.create(week, { phone });
  for (const tab of ['cuentas', 'comparar', 'pomodoro']) {
    const html = R.html(tab);
    assert.ok(!/NaN|undefined|Infinity/.test(html), `${tab}${phone ? ' (celular)' : ''} sin NaN/undefined`);
  }
  const cuentas = R.html('cuentas');
  assert.ok(cuentas.includes('<b>Antigravity</b><small>main</small>'), 'agy tiene su placa en Cuentas');
  for (const cap of ['<span>Semana</span><b class="">74%</b>', '<span>Claude·GPT</span><b class="">80%</b>', '<span>Sesión 5 h</span><b class="">90%</b>'])
    assert.ok(cuentas.includes(cap), cap);
  assert.ok(R.shelf().includes('<b>Antigravity</b>'), 'y en la repisa de la columna izquierda');
}
console.log('agy en Analytics: ok');
