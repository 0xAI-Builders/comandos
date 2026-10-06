// Herramienta de pruebas (no se publica ni se carga en ninguna página):
// regenera xterm-fit.json ejecutando el código del bundle vendorizado —
// `FitAddon.fit()` de @xterm/addon-fit 0.10 y
// `CanvasRenderer._updateDimensions` de @xterm/addon-canvas 0.7.0 — sobre
// una terminal falsa. Ver README.md de este directorio.
//
// Uso (desde la raíz del repo):
//   node crates/comandos-term-web/tests/fixtures/gen-xterm-fit.js \
//     assets/xterm/addon-fit.js assets/xterm/addon-canvas.js \
//     > crates/comandos-term-web/tests/fixtures/xterm-fit.json
'use strict';
const fs = require('fs');
const vm = require('vm');

const [fitPath, canvasPath] = process.argv.slice(2);
if (!fitPath || !canvasPath) throw new Error('uso: gen-xterm-fit.js addon-fit.js addon-canvas.js');

// `_updateDimensions(){…}` del CanvasRenderer, con sus llaves equilibradas.
const canvasSrc = fs.readFileSync(canvasPath, 'utf8');
const start = canvasSrc.indexOf('_updateDimensions(){');
if (start < 0) throw new Error('no encuentro _updateDimensions');
let depth = 0;
let end = -1;
for (let i = start; i < canvasSrc.length; i++) {
  const c = canvasSrc[i];
  if (c === '{') depth++;
  if (c === '}' && --depth === 0) { end = i + 1; break; }
}
if (end < 0) throw new Error('_updateDimensions sin cerrar');
const updateDimensions = new Function(`return {${canvasSrc.slice(start, end)}}`)()._updateDimensions;

// `window.getComputedStyle` del addon: el padre mide W×H, la terminal no
// tiene relleno (xterm.css).
const PARENT = {};
const ELEMENT = { parentElement: PARENT };
let parentSize = { w: 0, h: 0 };
const fakeWindow = {
  getComputedStyle(el) {
    return {
      getPropertyValue(k) {
        if (el === PARENT) return k === 'height' ? `${parentSize.h}px` : k === 'width' ? `${parentSize.w}px` : '0px';
        return '0px';
      },
    };
  },
};

// FitAddon: el UMD se registra en `self.FitAddon` y lee `window` de su contexto.
const sandbox = { self: {}, window: fakeWindow };
vm.runInNewContext(fs.readFileSync(fitPath, 'utf8'), sandbox);
const FitAddon = sandbox.self.FitAddon && sandbox.self.FitAddon.FitAddon;
if (!FitAddon) throw new Error('no encuentro FitAddon en el bundle');

// `term.html`: barra de 10 px (CSS) y scrollback 10000.
const SCROLLBAR = 10;

function fakeTerminal(font, dpr) {
  const renderer = {
    _charSizeService: { hasValidSize: true, width: font.w, height: font.h },
    _coreBrowserService: { dpr },
    _optionsService: { rawOptions: { lineHeight: font.lineHeight, letterSpacing: font.letterSpacing } },
    _bufferService: { cols: 80, rows: 24 },
    dimensions: {
      css: { canvas: { width: 0, height: 0 }, cell: { width: 0, height: 0 } },
      device: { canvas: { width: 0, height: 0 }, cell: { width: 0, height: 0 }, char: { width: 0, height: 0, left: 0, top: 0 } },
    },
  };
  const update = () => updateDimensions.call(renderer);
  update();
  const term = {
    element: ELEMENT,
    options: { scrollback: 10000 },
    get cols() { return renderer._bufferService.cols; },
    get rows() { return renderer._bufferService.rows; },
    resize(cols, rows) {
      renderer._bufferService.cols = cols;
      renderer._bufferService.rows = rows;
      update();
    },
    _core: { _renderService: { dimensions: renderer.dimensions, clear() {} }, viewport: { scrollBarWidth: SCROLLBAR } },
  };
  const fit = new FitAddon();
  fit.activate(term);
  return { term, fit };
}

const FONTS = {
  // Ubuntu Sans Mono 14 px de `term.html` y 11 px de las preferencias (medidas de ejemplo).
  '14px': { w: 8.4, h: 17, lineHeight: 1.2, letterSpacing: 0 },
  '11px': { w: 6.6, h: 13, lineHeight: 1.2, letterSpacing: 0 },
};

const cases = [];
// Cada caso: desde 80×24, tres `fit()` seguidos sobre el mismo contenedor
// (la columna actual cambia la celda CSS: hay histéresis).
function run(fontName, dpr, w, h) {
  const { term, fit } = fakeTerminal(FONTS[fontName], dpr);
  parentSize = { w, h };
  const steps = [];
  for (let i = 0; i < 3; i++) {
    fit.fit();
    steps.push([term.cols, term.rows]);
  }
  cases.push({ font: fontName, dpr, w, h, steps });
}

const named = [[390, 3], [412, 2.625], [360, 3], [1400, 1], [844, 2], [320, 2], [377, 3]];
for (const font of Object.keys(FONTS)) {
  for (const [w, dpr] of named) for (const h of [700, 1969, 1987]) run(font, dpr, w, h);
  for (const dpr of [1, 1.25, 2, 2.625, 3]) {
    for (let w = 300; w <= 2000; w += 7) run(font, dpr, w, 800 + (w % 13) * 37);
  }
}

process.stdout.write(JSON.stringify({ scrollbar: SCROLLBAR, fonts: FONTS, cases }) + '\n');
