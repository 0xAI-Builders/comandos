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

  function tabName(name) { return ALIASES[String(name || '').toLowerCase()] || 'cuentas'; }

  function create(el, opts) {
    const render = opts.render || root.AnalyticsRender;
    const width = opts.width || (() => el.getBoundingClientRect().width);
    const S = { tab: tabName(opts.tab), offset: 0, phoneDay: null, model: null, error: '', loading: null };
    const phone = () => width() < PHONE_PX;

    function paint() {
      el.classList.toggle('phone', phone());
      if (!S.model) {
        el.innerHTML = `<div class="mhead"><h2>Analytics</h2></div><p class="dim">${S.error || 'Leyendo el uso…'}</p>`;
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
      const rows = JSON.parse(decodeURIComponent(btn.dataset.pop));
      pop.innerHTML = `<h6>${btn.dataset.title}</h6>` + rows.map(r => `<div><span class="dot" style="background:${r[3]}"></span><b>${r[1]}</b><span>${r[0]}</span><em>${r[2]}</em></div>`).join('');
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
      tip.innerHTML = `<b>${b}</b><span>${a}</span><em>${c}</em>`;
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
