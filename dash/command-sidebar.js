// Barra de comandos (S1): acordeón por CLI con los comandos del catálogo,
// cadenas guardadas que se corren paso a paso con «Siguiente» y la lista de
// terminales rápidas. Un clic teclea el texto letra a letra en el pane de
// destino vía POST /pane/type y NUNCA envía Enter: el Enter lo da el usuario.
// Sin temporizadores: una cadena avanza solo cuando el servidor respondió 200.
//
// rowHTML/cliHTML son puros y se exportan para el constructor de cadenas (S3):
// mode 'run' (por defecto) pone data-cmd en filas y chips; mode 'build' pone
// data-add y un botón «+ cadena».
(function (root) {
  'use strict';
  const KEY_OPEN = 'comandos.commands.open', KEY_PREF = 'comandos.commands.preferred.';
  const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const STATUS_TEXT = { ok: '', drift: 'sin verificar · el CLI confirma', missing: 'no instalado', unverified: 'sin verificar' };
  const CONTROL_RE = /[\x00-\x1f\x7f]/;
  const WAIT = 'Espera a que termine de escribir';
  const chev = '<span class="chev" data-icon="chevron" data-size="12"></span>';
  // Los encabezados plegables son <div> (sin el estilo 3D de los botones) pero se
  // operan con teclado: role=button, foco con Tab, Enter/Espacio (isToggleKey).
  const toggleAttrs = open => ` role="button" tabindex="0" aria-expanded="${open ? 'true' : 'false'}"`;
  const isToggleKey = e => !!e && (e.key === 'Enter' || e.key === ' ' || e.key === 'Spacebar') && !e.isComposing;

  // La consulta llega cruda (con espacios); aquí se parte en palabras y cada
  // una debe aparecer en el texto, la descripción o algún argumento.
  function hits(cmd, q) {
    const words = String(q || '').toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return true;
    const hay = [cmd.text, cmd.description, ...(Array.isArray(cmd.args) ? cmd.args : [])].map(x => String(x || '')).join(' ').toLowerCase();
    return words.every(w => hay.includes(w));
  }

  // Fila de dos líneas: comando (con «…» si espera argumento) y descripción;
  // los args son chips que teclean «texto arg».
  function rowHTML(cmd, opts = {}) {
    const mode = opts.mode === 'build' ? 'build' : 'run';
    const kind = opts.kind === 'shell' ? 'shell' : 'pane';
    const attr = mode === 'build' ? 'data-add' : 'data-cmd';
    const text = String(cmd.text ?? '');
    const withArg = a => (text.endsWith(' ') ? text : text + ' ') + a;
    const fresh = new Set(Array.isArray(cmd.newArgs) ? cmd.newArgs : []);
    const chips = (Array.isArray(cmd.args) ? cmd.args : [])
      .map(a => fresh.has(a)
        ? `<button type="button" data-flat class="new" aria-label="${esc(a)} (nuevo)" title="nuevo" ${attr}="${esc(withArg(a))}" data-kind="${kind}">${esc(a)}</button>`
        : `<button type="button" data-flat ${attr}="${esc(withArg(a))}" data-kind="${kind}">${esc(a)}</button>`).join('');
    const add = mode === 'build' ? `<button type="button" data-flat class="add" data-add="${esc(text)}" data-kind="${kind}">+ cadena</button>` : '';
    const dis = opts.dis ? ' dis' : '';
    const hidden = hits(cmd, opts.q) ? '' : ' hidden';
    return `<div class="cmd${dis}" ${attr}="${esc(text)}" data-kind="${kind}"${hidden}>`
      + `<code>${esc(text)}${text.endsWith(' ') ? '<em>…</em>' : ''}</code>`
      + (cmd.description ? `<small>${esc(cmd.description)}</small>` : '')
      + (chips ? `<span class="opts">${chips}</span>` : '') + add + '</div>';
  }

  // Arranques como píldoras monoespaciadas (mockup): yolo en ámbar, normal neutro.
  // Siguen siendo .cmd[data-cmd|data-add] para que clic/teclado y las pruebas no cambien.
  // Monograma por CLI: colores del mockup aprobado; el pictograma es el icono de
  // proveedor del producto (data-icon) y, sin icono, la inicial.
  const MONO = { claude: ['#d97757', '#1a0f0a', 'anthropic', 'C'], codex: ['#e8e8e8', '#111', 'openai', 'X'],
    grok: ['#111', '#fff', 'grok', 'G'], opencode: ['#f2f2f2', '#111', '', 'O'], agy: ['#4285f4', '#fff', 'gemini', 'A'] };
  function monoHTML(id) {
    const m = MONO[id] || ['#1c2130', '#eaf0fb', '', String(id || '?').slice(0, 1).toUpperCase()];
    const inner = m[2] ? `<span data-icon="${m[2]}" data-size="15"></span>` : esc(m[3]);
    return `<span class="mono" style="background:${m[0]};color:${m[1]}">${inner}</span>`;
  }
  function launchHTML(cli, which, o) {
    const key = `${cli.id}:${which}`;
    const list = (cli.launch && Array.isArray(cli.launch[which])) ? cli.launch[which] : [];
    const missing = (cli.version && cli.version.status === 'missing') || o.noTarget;
    const attr = o.mode === 'build' ? 'data-add' : 'data-cmd';
    const pills = list.map(text => `<button type="button" data-flat class="cmd pill${which === 'yolo' ? ' y' : ''}${missing ? ' dis' : ''}" ${attr}="${esc(text)}" data-kind="shell"${hits({ text }, o.q) ? '' : ' hidden'}>${esc(text)}</button>`).join('');
    const note = which === 'yolo' && !list.length
      ? `<small class="note"${o.q ? ' hidden' : ''}>${esc((cli.launch && cli.launch.yoloNote) || 'sin flag yolo')}</small>` : '';
    const empty = o.q && !list.some(text => hits({ text }, o.q));
    const label = which === 'yolo'
      ? '<span data-icon="zap" data-size="12"></span> arrancar en modo yolo · sin permisos'
      : '<span data-icon="flame" data-size="12"></span> arrancar normal';
    return `<div class="launch ${which}${o.open.has(key) ? ' open' : ''}"${empty ? ' hidden' : ''}>`
      + `<div class="lab3" data-toggle="${esc(key)}"${toggleAttrs(o.open.has(key))}>${label}<span class="chev">▾</span></div>`
      + pills + note + '</div>';
  }

  // Lista plana (ronda 6 A aprobada): los grupos del catálogo solo ordenan; no
  // se pintan subtítulos ni se pliegan. Solo llegan los comandos detectados en
  // el binario (cli.detected); si nada quedó, se dice.
  function commandsHTML(cli, o) {
    const cmds = (Array.isArray(cli.groups) ? cli.groups : []).flatMap(g => Array.isArray(g.commands) ? g.commands : []);
    const rows = cmds.map(c => rowHTML(c, { mode: o.mode, kind: 'pane', dis: o.noCli, q: o.q })).join('');
    const d = cli.detected;
    const none = !cmds.length && d && d.total
      ? `<div class="note"${o.q ? ' hidden' : ''}>ninguno de los ${d.total} comandos del catálogo está en este binario</div>` : '';
    return `<div class="cmds">${rows}${none}</div>`;
  }

  // o: {mode, open: Set, here, noCli, noTarget, q}. Todo CLI y arranque se pinta
  // siempre; la clase .open solo decide la visibilidad por CSS.
  function cliHTML(cli, opts = {}) {
    const o = { mode: opts.mode === 'build' ? 'build' : 'run', open: opts.open || new Set(), q: String(opts.q || '').trim(),
      noCli: opts.mode === 'build' ? false : !!opts.noCli, noTarget: opts.mode === 'build' ? false : !!opts.noTarget };
    const v = cli.version || {};
    // Versión distinta a la del catálogo pero todos los comandos presentes en
    // el binario: verificado de verdad, sin ámbar. Si falta alguno, sigue ámbar.
    const d = cli.detected && cli.detected.total ? cli.detected : null;
    const st0 = STATUS_TEXT[v.status] !== undefined ? v.status : 'ok';
    const st = (st0 === 'drift' || st0 === 'unverified') && d && d.found === d.total ? 'ok' : st0;
    const ver = v.installed ? `${v.installed}` : (st === 'missing' ? 'no instalado' : '');
    const groups = Array.isArray(cli.groups) ? cli.groups : [];
    const launch = cli.launch || {};
    const empty = o.q && ![...(launch.yolo || []), ...(launch.normal || [])].some(text => hits({ text }, o.q))
      && !groups.some(g => (g.commands || []).some(c => hits(c, o.q)));
    const det = '';   // el conteo detectado va en el title; la fila muestra solo la versión (mockup)
    const cls = ['cs-cli', o.open.has(cli.id) ? 'open' : '', opts.here ? 'here' : '', st !== 'ok' ? st : ''].filter(Boolean).join(' ');
    const badge = opts.here ? '<span class="badge here-tag">en este pane</span>'
      : st === 'missing' ? '<span class="badge off">no instalado</span>'
      : st === 'ok' ? '<span class="badge off">instalado</span>'
      : '<span class="badge warn">sin verificar</span>';
    const title = [`catálogo v${v.pinned}`, STATUS_TEXT[st0], d ? `${d.found} de ${d.total} comandos presentes en el binario` : ''].filter(Boolean).join(' · ');
    return `<div class="${cls}" data-cli="${esc(cli.id)}"${empty ? ' hidden' : ''}>`
      + `<div class="cli-h" data-toggle="${esc(cli.id)}"${toggleAttrs(o.open.has(cli.id))} title="${esc(title)}">${monoHTML(cli.id)}<span class="nm">${esc(cli.label || cli.id)}</span>`
      + `<span class="ver">${esc(ver + det)}</span>${badge}<span class="chev">▾</span></div>`
      + launchHTML(cli, 'yolo', o) + launchHTML(cli, 'normal', o) + commandsHTML(cli, o) + '</div>';
  }

  function createCommandSidebar(opts) {
    const { api, root: el, storage, makeId, getTarget, focusTarget = () => {}, openBuilder = () => {},
      toast = () => {}, terminals = () => [], newTerm = () => {}, hydrate = () => {} } = opts;
    const state = { catalog: null, cliInPane: '', catalogTarget: null, chains: [], open: new Set(), run: null,
      typing: null, q: '', firstRender: true, appliedTarget: null };

    // móvil (≤900): el bloque «arrancar normal» nace plegado; en escritorio, abierto
    const narrow = () => { try { return typeof window !== 'undefined' && window.innerWidth > 0 && window.innerWidth <= 900; } catch (_) { return false; } };
    const read = k => { try { return storage ? storage.getItem(k) : null; } catch (_) { return null; } };
    const write = (k, v) => { try { if (storage) storage.setItem(k, v); } catch (_) {} };
    (function readOpen() {
      try {
        const saved = JSON.parse(read(KEY_OPEN) || 'null');
        if (Array.isArray(saved)) { state.open = new Set(saved.map(String)); state.firstRender = false; }
      } catch (_) {}
    })();
    const writeOpen = () => write(KEY_OPEN, JSON.stringify([...state.open]));

    function target() { try { return getTarget() || null; } catch (_) { return null; } }
    const prefKey = t => KEY_PREF + (t.paneKey || t.pane);
    const sameTarget = (a, b) => !!a && !!b && a.session === b.session && a.pane === b.pane;
    // El CLI del catálogo solo vale para el destino con el que se pidió: al
    // cambiar de pane, hasta el próximo refresh() no se afirma ningún CLI.
    function hereCli() {
      const t = target();
      if (!t) return '';
      if (state.catalogTarget && !sameTarget(state.catalogTarget, t)) return '';
      return state.cliInPane;
    }
    function launchCli(text) {
      const exe = String(text).replace(/^(?:[A-Z_]+=\S+\s+)+/, '').split(' ')[0];
      return ((state.catalog && state.catalog.clis) || []).find(c => c.binary === exe)?.id || '';
    }
    const clis = () => (state.catalog && Array.isArray(state.catalog.clis)) ? state.catalog.clis : [];

    function applyCatalog(payload) {
      state.catalog = payload.catalog || null;
      state.cliInPane = payload.cliInPane || '';
      state.catalogTarget = payload.target || null;
      let changed = false;
      if (state.firstRender && state.catalog) {
        const t = target();
        const id = hereCli() || (t ? read(prefKey(t)) || '' : '');
        state.open.add('saved');
        const cli = clis().find(c => c.id === id);
        if (cli) {
          state.open.add(cli.id); state.open.add(`${cli.id}:yolo`);
          if (!narrow()) state.open.add(`${cli.id}:normal`);
        }
        state.firstRender = false;
        changed = true;
      }
      // Destino nuevo (también con estado abierto persistido): el CLI detectado
      // en el pane se abre solo; sin CLI, el último arrancado ahí. Solo añade;
      // nunca pliega nada.
      const t = target();
      const applied = state.catalogTarget || (t ? { session: t.session, pane: t.pane } : null);
      if (applied && !sameTarget(applied, state.appliedTarget) && state.catalog) {
        const key = t && sameTarget(t, applied) ? (t.paneKey || t.pane) : applied.pane;
        const id = state.cliInPane || (key ? read(KEY_PREF + key) || '' : '');
        const cli = id ? clis().find(c => c.id === id) : null;
        if (cli && !(state.open.has(cli.id) && state.open.has(`${cli.id}:yolo`))) {
          state.open.add(cli.id); state.open.add(`${cli.id}:yolo`); changed = true;
          if (!narrow()) state.open.add(`${cli.id}:normal`);
        }
      }
      state.appliedTarget = applied ? { session: applied.session, pane: applied.pane } : null;
      if (changed) writeOpen();
      render();
    }

    async function refresh() {
      // Sin destino se pide el catálogo sin pane: se ve, pero todo queda .dis.
      const t = target();
      const [cat, ch] = await Promise.all([
        api(t && t.session && t.pane
          ? `/commands/catalog?session=${encodeURIComponent(t.session)}&pane=${encodeURIComponent(t.pane)}`
          : '/commands/catalog'),
        api('/chains'),
      ]);
      state.chains = Array.isArray(ch && ch.chains) ? ch.chains : [];
      applyCatalog(cat || {});
    }

    const setBusy = on => { try { el.classList && el.classList.toggle('typing', on); } catch (_) {} };

    // Único punto que llama a /pane/type. Devuelve una promesa que nunca
    // rechaza: {ok, result} o {ok: false, error}. `after` corre antes de liberar
    // state.typing para que quien espere la promesa vea el estado final.
    function typeInto(tgt, text, kind, after) {
      const body = { session: tgt.session, pane: tgt.pane, text, requestId: makeId() };
      const p = (async () => {
        await null;
        let res;
        try {
          res = { ok: true, result: await api('/pane/type', body) };
          if (kind === 'shell') { const cli = launchCli(text); if (cli) write(prefKey(tgt), cli); }
        } catch (err) {
          res = { ok: false, error: (err && err.message) || 'No se pudo escribir en el pane' };
          toast(res.error, true);
        }
        try { if (after) after(res); } finally {
          if (state.typing === p) { state.typing = null; setBusy(false); }
        }
        return res;
      })();
      state.typing = p;
      setBusy(true);
      return p;
    }

    function insert(text, kind = 'pane') {
      if (state.typing) { toast(WAIT); return null; }
      const tgt = target();
      if (!tgt || !tgt.session || !tgt.pane) { toast('Selecciona un pane primero', true); return null; }
      text = String(text ?? '');
      if (!text || CONTROL_RE.test(text)) { toast('El comando no puede llevar saltos de línea', true); return null; }
      if (kind === 'pane' && !hereCli()) return null;   // sin CLI en el destino: /… no aplica
      return typeInto({ ...tgt }, text, kind === 'shell' ? 'shell' : 'pane');
    }

    // Los archivos .md se editan a mano: antes de correr se releen del disco
    // (si falla la lectura se usa la copia en memoria).
    async function reloadChains() {
      try {
        const ch = await api('/chains');
        if (ch && Array.isArray(ch.chains)) state.chains = ch.chains;
      } catch (_) {}
      return state.chains;
    }
    function beginRun(chain, tgt) {
      state.run = { slug: chain.slug, name: chain.name || chain.slug, steps: chain.steps.map(s => ({ ...s })),
        step: 0, target: { ...tgt }, error: '' };
      render();
      return state.run;
    }
    function startChain(slug) {
      const tgt = target();
      if (!tgt || !tgt.session || !tgt.pane) { toast('Selecciona un pane primero', true); return null; }
      const runnable = c => c && !c.error && Array.isArray(c.steps) && c.steps.length;
      return reloadChains().then(chains => {
        const chain = chains.find(c => c.slug === slug);
        if (!runnable(chain)) { toast('Esa cadena no se puede correr', true); render(); return null; }
        return beginRun(chain, tgt);
      });
    }

    function next() {
      const run = state.run;
      if (!run || run.step >= run.steps.length) return null;
      if (state.typing) { toast(WAIT); return null; }
      const step = run.steps[run.step];
      if (CONTROL_RE.test(String(step.text ?? ''))) { toast('El paso tiene saltos de línea', true); return null; }
      return typeInto(run.target, String(step.text), step.kind === 'shell' ? 'shell' : 'pane', res => {
        if (state.run !== run) return;
        if (res.ok) { run.step += 1; run.error = ''; } else { run.error = res.error; }
        render();
      });
    }

    function stop() { state.run = null; render(); }

    function toggle(key, keepFocus) {
      state.open.has(key) ? state.open.delete(key) : state.open.add(key);
      writeOpen();
      render();
      if (keepFocus) {   // el repintado del cuerpo suelta el foco: se devuelve al mismo encabezado
        const h = [...el.querySelectorAll('[data-toggle]')].find(n => n.dataset.toggle === key);
        if (h && h.focus) h.focus();
      }
    }

    function targetTitle() {
      const t = target();
      return t ? (t.title || `${t.session} ${t.pane}`) : 'Sin destino';
    }
    function catalogPill() {
      const bad = clis().some(c => { const v = c.version || {}; const d = c.detected;
        return v.status === 'missing' ? false : v.status !== 'ok' && !(d && d.total && d.found === d.total); });
      return bad ? '<span class="pill warn"><span data-icon="zap" data-size="11"></span>cambio de versión</span>'
                 : '<span class="pill ok"><span data-icon="sparkles" data-size="11"></span>catálogo ok</span>';
    }
    function headHTML() {
      return '<div class="cs-head"><div class="sb-head"><span class="ic" data-icon="snippet" data-size="18"></span>'
        + `<h2>Comandos<small>destino: <span class="cs-target">${esc(targetTitle())}</span></small></h2>`
        + `<span class="cs-pill">${catalogPill()}</span>`
        + '<button type="button" data-flat class="cs-chains chains-btn" data-open-builder><span data-icon="layers" data-size="13"></span>Cadenas</button></div>'
        + `<div class="search"><span class="ic" data-icon="search" data-size="14"></span><input class="cs-search" type="search" placeholder="Buscar en todos los CLI…" value="${esc(state.q)}"></div></div>`;
    }

    function savedHTML() {
      const items = state.chains.map(c => c.error
        ? `<div class="cs-saved-item it error"><span class="nm">${esc(c.name || c.slug)}</span><small>${esc(c.error)}</small></div>`
        : `<div class="cs-saved-item it" data-run="${esc(c.slug)}"><span class="nm">${esc(c.name || c.slug)}</span>`
          + `<small>${(c.steps || []).length} pasos</small><button type="button" data-flat class="run pill"><span data-icon="play" data-size="11"></span>Correr</button></div>`).join('');
      return `<div class="cs-saved saved${state.open.has('saved') ? ' open' : ''}">`
        + `<div class="cli-h" data-toggle="saved"${toggleAttrs(state.open.has('saved'))}><span data-icon="layers" data-size="16"></span><span class="nm">Cadenas guardadas</span><small class="n">${state.chains.length}</small><span class="chev">▾</span></div>`
        + (items || '<div class="cs-empty">Sin cadenas guardadas</div>') + '</div>';
    }

    function runnerHTML() {
      const run = state.run;
      if (!run) return '';
      const n = run.steps.length, done = run.step >= n;
      return `<div class="cs-runner runner${done ? ' done' : ''}">`
        + `<div class="r-h"><span data-icon="${done ? 'check' : 'timer'}" data-size="16"></span><b>${esc(run.name)}</b><small>${done ? 'completa' : `paso ${run.step + 1} de ${n}`}</small>`
        + `<span class="right"><button type="button" data-flat class="ghost" data-run-stop>${done ? 'Cerrar' : 'Parar'}</button></span></div>`
        + `<div class="r-t">en ${esc(run.target.title || `${run.target.session} ${run.target.pane}`)}</div>`
        + '<div class="steps">' + run.steps.map((s, i) =>
          `<div class="step${i < run.step ? ' done' : ''}${i === run.step ? ' cur' : ''}" data-kind="${esc(s.kind)}"><span class="k">${esc(s.kind)}</span><code>${esc(s.text)}</code></div>`).join('') + '</div>'
        + (run.error ? `<div class="r-err">${esc(run.error)}</div>` : '')
        + (done ? '' : '<div class="acts"><button type="button" data-flat class="cs-next primary" data-run-next><span data-icon="play" data-size="13"></span>Siguiente</button><small>Se escribe sin Enter; tú das Enter.</small></div>') + '</div>';
    }

    function termList() { try { return terminals() || []; } catch (_) { return []; } }
    function termsHTML() {
      const t = target();
      const cur = x => !!t && (t.paneKey && x.paneKey ? t.paneKey === x.paneKey : sameTarget(t, x));
      return '<div class="cs-terms tt">'
        + termList().map(x => `<button type="button" data-flat class="t${cur(x) ? ' on cur' : ''}" data-focus-term="${esc(x.tabId)}">`
          + `<span data-icon="zap" data-size="12"></span>${esc(x.label || x.tabId)}${cur(x) ? ' · destino' : ''}</button>`).join('')
        + '<button type="button" data-flat class="t plus" data-new-term>+ Terminal</button></div>';
    }

    function bodyHTML() {
      const here = hereCli(), t = target(), noTarget = !t || !t.session || !t.pane;
      return savedHTML() + runnerHTML()
        + clis().map(c => cliHTML(c, { mode: 'run', open: state.open, here: !!here && c.id === here, noCli: !here, noTarget, q: state.q })).join('')
        + termsHTML();
    }

    // La cabecera (con el input de búsqueda) se pinta una sola vez; después solo
    // se actualiza el título del destino y se repinta .cs-body, así el input
    // conserva foco, selección y composición (IME, teclas muertas).
    function render() {
      const head = el.querySelector('.cs-head'), body = el.querySelector('.cs-body');
      if (head && body) {
        const tgt = head.querySelector('.cs-target');
        if (tgt) tgt.textContent = targetTitle();
        const pill = head.querySelector('.cs-pill');
        if (pill) pill.innerHTML = catalogPill();   // hydrate(el) al final del render pinta su icono
        body.innerHTML = bodyHTML();
      } else {
        el.innerHTML = headHTML() + `<div class="cs-body">${bodyHTML()}</div>`;
      }
      try { el.classList && el.classList.toggle('searching', !!state.q.trim()); } catch (_) {}
      try { hydrate(el); } catch (_) {}   // pinta los data-icon del marcado recién puesto
    }

    el.addEventListener('click', e => {
      const t = e.target;
      if (!t || typeof t.closest !== 'function') return;
      let n;
      if (t.closest('[data-run-next]')) return void next();
      if (t.closest('[data-run-stop]')) return void stop();
      if (t.closest('[data-open-builder]')) return void openBuilder();
      if (t.closest('[data-new-term]')) return void newTerm();
      if ((n = t.closest('[data-focus-term]'))) {
        const x = termList().find(q => String(q.tabId) === n.dataset.focusTerm);
        if (x) focusTarget({ kind: 'term', tabId: x.tabId, paneKey: x.paneKey, session: x.session, pane: x.pane, title: x.label || x.tabId });
        return;
      }
      if ((n = t.closest('[data-cmd]'))) {
        const row = n.closest('.cmd');
        if (row && row.classList.contains('dis')) {
          const t = target();
          if (!t || !t.session || !t.pane) toast('Selecciona un pane primero', true);
          return;
        }
        return void insert(n.dataset.cmd, n.dataset.kind === 'shell' ? 'shell' : 'pane');
      }
      if (t.closest('button') && (n = t.closest('.cs-saved-item[data-run]'))) return void startChain(n.dataset.run);
      if ((n = t.closest('[data-toggle]'))) return void toggle(n.dataset.toggle);
    });
    el.addEventListener('keydown', e => {
      const t = e.target, n = t && typeof t.closest === 'function' ? t.closest('[data-toggle]') : null;
      if (!n || n !== t || !isToggleKey(e)) return;
      if (e.preventDefault) e.preventDefault();   // Espacio no debe desplazar la lista
      toggle(n.dataset.toggle, true);
    });
    const isSearch = t => !!(t && t.classList && t.classList.contains('cs-search'));
    const search = t => { state.q = String(t.value ?? ''); render(); };
    el.addEventListener('input', e => { if (isSearch(e.target) && !e.isComposing) search(e.target); });
    el.addEventListener('compositionend', e => { if (isSearch(e.target)) search(e.target); });

    return { refresh, render, insert, startChain, next, stop, applyCatalog, get state() { return state; } };
  }

  root.ComandosCommandSidebar = { createCommandSidebar, rowHTML, cliHTML, esc, isToggleKey };
  if (typeof module !== 'undefined' && module.exports) module.exports = { createCommandSidebar, rowHTML, cliHTML, esc, isToggleKey };
})(typeof window !== 'undefined' ? window : globalThis);
