#!/usr/bin/env node
// Exporta del mockup aprobado (rama prototype/analytics-grill) los datos de ejemplo con la
// forma de GET /analytics/week y el HTML exacto que pinta cada caso. Los tests de paridad
// comparan dash/analytics.js contra estos archivos. Uso:
//   git show prototype/analytics-grill:dash/prototypes/prototype-analytics.html > /tmp/proto.html
//   node tools/analytics_fixture.cjs /tmp/proto.html tests/fixtures/analytics
// Casos: normal en semana actual y anterior, escritorio y celular; pesada, vacío y límite en escritorio.
const fs = require('fs'), vm = require('vm'), path = require('path');
const [src, outDir] = process.argv.slice(2);
const html = fs.readFileSync(src, 'utf8');
const js = html.slice(html.indexOf('<script>') + 8, html.lastIndexOf('</script>'));
const store = {};
const el = id => (store[id] ||= { id, html: '', style: {}, classList: { toggle() {} }, set innerHTML(v) { this.html = v; }, get innerHTML() { return this.html; },
  querySelectorAll: () => [], querySelector: () => el('x'), set onclick(_) {}, set textContent(_) {}, addEventListener() {}, getBoundingClientRect: () => ({}) });
const doc = { getElementById: el, querySelector: s => el(s), querySelectorAll: () => [], addEventListener() {} };
const ctx = { document: doc, localStorage: { getItem: () => null, setItem() {} }, addEventListener() {}, innerWidth: 1300, console, structuredClone };
vm.createContext(ctx);
vm.runInContext(js + `
;globalThis.__api = {
  model(st, off) {
    state = st; setWeek(off); const id = x => x;
    return { week: { offset: off, label: weekLabel(), start: DAYS.at(-1)[0], end: DAYS[0][0], today: off ? null : TODAYD, now: off ? null : NOWH, measuredAt: '08:16' },
      days: DAYS.map(x => [...x]),
      accounts: accs().map(a => ({ id: a.id, provider: a.provider, cli: a.cli, alias: a.alias, color: a.c, week: a.week, weekUsed: a.weekUsed === undefined ? a.week : a.weekUsed,
        model: a.model ? { n: a.model.n, v: a.model.v } : null, h5: a.h5, reset: a.reset, left: a.left,
        h5Reset: H5[a.id] ? H5[a.id].reset : null, h5Left: H5[a.id] ? H5[a.id].left : null, hoy: a.hoy, sem: a.sem })),
      sessions: sessions().map(s => ({ d: s.d, acc: s.acc, proj: s.proj, st: s.st, en: s.en, tok: (s.en - s.st) * TOKH[s.acc] })),
      lastWeek: LAST, waste: WASTE,
      pomodoros: focusAll().map(f => ({ d: f.d, st: f.st, en: f.en, plan: f.plan, act: f.act, pause: f.pause, status: f.status === 'completado' ? 'completed' : 'cancelled', proj: f.proj })) };
  },
  html(st, off, t, ph) { state = st; setWeek(off); tab = t; phone = ph; phoneDay = 5; draw(); return document.getElementById('modal').innerHTML; },
};`, ctx);
fs.mkdirSync(outDir, { recursive: true });
const ref = {};
const CASES = [['normal', 0, [false, true]], ['normal', -1, [false, true]], ['pesada', 0, [false]], ['vacio', 0, [false]], ['limite', 0, [false]]];
for (const [st, off, phones] of CASES) {
  const name = `week-${st}${off ? '-prev' : ''}`;
  fs.writeFileSync(path.join(outDir, name + '.json'), JSON.stringify(ctx.__api.model(st, off), null, 1) + '\n');
  for (const t of ['cuentas', 'comparar', 'pomodoro']) for (const ph of phones) ref[`${name}|${t}|${ph ? 'phone' : 'desk'}`] = ctx.__api.html(st, off, t, ph);
}
fs.writeFileSync(path.join(outDir, 'reference.json'), JSON.stringify(ref, null, 1) + '\n');
console.log('casos:', Object.keys(ref).length);
