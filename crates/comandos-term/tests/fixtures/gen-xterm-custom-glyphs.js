// Herramienta de pruebas (no se publica ni se carga en ninguna página):
// regenera xterm-custom-glyphs.json ejecutando `tryDrawCustomChar` del
// bundle vendorizado de @xterm/addon-canvas 0.7.0 sobre un contexto 2D falso
// que anota cada llamada. Ver README.md de este directorio.
//
// Uso (desde la raíz del repo):
//   node crates/comandos-term/tests/fixtures/gen-xterm-custom-glyphs.js \
//     assets/xterm/addon-canvas.js > crates/comandos-term/tests/fixtures/xterm-custom-glyphs.json
'use strict';
const fs = require('fs');

const src = fs.readFileSync(process.argv[2], 'utf8');

// Módulo webpack de CustomGlyphs: desde el último `N:(e,t,i)=>{` anterior a
// `blockElementDefinitions` hasta el siguiente `},N:(e,t` tras
// `tryDrawCustomChar`.
const anchor = src.indexOf('blockElementDefinitions=void 0');
if (anchor < 0) throw new Error('no encuentro blockElementDefinitions');
const startRe = /(\d+):\(e,t,i\)=>\{/g;
let start = -1;
for (let m; (m = startRe.exec(src)) && m.index < anchor; ) start = m.index + m[0].length;
const endRe = /\},\d+:\(e,t/g;
endRe.lastIndex = src.indexOf('t.tryDrawCustomChar=', anchor);
const end = endRe.exec(src);
if (start < 0 || !end) throw new Error('no encuentro el módulo de CustomGlyphs');
const glyphs = {};
// El módulo solo usa `throwIfFalsy` de su dependencia.
new Function('e', 't', 'i', src.slice(start, end.index))({}, glyphs, () => ({ throwIfFalsy: (x) => x }));

globalThis.Path2D = class { rect() {} };
globalThis.ImageData = class {
  constructor(w, h) { this.width = w; this.height = h; this.data = new Uint8ClampedArray(w * h * 4); }
};

// Contexto que anota las llamadas con los nombres de `glyphs::DrawOp`.
function recorder() {
  const ops = [];
  const ctx = {
    fillStyle: '#ffffff',
    strokeStyle: null,
    lineWidth: 1,
    fillRect(x, y, w, h) {
      if (this.fillStyle && this.fillStyle.mask) ops.push(['FillPattern', this.fillStyle.mask]);
      else ops.push(['FillRect', x, y, w, h]);
    },
    beginPath: () => ops.push(['BeginPath']),
    moveTo: (x, y) => ops.push(['MoveTo', x, y]),
    lineTo: (x, y) => ops.push(['LineTo', x, y]),
    bezierCurveTo: (a, b, c, d, e, f) => ops.push(['CurveTo', a, b, c, d, e, f]),
    stroke() { ops.push(['Stroke', this.lineWidth]); },
    fill: () => ops.push(['Fill']),
    closePath: () => {},
    clip: () => ops.push(['ClipCell']),
    // Una trama se anota como su máscara 0/1 (alfa de la ImageData).
    createPattern: (canvas) => {
      const d = canvas.image;
      const mask = [];
      for (let y = 0; y < d.height; y++) {
        const row = [];
        for (let x = 0; x < d.width; x++) row.push(d.data[4 * (y * d.width + x) + 3] ? 1 : 0);
        mask.push(row);
      }
      return { mask };
    },
    canvas: {
      ownerDocument: {
        createElement: () => {
          const c = { width: 0, height: 0, getContext: () => ({ putImageData: (d) => { c.image = d; } }) };
          return c;
        },
      },
    },
  };
  return { ctx, ops };
}

// [deviceCellWidth, deviceCellHeight, devicePixelRatio, fontSize].
const CELLS = [[9, 20, 1, 14], [13, 29, 1.25, 15]];
const chars = [];
for (let c = 0x2500; c <= 0x259f; c++) chars.push(c);
for (let c = 0x1fb70; c <= 0x1fb8b; c++) chars.push(c);
for (let c = 0x1fb95; c <= 0x1fb97; c++) chars.push(c);
for (let c = 0xe0b0; c <= 0xe0bf; c++) chars.push(c);

const entries = [];
for (const [w, h, dpr, font] of CELLS) {
  for (const cp of chars) {
    const { ctx, ops } = recorder();
    if (!glyphs.tryDrawCustomChar(ctx, String.fromCodePoint(cp), 0, 0, w, h, font, dpr)) {
      throw new Error(`xterm.js no dibuja U+${cp.toString(16)}`);
    }
    entries.push([`${w}x${h}@${dpr}/${font}:${cp.toString(16)}`, ops]);
  }
}
entries.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
const lines = entries.map(([k, v]) => `${JSON.stringify(k)}:${JSON.stringify(v)}`);
process.stdout.write(`{\n${lines.join(',\n')}\n}\n`);
