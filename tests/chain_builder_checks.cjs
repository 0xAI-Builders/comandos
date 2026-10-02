// node tests/chain_builder_checks.cjs — desde la raíz del checkout.
// Checks de comportamiento de dash/chain-builder.js sin navegador. Carga el
// dash/command-sidebar.js real (filas y acordeón por CLI) y el DOM stub de
// tests/dom_stub.cjs. Los eventos no burbujean: se despachan sobre el fondo,
// donde el módulo delega, igual que el navegador.
const assert = require('node:assert/strict');
const fs = require('node:fs');
require(process.cwd() + '/dash/command-sidebar.js');
const { createChainBuilder } = require(process.cwd() + '/dash/chain-builder.js');
const { doc } = require('./dom_stub.cjs');

const catalog = JSON.parse(fs.readFileSync(process.cwd() + '/tests/fixtures/command-catalog.json'));
const YOLO = 'codex --dangerously-bypass-approvals-and-sandbox';
const MODAL = '.backdrop[data-mclose] .modal.chain-only';
const tick = () => new Promise(r => setImmediate(r));

(async () => {
  const calls = [], toasts = [], saved = [];
  let mode = 'ok', release = null;
  const chainsList = [
    { slug: 'yolo', name: 'Codex yolo', steps: [{ kind: 'shell', text: YOLO }, { kind: 'pane', text: '/model' }] },
    { slug: 'rota', name: 'rota', error: 'Paso inválido: - foo: bar' },
  ];
  const api = async (path, body) => {
    calls.push([path, body]);
    if (path !== '/chains') throw new Error('ruta inesperada ' + path);
    if (mode === 'hold') await new Promise(r => { release = r; });
    if (mode === 'fail') throw new Error('No se pudo guardar la cadena: OSError');
    return { ok: true, chain: { slug: body.slug || 'mi-cadena', name: body.name, steps: body.steps } };
  };
  let here = 'codex';
  const hydrated = [];
  const b = createChainBuilder({ hydrate: el => hydrated.push(el), api, root: doc.body, catalog: () => catalog, chains: () => chainsList, here: () => here,
    onSaved: (chain, o) => { saved.push([chain, o]); }, toast: (m, e) => toasts.push([m, e]) });
  const bd = () => doc.body.querySelector('.backdrop[data-mclose]');
  const q = sel => bd().querySelector(sel);
  const qa = sel => bd().querySelectorAll(sel);
  const click = sel => bd().dispatch('click', q(sel));
  const stepsText = () => qa(MODAL + ' .slots .step code').map(c => c.textContent);
  const dt = () => { const d = {}; return { setData: (t, v) => { d[t] = v; }, getData: t => d[t] ?? '', effectAllowed: '', dropEffect: '' }; };

  // 1) abrir: modal con acordeón de los 5 CLIs, filas en modo «construir», nombre enfocado, sin llamadas
  assert.equal(bd(), null);
  b.open();
  assert.ok(q(MODAL), 'fondo > modal.chain-only');
  assert.ok(q(MODAL + ' .m-head'));
  // los iconos de la cabecera (lupa, snippet) son SVG hidratados, no el sprite pixel .ic
  assert.ok(q(MODAL + ' .m-head .hic[data-icon="snippet"]') && q(MODAL + ' .m-head .search .hic[data-icon="search"]'));
  assert.equal(q(MODAL + ' .m-head .ic'), null);
  assert.ok(hydrated.some(el => el.querySelector && el.querySelector('.m-head .hic[data-icon="search"]')), 'el modal entero se hidrata');
  assert.equal(qa('.cs-cli').length, 5);
  assert.equal(qa('.cmd[data-cmd]').length, 0);                       // nada de data-cmd: no teclea
  assert.ok(qa('.cmd[data-add]').length > 20);
  assert.equal(q('.cs-cli[data-cli="codex"] .cmd .add').textContent, '+ cadena');
  assert.equal(qa('.cmd.dis').length, 0);                              // construir no exige destino ni CLI en el pane
  // el CLI del pane abre solo y se marca (mockup aprobado); los demás plegados, sin subtítulos de grupo
  assert.equal(q('.cs-cli[data-cli="codex"]').classList.contains('here'), true);
  assert.equal(q('.cs-cli[data-cli="codex"]').classList.contains('open'), true);
  assert.equal(q('.cs-cli[data-cli="codex"] .srows .cmd.y').dataset.add, 'codex --dangerously-bypass-approvals-and-sandbox');
  assert.equal(q('.cs-cli[data-cli="claude"]').classList.contains('open'), false);
  assert.equal(qa('.grp').length, 0);
  assert.equal(doc.activeElement, q('.m-name'));
  assert.equal(calls.length, 0);
  assert.equal(b.state.name, ''); assert.deepEqual(b.state.steps, []); assert.equal(b.state.dragging, null);
  assert.equal(q('.slots .step'), null);

  // 2) plegar y reabrir el CLI Codex (todo es plegable, también el del pane) y añadir:
  //    un clic añade un paso (kind por fila) y no teclea nada
  click('.cs-cli[data-cli="codex"] .cli-h');
  assert.equal(q('.cs-cli[data-cli="codex"]').classList.contains('open'), false);
  click('.cs-cli[data-cli="codex"] .cli-h');
  assert.equal(q('.cs-cli[data-cli="codex"]').classList.contains('open'), true);
  click('.cs-cli[data-cli="codex"] .srows .cmd.y');
  bd().dispatch('click', q('.cs-cli[data-cli="codex"] .cmds .cmd[data-add="/model"]'));   // /model
  bd().dispatch('click', q('.cs-cli[data-cli="claude"] .cmd[data-add="/model "] .opts button'));  // chip: se abre solo con el arranque claude
  bd().dispatch('click', q('.cs-cli[data-cli="claude"] .cmds .cmd[data-add="/model "] .add')); // «+ cadena» de /model  (con espacio final)
  assert.deepEqual(b.state.steps.map(s => [s.kind, s.text]), [
    ['shell', YOLO], ['pane', '/model'], ['pane', '/model claude-fable-5-1'], ['pane', '/model ']]);
  assert.equal(calls.length, 0);                                       // ni API ni /pane/type
  assert.equal(qa(MODAL + ' .slots .step').length, 4);
  assert.deepEqual(qa(MODAL + ' .slots .step').map(s => s.dataset.i), ['0', '1', '2', '3']);
  assert.equal(qa(MODAL + ' .slots .step').every(s => s.getAttribute('draggable') === 'true'), true);
  assert.equal(q('.slots .step[data-i="0"]').dataset.kind, 'shell');
  assert.equal(q('.slots .step[data-i="0"] code').textContent, YOLO);
  click('.slots .step[data-i="3"] [data-del]'); click('.slots .step[data-i="2"] [data-del]');   // ✕: quitar los dos últimos
  assert.deepEqual(stepsText(), [YOLO, '/model']);
  click('.cs-cli[data-cli="codex"] .cli-h'); assert.equal(q('.cs-cli[data-cli="codex"]').classList.contains('open'), false);

  // 3) reordenar: arrastrar 1 → 0 sobre .slots, dragend limpia; teclado con ← →
  const d1 = dt();
  bd().dispatch('dragstart', q('.slots .step[data-i="1"]'), { dataTransfer: d1 });
  assert.equal(b.state.dragging, 1);
  let prevented = 0;
  bd().dispatch('dragover', q('.slots .step[data-i="0"]'), { dataTransfer: d1, preventDefault() { prevented++; } });
  assert.equal(prevented, 1);
  assert.equal(q('.slots .step[data-i="0"]').classList.contains('over'), true);
  assert.equal(q('.slots .step[data-i="1"]').classList.contains('dragging'), true);
  bd().dispatch('drop', q('.slots .step[data-i="0"]'), { dataTransfer: d1 });
  assert.equal(b.state.dragging, null);
  assert.deepEqual(stepsText(), ['/model', YOLO]);
  bd().dispatch('drop', q('.slots .step[data-i="0"]'), { dataTransfer: { getData: () => '1' } });   // sin dragstart previo: sin cambios
  assert.deepEqual(stepsText(), ['/model', YOLO]);
  const d2 = dt();                                                      // soltar en hueco de .slots → al final
  bd().dispatch('dragstart', q('.slots .step[data-i="0"]'), { dataTransfer: d2 });
  bd().dispatch('drop', q('.slots'), { dataTransfer: d2 });
  assert.deepEqual(stepsText(), [YOLO, '/model']);
  bd().dispatch('dragstart', q('.slots .step[data-i="0"]'), { dataTransfer: dt() });   // cancelado
  assert.equal(q('.slots .step[data-i="0"]').classList.contains('dragging'), true);
  bd().dispatch('dragend', q('.slots .step[data-i="0"]'), {});
  assert.equal(b.state.dragging, null);
  assert.equal(qa('.slots .step.dragging').length + qa('.slots .step.over').length, 0);
  click('.slots .step[data-i="0"] [data-move="1"]');                    // teclado: bajar el primero
  assert.deepEqual(stepsText(), ['/model', YOLO]);
  assert.equal(q('.slots .step[data-i="0"] [data-move="-1"]').hasAttribute('disabled'), true);
  assert.equal(q('.slots .step[data-i="1"] [data-move="1"]').hasAttribute('disabled'), true);
  click('.slots .step[data-i="1"] [data-move="-1"]');
  assert.deepEqual(stepsText(), [YOLO, '/model']);

  // 4) validación en cliente: sin nombre / sin pasos no llaman a la API y pintan .m-error
  click('[data-save]');
  assert.equal(calls.length, 0);
  assert.match(q('.m-error').textContent, /nombre/i);
  assert.equal(q(MODAL) !== null, true);
  q('.m-name').value = '  Mi cadena  '; bd().dispatch('input', q('.m-name'), {});
  assert.equal(b.state.name, '  Mi cadena  ');
  assert.equal(q('.m-error'), null);                                    // escribir limpia el error
  click('.slots .step[data-i="1"] [data-del]'); click('.slots .step[data-i="0"] [data-del]');
  assert.deepEqual(b.state.steps, []); assert.equal(qa('.slots .slot.empty').length, 6);
  click('[data-run]');
  assert.equal(calls.length, 0); assert.match(q('.m-error').textContent, /paso/i);

  // 5) Guardar: POST /chains {name, steps} (sin slug), cierra y onSaved(chain, {run:false})
  bd().dispatch('click', q('.cs-cli[data-cli="codex"] .srows .cmd.y'));
  bd().dispatch('click', q('.cs-cli[data-cli="codex"] .cmds .cmd[data-add="/model"]'));
  click('[data-save]'); await tick(); await tick();
  assert.deepEqual(calls, [['/chains', { name: 'Mi cadena', steps: [{ kind: 'shell', text: YOLO }, { kind: 'pane', text: '/model' }] }]]);
  assert.equal(bd(), null);
  assert.equal(doc.listenerCount('keydown'), 0);
  assert.equal(saved.length, 1);
  assert.equal(saved[0][0].slug, 'mi-cadena'); assert.deepEqual(saved[0][1], { run: false });

  // 6) Correr: guarda y avisa con {run: true}; error del servidor se queda en el modal
  calls.length = 0; b.open();
  q('.m-name').value = 'otra'; bd().dispatch('input', q('.m-name'), {});
  bd().dispatch('click', q('.cs-cli[data-cli="codex"] .cmds .cmd[data-add="/model"]'));
  mode = 'fail'; click('[data-run]'); await tick(); await tick();
  assert.equal(calls.length, 1);
  assert.equal(q('.m-error').textContent, 'No se pudo guardar la cadena: OSError');
  assert.ok(q(MODAL)); assert.equal(saved.length, 1);                   // sigue abierto y sin onSaved
  assert.equal(q('[data-save]').hasAttribute('disabled'), false);
  mode = 'hold'; click('[data-run]'); click('[data-save]'); click('[data-run]');   // doble clic: una sola petición
  await tick(); assert.equal(calls.length, 2);
  assert.equal(q('[data-save]').hasAttribute('disabled'), true);
  release(); await tick(); await tick();
  assert.equal(bd(), null); assert.equal(saved.length, 2);
  assert.deepEqual(saved[1][1], { run: true });
  mode = 'ok';

  // 7) cerrar: solo el fondo (e.target === backdrop), Cerrar y Escape; el modal no traga clics
  b.open();
  bd().dispatch('click', q(MODAL)); bd().dispatch('click', q('.m-head')); bd().dispatch('click', q('.cs-cli .cli-h'));
  assert.ok(bd());
  const bdEl = bd(); bdEl.dispatch('click', bdEl); assert.equal(bd(), null);
  b.open(); click('[data-close]'); assert.equal(bd(), null);
  b.open(); assert.equal(doc.listenerCount('keydown'), 1);
  doc.dispatch('keydown', { key: 'a' }); assert.ok(bd());
  doc.dispatch('keydown', { key: 'Escape' }); assert.equal(bd(), null);
  assert.equal(doc.listenerCount('keydown'), 0);
  b.open(); b.open(); assert.equal(doc.body.querySelectorAll('.backdrop').length, 1);   // reabrir no duplica
  assert.equal(doc.listenerCount('keydown'), 1);
  b.close(); assert.equal(b.close(), false);

  // 8) editar: open(slug) precarga y envía slug; cadenas con error o inexistentes no abren
  calls.length = 0; b.open('yolo');
  assert.equal(q('.m-name').value, 'Codex yolo');
  assert.deepEqual(stepsText(), [YOLO, '/model']);
  click('[data-save]'); await tick(); await tick();
  assert.deepEqual(calls[0][1], { name: 'Codex yolo', slug: 'yolo', steps: [{ kind: 'shell', text: YOLO }, { kind: 'pane', text: '/model' }] });
  chainsList[0].steps.push({ kind: 'pane', text: 'x' });                // precargar copia: editar no muta la lista viva
  b.open('yolo'); b.state.steps.pop(); b.state.steps.pop(); assert.equal(chainsList[0].steps.length, 3); b.close();
  toasts.length = 0;
  assert.equal(b.open('rota'), null); assert.equal(bd(), null);
  assert.equal(b.open('no-existe'), null);
  assert.equal(toasts.length, 2); assert.equal(toasts.every(([m, e]) => /no se puede editar/i.test(m) && e === true), true);
  b.open(); assert.equal(q('.m-name').value, ''); assert.deepEqual(b.state.steps, []);   // abrir sin slug vuelve a cero
  b.close();

  // 9) onSaved que falla: se avisa por toast, el modal ya está cerrado
  {
    const b2 = createChainBuilder({ api, root: doc.body, catalog: () => catalog, chains: () => [],
      onSaved: () => { throw new Error('refresco caído'); }, toast: (m, e) => toasts.push([m, e]) });
    toasts.length = 0; b2.open(); q('.m-name').value = 'x'; bd().dispatch('input', q('.m-name'), {});
    bd().dispatch('click', qa('.cmd[data-add]')[0]); click('[data-save]'); await tick(); await tick();
    assert.equal(bd(), null); assert.deepEqual(toasts, [['refresco caído', true]]);
  }
  // 10) sin catálogo: el modal abre igual y avisa
  {
    const b3 = createChainBuilder({ api, root: doc.body, catalog: () => null, toast: () => {} });
    b3.open(); assert.ok(q('.cb-empty')); assert.equal(qa('.cs-cli').length, 0); b3.close();
  }
  // 11) teclado: el acordeón se abre con Enter/Espacio y aria-expanded lo refleja; sin ratón se puede añadir un paso
  {
    const b4 = createChainBuilder({ api, root: doc.body, catalog: () => catalog, chains: () => [], toast: () => {} });
    b4.open();
    const H = () => q('.cs-cli[data-cli="grok"] .cli-h');
    assert.equal(H().getAttribute('role'), 'button'); assert.equal(H().getAttribute('tabindex'), '0'); assert.equal(H().getAttribute('aria-expanded'), 'false');
    bd().dispatch('keydown', H(), { key: 'Enter' });
    assert.equal(H().getAttribute('aria-expanded'), 'true'); assert.equal(doc.activeElement, H());
    assert.equal(q('.cs-cli[data-cli="grok"] .hsec .hsec-h').getAttribute('aria-expanded'), 'false');
    bd().dispatch('keydown', H(), { key: ' ' }); assert.equal(H().getAttribute('aria-expanded'), 'false');
    bd().dispatch('keydown', H(), { key: 'x' }); assert.equal(H().getAttribute('aria-expanded'), 'false');
    b4.close();
  }
  // 12) guardado en vuelo que sobrevive a close(): no arma el Correr cancelado, no cierra el modal reabierto y no permite un 2.º POST
  {
    const saved5 = [], toasts5 = [], calls5 = [];
    let rel; const gate = new Promise(r => { rel = r; });
    const api5 = async (path, body) => { calls5.push(body); await gate; return { ok: true, chain: { slug: 'x', name: body.name, steps: body.steps } }; };
    const b5 = createChainBuilder({ api: api5, root: doc.body, catalog: () => catalog, chains: () => [], onSaved: (c, o) => { saved5.push([c.slug, o]); }, toast: (m, e) => toasts5.push([m, e]) });
    b5.open(); q('.m-name').value = 'x'; bd().dispatch('input', q('.m-name'), {});
    bd().dispatch('click', qa('.cmd[data-add]')[0]);
    click('[data-run]'); assert.equal(calls5.length, 1);
    b5.close();                                                             // cancela el Correr
    b5.open();                                                              // reabre mientras el POST sigue en vuelo
    assert.equal(q('[data-save]').hasAttribute('disabled'), true);           // botones bloqueados
    q('.m-name').value = 'y'; bd().dispatch('input', q('.m-name'), {});
    bd().dispatch('click', qa('.cmd[data-add]')[0]);
    click('[data-save]'); click('[data-run]'); assert.equal(calls5.length, 1);   // ningún segundo POST
    rel(); await tick(); await tick(); await tick();
    assert.ok(bd());                                                        // el modal reabierto sigue abierto
    assert.deepEqual(saved5, [['x', { run: false }]]);                       // refresca, pero no corre
    assert.equal(q('[data-save]').hasAttribute('disabled'), false);
    b5.close();
    // fallo con el modal ya cerrado: toast (no hay .m-error visible) y sin onSaved
    let rel2; const gate2 = new Promise(r => { rel2 = r; });
    const b6 = createChainBuilder({ api: async () => { await gate2; throw new Error('disco lleno'); }, root: doc.body, catalog: () => catalog, chains: () => [],
      onSaved: () => saved5.push('no'), toast: (m, e) => toasts5.push([m, e]) });
    b6.open(); q('.m-name').value = 'z'; bd().dispatch('input', q('.m-name'), {}); bd().dispatch('click', qa('.cmd[data-add]')[0]);
    click('[data-save]'); b6.close(); rel2(); await tick(); await tick();
    assert.deepEqual(toasts5, [['disco lleno', true]]); assert.equal(saved5.length, 1);
  }
  // 13) añadir un paso inválido usa .m-error (un toast quedaría bajo el modal)
  {
    const t7 = [];
    const b7 = createChainBuilder({ api, root: doc.body, catalog: () => ({ clis: [{ id: 'z', label: 'Z', binary: 'z', version: { status: 'ok' }, start: { command: 'z --help', rows: [{ text: 'a\nb', description: '', args: [] }], yolo: [], sections: [] }, groups: [] }] }), chains: () => [], toast: (m, e) => t7.push([m, e]) });
    b7.open(); bd().dispatch('click', qa('.cmd[data-add]')[0]);
    assert.deepEqual(b7.state.steps, []); assert.match(q('.m-error').textContent, /no se puede añadir/); assert.equal(t7.length, 0);
    b7.close();
  }
  console.log('chain-builder checks ok');
})().catch(e => { console.error(e); process.exit(1); });
