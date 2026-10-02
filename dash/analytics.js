// Analytics del tablero (grill 1–2 oct): Cuentas · Comparar · Pomodoro.
// El marcado sale de dash/analytics-render.js (copia exacta del mockup aprobado);
// aquí solo van los datos (GET /analytics/week), las pestañas, la semana, el celular y los tooltips.
(function (root) {
  const TABS = ['cuentas', 'comparar', 'pomodoro'];
  // Nombres viejos que aún llegan por openAnalyticsTab (avisos, catálogo del operador, ?tab=).
  const ALIASES = { resumen: 'cuentas', guardia: 'cuentas', alertas: 'cuentas', reparto: 'cuentas',
    proyectos: 'comparar', proveedores: 'comparar', comparar: 'comparar', pomodoro: 'pomodoro', cuentas: 'cuentas' };
  const MIN_OFFSET = -1;
  const PHONE_PX = 600;

  // Los nombres (carpetas, cuentas) llegan decodificados desde data-*: se escapan antes de volver a innerHTML.
  const esc = v => String(v == null ? '' : v).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

  // Inverso exacto de esc (y del escape del dibujo): así un nombre se escapa una sola vez, venga como venga.
  const UNESC = { '&amp;': '&', '&lt;': '<', '&gt;': '>', '&quot;': '"', '&#39;': "'", '&#127813;': '🍅' };
  const unesc = v => String(v == null ? '' : v).replace(/&(?:amp|lt|gt|quot|#39|#127813);/g, m => UNESC[m]);

  function tabName(name) { return ALIASES[String(name || '').toLowerCase()] || 'cuentas'; }

  function create(el, opts) {
    const render = opts.render || root.AnalyticsRender;
    const width = opts.width || (() => el.getBoundingClientRect().width);
    const S = { tab: tabName(opts.tab), offset: 0, phoneDay: null, model: null, error: '', loading: null };
    const phone = () => width() < PHONE_PX;

    function paint() {
      el.classList.toggle('phone', phone());
      if (!S.model) {
        el.innerHTML = `<div class="mhead"><h2>Analytics</h2></div><p class="dim">${esc(S.error || 'Leyendo el uso…')}</p>`;
        return;
      }
      const view = render.create(S.model, { phone: phone(), phoneDay: S.phoneDay, minOffset: MIN_OFFSET });
      el.innerHTML = view.html(S.tab) + '<div class="tip" hidden></div><div class="pop" hidden></div>';
      if (phone()) addScrollDots();
    }

    // Celular: la repisa se desliza de lado; los puntos dicen qué cuenta estás viendo.
    function addScrollDots() {
      el.querySelectorAll('.bar-row').forEach(row => {
        const bar = row.closest('.bar');
        if (!bar || typeof bar.insertAdjacentHTML !== 'function') return;
        const n = row.children.length;
        bar.insertAdjacentHTML('afterend', `<div class="sdots">${Array.from({ length: n }, (_, i) => `<i class="${i ? '' : 'on'}"></i>`).join('')}</div>`);
        const dots = bar.nextElementSibling;
        row.addEventListener('scroll', () => {
          const i = Math.round(row.scrollLeft / row.clientWidth);
          dots.querySelectorAll('i').forEach((d, j) => d.classList.toggle('on', j === i));
        }, { passive: true });
      });
    }

    async function load() {
      const offset = S.offset;
      const job = opts.fetchWeek(offset).then(model => {
        if (offset !== S.offset) return;
        S.model = model; S.error = '';
      }, err => {
        if (offset !== S.offset) return;
        if (!S.model) S.error = `No pude leer el uso: ${err && err.message ? err.message : err}`;
        // Falló otra semana: se queda la que se ve, y las flechas siguen hablando de ella.
        else S.offset = S.model.week.offset;
      });
      S.loading = job;
      await job;
      if (offset === S.offset) paint();
    }

    function open(name) {
      if (name) S.tab = tabName(name);
      paint();
      return load();
    }

    function showPop(btn) {
      const pop = el.querySelector('.pop');
      // data-pop va en URI (el navegador no lo decodifica): sus nombres pueden venir ya escapados del dibujo.
      const rows = JSON.parse(decodeURIComponent(btn.dataset.pop));
      pop.innerHTML = `<h6>${esc(btn.dataset.title)}</h6>` + rows.map(r => `<div><span class="dot" style="background:${esc(r[3])}"></span><b>${esc(unesc(r[1]))}</b><span>${esc(unesc(r[0]))}</span><em>${esc(unesc(r[2]))}</em></div>`).join('');
      pop.hidden = false;
      const rc = btn.getBoundingClientRect();
      pop.style.left = Math.max(8, Math.min(innerWidth - pop.offsetWidth - 10, rc.right + 8)) + 'px';
      pop.style.top = Math.min(innerHeight - pop.offsetHeight - 10, rc.top) + 'px';
    }

    el.addEventListener('click', e => {
      const t = e.target;
      const tab = t.closest('[data-tab]');
      if (tab) { S.tab = tabName(tab.dataset.tab); opts.onTab && opts.onTab(S.tab); paint(); return; }
      const week = t.closest('[data-w]');
      if (week && !week.disabled && !week.hasAttribute('disabled')) {
        S.offset = Math.max(MIN_OFFSET, Math.min(0, S.offset + Number(week.dataset.w)));
        S.phoneDay = null;
        load();
        return;
      }
      const day = t.closest('[data-pd]');
      if (day && S.model) {
        const last = S.model.days.length - 3;
        const cur = S.phoneDay == null ? last : S.phoneDay;
        S.phoneDay = Math.max(0, Math.min(last, cur + Number(day.dataset.pd)));
        paint();
        return;
      }
      const more = t.closest('[data-pop]');
      if (more) { e.stopPropagation(); showPop(more); return; }
      const pop = el.querySelector('.pop');
      if (pop && !t.closest('.pop')) pop.hidden = true;
    });

    el.addEventListener('pointerover', e => {
      const tip = el.querySelector('.tip');
      if (!tip) return;
      const src = e.target.closest('[data-tip]');
      if (!src) { tip.hidden = true; return; }
      const [a, b, c] = src.dataset.tip.split('|');
      tip.innerHTML = `<b>${esc(b)}</b><span>${esc(a)}</span><em>${esc(c)}</em>`;
      tip.hidden = false;
    });
    el.addEventListener('pointermove', e => {
      const tip = el.querySelector('.tip');
      if (!tip || tip.hidden) return;
      const x = Math.min(innerWidth - tip.offsetWidth - 8, e.clientX + 14);
      const y = e.clientY + 18 + tip.offsetHeight > innerHeight ? e.clientY - tip.offsetHeight - 10 : e.clientY + 18;
      tip.style.left = x + 'px';
      tip.style.top = y + 'px';
    });

    return { open, load, paint, state: S };
  }

  const api = { create, tabName, TABS };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.Analytics = api;
})(typeof window !== 'undefined' ? window : globalThis);
