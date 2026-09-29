// Terminal rápida (E2): una shell en su propia carpeta fechada.
// Un clic = un requestId. Mientras la petición no confirme, el mismo requestId
// se reutiliza (doble clic, error de red, recarga), así el servidor devuelve
// la MISMA terminal y nunca abre dos shells. Tras el éxito se descarta.
(function (root) {
  'use strict';
  const KEY = 'comandos.quickTerminal.pending';

  function newId() {
    return (root.crypto && root.crypto.randomUUID) ? root.crypto.randomUUID()
      : `qt-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  }

  function createQuickTerminal({ api, openTerm, toast = () => {}, onOpened = () => {}, storage = null, makeId = newId }) {
    let inflight = null;
    const read = () => { try { return storage && storage.getItem(KEY); } catch (_) { return null; } };
    const write = (value) => {
      try { if (!storage) return; value ? storage.setItem(KEY, value) : storage.removeItem(KEY); } catch (_) {}
    };
    let requestId = read();

    function open() {
      if (inflight) return inflight;
      if (!requestId) { requestId = makeId(); write(requestId); }
      const id = requestId;
      inflight = (async () => {
        try {
          const r = await api('/terminal/quick', { requestId: id });
          if (requestId === id) { requestId = null; write(null); }
          openTerm(r.tabId, r.label || r.tabId);
          onOpened(r);
          return r;
        } catch (err) {
          toast((err && err.message) || 'No se pudo abrir la terminal', true);
          return null;
        } finally {
          inflight = null;
        }
      })();
      return inflight;
    }

    return { open, get pendingRequestId() { return requestId; }, get busy() { return !!inflight; } };
  }

  root.ComandosQuickTerminal = { createQuickTerminal };
  if (typeof module !== 'undefined' && module.exports) module.exports = { createQuickTerminal };
})(typeof window !== 'undefined' ? window : globalThis);
