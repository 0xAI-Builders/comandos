// addon-ligatures-web — ligaduras OpenType para xterm.js en browser puro.
//
// El addon oficial (@xterm/addon-ligatures) depende de font-ligatures + Node.js.
// Esta implementación es browser-only: parsea la TTF con opentype.js, indexa los
// glifos por nombre y — al detectar en el buffer secuencias como `!=`, `==`,
// `->`, `<=>` — busca el glyph `exclam_equal.liga` / `equal_equal.liga` /
// `hyphen_greater.liga` / etc. y lo dibuja en overlay sobre las celdas del
// terminal, tapando previamente el rectángulo con el bg color.
//
// Ventaja sobre `font.getPath(str, {features})`: opentype.js no aplica bien la
// feature `calt` que usa JetBrainsMono, así que shapeamos a mano con el mapa
// canonical `char → adobe glyph name`.

(function (global) {
  'use strict';

  // Adobe Glyph Naming — solo los que aparecen en ligs de JetBrainsMono/Fira/etc.
  var CHAR_TO_ADOBE = {
    '!':'exclam', '"':'quotedbl', '#':'numbersign', '$':'dollar', '%':'percent',
    '&':'ampersand', "'":'quotesingle', '(':'parenleft', ')':'parenright',
    '*':'asterisk', '+':'plus', ',':'comma', '-':'hyphen', '.':'period',
    '/':'slash', ':':'colon', ';':'semicolon', '<':'less', '=':'equal',
    '>':'greater', '?':'question', '@':'at', '[':'bracketleft', '\\':'backslash',
    ']':'bracketright', '^':'asciicircum', '_':'underscore', '`':'grave',
    '{':'braceleft', '|':'bar', '}':'braceright', '~':'asciitilde'
  };

  // Secuencias candidatas. Priorizamos las largas primero para greedy match.
  // (Este set cubre las más comunes de JetBrainsMono; si falta alguna, se
  // añade acá — el name lookup en la font descarta ligs no soportadas.)
  var CANDIDATES = [
    // 4 chars
    '<===>','<==>','<-->','<-<<','<<--','<<==','==>>','<==>','<==<','===>',
    // 3 chars
    '<=>','<=<','<==','===','==>','!==','<--','-->','-<<','->>','<<-','<<=',
    '>>=','||=','&&&','+++','---','***','...','::=','::<','::>','//=','//<','//>',
    '///','###','##_','?!.','?::','?<>','~~>','<~~','|||','|=>','||>','|>=',
    '<|>','<|=','|=<','}}<','{{-','!!}',
    // 2 chars
    '<=','>=','==','!=','=>','->','<-','<<','>>','::','&&','||','++','--','**',
    '//','/*','*/','</','/>','<>','<|','|>','##','..','::','.=','.?','?=','?.',
    '?:','~=','~@','~~','^=','<~','~>','$>','#!','#(','#{','#[','#:','#?','#=',
    '#_','\\/','/\\','_(','__','@_','}}','{{','{|','[|','[<','|-','|=','|]','|}',
    '|{',';:', '@?'
  ];
  // ordenar longest-first para greedy
  CANDIDATES.sort(function (a,b) { return b.length - a.length; });

  var LIG_RE = new RegExp(
    '(' + CANDIDATES.map(function (s) {
      return s.replace(/[.*+?^${}()|[\]\\]/g,'\\$&');
    }).join('|') + ')',
    'g'
  );

  function seqToLigName(seq) {
    var parts = [];
    for (var i = 0; i < seq.length; i++) {
      var name = CHAR_TO_ADOBE[seq[i]];
      if (!name) return null;
      parts.push(name);
    }
    return parts.join('_') + '.liga';
  }

  function LigaturesWebAddon(opts) {
    opts = opts || {};
    this._fontUrl = opts.fontUrl;
    this._fontSize = opts.fontSize || 14;
    this._fg = opts.foreground || '#EAF0FB';
    this._bg = opts.background || '#0A0D13';
    this._font = null;
    this._glyphByName = null;  // dict name -> glyph
    this._ligCache = {};       // seq -> glyph|false (miss)
    this._term = null;
    this._host = null;
    this._overlay = null;
    this._ctx = null;
    this._scheduled = false;
    this._onRenderDisposable = null;
    this._resizeHandler = null;
    this._debug = !!opts.debug;
  }

  LigaturesWebAddon.prototype._setDbg = function (msg) {
    if (!this._debug) return;
    var el = document.getElementById('dbg');
    if (el) el.textContent = msg;
  };

  LigaturesWebAddon.prototype.activate = function (term) {
    var self = this;
    this._term = term;

    var screen = term.element && term.element.querySelector('.xterm-screen');
    var host = screen || term.element;
    if (!host) return;
    var overlay = document.createElement('canvas');
    overlay.className = 'xterm-ligatures-overlay';
    overlay.style.cssText =
      'position:absolute; left:0; top:0; pointer-events:none; z-index:9999;';
    host.appendChild(overlay);
    this._host = host;
    this._overlay = overlay;
    this._ctx = overlay.getContext('2d', { alpha: true });

    if (!global.opentype) { this._setDbg('lig: opentype.js no cargado'); return; }

    global.opentype.load(this._fontUrl, function (err, font) {
      if (err) { self._setDbg('lig font err: ' + (err.message || err)); return; }
      self._font = font;
      self._indexGlyphs();
      self._resize();
      self._scheduleRedraw();
    });

    this._onRenderDisposable = term.onRender(function () { self._scheduleRedraw(); });
    this._resizeHandler = function () { self._resize(); self._scheduleRedraw(); };
    window.addEventListener('resize', this._resizeHandler);
  };

  LigaturesWebAddon.prototype._indexGlyphs = function () {
    var by = {};
    var glyphs = this._font.glyphs;
    var n = glyphs.length;
    for (var i = 0; i < n; i++) {
      var g = glyphs.get(i);
      if (g && g.name) by[g.name] = g;
    }
    this._glyphByName = by;
  };

  LigaturesWebAddon.prototype._lookupLig = function (seq) {
    if (seq in this._ligCache) return this._ligCache[seq];
    var name = seqToLigName(seq);
    var glyph = name && this._glyphByName[name] || null;
    this._ligCache[seq] = glyph;
    return glyph;
  };

  LigaturesWebAddon.prototype.dispose = function () {
    if (this._onRenderDisposable) this._onRenderDisposable.dispose();
    if (this._resizeHandler) window.removeEventListener('resize', this._resizeHandler);
    if (this._overlay && this._overlay.parentNode) this._overlay.parentNode.removeChild(this._overlay);
    this._overlay = null; this._ctx = null; this._font = null; this._term = null;
    this._glyphByName = null; this._ligCache = {};
  };

  LigaturesWebAddon.prototype._resize = function () {
    if (!this._term || !this._overlay) return;
    var host = this._host || this._term.element;
    var w = host.clientWidth, h = host.clientHeight;
    var dpr = global.devicePixelRatio || 1;
    this._overlay.width  = Math.max(1, Math.floor(w * dpr));
    this._overlay.height = Math.max(1, Math.floor(h * dpr));
    this._overlay.style.width  = w + 'px';
    this._overlay.style.height = h + 'px';
    this._ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  };

  LigaturesWebAddon.prototype._cellDims = function () {
    var host = this._host || this._term.element;
    return {
      w: host.clientWidth  / Math.max(1, this._term.cols),
      h: host.clientHeight / Math.max(1, this._term.rows)
    };
  };

  LigaturesWebAddon.prototype._scheduleRedraw = function () {
    if (this._scheduled) return;
    this._scheduled = true;
    var self = this;
    global.requestAnimationFrame(function () {
      self._scheduled = false;
      self._redraw();
    });
  };

  LigaturesWebAddon.prototype._baselineOffset = function (cell) {
    // Baseline vertical dentro de la celda usando font metrics (hhea).
    // Ascender/descender vienen en font units → escalar por fontSize/unitsPerEm.
    try {
      var head = this._font.tables.head;
      var hhea = this._font.tables.hhea;
      var upem = head.unitsPerEm || this._font.unitsPerEm || 2048;
      var scale = this._fontSize / upem;
      var ascent  = hhea.ascender  * scale;      // > 0
      var descent = -hhea.descender * scale;     // hhea.descender es negativo
      var glyphH  = ascent + descent;
      // Centrar la línea de texto dentro del cell; baseline = padding + ascent
      var pad = Math.max(0, (cell.h - glyphH) / 2);
      return pad + ascent;
    } catch (_) {
      return cell.h * 0.8;
    }
  };

  LigaturesWebAddon.prototype._redraw = function () {
    if (!this._term || !this._font || !this._ctx || !this._overlay) return;
    var ctx = this._ctx;
    ctx.clearRect(0, 0, this._overlay.width, this._overlay.height);

    var buf = this._term.buffer.active;
    var rows = this._term.rows;
    var cell = this._cellDims();
    var fs = this._fontSize;
    var baselineOffset = this._baselineOffset(cell);
    var found = 0, drawn = 0;

    for (var y = 0; y < rows; y++) {
      var line = buf.getLine(buf.viewportY + y);
      if (!line) continue;
      var text = line.translateToString(true);
      if (!text) continue;

      LIG_RE.lastIndex = 0;
      var m;
      while ((m = LIG_RE.exec(text)) !== null) {
        found++;
        var seq = m[0];
        var glyph = this._lookupLig(seq);
        if (!glyph) continue;   // la font no tiene esta liga

        var col = m.index;
        var px = col * cell.w;
        var py = y * cell.h;
        var w = seq.length * cell.w;

        // Tapar el rect original con bg.
        ctx.fillStyle = this._bg;
        ctx.fillRect(px, py, w, cell.h);

        // Alinear el glyph con el cell: el glyph puede tener LSB negativo
        // (sobresale a la izquierda del origen) y advance width menor al
        // ancho total de las celdas. Centramos el glyph dentro del span
        // usando su advance width real.
        var upem = this._font.unitsPerEm || 2048;
        var glyphScale = fs / upem;
        var advPx = (glyph.advanceWidth || 0) * glyphScale;
        var xShift = (w - advPx) / 2;   // centrar horizontalmente
        var path = glyph.getPath(px + xShift, py + baselineOffset, fs);
        ctx.beginPath();
        var cmds = path.commands || [];
        for (var ci = 0; ci < cmds.length; ci++) {
          var cmd = cmds[ci];
          switch (cmd.type) {
            case 'M': ctx.moveTo(cmd.x, cmd.y); break;
            case 'L': ctx.lineTo(cmd.x, cmd.y); break;
            case 'C': ctx.bezierCurveTo(cmd.x1, cmd.y1, cmd.x2, cmd.y2, cmd.x, cmd.y); break;
            case 'Q': ctx.quadraticCurveTo(cmd.x1, cmd.y1, cmd.x, cmd.y); break;
            case 'Z': ctx.closePath(); break;
          }
        }
        ctx.fillStyle = this._fg;
        ctx.fill();
        drawn++;
      }
    }

    this._setDbg('lig · found=' + found + ' drawn=' + drawn +
                 ' · cell=' + cell.w.toFixed(2) + 'x' + cell.h.toFixed(2));
  };

  if (typeof module !== 'undefined' && module.exports) module.exports = { LigaturesWebAddon: LigaturesWebAddon };
  else global.LigaturesWebAddon = LigaturesWebAddon;

})(typeof self !== 'undefined' ? self : this);
