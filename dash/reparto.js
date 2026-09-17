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

  function esc(v) {
    return String(v == null ? "" : v).replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
    });
  }
  function fmtDur(sec) {
    if (sec == null) return "";
    if (sec < 3600) return Math.round(sec / 60) + " min";
    if (sec < 48 * 3600) return Math.round(sec / 3600) + " h";
    return Math.floor(sec / 86400) + "d " + Math.round((sec % 86400) / 3600) + "h";
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
        after: a
      };
    });
    // Un destino que el plan conoce pero sin fila de cuota: se muestra, sin cifras.
    Object.keys(impact).forEach(function (k) {
      if (out.some(function (p) { return p.k === k; })) return;
      out.push({ k: k, label: impact[k].label || k, used: 0, burn: null,
                 verdict: "sin cuota conocida", resetsIn: null, after: impact[k], unknown: true });
    });
    return out;
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
  }

  async function analyze() {
    S.busy = true; render();
    try {
      var r = await api("/allocation/propose", { overrides: S.overrides });
      S.plan = r.plan; S.phase = "curar"; S.error = "";
    } catch (e) { S.error = e.message; }
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
        S.error = "Algo cambió en tus sesiones desde que analizamos. Vuelve a analizar.";
        S.plan = null; S.phase = "idle";
      } else { S.error = e.message; }
    }
    S.busy = false; render();
  }

  function pollBatch() {
    clearInterval(S.poll);
    S.poll = setInterval(async function () {
      try {
        var b = await api("/allocation/status?batchId=" + encodeURIComponent(S.batch.batchId));
        S.batch = b;
        if (b.state === "terminado") { clearInterval(S.poll); S.poll = null; S.phase = "terminado"; }
        render();
      } catch (e) { clearInterval(S.poll); S.poll = null; S.error = e.message; render(); }
    }, 800);
  }

  async function retry(key) {
    try { await api("/allocation/retry", { batchId: S.batch.batchId, key: key }); pollBatch(); }
    catch (e) { S.error = e.message; render(); }
  }

  async function revert() {
    try {
      var r = await api("/allocation/revert", { batchId: S.batch.batchId });
      S.batch = { batchId: r.batchId, items: [], done: 0, total: r.total };
      S.phase = "aplicando"; pollBatch(); render();
    } catch (e) { S.error = e.message; render(); }
  }

  /* ---------------- pintado ---------------- */

  function tankHtml(p) {
    var a = p.after;
    var usedAfter = a && a.usedAfter != null ? a.usedAfter : p.used;
    var burn = a && a.burnAfter != null ? a.burnAfter : p.burn;
    var sev = burn == null ? "" : burn > 1.15 ? "err" : burn > 0.9 ? "warn" : "";
    var verdict = a && a.verdict ? a.verdict : (p.verdict || "");
    return '<div class="rp-tank rp-drop ' + (usedAfter >= 100 ? "rp-full " : "") +
      (p.unknown ? "rp-unknown" : "") + '" data-pool="' + esc(p.k) + '" title="' +
      esc(p.label + " · " + verdict) + '">' +
      '<div class="rp-cap">' + esc(p.label) +
        '<small data-verdict="' + esc(p.k) + '">' + esc(verdict) +
        (burn == null ? "" : ' · ritmo <span data-burn="' + esc(p.k) + '">' + burn.toFixed(1) + "x</span>") +
        "</small></div>" +
      '<div class="rp-base" style="--n:' + p.used + '%"></div>' +
      '<div class="rp-pre" data-pre="' + esc(p.k) + '" style="--h:' + usedAfter + '%"></div>' +
      '<div class="rp-liq ' + sev + '" data-liq="' + esc(p.k) + '" style="--h:' + usedAfter + '%"></div>' +
      '<div class="rp-foam"></div>' +
      '<div class="rp-lvl rp-num" data-lvl="' + esc(p.k) + '">' + Math.round(usedAfter) + "%</div>" +
      (p.resetsIn == null ? "" : '<div class="rp-reset">reset en ' + esc(fmtDur(p.resetsIn)) + "</div>") +
      "</div>";
  }

  function seg(item, field, options) {
    var current = item.to[field];
    var changed = item.to[field] !== item.from[field];
    return '<span class="rp-seg ' + (changed ? "rp-chg" : "") + '">' + options.map(function (o) {
      return '<button type="button" class="' + (o === current ? "on" : "") + '" data-set="' +
        esc(item.key) + "|" + esc(field) + "|" + esc(o) + '" title="' + esc(o) + '">' +
        esc(field === "model" ? short(o) : o) + "</button>";
    }).join("") + "</span>";
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
    return '<div class="rp-tk ' + (S.overrides[item.key] ? "rp-moved " : "") +
      (item.locked ? "rp-locked " : "") + (S.armed === item.key ? "rp-armed" : "") +
      '" draggable="' + (editable ? "true" : "false") + '" data-key="' + esc(item.key) + '" title="' +
      esc(item.reason || "") + '">' +
      '<span class="rp-dot ' + esc(item.status || "") + '"></span>' +
      '<span class="rp-body"><span class="rp-name">' + esc(item.session) +
        ' <span class="rp-pane">' + esc(item.pane) + "</span></span>" +
        '<span class="rp-diff">' + diff + "</span></span>" +
      (editable ? '<button type="button" class="rp-lk" data-lock="' + esc(item.key) +
        '" title="fijar: el reparto no toca esta sesión">🔒</button>' : "") +
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
      if (lvl) lvl.textContent = Math.round(used) + "%";
      var burn = root.querySelector('[data-burn="' + (window.CSS && CSS.escape ? CSS.escape(k) : k) + '"]');
      if (burn && a.burnAfter != null) burn.textContent = a.burnAfter.toFixed(1) + "x";
      var v = root.querySelector('[data-verdict="' + (window.CSS && CSS.escape ? CSS.escape(k) : k) + '"]');
      if (v && a.verdict) v.firstChild.nodeValue = a.verdict + (a.burnAfter != null ? " · ritmo " : "");
      var tank = liq.closest(".rp-tank");
      if (tank) tank.classList.toggle("rp-full", used >= 100);
    });
  }

  function footHtml() {
    if (S.phase === "idle") {
      return '<div class="rp-foot"><span class="rp-note">Analiza para ver un reparto propuesto de tus sesiones vivas.</span>' +
        '<span class="rp-sp"></span><button type="button" class="rp-go" data-analyze ' +
        (S.busy ? "disabled" : "") + ">" + (S.busy ? "Analizando…" : "Analizar") + "</button></div>";
    }
    if (S.phase === "curar") {
      var n = changes(), tuned = Object.keys(S.overrides).length;
      return '<div class="rp-foot"><span class="rp-note">' + n + " cambios · " +
        (items().length - n) + " sin cambio" +
        (tuned ? ' · <b class="warn">' + tuned + " ajustadas por ti</b>" : "") +
        " · se reinician ahora, conservando la conversación</span>" +
        '<span class="rp-sp"></span>' +
        (tuned ? '<button type="button" class="rp-ghost" data-reset>Propuesta original</button>' : "") +
        '<button type="button" class="rp-go" data-apply ' + (n && !S.busy ? "" : "disabled") + ">" +
        (S.busy ? "Aplicando…" : "Aplicar " + n) + "</button></div>";
    }
    var b = S.batch || { items: [], done: 0, total: 0 };
    var list = (b.items || []).filter(function (i) { return i.state !== "omitida"; });
    var stopped = list.filter(function (i) { return i.state === "detenida"; });
    var ready = list.filter(function (i) { return i.state === "lista"; });
    var pct = b.total ? Math.round((b.done / b.total) * 100) : 0;
    var html = '<div class="rp-progress"><div class="rp-note">' +
      (S.phase === "terminado"
        ? "Listo: " + ready.length + " sesiones reconfiguradas" + (stopped.length ? ", " + stopped.length + " detenidas" : "")
        : "Aplicando " + b.total + " cambios · reinicio con la conversación conservada") +
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
      (stopped.length ? " · " + stopped.length + " detenidas" : "") + "</span><span class=\"rp-sp\"></span>" +
      (S.phase === "terminado"
        ? (ready.length ? '<button type="button" class="rp-ghost" data-revert>Revertir todo</button>' : "") +
          '<button type="button" class="rp-go" data-again>Volver a analizar</button>'
        : "") + "</div>";
    return html;
  }

  function render() {
    var root = document.getElementById("reparto");
    if (!root) return;
    var P = pools();
    var html = '<div class="rp-hint">Cada tanque es una cuota. La capa oscura es lo que ya gastaste; ' +
      "la clara, lo que tus sesiones añadirán cuando se renueve. Arrastra una sesión a otro tanque " +
      "para moverla de cuenta o de motor, o tócala y luego toca el tanque.</div>";
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
        html += '<div class="rp-bin" data-label="sin cuota conocida">' + orphans.map(tileHtml).join("") + "</div>";
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
    var parts = pool.split(":");
    var motor = parts[0], account = parts[1] || "main";
    var harness = item.to.harness || item.from.harness || item.from.motor;
    var gateway = motor !== harness;
    return { to: {
      harness: harness, motor: motor,
      model: item.to.model, effort: item.to.effort,
      harnessAccount: gateway ? account : account,
      motorAccount: gateway ? "main" : account,
      routeId: harness + ":" + motor
    } };
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
    var rs = root.querySelector("[data-reset]");
    if (rs) rs.onclick = function () { S.overrides = {}; analyze(); };

    q("[data-set]").forEach(function (b) {
      b.onclick = function (e) {
        e.stopPropagation();
        var p = b.dataset.set.split("|");
        var item = items().filter(function (i) { return i.key === p[0]; })[0];
        if (!item) return;
        var to = Object.assign({}, item.to);
        to[p[1]] = p[2];
        setOverride(p[0], { to: to });
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
