/** Opt-in one-shot adapter for UISFX 0.4.0. No audio is created on import. */
// Level-up is enabled for the user's explicitly requested Pomodoro gamification.
export const cues = Object.freeze(['success', 'error', 'copy', 'select', 'open', 'close', 'complete', 'level-up']);

export function createUISounds({
  createPlayer,
  browser = typeof window === 'undefined' ? undefined : window,
  key = 'comandos:prototype:pomodoro:sounds',
  pack = 'soft',
  volume = 0.18,
} = {}) {
  let storage;
  let enabled = false;
  try {
    storage = browser?.localStorage;
    enabled = storage?.getItem(key) === 'on';
  } catch { /* Storage restrictions leave the session usable and initially muted. */ }
  const doc = browser?.document;
  let player;
  let armed = false;
  let disposed = false;
  let generation = 0;
  let unlocking;
  const visible = () => Boolean(doc) && doc.visibilityState !== 'hidden';
  const safe = (fn) => { try { return fn(); } catch { return undefined; } };

  function stop() {
    generation += 1;
    armed = false;
    unlocking = undefined;
    safe(() => player?.stopAll());
  }

  async function unlockFromGesture(event) {
    if (disposed || !enabled || !visible() || event?.isTrusted !== true) return false;
    if (armed) return true;
    if (unlocking) return unlocking;
    const ticket = generation;
    // Call unlock synchronously in this gesture's stack, before any await/import.
    let attempt;
    try {
      player ??= createPlayer({
        pack, enabled: false,
        volume: Number.isFinite(volume) ? Math.max(0, Math.min(0.3, volume)) : 0.18,
        maxVoices: 2, cooldownMs: 180,
      });
      player.setEnabled(true);
      attempt = player.unlock();
    } catch { return false; }
    const pending = Promise.resolve(attempt).then((ok) => {
      if (disposed || !enabled || ticket !== generation || !visible()) return false;
      armed = ok === true;
      return armed;
    }, () => false);
    unlocking = pending;
    try { return await pending; }
    finally { if (unlocking === pending) unlocking = undefined; }
  }

  async function setEnabled(next, event) {
    if (disposed) return false;
    // A script or a timer must never opt a visitor in.
    if (next && event?.isTrusted !== true) return false;
    enabled = Boolean(next);
    safe(() => storage?.setItem(key, enabled ? 'on' : 'off'));
    if (!enabled) {
      stop();
      safe(() => player?.setEnabled(false));
      return false;
    }
    return unlockFromGesture(event);
  }

  function play(cue) {
    if (disposed || !enabled || !armed || !visible() || !cues.includes(cue)) return null;
    // Even if the catalog changes, this adapter never creates loops.
    return safe(() => player.play(cue, { loop: false, retrigger: 'ignore' })) ?? null;
  }

  function task() {
    const ticket = generation;
    // A request started muted must not acquire sound halfway through completion.
    const audible = enabled && armed && visible();
    let settled = false;
    return {
      finish(cue) {
        if (settled) return null;
        settled = true;
        if (!audible || disposed || ticket !== generation) return null;
        safe(() => player?.stopAll());
        return play(cue);
      },
      cancel() {
        if (settled) return;
        settled = true;
        // Do not invalidate unrelated requests; this token can never finish again.
        safe(() => player?.stopAll());
      },
    };
  }

  const onGesture = (event) => { void unlockFromGesture(event); };
  const onVisibility = () => { if (!visible()) stop(); };
  const listeners = [
    [doc, 'pointerdown', onGesture], [doc, 'keydown', onGesture],
    [doc, 'visibilitychange', onVisibility], [browser, 'pagehide', stop],
    [browser, 'popstate', stop], [browser, 'hashchange', stop],
  ];
  for (const [target, type, listener] of listeners) target?.addEventListener(type, listener);

  return {
    isEnabled: () => enabled,
    isReady: () => enabled && armed && !disposed && visible(),
    setEnabled, unlockFromGesture, play, task, stop,
    async dispose() {
      if (disposed) return;
      disposed = true;
      stop();
      for (const [target, type, listener] of listeners) target?.removeEventListener(type, listener);
      try { await player?.destroy(); } catch { /* Optional audio cannot break teardown. */ }
      player = undefined;
    },
  };
}
