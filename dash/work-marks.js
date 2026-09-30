/* E1 work marks in the web client: a state chosen by the human for a tab
   (tmux session) or a pane (W1 paneKey), with animated SVG indicators.
   Marks are shared through cc-dash (GET/POST /work-marks); the desktop app
   uses the same endpoint. D1: Congelado / Esperando respuesta only change by
   hand; only Resuelto reopens, with a confirmed accepted prompt; finishing a
   turn assigns no mark (neutral icon). Activity (Trabajando) comes from N1
   events and is shown, never stored as a mark.
   Uses index.html globals when present: authToken, toast, tf, S,
   setSessionFavorite. The pure helpers are tested in Node. */
(function(root, factory) {
  const api = factory();
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.WorkMarks = api;
})(typeof self !== 'undefined' ? self : this, function() {
  const MARKS = ['none', 'resolved', 'frozen', 'awaiting_reply'];
  const LABELS = {
    none: ['Sin marca', 'No mark'],
    resolved: ['Resuelto', 'Resolved'],
    frozen: ['Congelado', 'Frozen'],
    awaiting_reply: ['Esperando respuesta', 'Awaiting reply'],
    working: ['Trabajando', 'Working'],
    favorite: ['Favorito', 'Favorite'],
  };
  const SCOPE_LABELS = {session: ['Marca de la pestaña', 'Tab mark'], pane: ['Marca del pane', 'Pane mark']};
  // Fixed 24x24 boxes; animation only moves parts inside the box.
  const ICONS = {
    none: '<circle class="wm-ring" cx="12" cy="12" r="6.5"/>',
    resolved: '<circle cx="12" cy="12" r="9"/><path class="wm-draw" pathLength="1" d="m7.5 12.4 3 3 6-6.6"/>',
    frozen: '<g class="wm-spin"><path d="M12 3v18M4.2 7.5l15.6 9M4.2 16.5l15.6-9"/>' +
      '<path d="m9.5 4.5 2.5 2 2.5-2M9.5 19.5l2.5-2 2.5 2"/></g>',
    awaiting_reply: '<path d="M4 5.5h16v10H10l-4.5 3.8V15.5H4z"/>' +
      '<circle class="wm-dot wm-d1" cx="8.5" cy="10.5" r="1.1"/><circle class="wm-dot wm-d2" cx="12" cy="10.5" r="1.1"/>' +
      '<circle class="wm-dot wm-d3" cx="15.5" cy="10.5" r="1.1"/>',
    working: '<circle class="wm-track" cx="12" cy="12" r="8"/><path class="wm-spin" d="M12 4a8 8 0 0 1 8 8"/>',
    favorite: '<path class="wm-twinkle" d="m12 3.6 2.6 5.3 5.8.8-4.2 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.2-4.1 5.8-.8z"/>',
  };

  // Canal de la IA (grill 30-sep): semáforo pixel 8-bit, mismos dibujos que
  // lib/work_marks.py AI_ICONS (paridad probada). La marca humana va aparte, como sticker.
  const AI_ICONS = {
    work: "<g class=\"ai-gear\" fill=\"#4ade80\"><rect x=\"8\" y=\"1\" width=\"4\" height=\"4\"/><rect x=\"8\" y=\"15\" width=\"4\" height=\"4\"/><rect x=\"1\" y=\"8\" width=\"4\" height=\"4\"/><rect x=\"15\" y=\"8\" width=\"4\" height=\"4\"/><rect x=\"3\" y=\"3\" width=\"3\" height=\"3\"/><rect x=\"14\" y=\"3\" width=\"3\" height=\"3\"/><rect x=\"3\" y=\"14\" width=\"3\" height=\"3\"/><rect x=\"14\" y=\"14\" width=\"3\" height=\"3\"/><rect x=\"5\" y=\"5\" width=\"10\" height=\"10\"/></g><rect x=\"8\" y=\"8\" width=\"4\" height=\"4\" fill=\"#0b1119\"/>",
    need: "<rect x=\"2\" y=\"2\" width=\"16\" height=\"12\" fill=\"#f5b83d\"/><rect x=\"6\" y=\"14\" width=\"4\" height=\"4\" fill=\"#f5b83d\"/><g class=\"ai-bang\" fill=\"#1a1300\"><rect x=\"9\" y=\"4\" width=\"2\" height=\"5\"/><rect x=\"9\" y=\"10\" width=\"2\" height=\"2\"/></g>",
    done: "<rect x=\"5\" y=\"2\" width=\"2\" height=\"16\" fill=\"#9aa6bf\"/><g class=\"ai-flag\" fill=\"#4ade80\"><rect x=\"7\" y=\"3\" width=\"10\" height=\"7\"/></g>",
    error: "<rect x=\"2\" y=\"2\" width=\"16\" height=\"16\" fill=\"#f87171\"/><g class=\"ai-x\" fill=\"#2a0a0e\"><rect x=\"5\" y=\"5\" width=\"3\" height=\"3\"/><rect x=\"12\" y=\"5\" width=\"3\" height=\"3\"/><rect x=\"8\" y=\"8\" width=\"4\" height=\"4\"/><rect x=\"5\" y=\"12\" width=\"3\" height=\"3\"/><rect x=\"12\" y=\"12\" width=\"3\" height=\"3\"/></g>",
    idle: "<rect x=\"6\" y=\"6\" width=\"8\" height=\"8\" fill=\"none\" stroke=\"#5d6b7e\" stroke-width=\"2\"/>",
  };
  const AI_LABELS = {work: ['Trabajando', 'Working'], need: ['Te necesita', 'Needs you'], done: ['Terminó', 'Finished'],
    error: ['Error', 'Error'], idle: ['Quieta', 'Idle']};
  const AI_OF = {working: 'work', awaiting_permission: 'need', awaiting_input: 'need', waiting: 'need',
    completed: 'done', done: 'done', failed: 'error', error: 'error'};
  const STICKERS = {frozen: 'Aparcado', awaiting_reply: 'Esperando', resolved: 'Hecho'};
  const URGENCY = ['awaiting_permission', 'awaiting_input', 'failed', 'working', 'completed'];

  // ---- pure helpers (tested in Node) --------------------------------------
  const lang = () => (typeof tf === 'function' ? (tf('es', 'en') === 'en' ? 1 : 0) : 0);
  function label(name, l = lang()) { return (LABELS[name] || [name, name])[l]; }
  function iconSvg(name, size = 16) {
    const body = ICONS[name] || ICONS.none;
    return `<svg class="wm-icon wm-${name}" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" ` +
      `stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" ` +
      `aria-hidden="true" focusable="false">${body}</svg>`;
  }
  function aiIconSvg(name, size = 20) {
    const body = AI_ICONS[name] || AI_ICONS.idle;
    return `<svg class="ai-icon ai-${name in AI_ICONS ? name : 'idle'}" width="${size}" height="${size}" viewBox="0 0 20 20" ` +
      `shape-rendering="crispEdges" aria-hidden="true" focusable="false">${body}</svg>`;
  }
  /** Dos canales: lo que pone la IA (semáforo) y lo que pones tú (sticker); la IA sugiere «Hecho». */
  function channels(mark, activityState) {
    const m = MARKS.includes(mark) ? mark : 'none';
    const ai = AI_OF[activityState] || 'idle';
    return {ai, sticker: STICKERS[m] || null, mark: m, suggest: ai === 'done' && m === 'none'};
  }
  const ACTIVE = new Set(['working']);
  /** What an indicator shows: the human mark wins; otherwise live activity; otherwise neutral. */
  function display(mark, activityState) {
    const m = MARKS.includes(mark) ? mark : 'none';
    if (m !== 'none') return {icon: m, label: label(m), animated: true, mark: m};
    if (ACTIVE.has(activityState)) return {icon: 'working', label: label('working'), animated: true, mark: 'none'};
    return {icon: 'none', label: label('none'), animated: false, mark: 'none'};
  }
  /** Row keys are "session" or "session|%N"; panes map W1 paneKeys to tmux ids. */
  function targetForRow(rk, panes) {
    const [session, paneId] = String(rk || '').split('|');
    if (!session) return null;
    if (paneId) {
      const hit = (panes || []).filter(p => p.session === session && p.paneId === paneId);
      if (hit.length === 1) return {scope: 'pane', key: hit[0].paneKey, session, paneId};
    }
    return {scope: 'session', key: session, session, paneId: paneId || null};
  }
  function activityFor(target, activity) {
    const a = activity || {};
    if (!target) return null;
    if (target.scope === 'pane') {
      const hit = a['pane:' + target.key] || (target.paneId && a[`tmux:${target.session}:${target.paneId}`]);
      return hit ? hit.state : null;
    }
    // A tab shows its most urgent pane: te necesita > error > trabajando > terminó.
    const states = Object.values(a).filter(x => x && x.session === target.key).map(x => x.state);
    return URGENCY.find(s => states.includes(s)) || null;
  }
  function menuItems(scope, row, sessionFavorite) {
    const items = MARKS.map(m => ({kind: 'mark', value: m, label: label(m), checked: row.mark === m}));
    const fav = scope === 'session' ? !!sessionFavorite : !!row.favorite;
    items.push({kind: 'favorite', value: !fav, label: label('favorite'), checked: fav});
    return items;
  }
  function nextIndex(key, index, count) {
    if (!count) return -1;
    if (key === 'ArrowDown') return (index + 1) % count;
    if (key === 'ArrowUp') return (index - 1 + count) % count;
    if (key === 'Home') return 0;
    if (key === 'End') return count - 1;
    return index;
  }
  function blank(scope, key) { return {scope, key, mark: 'none', favorite: false, revision: 0}; }
  function indexMarks(list) {
    const out = new Map();
    for (const row of list || []) out.set(row.scope + '\u0000' + row.key, row);
    return out;
  }

  // ---- state + transport --------------------------------------------------
  const W = {marks: new Map(), panes: [], activity: {}, timer: null, menu: null, loading: null};
  const rowOf = (scope, key) => W.marks.get(scope + '\u0000' + key) || blank(scope, key);

  async function request(path, body) {
    const headers = {};
    const tok = typeof authToken === 'function' ? authToken() : '';
    if (tok) headers['X-Comandos-Token'] = tok;
    const opt = body ? {method: 'POST', headers: {...headers, 'Content-Type': 'application/json'},
      body: JSON.stringify(body)} : {headers};
    const r = await fetch(path, opt);
    const j = await r.json().catch(() => ({}));
    return {status: r.status, body: j};
  }
  function adopt(body) {
    if (!body || !Array.isArray(body.marks)) return;
    W.marks = indexMarks(body.marks);
    W.panes = Array.isArray(body.panes) ? body.panes : [];
    W.activity = body.activity && typeof body.activity === 'object' ? body.activity : {};
    decorate();
  }
  async function load() {
    if (W.loading) return W.loading;
    W.loading = request('/work-marks').then(r => { if (r.status === 200) adopt(r.body); })
      .catch(() => {}).finally(() => { W.loading = null; });
    return W.loading;
  }
  /** One write; a stale revision adopts the current value and reports it. */
  async function setMark(scope, key, value) {
    const current = rowOf(scope, key);
    const r = await request('/work-marks', {scope, key, value, expectedRevision: current.revision});
    if (r.status === 200 && r.body.mark) {
      W.marks.set(scope + '\u0000' + key, r.body.mark);
      decorate();
      return r.body.mark;
    }
    if (r.status === 409 && r.body.current) {
      W.marks.set(scope + '\u0000' + key, r.body.current);
      decorate();
      throw new Error(typeof tf === 'function' ? tf('Otro dispositivo cambió esta marca; se muestra la actual.',
        'Another device changed this mark; showing the current one.') : 'Revisión desactualizada');
    }
    throw new Error(r.body.error || 'No se pudo guardar la marca');
  }

  // ---- DOM ----------------------------------------------------------------
  function sessionFavorite(session) {
    try { return typeof S !== 'undefined' && S.favs && S.favs.has(session); } catch (e) { return false; }
  }
  function paint(button, target) {
    const row = rowOf(target.scope, target.key);
    const c = channels(row.mark, activityFor(target, W.activity));
    const fav = target.scope === 'pane' && row.favorite;
    const signature = [c.ai, c.sticker, c.suggest, fav].join('|');
    button._wmTarget = target;
    if (button._wmSignature === signature) return;
    button._wmSignature = signature;
    button.innerHTML = aiIconSvg(c.ai) + (fav ? iconSvg('favorite', 12) : '');
    button.classList.toggle('wm-animated', c.ai !== 'idle' || fav);
    button.dataset.wmState = c.ai;
    const aiLabel = AI_LABELS[c.ai][lang()];
    const title = `IA: ${aiLabel}` + (c.sticker ? ` · ${(SCOPE_LABELS[target.scope] || SCOPE_LABELS.session)[lang()]}: ${c.sticker}` : '') +
      (fav ? ` · ${label('favorite')}` : '');
    button.setAttribute('aria-label', title);
    button.title = title;
    // Tu canal: sticker después del nombre y, si la IA terminó sin preguntar, el chip «¿Hecho? ✓».
    const host = button.parentNode;
    if (!host) return;
    let extra = host.querySelector(':scope > .wm-extra');
    if (!extra) {
      extra = document.createElement('span');
      extra.className = 'wm-extra';
      const name = button._wmBefore;
      host.insertBefore(extra, name && name.parentNode === host ? name.nextSibling : null);
      extra.addEventListener('click', e => {
        const chip = e.target.closest('[data-wm-suggest]');
        if (!chip) return;
        e.stopPropagation(); e.preventDefault();
        const t = button._wmTarget;
        setMark(t.scope, t.key, 'resolved');
      });
    }
    extra.innerHTML = (c.sticker ? `<span class="wm-sticker wm-st-${row.mark}">${c.sticker}</span>` : '') +
      (c.suggest ? `<button type="button" class="wm-suggest" data-wm-suggest title="La IA terminó y no preguntó nada">¿Hecho? ✓</button>` : '');
  }
  function indicator(host, target, before) {
    let button = host.querySelector(':scope > .wm-ind');
    if (!button) {
      button = document.createElement('button');
      button.type = 'button';
      button.className = 'wm-ind';
      button.setAttribute('aria-haspopup', 'menu');
      button.setAttribute('aria-expanded', 'false');
      button.addEventListener('click', e => { e.stopPropagation(); e.preventDefault(); openMenu(button); });
      button.addEventListener('keydown', e => {
        if (e.key === 'ArrowDown' || e.key === 'Enter' || e.key === ' ') {
          e.stopPropagation(); e.preventDefault(); openMenu(button);
        }
      });
      host.insertBefore(button, before || null);
      button._wmBefore = before || null;
      watchVisibility(button);
    }
    paint(button, target);
  }
  function decorate() {
    if (typeof document === 'undefined') return;
    document.querySelectorAll('#tabbar .apptab').forEach(tab => {
      const session = tab._session;
      if (!session || session === 'local' || !String(tab.dataset.tabKey || '').startsWith('term:')) return;
      indicator(tab, {scope: 'session', key: session, session}, tab.querySelector('.lbl'));
      tab.dataset.wmInd = '1';   // una sola señal: el semáforo pixel reemplaza el punto de color viejo
    });
    document.querySelectorAll('.row[data-rk]').forEach(row => {
      const target = targetForRow(row.dataset.rk, W.panes);
      const host = row.querySelector('.ident');
      if (target && host) indicator(host, target, host.querySelector('.name'));
    });
  }

  let visibility = null;
  function watchVisibility(el) {
    if (typeof IntersectionObserver !== 'function') return;
    visibility = visibility || new IntersectionObserver(entries => {
      for (const entry of entries) entry.target.classList.toggle('wm-offscreen', !entry.isIntersecting);
    });
    visibility.observe(el);
  }

  function closeMenu(focusBack = true) {
    const m = W.menu;
    if (!m) return;
    W.menu = null;
    m.el.remove();
    m.button.setAttribute('aria-expanded', 'false');
    document.removeEventListener('pointerdown', m.outside, true);
    if (focusBack && m.button.isConnected) m.button.focus({preventScroll: true});
  }
  function openMenu(button) {
    const target = button._wmTarget;
    if (!target) return;
    if (W.menu && W.menu.button === button) return closeMenu();
    closeMenu(false);
    const row = rowOf(target.scope, target.key);
    const items = menuItems(target.scope, row, sessionFavorite(target.key));
    const el = document.createElement('div');
    el.className = 'wm-menu';
    el.setAttribute('role', 'menu');
    el.setAttribute('aria-label', (SCOPE_LABELS[target.scope] || SCOPE_LABELS.session)[lang()]);
    el.innerHTML = items.map((it, i) => {
      const role = it.kind === 'favorite' ? 'menuitemcheckbox' : 'menuitemradio';
      const icon = it.kind === 'favorite' ? 'favorite' : it.value;
      return (it.kind === 'favorite' ? '<div class="wm-sep" role="separator"></div>' : '') +
        `<button type="button" role="${role}" aria-checked="${it.checked}" data-i="${i}" tabindex="-1" ` +
        `class="wm-item${it.checked ? ' on' : ''}">${iconSvg(icon)}<span></span></button>`;
    }).join('');
    el.querySelectorAll('.wm-item').forEach((b, i) => { b.querySelector('span').textContent = items[i].label; });
    document.body.appendChild(el);
    const r = button.getBoundingClientRect();
    const w = el.offsetWidth, h = el.offsetHeight;
    el.style.left = Math.max(8, Math.min(r.left, innerWidth - w - 8)) + 'px';
    el.style.top = (r.bottom + h + 8 > innerHeight ? Math.max(8, r.top - h - 4) : r.bottom + 4) + 'px';
    const buttons = [...el.querySelectorAll('.wm-item')];
    const choose = async i => {
      const it = items[i];
      closeMenu();
      try {
        if (it.kind === 'favorite' && target.scope === 'session' && typeof setSessionFavorite === 'function') {
          await setSessionFavorite(target.key, it.value);
          decorate();
        } else {
          await setMark(target.scope, target.key, it.kind === 'favorite' ? {favorite: it.value} : it.value);
        }
      } catch (err) {
        if (typeof toast === 'function') toast(err.message, true);
      }
    };
    buttons.forEach((b, i) => b.addEventListener('click', e => { e.stopPropagation(); choose(i); }));
    el.addEventListener('keydown', e => {
      const i = buttons.indexOf(document.activeElement);
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); closeMenu(); return; }
      if (e.key === 'Tab') { closeMenu(false); return; }
      if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); if (i >= 0) choose(i); return; }
      const n = nextIndex(e.key, i, buttons.length);
      if (n !== i) { e.preventDefault(); buttons[n].focus(); }
    });
    const outside = e => { if (!el.contains(e.target) && e.target !== button) closeMenu(false); };
    document.addEventListener('pointerdown', outside, true);
    W.menu = {el, button, outside};
    button.setAttribute('aria-expanded', 'true');
    (buttons[items.findIndex(it => it.checked)] || buttons[0]).focus({preventScroll: true});
  }

  function init() {
    const sync = () => document.body.classList.toggle('wm-paused', document.hidden);
    document.addEventListener('visibilitychange', () => { sync(); if (!document.hidden) load(); });
    sync();
    const observer = new MutationObserver(() => decorate());
    const watch = () => ['tabbar', 'rows'].forEach(id => {
      const el = document.getElementById(id);
      if (el && !el._wmObserved) { el._wmObserved = true; observer.observe(el, {childList: true, subtree: true}); }
    });
    watch();
    load();
    W.timer = setInterval(() => { watch(); if (!document.hidden) load(); }, 5000);
  }
  if (typeof document !== 'undefined') {
    if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init); else init();
  }

  return {MARKS, ICONS, iconSvg, display, channels, aiIconSvg, AI_ICONS, STICKERS, targetForRow, activityFor, menuItems, nextIndex, indexMarks, label,
          load, setMark, decorate, adopt};
});
