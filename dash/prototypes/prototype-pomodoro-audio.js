// Prototype-only sound preview. UISFX 0.4.0, opt-in, session-only preference.
import { createUISFX, packNames } from './vendor/uisfx-0.4.0.js';
import { createUISounds } from './prototype-ui-sounds.js';
let player;
let volume = .18;
const sounds = createUISounds({
  createPlayer: options => (player = createUISFX({...options, volume})),
  pack: packNames.includes('arcade') ? 'arcade' : 'retro',
  volume,
  key: 'comandos:prototype:pomodoro:sounds',
  browser: {
    document,
    addEventListener: window.addEventListener.bind(window),
    removeEventListener: window.removeEventListener.bind(window),
  },
});
window.pmAudio = {
  enabled: () => sounds.isEnabled(),
  ready: () => sounds.isReady(),
  async toggle(event) {
    await sounds.setEnabled(!sounds.isEnabled(), event);
    if (sounds.isEnabled() && !sounds.isReady()) window.say?.('El navegador no habilitó el audio. Puedes volver a probarlo.');
    window.pmPaintSound?.();
  },
  async preview(event, cue = 'complete') {
    await sounds.setEnabled(true, event);
    sounds.play(cue);
    window.pmPaintSound?.();
  },
  cue(name) { sounds.play(name); },
  unlock(event) { void sounds.unlockFromGesture(event); },
  volume(value) { volume = Math.max(0, Math.min(.3, Number(value) / 100 * .3)); player?.setVolume(volume); },
  stop: () => sounds.stop(),
};
window.pmPaintSound?.();
window.addEventListener('pagehide', () => { void sounds.dispose(); }, {once: true});
