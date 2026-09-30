// Lector de Resúmenes (N5): resumen continuo B con terminal opcional a la
// izquierda y línea del día arriba. Solo LEE resúmenes ya generados
// (/news/editions, /news/edition); abrir, filtrar o releer nunca llama al
// modelo ni al buscador. Remoto: vive en el área de terminales (la barra
// izquierda sigue visible). Escritorio: la app GTK lo abre junto a su VTE.
//
// Markdown: markdown-it (vendor, HTML crudo desactivado, enlaces solo
// http/https, imágenes remotas desactivadas) + DOMPurify como segunda capa.
// Sin DOMPurify el lector muestra texto plano escapado.
(function (root, factory) {
  if (typeof module === "object" && module.exports) module.exports = factory();
  else root.NewsReader = factory();
})(typeof self !== "undefined" ? self : this, function () {
  "use strict";

  const FILTERS = ["Todo", "Modelos", "MCPs", "Skills", "Bounties", "IA", "Guardados", "Siguiendo"];
  const TOPICS = ["Modelos", "MCPs", "Skills", "Bounties", "IA"];
  const FILTER_CATEGORIES = { Modelos: ["modelo"], MCPs: ["mcp"], Skills: ["skill"],
    Bounties: ["bounty", "hackathon"], IA: ["ia"] };
  const CATEGORY_LABEL = { modelo: "Modelos", mcp: "MCP", skill: "Skill", bounty: "Bounty",
    hackathon: "Hackathon", ia: "IA" };
  const STATUS_LABEL = { published: "Listo", partial: "Parcial", empty: "Sin novedades",
    failed: "Falló", not_published: "No se generó", running: "Generando…", scheduled: "Programado" };
  const TERMINAL_STATUS = new Set(["published", "partial", "empty", "failed", "not_published"]);
  const HTTP_URL = /^https?:\/\//i;
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

  function storyKey(editionId, story) { return `${editionId}#${story.id}`; }

  function topicOf(story) {
    return TOPICS.find(t => (FILTER_CATEGORIES[t] || []).includes(story && story.category)) || "IA";
  }

  function filterStories(stories, filter, saved, editionId, following) {
    const list = Array.isArray(stories) ? stories : [];
    if (filter === "Guardados") return list.filter(s => saved.has(storyKey(editionId, s)));
    if (filter === "Siguiendo") return list.filter(s => following && following.has(topicOf(s)));
    const cats = FILTER_CATEGORIES[filter];
    return cats ? list.filter(s => cats.includes(s.category)) : list.slice();
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

  /** Índice por temas: pocas chips con su conteo, en vez de una por historia. */
  function topicIndex(stories) {
    const out = [];
    for (const t of TOPICS) {
      const of = (stories || []).filter(s => topicOf(s) === t);
      if (of.length) out.push({ label: t, count: of.length, first: of[0].id });
    }
    return out;
  }

  function clampShare(value) {
    const n = Number(value);
    return Math.min(70, Math.max(40, Number.isFinite(n) ? n : 58));
  }

  function createStore(fetchJson) {
    const cache = new Map();
    return {
      list: () => fetchJson("/news/editions"),
      async edition(id) {
        if (cache.has(id)) return cache.get(id);
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

  function defaultFetchJson(url) {
    const headers = {};
    try { const t = localStorage.getItem("cc_token"); if (t) headers["X-Comandos-Token"] = t; } catch (e) { /* none */ }
    return fetch(url, { headers }).then(async r => {
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
      ? `${base}/?auth=${encodeURIComponent(token)}&arg=${encodeURIComponent(session)}&theme=${encodeURIComponent(theme)}`
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
      <nav class="nr-filters" aria-label="Filtrar noticias"></nav>
      <div class="nr-reader-body" tabindex="-1"><nav class="nr-day" aria-label="Resúmenes del día"></nav><div class="nr-edition"></div></div>
    </section>
  </div>`;

  function mount(options) {
    const opts = options || {};
    const doc = opts.document || document;
    const render = createRenderer(opts.markdownit || (typeof markdownit !== "undefined" ? markdownit : null),
      opts.purify === undefined ? (typeof DOMPurify !== "undefined" ? DOMPurify : null) : opts.purify);
    const store = createStore(opts.fetchJson || defaultFetchJson);
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
      list: null, current: null, filter: "Todo", terminal: false, terminalMounted: false,
      mobilePane: "reader", opener: null,
      size: Math.min(22, Math.max(14, Number(readStore("comandos.news.size", 16)) || 16)),
      share: clampShare(readStore("comandos.news.share", 58)),
      saved: new Set(readStore("comandos.news.saved", [])),
      following: new Set(readStore("comandos.news.following", [])),
      details: false,
    };
    const docEl = doc.documentElement, body = doc.body;

    function applyPrefs() {
      el.style.setProperty("--read-size", state.size + "px");
      el.style.setProperty("--nr-share", state.share + "%");
      if (opts.embedded) docEl.style.setProperty("--nr-share", state.share + "%");
      $(".nr-edition-divider").setAttribute("aria-valuenow", String(Math.round(state.share)));
    }

    function renderFilters() {
      const saved = state.saved;
      $(".nr-filters").innerHTML = FILTERS.map(f => {
        const count = state.current ? filterStories(state.current.stories, f, saved, state.current.edition.id, state.following).length : 0;
        const cls = [state.filter === f ? "on" : "", count ? "" : "zero"].join(" ").trim();
        return `<button type="button" data-filter="${esc(f)}" class="${cls}" aria-pressed="${state.filter === f}">${esc(f)}<i>${count}</i></button>`;
      }).join("");
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
        return `<button type="button" class="nr-slot${c.id === cur ? " on" : ""}" data-edition="${esc(c.id)}" ${c.status === "scheduled" ? "disabled" : ""} aria-pressed="${c.id === cur}">`
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

    function storyHtml(edition, story, tz) {
      const key = storyKey(edition.id, story);
      const isSaved = state.saved.has(key);
      const sources = (story.sources || []).map(s => {
        const href = safeHref(s.url);
        const link = href ? `<a href="${esc(href)}" target="_blank" rel="noopener noreferrer nofollow">${esc(s.title)}</a>` : esc(s.title);
        return `<li>${link}<small>${esc(s.origin)} · ${esc(hostOf(s.url))} · ${esc(sourceDateLabel(s, tz))}</small></li>`;
      }).join("");
      const topic = topicOf(story);
      const follows = state.following.has(topic);
      return `<section class="nr-edition-story" id="nr-story-${esc(story.id)}" data-story="${esc(story.id)}" data-topic="${esc(topic)}">
        <small class="nr-kicker">${esc(CATEGORY_LABEL[story.category] || story.category)}
          <span class="nr-ai" title="Texto escrito por IA a partir de las fuentes">Resumen IA${story.model ? " · " + esc(modelName(story.model)) : ""}</span>
          <button type="button" class="nr-follow" data-follow="${esc(topic)}" aria-pressed="${follows}">${follows ? "Siguiendo " + esc(topic) : "Seguir " + esc(topic)}</button></small>
        <h2>${esc(story.title)}</h2>
        <div class="nr-markdown nr-summary">${render(story.summary)}</div>
        ${opportunityHtml(story.opportunity)}
        <details class="nr-more"><summary>Leer más</summary><div class="nr-markdown">${render(story.body)}</div></details>
        <details class="nr-sources"><summary>Fuentes (${(story.sources || []).length})</summary><ul>${sources}</ul></details>
        <button type="button" class="nr-save" data-save="${esc(key)}" aria-pressed="${isSaved}">${isSaved ? "Guardada" : "Guardar"}</button>
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
      const data = state.current;
      if (!data) return;
      const edition = data.edition;
      const tz = edition.timezone || "America/Mexico_City";
      const stories = filterStories(data.stories, state.filter, state.saved, edition.id, state.following);
      const readable = edition.status === "published" || edition.status === "partial";
      const p = provenance(edition);
      const line = readable
        ? `<p class="nr-prov"><span>Resumido con <b>${esc(p.models || "IA")}</b></span><span class="nr-ok">${esc(p.cost)}</span>`
          + `<span>${esc(edition.storyCount)} noticias</span>${p.sources ? `<span>${esc(p.sources)}</span>` : ""}${p.duration ? `<span>${esc(p.duration)}</span>` : ""}`
          + `<button type="button" class="nr-details-toggle" aria-expanded="${state.details}">${p.problems ? `${p.problems} aviso${p.problems === 1 ? "" : "s"} · ` : ""}${state.details ? "Ocultar detalles" : "Detalles del resumen"}</button></p>`
        : `<p class="nr-prov nr-bad">${esc(statusLabel(edition.status))}: ${esc((edition.notes || [])[0] || "sin noticias publicadas")}</p>`;
      const head = `<header class="nr-edition-head">
        <small>${esc(edition.localDate)} · ${esc(statusLabel(edition.status))}</small>
        <h1>Resumen de las ${esc(edition.slot)}</h1>
        ${line}${state.details || !readable ? detailsHtml(edition, tz) : ""}
      </header>`;
      const topics = topicIndex(stories);
      const index = topics.length > 1 ? `<nav class="nr-edition-index" aria-label="Temas del resumen">${topics.map((t, i) =>
        `<button type="button" data-jump="${esc(t.first)}">${i + 1}. ${esc(t.label)} <i>${t.count}</i></button>`).join("")}</nav>` : "";
      const body = stories.length ? stories.map(s => storyHtml(edition, s, tz)).join("")
        : `<p class="nr-empty">${readable ? (state.filter === "Siguiendo" ? "Aún no sigues ningún tema: toca «Seguir» en una noticia." : "No hay noticias en este filtro.") : "Este resumen no tiene noticias."}</p>`;
      $(".nr-edition").innerHTML = head + index + body;
      renderFilters();
    }

    // Keep the reading position when the layout changes (terminal, font, pane).
    function captureAnchor() {
      const body = $(".nr-reader-body");
      const top = body.getBoundingClientRect().top;
      const nodes = body.querySelectorAll(".nr-edition-head, .nr-edition-story");
      let anchor = null;
      for (const n of nodes) { if (n.getBoundingClientRect().top <= top + 12) anchor = n; }
      return anchor ? { node: anchor, offset: anchor.getBoundingClientRect().top - top } : null;
    }
    function restoreAnchor(mark) {
      if (!mark || !mark.node.isConnected) return;
      const body = $(".nr-reader-body");
      body.scrollTop += (mark.node.getBoundingClientRect().top - body.getBoundingClientRect().top) - mark.offset;
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
        const data = await store.edition(id);
        state.current = data;
        state.filter = state.filter === "Guardados" ? "Guardados" : state.filter;
        renderPicker();
        renderEdition();
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
      doc.documentElement.classList.remove("nr-open");
      body.classList.remove("nr-reading", "nr-docked", "nr-peek");
      if (opts.onOpenChange) opts.onOpenChange(false);
      if (state.opener && state.opener.focus) state.opener.focus({ preventScroll: true });
    }

    // Events -----------------------------------------------------------
    $(".nr-close").addEventListener("click", close);
    el.addEventListener("keydown", e => { if (e.key === "Escape" && !e.target.closest(".nr-terminal")) close(); });
    $(".nr-terminal-toggle").addEventListener("click", () => setTerminal(!state.terminal));
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
    $(".nr-filters").addEventListener("click", e => {
      const b = e.target.closest("[data-filter]");
      if (!b) return;
      state.filter = b.dataset.filter;
      renderEdition();
      $(".nr-reader-body").scrollTop = 0;
    });
    $(".nr-edition").addEventListener("click", e => {
      const jump = e.target.closest("[data-jump]");
      if (jump) {
        const target = el.querySelector(`#nr-story-${CSS.escape(jump.dataset.jump)}`);
        if (target) target.scrollIntoView({ block: "start", behavior: "smooth" });
        return;
      }
      if (e.target.closest(".nr-details-toggle")) { state.details = !state.details; renderEdition(); return; }
      const follow = e.target.closest("[data-follow]");
      if (follow) {
        const t = follow.dataset.follow;
        if (state.following.has(t)) state.following.delete(t); else state.following.add(t);
        writeStore("comandos.news.following", [...state.following]);
        renderEdition();
        return;
      }
      const save = e.target.closest("[data-save]");
      if (save) {                                   // saving never creates a reminder
        const key = save.dataset.save;
        if (state.saved.has(key)) state.saved.delete(key); else state.saved.add(key);
        writeStore("comandos.news.saved", [...state.saved].slice(-500));
        const on = state.saved.has(key);
        save.setAttribute("aria-pressed", String(on));
        save.textContent = on ? "Guardada" : "Guardar";
        renderFilters();
      }
    });
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
    return { open, close, element: el, setTerminal, state };
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

  const api = { createRenderer, filterStories, storyKey, sourceDateLabel, statusLabel, safeHref,
    clampShare, createStore, mount, install, FILTERS, provenance, dayLine, topicIndex, topicOf, modelName };
  // <script src="/news-reader.js" data-autoinstall> wires the dashboard button.
  if (typeof document !== "undefined" && document.currentScript &&
      document.currentScript.hasAttribute("data-autoinstall")) {
    const start = () => { api.instance = install(); };
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start); else start();
  }
  return api;
});
