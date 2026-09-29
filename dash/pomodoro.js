/* Pomodoro client. The backend (GET/POST /pomodoro) owns the block, its
 * revision and its completion; this file only renders the confirmed state and
 * sends commands. Time shown = confirmed block + server clock offset. No
 * client ever sends "finish": the backend scheduler completes blocks even with
 * every page closed. Works in the dashboard, the cc-app popover and remote. */
(function (root, factory) {
  const api = factory();
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.ComandosPomodoro = api;
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  const MIN = 60000;
  const MIN_TARGET_MS = MIN;
  const MAX_TARGET_MS = 180 * MIN;
  const LIVE = ['running', 'paused'];

  function elapsedMs(block, nowMs) {
    if (!block) return 0;
    const delta = block.status === 'running' ? Math.max(0, nowMs - block.resumedAtMs) : 0;
    return Math.min(block.targetMs, block.activeMs + delta);
  }

  function remainingMs(block, nowMs) {
    return block ? Math.max(0, block.targetMs - elapsedMs(block, nowMs)) : 0;
  }

  function fmt(ms) {
    const s = Math.ceil(Math.max(0, ms) / 1000);
    return `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
  }

  function newRequestId() {
    const c = typeof crypto !== 'undefined' ? crypto : null;
    if (c && typeof c.randomUUID === 'function') return c.randomUUID();
    return 'r-' + Date.now().toString(36) + '-' + Math.random().toString(36).slice(2, 12);
  }

  /** Delta that turns the current remaining time into `minutes` (ruler while live). */
  function deltaForRemaining(block, nowMs, minutes) {
    const wanted = Math.round(minutes) * MIN;
    const spent = elapsedMs(block, nowMs);
    const target = Math.max(MIN_TARGET_MS, spent + wanted);
    return Math.min(MAX_TARGET_MS, target) - block.targetMs;
  }

  /**
   * Client state machine. `transport(method, path, body)` resolves to
   * {status, body} and rejects only on network failure.
   */
  function createClient({ transport, now = () => Date.now(), onChange = () => {}, requestId = newRequestId } = {}) {
    let confirmed = null;      // last server snapshot {revision, serverNowMs, block, ...}
    let offset = 0;            // serverNow - localNow, measured at the last response
    let pending = null;        // {request} awaiting a confirmed answer (retry keeps requestId)
    let error = null;          // {code, message, retryable}

    function accept(snap, localAt) {
      if (!snap || typeof snap.revision !== 'number') return false;
      if (confirmed && snap.revision < confirmed.revision) return false;   // out-of-order answer
      confirmed = snap;
      if (typeof snap.serverNowMs === 'number') offset = snap.serverNowMs - localAt;
      return true;
    }

    async function refresh() {
      const at = now();
      let r;
      try { r = await transport('GET', '/pomodoro'); }
      catch (e) {
        if (!pending) error = { code: 'network', message: 'Sin conexión con CommandOS. Se muestra el último estado confirmado.', retryable: false };
        onChange();
        return false;
      }
      if (r.status === 200) {
        accept(r.body, at);
        if (!pending && error && error.code === 'network') error = null;
      }
      onChange();
      return r.status === 200;
    }

    async function dispatch() {
      const request = pending.request;
      const at = now();
      let r;
      try { r = await transport('POST', '/pomodoro', request); }
      catch (e) {
        error = { code: 'network', message: 'No se pudo confirmar. El último estado confirmado sigue visible.', retryable: true };
        onChange();
        return { ok: false, error, request };
      }
      const body = r.body || {};
      if (r.status === 200 && body.ok !== false) {
        accept(body, at);
        pending = null;
        error = null;
        onChange();
        return { ok: true, result: body, request };
      }
      if (body.state) accept(body.state, at);
      if (r.status >= 500) {
        error = { code: body.code || 'server', message: body.error || 'El servidor no confirmó la acción.', retryable: true };
      } else {
        pending = null;
        error = { code: body.code || 'rejected', message: body.error || 'Acción rechazada', retryable: false };
      }
      onChange();
      return { ok: false, error, request };
    }

    async function send(action, fields = {}) {
      if (pending) return { ok: false, busy: true };
      const request = Object.assign({ requestId: requestId(), expectedRevision: confirmed ? confirmed.revision : 0, action }, fields);
      pending = { request };
      error = null;
      onChange();
      return dispatch();
    }

    function retry() { return pending ? dispatch() : Promise.resolve({ ok: false }); }
    function discard() { pending = null; error = null; onChange(); }
    function serverNow() { return now() + offset; }

    function view() {
      const block = confirmed ? confirmed.block || null : null;
      const at = serverNow();
      const live = !!block && LIVE.includes(block.status);
      const remaining = live ? remainingMs(block, at) : 0;
      return {
        revision: confirmed ? confirmed.revision : null,
        block,
        live,
        status: block ? block.status : 'idle',
        remainingMs: remaining,
        elapsedMs: block ? elapsedMs(block, at) : 0,
        due: live && block.status === 'running' && remaining === 0,
        pending: pending ? pending.request.action : null,
        error,
        serverNowMs: at,
        snapshot: confirmed,
      };
    }

    return { refresh, send, retry, discard, view, accept, serverNow, snapshot: () => confirmed };
  }

  return { MIN, MIN_TARGET_MS, MAX_TARGET_MS, elapsedMs, remainingMs, fmt, deltaForRemaining, createClient, newRequestId };
});

/* ---------------------------------------------------------------------------
 * Browser view: header indicator (#btn-pomo) + panel (#pomo-panel). */
(function () {
  'use strict';
  if (typeof document === 'undefined' || typeof window === 'undefined') return;
  const P = window.ComandosPomodoro;
  if (!P) return;
  const MIN = P.MIN;
  const t = (es, en) => (typeof tf === 'function' ? tf(es, en) : es);
  const esc = s => (typeof mdEsc === 'function' ? mdEsc(String(s ?? '')) : String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c])));
  const icon = (name, size = 14) => (typeof svg === 'function' ? svg(name, size) : '');
  const say = (msg, bad) => { if (typeof toast === 'function') toast(msg, bad); };

  async function transport(method, path, body) {
    const headers = {};
    const tok = typeof authToken === 'function' ? authToken() : '';
    if (tok) headers['X-Comandos-Token'] = tok;
    const opt = { method, headers };
    if (body) { headers['Content-Type'] = 'application/json'; opt.body = JSON.stringify(body); }
    const r = await fetch(path, opt);
    let json = {};
    try { json = await r.json(); } catch (e) { json = {}; }
    return { status: r.status, body: json };
  }

  const ui = {
    mode: 'focus',
    draft: { focus: 25, break: 5 },
    settings: {},
    seenCompletion: null,
    banner: '',
    rulerPreview: null,
  };
  const client = P.createClient({ transport, onChange: () => render() });

  function selectedTarget() {
    try {
      const it = typeof pickSel === 'function' && typeof S !== 'undefined' ? pickSel(S.list || []) : null;
      if (!it) return { project: '', sessionKey: '', paneKey: '' };
      return { project: it.project || it.session || '', sessionKey: it.session || '', paneKey: it.pane || '' };
    } catch (e) { return { project: '', sessionKey: '', paneKey: '' }; }
  }

  function applySettings(settings) {
    if (!settings || typeof settings !== 'object') return;
    ui.settings = settings;
    const f = Number(settings.focusMinutes), b = Number(settings.shortBreakMinutes);
    if (f >= 1 && f <= 180) ui.draft.focus = f;
    if (b >= 1 && b <= 180) ui.draft.break = b;
  }

  let saveTimer = null;
  function saveDraft() {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      transport('POST', '/pomodoro', { settings: { focusMinutes: ui.draft.focus, shortBreakMinutes: ui.draft.break } })
        .catch(() => {});
    }, 500);
  }

  function stateLabel(v) {
    if (v.pending) return t('Confirmando…', 'Confirming…');
    if (v.due) return t('Terminando…', 'Finishing…');
    if (v.status === 'running') return v.block.mode === 'break' ? t('Descanso', 'Break') : t('En foco', 'Focusing');
    if (v.status === 'paused') return t('En pausa', 'Paused');
    if (v.status === 'completed') return v.block.mode === 'break' ? t('Descanso terminado', 'Break over') : t('Bloque completado', 'Block complete');
    if (v.status === 'cancelled') return t('Bloque cancelado', 'Block cancelled');
    return t('Listo para empezar', 'Ready');
  }

  function shownMinutes(v) {
    if (ui.rulerPreview != null) return ui.rulerPreview;
    if (v.live) return Math.max(1, Math.ceil(v.remainingMs / MIN));
    return ui.draft[ui.mode];
  }

  function timeText(v) {
    if (v.live) return P.fmt(v.remainingMs);
    return P.fmt(ui.draft[ui.mode] * MIN);
  }

  function header(v) {
    const b = document.getElementById('btn-pomo');
    if (!b) return;
    const mini = v.live ? P.fmt(v.remainingMs) : '';
    b.classList.toggle('running', v.status === 'running');
    b.classList.toggle('paused', v.status === 'paused');
    const iconName = v.block && v.live && v.block.mode === 'break' ? 'coffee' : 'timer';
    const html = `${icon(iconName, 16)}<span class="pomo-mini">${esc(mini)}</span>`;
    if (b.dataset.pmHtml !== html) { b.innerHTML = html; b.dataset.pmHtml = html; }
    b.setAttribute('aria-label', v.live ? `Pomodoro: ${mini} ${stateLabel(v)}` : 'Pomodoro');
  }

  function noteCompletion(v) {
    const b = v.block;
    if (!b || b.status !== 'completed' || ui.seenCompletion === b.blockId) return;
    const first = ui.seenCompletion === null;
    ui.seenCompletion = b.blockId;
    const recent = v.serverNowMs - (b.endedAtMs || 0) < 10 * MIN;
    if (first && !recent) return;          // an old completion is not news after a reload
    ui.banner = b.mode === 'focus'
      ? t(`Bloque completado · ${Math.round(b.activeMs / MIN)} min en ${b.project || 'sin proyecto'}`, `Block complete · ${Math.round(b.activeMs / MIN)} min`)
      : t('Descanso terminado · listo para continuar', 'Break over · ready to continue');
    if (b.mode === 'focus') ui.mode = 'break';           // D3: manual cycles, suggest the break only
    else ui.mode = 'focus';
  }

  function panelHtml(v) {
    const b = v.block;
    const live = v.live;
    const mode = live ? b.mode : ui.mode;
    const target = live ? { project: b.project } : selectedTarget();
    const minutes = shownMinutes(v);
    const primary = v.status === 'running' ? ['pause', t('Pausar', 'Pause'), 'pause']
      : v.status === 'paused' ? ['resume', t('Reanudar', 'Resume'), 'play'] : ['start', t('Iniciar', 'Start'), 'play'];
    const disabled = v.pending ? 'disabled' : '';
    const err = v.error ? `<div class="pm-error" role="alert">${esc(v.error.message)}${v.error.retryable ? ` <button type="button" data-pm="retry">${t('Reintentar', 'Retry')}</button> <button type="button" data-pm="discard">${t('Descartar', 'Discard')}</button>` : ''}</div>` : '';
    return `
      <div class="pm-head">
        <div class="pm-context"><div class="pm-kicker">Pomodoro</div>
          <strong title="${esc(target.project)}">${esc(target.project || t('sin proyecto', 'no project'))}</strong></div>
        <div class="pm-modes" role="group" aria-label="${t('Tipo de bloque', 'Block type')}">
          <button type="button" data-pm-mode="focus" class="${mode === 'focus' ? 'on' : ''}" ${live ? 'disabled' : ''}>${t('Foco', 'Focus')}</button>
          <button type="button" data-pm-mode="break" class="${mode === 'break' ? 'on' : ''}" ${live ? 'disabled' : ''}>${t('Descanso', 'Break')}</button>
        </div>
      </div>
      <div class="pm-row-clock">
        <div class="pm-ruler-clock"><span data-pm-art></span>
          <div><div class="pm-time" data-pm-time>${timeText(v)}</div><small data-pm-label>${esc(stateLabel(v))}</small></div></div>
        <div class="pm-ruler-assembly">
          <input class="pm-ruler" data-pm-ruler type="range" min="1" max="90" step="1" value="${Math.min(90, minutes)}"
            aria-label="${live ? t('Minutos restantes', 'Minutes remaining') : t('Minutos del bloque', 'Block minutes')}"
            aria-valuetext="${minutes} ${t('minutos', 'minutes')}" style="--pm-ruler-fill:${((Math.min(90, minutes) - 1) / 89 * 100).toFixed(1)}%" ${disabled}>
          <div class="pm-ruler-labels" aria-hidden="true">${[1, 15, 30, 45, 60, 75, 90].map(n => `<span>${n}</span>`).join('')}</div>
          <div class="pm-ruler-caption"><span>${live ? t('Arrastra para cambiar el tiempo restante', 'Drag to change the remaining time') : t('Arrastra para elegir minutos; no inicia el bloque', 'Drag to choose minutes; it does not start')}</span></div>
        </div>
        <div class="pm-actions">
          <button type="button" class="primary" id="pp-go" data-pm="${primary[0]}" ${disabled}>${icon(primary[2], 13)} ${primary[1]}</button>
          ${live ? `<button type="button" id="pp-extend" data-pm="extend" ${disabled}>+5 min</button>
          <button type="button" id="pp-skip" data-pm="cancel" aria-label="${t('Cancelar bloque', 'Cancel block')}" title="${t('Cancelar bloque', 'Cancel block')}" ${disabled}>${icon('close', 13)} ${t('Cancelar', 'Cancel')}</button>` : ''}
        </div>
      </div>
      <div class="pm-presets">
        ${[15, 25, 50].map(n => `<button type="button" data-pm-preset="${n}" ${disabled}>${n} min</button>`).join('')}
        <label>${t('Min', 'Min')} <input type="number" data-pm-minutes min="1" max="180" value="${minutes}" aria-label="${t('Minutos', 'Minutes')}" ${disabled}></label>
      </div>
      ${ui.banner ? `<div class="pm-banner" role="status">${esc(ui.banner)}</div>` : ''}
      ${err}
      <div data-pm-extra></div>`;
  }

  function render() {
    const v = client.view();
    const snap = v.snapshot;
    if (snap && snap.settings && snap.settings !== ui._appliedSettings) { ui._appliedSettings = snap.settings; applySettings(snap.settings); }
    noteCompletion(v);
    header(v);
    const panel = document.getElementById('pomo-panel');
    if (!panel || panel.classList.contains('hidden')) return;
    if (panel.contains(document.activeElement) && document.activeElement.matches('[data-pm-ruler],[data-pm-minutes]') && ui.rulerPreview != null) return;
    panel.classList.add('pm-v1');
    panel.classList.toggle('break', !!(v.block && v.live && v.block.mode === 'break'));
    panel.innerHTML = panelHtml(v);
    wire(panel);
    if (typeof window.ComandosPomodoroExtras === 'function') {
      try { window.ComandosPomodoroExtras(panel, v, ui); } catch (e) { /* optional decorations */ }
    }
  }

  function tick() {
    const v = client.view();
    header(v);
    const panel = document.getElementById('pomo-panel');
    if (panel && !panel.classList.contains('hidden')) {
      panel.querySelectorAll('[data-pm-time]').forEach(el => { el.textContent = timeText(v); });
      panel.querySelectorAll('[data-pm-label]').forEach(el => { el.textContent = stateLabel(v); });
    }
  }

  async function command(action, fields) {
    ui.banner = '';
    const res = await client.send(action, fields);
    if (res.ok && typeof window.ComandosPomodoroConfirmed === 'function') {
      try { window.ComandosPomodoroConfirmed(action, res); } catch (e) { /* optional */ }
    }
    if (!res.ok && res.error && !res.error.retryable) say(res.error.message, true);
    scheduleRefresh();
    return res;
  }

  function startBlock() {
    const target = selectedTarget();
    return command('start', { mode: ui.mode, targetMs: ui.draft[ui.mode] * MIN, project: target.project, sessionKey: target.sessionKey, paneKey: target.paneKey });
  }

  function setMinutes(value, commit) {
    const n = Math.round(Number(value));
    if (!Number.isFinite(n)) return;
    const minutes = Math.max(1, Math.min(180, n));
    const v = client.view();
    if (v.live) {
      if (!commit) { ui.rulerPreview = minutes; tick(); return; }
      ui.rulerPreview = null;
      const delta = P.deltaForRemaining(v.block, v.serverNowMs, minutes);
      if (delta) command('extend', { deltaMs: delta }); else render();
      return;
    }
    ui.rulerPreview = null;
    ui.draft[ui.mode] = minutes;
    if (commit) saveDraft();
    render();
  }

  function wire(panel) {
    panel.querySelectorAll('[data-pm]').forEach(btn => btn.addEventListener('click', e => {
      e.stopPropagation();
      const act = btn.dataset.pm;
      if (act === 'start') startBlock();
      else if (act === 'pause' || act === 'resume' || act === 'cancel') command(act);
      else if (act === 'extend') command('extend', { deltaMs: 5 * MIN });
      else if (act === 'retry') client.retry();
      else if (act === 'discard') client.discard();
    }));
    panel.querySelectorAll('[data-pm-mode]').forEach(btn => btn.addEventListener('click', e => {
      e.stopPropagation(); ui.mode = btn.dataset.pmMode; ui.banner = ''; render();
    }));
    panel.querySelectorAll('[data-pm-preset]').forEach(btn => btn.addEventListener('click', e => {
      e.stopPropagation(); setMinutes(btn.dataset.pmPreset, true);
    }));
    const ruler = panel.querySelector('[data-pm-ruler]');
    if (ruler) {
      ruler.addEventListener('input', () => {
        ruler.style.setProperty('--pm-ruler-fill', ((Number(ruler.value) - 1) / 89 * 100).toFixed(1) + '%');
        const v = client.view();
        if (v.live) setMinutes(ruler.value, false);
        else { ui.draft[ui.mode] = Number(ruler.value); ui.rulerPreview = null; tick(); const n = panel.querySelector('[data-pm-minutes]'); if (n) n.value = ruler.value; }
      });
      ruler.addEventListener('change', () => setMinutes(ruler.value, true));
    }
    const num = panel.querySelector('[data-pm-minutes]');
    if (num) num.addEventListener('change', () => setMinutes(num.value, true));
  }

  let refreshTimer = null;
  function scheduleRefresh() {
    clearTimeout(refreshTimer);
    const v = client.view();
    let wait = 15000;
    if (v.live && v.status === 'running') wait = Math.min(wait, Math.max(400, v.remainingMs + 400));
    if (v.due) wait = 1000;
    if (document.hidden) wait = Math.max(wait, 30000);
    refreshTimer = setTimeout(async () => { await client.refresh(); scheduleRefresh(); }, wait);
  }

  function openPanel(e) {
    const panel = document.getElementById('pomo-panel');
    if (!panel) return;
    if (!panel.classList.contains('hidden')) { panel.classList.add('hidden'); return; }
    const r = e && e.currentTarget ? e.currentTarget.getBoundingClientRect() : { height: 0 };
    panel.style.top = (r.height ? r.bottom + 8 : 8) + 'px';
    const width = Math.min(560, window.innerWidth - 16);
    if (r.height) { panel.style.left = Math.max(8, Math.min(r.left, window.innerWidth - width - 8)) + 'px'; panel.style.right = 'auto'; }
    else { panel.style.left = 'auto'; panel.style.right = '12px'; }
    panel.classList.remove('hidden');
    render();
    client.refresh();
  }

  function init() {
    const btn = document.getElementById('btn-pomo');
    if (btn && !btn._pmWired) { btn._pmWired = true; btn.addEventListener('click', openPanel); }
    document.addEventListener('click', e => {
      if (!e.target.closest || e.target.closest('#pomo-panel') || e.target.closest('#btn-pomo')) return;
      document.getElementById('pomo-panel')?.classList.add('hidden');
    });
    document.addEventListener('visibilitychange', () => { if (!document.hidden) { client.refresh(); scheduleRefresh(); } });
    setInterval(() => { if (!document.hidden) tick(); }, 1000);
    client.refresh().then(scheduleRefresh);
  }

  window.pomoRender = () => { render(); client.refresh(); };
  window.ComandosPomodoro.ui = { client, render, state: ui, command };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
  else init();
})();
