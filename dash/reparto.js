/* Reparto de cuota — Analytics → Reparto.
 *
 * Cuatro tanques (uno por cuota con fuente), las sesiones vivas como fichas que
 * se arrastran entre ellos, y un panel de progreso al aplicar. Las cifras las
 * calcula siempre el servidor (lib/allocation.py): aquí no se recalcula nada,
 * para que la vista previa y el plan no puedan divergir.
 */
(function () {
  "use strict";

  var S = {
    phase: "idle",      // idle → curar → aplicando → terminado
    plan: null,
    overrides: {},
    batch: null,
    limits: [],
    armed: null,        // ficha "armada" con un clic, para táctil y como respaldo del drag
    drag: null,
    poll: null,
    busy: false,
    error: ""
  };

  /* El tablero define tf(es, en) global segun CC_LANG. Aqui se usa a traves de
     un puente para que este fichero siga cargando suelto (tests, parseo). */
  function T(es, en) { return typeof tf === "function" ? tf(es, en) : es; }

  function esc(v) {
    return String(v == null ? "" : v).replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
    });
  }
  function fmtDur(sec) {
    if (sec == null) return "";
    if (sec < 3600) return Math.round(sec / 60) + " min";
    if (sec < 48 * 3600) return Math.round(sec / 3600) + " h";
    return Math.floor(sec / 86400) + T("d ", "d ") + Math.round((sec % 86400) / 3600) + "h";
  }
  function locale() {
    return typeof L === "string" && L === "en" ? "en" : (typeof L === "string" ? "es" : undefined);
  }
  /* Cuándo se renueva la cuota, en fecha y hora reales: "sáb 20, 19:40".
     El "en 34 h" dice cuánto falta, esto dice cuándo, que es lo que sirve
     para planear el día. */
  function fmtWhen(ts, withYear) {
    if (!ts) return "";
    var o = { weekday: "short", day: "numeric", month: "short",
              hour: "2-digit", minute: "2-digit" };
    if (withYear) o.year = "numeric";
    try { return new Date(ts * 1000).toLocaleString(locale(), o); }
    catch (e) { return new Date(ts * 1000).toLocaleString(); }
  }

  function poolKey(motor, account) { return motor + ":" + (account || "main"); }
  function short(model) {
    return String(model || "").replace("claude-", "").replace("gpt-5.6-", "").replace("gpt-6-", "");
  }

  /* ---------------- datos ---------------- */

  function pools() {
    var impact = (S.plan && S.plan.impact) || {};
    var rows = {};
    (S.limits || []).forEach(function (l) {
      if (l.window !== "7d") return;
      var k = poolKey(l.provider, l.account);
      // Claude publica varias filas semanales; manda la general, no la de un modelo.
      if (!rows[k] || l.kind === "weekly_all") rows[k] = l;
    });
    var out = Object.keys(rows).sort().map(function (k) {
      var l = rows[k], a = impact[k] || null;
      return {
        k: k, label: l.provider + " " + (l.account || "main"),
        used: l.percent == null ? 0 : l.percent,
        burn: l.burn, verdict: l.verdict,
        resetsIn: (l.resets_at || 0) - Date.now() / 1000,
        resetsAt: l.resets_at || null,
        unit: windowName(l), others: others(k, l), after: a
      };
    });
    // Un destino que el plan conoce pero sin fila de cuota: se muestra, sin cifras.
    Object.keys(impact).forEach(function (k) {
      if (out.some(function (p) { return p.k === k; })) return;
      out.push({ k: k, label: impact[k].label || k, used: 0, burn: null,
                 verdict: T("sin cuota conocida", "no known quota"), resetsIn: null, resetsAt: null,
                 unit: "", others: [], after: impact[k], unknown: true });
    });
    return out;
  }

  /* Cada proveedor mide su límite de forma distinta y con nombres internos que
     no dicen nada. Se traducen a lo que son:
       semana  = presupuesto semanal de la cuenta (el que manda el cilindro)
       modelo  = tope semanal de un modelo concreto, aparte del anterior
       5 h     = ventana corta que te frena ahora mismo, se renueva sola
     Claude trae las tres por cuenta; Codex y Grok solo la semanal. */
  function windowName(l) {
    if (l.window === "5h") return T("5 h", "5 h");
    if (l.kind === "weekly_scoped") return T("modelo", "model");
    if (l.window === "7d") return T("semana", "week");
    return l.window || "";
  }

  /* Las mediciones del grupo que NO son la que gobierna el cilindro. Se ven
     todas, no solo la peor, porque estar al 0 % en las 5 h y al 100 % por
     modelo son dos situaciones distintas y ambas cambian la decisión. */
  function others(k, governing) {
    var out = [];
    (S.limits || []).forEach(function (l) {
      if (poolKey(l.provider, l.account) !== k) return;
      if (l === governing || l.percent == null) return;
      out.push({ name: windowName(l), pct: Math.round(l.percent),
                 full: l.percent >= 100, hot: l.percent >= 85 });
    });
    return out.sort(function (a, b) { return b.pct - a.pct; }).slice(0, 2);
  }

  function items() { return (S.plan && S.plan.items) || []; }
  function changes() {
    return items().filter(function (i) { return !i.same && !i.locked; }).length;
  }
  function batchItem(key) {
    if (!S.batch || !S.batch.items) return null;
    for (var i = 0; i < S.batch.items.length; i++) if (S.batch.items[i].key === key) return S.batch.items[i];
    return null;
  }

  /* ---------------- llamadas ---------------- */

  async function load() {
    try {
      var st = await api("/usage/state");
      S.limits = st.limits || [];
      S.error = "";
    } catch (e) { S.error = e.message; }
    render();
    // Volver a la pestaña con un lote a medias reengancha el seguimiento.
    if (S.phase === "aplicando" && S.batch && !S.poll) pollBatch();
  }

  var seq = 0;
  async function analyze() {
    var mine = ++seq;
    S.busy = true; render();
    try {
      var r = await api("/allocation/propose", { overrides: S.overrides });
      if (mine !== seq) return;            // llegó tarde: ya hay un análisis más nuevo
      S.plan = r.plan; S.phase = "curar"; S.error = "";
    } catch (e) {
      if (mine !== seq) return;
      S.error = e.message;
    }
    if (mine !== seq) return;
    S.busy = false; render();
  }

  async function setOverride(key, target) {
    S.overrides[key] = target;
    await analyze();
  }

  async function preview(key, target) {
    if (!S.plan || !key) return;
    try {
      var r = await api("/allocation/preview", { planId: S.plan.planId, key: key, target: target });
      paintImpact(r.impact);
    } catch (e) { /* la vista previa nunca interrumpe el arrastre */ }
  }

  async function apply() {
    S.busy = true; render();
    try {
      var r = await api("/allocation/apply", { planId: S.plan.planId });
      S.batch = { batchId: r.batchId, items: [], done: 0, total: r.total };
      S.phase = "aplicando"; S.error = "";
      pollBatch();
    } catch (e) {
      if (/plan_stale/.test(e.message)) {
        S.error = T("Algo cambió en tus sesiones desde que analizamos. Vuelve a analizar.",
             "Something changed in your sessions since we analysed. Analyse again.");
        S.plan = null; S.phase = "idle";
      } else { S.error = e.message; }
    }
    S.busy = false; render();
  }

  function visible() {
    var el = document.getElementById("reparto");
    return !!(el && el.offsetParent !== null);
  }

  function pollBatch() {
    clearInterval(S.poll);
    S.poll = setInterval(async function () {
      // Si el panel dejó de verse (modal cerrado, otra pestaña), se suelta el
      // intervalo. El lote sigue en el servidor y load() lo retoma al volver.
      if (!visible()) { clearInterval(S.poll); S.poll = null; return; }
      try {
        var b = await api("/allocation/status?batchId=" + encodeURIComponent(S.batch.batchId));
        S.batch = b;
        if (b.state === "terminado") { clearInterval(S.poll); S.poll = null; S.phase = "terminado"; }
        render();
      } catch (e) {
        clearInterval(S.poll); S.poll = null;
        S.error = e.message + " — el lote sigue en marcha en el servidor.";
        render();
      }
    }, 800);
  }

  async function retry(key) {
    if (S.busy || !S.batch) return;
    S.busy = true; S.error = ""; render();
    try {
      await api("/allocation/retry", { batchId: S.batch.batchId, key: key });
      S.phase = "aplicando"; pollBatch();
    } catch (e) { S.error = e.message; }
    S.busy = false; render();
  }

  async function revert() {
    if (S.busy || !S.batch) return;
    S.busy = true; S.error = ""; render();
    try {
      var r = await api("/allocation/revert", { batchId: S.batch.batchId });
      S.batch = { batchId: r.batchId, items: [], done: 0, total: r.total };
      S.phase = "aplicando"; pollBatch();
    } catch (e) { S.error = e.message; }
    S.busy = false; render();
  }

  /* ---------------- pintado ---------------- */

  /* Por que propone lo que propone. Va en la pestana y no solo en la
     documentacion: si no se puede auditar el criterio, la propuesta es un
     oraculo y no una herramienta. */
  function metodoHtml() {
    var pasos = [
      [T("1. Le pone una capa a cada sesión", "1. It assigns a layer to each session"),
       T("Por el effort que tiene hoy: alto, extra alto o máximo son capa 3; bajo es capa 1; el resto capa 2. " +
         "Si la sesión ya terminó o está en reposo, baja una capa. La capa decide el effort propuesto " +
         "(3 alto, 2 medio, 1 bajo) y el modelo, tomando el de ese nivel en model-tiers. " +
         "La capa 3 nunca cambia de motor, solo de cuenta.",
         "By its current effort: high, xhigh or max are layer 3; low is layer 1; the rest layer 2. " +
         "A finished or idle session drops one layer. The layer sets the proposed effort " +
         "(3 high, 2 medium, 1 low) and the model, taking that tier from model-tiers. " +
         "Layer 3 never changes engine, only account.")],
      [T("2. Calcula el ritmo que dejaría cada destino", "2. It computes the pace each destination would leave"),
       T("Cada sesión pesa según su effort: bajo 0.5, medio 1, alto 1.6, extra alto 2.2, máximo 3. " +
         "Mueve ese peso de su cuota actual a la candidata y proyecta el ritmo resultante. " +
         "Ritmo 1.0 es ir justo para que te dure hasta el reset; por encima, te pasas antes.",
         "Each session weighs by effort: low 0.5, medium 1, high 1.6, xhigh 2.2, max 3. " +
         "It moves that weight from its current quota to the candidate and projects the resulting pace. " +
         "Pace 1.0 is exactly on track to reach reset; above that, you run out early.")],
      [T("3. Ordena los candidatos y toma el primero", "3. It ranks the candidates and takes the first"),
       T("Cuota conocida antes que desconocida · ritmo de 1.0 o menos, o sea que te dure hasta el reset · " +
         "el ritmo más bajo · quedarse donde está · no cambiar de motor · el reset más lejano. " +
         "Si ninguna cuota te dura hasta el reset con esa carga, lo dice en la ficha y elige la que menos se pasa.",
         "Known quota before unknown · pace of 1.0 or less, meaning it reaches reset · " +
         "lowest pace · staying put · not switching engine · furthest reset. " +
         "If no quota reaches reset under your load, the tile says so and it picks the one that overshoots least.")]
    ];
    return '<details class="rp-how"><summary>' +
      esc(T("¿Cómo se calcula esta propuesta?", "How is this proposal calculated?")) + "</summary>" +
      pasos.map(function (p) {
        return "<p><b>" + esc(p[0]) + "</b> " + esc(p[1]) + "</p>";
      }).join("") +
      "<p class=\"rp-how-foot\">" +
      esc(T("Es determinista: con los mismos datos sale siempre la misma propuesta. " +
            "Lo que fijes con el candado o cambies a mano manda sobre el cálculo.",
            "It is deterministic: the same data always yields the same proposal. " +
            "What you pin or change by hand overrides the calculation.")) +
      "</p></details>";
  }

  function tankHtml(p) {
    var a = p.after;
    var usedAfter = a && a.usedAfter != null ? a.usedAfter : p.used;
    var burn = a && a.burnAfter != null ? a.burnAfter : p.burn;
    var sev = burn == null ? "" : burn > 1.15 ? "err" : burn > 0.9 ? "warn" : "";
    var verdict = a && a.verdict ? a.verdict : (p.verdict || "");
    return '<div class="rp-tank rp-drop ' + (usedAfter >= 100 ? "rp-full " : "") +
      (p.unknown ? "rp-unknown" : "") + '" data-pool="' + esc(p.k) + '" title="' +
      esc(p.label + " · " + verdict +
          (p.resetsAt ? "\n" + T("se renueva el ", "renews on ") + fmtWhen(p.resetsAt, true) : "")) +
      '">' +
      // Dos filas: etiqueta y distintivo arriba, veredicto debajo. Antes el
      // distintivo flotaba y partia el veredicto por la mitad.
      '<div class="rp-cap"><span class="rp-top"><b>' + esc(p.label) + "</b>" +
        (p.others || []).map(function (o) {
          return '<em class="rp-tight ' + (o.full ? "full" : o.hot ? "hot" : "") + '">' +
            esc(o.name + " " + o.pct + "%") + "</em>";
        }).join("") + "</span>" +
        '<small data-verdict="' + esc(p.k) + '">' + esc(verdict) +
        (burn == null ? "" : T(" · ritmo ", " · pace ") + '<span data-burn="' + esc(p.k) + '">' +
          burn.toFixed(1) + "x</span>") +
        "</small></div>" +
      '<div class="rp-base" style="--n:' + p.used + '%"></div>' +
      '<div class="rp-pre" data-pre="' + esc(p.k) + '" style="--h:' + usedAfter + '%"></div>' +
      '<div class="rp-liq ' + sev + '" data-liq="' + esc(p.k) + '" style="--h:' + usedAfter + '%"></div>' +
      '<div class="rp-foam"></div>' +
      '<div class="rp-lvl rp-num"><b>' + Math.round(p.used) + "%</b>" +
        '<span class="rp-lvl-lbl">' + esc(p.unit || T("usado", "used")) + "</span>" +
        '<span class="rp-proj" data-lvl="' + esc(p.k) + '">' +
          (usedAfter != null && Math.abs(usedAfter - p.used) >= 1
            ? "\u2192 " + Math.round(usedAfter) + "%" : "") + "</span></div>" +
      (p.resetsIn == null ? "" : '<div class="rp-reset">' + T("reset en ", "resets in ") +
        esc(fmtDur(p.resetsIn)) +
        (p.resetsAt ? '<em>' + esc(fmtWhen(p.resetsAt)) + "</em>" : "") + "</div>") +
      "</div>";
  }

  function seg(item, field, options) {
    var current = item.to[field];
    var changed = item.to[field] !== item.from[field];
    return '<span class="rp-seg ' + (changed ? "rp-chg" : "") + '">' + options.map(function (o) {
      // Tres atributos y no uno compuesto: item.key ya contiene "|" (session|pane),
      // así que cualquier delimitador colisiona y el chip se vuelve un no-op.
      return '<button type="button" class="' + (o === current ? "on" : "") + '" data-set-key="' +
        esc(item.key) + '" data-set-field="' + esc(field) + '" data-set-value="' + esc(o) +
        '" title="' + esc(o) + '">' + esc(field === "model" ? short(o) : o) + "</button>";
    }).join("") + "</span>";
  }

  /* El nombre de tmux (term-3692-1) no identifica nada. Se usa la misma etiqueta
     que el resto del tablero, y la carpeta como respaldo antes que el nombre crudo. */
  function nameOf(item) {
    if (item.project) return item.project;
    var cwd = String(item.cwd || "").replace(/\/+$/, "");
    if (cwd) {
      var parts = cwd.split("/");
      return parts[parts.length - 1] || cwd;
    }
    return item.session;
  }

  /* El tablero trae los iconos como SVG en linea. El emoji se pinta con su propio
     color y no respeta `color`, asi que fijada y libre se veian identicas. */
  function lockIcon() {
    if (typeof svg === "function") { var m = svg("lock", 13); if (m) return m; }
    return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"' +
      ' stroke-linecap="round"><rect x="4" y="11" width="16" height="10" rx="2"/>' +
      '<path d="M8 11V7a4 4 0 0 1 8 0v4"/></svg>';
  }

  var STATUS = { working: ["trabajando", "working"], waiting: ["te espera", "waiting on you"],
                 done: ["terminó", "finished"], idle: ["en reposo", "idle"] };

  /* Sin esto, una sesión que acabó hace 17 h y otra que trabaja ahora se ven
     idénticas, y dos fichas del mismo proyecto parecen un duplicado muerto.
     Se dice el estado con palabras y, si lleva más de una hora quieta, cuánto. */
  function stateHtml(item) {
    var st = String(item.status || "");
    var name = STATUS[st] ? T(STATUS[st][0], STATUS[st][1]) : st;
    var age = item.seenAt ? Date.now() / 1000 - item.seenAt : null;
    var old = age != null && age >= 3600;
    if (!name && !old) return "";
    return '<span class="rp-state ' + esc(st) + (old ? " stale" : "") + '">' + esc(name) +
      (old ? " · " + esc(fmtDur(age)) : "") + "</span>";
  }

  function tileHtml(item) {
    var opts = (S.plan.options || {})[item.to.motor] || {};
    var models = opts.models && opts.models.length ? opts.models : [item.to.model];
    var efforts = opts.efforts && opts.efforts.length ? opts.efforts : ["low", "medium", "high"];
    var b = batchItem(item.key);
    var editable = S.phase === "curar" && !item.locked;
    var after = editable
      ? seg(item, "model", models) + " " + seg(item, "effort", efforts)
      : '<b>' + esc(short(item.to.model)) + " · " + esc(item.to.effort) + "</b>";
    var diff = item.locked
      ? '<span class="rp-old" style="text-decoration:none">' + esc(item.reason) + "</span>"
      : '<span class="rp-old">' + esc(short(item.from.model) + " · " + item.from.effort) + "</span>" +
        '<span class="rp-arrow">→</span>' + after;
    var quiet = item.seenAt && (Date.now() / 1000 - item.seenAt) >= 3 * 3600;
    return '<div class="rp-tk ' + (quiet ? "rp-quiet " : "") + (S.overrides[item.key] ? "rp-moved " : "") +
      (item.locked ? "rp-locked " : "") + (S.armed === item.key ? "rp-armed" : "") +
      '" draggable="' + (editable ? "true" : "false") + '" data-key="' + esc(item.key) + '" title="' +
      esc((item.cwd ? item.cwd + " · " : "") + (item.session || "") + " " + (item.pane || "") +
          (item.reason ? "\n" + item.reason : "")) + '">' +
      '<span class="rp-dot ' + esc(item.status || "") + '"></span>' +
      '<span class="rp-body"><span class="rp-name">' + esc(nameOf(item)) +
        (String(item.project || "").indexOf("\u2AFD") >= 0 ? ""
          : ' <span class="rp-pane">' + esc(item.pane) + "</span>") + "</span>" +
        stateHtml(item) +
        '<span class="rp-diff">' + diff + "</span></span>" +
      (S.phase === "curar"
        ? '<button type="button" class="rp-lk ' + (item.locked ? "on" : "") + '" data-lock="' +
          esc(item.key) + '" title="' + (item.locked ? T("soltar: dejar que el reparto la mueva", "unpin: let the split move it")
            : T("fijar: el reparto no toca esta sesión", "pin: leave this session alone")) + '">' + lockIcon() + "</button>"
        : "") +
      (b && b.state !== "omitida" ? '<span class="rp-st ' + esc(b.state) + '">' + esc(b.state) + "</span>" : "") +
      "</div>";
  }

  function paintImpact(impact) {
    var root = document.getElementById("reparto");
    if (!root || !impact) return;
    Object.keys(impact).forEach(function (k) {
      var a = impact[k];
      var liq = root.querySelector('[data-liq="' + (window.CSS && CSS.escape ? CSS.escape(k) : k) + '"]');
      if (!liq) return;
      var used = a.usedAfter == null ? 0 : a.usedAfter;
      var sev = a.burnAfter == null ? "" : a.burnAfter > 1.15 ? "err" : a.burnAfter > 0.9 ? "warn" : "";
      liq.style.setProperty("--h", used + "%");
      liq.className = "rp-liq " + sev;
      var pre = root.querySelector('[data-pre="' + (window.CSS && CSS.escape ? CSS.escape(k) : k) + '"]');
      if (pre) pre.style.setProperty("--h", used + "%");
      var lvl = root.querySelector('[data-lvl="' + (window.CSS && CSS.escape ? CSS.escape(k) : k) + '"]');
      if (lvl) {
        var base = lvl.closest(".rp-tank");
        var b = base && base.querySelector(".rp-lvl b");
        var nowPct = b ? parseFloat(b.textContent) : null;
        lvl.textContent = (nowPct == null || Math.abs(used - nowPct) >= 1)
          ? "\u2192 " + Math.round(used) + "%" : "";
      }
      var burn = root.querySelector('[data-burn="' + (window.CSS && CSS.escape ? CSS.escape(k) : k) + '"]');
      if (burn && a.burnAfter != null) burn.textContent = a.burnAfter.toFixed(1) + "x";
      var v = root.querySelector('[data-verdict="' + (window.CSS && CSS.escape ? CSS.escape(k) : k) + '"]');
      if (v && a.verdict && v.firstChild && v.firstChild.nodeType === 3) {
        v.firstChild.nodeValue = a.verdict + (a.burnAfter != null ? T(" · ritmo ", " · pace ") : "");
      }
      var tank = liq.closest(".rp-tank");
      if (tank) tank.classList.toggle("rp-full", used >= 100);
    });
  }

  function footHtml() {
    if (S.phase === "idle") {
      return '<div class="rp-foot"><span class="rp-note">' +
        T("Analiza para ver un reparto propuesto de tus sesiones vivas.",
          "Analyse to see a proposed split of your live sessions.") + '</span>' +
        '<span class="rp-sp"></span><button type="button" class="rp-go" data-analyze ' +
        (S.busy ? "disabled" : "") + ">" + (S.busy ? T("Analizando…", "Analysing…") : T("Analizar", "Analyse")) + "</button></div>";
    }
    if (S.phase === "curar") {
      var n = changes(), tuned = Object.keys(S.overrides).length;
      return '<div class="rp-foot"><span class="rp-note">' + n + T(" cambios · ", " changes · ") +
        (items().length - n) + T(" sin cambio", " unchanged") +
        (tuned ? ' · <b class="warn">' + tuned + T(" ajustadas por ti</b>", " tuned by you</b>") : "") +
        T(" · se reinician ahora, conservando la conversación</span>",
             " · they restart now, keeping the conversation</span>") +
        '<span class="rp-sp"></span>' +
        (tuned ? '<button type="button" class="rp-ghost" data-reset>' + T("Propuesta original", "Original proposal") + '</button>' : "") +
        '<button type="button" class="rp-go" data-apply ' + (n && !S.busy ? "" : "disabled") + ">" +
        (S.busy ? T("Aplicando…", "Applying…") : T("Aplicar ", "Apply ") + n) + "</button></div>";
    }
    var b = S.batch || { items: [], done: 0, total: 0 };
    var list = (b.items || []).filter(function (i) { return i.state !== "omitida"; });
    var stopped = list.filter(function (i) { return i.state === "detenida"; });
    var ready = list.filter(function (i) { return i.state === "lista"; });
    var pct = b.total ? Math.round((b.done / b.total) * 100) : 0;
    var html = '<div class="rp-progress"><div class="rp-note">' +
      (S.phase === "terminado"
        ? T("Listo: ", "Done: ") + ready.length + T(" sesiones reconfiguradas", " sessions reconfigured") + (stopped.length ? ", " + stopped.length + T(" detenidas", " stopped") : "")
        : T("Aplicando ", "Applying ") + b.total +
          T(" cambios · reinicio con la conversación conservada",
            " changes · restart with the conversation kept")) +
      '</div><div class="rp-pb"><i style="--p:' + pct + '%"></i></div><div class="rp-steps">' +
      list.map(function (i) {
        return '<span class="' + esc(i.state) + '">' +
          (i.state === "aplicando" ? '<span class="rp-spin"></span>' : "") +
          esc(i.session) + " " + esc(i.pane) +
          (i.state === "detenida"
            ? " · " + esc(i.error || "") + ' <button type="button" data-retry="' + esc(i.key) + '">reintentar</button>'
            : "") + "</span>";
      }).join("") + "</div></div>";
    if (S.phase === "terminado" && !stopped.length) {
      html += '<div class="rp-done">✓ ' + ready.length + " sesiones reconfiguradas</div>";
    }
    html += '<div class="rp-foot"><span class="rp-note">' + b.done + "/" + b.total +
      (stopped.length ? " · " + stopped.length + T(" detenidas", " stopped") : "") + "</span><span class=\"rp-sp\"></span>" +
      // En "aplicando" siempre hay salida: si el sondeo se cortó, reanudarlo; y
      // volver a empezar nunca depende de que el lote haya terminado.
      (S.phase === "aplicando" && !S.poll
        ? '<button type="button" class="rp-ghost" data-repoll>' + T("Reanudar seguimiento", "Resume tracking") + '</button>' : "") +
      (S.phase === "terminado" && ready.length
        ? '<button type="button" class="rp-ghost" data-revert ' + (S.busy ? "disabled" : "") + ">" + T("Revertir todo", "Revert all") + "</button>" : "") +
      '<button type="button" class="rp-go" data-again>' + T("Volver a analizar", "Analyse again") + '</button></div>';
    return html;
  }

  function render() {
    var root = document.getElementById("reparto");
    if (!root) return;
    var P = pools();
    // Leyenda de las mediciones: cada proveedor mide distinto y sin nombrarlas el
    // cilindro es un número suelto. Solo se listan las que existen ahora mismo.
    var units = {};
    P.forEach(function (p) {
      if (p.unit) units[p.unit] = 1;
      (p.others || []).forEach(function (o) { units[o.name] = 1; });
    });
    var GLOSS = {};
    GLOSS[T("semana", "week")] = T("presupuesto semanal de la cuenta",
                                   "the account's weekly budget");
    GLOSS[T("modelo", "model")] = T("tope semanal de un modelo concreto, aparte del anterior",
                                    "weekly cap of one model, separate from the above");
    GLOSS[T("5 h", "5 h")] = T("ventana corta que te frena ahora; se renueva sola",
                               "short window that stops you now; renews on its own");
    var legend = Object.keys(units).filter(function (u) { return GLOSS[u]; }).map(function (u) {
      return '<span><b>' + esc(u) + "</b> " + esc(GLOSS[u]) + "</span>";
    }).join("");

    var html = '<div class="rp-hint">' +
      T("Cada cilindro es una cuenta. El número grande es su ",
        "Each cylinder is one account. The big number is its ") +
      "<b>" + esc(T("semana", "week")) + "</b>" +
      T("; la capa oscura es lo gastado y la clara, hasta dónde llegaría al reset con la carga propuesta. ",
        "; the dark layer is what you spent and the light one how far it would go by reset under the proposed load. ") +
      T("Arrastra una sesión a otro cilindro para moverla de cuenta o de motor, o tócala y luego tócalo.",
        "Drag a session onto another cylinder to move it between accounts or engines, or tap it then tap the cylinder.") +
      "</div>" +
      (legend ? '<div class="rp-gloss">' + legend + "</div>" : "") +
      metodoHtml();
    if (S.error) html += '<div class="rp-hint" style="color:var(--err)">' + esc(S.error) + "</div>";
    html += '<div class="rp-tanks">' + P.map(tankHtml).join("") + "</div>";

    if (S.phase !== "idle") {
      html += '<div class="rp-bins">' + P.map(function (p) {
        var mine = items().filter(function (i) {
          return poolKey(i.to.motor, i.to.motorAccount) === p.k;
        });
        return '<div class="rp-bin rp-drop" data-pool="' + esc(p.k) + '" data-label="' + esc(p.label) + '">' +
          mine.map(tileHtml).join("") + "</div>";
      }).join("") + "</div>";
      var orphans = items().filter(function (i) {
        return !P.some(function (p) { return p.k === poolKey(i.to.motor, i.to.motorAccount); });
      });
      if (orphans.length) {
        html += '<div class="rp-bin" data-label="' + esc(T("sin cuota conocida", "no known quota")) +
          '">' + orphans.map(tileHtml).join("") + "</div>";
      }
      html += '<div class="rp-dropbar">' + P.map(function (p) {
        return '<div class="rp-dz rp-drop" data-pool="' + esc(p.k) + '"><b>' + esc(p.label) + "</b><span>" +
          esc((p.after && p.after.verdict) || p.verdict || "") + "</span></div>";
      }).join("") + "</div>";
    }
    html += footHtml();
    root.innerHTML = html;
    root.classList.toggle("rp-dragging", !!S.armed);
    wire(root);
  }

  /* ---------------- interacción ---------------- */

  function targetFor(pool, key) {
    var item = items().filter(function (i) { return i.key === key; })[0];
    if (!item) return null;
    var cut = pool.indexOf(":");
    var motor = pool.slice(0, cut), account = pool.slice(cut + 1) || "main";
    var harness = item.to.harness || item.from.harness || item.from.motor;
    if (motor === harness) {
      // Ruta nativa: la cuenta del tanque es la del harness y la del motor a la vez.
      return { to: { harness: harness, motor: motor, model: item.to.model, effort: item.to.effort,
                     harnessAccount: account, motorAccount: account,
                     routeId: harness + ":" + motor } };
    }
    // Ruta pasarela: el motor gasta siempre con "main", así que un tanque de otra
    // cuenta no es un destino representable; se ignora en vez de mandar un destino
    // que el servidor rechazaría. La cuenta del harness no sale nunca del tanque:
    // es del otro lado de la ruta y se conserva tal cual.
    if (account !== "main") return null;
    return { to: { harness: harness, motor: motor, model: item.to.model, effort: item.to.effort,
                   harnessAccount: item.to.harnessAccount || item.from.account || "main",
                   motorAccount: "main", routeId: harness + ":" + motor } };
  }

  function wire(root) {
    var q = function (sel) { return Array.prototype.slice.call(root.querySelectorAll(sel)); };

    var go = root.querySelector("[data-analyze]"); if (go) go.onclick = analyze;
    var ap = root.querySelector("[data-apply]"); if (ap) ap.onclick = apply;
    var rv = root.querySelector("[data-revert]"); if (rv) rv.onclick = revert;
    var ag = root.querySelector("[data-again]");
    if (ag) ag.onclick = function () {
      clearInterval(S.poll); S.poll = null;
      S.plan = null; S.batch = null; S.overrides = {}; S.armed = null; S.phase = "idle";
      load();
    };
    var rp = root.querySelector("[data-repoll]");
    if (rp) rp.onclick = function () { S.error = ""; pollBatch(); render(); };
    var rs = root.querySelector("[data-reset]");
    if (rs) rs.onclick = function () { S.overrides = {}; analyze(); };

    q("[data-set-key]").forEach(function (b) {
      b.onclick = function (e) {
        e.stopPropagation();
        if (S.busy) return;
        var key = b.dataset.setKey;
        var item = items().filter(function (i) { return i.key === key; })[0];
        if (!item) return;
        var to = Object.assign({}, item.to);
        to[b.dataset.setField] = b.dataset.setValue;
        setOverride(key, { to: to });
      };
    });
    q("[data-lock]").forEach(function (b) {
      b.onclick = function (e) {
        e.stopPropagation();
        var key = b.dataset.lock;
        if (S.overrides[key] && S.overrides[key].locked) { delete S.overrides[key]; analyze(); }
        else setOverride(key, { locked: true });
      };
    });
    q("[data-retry]").forEach(function (b) {
      b.onclick = function () { retry(b.dataset.retry); };
    });

    q('.rp-tk[draggable="true"]').forEach(function (el) {
      el.addEventListener("dragstart", function (e) {
        S.drag = el.dataset.key;
        el.classList.add("rp-lift");
        root.classList.add("rp-dragging");
        try { e.dataTransfer.setData("text/plain", S.drag); e.dataTransfer.effectAllowed = "move"; } catch (_) {}
      });
      el.addEventListener("dragend", function () {
        el.classList.remove("rp-lift");
        S.drag = null;
        root.classList.remove("rp-dragging");
        if (S.plan) paintImpact(S.plan.impact);
        q(".rp-drop.rp-over").forEach(function (z) { z.classList.remove("rp-over"); });
      });
      el.addEventListener("click", function (e) {
        if (e.target.closest("button")) return;
        S.armed = S.armed === el.dataset.key ? null : el.dataset.key;
        render();
      });
    });

    q(".rp-drop").forEach(function (z) {
      z.addEventListener("dragover", function (e) {
        e.preventDefault();
        if (z.classList.contains("rp-over")) return;
        q(".rp-drop.rp-over").forEach(function (o) { o.classList.remove("rp-over"); });
        z.classList.add("rp-over");
        var t = targetFor(z.dataset.pool, S.drag);
        if (t) preview(S.drag, t);
      });
      z.addEventListener("dragleave", function (e) {
        if (!z.contains(e.relatedTarget)) z.classList.remove("rp-over");
      });
      z.addEventListener("drop", function (e) {
        e.preventDefault();
        var key = S.drag || (e.dataTransfer && e.dataTransfer.getData("text/plain"));
        var t = key && targetFor(z.dataset.pool, key);
        if (t) setOverride(key, t);
      });
      z.addEventListener("click", function (e) {
        if (!S.armed || e.target.closest(".rp-tk")) return;
        var key = S.armed;
        S.armed = null;
        var t = targetFor(z.dataset.pool, key);
        if (t) setOverride(key, t);
      });
    });
  }

  window.Reparto = { load: load, render: render, state: S };
})();
