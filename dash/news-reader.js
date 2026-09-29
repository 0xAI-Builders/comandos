// Lector de novedades (N5): edición continua B con terminal opcional a la
// izquierda. Solo LEE ediciones ya publicadas (/news/editions, /news/edition);
// abrir, filtrar o releer nunca llama al modelo ni al buscador.
//
// Markdown: markdown-it (vendor, HTML crudo desactivado, enlaces solo
// http/https, imágenes remotas desactivadas) + DOMPurify como segunda capa.
// Sin DOMPurify el lector muestra texto plano escapado.
(function (root, factory) {
  if (typeof module === "object" && module.exports) module.exports = factory();
  else root.NewsReader = factory();
})(typeof self !== "undefined" ? self : this, function () {
  "use strict";

  const FILTERS = ["Todo", "Modelos", "MCPs", "Skills", "Bounties", "IA", "Guardados"];
  const FILTER_CATEGORIES = { Modelos: ["modelo"], MCPs: ["mcp"], Skills: ["skill"],
    Bounties: ["bounty", "hackathon"], IA: ["ia"] };
  const CATEGORY_LABEL = { modelo: "Modelos", mcp: "MCP", skill: "Skill", bounty: "Bounty",
    hackathon: "Hackathon", ia: "IA" };
  const STATUS_LABEL = { published: "Publicada", partial: "Parcial", empty: "Sin novedades",
    failed: "Falló", not_published: "No publicada", running: "Generando…", scheduled: "Programada" };
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

  function filterStories(stories, filter, saved, editionId) {
    const list = Array.isArray(stories) ? stories : [];
    if (filter === "Guardados") return list.filter(s => saved.has(storyKey(editionId, s)));
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
    <button type="button" class="nr-close" aria-label="Cerrar novedades">←<span class="nr-close-label"> Volver</span></button>
    <strong class="nr-title">Novedades</strong>
    <select class="nr-edition-pick" aria-label="Elegir edición"></select>
    <div class="nr-mobile-switch" role="group" aria-label="Vista">
      <button type="button" data-pane="terminal">Terminal</button>
      <button type="button" data-pane="reader">Novedades</button>
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
    <section class="nr-reader" aria-label="Edición">
      <nav class="nr-filters" aria-label="Filtrar novedades"></nav>
      <div class="nr-reader-body" tabindex="-1"><div class="nr-edition"></div></div>
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
    el.className = "nr-app nr-B";
    el.setAttribute("role", "dialog");
    el.setAttribute("aria-modal", "true");
    el.setAttribute("aria-label", "Novedades");
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
    };

    function applyPrefs() {
      el.style.setProperty("--read-size", state.size + "px");
      el.style.setProperty("--nr-share", state.share + "%");
      $(".nr-edition-divider").setAttribute("aria-valuenow", String(Math.round(state.share)));
    }

    function renderFilters() {
      const saved = state.saved;
      $(".nr-filters").innerHTML = FILTERS.map(f => {
        const count = state.current ? filterStories(state.current.stories, f, saved, state.current.edition.id).length : 0;
        return `<button type="button" data-filter="${esc(f)}" class="${state.filter === f ? "on" : ""}" aria-pressed="${state.filter === f}">${esc(f)}<i>${count}</i></button>`;
      }).join("");
    }

    function renderPicker() {
      const list = (state.list && state.list.editions) || [];
      const pick = $(".nr-edition-pick");
      pick.innerHTML = list.map(e => `<option value="${esc(e.id)}">${esc(e.localDate)} ${esc(e.slot)} · ${esc(statusLabel(e.status))}</option>`).join("");
      if (state.current) pick.value = state.current.edition.id;
      pick.hidden = !list.length;
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
      return `<section class="nr-edition-story" id="nr-story-${esc(story.id)}" data-story="${esc(story.id)}">
        <small class="nr-kicker">${esc(CATEGORY_LABEL[story.category] || story.category)}</small>
        <h2>${esc(story.title)}</h2>
        <div class="nr-markdown nr-summary">${render(story.summary)}</div>
        ${opportunityHtml(story.opportunity)}
        <details class="nr-more"><summary>Leer más</summary><div class="nr-markdown">${render(story.body)}</div></details>
        <details class="nr-sources"><summary>Fuentes (${(story.sources || []).length})</summary><ul>${sources}</ul></details>
        <button type="button" class="nr-save" data-save="${esc(key)}" aria-pressed="${isSaved}">${isSaved ? "Guardada" : "Guardar"}</button>
      </section>`;
    }

    function renderEdition() {
      const data = state.current;
      if (!data) return;
      const edition = data.edition;
      const tz = edition.timezone || "America/Mexico_City";
      const stories = filterStories(data.stories, state.filter, state.saved, edition.id);
      const notes = (edition.notes || []).map(n => `<li>${esc(n)}</li>`).join("");
      const readable = edition.status === "published" || edition.status === "partial";
      const head = `<header class="nr-edition-head">
        <small>Edición ${esc(edition.slot)} · ${esc(edition.localDate)} · ${esc(statusLabel(edition.status))}</small>
        <h1>${esc(edition.title || "Tu edición de IA")}</h1>
        <p>${esc(edition.storyCount)} historias · ${esc(edition.sourceCount)} fuentes${edition.failedSourceCount ? ` · ${esc(edition.failedSourceCount)} sin leer` : ""}${edition.publishedAt ? ` · publicada ${esc(formatDate(edition.publishedAt, tz))}` : ""}</p>
        ${notes ? `<details class="nr-notes" ${readable ? "" : "open"}><summary>Notas de la edición</summary><ul>${notes}</ul></details>` : ""}
      </header>`;
      const index = stories.length > 1 ? `<nav class="nr-edition-index" aria-label="Índice de la edición">${stories.map(s =>
        `<button type="button" data-jump="${esc(s.id)}">${esc(s.title)}</button>`).join("")}</nav>` : "";
      const body = stories.length ? stories.map(s => storyHtml(edition, s, tz)).join("")
        : `<p class="nr-empty">${readable ? "No hay historias en este filtro." : "Esta edición no tiene historias publicadas."}</p>`;
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
      const stage = $(".nr-stage");
      if (state.terminal && narrow()) stage.scrollTo({ left: pane === "reader" ? stage.scrollWidth : 0, behavior: "smooth" });
      el.querySelectorAll(".nr-mobile-switch button").forEach(b => b.setAttribute("aria-pressed", String(b.dataset.pane === pane)));
    }

    async function setTerminal(on) {
      relayout(() => {
        state.terminal = on;
        el.classList.toggle("nr-with-terminal", on);
        $(".nr-terminal").hidden = !on;          // hidden, never removed: the session is not restarted
        $(".nr-edition-divider").hidden = !on;
        const t = $(".nr-terminal-toggle");
        t.setAttribute("aria-pressed", String(on));
        t.textContent = on ? "Ocultar terminal" : "Ver terminal";
        t.classList.toggle("on", on);
      });
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
        message(`No se pudo abrir la edición: ${esc(err.message || err)}`);
      }
    }

    async function open(editionId) {
      state.opener = doc.activeElement;
      el.hidden = false;
      doc.documentElement.classList.add("nr-open");
      applyPrefs();
      $(".nr-reader-body").focus({ preventScroll: true });
      if (state.current && !editionId) return;          // reopen: keep position, no fetch
      if (!state.current) message("Cargando novedades…");
      try {
        state.list = await store.list();
      } catch (err) {
        message(`No se pudo leer la lista de ediciones: ${esc(err.message || err)}. Revisa la conexión con CommandOS.`);
        return;
      }
      renderPicker();
      const target = editionId || state.list.latest;
      if (!target) {
        message(state.list.configured
          ? "Aún no hay ediciones publicadas. La próxima llega a su hora programada."
          : `Novedades ${esc(state.list.reason || "sin configurar")}. No se muestran noticias inventadas; configura <code>news-editions.json</code> para activar las tres ediciones diarias.`);
        return;
      }
      await loadEdition(target);
    }

    function close() {
      el.hidden = true;
      doc.documentElement.classList.remove("nr-open");
      if (state.opener && state.opener.focus) state.opener.focus({ preventScroll: true });
    }

    // Events -----------------------------------------------------------
    $(".nr-close").addEventListener("click", close);
    el.addEventListener("keydown", e => { if (e.key === "Escape" && !e.target.closest(".nr-terminal")) close(); });
    $(".nr-terminal-toggle").addEventListener("click", () => setTerminal(!state.terminal));
    $(".nr-edition-pick").addEventListener("change", e => loadEdition(e.target.value));
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
      const box = $(".nr-stage").getBoundingClientRect();
      state.share = clampShare((box.right - e.clientX) / box.width * 100);
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
    let reader = null;
    const get = () => (reader = reader || mount(options));
    const button = document.getElementById("btn-news");
    if (button) button.addEventListener("click", () => get().open());
    const wanted = new URLSearchParams(location.search).get("news");
    if (wanted !== null) get().open(/^\d{4}-\d{2}-\d{2}@\d{2}:\d{2}$/.test(wanted) ? wanted : undefined);
    return { open: id => get().open(id) };
  }

  const api = { createRenderer, filterStories, storyKey, sourceDateLabel, statusLabel, safeHref,
    clampShare, createStore, mount, install, FILTERS };
  // <script src="/news-reader.js" data-autoinstall> wires the dashboard button.
  if (typeof document !== "undefined" && document.currentScript &&
      document.currentScript.hasAttribute("data-autoinstall")) {
    const start = () => { api.instance = install(); };
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start); else start();
  }
  return api;
});
