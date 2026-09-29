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

  /* ---- Art: six styles, original artist frames (assets/pomodoro, see CREDITS.md).
   * The hourglass is decorative in every style: its sand never encodes the
   * remaining time; the number does. Changing style never touches the block. */
  const ASSET_ROOT = '/assets/pomodoro/';
  const cell = (col, row) => ({ file: 'shikashi/icons.png', x: col * 32, y: row * 32 });
  const soul = name => ({ file: '7soul/' + name + '.png', x: 1, y: 1 });
  const strip = (name, frames) => ({ file: 'lared/' + name + '.png', native: 16, frames });
  const anim = (file, width, height, frames, fps, motion) => ({ file, width, height, frames, fps, motion });
  const HOURGLASS = anim('zoedoz/hourglass.png', 42, 42, 15, 10);
  const CHEST = anim('karsiori-chests/golden.png', 40, 25, 5, 5 / 3, 'chest');
  const CAMPFIRE = anim('arlantr/campfire.png', 32, 32, 4, 8);
  const ART_SOURCES = {
    zoedoz: { author: 'Zoedoz', url: 'https://opengameart.org/content/animated-hourglass', license: 'CC0', licenseUrl: 'https://creativecommons.org/publicdomain/zero/1.0/' },
    karsioriChests: { author: 'karsiori', url: 'https://karsiori.itch.io/pixel-art-chest-pack-animated', license: 'CC0', licenseUrl: 'https://creativecommons.org/publicdomain/zero/1.0/' },
    arlantr: { author: 'ArlanTR', url: 'https://opengameart.org/content/campfire-pixel-art-animated', license: 'CC0', licenseUrl: 'https://creativecommons.org/publicdomain/zero/1.0/' },
    karsioriGems: { author: 'karsiori', url: 'https://karsiori.itch.io/free-pixel-art-gem-pack', license: 'CC0', licenseUrl: 'https://creativecommons.org/publicdomain/zero/1.0/' },
    karsiori: { author: 'karsiori', url: 'https://karsiori.itch.io/pixel-art-potion-pack-animated', license: 'CC0', licenseUrl: 'https://creativecommons.org/publicdomain/zero/1.0/' },
    shikashi: { author: 'Matt Firth (shikashipx) + game-icons.net', url: 'https://shikashipx.itch.io/shikashis-fantasy-icons-pack', license: 'CC BY 4.0', licenseUrl: 'https://creativecommons.org/licenses/by/4.0/' },
    lared: { author: 'La Red Games', url: 'https://laredgames.itch.io/gems-coins-free', license: 'CC0', licenseUrl: 'https://creativecommons.org/publicdomain/zero/1.0/' },
    soul: { author: 'Henrique Lazarini (7Soul1)', url: 'https://opengameart.org/content/496-pixel-art-icons-for-medievalfantasy-rpg', license: 'Dominio público / atribución conservada', licenseUrl: 'https://www.deviantart.com/7soul1/art/420-Pixel-Art-Icons-for-RPG-129892453' },
  };
  const STYLES = {
    alchemy: { title: 'Alquimia', sources: ['karsiori', 'zoedoz'], assets: { clock: HOURGLASS, crystal: anim('karsiori/crystal.png', 15, 30, 7, 10), first: anim('karsiori/first.png', 14, 24, 9, 10), hundred: anim('karsiori/hundred.png', 18, 34, 24, 10), streak: anim('karsiori/streak.png', 14, 25, 8, 10), level: anim('karsiori/level.png', 24, 39, 12, 10) } },
    arcade: { title: 'Arcade', sources: ['lared', 'zoedoz', 'karsioriChests'], assets: { clock: HOURGLASS, crystal: strip('spr_coin_strip4', 4), first: strip('MonedaP', 5), hundred: CHEST, streak: strip('spr_coin_roj', 4), level: strip('MonedaD', 5) } },
    shikashi: { title: 'Fantasía', sources: ['shikashi', 'zoedoz', 'karsioriChests', 'arlantr'], assets: { clock: HOURGLASS, crystal: cell(15, 12), first: cell(8, 13), hundred: CHEST, streak: CAMPFIRE, level: cell(7, 12) } },
    soul: { title: 'RPG clásico', sources: ['soul', 'zoedoz', 'karsioriChests', 'arlantr'], assets: { clock: HOURGLASS, crystal: soul('I_Crystal01'), first: soul('Ac_Medal04'), hundred: CHEST, streak: CAMPFIRE, level: soul('Ac_Medal01') } },
    garden: { title: 'Jardín', sources: ['shikashi', 'zoedoz'], assets: { clock: HOURGLASS, crystal: cell(14, 12), first: cell(3, 12), hundred: cell(4, 12), streak: cell(5, 12), level: cell(8, 21) } },
    crystals: { title: 'Cristales', sources: ['karsioriGems', 'zoedoz'], assets: { clock: HOURGLASS, crystal: anim('karsiori-gems/crystal.png', 23, 27, 10, 10), first: anim('karsiori-gems/first.png', 28, 28, 11, 10), hundred: anim('karsiori-gems/hundred.png', 20, 30, 11, 10), streak: anim('karsiori-gems/streak.png', 19, 22, 11, 10), level: anim('karsiori-gems/level.png', 27, 26, 10, 10) } },
  };
  const STYLE_ORDER = ['alchemy', 'arcade', 'shikashi', 'soul', 'garden', 'crystals'];
  const DEFAULT_STYLE = 'alchemy';

  function styleOf(id) { return Object.prototype.hasOwnProperty.call(STYLES, id) ? id : DEFAULT_STYLE; }

  function assetHtml(name, style, extra = '') {
    const set = styleOf(style);
    const a = STYLES[set].assets[name] || STYLES[set].assets.clock;
    const native = a.native || 32;
    const variable = !!a.width;
    const vars = [`background-image:url('${ASSET_ROOT}${a.file}')`, `background-position:-${a.x || 0}px -${a.y || 0}px`, `--native:${native}`, `--pixel-multiplier:${32 / native}`];
    if (variable) vars.push(`--frame-width:${a.width}`, `--frame-height:${a.height}`, `--strip-duration:${(a.frames / a.fps).toFixed(3)}s`);
    if (a.frames) vars.push(`--frames:${a.frames}`, `--strip-end:-${a.frames * (a.width || native)}px`);
    const cls = ['pm-asset', 'pm-asset-' + name, a.frames ? 'pm-framed' : '', variable ? 'pm-variable' : '', a.motion ? 'pm-motion-' + a.motion : '', extra].filter(Boolean).join(' ');
    return `<span class="${cls}" aria-hidden="true" data-art="${set}" data-asset="${name}"><span class="pm-pixel" style="${vars.join(';')}"></span></span>`;
  }

  function artFiles() {
    const out = new Set();
    for (const set of Object.values(STYLES)) for (const a of Object.values(set.assets)) out.add(a.file);
    return [...out].sort();
  }

  return { MIN, MIN_TARGET_MS, MAX_TARGET_MS, elapsedMs, remainingMs, fmt, deltaForRemaining, createClient, newRequestId,
    STYLES, STYLE_ORDER, DEFAULT_STYLE, ART_SOURCES, styleOf, assetHtml, artFiles };
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
    style: P.DEFAULT_STYLE,
  };
  const sounds = () => (window.uiSounds && typeof window.uiSounds.play === 'function' ? window.uiSounds : null);
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
    ui.style = P.styleOf(settings.style);
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
    const html = `${P.assetHtml('clock', ui.style, 'pm-header-clock')}<span class="pomo-mini">${esc(mini)}</span>`;
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
    const prog = v.snapshot && v.snapshot.progress;
    const levelUp = prog && prog.lastLevelUp && prog.lastLevelUp.blockId === b.blockId ? prog.lastLevelUp : null;
    if (v.serverNowMs - (b.endedAtMs || 0) < 90 * 1000) {
      if (levelUp) sounds()?.play('level-up', { eventId: `pomodoro:level:${prog.policyVersion}:${levelUp.level}` });
      else sounds()?.play(b.mode === 'focus' ? 'focus-complete' : 'break-complete', { eventId: `pomodoro:${b.blockId}:completed` });
    }
    const xp = prog && prog.xpPerMinute ? Math.floor(b.activeMs / MIN) * prog.xpPerMinute : 0;
    ui.banner = b.mode === 'focus'
      ? t(`Bloque completado · ${Math.round(b.activeMs / MIN)} min en ${b.project || 'sin proyecto'}`, `Block complete · ${Math.round(b.activeMs / MIN)} min`) + (xp ? ` · +${xp} XP` : '') + (levelUp ? t(` · ¡Nivel ${levelUp.level}!`, ` · Level ${levelUp.level}!`) : '')
      : t('Descanso terminado · listo para continuar', 'Break over · ready to continue');
    ui.bannerLevel = !!levelUp;
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
        <div class="pm-ruler-clock">${P.assetHtml('clock', ui.style)}
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
      ${ui.banner ? `<div class="pm-banner" role="status">${ui.bannerLevel ? P.assetHtml('level', ui.style) : ''}<span>${esc(ui.banner)}</span></div>` : ''}
      ${miniHtml(v)}
      ${err}
      <button type="button" class="pm-link" data-pm-analytics>${icon('bars', 12)} ${t('Ver mi historial y progreso', 'See my history and progress')}</button>
      ${soundHtml()}
      ${styleHtml()}
      <div data-pm-extra></div>`;
  }

  function miniHtml(v) {
    const g = v.snapshot && v.snapshot.progress;
    if (!g) return '';
    const goal = Math.min(100, (g.todayMinutes / Math.max(1, g.dailyGoalMinutes)) * 100);
    return `<div class="pm-milestone">
        <div class="pm-mini-level">${P.assetHtml('level', ui.style)}<div><b>${t('Nivel', 'Level')} ${g.level}</b><small>${g.xp.toLocaleString('es-MX')} XP</small></div></div>
        <div class="pm-mini-track">
          <div class="pm-progress" role="progressbar" aria-label="${t('Progreso al siguiente nivel', 'Progress to next level')}" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${Math.round(g.levelPct)}"><span style="width:${g.levelPct}%"></span></div>
          <p><span>${g.xpToNextLevel} ${t('XP para subir', 'XP to level up')}</span><span>${g.todayMinutes} / ${g.dailyGoalMinutes} min ${t('hoy', 'today')}</span></p>
          <div class="pm-progress goal" aria-hidden="true"><span style="width:${goal.toFixed(1)}%"></span></div>
        </div>
      </div>`;
  }

  window.ComandosPomodoroProgress = function (slot, snapshot) {
    const g = snapshot && snapshot.progress;
    if (!g) { slot.innerHTML = ''; return; }
    const goal = Math.min(100, (g.todayMinutes / Math.max(1, g.dailyGoalMinutes)) * 100);
    slot.innerHTML = `<section class="pm-game">
        <div class="pm-game-level">${P.assetHtml('level', ui.style)}<div><div class="pm-kicker">${t('Tu progreso · todos los proyectos', 'Your progress · all projects')}</div>
          <h4>${t('Nivel', 'Level')} ${g.level} <small>· ${g.xp.toLocaleString('es-MX')} XP</small></h4></div></div>
        <div class="pm-progress"><span style="width:${g.levelPct}%"></span></div>
        <small>${g.xpToNextLevel} XP ${t('para el nivel', 'to level')} ${g.level + 1} · ${g.xpPerMinute} XP ${t('por minuto de foco medido', 'per measured focus minute')}</small>
        <div class="pm-badges">${(g.achievements || []).map(a => `<span class="${a.unlocked ? '' : 'locked'}" title="${a.unlocked ? t('Logro conseguido', 'Achievement unlocked') : t('Logro pendiente', 'Pending')}">${P.assetHtml(a.asset, ui.style)}${esc(a.title)}</span>`).join('')}</div>
        <div class="pm-goal-head">${P.assetHtml('crystal', ui.style)}<b>${t('Meta de hoy', 'Today')}: ${g.todayMinutes} / ${g.dailyGoalMinutes} min</b></div>
        <div class="pm-progress goal"><span style="width:${goal.toFixed(1)}%"></span></div>
        <p class="pma-note">${g.streakDays} ${t('días seguidos con un bloque completado. Tu XP y tus logros se conservan al descansar. Solo cuentan bloques terminados desde que se activó la política', 'days in a row. XP and achievements are kept. Only blocks finished since the policy started count')} ${esc(g.policyVersion)}.</p>
      </section>`;
  };

  function soundHtml() {
    const snd = sounds();
    if (!snd) return '';
    const on = snd.isEnabled();
    const vol = Math.round(snd.getVolume() * 100);
    const previews = [['focus-start', t('Inicio', 'Start')], ['focus-pause', t('Pausa', 'Pause')], ['focus-complete', t('Completado', 'Complete')], ['break-complete', t('Descanso', 'Break')], ['level-up', t('Subir de nivel', 'Level up')]];
    return `<div class="pm-audio">
        <button type="button" data-pm-sound aria-pressed="${on}">${icon('bell', 13)} ${on ? t('Sonido activado', 'Sound on') : t('Activar sonido', 'Enable sound')}</button>
        <label>${t('Volumen', 'Volume')} <input type="range" data-pm-volume min="0" max="100" value="${vol}" aria-label="${t('Volumen de efectos', 'Effects volume')}"></label>
        <button type="button" data-pm-preview="focus-complete">${t('Escuchar final', 'Hear the end')}</button>
      </div>
      <details class="pm-sound-options"><summary>${t('Probar sonidos de videojuego', 'Try the game sounds')}</summary>
        ${previews.map(([cue, label]) => `<button type="button" data-pm-preview="${cue}">${label}</button>`).join('')}
      </details>`;
  }

  function styleHtml() {
    const set = P.STYLES[ui.style];
    const credit = set.sources.map(k => { const a = P.ART_SOURCES[k]; return `<a href="${a.url}" target="_blank" rel="noopener">${esc(a.author)}</a> · <a href="${a.licenseUrl}" target="_blank" rel="noopener">${esc(a.license)}</a>`; }).join(' / ');
    return `<div class="pm-art-controls">
        <label>${t('Estilo', 'Style')} <select data-pm-style aria-label="${t('Estilo de Pomodoro (toda la app)', 'Pomodoro style (whole app)')}">${P.STYLE_ORDER.map(k => `<option value="${k}" ${k === ui.style ? 'selected' : ''}>${esc(P.STYLES[k].title)}</option>`).join('')}</select></label>
        <span class="pm-art-samples">${['crystal', 'first', 'hundred', 'streak', 'level'].map(n => P.assetHtml(n, ui.style)).join('')}</span>
        <small>${t('Arte', 'Art')}: ${credit}</small>
      </div>`;
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
    const cue = { start: 'focus-start', resume: 'focus-resume', pause: 'focus-pause' }[action];
    if (res.ok && cue) sounds()?.play(cue, { eventId: 'pomodoro-cmd:' + res.request.requestId });
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
    panel.querySelector('[data-pm-sound]')?.addEventListener('click', e => {
      e.stopPropagation();
      const snd = sounds(); if (!snd) return;
      const next = !snd.isEnabled();
      snd.setEnabled(next);
      if (next) snd.unlock(e);
      render();
    });
    panel.querySelector('[data-pm-analytics]')?.addEventListener('click', e => {
      e.stopPropagation(); panel.classList.add('hidden');
      if (typeof window.openAnalyticsTab === 'function') window.openAnalyticsTab('pomodoro');
    });
    panel.querySelector('[data-pm-volume]')?.addEventListener('input', e => { sounds()?.setVolume(Number(e.target.value) / 100); });
    panel.querySelectorAll('[data-pm-preview]').forEach(btn => btn.addEventListener('click', e => {
      e.stopPropagation(); sounds()?.unlock(e); sounds()?.preview(btn.dataset.pmPreview);
    }));
    panel.querySelector('[data-pm-style]')?.addEventListener('change', async e => {
      const style = P.styleOf(e.target.value);
      const previous = ui.style;
      ui.style = style; render();
      try {
        const r = await transport('POST', '/pomodoro', { settings: { style } });
        if (r.status !== 200) throw new Error((r.body && r.body.error) || 'no guardado');
        applySettings(r.body.settings);
      } catch (err) { ui.style = previous; say(t('No se pudo guardar el estilo', 'Could not save the style'), true); }
      render();
    });
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
    document.addEventListener('visibilitychange', () => {
      document.documentElement.classList.toggle('pm-page-hidden', document.hidden);
      if (!document.hidden) { client.refresh(); scheduleRefresh(); }
    });
    setInterval(() => { if (!document.hidden) tick(); }, 1000);
    client.refresh().then(scheduleRefresh);
  }

  /* ---- Analytics › Pomodoro: measured records only, filters by date and project. */
  const TZ = 'America/Mexico_City';
  const an = { period: '7', from: '', to: '', project: '', day: null, report: null, error: '', loading: false };
  function mxDate(ms) { return new Intl.DateTimeFormat('en-CA', { timeZone: TZ, year: 'numeric', month: '2-digit', day: '2-digit' }).format(new Date(ms)); }
  function mxTime(ms) { return new Intl.DateTimeFormat('es-MX', { timeZone: TZ, hour: '2-digit', minute: '2-digit', hour12: false }).format(new Date(ms)); }
  function minutes(ms) { return Math.round((ms || 0) / MIN); }
  function periodDates() {
    if (an.period === 'custom' && an.from && an.to) return [an.from, an.to];
    const days = Number(an.period) || 7;
    const today = mxDate(client.serverNow());
    const start = mxDate(Date.parse(today + 'T12:00:00Z') - (days - 1) * 86400000);
    return [start, today];
  }
  async function loadAnalytics() {
    const box = document.getElementById('pomodoro-analytics');
    if (!box) return;
    const [from, to] = periodDates();
    an.from = from; an.to = to; an.loading = true; renderAnalytics();
    const q = new URLSearchParams({ fromDate: from, toDate: to });
    if (an.project) q.set('project', an.project);
    try {
      const r = await transport('GET', '/pomodoro/report?' + q);
      if (r.status !== 200) throw new Error((r.body && r.body.error) || 'Sin datos');
      an.report = r.body; an.error = '';
      if (an.day && !an.report.byDay.some(d => d.date === an.day)) an.day = null;
    } catch (e) { an.error = t('No se pudo cargar el historial de Pomodoro. ', 'Could not load Pomodoro history. ') + e.message; }
    an.loading = false;
    renderAnalytics();
  }
  function renderAnalytics() {
    const box = document.getElementById('pomodoro-analytics');
    if (!box) return;
    const r = an.report;
    const m = r && r.measured;
    const maxDay = r ? Math.max(1, ...r.byDay.map(d => d.activeMs)) : 1;
    const history = r ? r.history.filter(h => !an.day || mxDate(h.startedAtMs) === an.day) : [];
    const status = { completed: t('completado', 'completed'), cancelled: t('cancelado', 'cancelled'), skipped: t('saltado', 'skipped'), unknown: t('sin cierre', 'unknown') };
    box.innerHTML = `
      <div class="pma-head"><p>${t('Tiempo de foco medido por bloque. Las pausas no cuentan y los descansos van aparte. Mide el temporizador, no tu atención.', 'Focus time measured per block. Pauses never count and breaks are separate.')}</p></div>
      <div class="pma-filters">
        <label>${t('Periodo', 'Period')} <select data-pma-period>
          ${[['1', t('Hoy', 'Today')], ['7', t('7 días', '7 days')], ['30', t('30 días', '30 days')], ['custom', t('Fechas', 'Dates')]].map(([v, l]) => `<option value="${v}" ${an.period === v ? 'selected' : ''}>${l}</option>`).join('')}
        </select></label>
        ${an.period === 'custom' ? `<label>${t('Desde', 'From')} <input type="date" data-pma-from value="${esc(an.from)}"></label><label>${t('Hasta', 'To')} <input type="date" data-pma-to value="${esc(an.to)}"></label>` : ''}
        <label>${t('Proyecto', 'Project')} <select data-pma-project><option value="">${t('Todos', 'All')}</option>
          ${((r && r.projects) || (an.project ? [an.project] : [])).map(p => `<option value="${esc(p)}" ${p === an.project ? 'selected' : ''}>${esc(p)}</option>`).join('')}
        </select></label>
      </div>
      ${an.error ? `<div class="pm-error" role="alert">${esc(an.error)} <button type="button" data-pma-retry>${t('Reintentar', 'Retry')}</button></div>` : ''}
      ${an.loading && !r ? `<p class="pma-empty">${t('Cargando…', 'Loading…')}</p>` : ''}
      ${r ? `
      <div class="pma-tiles">
        <div><b>${m.completed.blocks}</b><small>${t('completados', 'completed')}</small></div>
        <div><b>${m.cancelled.blocks}</b><small>${t('cancelados', 'cancelled')}</small></div>
        <div><b>${minutes(m.activeMs)}′</b><small>${t('foco medido', 'measured focus')}</small></div>
        <div><b>${minutes(m.plannedMs)}′</b><small>${t('planeado', 'planned')}</small></div>
        <div><b>${m.completionRate == null ? '—' : Math.round(m.completionRate * 100) + '%'}</b><small>${t('completado', 'completion')}</small></div>
      </div>
      <div class="pma-bars" role="list">${r.byDay.map(d => `<button type="button" role="listitem" data-pma-day="${d.date}" class="${an.day === d.date ? 'on' : ''}" title="${d.date}: ${minutes(d.activeMs)} min · ${d.completed} ${t('completados', 'completed')} · ${d.cancelled} ${t('cancelados', 'cancelled')}"><span style="height:${Math.round(100 * d.activeMs / maxDay)}%"></span><small>${d.date.slice(8)}</small></button>`).join('')}</div>
      ${r.byProject.length ? `<div class="pma-projects">${r.byProject.map(p => `<div><span>${esc(p.project || t('sin proyecto', 'no project'))}</span><b>${minutes(p.activeMs)} min</b><small>${p.completed || 0} ✓ · ${p.cancelled || 0} ✕${p.legacyPlannedMs ? ` · ${minutes(p.legacyPlannedMs)} min ${t('planeados antes', 'planned before')}` : ''}</small></div>`).join('')}</div>` : ''}
      ${r.legacy.blocks ? `<p class="pma-note">${esc(r.legacy.note)} ${r.legacy.completedBlocks} ${t('bloques', 'blocks')} · ${minutes(r.legacy.plannedMs)} min ${t('planeados', 'planned')}.</p>` : ''}
      ${r.breaks.blocks ? `<p class="pma-note">${t('Descansos', 'Breaks')}: ${r.breaks.blocks} · ${minutes(r.breaks.activeMs)} min (${t('no cuentan como foco', 'not counted as focus')})</p>` : ''}
      <h4>${an.day ? t('Historial del ', 'History for ') + an.day + ` <button type="button" data-pma-day="">${t('ver todo', 'show all')}</button>` : t('Historial', 'History')}</h4>
      ${history.length ? `<div class="pma-history">${history.map(h => `<div class="pma-row ${h.mode} ${h.status}">
          <span>${mxDate(h.startedAtMs).slice(5)} ${mxTime(h.startedAtMs)}</span>
          <span>${esc(h.project || '—')}</span>
          <span>${h.mode === 'break' ? t('descanso', 'break') : t('foco', 'focus')} · ${status[h.status] || esc(h.status)}</span>
          <span>${h.provenance === 'measured' ? minutes(h.activeMs) + ' / ' + minutes(h.plannedMs) + ' min' : minutes(h.plannedMs) + ' min ' + t('planeados (historial anterior)', 'planned (earlier history)')}</span>
        </div>`).join('')}</div>` : `<p class="pma-empty">${t('Sin bloques en este periodo.', 'No blocks in this period.')}</p>`}
      <div data-pm-progress></div>` : ''}`;
    box.querySelector('[data-pma-period]')?.addEventListener('change', e => { an.period = e.target.value; an.day = null; if (an.period !== 'custom') loadAnalytics(); else renderAnalytics(); });
    box.querySelectorAll('[data-pma-from],[data-pma-to]').forEach(el => el.addEventListener('change', () => {
      const f = box.querySelector('[data-pma-from]').value, to = box.querySelector('[data-pma-to]').value;
      if (f && to && f <= to) { an.from = f; an.to = to; an.day = null; loadAnalytics(); }
    }));
    box.querySelector('[data-pma-project]')?.addEventListener('change', e => { an.project = e.target.value; an.day = null; loadAnalytics(); });
    box.querySelector('[data-pma-retry]')?.addEventListener('click', loadAnalytics);
    box.querySelectorAll('[data-pma-day]').forEach(b => b.addEventListener('click', () => { an.day = b.dataset.pmaDay && an.day !== b.dataset.pmaDay ? b.dataset.pmaDay : null; renderAnalytics(); }));
    if (typeof window.ComandosPomodoroProgress === 'function') {
      const slot = box.querySelector('[data-pm-progress]');
      if (slot) { try { window.ComandosPomodoroProgress(slot, client.snapshot()); } catch (e) { /* optional */ } }
    }
  }
  window.ComandosPomodoro.loadAnalytics = loadAnalytics;
  window.ComandosPomodoro.analytics = an;

  window.pomoRender = () => { render(); client.refresh(); };
  window.ComandosPomodoro.ui = { client, render, state: ui, command, setMinutes, startBlock };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
  else init();
})();
