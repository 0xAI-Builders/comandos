/* Per-device unsent text and reading position (W4).
 *
 * The composer's text lives on the server under this device's id, saved in
 * small debounced patches while typing (a crash must not lose it), and comes
 * back into the COMPOSER only. Nothing here can write to the terminal: there
 * is no send hook, so a restore never re-types a partial command. The
 * reading anchor is the text of the line at the top of the history view; if
 * that line is no longer in the available history, the caller says so
 * instead of jumping somewhere else.
 */
(function (root, factory) {
  const api = factory();
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.ComandosDeviceDrafts = api;
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  function createDrafts({ key, load, save, read, write, select, now = () => Date.now(),
                          delayMs = 600, schedule = setTimeout, cancel = clearTimeout }) {
    let timer = 0, lastSaved = null, pending = null;

    async function restore() {
      // Text typed here before the answer arrives always wins.
      if (read()) return 'local';
      let state = null;
      try { state = await load(); } catch (_) { return 'unavailable'; }
      const draft = state && state.drafts && state.drafts[key];
      if (!draft || typeof draft.text !== 'string' || !draft.text || read()) return 'none';
      write(draft.text);
      if (select && Number.isInteger(draft.selStart))
        select(draft.selStart, Number.isInteger(draft.selEnd) ? draft.selEnd : draft.selStart);
      lastSaved = draft.text;
      return 'restored';
    }

    function flush() {
      cancel(timer); timer = 0;
      if (!pending) return Promise.resolve(false);
      const { text, selStart, selEnd } = pending;
      pending = null;
      if (text === lastSaved) return Promise.resolve(false);
      const entry = text ? { text, selStart, selEnd, updatedAt: now() } : null;
      lastSaved = text;
      return Promise.resolve(save({ draftsPatch: { [key]: entry } })).then(() => true, () => {
        lastSaved = null;          // retry on the next change
        return false;
      });
    }

    function changed(text, selStart, selEnd) {
      pending = { text: String(text || ''), selStart, selEnd };
      cancel(timer);
      timer = schedule(flush, delayMs);
    }

    return { restore, changed, flush };
  }

  function lineAt(text, index) {
    const start = text.lastIndexOf('\n', Math.max(0, index - 1)) + 1;
    const end = text.indexOf('\n', start);
    return text.slice(start, end < 0 ? text.length : end);
  }

  function createAnchor({ key, load, save, now = () => Date.now(), delayMs = 800,
                          schedule = setTimeout, cancel = clearTimeout }) {
    let timer = 0, saved = null;

    async function find(text) {
      let state = null;
      try { state = await load(); } catch (_) { return { state: 'unavailable' }; }
      const anchor = state && state.readingAnchors && state.readingAnchors[key];
      if (!anchor || !anchor.text) return { state: 'none' };
      const index = text.lastIndexOf(anchor.text);
      return index < 0 ? { state: 'missing', anchor } : { state: 'found', index };
    }

    function remember(text, topIndex, ratio) {
      const line = lineAt(text, topIndex).trim().slice(0, 300);
      if (!line || line === saved) return;
      cancel(timer);
      timer = schedule(() => {
        saved = line;
        Promise.resolve(save({ anchorsPatch: { [key]: { text: line, ratio, updatedAt: now() } } }))
          .catch(() => { saved = null; });
      }, delayMs);
    }

    return { find, remember };
  }

  return { createDrafts, createAnchor, lineAt };
});
