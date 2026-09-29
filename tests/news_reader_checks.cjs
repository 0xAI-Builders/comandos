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

  const stories = [
    { id: 1, category: 'modelo' }, { id: 2, category: 'mcp' }, { id: 3, category: 'skill' },
    { id: 4, category: 'bounty' }, { id: 5, category: 'hackathon' }, { id: 6, category: 'ia' },
  ];
  await check('filters by category and saved state', () => {
    const ids = (f, saved = new Set()) => NR.filterStories(stories, f, saved, 'E').map(s => s.id);
    assert.deepEqual(ids('Todo'), [1, 2, 3, 4, 5, 6]);
    assert.deepEqual(ids('Modelos'), [1]);
    assert.deepEqual(ids('MCPs'), [2]);
    assert.deepEqual(ids('Skills'), [3]);
    assert.deepEqual(ids('Bounties'), [4, 5]);
    assert.deepEqual(ids('IA'), [6]);
    assert.deepEqual(ids('Guardados', new Set([NR.storyKey('E', stories[2])])), [3]);
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
    assert.equal(NR.statusLabel('not_published'), 'No publicada');
    assert.equal(NR.statusLabel('partial'), 'Parcial');
    assert.equal(NR.statusLabel('empty'), 'Sin novedades');
    assert.equal(NR.statusLabel('failed'), 'Falló');
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

  console.log(`${passed} news reader checks passed`);
})().catch(err => { console.error(err); process.exit(1); });
