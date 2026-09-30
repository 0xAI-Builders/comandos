// Constructor de cadenas (S3): modal «Cadenas» con el mismo acordeón por CLI de
// la barra de comandos, pero solo para ARMAR una cadena. Un clic en un comando
// añade un paso; no escribe en ningún pane (este módulo no conoce esa ruta) y
// no pulsa ninguna tecla. Guardar escribe solo por POST /chains; Correr guarda y
// avisa a quien monta el módulo con onSaved(chain, {run: true}).
//
// Las filas y el acordeón salen de ComandosCommandSidebar.rowHTML/cliHTML con
// mode 'build' (data-add + botón «+ cadena»): no se duplica marcado.
(function (root) {
  'use strict';
  const STEP_MIME = 'application/x-comandos-step';
  const CONTROL_RE = /[\x00-\x1f\x7f]/;

  function sidebarModule() {
    if (root.ComandosCommandSidebar) return root.ComandosCommandSidebar;
    if (typeof require === 'function') return require('./command-sidebar.js');
    throw new Error('chain-builder necesita command-sidebar.js cargado antes');
  }

  // opts: {api, root, catalog: () => view, chains: () => lista, onSaved(chain, {run}),
  //        toast, hydrate(el) (pinta los data-icon del acordeón)}
  function createChainBuilder(opts) {
    const { api, root: host, catalog = () => null, chains = () => [], onSaved = () => {},
      toast = () => {}, hydrate = () => {} } = opts;
    const Sidebar = sidebarModule();
    const esc = Sidebar.esc;
    const doc = opts.doc || host.ownerDocument || (typeof document !== 'undefined' ? document : null);
    const state = { name: '', steps: [], dragging: null };
    let backdrop = null, editSlug = null, open_ = new Set(), saving = false, prevFocus = null, error = '';

    const $ = sel => (backdrop ? backdrop.querySelector(sel) : null);
    const view = () => { try { return catalog() || null; } catch (_) { return null; } };
    const clis = () => { const v = view(); return v && Array.isArray(v.clis) ? v.clis : []; };
    const chainList = () => { try { const l = chains(); return Array.isArray(l) ? l : []; } catch (_) { return []; } };
    const kindOf = k => (k === 'shell' ? 'shell' : 'pane');
    const stepIndex = el => { const s = el && el.closest ? el.closest('.slots .step') : null; return s ? Number(s.dataset.i) : -1; };

    // ---------- pintado por zonas: el input del nombre conserva foco y composición ----------
    function renderBody() {
      const body = $('.cb-body');
      if (!body) return;
      const list = clis();
      body.innerHTML = list.length
        ? list.map(c => Sidebar.cliHTML(c, { mode: 'build', open: open_ })).join('')
        : '<div class="cb-empty">El catálogo de comandos aún no cargó. Cierra y vuelve a abrir.</div>';
      try { hydrate(body); } catch (_) {}
    }

    function renderSlots(focus) {
      const slots = $('.slots');
      if (!slots) return;
      const n = state.steps.length;
      slots.innerHTML = n ? state.steps.map((s, i) =>
        `<div class="step" draggable="true" data-i="${i}" data-kind="${esc(s.kind)}">`
        + `<span class="k"><b>${i + 1}</b> ${esc(s.kind)}</span>`
        + '<span class="grab" aria-hidden="true">⠿</span>'
        + `<code title="${esc(s.text)}">${esc(s.text)}</code>`
        + '<span class="ctl">'
        + `<button type="button" data-move="-1" aria-label="Mover el paso ${i + 1} antes" title="Mover antes"${i === 0 ? ' disabled' : ''}>←</button>`
        + `<button type="button" data-move="1" aria-label="Mover el paso ${i + 1} después" title="Mover después"${i === n - 1 ? ' disabled' : ''}>→</button>`
        + `<button type="button" data-del aria-label="Quitar el paso ${i + 1}" title="Quitar">✕</button>`
        + '</span></div>').join('')
        : '<div class="cb-empty">Toca un comando para añadirlo como paso.</div>';
      const count = $('.cb-count');
      if (count) count.textContent = n === 1 ? '1 paso' : `${n} pasos`;
      if (focus) { const b = $(`.slots .step[data-i="${focus.i}"] [data-move="${focus.dir}"]`); if (b && !b.hasAttribute('disabled') && b.focus) b.focus(); }
    }

    function renderMsg() {
      const m = $('.cb-msg');
      if (m) m.innerHTML = error ? `<div class="m-error" role="alert">${esc(error)}</div>` : '';
    }
    function show(text) { error = text; renderMsg(); }
    function clearError() { if (error) { error = ''; renderMsg(); } }
    function setBusy(on) {
      saving = on;
      for (const sel of ['[data-save]', '[data-run]']) {
        const b = $(sel);
        if (!b) continue;
        if (on) b.setAttribute('disabled', ''); else b.removeAttribute('disabled');
      }
    }

    // ---------- pasos ----------
    function add(kind, text) {
      text = String(text ?? '');
      if (!text.trim() || CONTROL_RE.test(text)) { show('Ese comando no se puede añadir'); return; }   // .m-error: un toast queda bajo el modal
      state.steps.push({ kind: kindOf(kind), text });
      clearError();
      renderSlots();
      const slots = $('.slots');
      if (slots) slots.scrollLeft = slots.scrollWidth;   // el nuevo paso queda a la vista
    }
    function move(from, to, dir) {
      const n = state.steps.length;
      if (!(from >= 0 && from < n && to >= 0 && to < n) || from === to) return;
      const [m] = state.steps.splice(from, 1);
      state.steps.splice(to, 0, m);
      renderSlots(dir ? { i: to, dir } : null);
    }
    function remove(i) {
      if (!(i >= 0 && i < state.steps.length)) return;
      state.steps.splice(i, 1);
      clearError();
      renderSlots();
    }
    function toggle(key, keepFocus) {
      if (open_.has(key)) { open_.delete(key); }
      else {
        open_.add(key);
        const cli = clis().find(c => c.id === key);   // al abrir un CLI se abren su arranque yolo y sus grupos
        if (cli) { open_.add(`${key}:yolo`); (cli.groups || []).forEach((_, n) => open_.add(`${key}:${n}`)); }
      }
      renderBody();
      if (keepFocus) {   // el repintado suelta el foco: se devuelve al mismo encabezado
        const h = [...backdrop.querySelectorAll('[data-toggle]')].find(x => x.dataset.toggle === key);
        if (h && h.focus) h.focus();
      }
    }

    // ---------- guardar ----------
    // `saving` es de la instancia: sobrevive a close()/open(), así un modal reabierto
    // no lanza un segundo POST mientras el primero sigue en vuelo. Al resolver, si el
    // modal que lo pidió ya no es el actual, no se cierra nada ni se corre la cadena.
    async function save(run) {
      if (saving || !backdrop) return null;
      const name = state.name.trim();
      if (!name) { show('Ponle un nombre a la cadena'); const i = $('.m-name'); if (i && i.focus) i.focus(); return null; }
      if (!state.steps.length) { show('Añade al menos un paso'); return null; }
      const body = { name, steps: state.steps.map(s => ({ kind: s.kind, text: s.text })) };
      if (editSlug) body.slug = editSlug;
      const mine = backdrop;
      clearError();
      saving = true; setBusy(true);
      let res, failure = '';
      try { res = await api('/chains', body); }
      catch (err) { failure = (err && err.message) || 'No se pudo guardar la cadena'; }
      saving = false;
      const same = backdrop === mine;
      const chain = res && res.chain;
      if (!failure && !(chain && chain.slug)) failure = 'Respuesta inválida del servidor';
      if (failure) {
        setBusy(false);
        if (same) show(failure); else toast(failure, true);   // sin modal visible, el aviso va por toast
        return null;
      }
      if (same) close();
      setBusy(false);
      try { await onSaved(chain, { run: same && !!run }); }
      catch (err) { toast((err && err.message) || 'No se pudo refrescar las cadenas', true); }
      return chain;
    }

    // ---------- abrir / cerrar ----------
    const onKey = e => { if (e && e.key === 'Escape') close(); };

    function close() {
      if (!backdrop) return false;
      doc.removeEventListener('keydown', onKey);
      const el = backdrop; backdrop = null;
      state.dragging = null;
      if (el.remove) el.remove(); else if (el.parentNode) el.parentNode.removeChild(el);
      const back = prevFocus; prevFocus = null;
      try { if (back && back.focus) back.focus(); } catch (_) {}
      return true;
    }

    function open(slug) {
      let chain = null;
      if (typeof slug === 'string' && slug) {
        chain = chainList().find(c => c.slug === slug);
        if (!chain || chain.error || !Array.isArray(chain.steps)) { toast('Esa cadena no se puede editar', true); return null; }
      }
      if (backdrop) { const keep = prevFocus; close(); prevFocus = keep; }
      else prevFocus = doc.activeElement || null;
      // Copia de los pasos: editar aquí nunca muta la lista viva de la barra.
      state.name = chain ? String(chain.name || chain.slug) : '';
      state.steps = chain ? chain.steps.map(s => ({ kind: kindOf(s.kind), text: String(s.text) })) : [];
      state.dragging = null; editSlug = chain ? chain.slug : null;
      open_ = new Set(); error = '';

      backdrop = doc.createElement('div');
      backdrop.className = 'backdrop';
      backdrop.setAttribute('data-mclose', '');
      backdrop.innerHTML = '<div class="modal chain-only" role="dialog" aria-modal="true" aria-labelledby="cb-title">'
        + `<div class="m-head"><h2 id="cb-title">${chain ? 'Editar cadena' : 'Cadena nueva'}</h2>`
        + `<input class="m-name" type="text" maxlength="80" autocomplete="off" placeholder="Nombre de la cadena" aria-label="Nombre de la cadena" value="${esc(state.name)}">`
        + '<button type="button" class="x" data-close>Cerrar</button></div>'
        + '<div class="cb-body"></div><div class="cb-msg"></div>'
        + '<div class="m-foot"><div class="slots" aria-label="Pasos de la cadena"></div>'
        + '<div class="cb-actions"><span class="cb-count"></span>'
        + '<button type="button" class="primary" data-save>Guardar</button>'
        + '<button type="button" data-run title="Guarda la cadena y la corre en el pane de destino">Correr</button></div></div></div>';
      wire(backdrop);
      renderBody(); renderSlots(); renderMsg();
      if (saving) setBusy(true);   // un guardado anterior sigue en vuelo
      host.appendChild(backdrop);
      doc.addEventListener('keydown', onKey);
      const input = $('.m-name');
      if (input && input.focus) input.focus();
      return state;
    }

    // ---------- eventos: todo delegado en el fondo ----------
    function wire(el) {
      el.addEventListener('click', e => {
        const t = e.target;
        if (t === el) return void close();               // solo el fondo cierra; el modal no traga clics
        if (!t || typeof t.closest !== 'function') return;
        let n;
        if (t.closest('[data-close]')) return void close();
        if (t.closest('[data-save]')) return void save(false);
        if (t.closest('[data-run]')) return void save(true);
        if ((n = t.closest('[data-move]'))) {
          if (n.hasAttribute('disabled')) return;
          const i = stepIndex(n), dir = Number(n.dataset.move) < 0 ? -1 : 1;
          return void move(i, i + dir, dir);
        }
        if (t.closest('[data-del]')) return void remove(stepIndex(t));
        if ((n = t.closest('[data-add]'))) return void add(n.dataset.kind, n.dataset.add);
        if ((n = t.closest('[data-toggle]'))) return void toggle(n.dataset.toggle);
      });
      el.addEventListener('keydown', e => {
        const t = e.target, n = t && typeof t.closest === 'function' ? t.closest('[data-toggle]') : null;
        if (!n || n !== t || !Sidebar.isToggleKey(e)) return;
        if (e.preventDefault) e.preventDefault();
        toggle(n.dataset.toggle, true);
      });
      el.addEventListener('input', e => {
        const t = e.target;
        if (t && t.classList && t.classList.contains('m-name')) { state.name = String(t.value ?? ''); clearError(); }
      });

      // Reordenar arrastrando (misma lógica que wireDnD del laboratorio). El
      // origen sale de state.dragging; sin dragstart propio no se reordena, así
      // un texto arrastrado desde fuera no mueve pasos.
      const clearMarks = (...cs) => {
        for (const c of cs) for (const s of el.querySelectorAll('.slots .step.' + c)) s.classList.remove(c);
      };
      el.addEventListener('dragstart', e => {
        const s = e.target && e.target.closest ? e.target.closest('.slots .step') : null;
        if (!s) return;
        state.dragging = stepIndex(s);
        const dt = e.dataTransfer;
        if (dt) { try { dt.effectAllowed = 'move'; dt.setData(STEP_MIME, String(state.dragging)); dt.setData('text/plain', String(state.dragging)); } catch (_) {} }
        s.classList.add('dragging');
      });
      el.addEventListener('dragover', e => {
        if (state.dragging === null) return;
        const over = e.target && e.target.closest ? e.target.closest('.slots') : null;
        if (!over) return;
        e.preventDefault();
        if (e.dataTransfer) { try { e.dataTransfer.dropEffect = 'move'; } catch (_) {} }
        clearMarks('over');
        const s = e.target.closest('.slots .step');
        if (s) s.classList.add('over');
      });
      el.addEventListener('drop', e => {
        const slots = e.target && e.target.closest ? e.target.closest('.slots') : null;
        if (!slots || state.dragging === null) return;
        e.preventDefault();
        const from = state.dragging;
        state.dragging = null;
        const i = stepIndex(e.target);
        move(from, i >= 0 ? i : state.steps.length - 1);   // sobre el hueco de .slots: al final
      });
      el.addEventListener('dragend', () => { state.dragging = null; clearMarks('over', 'dragging'); });
    }

    return { open, close, get state() { return state; } };
  }

  root.ComandosChainBuilder = { createChainBuilder };
  if (typeof module !== 'undefined' && module.exports) module.exports = { createChainBuilder };
})(typeof window !== 'undefined' ? window : globalThis);
