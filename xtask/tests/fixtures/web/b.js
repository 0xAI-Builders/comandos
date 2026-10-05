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
  window.render = ((orig) => function () { return orig(); })(render);
})(window);
