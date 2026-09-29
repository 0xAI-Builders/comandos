/* Shared UI sound adapter (UISFX 0.4.0, vendored in /assets/uisfx, MIT).
 *
 *   uiSounds.play(cue, {eventId, volume})  one short cue; null when muted,
 *                                          hidden, not unlocked or already
 *                                          played for that eventId
 *   uiSounds.preview(cue)                  audition from a human gesture,
 *                                          even while muted
 *   uiSounds.stop()                        stop everything now
 *
 * Also: setEnabled(bool), isEnabled(), setVolume(0..1), getVolume(),
 * register(name, uisfxCue), cues(), unlock(event).
 *
 * Rules: opt-in (muted by default), no loops ever, one playback per eventId
 * (shared by the tabs of this browser), silent while the page is hidden,
 * audio created only after a trusted gesture. No remote audio is fetched. */
(function (root, factory) {
  const api = factory();
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.ComandosUISounds = api;
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  const LOOP_CUES = Object.freeze(['loading', 'processing', 'recording', 'connecting', 'scanning', 'streaming']);
  const DEFAULT_CATALOG = Object.freeze({
    // Pomodoro (approved prototype mapping, arcade pack)
    'focus-start': 'open', 'focus-resume': 'open', 'focus-pause': 'close',
    'focus-complete': 'complete', 'break-complete': 'success', 'level-up': 'level-up',
    // Generic cues for notices (the notification owner chooses among these)
    attention: 'notification', permission: 'notification', error: 'error',
    warning: 'warning', success: 'success', complete: 'complete',
  });
  const MAX_VOLUME = 0.3;          // master ceiling; user volume 1.0 maps here
  const KEYS = { enabled: 'comandos:ui-sounds:enabled', volume: 'comandos:ui-sounds:volume', played: 'comandos:ui-sounds:played' };
  const PLAYED_LIMIT = 300;

  function createUISounds({ loadEngine, win, doc, storage, pack = 'arcade', catalog = DEFAULT_CATALOG } = {}) {
    const safe = fn => { try { return fn(); } catch (e) { return undefined; } };
    const read = (k, d) => { const v = safe(() => storage && storage.getItem(k)); return v == null ? d : v; };
    const write = (k, v) => safe(() => storage && storage.setItem(k, v));
    const names = new Map(Object.entries(catalog));
    let enabled = read(KEYS.enabled, 'off') === 'on';
    let volume = Math.max(0, Math.min(1, Number(read(KEYS.volume, '0.6'))));
    if (!Number.isFinite(volume)) volume = 0.6;
    let engine = null, enginePromise = null, player = null, armed = false, disposed = false;
    const playedMemory = new Set();

    function visible() { return !doc || doc.visibilityState !== 'hidden'; }

    function ensureEngine() {
      if (engine || !loadEngine) return Promise.resolve(engine);
      if (!enginePromise) enginePromise = Promise.resolve().then(loadEngine).then(m => { engine = m; return m; }, () => null);
      return enginePromise;
    }

    function ensurePlayer() {
      if (player || !engine) return player;
      player = safe(() => engine.createUISFX({ pack, enabled: true, volume: volume * MAX_VOLUME, maxVoices: 2, cooldownMs: 180 })) || null;
      return player;
    }

    function trusted(event) {
      if (event && event.isTrusted === true) return true;
      const ua = win && win.navigator && win.navigator.userActivation;
      return !!(ua && ua.isActive);
    }

    // Must run inside the gesture's call stack: the AudioContext resumes here.
    function unlock(event) {
      if (disposed || !trusted(event) || !visible()) return false;
      if (!engine) { ensureEngine(); return false; }
      const p = ensurePlayer();
      if (!p) return false;
      const attempt = safe(() => p.unlock());
      Promise.resolve(attempt).then(ok => { if (ok === true && !disposed) armed = true; }, () => {});
      return true;
    }

    function playedBefore(eventId) {
      if (playedMemory.has(eventId)) return true;
      const list = safe(() => JSON.parse(read(KEYS.played, '[]'))) || [];
      return Array.isArray(list) && list.includes(eventId);
    }

    function remember(eventId) {
      playedMemory.add(eventId);
      const list = safe(() => JSON.parse(read(KEYS.played, '[]')));
      const next = (Array.isArray(list) ? list : []).filter(x => x !== eventId);
      next.push(eventId);
      write(KEYS.played, JSON.stringify(next.slice(-PLAYED_LIMIT)));
    }

    function resolve(cue) {
      const name = names.get(cue) || (typeof cue === 'string' && cue);
      if (!name || LOOP_CUES.includes(name)) return null;
      if (engine && Array.isArray(engine.cueNames) && !engine.cueNames.includes(name)) return null;
      return name;
    }

    function fire(name, gain) {
      const p = ensurePlayer();
      if (!p) return null;
      const options = { loop: false, retrigger: 'ignore' };
      if (typeof gain === 'number' && Number.isFinite(gain)) options.volume = Math.max(0, Math.min(1, gain)) * volume * MAX_VOLUME;
      return safe(() => p.play(name, options)) || null;
    }

    function play(cue, { eventId, volume: gain } = {}) {
      if (disposed || !enabled || !armed || !visible()) return null;
      const name = resolve(cue);
      if (!name) return null;
      if (eventId != null) {
        const id = String(eventId);
        if (playedBefore(id)) return null;
        remember(id);
      }
      return fire(name, gain);
    }

    function preview(cue) {
      if (disposed || !visible()) return null;
      const name = resolve(cue);
      if (!name) return null;
      unlock();
      if (!player) return null;
      armed = true;
      return fire(name);
    }

    function stop() { safe(() => player && player.stopAll()); }

    function setEnabled(next) {
      enabled = !!next;
      write(KEYS.enabled, enabled ? 'on' : 'off');
      if (!enabled) stop();
      return enabled;
    }

    function setVolume(next) {
      const v = Number(next);
      if (!Number.isFinite(v)) return volume;
      volume = Math.max(0, Math.min(1, v));
      write(KEYS.volume, String(volume));
      safe(() => player && player.setVolume(volume * MAX_VOLUME));
      return volume;
    }

    function register(name, uisfxCue) {
      if (typeof name !== 'string' || !name || typeof uisfxCue !== 'string' || LOOP_CUES.includes(uisfxCue)) return false;
      names.set(name, uisfxCue);
      return true;
    }

    const onGesture = event => { if (enabled) unlock(event); };
    const onHidden = () => { if (!visible()) stop(); };
    const listeners = [[doc, 'pointerdown', onGesture], [doc, 'keydown', onGesture], [doc, 'visibilitychange', onHidden], [win, 'pagehide', stop]];
    for (const [target, type, fn] of listeners) safe(() => target && target.addEventListener(type, fn, true));
    ensureEngine();

    return {
      play, preview, stop, unlock, setEnabled, setVolume, register,
      isEnabled: () => enabled, getVolume: () => volume, isReady: () => enabled && armed && visible(),
      cues: () => [...names.keys()],
      async dispose() {
        disposed = true; stop();
        for (const [target, type, fn] of listeners) safe(() => target && target.removeEventListener(type, fn, true));
        try { if (player) await player.destroy(); } catch (e) { /* optional audio never breaks teardown */ }
        player = null;
      },
    };
  }

  return { createUISounds, DEFAULT_CATALOG, LOOP_CUES, MAX_VOLUME, KEYS };
});

(function () {
  'use strict';
  if (typeof window === 'undefined' || typeof document === 'undefined' || window.uiSounds) return;
  let storage = null;
  try { storage = window.localStorage; } catch (e) { storage = null; }
  window.uiSounds = window.ComandosUISounds.createUISounds({
    win: window, doc: document, storage,
    loadEngine: () => import('/assets/uisfx/uisfx-0.4.0.js'),
  });
})();
