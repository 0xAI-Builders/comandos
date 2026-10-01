// Lector de Resúmenes: la edición del día (lo oficial y lo más caliente en IA)
// con la terminal opcional a la izquierda y la línea del día arriba. Tocar una
// noticia abre un panel a la derecha: Resumen IA · Fuentes (capturadas
// completas, con imágenes y traducción guardada) · Chat · Notas.
//
// Leer nunca genera una edición: /news/editions y /news/edition solo leen.
// Chat y traducción sí llaman a un agente, y solo cuando Jesús lo pide.
// Remoto: vive en el área de terminales. Escritorio: la app GTK lo abre junto
// a su VTE con la misma página.
//
// Markdown (resúmenes y chat): markdown-it (HTML crudo desactivado, enlaces solo
// http/https, sin imágenes remotas) + DOMPurify. Las fuentes capturadas se
// pintan desde bloques de texto escapado; sus imágenes son archivos locales que
// se piden con el token y se muestran como blob:, nunca desde el sitio original.
(function (root, factory) {
  if (typeof module === "object" && module.exports) module.exports = factory();
  else root.NewsReader = factory();
})(typeof self !== "undefined" ? self : this, function () {
  "use strict";

  const CATEGORY_LABEL = { modelo: "Modelos", mcp: "MCP", skill: "Skill", bounty: "Bounty",
    hackathon: "Hackathon", ia: "IA", oficial: "Oficial", hot: "Hot" };
  const STATUS_LABEL = { published: "Listo", partial: "Parcial", empty: "Sin novedades",
    failed: "Falló", not_published: "No se generó", running: "Generando…", scheduled: "Programado" };
  const TERMINAL_STATUS = new Set(["published", "partial", "empty", "failed", "not_published"]);
  const TABS = [["resumen", "Resumen IA"], ["fuentes", "Fuentes"], ["chat", "Chat"], ["notas", "Notas"]];
  const HTTP_URL = /^https?:\/\//i;
  const MEDIA_NAME = /^[0-9a-f]{32}\.(png|jpg|webp|gif|avif)$/;
  const PURIFY_CONFIG = {
    ALLOWED_TAGS: ["p", "br", "strong", "em", "del", "s", "a", "ul", "ol", "li", "blockquote", "code",
      "pre", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "table", "thead", "tbody", "tr", "th", "td",
      "div", "span"],
    ALLOWED_ATTR: ["href", "target", "rel", "class", "title", "start"],
    ALLOWED_URI_REGEXP: /^https?:\/\//i,
    ALLOW_DATA_ATTR: false,
  };

  function esc(value) {
    return String(value == null ? "" : value).replace(/[&<>"']/g, c =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
  }

  function hostOf(url) {
    try { return new URL(url).hostname.replace(/^www\./, "").toLowerCase(); } catch (e) { return ""; }
  }

  function safeHref(url) {
    try {
      const u = new URL(String(url || ""));
      if (!/^https?:$/.test(u.protocol) || u.username || u.password) return null;
      return u.href;
    } catch (e) { return null; }
  }

  // A link whose text looks like an address must point to that address.
  function misleading(text, href) {
    const t = String(text || "").trim();
    const m = t.match(/^(?:https?:\/\/)?((?:[a-z0-9-]+\.)+[a-z]{2,})(?:[/:?#]\S*)?$/i);
    if (!m) return false;
    return m[1].replace(/^www\./, "").toLowerCase() !== hostOf(href);
  }

  function createRenderer(markdownit, purify) {
    if (typeof markdownit !== "function") throw new Error("markdown-it no está disponible");
    const md = markdownit({ html: false, linkify: true, typographer: false, breaks: false });
    md.disable(["image"]);
    md.linkify.set({ fuzzyEmail: false });
    md.validateLink = url => HTTP_URL.test(String(url || "").trim());
    const renderToken = (tokens, idx, options, env, self) => self.renderToken(tokens, idx, options);
    md.renderer.rules.link_open = (tokens, idx, options, env, self) => {
      tokens[idx].attrSet("target", "_blank");
      tokens[idx].attrSet("rel", "noopener noreferrer nofollow");
      return renderToken(tokens, idx, options, env, self);
    };
    md.renderer.rules.link_close = (tokens, idx, options, env, self) => {
      let open = idx - 1, text = "";
      while (open >= 0 && tokens[open].type !== "link_open") {
        if (tokens[open].type === "text" || tokens[open].type === "code_inline") text = tokens[open].content + text;
        open--;
      }
      const href = open >= 0 ? tokens[open].attrGet("href") : "";
      const tail = open >= 0 && misleading(text, href) ? ` <span class="nr-host">(${esc(hostOf(href))})</span>` : "";
      return "</a>" + tail;
    };
    md.renderer.rules.table_open = () => '<div class="nr-table-wrap"><table>';
    md.renderer.rules.table_close = () => "</table></div>";
    return function render(source) {
      const text = String(source || "");
      if (!purify || typeof purify.sanitize !== "function")
        return `<p class="nr-plain">${esc(text).replace(/\n/g, "<br>")}</p>`;
      return String(purify.sanitize(md.render(text), PURIFY_CONFIG));
    };
  }

  function formatDate(ms, tz) {
    if (!ms) return "";
    try {
      return new Intl.DateTimeFormat("es-MX", { timeZone: tz || "America/Mexico_City", day: "numeric",
        month: "short", hour: "2-digit", minute: "2-digit" }).format(new Date(ms));
    } catch (e) { return new Date(ms).toISOString(); }
  }

  function sourceDateLabel(source, tz) {
    if (source && source.publishedAt) return `publicada ${formatDate(source.publishedAt, tz)}`;
    return `descubierta ${formatDate(source && source.discoveredAt, tz)} · publicación desconocida`;
  }

  function statusLabel(status) { return STATUS_LABEL[status] || String(status || ""); }

  // "claude:claude-opus-5-5" / "opencode:opencode/longcat-2.5-preview-free" → nombre corto.
  function modelName(label) { return String(label || "").split(":").slice(1).join(":").split("/").pop() || String(label || ""); }

  /** Procedencia: qué IA escribió el resumen, cuánto costó y cuánto tardó. */
  function provenance(edition) {
    const e = edition || {};
    const models = Object.entries(e.models || {}).map(([m, n]) => `${modelName(m)} (${n})`).join(" · ")
      || (e.model ? String(e.model).split("/").pop() : "");
    const job = e.job || {};
    const mins = job.startedAt && job.finishedAt ? Math.max(1, Math.round((job.finishedAt - job.startedAt) / 60000)) : null;
    const total = Number(e.sourceCount) || 0, failed = Number(e.failedSourceCount) || 0;
    return { models, cost: "$" + (Number(e.costUsd) || 0).toFixed(2), duration: mins ? `${mins} min` : "",
      sources: total ? `${total - failed} de ${total} fuentes` : "", problems: (e.notes || []).length };
  }

  /** Línea del día: los resúmenes de ese día (en orden) y el siguiente programado. */
  function dayLine(list, today) {
    const eds = ((list && list.editions) || []).filter(e => e.localDate === today)
      .sort((a, b) => String(a.slot).localeCompare(String(b.slot)));
    const next = list && list.next;
    const cards = eds.map(e => ({ ...e, day: "Hoy" }));
    if (next && !cards.some(c => c.id === next.id)) cards.push({ ...next, day: next.localDate === today ? "Hoy" : "Mañana" });
    return cards;
  }

  /** Kicker de una noticia: de dónde viene. Las ediciones viejas conservan su categoría. */
  function storyKicker(story) {
    const s = story || {}, meta = s.meta || {};
    if (s.category === "oficial") return { label: `Oficial · ${meta.lab || "anuncio"}`, hot: false };
    if (s.category === "hot") return { label: meta.lab && meta.lab !== "Comunidad" ? `Hot · ${meta.lab}` : "Hot · comunidad", hot: true };
    return { label: CATEGORY_LABEL[s.category] || String(s.category || "IA"), hot: s.category === "bounty" || s.category === "hackathon" };
  }

  /** Texto en línea de una fuente capturada: escapado, con `código`. */
  function inlineText(text) {
    return esc(text).replace(/`([^`\n]{1,300})`/g, "<code>$1</code>");
  }

  /** Bloques capturados → HTML. Solo texto escapado y nuestras propias etiquetas;
   *  una imagen es un hueco con data-media que se llena con un blob local. */
  function blocksHtml(blocks) {
    const out = [];
    let list = false;
    for (const b of Array.isArray(blocks) ? blocks : []) {
      if (!b || typeof b !== "object") continue;
      if (b.type === "li") {
        if (!list) { out.push("<ul>"); list = true; }
        out.push(`<li>${inlineText(b.text)}</li>`);
        continue;
      }
      if (list) { out.push("</ul>"); list = false; }
      if (b.type === "img") {
        if (!MEDIA_NAME.test(String(b.media || ""))) continue;
        out.push(`<figure class="nr-fig"><img data-media="${esc(b.media)}" alt="${esc(b.alt || "")}" loading="lazy">`
          + (b.alt ? `<figcaption>${esc(b.alt)}</figcaption>` : "") + "</figure>");
      } else if (b.type === "h") out.push(`<h3>${inlineText(b.text)}</h3>`);
      else if (b.type === "quote") out.push(`<blockquote>${inlineText(b.text)}</blockquote>`);
      else if (b.type === "code") out.push(`<pre><code>${esc(b.text)}</code></pre>`);
      else if (b.type === "caption") out.push(`<p class="nr-caption">${inlineText(b.text)}</p>`);
      else out.push(`<p>${inlineText(b.text)}</p>`);
    }
    if (list) out.push("</ul>");
    return out.join("");
  }

  /** «/nota texto» en el chat guarda una nota en vez de preguntar. */
  function noteCommand(text) {
    const m = String(text || "").match(/^\s*\/nota\s+([\s\S]+)$/i);
    return m ? m[1].trim() : null;
  }

  /** Notas agrupadas por día local: Hoy, Ayer y luego la fecha. */
  function notesByDay(notes, now, tz) {
    const zone = tz || "America/Mexico_City";
    const dayOf = ms => { try { return new Intl.DateTimeFormat("en-CA", { timeZone: zone }).format(new Date(ms)); } catch (e) { return new Date(ms).toISOString().slice(0, 10); } };
    const today = dayOf(now), yesterday = dayOf(now - 86400000);
    const groups = [];
    for (const n of notes || []) {
      const d = dayOf(n.createdAt);
      const label = d === today ? "Hoy" : d === yesterday ? "Ayer"
        : (() => { try { return new Intl.DateTimeFormat("es-MX", { timeZone: zone, day: "numeric", month: "long" }).format(new Date(n.createdAt)); } catch (e) { return d; } })();
      const last = groups[groups.length - 1];
      if (last && last.label === label) last.notes.push(n); else groups.push({ label, notes: [n] });
    }
    return groups;
  }

  function clampShare(value) {
    const n = Number(value);
    return Math.min(70, Math.max(40, Number.isFinite(n) ? n : 58));
  }

  function createStore(fetchJson) {
    const cache = new Map();
    return {
      list: () => fetchJson("/news/editions"),
      async edition(id, fresh) {
        if (!fresh && cache.has(id)) return cache.get(id);
        const data = await fetchJson("/news/edition?id=" + encodeURIComponent(id));
        if (data && data.edition && TERMINAL_STATUS.has(data.edition.status)) cache.set(id, data);
        return data;
      },
    };
  }

  // ------------------------------------------------------------------ DOM
  function readStore(key, fallback) {
    try { const v = localStorage.getItem(key); return v == null ? fallback : JSON.parse(v); }
    catch (e) { return fallback; }
  }
  function writeStore(key, value) { try { localStorage.setItem(key, JSON.stringify(value)); } catch (e) { /* private mode */ } }

  function tokenHeaders() {
    const headers = {};
    try { const t = localStorage.getItem("cc_token"); if (t) headers["X-Comandos-Token"] = t; } catch (e) { /* none */ }
    return headers;
  }

  function defaultFetchJson(url, body) {
    const init = { headers: tokenHeaders() };
    if (body !== undefined) {
      init.method = "POST";
      init.headers["Content-Type"] = "application/json";
      init.body = JSON.stringify(body);
    }
    return fetch(url, init).then(async r => {
      const j = await r.json().catch(() => ({}));
      if (!r.ok) throw new Error(j.error || r.statusText || "No se pudo leer");
      return j;
    });
  }

  // Uses the dashboard's own terminal helpers when present (index.html).
  async function defaultMountTerminal(host) {
    /* global sidebarActiveTab, resolveTermBase, webtermAccessToken, TERM_BASE, curTheme */
    const session = typeof sidebarActiveTab === "function" ? (sidebarActiveTab().session || "") : "";
    if (!session || typeof resolveTermBase !== "function" || typeof webtermAccessToken !== "function") {
      host.innerHTML = '<p class="nr-message">Abre una terminal en el tablero para verla aquí.</p>';
      return;
    }
    const base = await resolveTermBase();
    if (!base) {
      host.innerHTML = '<p class="nr-message">La terminal web no está disponible en esta vista.</p>';
      return;
    }
    const token = await webtermAccessToken();
    const frame = document.createElement("iframe");
    frame.className = "nr-term-frame";
    frame.title = "Terminal " + session;
    const primary = typeof TERM_BASE !== "undefined" && base === TERM_BASE;
    const theme = typeof curTheme !== "undefined" ? curTheme : "";
    frame.src = primary
      ? `${base}/?auth=${encodeURIComponent(token)}&arg=${encodeURIComponent(session)}&theme=${encodeURIComponent(theme)}&btn=${encodeURIComponent(document.documentElement.dataset.btnStyle || "sutil")}`
      : `${base}/?arg=${encodeURIComponent(token)}&arg=${encodeURIComponent(session)}`;
    host.replaceChildren(frame);
  }

  const TEMPLATE = `
  <header class="nr-header">
    <button type="button" class="nr-close" aria-label="Cerrar resúmenes">←<span class="nr-close-label"> Volver</span></button>
    <strong class="nr-title">Resúmenes</strong>
    <div class="nr-mobile-switch" role="group" aria-label="Vista">
      <button type="button" data-pane="terminal">Terminal</button>
      <button type="button" data-pane="reader">Resúmenes</button>
    </div>
    <button type="button" class="nr-saved-btn" aria-pressed="false">Guardados <i class="nr-count" data-count="saved"></i></button>
    <button type="button" class="nr-notes-btn">Mis notas <i class="nr-count" data-count="notes"></i></button>
    <button type="button" class="nr-terminal-toggle" aria-pressed="false">Ver terminal</button>
    <div class="nr-font" role="group" aria-label="Tamaño del texto">
      <button type="button" data-font="-1" aria-label="Reducir tamaño del texto">A−</button>
      <button type="button" data-font="1" aria-label="Aumentar tamaño del texto">A+</button>
    </div>
  </header>
  <div class="nr-stage">
    <section class="nr-terminal" aria-label="Terminal" hidden><div class="nr-terminal-host"></div></section>
    <div class="nr-edition-divider" role="separator" aria-orientation="vertical" aria-label="Ajustar ancho"
      aria-valuemin="40" aria-valuemax="70" tabindex="0" hidden></div>
    <section class="nr-reader" aria-label="Resumen">
      <div class="nr-reader-body" tabindex="-1"><nav class="nr-day" aria-label="Resúmenes del día"></nav><div class="nr-edition"></div></div>
      <aside class="nr-panel" aria-label="Noticia" hidden></aside>
    </section>
  </div>
  <div class="nr-toast" role="status" aria-live="polite"></div>`;

  function mount(options) {
    const opts = options || {};
    const doc = opts.document || document;
    const render = createRenderer(opts.markdownit || (typeof markdownit !== "undefined" ? markdownit : null),
      opts.purify === undefined ? (typeof DOMPurify !== "undefined" ? DOMPurify : null) : opts.purify);
    const fetchJson = opts.fetchJson || defaultFetchJson;
    const store = createStore(fetchJson);
    const mountTerminal = opts.mountTerminal || defaultMountTerminal;
    const el = doc.createElement("div");
    el.id = "news-reader";
    // embedded: dentro del área de terminales (remoto); desktop: vista propia de la app GTK.
    el.className = "nr-app nr-B" + (opts.embedded ? " nr-embedded" : "") + (opts.desktop ? " nr-desktop" : "");
    if (!opts.embedded && !opts.desktop) { el.setAttribute("role", "dialog"); el.setAttribute("aria-modal", "true"); }
    else el.setAttribute("role", "region");
    el.setAttribute("aria-label", "Resúmenes");
    el.hidden = true;
    el.innerHTML = TEMPLATE;
    (opts.parent || doc.body).appendChild(el);
    const $ = sel => el.querySelector(sel);
    const state = {
      list: null, current: null, view: "edition", terminal: false, terminalMounted: false,
      mobilePane: "reader", opener: null, details: false,
      size: Math.min(22, Math.max(14, Number(readStore("comandos.news.size", 16)) || 16)),
      share: clampShare(readStore("comandos.news.share", 58)),
      // Panel de la noticia abierta.
      open: null, tab: "resumen", scope: "esta", query: "", sourceId: null,
      original: new Set(), chat: new Map(), notes: null, allNotes: null, sources: new Map(),
      saved: null, noteDraft: "", chatDraft: "", editing: null, confirmDelete: null,
    };
    const media = new Map();                       // nombre → blob: URL
    const timers = { chat: null, translate: null, toast: null, search: null };
    const docEl = doc.documentElement, body = doc.body;

    function applyPrefs() {
      el.style.setProperty("--read-size", state.size + "px");
      el.style.setProperty("--nr-share", state.share + "%");
      if (opts.embedded) docEl.style.setProperty("--nr-share", state.share + "%");
      $(".nr-edition-divider").setAttribute("aria-valuenow", String(Math.round(state.share)));
    }

    function toast(text) {
      const t = $(".nr-toast");
      t.textContent = text;
      t.classList.add("show");
      clearTimeout(timers.toast);
      timers.toast = setTimeout(() => t.classList.remove("show"), 1800);
    }

    function storyById(id) {
      return state.current && (state.current.stories || []).find(s => s.id === id);
    }

    function setCount(name, n) {
      const node = el.querySelector(`[data-count="${name}"]`);
      if (node) node.textContent = n ? String(n) : "";
    }

    // Línea del día: una tarjeta por resumen de hoy (y el siguiente programado).
    function renderPicker() {
      const today = state.current ? state.current.edition.localDate
        : (((state.list && state.list.editions) || [])[0] || {}).localDate;
      const cards = dayLine(state.list, today);
      const cur = state.current && state.current.edition.id;
      $(".nr-day").innerHTML = cards.map(c => {
        const readable = c.status === "published" || c.status === "partial";
        const sub = readable ? `${esc(c.storyCount)} noticias${Object.keys(c.models || {}).length ? " · " + esc(Object.keys(c.models).map(modelName)[0]) : ""}`
          : c.status === "scheduled" ? "Se generará a su hora" : esc(((c.notes || [])[0] || "").slice(0, 70));
        return `<button type="button" class="nr-slot${c.id === cur && state.view === "edition" ? " on" : ""}" data-edition="${esc(c.id)}" ${c.status === "scheduled" ? "disabled" : ""} aria-pressed="${c.id === cur}">`
          + `<small>${esc(c.day)}</small><b>${esc(c.slot)}</b><span class="nr-st st-${esc(c.status)}">${esc(statusLabel(c.status))}</span><em>${sub}</em></button>`;
      }).join("");
      $(".nr-day").hidden = !cards.length;
    }

    function message(html) { $(".nr-edition").innerHTML = `<div class="nr-state">${html}</div>`; }

    function opportunityHtml(opp) {
      if (!opp) return "";
      const rows = [["Recompensa", opp.reward], ["Fecha límite", opp.deadline], ["Zona horaria", opp.timezone],
        ["Elegibilidad", opp.eligibility], ["Entrega", opp.submission]];
      return `<dl class="nr-bounty-facts">${rows.map(([k, v]) => `<div><dt>${esc(k)}</dt><dd>${esc(v || "desconocido")}</dd></div>`).join("")}</dl>`;
    }

    function storyHtml(story, rank) {
      const k = storyKicker(story);
      const counts = story.counts || {};
      const isOpen = state.open === story.id;
      const n = (story.sources || []).length;
      return `<section class="nr-edition-story${isOpen ? " on" : ""}" id="nr-story-${esc(story.id)}" data-story="${esc(story.id)}">
        <small class="nr-kicker"><span class="${k.hot ? "nr-hot" : ""}">${esc(k.label)}</span><span class="nr-n">#${rank} del día</span>`
        + `<span class="nr-n">${n} fuente${n === 1 ? "" : "s"}</span>${counts.notes ? `<span class="nr-nn">${counts.notes} nota${counts.notes === 1 ? "" : "s"}</span>` : ""}</small>
        <h2><button type="button" class="nr-open-story" data-open="${esc(story.id)}">${esc(story.title)}</button></h2>
        <div class="nr-markdown nr-summary">${render(story.summary)}</div>
        <div class="nr-acts"><button type="button" class="nr-open-btn${isOpen ? " on" : ""}" data-open="${esc(story.id)}">${isOpen ? "Abierta →" : "Abrir"}</button>`
        + `<button type="button" class="nr-save" data-save="${esc(story.id)}" aria-pressed="${!!counts.saved}">${counts.saved ? "Guardada" : "Guardar"}</button></div>
      </section>`;
    }

    function detailsHtml(edition, tz) {
      const job = edition.job || {};
      const p = provenance(edition);
      const chain = ((state.list && state.list.chain) || []).map(modelName).join(" → ");
      const rows = [
        ["Modelos que escribieron", p.models || "—"],
        ["Cadena de respaldo", chain || "—"],
        ["Costo", p.cost],
        ["Programado", `${edition.localDate} ${edition.slot}`],
        ["Inicio · fin", job.startedAt ? `${formatDate(job.startedAt, tz)} · ${job.finishedAt ? formatDate(job.finishedAt, tz) : "en curso"}` : "—"],
        ["Duración", p.duration || "—"],
        ["Intentos", job.attempts || "—"],
        ["Fuentes", p.sources || "—"],
      ];
      const notes = (edition.notes || []).map(n => `<li>${esc(n)}</li>`).join("");
      return `<div class="nr-details"><dl>${rows.map(([k, v]) => `<div><dt>${esc(k)}</dt><dd>${esc(v)}</dd></div>`).join("")}</dl>`
        + (notes ? `<h3>Qué falló o quedó incompleto</h3><ul>${notes}</ul>` : "") + "</div>";
    }

    function renderEdition() {
      if (state.view === "saved") return renderSaved();
      const data = state.current;
      if (!data) return;
      const edition = data.edition;
      const tz = edition.timezone || "America/Mexico_City";
      const stories = data.stories || [];
      const readable = edition.status === "published" || edition.status === "partial";
      const p = provenance(edition);
      const line = readable
        ? `<p class="nr-prov"><span>Escrito por <b>${esc(p.models || "IA")}</b></span><span>${esc(edition.storyCount)} noticias</span>`
          + `${p.sources ? `<span>${esc(p.sources)}</span>` : ""}<span>${esc(p.cost)}${p.duration ? " · " + esc(p.duration) : ""}</span>`
          + `<button type="button" class="nr-details-toggle" aria-expanded="${state.details}">${p.problems ? `${p.problems} aviso${p.problems === 1 ? "" : "s"} · ` : ""}${state.details ? "Ocultar detalles" : "Detalles"}</button></p>`
        : `<p class="nr-prov nr-bad">${esc(statusLabel(edition.status))}: ${esc((edition.notes || [])[0] || "sin noticias publicadas")}</p>`;
      const head = `<header class="nr-edition-head">
        <small>${esc(edition.localDate)} · ${esc(statusLabel(edition.status))}</small>
        <h1>Resumen de las ${esc(edition.slot)}</h1>
        ${edition.lead ? `<p class="nr-lead">${esc(edition.lead)}</p>` : ""}
        ${line}${state.details || !readable ? detailsHtml(edition, tz) : ""}
      </header>`;
      const list = stories.length ? stories.map((s, i) => storyHtml(s, i + 1)).join("")
        : `<p class="nr-empty">${readable ? "No hay noticias en este resumen." : "Este resumen no tiene noticias."}</p>`;
      const next = state.list && state.list.next;
      const foot = stories.length ? `<p class="nr-foot">Fin del resumen${next ? ` · el siguiente sale a las ${esc(next.slot)}` : ""}.</p>` : "";
      $(".nr-edition").innerHTML = head + list + foot;
      renderCounts();
    }

    function renderCounts() {
      const total = state.allNotes ? state.allNotes.total : null;
      if (total !== null) setCount("notes", total);
      if (state.saved) setCount("saved", state.saved.length);
    }

    async function loadSaved() {
      try { state.saved = (await fetchJson("/news/saved")).stories || []; }
      catch (err) { state.saved = state.saved || []; toast(`No pude leer las guardadas: ${err.message || err}`); }
      renderCounts();
    }

    function renderSaved() {
      const list = state.saved || [];
      $(".nr-edition").innerHTML = `<header class="nr-edition-head"><small>Guardadas</small><h1>Noticias guardadas</h1>
        <p class="nr-prov"><span>${list.length} noticia${list.length === 1 ? "" : "s"}</span><button type="button" class="nr-back-edition">Volver al resumen</button></p></header>`
        + (list.length ? list.map(s => {
          const k = storyKicker({ category: "", ...s });
          return `<section class="nr-edition-story"><small class="nr-kicker"><span>${esc(s.meta && s.meta.lab ? s.meta.lab : k.label)}</span>`
            + `<span class="nr-n">${esc(s.editionId.replace("@", " · "))}</span></small>
            <h2><button type="button" class="nr-open-story" data-goto="${esc(s.editionId)}" data-goto-story="${esc(s.id)}">${esc(s.title)}</button></h2>
            <div class="nr-markdown nr-summary">${render(s.summary)}</div>
            <div class="nr-acts"><button type="button" data-goto="${esc(s.editionId)}" data-goto-story="${esc(s.id)}">Abrir</button>`
            + `<button type="button" class="nr-save" data-save="${esc(s.id)}" aria-pressed="true">Guardada</button></div></section>`;
        }).join("") : `<p class="nr-empty">Aún no guardas noticias. Toca «Guardar» en una para tenerla aquí.</p>`);
      $(".nr-saved-btn").setAttribute("aria-pressed", "true");
    }

    // ---------------------------------------------------------------- panel
    function sourcesOf(story) { return (story && story.sources) || []; }

    function panelTabs(story) {
      const counts = story.counts || {};
      const n = { fuentes: sourcesOf(story).length, chat: counts.chat || 0, notas: counts.notes || 0 };
      return `<div class="nr-tabs" role="tablist">${TABS.map(([key, label]) =>
        `<button type="button" role="tab" data-tab="${key}" aria-selected="${state.tab === key}" class="${state.tab === key ? "on" : ""}">${label}${n[key] ? `<i>${n[key]}</i>` : ""}</button>`).join("")}</div>`;
    }

    function renderPanel() {
      const panel = $(".nr-panel");
      const reader = $(".nr-reader");
      const notesOnly = state.open === "notes";
      const story = notesOnly ? null : storyById(state.open);
      if (!story && !notesOnly) {
        panel.hidden = true;
        reader.classList.remove("nr-panel-open");
        return;
      }
      panel.hidden = false;
      reader.classList.add("nr-panel-open");
      if (notesOnly) {
        panel.innerHTML = `<div class="nr-ph"><button type="button" class="nr-panel-back" aria-label="Volver a la lista">←</button><strong class="nr-ph-title">Mis notas</strong><button type="button" class="nr-panel-close" aria-label="Cerrar panel">✕</button></div>
          <div class="nr-pb">${notesView(null)}</div>`;
        afterPanel();
        return;
      }
      const k = storyKicker(story);
      let bodyHtml = "", dock = "";
      if (state.tab === "resumen") {
        bodyHtml = `<div class="nr-markdown nr-summary">${render(story.summary)}</div>${opportunityHtml(story.opportunity)}<div class="nr-markdown">${render(story.body)}</div>`
          + `<p class="nr-foot">Resumen IA${story.model ? " · " + esc(modelName(story.model)) : ""}. Las fuentes completas están en «Fuentes».</p>`;
      } else if (state.tab === "fuentes") {
        bodyHtml = sourcesView(story);
      } else if (state.tab === "chat") {
        bodyHtml = chatView(story);
        dock = `<form class="nr-dock"><input class="nr-chat-input" autocomplete="off" placeholder="Pregunta a esta noticia… (/nota guarda lo que escribas)" value="${esc(state.chatDraft)}" aria-label="Mensaje"><button type="submit" class="nr-pri">Enviar</button></form>`;
      } else {
        bodyHtml = notesView(story);
      }
      panel.innerHTML = `<div class="nr-ph"><button type="button" class="nr-panel-back" aria-label="Volver a la lista">←</button>${panelTabs(story)}<button type="button" class="nr-panel-close" aria-label="Cerrar panel">✕</button></div>
        <div class="nr-pt"><span class="${k.hot ? "nr-hot" : ""}">${esc(k.label)}</span><b>${esc(story.title)}</b></div>
        <div class="nr-pb">${bodyHtml}</div>${dock}`;
      afterPanel();
    }

    function afterPanel() {
      loadMedia($(".nr-panel"));
      const pb = $(".nr-panel .nr-pb");
      if (state.tab === "chat" && pb && state.stickBottom) { pb.scrollTop = pb.scrollHeight; state.stickBottom = false; }
    }

    async function loadMedia(scope) {
      for (const img of scope.querySelectorAll("img[data-media]")) {
        const name = img.dataset.media;
        if (!MEDIA_NAME.test(name) || img.dataset.loaded) continue;
        img.dataset.loaded = "1";
        try {
          if (!media.has(name)) {
            const r = await fetch("/news/media/" + name, { headers: tokenHeaders() });
            if (!r.ok) throw new Error(String(r.status));
            media.set(name, URL.createObjectURL(await r.blob()));
          }
          img.src = media.get(name);
        } catch (e) {
          img.closest("figure")?.classList.add("nr-fig-missing");
        }
      }
    }

    function sourcesView(story) {
      const sources = sourcesOf(story);
      if (!sources.length) return `<p class="nr-empty">Esta noticia no tiene fuentes guardadas.</p>`;
      const current = sources.find(s => s.id === state.sourceId) || sources.find(s => s.captured) || sources[0];
      state.sourceId = current.id;
      const tabs = `<div class="nr-src-list">${sources.map(s =>
        `<button type="button" class="nr-src-pick${s.id === current.id ? " on" : ""}" data-source="${esc(s.id)}">`
        + `<b>${esc(s.official ? "Oficial · " + s.origin : s.origin)}</b><small>${esc(s.heat || hostOf(s.url))}</small></button>`).join("")}</div>`;
      const loaded = state.sources.get(current.id);
      let article;
      if (!loaded) article = `<p class="nr-empty">Cargando la fuente…</p>`;
      else if (loaded.error) article = `<p class="nr-empty">No pude abrir la fuente: ${esc(loaded.error)}</p>`;
      else article = sourceArticle(loaded);
      if (!loaded) loadSource(current.id);
      return tabs + article;
    }

    function sourceArticle(src) {
      const cap = src.capture;
      const href = safeHref(src.url);
      const tr = src.translation;
      const showOriginal = state.original.has(src.id) || !tr || tr.state !== "done";
      const trButton = !cap ? "" : !tr || tr.state === "failed"
        ? `<button type="button" class="nr-xs" data-translate="${esc(src.id)}">${tr && tr.state === "failed" ? "Reintentar traducción" : "Traducir con IA"}</button>`
        : tr.state === "running" ? `<span class="nr-xs nr-busy">Traduciendo…</span>`
        : `<button type="button" class="nr-xs${showOriginal ? "" : " nr-saved"}" data-lang="es" data-source-lang="${esc(src.id)}">★ Traducción guardada</button>`
          + `<button type="button" class="nr-xs${showOriginal ? " nr-saved" : ""}" data-lang="orig" data-source-lang="${esc(src.id)}">Original</button>`;
      const head = `<div class="nr-src-head">${src.official ? `<span class="nr-of">Fuente oficial</span>` : ""}<span class="nr-src-host">${esc(hostOf(src.url))}${src.heat ? " · " + esc(src.heat) : ""}</span>`
        + `<span class="nr-sp"></span>${trButton}${href ? `<a class="nr-xs" href="${esc(href)}" target="_blank" rel="noopener noreferrer nofollow">Abrir ↗</a>` : ""}</div>`;
      if (!cap) {
        return `<article class="nr-article">${head}<h1>${esc(src.title)}</h1><p class="nr-empty">Esta fuente no se pudo capturar completa${src.role === "discussion" ? " (el hilo no respondió)" : ""}. Ábrela en su sitio para leerla.</p></article>`;
      }
      const translated = !showOriginal;
      const title = translated ? (tr.title || cap.title || src.title) : (cap.title || src.title);
      const blocks = translated ? tr.blocks : cap.blocks;
      const by = [cap.byline, src.publishedAt ? formatDate(src.publishedAt) : "", translated && tr.model ? "traducido con " + modelName(tr.model) : ""].filter(Boolean).join(" · ");
      const failed = tr && tr.state === "failed" ? `<p class="nr-note-bad">La traducción falló: ${esc(tr.error || "sin detalle")}.</p>` : "";
      return `<article class="nr-article">${head}${failed}<h1>${esc(title)}</h1>${by ? `<div class="nr-by">${esc(by)}</div>` : ""}`
        + `<div class="nr-markdown nr-captured">${blocksHtml(blocks)}</div>`
        + `<p class="nr-foot">Capturada ${esc(formatDate(cap.capturedAt))}${cap.partial ? " · solo el extracto que publicó el feed" : ""}; se lee aunque el sitio cambie.</p></article>`;
    }

    async function loadSource(id, quiet) {
      try {
        const src = await fetchJson("/news/source?id=" + encodeURIComponent(id));
        state.sources.set(id, src);
      } catch (err) {
        state.sources.set(id, { error: err.message || String(err) });
      }
      if (!quiet || state.tab === "fuentes") renderPanel();
      const tr = state.sources.get(id) && state.sources.get(id).translation;
      clearTimeout(timers.translate);
      if (tr && tr.state === "running" && state.tab === "fuentes" && state.sourceId === id)
        timers.translate = setTimeout(() => loadSource(id, true), 3000);
    }

    function chatView(story) {
      const msgs = state.chat.get(story.id);
      if (!msgs) { loadChat(story.id); return `<p class="nr-empty">Cargando la conversación…</p>`; }
      const ctx = `<div class="nr-ctx">Chat con esta noticia · ${sourcesOf(story).length} fuentes capturadas · se guarda con la noticia</div>`;
      if (!msgs.length) {
        return ctx + `<p class="nr-empty">Pregunta lo que quieras de esta noticia. La IA responde con las fuentes capturadas y cita el párrafo.</p>
          <div class="nr-chips">${["¿Qué cambia para mí?", "¿Qué dice la comunidad?", "¿Qué no dice la fuente?"].map(q =>
            `<button type="button" data-ask="${esc(q)}">${esc(q)}</button>`).join("")}</div>`;
      }
      return ctx + msgs.map(m => {
        const mine = m.role === "user";
        const text = m.state === "pending" ? `<span class="nr-thinking">Pensando<i>.</i><i>.</i><i>.</i></span>`
          : mine ? `<p>${esc(m.text).replace(/\n/g, "<br>")}</p>` : `<div class="nr-markdown">${render(m.text)}</div>`;
        const tools = m.state === "done" ? `<div class="nr-tip"><button type="button" class="nr-xs${m.noted ? " nr-saved" : ""}" data-note-chat="${esc(m.id)}" title="${m.noted ? "Quitar de mis notas" : "Guardar como nota"}" aria-pressed="${!!m.noted}">${m.noted ? "★" : "☆"}</button>`
          + `<button type="button" class="nr-xs" data-copy-chat="${esc(m.id)}" title="Copiar">⎘</button></div>` : "";
        return `<div class="nr-msg${mine ? " me" : ""}${m.noted ? " noted" : ""}${m.state === "failed" ? " failed" : ""}">${tools}<small class="nr-who">${mine ? "Tú" : esc(m.model ? modelName(m.model) : "IA") + " · con las fuentes"}</small>${text}${m.cite ? `<cite>${esc(m.cite)}</cite>` : ""}</div>`;
      }).join("") + `<p class="nr-foot">☆ en una burbuja la guarda como nota con su cita.</p>`;
    }

    async function loadChat(storyId) {
      try {
        const data = await fetchJson("/news/chat?story=" + encodeURIComponent(storyId));
        state.chat.set(storyId, data.messages || []);
      } catch (err) {
        state.chat.set(storyId, []);
        toast(`No pude leer el chat: ${err.message || err}`);
      }
      syncCounts(storyId);
      if (state.open === storyId && state.tab === "chat") renderPanel();
      clearTimeout(timers.chat);
      if ((state.chat.get(storyId) || []).some(m => m.state === "pending"))
        timers.chat = setTimeout(() => { state.stickBottom = true; loadChat(storyId); }, 2500);
    }

    async function sendChat(text) {
      const story = storyById(state.open);
      const msg = String(text || "").trim();
      if (!story || !msg) return;
      const note = noteCommand(msg);
      state.chatDraft = "";
      if (note) {
        try {
          await fetchJson("/news/notes", { action: "add", storyId: story.id, text: note });
          toast("Nota guardada");
          state.notes = null; state.allNotes = null;
          await refreshNotes(story.id);
        } catch (err) { toast(`No se guardó la nota: ${err.message || err}`); }
        renderPanel();
        return;
      }
      try {
        const data = await fetchJson("/news/chat", { storyId: story.id, message: msg });
        state.chat.set(story.id, data.messages || []);
        state.stickBottom = true;
        syncCounts(story.id);
        renderPanel();
        clearTimeout(timers.chat);
        timers.chat = setTimeout(() => loadChat(story.id), 2500);
      } catch (err) {
        state.chatDraft = msg;
        toast(`No se envió: ${err.message || err}`);
        renderPanel();
      }
    }

    function syncCounts(storyId) {
      const story = storyById(storyId);
      if (!story) return;
      story.counts = story.counts || {};
      const msgs = state.chat.get(storyId);
      if (msgs) story.counts.chat = msgs.length;
      if (state.notes && state.notes.storyId === storyId) story.counts.notes = state.notes.notes.length;
      const node = el.querySelector(`#nr-story-${CSS.escape(String(storyId))} .nr-kicker`);
      if (node) {
        const old = node.querySelector(".nr-nn");
        if (old) old.remove();
        if (story.counts.notes) node.insertAdjacentHTML("beforeend", `<span class="nr-nn">${story.counts.notes} nota${story.counts.notes === 1 ? "" : "s"}</span>`);
      }
    }

    async function refreshNotes(storyId) {
      try {
        if (storyId) {
          const data = await fetchJson("/news/notes?story=" + encodeURIComponent(storyId));
          state.notes = { storyId, notes: data.notes || [] };
          state.allNotes = state.allNotes ? { ...state.allNotes, total: data.total } : { notes: null, total: data.total };
          syncCounts(storyId);
        }
        if (state.scope === "todas" || !storyId) {
          const data = await fetchJson("/news/notes?q=" + encodeURIComponent(state.query));
          state.allNotes = { notes: data.notes || [], total: data.total };
        }
      } catch (err) {
        // Sin esto el panel volvería a pedirlas en cada pintado.
        if (storyId && (!state.notes || state.notes.storyId !== storyId)) state.notes = { storyId, notes: [] };
        if (!state.allNotes || !state.allNotes.notes) state.allNotes = { notes: [], total: 0 };
        toast(`No pude leer las notas: ${err.message || err}`);
      }
      renderCounts();
    }

    function noteItem(n, withStory) {
      const editing = state.editing === n.id;
      const confirm = state.confirmDelete === n.id;
      const meta = `${esc(formatDate(n.createdAt))}${withStory ? ` · <b>${esc(n.storyTitle)}</b>` : ""} · ${n.kind === "chat" ? "del chat" : "escrita"}`;
      return `<div class="nr-note" data-note="${esc(n.id)}"><small>${meta}</small>`
        + (n.quote ? `<div class="nr-quote">${render(n.quote)}</div>` : "")
        + (editing ? `<textarea class="nr-note-edit" aria-label="Editar nota">${esc(n.text)}</textarea>`
          : n.text ? `<p>${esc(n.text).replace(/\n/g, "<br>")}</p>` : "")
        + (n.cite ? `<cite>${esc(n.cite)}</cite>` : "")
        + `<div class="nr-note-acts">`
        + (editing ? `<button type="button" class="nr-xs nr-saved" data-note-save="${esc(n.id)}">Guardar</button><button type="button" class="nr-xs" data-note-cancel="1">Cancelar</button>`
          : `<button type="button" class="nr-xs" data-note-edit="${esc(n.id)}">Editar</button>`
            + (withStory ? `<button type="button" class="nr-xs" data-goto="${esc(n.editionId)}" data-goto-story="${esc(n.storyId)}" data-goto-tab="notas">Ir a la noticia</button>` : "")
            + `<button type="button" class="nr-xs" data-note-copy="${esc(n.id)}">Copiar</button>`
            + (confirm ? `<button type="button" class="nr-xs nr-danger" data-note-delete="${esc(n.id)}">¿Borrar? Sí</button><button type="button" class="nr-xs" data-note-cancel="1">No</button>`
              : `<button type="button" class="nr-xs" data-note-ask-delete="${esc(n.id)}">Borrar</button>`))
        + `</div></div>`;
    }

    function notesView(story) {
      const all = !story || state.scope === "todas";
      if (story && (!state.notes || state.notes.storyId !== story.id)) {
        refreshNotes(story.id).then(renderPanel);
        return `<p class="nr-empty">Cargando notas…</p>`;
      }
      if (all && (!state.allNotes || !state.allNotes.notes)) {
        refreshNotes(story ? story.id : null).then(renderPanel);
        return `<p class="nr-empty">Cargando notas…</p>`;
      }
      const mine = story ? state.notes.notes : [];
      const total = state.allNotes ? state.allNotes.total : mine.length;
      const scope = story ? `<div class="nr-chips"><button type="button" data-scope="esta" class="${!all ? "on" : ""}">Esta noticia <i>${mine.length}</i></button>`
        + `<button type="button" data-scope="todas" class="${all ? "on" : ""}">Todas <i>${total}</i></button></div>` : "";
      if (!all) {
        return scope + (mine.length ? mine.map(n => noteItem(n, false)).join("")
          : `<p class="nr-empty">Sin notas en esta noticia. Escribe una abajo o guarda una burbuja del chat con ☆.</p>`)
          + `<div class="nr-newnote"><textarea class="nr-note-new" placeholder="Nueva nota sobre esta noticia… (Ctrl+Enter guarda)" aria-label="Nueva nota">${esc(state.noteDraft)}</textarea>`
          + `<div class="nr-chips"><button type="button" class="nr-pri" data-note-add="1">Guardar nota</button></div></div>`;
      }
      const notes = state.allNotes.notes || [];
      const groups = notesByDay(notes, Date.now());
      return scope + `<div class="nr-search"><input class="nr-notes-q" type="search" placeholder="Buscar en todas mis notas…" value="${esc(state.query)}" aria-label="Buscar en mis notas">`
        + `<button type="button" class="nr-xs" data-notes-export="1">Copiar como .md</button></div>`
        + (groups.length ? groups.map(g => `<div class="nr-day-label">${esc(g.label)}</div>` + g.notes.map(n => noteItem(n, true)).join("")).join("")
          : `<p class="nr-empty">${state.query ? "Ninguna nota coincide." : "Aún no tienes notas."}</p>`);
    }

    function notesMarkdown(notes) {
      return notesByDay(notes, Date.now()).map(g => `## ${g.label}\n\n` + g.notes.map(n =>
        `### ${n.storyTitle}\n` + (n.quote ? n.quote.split("\n").map(l => "> " + l).join("\n") + "\n\n" : "")
        + (n.text ? n.text + "\n" : "") + (n.cite ? `\n_${n.cite}_\n` : "")).join("\n")).join("\n");
    }

    async function copyText(text) {
      try { await navigator.clipboard.writeText(text); toast("Copiado"); }
      catch (e) { toast("No se pudo copiar"); }
    }

    function openStory(id, tab) {
      const story = storyById(id);
      if (!story) return;
      const same = state.open === id;
      if (!same) { state.sourceId = null; state.notes = null; state.scope = "esta"; state.editing = null; state.confirmDelete = null; }
      state.open = id;
      state.tab = tab || (same ? state.tab : "resumen");
      el.querySelectorAll(".nr-edition-story").forEach(n => {
        const on = n.dataset.story === String(id);
        n.classList.toggle("on", on);
        const b = n.querySelector(".nr-open-btn");
        if (b) { b.classList.toggle("on", on); b.textContent = on ? "Abierta →" : "Abrir"; }
      });
      renderPanel();
      const pb = $(".nr-panel .nr-pb");
      if (pb) pb.scrollTop = 0;
      el.querySelector(`#nr-story-${CSS.escape(String(id))}`)?.scrollIntoView({ block: "nearest" });
    }

    function closePanel() {
      state.open = null;
      clearTimeout(timers.chat);
      clearTimeout(timers.translate);
      el.querySelectorAll(".nr-edition-story.on").forEach(n => n.classList.remove("on"));
      el.querySelectorAll(".nr-open-btn.on").forEach(b => { b.classList.remove("on"); b.textContent = "Abrir"; });
      renderPanel();
    }

    async function gotoStory(editionId, storyId, tab) {
      state.view = "edition";
      $(".nr-saved-btn").setAttribute("aria-pressed", "false");
      if (!state.current || state.current.edition.id !== editionId) await loadEdition(editionId);
      else renderEdition();
      renderPicker();
      openStory(Number(storyId), tab || "resumen");
    }

    // Keep the reading position when the layout changes (terminal, font, pane).
    function captureAnchor() {
      const rb = $(".nr-reader-body");
      const top = rb.getBoundingClientRect().top;
      const nodes = rb.querySelectorAll(".nr-edition-head, .nr-edition-story");
      let anchor = null;
      for (const n of nodes) { if (n.getBoundingClientRect().top <= top + 12) anchor = n; }
      return anchor ? { node: anchor, offset: anchor.getBoundingClientRect().top - top } : null;
    }
    function restoreAnchor(mark) {
      if (!mark || !mark.node.isConnected) return;
      const rb = $(".nr-reader-body");
      rb.scrollTop += (mark.node.getBoundingClientRect().top - rb.getBoundingClientRect().top) - mark.offset;
    }
    function relayout(change) {
      const mark = captureAnchor();
      change();
      applyPrefs();
      requestAnimationFrame(() => restoreAnchor(mark));
    }

    function narrow() { return (doc.defaultView || window).matchMedia("(max-width: 560px)").matches; }

    function showPane(pane) {
      state.mobilePane = pane;
      if (opts.embedded) {                         // móvil: páginas completas Terminal / Resúmenes
        body.classList.toggle("nr-peek", pane === "terminal" && narrow());
        el.querySelectorAll(".nr-mobile-switch button").forEach(b => b.setAttribute("aria-pressed", String(b.dataset.pane === pane)));
        return;
      }
      const stage = $(".nr-stage");
      if (state.terminal && narrow()) stage.scrollTo({ left: pane === "reader" ? stage.scrollWidth : 0, behavior: "smooth" });
      el.querySelectorAll(".nr-mobile-switch button").forEach(b => b.setAttribute("aria-pressed", String(b.dataset.pane === pane)));
    }

    async function setTerminal(on) {
      relayout(() => {
        state.terminal = on;
        el.classList.toggle("nr-with-terminal", on && !opts.embedded && !opts.desktop);
        el.classList.toggle("nr-docked", on);
        if (opts.embedded) body.classList.toggle("nr-docked", on);
        $(".nr-terminal").hidden = !on || !!opts.embedded || !!opts.desktop;   // hidden, never removed
        $(".nr-edition-divider").hidden = !on;
        const t = $(".nr-terminal-toggle");
        t.setAttribute("aria-pressed", String(on));
        t.textContent = on ? "Ocultar terminal" : "Ver terminal";
        t.classList.toggle("on", on);
      });
      if (opts.desktop) { if (opts.externalTerminal) opts.externalTerminal(on); return; }
      if (opts.embedded) {                          // la terminal real queda a la izquierda
        if (on) { writeStore("comandos.news.terminal", true); showPane(narrow() ? "terminal" : "reader"); }
        else { writeStore("comandos.news.terminal", false); showPane("reader"); }
        return;
      }
      if (on) {
        showPane("terminal");
        if (!state.terminalMounted) {
          state.terminalMounted = true;
          try { await mountTerminal($(".nr-terminal-host")); }
          catch (err) { $(".nr-terminal-host").innerHTML = `<p class="nr-message">${esc(err.message || err)}</p>`; }
        }
      } else {
        showPane("reader");
      }
    }

    async function loadEdition(id) {
      try {
        const data = await store.edition(id, true);
        state.current = data;
        state.view = "edition";
        $(".nr-saved-btn").setAttribute("aria-pressed", "false");
        if (state.open !== "notes" && !storyById(state.open)) state.open = null;
        renderPicker();
        renderEdition();
        renderPanel();
        $(".nr-reader-body").scrollTop = 0;
      } catch (err) {
        message(`No se pudo abrir el resumen: ${esc(err.message || err)}`);
      }
    }

    async function open(editionId) {
      state.opener = doc.activeElement;
      el.hidden = false;
      if (opts.embedded) {
        body.classList.add("nr-reading");
        if (readStore("comandos.news.terminal", false) && !state.terminal) setTerminal(true);
        else if (state.terminal) body.classList.add("nr-docked");
      } else doc.documentElement.classList.add("nr-open");
      if (opts.onOpenChange) opts.onOpenChange(true);
      applyPrefs();
      $(".nr-reader-body").focus({ preventScroll: true });
      loadSaved();
      refreshNotes(null);
      if (state.current && !editionId) return;          // reopen: keep position, no fetch
      if (!state.current) message("Cargando resúmenes…");
      try {
        state.list = await store.list();
      } catch (err) {
        message(`No se pudo leer la lista de resúmenes: ${esc(err.message || err)}. Revisa la conexión con ComandOS.`);
        return;
      }
      renderPicker();
      const target = editionId || state.list.latest;
      if (!target) {
        message(state.list.configured
          ? "Aún no hay resúmenes. El próximo llega a su hora programada."
          : `Resúmenes ${esc(state.list.reason || "sin configurar")}. No se muestran noticias inventadas; configura <code>news-editions.json</code> para activar los tres resúmenes diarios.`);
        return;
      }
      await loadEdition(target);
    }

    function close() {
      if (opts.desktop && opts.onClose) { opts.onClose(); return; }
      el.hidden = true;
      clearTimeout(timers.chat);
      clearTimeout(timers.translate);
      doc.documentElement.classList.remove("nr-open");
      body.classList.remove("nr-reading", "nr-docked", "nr-peek");
      if (opts.onOpenChange) opts.onOpenChange(false);
      if (state.opener && state.opener.focus) state.opener.focus({ preventScroll: true });
    }

    async function toggleSave(id, button) {
      const on = button.getAttribute("aria-pressed") !== "true";
      try {
        await fetchJson("/news/saved", { storyId: Number(id), saved: on });
        const story = storyById(Number(id));
        if (story) story.counts = { ...(story.counts || {}), saved: on };
        el.querySelectorAll(`[data-save="${CSS.escape(String(id))}"]`).forEach(b => {
          b.setAttribute("aria-pressed", String(on));
          b.textContent = on ? "Guardada" : "Guardar";
        });
        await loadSaved();
        if (state.view === "saved") renderSaved();
      } catch (err) { toast(`No se guardó: ${err.message || err}`); }
    }

    // Events -----------------------------------------------------------
    $(".nr-close").addEventListener("click", close);
    el.addEventListener("keydown", e => {
      if (e.key !== "Escape" || e.target.closest(".nr-terminal")) return;
      if (state.open !== null && !e.target.closest("textarea, input")) { closePanel(); return; }
      if (!e.target.closest("textarea, input")) close();
    });
    $(".nr-terminal-toggle").addEventListener("click", () => setTerminal(!state.terminal));
    $(".nr-saved-btn").addEventListener("click", async () => {
      if (state.view === "saved") { state.view = "edition"; $(".nr-saved-btn").setAttribute("aria-pressed", "false"); renderEdition(); renderPicker(); return; }
      state.view = "saved";
      await loadSaved();
      renderSaved();
      renderPicker();
      $(".nr-reader-body").scrollTop = 0;
    });
    $(".nr-notes-btn").addEventListener("click", () => {
      state.scope = "todas";
      if (state.open !== null && state.open !== "notes") { state.tab = "notas"; renderPanel(); return; }
      state.open = "notes";
      state.allNotes = state.allNotes && { ...state.allNotes, notes: null };
      renderPanel();
    });
    $(".nr-day").addEventListener("click", e => {
      const b = e.target.closest("[data-edition]");
      if (b && !b.disabled) loadEdition(b.dataset.edition);
    });
    el.querySelectorAll(".nr-mobile-switch button").forEach(b => b.addEventListener("click", () => {
      if (b.dataset.pane === "terminal" && !state.terminal) setTerminal(true); else showPane(b.dataset.pane);
    }));
    el.querySelectorAll("[data-font]").forEach(b => b.addEventListener("click", () => relayout(() => {
      state.size = Math.min(22, Math.max(14, state.size + Number(b.dataset.font)));
      writeStore("comandos.news.size", state.size);
    })));
    $(".nr-edition").addEventListener("click", e => {
      if (e.target.closest(".nr-details-toggle")) { state.details = !state.details; renderEdition(); return; }
      if (e.target.closest(".nr-back-edition")) { $(".nr-saved-btn").click(); return; }
      const go = e.target.closest("[data-goto]");
      if (go) { gotoStory(go.dataset.goto, go.dataset.gotoStory); return; }
      const openBtn = e.target.closest("[data-open]");
      if (openBtn) {
        const id = Number(openBtn.dataset.open);
        if (state.open === id && openBtn.classList.contains("nr-open-btn")) closePanel(); else openStory(id, state.open === id ? state.tab : "resumen");
        return;
      }
      const save = e.target.closest("[data-save]");
      if (save) toggleSave(save.dataset.save, save);   // saving never creates a reminder
    });

    const panel = $(".nr-panel");
    panel.addEventListener("click", async e => {
      const t = e.target;
      if (t.closest(".nr-panel-close") || t.closest(".nr-panel-back")) { closePanel(); return; }
      const tab = t.closest("[data-tab]");
      if (tab) { state.tab = tab.dataset.tab; state.confirmDelete = null; state.editing = null; renderPanel(); return; }
      const pick = t.closest("[data-source]");
      if (pick) { state.sourceId = Number(pick.dataset.source); renderPanel(); return; }
      const lang = t.closest("[data-source-lang]");
      if (lang) {
        const id = Number(lang.dataset.sourceLang);
        if (lang.dataset.lang === "orig") state.original.add(id); else state.original.delete(id);
        renderPanel();
        return;
      }
      const tr = t.closest("[data-translate]");
      if (tr) {
        const id = Number(tr.dataset.translate);
        tr.disabled = true;
        try {
          const data = await fetchJson("/news/translate", { sourceId: id });
          const src = state.sources.get(id);
          if (src) src.translation = data.translation;
          state.original.delete(id);
          toast("Traduciendo con IA; queda guardada");
        } catch (err) { toast(`No se pudo traducir: ${err.message || err}`); }
        loadSource(id, true);
        return;
      }
      const ask = t.closest("[data-ask]");
      if (ask) { sendChat(ask.dataset.ask); return; }
      const noteChat = t.closest("[data-note-chat]");
      if (noteChat) {
        try {
          const r = await fetchJson("/news/chat/note", { chatId: Number(noteChat.dataset.noteChat) });
          toast(r.noted ? "Guardada como nota · pestaña Notas" : "Quitada de tus notas");
          state.notes = null;
          if (state.allNotes) state.allNotes.notes = null;
          await loadChat(state.open);
          await refreshNotes(state.open);
          renderPanel();
        } catch (err) { toast(`No se guardó: ${err.message || err}`); }
        return;
      }
      const copyChat = t.closest("[data-copy-chat]");
      if (copyChat) {
        const m = (state.chat.get(state.open) || []).find(x => x.id === Number(copyChat.dataset.copyChat));
        if (m) copyText(m.text);
        return;
      }
      const scope = t.closest("[data-scope]");
      if (scope) { state.scope = scope.dataset.scope; if (state.allNotes) state.allNotes.notes = null; renderPanel(); return; }
      if (t.closest("[data-note-add]")) { addNote(); return; }
      const edit = t.closest("[data-note-edit]");
      if (edit) { state.editing = Number(edit.dataset.noteEdit); state.confirmDelete = null; renderPanel(); panel.querySelector(".nr-note-edit")?.focus(); return; }
      if (t.closest("[data-note-cancel]")) { state.editing = null; state.confirmDelete = null; renderPanel(); return; }
      const saveNote = t.closest("[data-note-save]");
      if (saveNote) {
        const text = panel.querySelector(".nr-note-edit")?.value || "";
        try {
          await fetchJson("/news/notes", { action: "update", noteId: Number(saveNote.dataset.noteSave), text });
          state.editing = null;
          await reloadNotes();
          toast("Nota actualizada");
        } catch (err) { toast(`No se guardó: ${err.message || err}`); }
        return;
      }
      const askDel = t.closest("[data-note-ask-delete]");
      if (askDel) { state.confirmDelete = Number(askDel.dataset.noteAskDelete); renderPanel(); return; }
      const del = t.closest("[data-note-delete]");
      if (del) {
        try {
          await fetchJson("/news/notes", { action: "delete", noteId: Number(del.dataset.noteDelete) });
          state.confirmDelete = null;
          await reloadNotes();
          if (state.open !== "notes") { state.chat.delete(state.open); }
          toast("Nota borrada");
        } catch (err) { toast(`No se borró: ${err.message || err}`); }
        return;
      }
      const copyNote = t.closest("[data-note-copy]");
      if (copyNote) {
        const pool = [...((state.notes && state.notes.notes) || []), ...((state.allNotes && state.allNotes.notes) || [])];
        const n = pool.find(x => x.id === Number(copyNote.dataset.noteCopy));
        if (n) copyText([n.quote, n.text, n.cite].filter(Boolean).join("\n\n"));
        return;
      }
      if (t.closest("[data-notes-export]")) { copyText(notesMarkdown((state.allNotes && state.allNotes.notes) || [])); return; }
      const go = t.closest("[data-goto]");
      if (go) gotoStory(go.dataset.goto, go.dataset.gotoStory, go.dataset.gotoTab);
    });
    panel.addEventListener("submit", e => {
      e.preventDefault();
      const input = panel.querySelector(".nr-chat-input");
      if (input) sendChat(input.value);
    });
    panel.addEventListener("input", e => {
      if (e.target.classList.contains("nr-chat-input")) state.chatDraft = e.target.value;
      if (e.target.classList.contains("nr-note-new")) state.noteDraft = e.target.value;
      if (e.target.classList.contains("nr-notes-q")) {
        state.query = e.target.value;
        clearTimeout(timers.search);
        timers.search = setTimeout(async () => {
          await refreshNotes(state.open === "notes" ? null : state.open);
          renderPanel();
          const q = panel.querySelector(".nr-notes-q");
          if (q) { q.focus(); q.setSelectionRange(q.value.length, q.value.length); }
        }, 250);
      }
    });
    panel.addEventListener("keydown", e => {
      if (e.target.classList.contains("nr-note-new") && e.key === "Enter" && (e.ctrlKey || e.metaKey)) { e.preventDefault(); addNote(); }
    });

    async function addNote() {
      const text = state.noteDraft.trim();
      if (!text || state.open === "notes") return;
      try {
        await fetchJson("/news/notes", { action: "add", storyId: state.open, text });
        state.noteDraft = "";
        await reloadNotes();
        toast("Nota guardada");
      } catch (err) { toast(`No se guardó: ${err.message || err}`); }
    }

    async function reloadNotes() {
      const storyId = state.open === "notes" ? null : state.open;
      if (state.allNotes) state.allNotes.notes = null;
      await refreshNotes(storyId);
      if (storyId && state.scope === "todas") await refreshNotes(null);
      renderPanel();
    }

    $(".nr-stage").addEventListener("scroll", () => {
      if (!state.terminal || !narrow()) return;
      const stage = $(".nr-stage");
      state.mobilePane = stage.scrollLeft > stage.clientWidth / 2 ? "reader" : "terminal";
    }, { passive: true });

    const divider = $(".nr-edition-divider");
    let drag = null;
    divider.addEventListener("pointerdown", e => {
      if (e.button !== 0) return;
      e.preventDefault();
      drag = captureAnchor();
      divider.setPointerCapture(e.pointerId);
      divider.classList.add("dragging");
    });
    divider.addEventListener("pointermove", e => {
      if (!drag && !divider.classList.contains("dragging")) return;
      const box = opts.embedded && el.parentElement
        ? (doc.getElementById("term-area") || el.parentElement).getBoundingClientRect()
        : $(".nr-stage").getBoundingClientRect();
      const right = opts.embedded ? el.getBoundingClientRect().right : box.right;
      const left = opts.embedded ? Math.min(box.left, right - 10) : box.left;
      state.share = clampShare((right - e.clientX) / Math.max(1, right - left) * 100);
      applyPrefs();
    });
    const endDrag = () => {
      if (!divider.classList.contains("dragging")) return;
      divider.classList.remove("dragging");
      writeStore("comandos.news.share", state.share);
      const mark = drag;
      drag = null;
      requestAnimationFrame(() => restoreAnchor(mark));
    };
    divider.addEventListener("pointerup", endDrag);
    divider.addEventListener("pointercancel", endDrag);
    divider.addEventListener("keydown", e => {
      const step = e.key === "ArrowLeft" ? 5 : e.key === "ArrowRight" ? -5 : 0;
      if (!step) return;
      e.preventDefault();
      relayout(() => { state.share = clampShare(state.share + step); writeStore("comandos.news.share", state.share); });
    });

    applyPrefs();
    return { open, close, element: el, setTerminal, state, openStory, closePanel };
  }

  // Wires the header button and ?news=<id> in the dashboard.
  function install(options) {
    const params = new URLSearchParams(location.search);
    const bridge = typeof window !== "undefined" && window.webkit && window.webkit.messageHandlers
      && window.webkit.messageHandlers.centro;
    const post = msg => { try { bridge.postMessage(JSON.stringify(msg)); } catch (e) { /* sin app */ } };
    const desktop = params.get("panel") === "news" && !!bridge;
    // El modo se decide al abrir: la vista "app" (remoto) se activa después de cargar.
    const optsNow = () => {
      const panes = document.getElementById("panes");
      const embedded = !desktop && !!panes && document.body.classList.contains("app");
      return { ...(options || {}), desktop, embedded, parent: embedded ? panes : undefined,
        externalTerminal: on => post({ type: "reader", action: "terminal", on }),
        onClose: () => post({ type: "reader", action: "close" }) };
    };
    // Dentro de la app de escritorio el lector vive junto a la terminal GTK, no en el panel.
    if (bridge && !desktop) {
      const openInApp = i => post({ type: "reader", action: "open", id: i || "" });
      const button = document.getElementById("btn-news");
      if (button) button.addEventListener("click", () => openInApp());
      return { open: openInApp, close: () => post({ type: "reader", action: "close" }), toggle: () => openInApp() };
    }
    let reader = null;
    const get = () => (reader = reader || mount(optsNow()));
    const button = document.getElementById("btn-news");
    const toggle = () => { const r = get(); if (r.element.hidden) r.open(); else r.close(); };
    if (button) button.addEventListener("click", toggle);
    const wanted = params.get("news");
    const id = wanted && /^\d{4}-\d{2}-\d{2}@\d{2}:\d{2}$/.test(wanted) ? wanted : undefined;
    if (desktop || wanted !== null) setTimeout(() => get().open(id), desktop ? 0 : 400);
    return { open: i => get().open(i), close: () => reader && reader.close(), toggle, get reader() { return reader; } };
  }

  const api = { createRenderer, sourceDateLabel, statusLabel, safeHref, clampShare, createStore, mount, install,
    provenance, dayLine, modelName, storyKicker, blocksHtml, inlineText, noteCommand, notesByDay };
  // <script src="/news-reader.js" data-autoinstall> wires the dashboard button.
  if (typeof document !== "undefined" && document.currentScript &&
      document.currentScript.hasAttribute("data-autoinstall")) {
    const start = () => { api.instance = install(); };
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start); else start();
  }
  return api;
});
