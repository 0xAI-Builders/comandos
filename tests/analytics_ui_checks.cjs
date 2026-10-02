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

  // ---------- Nombres del modelo y sesiones instantáneas ----------
  // Estas comprobaciones no abortan en la primera falla: una corrida en rojo lista todas las que fallan.
  const failures = [];
  const check = async (name, fn) => {
    try { await fn(); } catch (e) { failures.push(name); console.error(`FAIL ${name}\n     ${String(e.message).split('\n').slice(0, 3).join('\n     ')}`); }
  };
  const TABS = ['cuentas', 'comparar', 'pomodoro'];
  const EVIL = '<i id="pwn">x</i>"&\'';
  const EVIL_E = '&lt;i id=&quot;pwn&quot;&gt;x&lt;/i&gt;&quot;&amp;&#39;';
  const where = (tab, phone) => `${tab}/${phone ? 'celular' : 'escritorio'}`;
  // Semana pesada con el nombre hostil en un tercio de las sesiones y de los pomodoros, en lastWeek y en
  // las etiquetas de cuenta (alias, cli, nombre del modelo).
  const hostile = () => {
    const m = fx('week-pesada');
    m.sessions.forEach((s, i) => { if (i % 3 === 0) s.proj = EVIL; });
    m.pomodoros.forEach((f, i) => { if (i % 3 === 0) f.proj = EVIL; });
    m.lastWeek[EVIL] = 400;
    m.accounts[0].alias = EVIL; m.accounts[1].cli = EVIL; m.accounts[0].model.n = EVIL;
    return m;
  };
  // Vista con el módulo de dibujo real; en los dos casos de +N y de error se pasa a mano lo que haga falta.
  const open = async (fetchWeek, extra = {}) => {
    const root = mkRoot();
    const view = create(root, { render, width: () => 1100, fetchWeek, ...extra });
    await view.open('cuentas');
    return { root, view };
  };
  const geometry = (root, btn) => {   // el stub no mide: lo mínimo que usa showPop
    btn.getBoundingClientRect = () => ({ right: 500, top: 120 });
    const pop = root.querySelector('.pop');
    pop.offsetWidth = 220; pop.offsetHeight = 90;
    return pop;
  };

  await check('nombres hostiles: ningún nombre del modelo se vuelve marcado', () => {
    for (const tab of TABS) for (const phone of [false, true]) {
      const w = where(tab, phone), out = render.create(hostile(), { phone }).html(tab);
      assert.ok(!out.includes('<i id="pwn">'), `${w}: aparece <i id="pwn"> sin escapar`);
      assert.ok(!/<i\s+id=/i.test(out), `${w}: aparece un <i id=…> (en mayúsculas o minúsculas)`);
      assert.ok(out.includes(EVIL_E), `${w}: falta el nombre escapado ${EVIL_E}`);
      const dom = mkRoot(); dom.innerHTML = out;
      assert.strictEqual(dom.querySelector('#pwn'), null, `${w}: el nombre se volvió un elemento`);
      const odd = dom.querySelectorAll('[data-tip]').filter(n => n.dataset.tip.split('|').length !== 3);
      assert.strictEqual(odd.length, 0, `${w}: ${odd.length} tooltips con los campos rotos`);
    }
  });

  await check('lastWeek: la clave escapada sigue encontrando al proyecto de esta semana', () => {
    const out = render.create(hostile(), {}).html('comparar');
    assert.ok(out.includes(`<span>${EVIL_E}</span><em>400.0 h → `), 'el proyecto hostil no junta sus horas con las de la semana pasada');
  });

  await check('el id de cuenta hostil tampoco llega al marcado', () => {
    const m = fx('week-pesada'), old = m.accounts[0].id, bad = 'claude:' + EVIL;
    m.accounts[0].id = bad;
    m.sessions.forEach(s => { if (s.acc === old) s.acc = bad; });
    m.waste.forEach(w => { if (w.id === old) w.id = bad; });
    for (const tab of TABS) for (const phone of [false, true]) {
      assert.ok(!/<i\s+id=/i.test(render.create(m, { phone }).html(tab)), `${where(tab, phone)}: el id hostil apareció sin escapar`);
    }
  });

  await check('un 🍅 en un nombre no rompe los atributos (draw() lo cambia por un icono)', () => {
    const m = fx('week-pesada');
    m.sessions.forEach((s, i) => { if (i % 3 === 0) s.proj = 'ab🍅cd'; });
    m.pomodoros.forEach((f, i) => { if (i % 3 === 0) f.proj = 'ab🍅cd'; });
    for (const tab of TABS) for (const phone of [false, true]) {
      const w = where(tab, phone), out = render.create(m, { phone }).html(tab);
      const cut = out.match(/(?:data-tip|aria-label)="[^"]*<span class="tin"/g) || [];
      assert.strictEqual(cut.length, 0, `${w}: ${cut.length} atributos cortados por el icono 🍅`);
      assert.ok(out.includes('ab&#127813;cd'), `${w}: el nombre con 🍅 no sale codificado`);
    }
  });

  await check('tooltip: el nombre hostil se pinta como texto', async () => {
    const { root } = await open(async () => hostile());
    const src = root.querySelectorAll('[data-tip]').find(n => n.dataset.tip.includes(EVIL));
    assert.ok(src, 'ninguna sesión con el nombre hostil en data-tip');
    const tip = root.querySelector('.tip');
    root.dispatch('pointerover', src);
    assert.strictEqual(tip.hidden, false);
    assert.strictEqual(tip.querySelector('i'), null, 'el nombre se volvió un elemento del tooltip');
    assert.ok(tip.textContent.includes(EVIL), `se esperaba ver el nombre tal cual y salió: ${tip.textContent}`);
  });

  await check('+N: los nombres hostiles se pintan como texto, escapados una sola vez', async () => {
    const { root } = await open(async () => hostile());
    const btn = root.querySelectorAll('[data-pop]').find(b => decodeURIComponent(b.dataset.pop).includes(EVIL_E));
    assert.ok(btn, 'ningún +N con el nombre hostil en data-pop');
    const pop = geometry(root, btn);
    root.dispatch('click', btn);
    assert.strictEqual(pop.hidden, false);
    assert.strictEqual(pop.querySelector('i'), null, 'el nombre se volvió un elemento de la ventana');
    assert.ok(pop.textContent.includes(EVIL), `se esperaba ver el nombre tal cual (sin escapar dos veces) y salió: ${pop.textContent}`);
  });

  await check('+N: un renderizador que no escapa tampoco inyecta marcado', async () => {
    const rows = [['19:16–20:13', EVIL, 'Claude ' + EVIL, '#FF9A5C']];
    const fake = { create: () => ({ html: () => `<button class="more" data-pop="${encodeURIComponent(JSON.stringify(rows))}" data-title="${EVIL_E}">+1</button>` }) };
    const { root } = await open(async () => ({}), { render: fake });
    const btn = root.querySelector('[data-pop]');
    const pop = geometry(root, btn);
    root.dispatch('click', btn);
    assert.strictEqual(pop.querySelector('i'), null, 'el nombre se volvió un elemento de la ventana');
    assert.ok(pop.textContent.includes(EVIL), `se esperaba ver el nombre tal cual y salió: ${pop.textContent}`);
  });

  await check('el mensaje de error hostil se pinta como texto', async () => {
    const root = mkRoot();
    const view = create(root, { render, width: () => 1100, fetchWeek: async () => { throw new Error(EVIL); } });
    await view.open();
    assert.ok(!root.innerHTML.includes('<i id="pwn">'), 'el mensaje de error entró como marcado');
    assert.ok(root.innerHTML.includes(EVIL_E), 'falta el mensaje escapado');
    assert.strictEqual(root.querySelector('#pwn'), null);
    assert.ok(root.textContent.includes(EVIL), `se esperaba ver el mensaje tal cual y salió: ${root.textContent}`);
  });

  // Ninguna sesión mide cero: el mockup divide entre las horas de la cuenta y 0/0 daba NaN.
  const clean = (m, label) => {
    for (const tab of TABS) for (const phone of [false, true]) {
      const out = render.create(m, { phone }).html(tab), hit = out.match(/NaN|Infinity|undefined/);
      assert.ok(!hit, `${label} ${where(tab, phone)}: aparece "${hit && hit[0]}" cerca de «${hit && out.slice(Math.max(0, hit.index - 40), hit.index + 30).replace(/\s+/g, ' ')}»`);
    }
  };
  const instant = (m, only) => { m.sessions.forEach(s => { if (!only || s.acc === only) s.en = s.st; }); return m; };

  await check('instantáneas: todas las sesiones de una cuenta duran cero', () => clean(instant(fx('week-normal'), 'codex'), 'una cuenta'));
  await check('instantáneas: la única cuenta tiene solo sesiones de cero', () => {
    const m = fx('week-normal'), keep = 'main';
    m.accounts = m.accounts.filter(a => a.id === keep);
    m.sessions = m.sessions.filter(s => s.acc === keep);
    m.waste = m.waste.filter(w => w.id === keep);
    clean(instant(m), 'única cuenta');
  });
  await check('instantáneas: todas las cuentas', () => clean(instant(fx('week-normal')), 'todas'));

  if (failures.length) { console.error(`analytics ui: fallaron ${failures.length} comprobaciones`); process.exit(1); }
  console.log('analytics ui: ok');
})().catch(e => { console.error(e); process.exit(1); });
