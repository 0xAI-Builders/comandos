// node tests/command_sidebar_checks.cjs — desde la raíz del checkout.
// Checks de comportamiento de dash/command-sidebar.js sin navegador. El DOM es
// el stub mínimo de tests/dom_stub.cjs (no hay jsdom offline). Los clics se
// emulan con delegación en la raíz.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const mod = require(process.cwd() + '/dash/command-sidebar.js');
const { createCommandSidebar } = mod;

const { mkRoot, doc } = require('./dom_stub.cjs');

// ---------- checks ----------
const tick = () => new Promise(r => setImmediate(r));
const catalog = JSON.parse(fs.readFileSync(process.cwd() + '/tests/fixtures/command-catalog.json'));
(async () => {
  const calls = [], toasts = [];
  const store = new Map(), storage = { getItem: k => store.get(k) ?? null, setItem: (k, v) => store.set(k, v), removeItem: k => store.delete(k) };
  let target = { session: 'demo', pane: '%2', kind: 'pane', title: 'demo %2' }, ids = 0, typeStatus = 200, yoloStep2 = '/model';
  const api = async (path, body) => { calls.push([path, body]);
    if (path.startsWith('/commands/catalog')) return { cliInPane: 'codex', target: { session: 'demo', pane: '%2' }, catalog, versionsAt: 1 };
    if (path === '/chains') return { chains: [{ slug: 'yolo', name: 'Codex yolo', steps: [{ kind: 'shell', text: 'codex --dangerously-bypass-approvals-and-sandbox' }, { kind: 'pane', text: yoloStep2 }] }, { slug: 'rota', name: 'rota', error: 'Paso inválido: - foo: bar' }] };
    if (path === '/pane/type') { if (typeStatus !== 200) { const e = new Error('El pane %2 ya no existe'); e.code = 'pane_gone'; throw e; } return { ok: true, typed: body.text.length, requestId: body.requestId }; }
    throw new Error('ruta inesperada ' + path); };
  const root = mkRoot();
  const sb = createCommandSidebar({ api, root, storage, makeId: () => 'id-' + (++ids), getTarget: () => target, focusTarget: () => {}, openBuilder: () => {}, toast: (m, e) => toasts.push([m, e]), terminals: () => [] });
  await sb.refresh();
  assert.equal(calls[0][0], '/commands/catalog?session=demo&pane=%252');
  // 1) todo CLI arranca cerrado; el del pane va marcado, con yolo primero y plegable
  assert.equal(root.querySelector('.cs-cli.here').dataset.cli, 'codex');
  assert.equal(root.querySelectorAll('.cs-cli.open').length, 0);
  assert.deepEqual(JSON.parse(store.get('comandos.commands.open.v2')), ['saved']);
  // arranques de `codex --help`: primero el binario con su resumen, luego lo que el CLI describe como saltarse permisos
  assert.deepEqual(root.querySelectorAll('.cs-cli.here .srows.top .cmd').map(r => r.dataset.cmd),
    ['codex', 'codex --dangerously-bypass-approvals-and-sandbox']);
  assert.equal(root.querySelector('.cs-cli.here .srows .cmd.y small').textContent.startsWith('Skip all confirmation prompts'), true);
  assert.equal(root.querySelector('.cs-cli.here .srows .cmd.bin small').textContent, 'Codex CLI');
  assert.deepEqual(root.querySelectorAll('.cs-cli.here .hsec .hsec-h .t').map(t => t.textContent), ['Commands', 'Options']);
  assert.equal(root.querySelector('.cs-cli.here .hsec-h .src').textContent, 'codex --help');
  assert.equal(root.querySelectorAll('.cs-cli.here .hsec.open').length, 0);                       // secciones plegadas
  assert.equal(root.querySelectorAll('.cs-cli.here .srows .cmd').some(r => r.dataset.cmd === 'codex --full-auto'), false);
  root.click('.cs-cli.here .cli-h'); assert.equal(root.querySelector('.cs-cli.here').classList.contains('open'), true);
  root.click('.cs-cli.here .cli-h'); assert.equal(root.querySelector('.cs-cli.here').classList.contains('open'), false);
  root.click('.cs-cli.here .cli-h'); assert.equal(root.querySelector('.cs-cli.here').classList.contains('open'), true);
  // abierto: se ve la lista entera del catálogo de ese CLI
  assert.equal(root.querySelectorAll('.cs-cli.here .cmds .cmd').length, catalog.clis.find(c => c.id === 'codex').groups[0].commands.length);
  assert.equal(JSON.parse(store.get('comandos.commands.open.v2')).includes('codex'), true);
  // R4: todo CLI, bloque de arranque y grupo está en el DOM aunque esté plegado
  assert.equal(root.querySelectorAll('.cs-cli').length, 5);
  // logo oficial de cada CLI (diseño «Oficial a color»): ninguna inicial suelta
  assert.deepEqual(root.querySelectorAll('.cs-cli .cli-h .logo-tile').map(t => t.dataset.cli), ['claude', 'codex', 'grok', 'opencode', 'agy']);
  assert.equal(root.querySelectorAll('.cs-cli .cli-h .logo-tile svg.logo').length, 5);
  // ronda 6 A aprobada: lista plana, sin subtítulos de grupo
  assert.equal(root.querySelectorAll('.cs-cli[data-cli="grok"] .grp').length, 0);
  assert.equal(root.querySelectorAll('.cs-cli[data-cli="grok"] .cmds').length, 1);
  assert.equal(root.querySelectorAll('.cs-cli[data-cli="grok"] .cmds .cmd').length > 4, true);
  assert.equal(root.querySelector('.cs-cli[data-cli="grok"]').classList.contains('open'), false);
  assert.equal(root.querySelector('.cs-cli[data-cli="opencode"] .srows .cmd.y').dataset.cmd, 'opencode --auto');
  // 2) un clic teclea sin Enter en el pane capturado, con requestId
  root.click('.cs-cli.here .cmds .cmd[data-cmd="/model"]');
  await sb.state.typing;
  const typed = calls.filter(c => c[0] === '/pane/type');
  assert.deepEqual(typed[0][1], { session: 'demo', pane: '%2', text: '/model', requestId: 'id-1' });
  assert.equal(calls.some(c => c[0] === '/send'), false);
  // segundo clic con un tecleo en vuelo: no llama y avisa
  root.click('.cs-cli.here .cmds .cmd[data-cmd="/model"]'); const inflight = calls.length;
  root.click('.cs-cli.here .cmds .cmd[data-cmd="/model"]');
  assert.equal(calls.length, inflight); assert.equal(toasts.at(-1)[0], 'Espera a que termine de escribir');
  await sb.state.typing;
  // 3) chips de argumento
  root.click('.cs-cli[data-cli="claude"] .cli-h'); root.click('.cs-cli[data-cli="claude"] .cmd[data-cmd="/model "] .opts button');
  await sb.state.typing; assert.equal(calls.at(-1)[1].text, '/model claude-fable-5-1');
  assert.equal(root.querySelector('.cs-cli[data-cli="claude"] .cmds .cmd[data-cmd="/model "] code').textContent, '/model …');
  // 4) cadena guardada: correr fija el destino; Siguiente avanza solo con 200; pane cerrado no salta de paso
  yoloStep2 = '/model gpt-6.1';                                                  // editado a mano en disco
  root.click('.cs-saved-item[data-run="yolo"] button'); await tick(); await tick();
  assert.equal(root.querySelector('.cs-runner .r-h').textContent.includes('paso 1 de 2'), true);
  assert.equal(sb.state.run.steps[1].text, '/model gpt-6.1');                     // M1: corre lo que hay en disco
  target = { session: 'demo', pane: '%9', kind: 'pane', title: 'otro' };            // el usuario cambió de pane
  root.click('.cs-next'); await sb.state.typing;
  assert.equal(calls.at(-1)[1].pane, '%2');                                       // destino fijado al arrancar
  assert.equal(sb.state.run.step, 1);
  typeStatus = 404; root.click('.cs-next'); await sb.state.typing.catch(() => {});
  assert.equal(sb.state.run.step, 1); assert.equal(toasts.at(-1)[0], 'El pane %2 ya no existe');
  assert.equal(root.querySelector('.cs-runner').textContent.includes('El pane %2 ya no existe'), true);
  typeStatus = 200; root.click('.cs-next'); await sb.state.typing;
  assert.equal(root.querySelector('.cs-runner').classList.contains('done'), true);
  assert.equal(root.querySelector('.cs-runner').textContent.includes('completa'), true);
  root.click('.cs-runner [data-run-stop]'); assert.equal(sb.state.run, null); assert.equal(root.querySelector('.cs-runner'), null);
  // 5) cadena rota se lista con error y sin botón Correr
  assert.equal(root.querySelector('.cs-saved-item[data-run="rota"]'), null);
  assert.equal(root.querySelector('.cs-saved-item.error').textContent.includes('Paso inválido'), true);
  assert.equal(root.querySelector('.cs-saved-item.error button'), null);
  // 6) terminal rápida sin CLI: comandos /… atenuados, arranques activos; al arrancar se recuerda el CLI
  target = { session: 'term-q1', pane: '%7', kind: 'term', title: 'Terminal 14:32' };
  sb.render(); const before = calls.length; root.click('.cs-cli[data-cli="claude"] .cmds .cmd'); assert.equal(calls.length, before);   // atenuado: no teclea
  assert.equal(root.querySelector('.cs-cli[data-cli="claude"] .cmds .cmd').classList.contains('dis'), true);
  root.click('.cs-cli[data-cli="claude"] .srows .cmd.y'); await sb.state.typing;
  assert.equal(store.get('comandos.commands.preferred.%7'), 'claude');
  // 7) el CLI sale del pane: la barra deja de marcar "en este pane"
  target = { session: 'demo', pane: '%2', kind: 'pane', title: 'demo %2' };
  sb.applyCatalog({ cliInPane: '', target: { session: 'demo', pane: '%2' }, catalog, versionsAt: 2 });
  assert.equal(root.querySelector('.cs-cli.here'), null);
  assert.equal(root.querySelector('.cs-cli[data-cli="codex"] .cmds .cmd').classList.contains('dis'), true);
  // 8) drift y missing
  const drift = JSON.parse(JSON.stringify(catalog)); drift.clis[1].version.status = 'drift'; drift.clis[4].version.status = 'missing';
  sb.applyCatalog({ cliInPane: 'codex', target: { session: 'demo', pane: '%2' }, catalog: drift, versionsAt: 3 });
  assert.equal(root.querySelector('.cs-cli[data-cli="codex"]').classList.contains('drift'), true);
  assert.equal(root.querySelector('.cs-cli[data-cli="codex"] .badge').textContent, 'en este pane');  // el badge del pane manda
  assert.equal(root.querySelector('.cs-cli[data-cli="codex"] .cli-h').getAttribute('title').includes('el CLI confirma'), true);
  assert.equal(root.querySelector('.cs-cli[data-cli="claude"] .badge').textContent, 'instalado');
  assert.equal(root.querySelector('.cs-cli[data-cli="agy"] .badge').textContent, 'no instalado');
  // 8b) drift pero con todos los comandos detectados en el binario: verificado, sin ámbar
  const det = JSON.parse(JSON.stringify(drift)); det.clis[1].detected = { found: 12, total: 12 };
  sb.applyCatalog({ cliInPane: 'codex', target: { session: 'demo', pane: '%2' }, catalog: det, versionsAt: 4 });
  assert.equal(root.querySelector('.cs-cli[data-cli="codex"]').classList.contains('drift'), false);
  assert.equal(root.querySelector('.cs-cli[data-cli="codex"] .cli-h').getAttribute('title').includes('12 de 12 comandos presentes'), true);
  det.clis[1].detected = { found: 11, total: 12 };
  sb.applyCatalog({ cliInPane: 'codex', target: { session: 'demo', pane: '%2' }, catalog: det, versionsAt: 5 });
  assert.equal(root.querySelector('.cs-cli[data-cli="codex"]').classList.contains('drift'), true);
  assert.equal(root.querySelector('.cs-cli[data-cli="codex"] .cli-h').getAttribute('title').includes('11 de 12 comandos presentes'), true);
  assert.equal(root.querySelector('.cs-cli[data-cli="agy"] .srows .cmd').classList.contains('dis'), true);

  // extra) la búsqueda filtra filas sin tocar state.open
  const openBefore = [...sb.state.open].sort().join();
  const searchEl = root.querySelector('.cs-search'), rendersBefore = root.renders;
  root.input('.cs-search', 'compact');
  const visible = root.querySelectorAll('.cmd').filter(r => !r.hasAttribute('hidden'));
  assert.ok(visible.length >= 3 && visible.every(r => /compact/i.test(r.textContent)));
  assert.equal(root.querySelector('.cs-cli[data-cli="codex"] .srows .cmd.y').hasAttribute('hidden'), true);
  assert.equal([...sb.state.open].sort().join(), openBefore);
  // fix 1: la consulta conserva los espacios y filtra por palabras
  root.input('.cs-search', 'claude fable');
  assert.equal(sb.state.q, 'claude fable');
  const hitsCF = root.querySelectorAll('.cmd').filter(r => !r.hasAttribute('hidden')).map(r => r.dataset.cmd);
  assert.deepEqual(hitsCF, ['claude --model ', '/claude-api', '/model ']);
  root.input('.cs-search', 'model ');
  assert.equal(sb.state.q, 'model ');
  assert.equal(root.querySelector('.cs-search').value, 'model ');
  // fix 2: el input de búsqueda es el mismo nodo; solo se repinta .cs-body
  assert.equal(root.querySelector('.cs-search'), searchEl);
  assert.equal(root.renders, rendersBefore);
  sb.render(); root.click('.cs-cli.here .cli-h'); root.click('.cs-cli.here .cli-h');
  assert.equal(root.querySelector('.cs-search'), searchEl);
  // …y no se filtra a mitad de una composición (IME, teclas muertas): se aplica al terminar
  const firstCli = root.querySelector('.cs-cli');
  root.input('.cs-search', 'resum', { isComposing: true });
  assert.equal(root.querySelector('.cs-cli'), firstCli);
  assert.equal(sb.state.q, 'model ');
  root.dispatch('compositionend', searchEl);
  assert.equal(sb.state.q, 'resum');
  assert.notEqual(root.querySelector('.cs-cli'), firstCli);
  assert.equal(root.querySelector('.cs-search'), searchEl);
  root.input('.cs-search', '');
  assert.equal(root.querySelectorAll('.cmd[hidden]').length, 0);
  // cambiar de destino nunca abre CLIs: el estado abierto es solo lo que el usuario abrió
  const store3 = new Map([['comandos.commands.open.v2', JSON.stringify(['saved', 'codex'])], ['comandos.commands.preferred.term-q2:%8', 'grok']]);
  const storage3 = { getItem: k => store3.get(k) ?? null, setItem: (k, v) => store3.set(k, v), removeItem: k => store3.delete(k) };
  let target3 = { session: 'demo', pane: '%2', kind: 'pane', title: 'demo %2' };
  const root3 = mkRoot();
  const sb3 = createCommandSidebar({ api, root: root3, storage: storage3, makeId: () => 'z', getTarget: () => target3, toast: () => {} });
  sb3.applyCatalog({ cliInPane: 'codex', target: { session: 'demo', pane: '%2' }, catalog, versionsAt: 1 });
  assert.equal(root3.querySelector('.cs-cli[data-cli="grok"]').classList.contains('open'), false);
  assert.equal(root3.querySelector('.cs-cli[data-cli="codex"]').classList.contains('open'), true);
  target3 = { session: 'term-q2', pane: '%8', paneKey: 'term-q2:%8', kind: 'term', title: 'Terminal' };
  sb3.applyCatalog({ cliInPane: '', target: { session: 'term-q2', pane: '%8' }, catalog, versionsAt: 1 });
  assert.equal(root3.querySelector('.cs-cli[data-cli="grok"]').classList.contains('open'), false);
  assert.deepEqual(JSON.parse(store3.get('comandos.commands.open.v2')).sort(), ['codex', 'saved']);
  root3.click('.cs-cli[data-cli="grok"] .cli-h');
  assert.equal(root3.querySelector('.cs-cli[data-cli="grok"]').classList.contains('open'), true);
  // el usuario lo pliega: repetir el catálogo del mismo destino no lo reabre
  root3.click('.cs-cli[data-cli="grok"] .cli-h');
  sb3.applyCatalog({ cliInPane: '', target: { session: 'term-q2', pane: '%8' }, catalog, versionsAt: 2 });
  assert.equal(root3.querySelector('.cs-cli[data-cli="grok"]').classList.contains('open'), false);
  // extra) terminales rápidas: foco y nueva
  const focused = [], mounted = [], root2 = mkRoot();
  const sb2 = createCommandSidebar({ api, root: root2, storage, makeId: () => 'x', getTarget: () => ({ session: 'term-q1', pane: '%7', paneKey: 'term-q1:%7', kind: 'term', title: 'T' }),
    focusTarget: t => focused.push(t), openBuilder: () => focused.push('builder'), toast: () => {}, newTerm: () => focused.push('new'),
    mountTerm: (sess, host, info) => mounted.push([sess, host && host.className, info]),
    terminals: () => [{ tabId: 'q1', paneKey: 'term-q1:%7', session: 'term-q1', pane: '%7', label: 'Terminal <14:32>', cwd: '/tmp' }] });
  sb2.render();
  // pila: comandos arriba y, abajo, UNA cabecera (agarre + pestañas, también asa de
  // altura; ya no hay separador aparte) sobre la terminal (.mini)
  assert.ok(root2.querySelector('.sec-cmds .cs-head')); assert.equal(root2.querySelector('.divider'), null);
  assert.ok(root2.querySelector('.sec-terms .cs-terms.tt .grip')); assert.ok(root2.querySelector('.sec-terms .cs-terms .t.plus[data-new-term]') && !root2.querySelector('.cs-terms .tabs .t.plus'));
  // píldoras con flechas: ‹ pista › y cada píldora con punto, nombre y ✕
  assert.equal(root2.querySelectorAll('.cs-terms [data-tscroll]').length, 2);
  assert.ok(root2.querySelector('.cs-terms .tabs .tw.on .t .dot')); assert.ok(root2.querySelector('.cs-terms .tabs .tw .tx[data-close-term="q1"]'));
  assert.ok(root2.querySelector('.sec-terms .mini'));
  assert.deepEqual(mounted.at(-1).slice(0, 2), ['q1', 'mini']);                        // la terminal se monta en la barra
  // el escritorio recibe las mismas pestañas para pintar su cabecera nativa
  assert.deepEqual(mounted.at(-1)[2], { session: 'q1', hidden: false,
    tabs: [{ id: 'q1', label: 'Terminal <14:32> · destino', title: 'Terminal <14:32>', on: true, sel: true, closing: false }],
    cmds: { open: false, sheet: '', h: 0, empty: false } });
  // persiana: el chevron vive en su propio hueco, fuera de las pestañas
  assert.ok(root2.querySelector('.cs-terms .tog-slot .tog[data-terms-toggle] svg'));
  assert.equal(root2.querySelector('.cs-terms .t[data-focus-term="q1"]').textContent.includes('Terminal <14:32>'), true);
  assert.equal(root2.querySelector('.cs-terms .t[data-focus-term="q1"]').classList.contains('on'), true);
  assert.equal(root2.querySelector('.cs-terms .t[data-focus-term="q1"]').classList.contains('sel'), true);   // es el destino
  root2.click('.t[data-focus-term="q1"]'); root2.click('.t.plus[data-new-term]'); root2.click('.cs-chains[data-open-builder]');
  assert.equal(focused[0].kind, 'term'); assert.equal(focused[0].pane, '%7'); assert.deepEqual(focused.slice(1), ['new', 'builder']);
  // «▾» esconde las terminales (se desmontan; quedan las pestañas) y «▴» las vuelve a mostrar
  root2.click('.tog[data-terms-toggle]');
  assert.equal(root2.classList.contains('terms-hidden'), true);
  assert.deepEqual(mounted.at(-1).slice(0, 2), ['', 'mini']);
  assert.equal(mounted.at(-1)[2].hidden, true); assert.equal(mounted.at(-1)[2].session, 'q1');
  // la cabecera nativa usa las mismas acciones
  sb2.termAction('toggle'); assert.equal(root2.classList.contains('terms-hidden'), false);
  sb2.termAction('toggle'); assert.equal(root2.classList.contains('terms-hidden'), true);
  assert.equal(store.get('comandos.commands.termsHidden'), '1');
  assert.ok(root2.querySelector('.cs-terms .t[data-focus-term="q1"]'));
  root2.click('.tog[data-terms-toggle]');
  assert.equal(root2.classList.contains('terms-hidden'), false);
  assert.deepEqual(mounted.at(-1).slice(0, 2), ['q1', 'mini']);
  root2.click('.tog[data-terms-toggle]'); root2.click('.t[data-focus-term="q1"]');        // tocar una pestaña también las muestra
  assert.equal(root2.classList.contains('terms-hidden'), false);
  // el panel flota encima de la terminal: ya no hay altura arrastrada que fijar
  store.set('comandos.commands.cmdsHeight', '300');
  sb2.render();
  assert.equal(root2.querySelector('.sec-cmds').style.flex, ''); assert.equal(root2.querySelector('.sec-terms').style.flex, '');
  // Barra de herramientas: fila Comandos · Cadenas · Servidores; el panel nace cerrado,
  // cada botón abre su pestaña (otro clic la cierra), Esc y «Cerrar» lo cierran, y elegir
  // un comando lo aparta. El escritorio recibe el estado para repartir la columna.
  {
    const stP = new Map(), storageP = { getItem: k => stP.get(k) ?? null, setItem: (k, v) => stP.set(k, v) };
    const mP = [], hits = [], srv = [], typed = [], rootP = mkRoot();
    const apiP = async (path, body) => {
      if (path === '/pane/type') { typed.push(body.text); return { ok: true }; }
      return api(path, body);
    };
    const sbP = createCommandSidebar({ api: apiP, root: rootP, storage: storageP, makeId: () => 'x', toast: () => {},
      getTarget: () => ({ session: 'term-q1', pane: '%7', paneKey: 'term-q1:%7', title: 'T' }),
      openBuilder: () => hits.push('builder'), mountTerm: (s, h, info) => mP.push(info), mountServers: slot => srv.push(slot ? 'in' : 'out'),
      terminals: () => [{ tabId: 'q1', paneKey: 'term-q1:%7', session: 'term-q1', pane: '%7', label: 'T' }] });
    await sbP.refresh();
    assert.equal(rootP.querySelectorAll('.cs-tools [data-sheet]').length, 3);
    assert.equal(rootP.querySelector('.cs-sheet').hidden, true);
    assert.equal(rootP.querySelector('.sb-head'), null);               // la cabecera recuadrada ya no existe
    assert.deepEqual(mP.at(-1).cmds, { open: false, sheet: '', h: 0, empty: false });
    rootP.click('.cs-tools [data-sheet="cmds"]');
    assert.equal(rootP.querySelector('.cs-sheet').hidden, false);
    assert.equal(rootP.getAttribute('data-panel'), 'cmds'); assert.equal(rootP.classList.contains('sheet-open'), true);
    assert.equal(rootP.querySelector('.cs-tools [data-sheet="cmds"]').getAttribute('aria-pressed'), 'true');
    assert.equal(rootP.querySelector('.cs-head .sh-t').textContent, 'Comandos del pane');
    assert.equal(rootP.querySelector('[data-sheet-tab]'), null);   // la fila de la barra hace de pestañas
    assert.equal(mP.at(-1).cmds.open, true); assert.equal(mP.at(-1).cmds.sheet, 'cmds');
    rootP.click('.cs-tools [data-sheet="chains"]');                 // otra pestaña: cambia sin cerrar
    assert.equal(rootP.getAttribute('data-panel'), 'chains');
    rootP.click('.cs-chains[data-open-builder]'); assert.deepEqual(hits, ['builder']);
    rootP.click('.cs-tools [data-sheet="srv"]'); assert.deepEqual(srv, ['in']);   // la fila SSH se muda al panel
    rootP.click('.cs-tools [data-sheet="srv"]');                     // otro clic en su botón lo cierra
    assert.equal(rootP.querySelector('.cs-sheet').hidden, true); assert.deepEqual(srv, ['in', 'out']);
    rootP.click('.cs-tools [data-sheet="chains"]'); rootP.click('[data-sheet-close]');
    assert.equal(rootP.getAttribute('data-panel'), '');
    rootP.click('.cs-tools [data-sheet="cmds"]');
    rootP.dispatch('keydown', rootP.querySelector('.cs-search'), { key: 'Escape' });
    assert.equal(rootP.getAttribute('data-panel'), '');
    // elegir un comando lo escribe en el pane (sin Enter) y el panel se aparta
    rootP.click('.cs-tools [data-sheet="cmds"]');
    rootP.click('.cs-cli[data-cli="claude"] .srows .cmd.y'); await sbP.state.typing;
    assert.equal(rootP.getAttribute('data-panel'), ''); assert.equal(typed.length, 1);
    // sin terminal que mostrar, una tarjeta llena la columna en vez de un hueco
    sbP.termAction('toggle');
    assert.equal(rootP.querySelector('.cs-empty-terms').hidden, false);
    assert.equal(rootP.querySelector('.cs-empty-terms .et-go').textContent, 'Mostrar terminal');
    assert.equal(mP.at(-1).cmds.empty, true);
    rootP.click('.cs-empty-terms .et-go');
    assert.equal(rootP.querySelector('.cs-empty-terms').hidden, true);
  }
  // ✕ de una pestaña: primer clic pide «¿Cerrar?», el segundo termina esa terminal
  const killed = [], rootX = mkRoot();
  const sbX = createCommandSidebar({ api, root: rootX, storage, makeId: () => 'x', getTarget: () => null,
    toast: () => {}, killTerm: id => { killed.push(id); return Promise.resolve(); }, mountTerm: (s, h, info) => mounted.push(['X', s, info]),
    terminals: () => [{ tabId: 'term-q9', paneKey: 'term-q9', session: 'term-q9', pane: '%9', label: 'T-2026-10-01-09-15-00' }] });
  sbX.render();
  rootX.click('.tx[data-close-term="term-q9"]');
  assert.equal(killed.length, 0);
  assert.equal(rootX.querySelector('.tx[data-close-term="term-q9"]').classList.contains('armed'), true);
  assert.equal(rootX.querySelector('.tx[data-close-term="term-q9"]').textContent, '¿Cerrar?');
  assert.equal(mounted.at(-1)[2].tabs[0].closing, true);                               // el escritorio también lo pinta
  rootX.click('.tx[data-close-term="term-q9"]');
  assert.deepEqual(killed, ['term-q9']);
  sbX.termAction('close', 'term-q9'); assert.deepEqual(killed, ['term-q9']);           // la cabecera nativa usa la misma acción
  assert.equal(sbX.state.closeArm, 'term-q9');
  // helpers puros para el constructor de cadenas (S3)
  const claude = catalog.clis[0];
  const build = mod.cliHTML(claude, { mode: 'build', open: new Set() });
  assert.equal(build.includes('data-add="/model claude-fable-5-1"'), true);
  assert.equal(build.includes('data-cmd='), false);
  assert.equal(mod.rowHTML({ text: '<x> "y"', description: 'd&d' }, {}).includes('&lt;x&gt; &quot;y&quot;'), true);
  // 9) sin destino: catálogo sin pane, título «Sin destino», toda fila .dis y nada teclea
  {
    const callsN = [], toastsN = [];
    const apiN = async (path, body) => { callsN.push([path, body]);
      if (path.startsWith('/commands/catalog')) return { cliInPane: '', target: { session: '', pane: '' }, catalog, versionsAt: 1 };
      if (path === '/chains') return { chains: [{ slug: 'yolo', name: 'Codex yolo', steps: [{ kind: 'pane', text: '/model' }] }] };
      throw new Error('ruta inesperada ' + path); };
    const rootN = mkRoot();
    const sbN = createCommandSidebar({ api: apiN, root: rootN, storage: null, makeId: () => 'n', getTarget: () => null, toast: (m, e) => toastsN.push([m, e]) });
    await sbN.refresh();
    assert.equal(callsN[0][0], '/commands/catalog');                            // nunca ?pane= vacío
    assert.equal(rootN.querySelector('.cs-target').textContent, 'Sin destino');
    const rowsN = rootN.querySelectorAll('.cmd');
    assert.equal(rowsN.length > 0, true);
    assert.equal(rowsN.every(r => r.classList.contains('dis')), true);           // arranques incluidos
    rootN.click('.srows .cmd.y'); rootN.click('.cmds .cmd');
    assert.equal(sbN.insert('/model'), null); assert.equal(sbN.insert('codex', 'shell'), null);
    assert.equal(sbN.startChain('yolo'), null);
    assert.equal(callsN.some(c => c[0] === '/pane/type'), false);
    assert.equal(toastsN.length, 5);
    assert.equal(toastsN.every(([m, e]) => m === 'Selecciona un pane primero' && e === true), true);
  }
  // chips «nuevo»: newArgs marca el chip con clase y texto accesible; el resto queda igual
  {
    const html = mod.rowHTML({ text: '/model ', description: '', args: ['claude-opus-5-5', 'claude-sonnet-5-5'], newArgs: ['claude-sonnet-5-5'] }, {});
    assert.match(html, /<button type="button" data-flat class="new" aria-label="claude-sonnet-5-5 \(nuevo\)"[^>]*data-cmd="\/model claude-sonnet-5-5"/);
    assert.match(html, /<button type="button" data-flat data-cmd="\/model claude-opus-5-5" data-kind="pane">claude-opus-5-5<\/button>/);
    assert.equal((html.match(/class="new"/g) || []).length, 1);
    assert.equal(/class="new"/.test(mod.rowHTML({ text: '/model ', description: '', args: ['a'] }, {})), false);
  }
  // teclado: los encabezados plegables son role=button con tabindex y aria-expanded; Enter/Espacio los operan
  {
    const rootK = mkRoot(), hyd = [];
    const sbK = createCommandSidebar({ api, root: rootK, storage: null, makeId: () => 'k', getTarget: () => ({ session: 'demo', pane: '%2', kind: 'pane', title: 'demo %2' }),
      toast: () => {}, hydrate: el => hyd.push(el) });
    await sbK.refresh();
    const H = key => rootK.querySelector(`.cs-cli[data-cli="${key}"] .cli-h`);
    assert.equal(H('grok').getAttribute('role'), 'button'); assert.equal(H('grok').getAttribute('tabindex'), '0');
    assert.equal(H('grok').getAttribute('aria-expanded'), 'false'); assert.equal(H('codex').getAttribute('aria-expanded'), 'false');
    for (const n of rootK.querySelectorAll('[data-toggle]')) assert.equal(n.getAttribute('role') === 'button' && n.getAttribute('tabindex') === '0' && (n.getAttribute('aria-expanded') === 'true' || n.getAttribute('aria-expanded') === 'false'), true, n.dataset.toggle);
    let prevented = 0;
    rootK.dispatch('keydown', H('grok'), { key: 'Enter', preventDefault() { prevented++; } });
    assert.equal(rootK.querySelector('.cs-cli[data-cli="grok"]').classList.contains('open'), true);
    assert.equal(H('grok').getAttribute('aria-expanded'), 'true'); assert.equal(prevented, 1);
    assert.equal(doc.activeElement, H('grok'));                                  // el foco vuelve al encabezado repintado
    rootK.dispatch('keydown', H('grok'), { key: ' ' });
    assert.equal(H('grok').getAttribute('aria-expanded'), 'false');
    rootK.dispatch('keydown', H('grok'), { key: 'a' }); rootK.dispatch('keydown', H('grok'), { key: 'Enter', isComposing: true });
    rootK.dispatch('keydown', H('grok').querySelector('.nm'), { key: 'Enter' });        // solo el encabezado mismo
    assert.equal(H('grok').getAttribute('aria-expanded'), 'false');
    rootK.dispatch('keydown', H('codex'), { key: 'Enter' });
    const lab = () => rootK.querySelector('.cs-cli[data-cli="codex"] .hsec .hsec-h');
    assert.equal(lab().getAttribute('aria-expanded'), 'false');                 // secciones de --help plegadas
    rootK.dispatch('keydown', lab(), { key: 'Enter' });
    assert.equal(lab().getAttribute('aria-expanded'), 'true');
    assert.equal(rootK.querySelector('.cs-cli[data-cli="codex"] .hsec').classList.contains('open'), true);
    // las cadenas guardadas viven en su pestaña: lista directa, sin encabezado plegable
    assert.equal(rootK.querySelector('.cs-chains-body .cs-saved .cli-h'), null);
    // hydrate corre sobre la raíz tras el primer pintado, tras cada repintado del cuerpo y con la búsqueda
    const n0 = hyd.length; assert.ok(n0 >= 1); assert.equal(hyd.every(x => x === rootK), true);
    rootK.click('.cs-cli[data-cli="claude"] .cli-h'); assert.equal(hyd.length, n0 + 1);
    rootK.input('.cs-search', 'model'); assert.equal(hyd.length, n0 + 2);
    sbK.render(); assert.equal(hyd.length, n0 + 3);
    const sbH = createCommandSidebar({ api, root: mkRoot(), storage: null, makeId: () => 'h', getTarget: () => null, toast: () => {}, hydrate: () => { throw new Error('boom'); } });
    await sbH.refresh();                                                     // un hydrate que falla no rompe el pintado
  }
  console.log('command-sidebar checks ok');
})().catch(e => { console.error(e); process.exit(1); });
