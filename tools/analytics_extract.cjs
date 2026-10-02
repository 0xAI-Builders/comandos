#!/usr/bin/env node
// Genera dash/analytics-render.js copiando TAL CUAL las piezas de dibujo del mockup aprobado
// (rama prototype/analytics-grill). Así el tablero pinta el mismo HTML que el mockup.
// Uso: git show prototype/analytics-grill:dash/prototypes/prototype-analytics.html > /tmp/proto.html
//      node tools/analytics_extract.cjs /tmp/proto.html dash/analytics-render.js
const fs = require('fs');
const [src, out] = process.argv.slice(2);
const html = fs.readFileSync(src, 'utf8');
const js = html.slice(html.indexOf('<script>') + 8, html.lastIndexOf('</script>'));
// Orden del mockup. Cada nombre es una declaración de primer nivel (const/let/function).
const NAMES = ['A_', 'fmtH', 'dur', 'defs', 'liquid', 'glass', 'floor', 'LOGO', 'limits', 'logoOf', 'liqC', 'colorLogo',
  'logoTile', 'brandBottle', 'capt', 'barShelf', 'dlab', 'accName', 'projTot', 'dayTot', 'weekHead', 'merged', 'lanes', 'tip2',
  'compAxis', 'calHead', 'axisHtml', 'gapBands', 'nowY', 'K1', 'usedFor', 'costData', 'fmtTok', 'franjas', 'vsWeek', 'sgn',
  'pHour', 'layeredBottle', 'wasteRest', 'insightCards', 'bottlesShelf', 'head2', 'C1', 'fmin', 'fstats', 'fByProj', 'fByHour',
  'PCOL', 'projColor', 'tomato', 'hh', 'pnums', 'pPerHour', 'pPerProj', 'S5', 'calendarCuentas', 'pomodoroPhone'];
const starts = [...js.matchAll(/^(?:const|let|function)\s+([A-Za-z_$][\w$]*)/gm)];
const decl = {};
starts.forEach((m, i) => {
  const end = i + 1 < starts.length ? starts[i + 1].index : js.length;
  // Quita el comentario de sección que precede a la siguiente declaración.
  const body = js.slice(m.index, end).replace(/(\n\/\*[^\n]*\*\/\s*)+$/, '\n').trimEnd();
  if (decl[m[1]]) throw new Error(`declaración repetida en el mockup: ${m[1]}`);
  decl[m[1]] = body;
});
const missing = NAMES.filter(n => !decl[n]);
if (missing.length) throw new Error('faltan en el mockup: ' + missing.join(', '));
const ADAPTER = `
  // Frontera con el modelo. El mockup dibujaba datos fijos y de confianza; aquí cada nombre que llega del modelo
  // (carpetas, alias de cuenta, nombre del modelo, proyectos de Pomodoro) se escapa antes de que lo vea el código copiado.
  // 🍅 va como entidad: el shell cambia el carácter por un icono y no debe tocar atributos.
  const esc = s => String(s).replace(/[&<>"']|🍅/gu, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;','🍅':'&#127813;'}[c]));
  let uid = 0, phoneDay = 0;
  const ACC = AN.accounts.map(a => ({ ...a, c: a.color, cli: esc(a.cli), alias: esc(a.alias), model: a.model && { ...a.model, n: esc(a.model.n) } }));
  const accs = () => ACC.map(a => ({ ...a, hoy: { ...a.hoy }, sem: { ...a.sem }, model: a.model && { ...a.model } }));
  const DAYS = AN.days;
  const CHRONO = DAYS.slice().reverse();
  // Ninguna sesión mide cero: el mockup divide entre las horas de la cuenta y 0/0 daba NaN.
  const SESS = AN.sessions.map(s => ({ ...s, proj: esc(s.proj), en: Math.min(24, Math.max(s.en, s.st + 1 / 60)) }));
  const sessions = () => SESS;
  const LAST = Object.fromEntries(Object.entries(AN.lastWeek).map(([p, h]) => [esc(p), h]));
  const WASTE = AN.waste;
  // Toda cuenta con 5 h tiene entrada: sin ventana abierta (0 % y sin reset) se dibuja sin hora de reset.
  const H5 = Object.fromEntries(ACC.filter(a => a.h5 != null).map(a => [a.id, { left: a.h5Left ?? null, reset: a.h5Reset ?? null }]));
  const FOCUS = AN.pomodoros.map(f => ({ ...f, proj: esc(f.proj), status: f.status === 'completed' ? 'completado' : 'cancelado', ag: {} }));
  const focusAll = () => FOCUS;
  const TODAYD = AN.week.today, NOWH = AN.week.now;
  const isPast = () => AN.week.offset < 0;
  const weekLabel = () => AN.week.label;
  const isPhone = () => !!view.phone;
`;
const SHELL = `
  const TABS = [['cuentas', 'Cuentas'], ['comparar', 'Comparar'], ['pomodoro', 'Pomodoro']];
  function body(tab) {
    const as = accs();
    if (tab === 'cuentas') return (isPast() ? '<div class="wk-note" style="margin:0 0 8px">Las botellas muestran tu cuota de ahora; el calendario es de la semana que elegiste.</div>' : '') + barShelf(as.filter(a => limits(a).length)) + \`<div class="sect"><h3>\${isPast() ? 'Semana' : 'Esta semana'}</h3><span>\${weekLabel()}</span></div>\` + calendarCuentas();
    if (tab === 'comparar') return C1();
    return isPhone() ? pomodoroPhone() : S5();
  }
  // Mismo marcado que draw() del mockup: cabecera con pestañas y flechas de semana, luego el cuerpo.
  function html(tab) {
    uid = 0;
    phoneDay = Math.max(0, Math.min(CHRONO.length - 3, view.phoneDay ?? CHRONO.length - 3));
    const off = AN.week.offset;
    const out = \`<div class="mhead"><h2>Analytics</h2><div class="tabs" role="tablist">\${TABS.map(([k, l]) => \`<button class="tab \${k === tab ? 'on' : ''}" role="tab" aria-selected="\${k === tab}" data-tab="\${k}">\${l}</button>\`).join('')}</div>
  <div class="wnav"><button data-w="-1" \${off <= (view.minOffset ?? -1) ? 'disabled' : ''} aria-label="Semana anterior">←</button><span>\${off ? 'Semana' : 'Esta semana'} · <b>\${weekLabel()}</b></span><button data-w="1" \${off >= 0 ? 'disabled' : ''} aria-label="Semana siguiente">→</button></div></div>\${body(tab)}\`;
    return out.replaceAll('🍅', \`<span class="tin">\${tomato(true, 16)}</span>\`);
  }
  // La repisa de botellas sola (límites de cada cuenta), para la columna izquierda de ComandOS.
  const shelf = () => barShelf(accs().filter(a => limits(a).length));
  return { html, shelf, phoneDays: () => CHRONO.length };
`;
const header = '/* GENERADO por tools/analytics_extract.cjs desde el mockup aprobado (rama prototype/analytics-grill).\n   No editar a mano: cambia el mockup, regenera y actualiza tests/fixtures/analytics. */\n';
const code = `${header}(function (root) {
  function create(AN, view = {}) {
${ADAPTER}
${NAMES.map(n => decl[n]).join('\n')}
${SHELL}
  }
  if (typeof module !== 'undefined' && module.exports) module.exports = { create };
  else root.AnalyticsRender = { create };
})(typeof window !== 'undefined' ? window : globalThis);
`;
fs.writeFileSync(out, code);
console.log(`${out}: ${NAMES.length} piezas, ${code.length} bytes`);
