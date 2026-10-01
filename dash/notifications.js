/* Project notices (N2): bottom strip grouped by project + one floating arrival.
 *
 * The server owns events, read state, pending requests, preferences and the
 * sound receipt (GET /notices, POST /notices/read, /notices/sound,
 * /notices/prefs, /presence). This file only renders them and sends the
 * explicit human actions. Four operations stay separate on purpose:
 *   - closing the float hides that floating copy (nothing is read or deleted);
 *   - reading marks events read (pending requests stay pending);
 *   - answering a permission happens in its terminal (never from here);
 *   - opening goes to the exact origin, or shows that it no longer exists.
 * An arrival never changes the tab, focus, draft or text selection.
 *
 *   ComandosNotices.install(options)      dashboard glue (see dash/index.html)
 *   ComandosNotices.mount(options)        same, explicit host/transport
 *   ComandosNotices.createController(o)   state machine (pure, testable)
 *   ComandosNotices.createPresence(o)     POST /presence discipline
 *   ComandosNotices.groupNotices / renderStrip / renderFloat / renderSettings
 */
(function (root, factory) {
  const api = factory();
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.ComandosNotices = api;
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  const GROUP_NEWS = '__news__';
  const GROUP_GENERAL = '__general__';
  const NEWS_KINDS = ['news_edition', 'announcement'];
  const CATEGORIES = ['attention', 'error', 'focus', 'usage', 'done', 'news', 'info'];
  const PRIORITY = { attention: 0, error: 1, focus: 2, usage: 3, done: 4, news: 5, info: 6 };
  const DEFAULT_PREFS = Object.freeze({
    modes: Object.freeze({ attention: 'sound', error: 'sound', focus: 'sound', done: 'visual', news: 'visual', usage: 'visual', info: 'visual' }),
    volume: 0.6, muted: false, floatMs: 6000, burstMs: 10000,
  });
  const FILTERS = [
    ['all', 'Todos'], ['unread', 'Nuevos'], ['pending', 'Pendientes'], ['error', 'Errores'],
    ['done', 'Terminados'], ['usage', 'Uso'], ['news', 'Noticias'],
  ];
  // Settings rows: category, label, preview cue (cues live in dash/ui-sounds.js).
  const TYPES = [
    ['attention', 'Permisos y preguntas', 'permission'],
    ['error', 'Errores', 'error'],
    ['focus', 'Pomodoro', 'success'],
    ['done', 'Turnos terminados', 'complete'],
    ['usage', 'Uso y límites', 'warning'],
    ['news', 'Noticias y anuncios', 'attention'],
    ['info', 'Otros', 'attention'],
  ];
  const CUE = Object.fromEntries(TYPES.map(([c, , cue]) => [c, cue]));
  const KIND_PLURAL = {
    turn_completed: 'turnos terminados', turn_failed: 'turnos con error', turn_cancelled: 'turnos cancelados',
    permission_requested: 'permisos pedidos', input_requested: 'preguntas', focus_completed: 'bloques de foco terminados',
    news_edition: 'ediciones', announcement: 'anuncios', usage_alert: 'alertas de uso',
  };
  const HISTORY_LIMIT = 200;
  const LIVE_LIMIT = 50;
  const KEEP = 500;
  const GONE_TEXT = 'El panel ya no existe';
  // The drawer's height is dragged by hand and remembered per device.
  const DEFAULT_HEIGHT = 260, MIN_HEIGHT = 96, TOP_RESERVE = 140;
  const HEIGHT_KEY = 'comandos.notices.height';
  function clampHeight(px, viewport) {
    const max = Math.max(MIN_HEIGHT, (Number(viewport) || 800) - TOP_RESERVE);
    return Math.round(Math.max(MIN_HEIGHT, Math.min(max, Number(px) || DEFAULT_HEIGHT)));
  }

  // ---------- pure helpers ----------
  const isNews = n => !!n && (n.category === 'news' || NEWS_KINDS.includes(n.kind));
  function groupKeyOf(n) {
    if (isNews(n)) return GROUP_NEWS;
    return n.projectKey || n.project || GROUP_GENERAL;
  }
  function groupLabel(key, n) {
    if (key === GROUP_NEWS) return 'Noticias';
    if (key === GROUP_GENERAL) return 'General';
    return (n && (n.project || n.projectKey)) || key;
  }
  function matchesFilter(n, filter, pending) {
    switch (filter) {
      case 'unread': return !n.read;
      case 'pending': return !!(pending && pending.has(n.eventId));
      case 'error': return n.category === 'error';
      case 'done': return n.category === 'done' || n.category === 'focus';
      case 'usage': return n.category === 'usage';
      case 'news': return isNews(n);
      default: return true;
    }
  }
  const bySequenceDesc = (a, b) => (b.sequence || 0) - (a.sequence || 0);

  /** Groups notices by project; news always in its own "Noticias" group, last. */
  function groupNotices(list, { filter = 'all', pending = new Set() } = {}) {
    const groups = new Map();
    for (const n of (list || []).slice().sort(bySequenceDesc)) {
      if (!matchesFilter(n, filter, pending)) continue;
      const key = groupKeyOf(n);
      if (!groups.has(key)) groups.set(key, { key, label: groupLabel(key, n), project: key === GROUP_NEWS || key === GROUP_GENERAL ? null : (n.project || n.projectKey), notices: [], unread: 0, pendingCount: 0, latest: 0 });
      const g = groups.get(key);
      g.notices.push(n);
      if (!n.read) g.unread += 1;
      if (pending.has(n.eventId)) g.pendingCount += 1;
      g.latest = Math.max(g.latest, n.sequence || 0);
    }
    const rank = g => (g.key === GROUP_NEWS ? 2 : g.key === GROUP_GENERAL ? 1 : 0);
    return [...groups.values()].sort((a, b) => rank(a) - rank(b) || b.latest - a.latest);
  }

  function normalizePrefs(p) {
    const src = p && typeof p === 'object' ? p : {};
    const modes = { ...DEFAULT_PREFS.modes };
    for (const [k, v] of Object.entries(src.modes || {})) if (v === 'visual' || v === 'sound') modes[k] = v;
    const volume = Number(src.volume);
    return {
      ...DEFAULT_PREFS, ...src, modes,
      volume: Number.isFinite(volume) ? Math.max(0, Math.min(1, volume)) : DEFAULT_PREFS.volume,
      muted: !!src.muted,
    };
  }

  function esc(x) {
    return String(x == null ? '' : x).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  }

  function relTime(ms, now) {
    if (!ms || !now) return '';
    const s = Math.max(0, Math.round((now - ms) / 1000));
    if (s < 45) return 'ahora';
    if (s < 3600) return `hace ${Math.max(1, Math.round(s / 60))} min`;
    if (s < 86400) return `hace ${Math.round(s / 3600)} h`;
    const d = new Date(ms);
    return `${String(d.getDate()).padStart(2, '0')}/${String(d.getMonth() + 1).padStart(2, '0')}`;
  }

  const SHAPES = {
    attention: '<path d="M8 12V6a2 2 0 0 1 4 0v6-8a2 2 0 0 1 4 0v8-5a2 2 0 0 1 4 0v8c0 4-3 6-7 6-2 0-4-1-5-3l-4-6c-1-2 1-4 3-2l1 1"/>',
    error: '<path d="m12 3 10 18H2Z M12 9v5M12 17h.01"/>',
    done: '<circle cx="12" cy="12" r="9"/><path d="m8 12 3 3 5-6"/>',
    focus: '<path d="M6 2h12M6 22h12M7 2c0 6 10 6 10 10S7 16 7 22M17 2c0 6-10 6-10 10s10 4 10 10"/>',
    usage: '<path d="M4 20V10M10 20V4M16 20v-7M22 20H2"/>',
    news: '<path d="M4 5h13v14H6a2 2 0 0 1-2-2ZM17 9h3v8a2 2 0 0 1-2 2M8 9h5M8 13h5"/>',
    info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/>',
    bell: '<path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9M10 21h4"/>',
    close: '<path d="m6 6 12 12M6 18l12-12"/>',
    arrow: '<path d="M4 12h16m-6-6 6 6-6 6"/>',
    read: '<path d="m3 12 5 5L18 7M13 17l8-8"/>',
    chevron: '<path d="m6 9 6 6 6-6"/>',
    grow: '<path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7"/>',
    shrink: '<path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7"/>',
  };
  function icon(name) {
    return `<svg class="nt-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${SHAPES[name] || SHAPES.bell}</svg>`;
  }
  const catOf = n => (CATEGORIES.includes(n && n.category) ? n.category : 'info');

  function defaultDescribe(n) {
    if (isNews(n)) return 'Noticias';
    const parts = [n.project || n.projectKey || 'General'];
    if (n.sessionKey && n.sessionKey !== n.project) parts.push(n.sessionKey);
    if (n.paneId) parts.push('pane ' + n.paneId);
    return parts.join(' · ');
  }

  // ---------- rendering (strings; the mount writes them into two slots) ----------
  function renderRow(n, v) {
    const pend = v.pending.has(n.eventId);
    const origin = (v.describe || defaultDescribe)(n);
    const cat = catOf(n);
    return `<article class="nt-row cat-${cat}${n.read ? ' read' : ''}${pend ? ' pending' : ''}" data-nt-event="${esc(n.eventId)}">`
      + `<button type="button" class="nt-open" data-nt-act="open" data-nt-id="${esc(n.eventId)}" data-nt-focus="open:${esc(n.eventId)}" aria-label="Abrir ${esc(origin)}: ${esc(n.title)}">`
      + `<span class="nt-type">${icon(cat)}</span><span class="nt-copy"><strong>${esc(n.title)}</strong>`
      + `<small>${esc(origin)}${pend ? ' · <em>Pendiente · se responde en su terminal</em>' : ''}</small>`
      + `${n.excerpt ? `<span class="nt-excerpt">${esc(n.excerpt)}</span>` : ''}</span>`
      + `<time>${esc(relTime(n.occurredAtMs, v.now))}</time></button>`
      + `<button type="button" class="nt-read" data-nt-act="read" data-nt-id="${esc(n.eventId)}" data-nt-focus="read:${esc(n.eventId)}" ${n.read ? 'disabled aria-label="Leído"' : `aria-label="Marcar leído: ${esc(n.title)}" title="Marcar leído"`}>${n.read ? icon('read') : '<i class="nt-dot"></i>'}</button>`
      + '</article>';
  }

  function renderGone(v) {
    const n = v.notices.find(x => x.eventId === v.unavailable);
    if (!n) return '';
    return `<div class="nt-gone" role="status"><b>${GONE_TEXT}</b><p>${esc((v.describe || defaultDescribe)(n))} · ${esc(n.title)}</p>`
      + `${n.excerpt ? `<p>${esc(n.excerpt)}</p>` : ''}<small>Conservamos el aviso; no se creó otra terminal.</small>`
      + '<button type="button" data-nt-act="gone-close" data-nt-focus="gone-close">Volver a los avisos</button></div>';
  }

  function emptyText(filter) {
    return {
      unread: 'Ya viste todos los avisos.', pending: 'Ninguna solicitud pendiente.', error: 'Sin errores.',
      done: 'Sin turnos terminados.', usage: 'Sin alertas de uso.', news: 'Sin noticias.',
    }[filter] || 'Aún no hay avisos. Cuando llegue uno, podrás volver a su terminal desde aquí.';
  }

  function counts(v) {
    const unread = v.notices.filter(n => !n.read).length;
    const pendingN = v.notices.filter(n => v.pending.has(n.eventId)).length;
    // El número es el del servidor (todo el historial, igual en todas las superficies);
    // la copia local solo sirve de respaldo si el servidor no lo manda.
    const local = v.notices.filter(n => !n.read || v.pending.has(n.eventId)).length;
    const attend = Number.isFinite(v.badge) ? v.badge : local;
    return { unread, pendingN, attend };
  }

  function renderStrip(view) {
    const v = { pending: new Set(), filter: 'all', loaded: true, ...view };
    v.notices = v.notices || [];
    const { unread, pendingN, attend } = counts(v);
    const collapsed = !!v.collapsed;
    const filters = FILTERS.map(([id, label]) => `<button type="button" data-nt-filter="${id}" data-nt-focus="filter:${id}" aria-pressed="${v.filter === id}" class="${v.filter === id ? 'on' : ''}">${label}</button>`).join('');
    let body = '';
    if (!collapsed) {
      const groups = groupNotices(v.notices, { filter: v.filter, pending: v.pending });
      const status = v.error ? '<div class="nt-status" role="status">No pudimos actualizar los avisos. Conservamos los anteriores.</div>'
        : !v.loaded ? '<div class="nt-status" role="status">Cargando avisos…</div>' : '';
      const list = groups.length
        ? `<div class="nt-groups">${groups.map(g => `<section class="nt-group" data-nt-group="${esc(g.key)}" aria-label="${esc(g.label)}">`
          + `<header class="nt-group-head"><h3>${esc(g.label)}</h3><span class="nt-group-meta">${g.notices.length} ${g.notices.length === 1 ? 'aviso' : 'avisos'}${g.pendingCount ? ` · <em>${g.pendingCount} pendiente${g.pendingCount === 1 ? '' : 's'}</em>` : ''}</span>`
          + `${g.unread ? `<button type="button" class="nt-group-read" data-nt-act="read-group" data-nt-group-key="${esc(g.key)}" data-nt-focus="read-group:${esc(g.key)}" aria-label="Marcar leídos los avisos de ${esc(g.label)}">${icon('read')}</button>` : ''}</header>`
          + `<div class="nt-group-list">${g.notices.map(n => renderRow(n, v)).join('')}</div></section>`).join('')}</div>`
        : (v.loaded ? `<div class="nt-empty">${icon('bell')}<p>${emptyText(v.filter)}</p></div>` : '');
      body = `<div class="nt-body" id="nt-body">${status}${v.unavailable ? renderGone(v) : ''}${list}`
        + (pendingN ? '<footer class="nt-foot"><span>Leer no resuelve permisos: se responden en su terminal.</span></footer>' : '') + '</div>';
    }
    return `<section class="nt-strip${collapsed ? ' collapsed' : ''}" aria-label="Avisos por proyecto">`
      + (collapsed ? '' : '<div class="nt-grip" role="separator" aria-orientation="horizontal" tabindex="0" data-nt-focus="grip" '
        + 'aria-label="Altura de los avisos: arrastra, usa ↑/↓ o doble clic para maximizar" title="Arrastra para cambiar la altura · doble clic: maximizar"></div>')
      + '<header class="nt-head">'
      + `<button type="button" class="nt-toggle" data-nt-act="toggle" data-nt-focus="toggle" aria-expanded="${!collapsed}" aria-controls="nt-body">${icon('bell')}<span>Avisos</span>${attend ? `<span class="nt-count">${attend}</span>` : ''}${icon('chevron')}</button>`
      + `<small class="nt-meta">${unread} nuevo${unread === 1 ? '' : 's'} · ${pendingN} pendiente${pendingN === 1 ? '' : 's'}</small>`
      + (collapsed ? '' : `<div class="nt-filters" role="group" aria-label="Filtrar avisos por tipo">${filters}</div>`)
      + `<button type="button" class="nt-read-all" data-nt-act="read-all" data-nt-focus="read-all" ${unread || attend > pendingN ? '' : 'disabled'}>Marcar leídos</button>`
      + (collapsed ? '' : `<button type="button" class="nt-icon-btn" data-nt-act="max" data-nt-focus="max" aria-label="${v.maximized ? 'Restaurar altura' : 'Maximizar'}" title="${v.maximized ? 'Restaurar altura' : 'Maximizar'}">${icon(v.maximized ? 'shrink' : 'grow')}</button>`
        + `<button type="button" class="nt-icon-btn" data-nt-act="close" data-nt-focus="close" aria-label="Cerrar avisos" title="Cerrar">${icon('close')}</button>`)
      + '</header>' + body + '</section>';
  }

  function floatMembers(v) {
    if (!v.float) return [];
    return v.float.eventIds.map(id => v.notices.find(n => n.eventId === id)).filter(Boolean);
  }

  function floatSummary(members) {
    const latest = members[members.length - 1];
    if (members.length === 1) return latest.title;
    const plural = KIND_PLURAL[latest.kind] || 'avisos';
    const where = isNews(latest) ? 'Noticias' : (latest.project || latest.projectKey || 'General');
    return `${members.length} ${plural} en ${where}`;
  }

  function renderFloat(view) {
    const v = { pending: new Set(), ...view };
    v.notices = v.notices || [];
    const members = floatMembers(v);
    if (!members.length) return '';
    const latest = members[members.length - 1];
    const cat = catOf(members.slice().sort((a, b) => PRIORITY[catOf(a)] - PRIORITY[catOf(b)])[0]);
    const origin = (v.describe || defaultDescribe)(latest);
    const gone = members.some(n => n.eventId === v.unavailable);
    const title = floatSummary(members);
    return `<aside class="nt-float cat-${cat}${v.float.persistent ? ' persistent' : ''}" aria-label="Nuevo aviso">`
      + `<span class="nt-type">${icon(cat)}</span>`
      + `<button type="button" class="nt-float-open" data-nt-act="float-open" data-nt-focus="float-open" aria-label="Abrir ${esc(origin)}: ${esc(title)}">`
      + `<span class="nt-float-origin">${esc(origin)}</span><strong>${esc(title)}</strong>`
      + (gone ? `<em>${GONE_TEXT}</em>` : `<b>Abrir ${icon('arrow')}</b>`) + '</button>'
      + `<button type="button" class="nt-float-close" data-nt-act="float-close" data-nt-focus="float-close" aria-label="Ocultar aviso flotante; se conserva en la franja" title="Ocultar; se conserva en la franja">${icon('close')}</button>`
      + '</aside>';
  }

  function renderSettings({ prefs, localSound = false, localAvailable = true } = {}) {
    const p = normalizePrefs(prefs);
    const rows = TYPES.map(([cat, label, cue]) => `<div class="nt-set-row"><span class="nt-set-type">${icon(cat)} ${esc(label)}</span>`
      + `<select data-nt-mode="${cat}" aria-label="Aviso para ${esc(label)}"><option value="visual"${p.modes[cat] === 'visual' ? ' selected' : ''}>Visual</option>`
      + `<option value="sound"${p.modes[cat] === 'sound' ? ' selected' : ''}>Visual + sonido</option></select>`
      + `<button type="button" class="test" data-nt-preview="${cue}" aria-label="Probar sonido de ${esc(label)}">Probar</button></div>`).join('');
    return '<div class="nt-settings">'
      + '<label>Avisos de proyectos: sonido por tipo</label>'
      + `<div class="nt-set-line"><button type="button" class="test" data-nt-local="1" role="switch" aria-checked="${!!localSound}"${localAvailable ? '' : ' disabled'}>${localSound ? 'Sonido activo en este navegador' : 'Activar sonido en este navegador'}</button>`
      + `<label class="nt-set-mute"><input type="checkbox" data-nt-muted="1"${p.muted ? ' checked' : ''}> Silenciar todos los avisos</label></div>`
      + `<label class="nt-set-vol">Volumen de avisos <input type="range" class="solid" min="0" max="100" step="5" data-nt-volume="1" value="${Math.round(p.volume * 100)}" aria-label="Volumen de avisos"></label>`
      + `<div class="nt-set-rows">${rows}</div>`
      + '<div class="desc">Suena una sola vez, en el último equipo que usaste y que tenga el sonido activo. Cargar el historial nunca suena.</div>'
      + '</div>';
  }

  // ---------- controller ----------
  function createController(o = {}) {
    const transport = o.transport;
    const now = o.now || (() => Date.now());
    const setTimer = o.setTimer || ((fn, ms) => setTimeout(fn, ms));
    const clearTimer = o.clearTimer || (t => clearTimeout(t));
    const isVisible = o.isVisible || (() => true);
    const isSessionLive = o.isSessionLive || (() => false);
    const openSource = o.openSource || (() => {});
    const openNews = o.openNews || (() => {});
    const onChange = o.onChange || (() => {});
    const describe = o.describe || null;
    const deviceId = o.deviceId || '';
    const state = {
      notices: [], pending: new Set(), prefs: normalizePrefs(null), focusActive: false,
      loaded: false, error: null, float: null, unavailable: null,
      filter: 'all', collapsed: !!o.collapsed, opened: false,
    };
    const byId = new Map();
    let after = 0, inflight = null, floatTimer = null, floatToken = 0, soundBusy = false, pendingOpen = null;

    const emit = () => { try { onChange(state); } catch (e) { /* rendering never breaks the poll */ } };

    // Nothing that needs a person: one quiet line, so the strip never takes
    // the height of the chat or the sessions. Unread news, finished turns and
    // usage only count in the badge (D5: visual); a pending request, an
    // unread error or attention opens it. Opening it by hand shows the history.
    const NEEDS = new Set(['attention', 'error']);
    function quiet() {
      return state.loaded && !state.error && !state.unavailable && !state.opened && state.filter === 'all'
        && !state.notices.some(n => state.pending.has(n.eventId) || (!n.read && NEEDS.has(n.category)));
    }

    function view() {
      return { notices: state.notices, pending: state.pending, badge: state.badge, filter: state.filter, collapsed: state.collapsed || quiet(),
        float: state.float, unavailable: state.unavailable, prefs: state.prefs, loaded: state.loaded,
        error: state.error, now: now(), describe };
    }

    function ingest(list, live, arrivals) {
      for (const raw of list || []) {
        if (!raw || !raw.eventId) continue;
        const prev = byId.get(raw.eventId);
        if (prev) { Object.assign(prev, raw); continue; }
        const n = { ...raw };
        byId.set(n.eventId, n);
        state.notices.push(n);
        after = Math.max(after, Number(n.sequence) || 0);
        if (live && !n.read) arrivals.push(n);
      }
    }

    function trim() {
      state.notices.sort(bySequenceDesc);
      while (state.notices.length > KEEP) {
        const n = state.notices.pop();
        if (state.pending.has(n.eventId) || (state.float && state.float.eventIds.includes(n.eventId))) { state.notices.push(n); break; }
        byId.delete(n.eventId);
      }
    }

    function clearFloatTimer() { if (floatTimer != null) { clearTimer(floatTimer); floatTimer = null; } }

    function armFloat(ms) {
      clearFloatTimer();
      if (!state.float || state.float.persistent) return;
      const token = state.float.token;
      floatTimer = setTimer(() => {
        floatTimer = null;
        if (state.float && state.float.token === token) { state.float = null; emit(); }
      }, Math.max(1000, Number(ms) || state.prefs.floatMs || 6000));
    }

    function maybeFloat(n) {
      if (!n.float || !n.float.show) return;
      const persistent = n.float.ms == null;
      const f = state.float;
      if (f && n.group && f.group === n.group) {
        if (!f.eventIds.includes(n.eventId)) f.eventIds.push(n.eventId);
        f.persistent = f.persistent || persistent;
        armFloat(n.float.ms);
        return;
      }
      if (f && f.persistent && !persistent) return;   // never bury a pending request under a transient notice
      floatToken += 1;
      state.float = { token: floatToken, eventIds: [n.eventId], group: n.group || null, persistent };
      armFloat(n.float.ms);
    }

    async function claimSound(arrivals) {
      if (soundBusy || !transport) return;
      const prefs = state.prefs;
      if (!prefs || prefs.muted || !isVisible()) return;
      const sounds = o.sounds;
      if (!sounds || typeof sounds.play !== 'function' || typeof sounds.isReady !== 'function' || !sounds.isReady()) return;
      const candidates = arrivals.filter(n => !n.read && prefs.modes[catOf(n)] === 'sound');
      if (!candidates.length) return;
      candidates.sort((a, b) => PRIORITY[catOf(a)] - PRIORITY[catOf(b)] || bySequenceDesc(a, b));
      const pick = candidates[0];
      soundBusy = true;
      try {
        const r = await transport('POST', '/notices/sound', { eventId: pick.eventId, deviceId });
        if (r && r.play === true) sounds.play(r.cue || CUE[catOf(pick)] || 'attention', { eventId: pick.eventId, volume: prefs.volume });
      } catch (e) { /* no receipt, no sound */ } finally { soundBusy = false; }
    }

    async function fetchPage(limit) {
      const qs = `after=${encodeURIComponent(after)}&limit=${limit}&deviceId=${encodeURIComponent(deviceId)}`;
      return transport('GET', '/notices?' + qs);
    }

    function applyMeta(r) {
      if (r.prefs) state.prefs = normalizePrefs(r.prefs);
      state.focusActive = !!r.focusActive;
      if (Array.isArray(r.pending)) state.pending = new Set(r.pending);
      if (Number.isFinite(Number(r.badge)) && r.badge != null) state.badge = Number(r.badge);
      const next = Number(r.nextAfter);
      if (Number.isFinite(next)) after = Math.max(after, next);
    }

    function poll() {
      if (inflight) return inflight;
      inflight = (async () => {
        const first = !state.loaded;
        const arrivals = [];
        try {
          let pages = 0, r;
          // A full page is a catch-up (first load, reconnection after a long
          // gap): it is history, so it never floats nor claims a sound.
          do {
            const limit = first || pages ? HISTORY_LIMIT : LIVE_LIMIT;
            r = await fetchPage(limit) || {};
            applyMeta(r);
            const got = (r.notices || []).length;
            ingest(r.notices, !first && !pages && got < limit, arrivals);
            pages += 1;
            if (got < limit) break;
          } while (pages < 20);
          state.error = null;
        } catch (e) {
          state.error = (e && e.message) || 'error';
          emit();
          return;
        }
        state.loaded = true;
        trim();
        arrivals.sort((a, b) => (a.sequence || 0) - (b.sequence || 0));
        for (const n of arrivals) maybeFloat(n);
        emit();
        if (pendingOpen && byId.has(pendingOpen)) { const id = pendingOpen; pendingOpen = null; await open(id); }
        if (arrivals.length) await claimSound(arrivals);
      })().finally(() => { inflight = null; });
      return inflight;
    }

    function hideFloatWith(id) {
      if (state.float && state.float.eventIds.includes(id)) { clearFloatTimer(); state.float = null; }
    }

    function dismissFloat() { clearFloatTimer(); state.float = null; emit(); }

    async function markRead(ids) {
      const wanted = (ids || []).filter(id => byId.has(id) && !byId.get(id).read);
      if (!wanted.length || !transport) return;
      for (const id of wanted) byId.get(id).read = true;
      if (Number.isFinite(state.badge)) state.badge = Math.max(0, state.badge - wanted.filter(id => !state.pending.has(id)).length);
      emit();
      try {
        const r = await transport('POST', '/notices/read', { eventIds: wanted });
        const done = new Set((r && r.read) || wanted);
        for (const id of wanted) if (!done.has(id)) byId.get(id).read = false;
      } catch (e) { for (const id of wanted) byId.get(id).read = false; state.error = (e && e.message) || 'error'; }
      emit();
      refreshBadge();
    }

    // Sincronía inmediata (1-oct): /notices/watch responde en cuanto llega un aviso o se
    // lee alguno en CUALQUIER equipo; aquí se aplica: número, filas leídas y pedidos.
    let watchRev = '';
    function applyWatch(r) {
      if (!r || typeof r !== 'object') return;
      if (typeof r.rev === 'string') watchRev = r.rev;
      if (Number.isFinite(Number(r.badge)) && r.badge != null) state.badge = Number(r.badge);
      if (Array.isArray(r.pending)) state.pending = new Set(r.pending);
      if (Array.isArray(r.unread)) {
        const unread = new Set(r.unread);
        for (const n of state.notices) n.read = !unread.has(n.eventId);
      }
      emit();
      if (Number(r.latest) > after) poll();      // llegó algo nuevo: traerlo ya
    }
    async function watchOnce(waitS = 25) {
      if (!transport) return null;
      const r = await transport('GET', `/notices/watch?rev=${encodeURIComponent(watchRev)}&wait=${waitS}`);
      applyWatch(r);
      return r;
    }

    // Tras marcar, el número se confirma con el servidor (lo leído en otro equipo también cuenta).
    async function refreshBadge() {
      if (!transport) return;
      try {
        const r = await transport('GET', '/notifs/count');
        if (r && Number.isFinite(Number(r.count))) { state.badge = Number(r.count); emit(); }
      } catch (e) { /* el siguiente poll lo trae */ }
    }

    async function markAllRead(project = null) {
      const hit = state.notices.filter(n => !n.read && (project == null || n.project === project));
      if (!transport) return;
      for (const n of hit) n.read = true;
      const before = state.badge;
      if (project == null && Number.isFinite(state.badge)) state.badge = state.pending.size;   // solo quedan los pedidos sin responder
      emit();
      try {
        const r = await transport('POST', '/notices/read', { all: true, project });
        if (r && Array.isArray(r.read)) for (const id of r.read) if (byId.has(id)) byId.get(id).read = true;
      } catch (e) { for (const n of hit) n.read = false; state.badge = before; state.error = (e && e.message) || 'error'; }
      emit();
      refreshBadge();
    }

    function markGroupRead(key) {
      const g = groupNotices(state.notices, { filter: 'all', pending: state.pending }).find(x => x.key === key);
      if (!g) return Promise.resolve();
      if (g.project) return markAllRead(g.project);
      return markRead(g.notices.filter(n => !n.read).map(n => n.eventId));
    }

    async function open(id) {
      const n = byId.get(id);
      if (!n) { pendingOpen = id; return false; }
      if (isNews(n)) {
        state.unavailable = null; hideFloatWith(id); emit();
        openNews(n);
        await markRead([id]);
        return true;
      }
      if (!n.sessionKey || !isSessionLive(n.sessionKey)) {
        state.unavailable = id; state.collapsed = false;
        emit();
        return false;
      }
      state.unavailable = null; hideFloatWith(id); emit();
      openSource(n);
      await markRead([id]);
      return true;
    }

    function openFloat() {
      const members = floatMembers(view());
      if (!members.length) return Promise.resolve(false);
      return open(members[members.length - 1].eventId);
    }

    function requestOpen(id) {
      if (!id) return;
      if (state.loaded && byId.has(id)) { open(id); return; }
      pendingOpen = id;
    }

    async function setPrefs(partial) {
      if (!transport) return state.prefs;
      const r = await transport('POST', '/notices/prefs', partial);
      state.prefs = normalizePrefs(r && r.modes ? r : { ...state.prefs, ...partial });
      emit();
      return state.prefs;
    }

    return {
      state, view, poll, open, openFloat, requestOpen, dismissFloat, markRead, markAllRead, markGroupRead, setPrefs, watchOnce, applyWatch,
      setFilter(f) { state.filter = FILTERS.some(([id]) => id === f) ? f : 'all'; emit(); },
      setCollapsed(c) { state.collapsed = !!c; if (c) state.opened = false; emit(); },
      toggle() {
        const open = state.collapsed || quiet();
        state.collapsed = !open; state.opened = open; emit();
        return state.collapsed;
      },
      closeUnavailable() { state.unavailable = null; emit(); },
      unreadCount: () => state.notices.filter(n => !n.read).length,
      pendingCount: () => state.pending.size,
    };
  }

  // ---------- presence: only explicit human interaction counts ----------
  function createPresence(o = {}) {
    const doc = o.doc || null;
    const now = o.now || (() => Date.now());
    const every = o.setInterval || ((fn, ms) => setInterval(fn, ms));
    const stopEvery = o.clearInterval || (t => clearInterval(t));
    const later = o.setTimeout || ((fn, ms) => setTimeout(fn, ms));
    const heartbeatMs = o.heartbeatMs || 30000;
    const throttleMs = o.throttleMs || 5000;
    const deferMs = o.deferMs || 0;
    let last = -Infinity, timer = null, started = false;
    const visible = () => !doc || doc.visibilityState !== 'hidden';
    const audio = () => { try { return !!(o.canPlayAudio && o.canPlayAudio()); } catch (e) { return false; } };

    function send(interaction) {
      const body = { deviceId: o.deviceId || '', visible: visible(), canPlayAudio: audio(), interaction: !!interaction };
      let p;
      try { p = o.transport && o.transport('POST', '/presence', body); } catch (e) { p = null; }
      Promise.resolve(p).catch(() => {});
    }
    // Called only from trusted pointerdown/keydown listeners.
    function onGesture(e) {
      if (!e || e.isTrusted !== true || !visible()) return;
      const t = now();
      if (t - last < throttleMs) return;
      last = t;
      if (deferMs > 0) later(() => send(true), deferMs); else send(true);
    }
    const onVisibility = () => send(false);
    function start() {
      if (started) return;
      started = true;
      if (doc) {
        doc.addEventListener('pointerdown', onGesture, true);
        doc.addEventListener('keydown', onGesture, true);
        doc.addEventListener('visibilitychange', onVisibility);
      }
      send(false);
      timer = every(() => { if (visible()) send(false); }, heartbeatMs);
    }
    function stop() {
      if (!started) return;
      started = false;
      if (doc) {
        doc.removeEventListener('pointerdown', onGesture, true);
        doc.removeEventListener('keydown', onGesture, true);
        doc.removeEventListener('visibilitychange', onVisibility);
      }
      if (timer != null) stopEvery(timer);
      timer = null;
    }
    return { start, stop, refresh: () => send(false), onGesture };
  }

  // ---------- DOM mount ----------
  function focusKeyOf(el) { return el && el.getAttribute ? el.getAttribute('data-nt-focus') : null; }

  function mount(o = {}) {
    const doc = o.doc || (typeof document !== 'undefined' ? document : null);
    const win = o.win !== undefined ? o.win : (typeof window !== 'undefined' ? window : null);
    const storage = o.storage !== undefined ? o.storage : (win && (() => { try { return win.localStorage; } catch (e) { return null; } })());
    const read = k => { try { return storage ? storage.getItem(k) : null; } catch (e) { return null; } };
    const write = (k, v) => { try { if (storage) storage.setItem(k, v); } catch (e) { /* private mode */ } };
    const host = o.host || doc.getElementById('panes') || doc.body;
    const every = o.setInterval || ((fn, ms) => setInterval(fn, ms));
    const stopEvery = o.clearInterval || (t => clearInterval(t));
    const isVisible = o.isVisible || (() => !doc || doc.visibilityState !== 'hidden');

    const rootEl = doc.createElement('div');
    rootEl.id = 'notices';
    rootEl.className = 'nt-root';
    const floatEl = doc.createElement('div');
    floatEl.className = 'nt-float-slot';
    floatEl.setAttribute('aria-live', 'polite');
    const stripEl = doc.createElement('div');
    stripEl.className = 'nt-strip-slot';
    rootEl.appendChild(floatEl);
    rootEl.appendChild(stripEl);
    host.appendChild(rootEl);
    if (doc.body && doc.body.classList) doc.body.classList.add('nt-mounted');

    const viewport = () => (win && win.innerHeight) || 800;
    let height = clampHeight(read(HEIGHT_KEY) || DEFAULT_HEIGHT, viewport());
    let restoreTo = null;
    function applyHeight() {
      if (rootEl.style && rootEl.style.setProperty) rootEl.style.setProperty('--nt-h', height + 'px');
    }
    function setHeight(px, { remember = true } = {}) {
      height = clampHeight(px, viewport());
      if (remember) write(HEIGHT_KEY, String(height));
      applyHeight();
      return height;
    }
    function toggleMax() {
      const max = clampHeight(Infinity, viewport());
      if (restoreTo != null && height >= max) { const back = restoreTo; restoreTo = null; setHeight(back); }
      else { restoreTo = height; setHeight(max, { remember: false }); }
      schedule();
    }
    applyHeight();
    const stored = read('comandos.notices.collapsed');
    let queued = false, lastSettings = '';
    const controller = createController({
      ...o, isVisible,
      collapsed: stored == null ? !!o.defaultCollapsed : stored === '1',
      onChange: () => schedule(),
    });

    // The strip is a drawer: closed until the bell opens it (Jesús, 2026-09-29).
    // Arrivals only float and count on the bell badge; nothing opens by itself.
    let hidden = true;
    function badge() {
      if (typeof o.onBadge !== 'function') return;
      const { notices, pending, badge: server } = controller.state;
      o.onBadge(Number.isFinite(server) ? server : notices.filter(n => !n.read || pending.has(n.eventId)).length);
    }
    function render() {
      queued = false;
      const v = controller.view();
      const active = doc.activeElement;
      const keep = active && rootEl.contains(active) ? focusKeyOf(active) : null;
      stripEl.innerHTML = hidden ? '' : renderStrip({ ...v, collapsed: false, maximized: restoreTo != null });
      // showFloat(n): la página puede ceder el aviso flotante a otro canal (en el
      // escritorio, el popup del sistema de cc-notifyd) para no verlo dos veces.
      const fv = v.float && typeof o.showFloat === 'function' && !floatMembers(v).some(n => o.showFloat(n)) ? { ...v, float: null } : v;
      floatEl.innerHTML = renderFloat(fv);
      rootEl.classList.toggle('nt-hidden', hidden);
      rootEl.classList.toggle('nt-collapsed', false);
      rootEl.classList.toggle('nt-has-float', !!fv.float);
      badge();
      if (keep && rootEl.querySelector) {
        const again = rootEl.querySelector(`[data-nt-focus="${String(keep).replace(/["\\]/g, '\\$&')}"]`);
        if (again && again.focus) again.focus({ preventScroll: true });
      }
      renderSettingsBox();
    }
    function schedule() {
      if (o.sync) { render(); return; }
      if (queued) return;
      queued = true;
      (win && win.requestAnimationFrame ? win.requestAnimationFrame.bind(win) : fn => setTimeout(fn, 0))(render);
    }

    const sounds = o.sounds || null;
    function renderSettingsBox() {
      const box = doc.getElementById && doc.getElementById('notice-settings');
      if (!box || (doc.activeElement && box.contains(doc.activeElement))) return;
      const localSound = !!(sounds && sounds.isEnabled && sounds.isEnabled());
      const html = renderSettings({ prefs: controller.state.prefs, localSound, localAvailable: !!sounds });
      if (html === lastSettings) return;
      lastSettings = html;
      box.innerHTML = html;
    }

    let presence = null;
    if (o.presence !== false) {
      presence = createPresence({
        transport: o.transport, deviceId: o.deviceId, doc, now: o.now,
        canPlayAudio: () => !!(sounds && sounds.isReady && sounds.isReady()), deferMs: 400,
      });
      presence.start();
    }

    rootEl.addEventListener('click', e => {
      const t = e.target && e.target.closest ? e.target.closest('[data-nt-act],[data-nt-filter]') : null;
      if (!t || !rootEl.contains(t)) return;
      const f = t.getAttribute('data-nt-filter');
      if (f) { controller.setFilter(f); return; }
      const act = t.getAttribute('data-nt-act'), id = t.getAttribute('data-nt-id');
      if (act === 'toggle' || act === 'close') { hidden = true; controller.state.opened = false; render(); }
      else if (act === 'max') toggleMax();
      else if (act === 'open') controller.open(id);
      else if (act === 'read') controller.markRead([id]);
      else if (act === 'read-all') controller.markAllRead(null);
      else if (act === 'read-group') controller.markGroupRead(t.getAttribute('data-nt-group-key'));
      else if (act === 'float-open') controller.openFloat();
      else if (act === 'float-close') controller.dismissFloat();
      else if (act === 'gone-close') controller.closeUnavailable();
    });

    const onGrip = e => !!(e.target && e.target.closest && e.target.closest('.nt-grip'));
    rootEl.addEventListener('pointerdown', e => {
      if (!onGrip(e) || !win || !win.addEventListener) return;
      e.preventDefault();
      const startY = e.clientY, startH = height;
      restoreTo = null;
      const move = ev => setHeight(startH + (startY - ev.clientY), { remember: false });
      const up = () => { win.removeEventListener('pointermove', move); win.removeEventListener('pointerup', up); setHeight(height); schedule(); };
      win.addEventListener('pointermove', move);
      win.addEventListener('pointerup', up);
    });
    rootEl.addEventListener('dblclick', e => { if (onGrip(e)) toggleMax(); });
    rootEl.addEventListener('keydown', e => {
      if (!onGrip(e)) return;
      if (e.key === 'ArrowUp' || e.key === 'ArrowDown') { e.preventDefault(); restoreTo = null; setHeight(height + (e.key === 'ArrowUp' ? 32 : -32)); }
      else if (e.key === 'Enter') { e.preventDefault(); toggleMax(); }
    });

    const settingsBox = doc.getElementById && doc.getElementById('notice-settings');
    if (settingsBox) {
      settingsBox.addEventListener('change', e => {
        const t = e.target;
        if (!t || !t.getAttribute) return;
        const mode = t.getAttribute('data-nt-mode');
        const done = () => { lastSettings = ''; renderSettingsBox(); };
        const fail = err => { if (o.onError) o.onError(err); done(); };
        if (mode) controller.setPrefs({ modes: { ...controller.state.prefs.modes, [mode]: t.value } }).then(done, fail);
        else if (t.hasAttribute('data-nt-muted')) controller.setPrefs({ muted: !!t.checked }).then(done, fail);
        else if (t.hasAttribute('data-nt-volume')) controller.setPrefs({ volume: Math.max(0, Math.min(1, Number(t.value) / 100)) }).then(done, fail);
      });
      settingsBox.addEventListener('click', e => {
        const t = e.target && e.target.closest ? e.target.closest('[data-nt-preview],[data-nt-local]') : null;
        if (!t || !sounds) return;
        if (t.hasAttribute('data-nt-local')) {
          const next = !sounds.isEnabled();
          sounds.setEnabled(next);
          if (next) sounds.unlock(e);
          lastSettings = '';
          if (doc.activeElement && doc.activeElement.blur) doc.activeElement.blur();
          renderSettingsBox();
          if (presence) setTimeout(() => presence.refresh(), 400);
          return;
        }
        sounds.unlock(e);
        sounds.preview(t.getAttribute('data-nt-preview'));
      });
    }

    let pollTimer = null;
    const pollMs = () => (isVisible() ? (o.pollMs || 3000) : (o.hiddenPollMs || 15000));
    function arm() { if (pollTimer != null) stopEvery(pollTimer); pollTimer = every(() => controller.poll(), pollMs()); }
    arm();
    if (doc.addEventListener) doc.addEventListener('visibilitychange', () => { arm(); if (isVisible()) controller.poll(); });
    if (win && win.addEventListener) {
      win.addEventListener('comandos:open-event', e => controller.requestOpen(e && e.detail && e.detail.eventId));
    }
    // Relative times refresh without touching anything that holds focus.
    const clock = every(() => { if (!(doc.activeElement && rootEl.contains(doc.activeElement))) schedule(); }, 60000);
    render();
    controller.poll();
    // Espera abierta contra el servidor: el número y las filas cambian en cuanto cambian
    // en cualquier equipo. Si falla (red, servidor reiniciando) reintenta en 2 s.
    let watching = o.watch !== false;
    (async function watchLoop() {
      while (watching) {
        try { await controller.watchOnce(25); }
        catch (e) { await new Promise(res => (o.setTimer || setTimeout)(res, 2000)); }
      }
    })();

    return {
      controller, presence, root: rootEl, rootEl, floatEl, stripEl, render,
      toggleStrip(open) {
        hidden = open === undefined ? !hidden : !open;
        if (!hidden) { controller.state.opened = true; controller.setCollapsed(false); }
        else { controller.state.opened = false; }
        render();
        return !hidden;
      },
      get hidden() { return hidden; },
      get height() { return height; },
      setHeight, toggleMax,
      destroy() { watching = false; if (pollTimer != null) stopEvery(pollTimer); stopEvery(clock); if (presence) presence.stop(); if (rootEl.parent || rootEl.parentNode) (rootEl.parentNode || rootEl.parent).removeChild && (rootEl.parentNode || rootEl.parent).removeChild(rootEl); },
    };
  }

  function install(o = {}) {
    if (typeof window === 'undefined' || typeof document === 'undefined') return null;
    const api = o.api;
    const transport = o.transport || ((method, path, body) => (method === 'GET' ? api(path) : api(path, body || {})));
    const inst = mount({ sounds: window.uiSounds || null, ...o, transport });
    module_instance = inst;
    return inst;
  }
  let module_instance = null;

  return {
    GROUP_NEWS, GROUP_GENERAL, FILTERS, TYPES, DEFAULT_PREFS, CATEGORIES,
    isNews, groupKeyOf, groupNotices, matchesFilter, normalizePrefs, relTime,
    renderStrip, renderFloat, renderSettings, floatSummary, clampHeight, DEFAULT_HEIGHT,
    createController, createPresence, mount, install,
    get instance() { return module_instance; },
  };
});
