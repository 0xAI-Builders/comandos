(function (root) {
  'use strict';
  const KEY = 'cc-b-key';
  // api es un parámetro: no es el api() global.
  function helper(api) { return api('/local'); }
  root.Bee = { helper };
  window.openPane = (n) => n;
  localStorage.setItem(KEY, '1');
  setInterval(() => render(), 60 * 1000);
  window.webkit.messageHandlers.extensions.postMessage('close');
  document.getElementById("b-panel");
  if (!window.lazy && window.Bee) window.lazy = 2;
  window.lazy2 ??= 3;
  const bridge = window.webkit && window.webkit.messageHandlers
    && window.webkit.messageHandlers.centro;
  const msg = {type: 'reader'};
  bridge.postMessage(JSON.stringify(msg));
  window.render = ((orig) => function () { return orig(); })(render);
})(window);
