// N5 reader contract, without a browser: Markdown safety, filters, dates and
// the read-only store. DOM behaviour is verified in Chrome remoto.
const assert = require('node:assert/strict');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const markdownit = require(path.join(root, 'dash/vendor/markdown-it-15.0.2.umd.min.js'));
const NR = require(path.join(root, 'dash/news-reader.js'));

let passed = 0;
async function check(name, fn) {
  await fn();
  passed++;
  console.log('ok -', name);
}

// Node has no DOM for DOMPurify; the identity purifier records its config so we
// can assert what the browser applies on top of markdown-it.
const seen = [];
const purify = { sanitize(html, cfg) { seen.push(cfg); return html; } };
const render = NR.createRenderer(markdownit, purify);

(async () => {
  await check('raw HTML is escaped, never parsed', () => {
    const out = render('<script>alert(1)</script><img src=x onerror=alert(1)>');
    assert.ok(!/<script|<img/i.test(out), out);
    assert.ok(out.includes('&lt;script&gt;'));
  });

  await check('javascript:, data: and vbscript: links are not linked', () => {
    for (const bad of ['javascript:alert(1)', 'JaVaScRiPt:alert(1)', 'data:text/html,<b>x</b>', 'vbscript:x', '//evil.example/x', '/local/path']) {
      const out = render(`[clic](${bad})`);
      assert.ok(!/<a\s/i.test(out), bad + ' -> ' + out);
    }
  });

  await check('http/https links open safely in a new tab', () => {
    const out = render('[docs](https://example.com/a) y https://example.org');
    const anchors = out.match(/<a [^>]+>/g) || [];
    assert.equal(anchors.length, 2);
    for (const a of anchors) {
      assert.match(a, /target="_blank"/);
      assert.match(a, /rel="noopener noreferrer nofollow"/);
    }
  });

  await check('a misleading link shows its real host', () => {
    const out = render('[google.com](https://evil.example/login)');
    assert.match(out, /evil\.example/);
    assert.match(out, /class="nr-host"/);
    const honest = render('[example.com](https://example.com/x)');
    assert.ok(!/nr-host/.test(honest), honest);
  });

  await check('wide tables are wrapped for horizontal scroll', () => {
    const out = render('| a | b |\n|---|---|\n| 1 | 2 |');
    assert.match(out, /<div class="nr-table-wrap"><table>/);
  });

  await check('code is text', () => {
    const out = render('```\n<b onclick="x()">x</b>\n```\n\n`<i>y</i>`');
    assert.ok(!/<b |<i>/.test(out), out);
    assert.match(out, /&lt;b onclick/);
  });

  await check('remote images are not loaded', () => {
    const out = render('![pixel](https://tracker.example/p.png)');
    assert.ok(!/<img/i.test(out), out);
  });

  await check('the purifier restricts URIs and tags', () => {
    const cfg = seen[seen.length - 1];
    assert.ok(cfg.ALLOWED_URI_REGEXP.test('https://x.example'));
    assert.ok(!cfg.ALLOWED_URI_REGEXP.test('javascript:alert(1)'));
    assert.ok(!cfg.ALLOWED_TAGS.includes('img') && !cfg.ALLOWED_TAGS.includes('script'));
    assert.ok(!cfg.ALLOWED_ATTR.some(a => /^on/i.test(a) || a === 'style' || a === 'src'));
  });

  await check('without a purifier the reader shows plain escaped text', () => {
    const plain = NR.createRenderer(markdownit, null);
    const out = plain('**hola** <b>x</b>');
    assert.ok(!/<strong>|<b>/.test(out), out);
    assert.match(out, /\*\*hola\*\* &lt;b&gt;x&lt;\/b&gt;/);
  });

  await check('discovery is never shown as publication', () => {
    const tz = 'America/Mexico_City';
    const unknown = NR.sourceDateLabel({ publishedAt: null, discoveredAt: Date.UTC(2026, 8, 29, 14) }, tz);
    assert.match(unknown, /descubierta/);
    assert.match(unknown, /publicación desconocida/);
    const known = NR.sourceDateLabel({ publishedAt: Date.UTC(2026, 8, 28, 20), discoveredAt: Date.UTC(2026, 8, 29, 14) }, tz);
    assert.match(known, /^publicada/);
  });

  await check('status labels are honest', () => {
    assert.equal(NR.statusLabel('not_published'), 'No se generó');
    assert.equal(NR.statusLabel('partial'), 'Parcial');
    assert.equal(NR.statusLabel('empty'), 'Sin novedades');
    assert.equal(NR.statusLabel('failed'), 'Falló');
    assert.equal(NR.statusLabel('scheduled'), 'Programado');
  });

  await check('source links are limited to http/https', () => {
    assert.equal(NR.safeHref('https://a.example/x'), 'https://a.example/x');
    assert.equal(NR.safeHref('javascript:alert(1)'), null);
    assert.equal(NR.safeHref('https://user:pw@a.example/'), null);
  });

  await check('the store only reads editions and caches them', async () => {
    const calls = [];
    const fetchJson = async (url) => {
      calls.push(url);
      if (url === '/news/editions') return { configured: true, latest: '2026-09-29@09:00', editions: [{ id: '2026-09-29@09:00', status: 'published' }] };
      return { edition: { id: '2026-09-29@09:00', status: 'published' }, stories: [] };
    };
    const store = NR.createStore(fetchJson);
    await store.list();
    await store.edition('2026-09-29@09:00');
    await store.edition('2026-09-29@09:00');
    await store.edition('2026-09-29@09:00');
    assert.deepEqual(calls, ['/news/editions', '/news/edition?id=2026-09-29%4009%3A00']);
    assert.ok(calls.every(u => u.startsWith('/news/edition')), 'no generation endpoint is ever called');
  });

  await check('reader share stays within 40-70 percent', () => {
    assert.equal(NR.clampShare(10), 40);
    assert.equal(NR.clampShare(90), 70);
    assert.equal(NR.clampShare(55), 55);
  });

  const ED = { id: '2026-09-29@20:42', slot: '20:42', localDate: '2026-09-29', status: 'partial', storyCount: 23,
    sourceCount: 25, failedSourceCount: 2, costUsd: 0,
    models: { 'opencode:opencode/longcat-2.5-preview-free': 20, 'claude:claude-opus-5-5': 3 },
    job: { startedAt: Date.UTC(2026, 8, 30, 2, 42), finishedAt: Date.UTC(2026, 8, 30, 2, 56), attempts: 1 },
    notes: ['3 fuente(s) no respondieron: dorahacks, reddit, searxng.'] };

  await check('provenance says which AI wrote the summary, what it cost and how long it took', () => {
    const p = NR.provenance(ED);
    assert.equal(p.models, 'longcat-2.5-preview-free (20) · claude-opus-5-5 (3)');
    assert.equal(p.cost, '$0.00');
    assert.equal(p.duration, '14 min');
    assert.equal(p.sources, '23 de 25 fuentes');
    assert.equal(p.problems, 1);
  });

  await check('the day line shows every summary of the day plus the next one', () => {
    const list = { editions: [ED, { id: '2026-09-29@21:00', slot: '21:00', localDate: '2026-09-29', status: 'not_published', notes: ['x'] },
      { id: '2026-09-28@21:00', slot: '21:00', localDate: '2026-09-28', status: 'published', storyCount: 9 }],
      next: { id: '2026-09-30@09:00', slot: '09:00', localDate: '2026-09-30', status: 'scheduled' } };
    const cards = NR.dayLine(list, '2026-09-29');
    assert.deepEqual(cards.map(c => c.id), ['2026-09-29@20:42', '2026-09-29@21:00', '2026-09-30@09:00']);
    assert.equal(cards[2].day, 'Mañana');
    assert.equal(cards[0].day, 'Hoy');
  });

  await check('the kicker says where a story comes from; old editions keep their category', () => {
    assert.deepEqual(NR.storyKicker({ category: 'oficial', meta: { lab: 'Google DeepMind' } }), { label: 'Oficial · Google DeepMind', hot: false });
    assert.deepEqual(NR.storyKicker({ category: 'hot', meta: { lab: 'Comunidad' } }), { label: 'Hot · comunidad', hot: true });
    assert.equal(NR.storyKicker({ category: 'mcp' }).label, 'MCP');
    assert.equal(NR.storyKicker({ category: 'bounty' }).hot, true);
  });

  await check('captured sources render as escaped blocks; only local media names become images', () => {
    const out = NR.blocksHtml([
      { type: 'h', text: 'Qué <b>cambia</b>' },
      { type: 'p', text: 'Corre `claude update` y <script>alert(1)</script>' },
      { type: 'li', text: 'uno' }, { type: 'li', text: 'dos' },
      { type: 'quote', text: 'autor: "hola"' },
      { type: 'code', text: '<i onclick="x()">x</i>\nlínea' },
      { type: 'img', media: '0123456789abcdef0123456789abcdef.png', alt: 'Diagrama "x"' },
      { type: 'img', media: '../../etc/passwd', alt: 'malo' },
      { type: 'img', media: 'https://tracker.example/p.png' },
    ]);
    assert.ok(!/<script|<b>|<i onclick/.test(out), out);
    assert.match(out, /<code>claude update<\/code>/);
    assert.match(out, /<ul><li>uno<\/li><li>dos<\/li><\/ul>/);
    assert.match(out, /<img data-media="0123456789abcdef0123456789abcdef.png" alt="Diagrama &quot;x&quot;"/);
    assert.equal((out.match(/<img /g) || []).length, 1, 'only the valid local media is an image');
    assert.ok(!/src=/.test(out), 'images get a blob: URL later, never a remote src');
  });

  await check('"/nota" in the chat saves a note instead of asking', () => {
    assert.equal(NR.noteCommand('/nota probar workspaces mañana'), 'probar workspaces mañana');
    assert.equal(NR.noteCommand('  /NOTA  dos\nlíneas'), 'dos\nlíneas');
    assert.equal(NR.noteCommand('¿qué es /nota?'), null);
    assert.equal(NR.noteCommand('/nota'), null);
  });

  await check('all notes are grouped by local day: Hoy, Ayer, then the date', () => {
    const now = Date.UTC(2026, 9, 1, 18);          // 1 oct 12:00 CDMX
    const groups = NR.notesByDay([
      { id: 1, createdAt: Date.UTC(2026, 9, 1, 16) },
      { id: 2, createdAt: Date.UTC(2026, 9, 1, 5) },   // 30 sep 23:00 CDMX
      { id: 3, createdAt: Date.UTC(2026, 8, 28, 20) },
    ], now, 'America/Mexico_City');
    assert.deepEqual(groups.map(g => [g.label, g.notes.map(n => n.id)]), [['Hoy', [1]], ['Ayer', [2]], ['28 de septiembre', [3]]]);
  });

  await check('the store can re-read an edition for fresh counts', async () => {
    const calls = [];
    const store = NR.createStore(async url => { calls.push(url); return { edition: { id: 'E', status: 'published' }, stories: [] }; });
    await store.edition('E');
    await store.edition('E');
    await store.edition('E', true);
    assert.equal(calls.length, 2);
  });

  console.log(`${passed} news reader checks passed`);
})().catch(err => { console.error(err); process.exit(1); });
