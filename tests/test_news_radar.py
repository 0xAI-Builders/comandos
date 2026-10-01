"""Radar de los Resúmenes: recolectar, juntar lo que habla de lo mismo,
puntuar lo oficial y lo caliente, y capturar páginas completas con imágenes
locales. Ningún test toca la red: `fetch` es un doble."""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "lib"))

import news_radar as r  # noqa: E402

NOW = 1790800000
H = 3600


def it(title, url, family="hn", official=False, heat=10.0, published=NOW - 2 * H, **kw):
    return r._item(family=family, origin=kw.pop("origin", family), title=title, url=url, published=published,
                   official=official, heat=heat, **kw)


class Fetch:
    """URL → (ok, body, content_type) fija; registra las llamadas."""

    def __init__(self, pages):
        self.pages, self.calls = pages, []

    def __call__(self, url, **kw):
        self.calls.append(url)
        if url not in self.pages:
            return False, b"", "", url, "HTTP 404"
        body, kind = self.pages[url]
        return True, body if isinstance(body, bytes) else body.encode(), kind, url, None


def test_parse_feed_reads_rss_and_atom():
    rss = """<?xml version="1.0"?><rss><channel><item><title>Gemini 4 &amp; more</title>
      <link>https://blog.example/g4</link><pubDate>Wed, 30 Sep 2026 16:00:00 +0000</pubDate>
      <description>&lt;p&gt;Nuevo modelo&lt;/p&gt;</description></item></channel></rss>"""
    atom = """<feed xmlns="http://www.w3.org/2005/Atom"><entry><title>Codex 0.160.0</title>
      <link rel="alternate" href="https://github.com/openai/codex/releases/tag/v0.160.0"/>
      <updated>2026-09-30T10:00:00Z</updated><content type="html">&lt;b&gt;notas&lt;/b&gt;</content></entry></feed>"""
    (a,), (b,) = r.parse_feed(rss), r.parse_feed(atom)
    assert a["title"] == "Gemini 4 & more" and a["url"] == "https://blog.example/g4" and a["summary"] == "Nuevo modelo"
    assert a["publishedAt"] == 1790784000
    assert b["url"].endswith("v0.160.0") and b["publishedAt"] and "notas" in b["summary"]


def test_anthropic_news_page_json_becomes_official_items():
    page = ('...\\"publishedOn\\":\\"2026-10-01T15:19:00.000Z\\",\\"slug\\":{\\"_type\\":\\"slug\\",\\"current\\":'
            '\\"claude-code-3\\"},\\"subjects\\":[],\\"summary\\":\\"Workspaces\\",\\"title\\":\\"Claude Code 3.0\\"...')
    (item,) = r._anthropic_news(page)
    assert item["url"] == "https://www.anthropic.com/news/claude-code-3" and item["official"]
    assert item["title"] == "Claude Code 3.0" and item["lab"] == "Anthropic" and item["summary"] == "Workspaces"


def test_same_story_is_grouped_by_target_url_and_by_version_plus_name():
    items = [it("Gemini 4 Argon: our next era", "https://blog.example/g4", family="oficial", official=True),
             it("Gemini 4 Argon", "https://blog.example/g4?utm_source=hn", discussion="https://news.ycombinator.com/item?id=1"),
             it("Gemini 4 is out: the competition has woken up", "https://reddit.com/r/x/1", family="reddit"),
             it("Anthropic announces that Sonnet 4-5 is deprecated", "https://reddit.com/r/x/2", family="reddit"),
             it("GPT-5.5 baja 40% en la API", "https://openai.example/p", family="oficial", official=True),
             it("OpenAI cuts GPT-5.5 prices by 40%", "https://press.example/gpt", family="prensa")]
    groups = sorted(sorted(i["title"][:9] for i in g) for g in r.cluster(items))
    assert ["Gemini 4 ", "Gemini 4 ", "Gemini 4 "] in groups
    assert ["Anthropic"] in groups
    assert ["GPT-5.5 b", "OpenAI cu"] in groups


def test_official_launches_beat_corporate_posts_and_old_news_fades():
    launch = [it("Introducing Gemini 4", "https://g.example/4", family="oficial", official=True, heat=0)]
    corporate = [it("Barclays scales Claude across operations", "https://a.example/b", family="oficial", official=True, heat=0)]
    old_hot = [it("Big AI news", "https://n.example/x", heat=60, published=NOW - 80 * H)]
    assert r.score(launch, NOW) > r.score(corporate, NOW)
    assert r.score(old_hot, NOW) < r.score([dict(old_hot[0], publishedAt=NOW - H)], NOW) / 5


def test_rank_skips_what_was_already_published_and_labels_the_kind():
    items = [it("Introducing Gemini 4", "https://g.example/4", family="oficial", official=True, lab="Google DeepMind"),
             it("Show HN: an agent runtime", "https://gh.example/rt", heat=40),
             it("Old launch v2.0", "https://old.example/v2", family="oficial", official=True, lab="Lab")]
    ranked = r.rank(items, NOW, limit=5, seen_urls=["https://old.example/v2?utm_source=x"])
    assert [g["title"] for g in ranked] == ["Introducing Gemini 4", "Show HN: an agent runtime"]
    assert (ranked[0]["kind"], ranked[0]["lab"]) == ("oficial", "Google DeepMind")
    assert (ranked[1]["kind"], ranked[1]["lab"]) == ("hot", "Comunidad")
    assert ranked[0]["key"].startswith("radar:")


def test_prereleases_are_not_launches(monkeypatch):
    atom = """<feed xmlns="http://www.w3.org/2005/Atom">
      <entry><title>0.161.0-alpha.9</title><link href="https://gh.example/a9"/><updated>2026-10-01T00:00:00Z</updated></entry>
      <entry><title>0.160.0</title><link href="https://gh.example/160"/><updated>2026-09-30T00:00:00Z</updated></entry></feed>"""
    monkeypatch.setattr(r, "OFFICIAL_FEEDS", [])
    monkeypatch.setattr(r, "OFFICIAL_RELEASES", [("OpenAI · Codex", "https://gh.example/codex.atom")])
    monkeypatch.setattr(r, "HF_LABS", {})
    fetch = Fetch({"https://gh.example/codex.atom": (atom, "application/atom+xml"),
                   "https://www.anthropic.com/news": ("", "text/html")})
    got = r.collect_official(NOW, fetch, [])
    assert [i["url"] for i in got] == ["https://gh.example/160"] and got[0]["release"]


def test_reddit_is_paced_and_stops_after_repeated_limits():
    sleeps = []
    fetch = lambda url, **kw: (False, b"", "", url, "HTTP 429")  # noqa: E731
    errors = []
    assert r.collect_reddit(NOW, fetch, errors, sleep=sleeps.append, clock=lambda: 0) == []
    limited = [e for e in errors if "429" in e["error"]]
    skipped = [e for e in errors if "omitido" in e["error"]]
    assert len(limited) == 2 and len(skipped) == len(r.SUBREDDITS) - 2
    assert 5 in sleeps and all(s <= 6 for s in sleeps)


def test_a_broken_collector_is_a_failure_not_silence():
    def boom(now, fetch, errors):
        raise ValueError("cambió el HTML")

    def fine(now, fetch, errors):
        return [it("Introducing X", "https://x.example/1", family="oficial", official=True)]
    got = r.collect(NOW, fetch=lambda *a, **k: None, collectors=(boom, fine))
    assert len(got["items"]) == 1 and got["failures"] == [{"source": "boom", "error": "cambió el HTML"}]


ARTICLE = """<html lang="en"><head><meta property="og:title" content="Gemini 4 Argon">
<meta property="og:image" content="/hero.png"><meta property="article:published_time" content="2026-09-30T16:00:00Z">
</head><body><nav><a>Home</a></nav><article><h1>Gemini 4 Argon</h1>
<p>Today we are introducing <code>gemini-4-argon</code>.</p><ul><li>x.com</li><li>Faster reasoning</li></ul>
<h2>What changes</h2><p>It ships   with a 2M context.</p><figure><img src="/fig1.png" alt="Benchmarks"></figure>
<img src="/icon.svg"><script>alert(1)</script><blockquote>"The best model yet"</blockquote>
<pre>pip install google-genai</pre></article><footer>Share</footer></body></html>"""


def test_capture_keeps_structure_downloads_images_and_drops_noise(tmp_path):
    png = b"\x89PNG" + b"0" * 4000
    fetch = Fetch({"https://blog.example/g4": (ARTICLE, "text/html; charset=utf-8"),
                   "https://blog.example/hero.png": (png, "image/png"),
                   "https://blog.example/fig1.png": (png + b"1", "image/png"),
                   "https://blog.example/icon.svg": (b"<svg onload=x>" + b" " * 3000, "image/svg+xml")})
    ok, cap, err = r.capture_page("https://blog.example/g4", str(tmp_path), fetch=fetch)
    assert ok and err is None
    assert (cap["title"], cap["lang"], cap["publishedAt"]) == ("Gemini 4 Argon", "en", 1790784000)
    kinds = [b["type"] for b in cap["blocks"]]
    assert kinds == ["img", "p", "li", "h", "p", "img", "quote", "code"]
    assert cap["blocks"][1]["text"] == "Today we are introducing `gemini-4-argon`."
    assert cap["blocks"][4]["text"] == "It ships with a 2M context."
    assert all("alert" not in (b.get("text") or "") for b in cap["blocks"])
    images = [b for b in cap["blocks"] if b["type"] == "img"]
    assert all(r._IMAGE_TYPES and (tmp_path / b["media"]).exists() for b in images)
    assert not any(p.suffix == ".svg" for p in tmp_path.iterdir()), "SVG nunca se guarda"
    assert "Benchmarks" in [b["alt"] for b in images]


def test_capture_falls_back_to_the_feed_text_and_marks_it_partial(tmp_path):
    fetch = Fetch({"https://js.example/app": ("<html><body><div id=root></div></body></html>", "text/html")})
    ok, cap, _ = r.capture_page("https://js.example/app", str(tmp_path), fetch=fetch,
                                fallback_blocks=[{"type": "p", "text": "Extracto del feed."}])
    assert ok and cap["partial"] and cap["blocks"] == [{"type": "p", "text": "Extracto del feed."}]


def test_fetch_public_refuses_private_hosts_and_checks_every_redirect():
    import urllib.error
    import email.message
    seen = []

    def opener(req, timeout):
        seen.append(req.full_url)
        headers = email.message.Message()
        headers["Location"] = "http://10.0.0.5/admin"
        raise urllib.error.HTTPError(req.full_url, 302, "found", headers, None)
    resolver = lambda host: host == "public.example"  # noqa: E731
    ok, _b, _k, _f, err = r.fetch_public("http://intranet.local/x", resolver=resolver, opener=opener)
    assert (ok, err, seen) == (False, "host no público", [])
    ok, _b, _k, final, err = r.fetch_public("https://public.example/a", resolver=resolver, opener=opener)
    assert (ok, err, final) == (False, "host no público", "http://10.0.0.5/admin")
    assert seen == ["https://public.example/a"]


def test_hn_discussion_capture_keeps_top_comments_as_quotes():
    import json
    data = {"text": "Ask: <i>is it real?</i>", "children": [{"author": "pg", "text": "<p>Yes &amp; fast</p>"}]}
    fetch = Fetch({"https://hn.algolia.com/api/v1/items/42": (json.dumps(data), "application/json")})
    item = it("Gemini 4", "https://g.example", signals={"hnId": "42"}, discussion="https://news.ycombinator.com/item?id=42")
    ok, cap, _ = r.discussion_capture(item, fetch=fetch)
    assert ok and cap["blocks"] == [{"type": "p", "text": "Ask: is it real?"}, {"type": "quote", "text": "pg: Yes & fast"}]


def test_heat_labels_say_why_something_is_hot():
    assert r.heat_label(it("x", "https://a", signals={"points": 612, "comments": 240})) == "612 pts · 240 comentarios"
    assert r.heat_label(it("x", "https://a", family="github", signals={"starsToday": 4100, "trendingRank": 1})) == "4,100 ★ hoy · #1 en trending"
    assert r.heat_label(it("x", "https://a", family="oficial", official=True)) == "oficial"


def test_a_bridge_title_does_not_chain_two_launches_together():
    items = [it("Gemini 4 Argon: our next era", "https://g.example/4", family="oficial", official=True, heat=0),
             it("Introducing GPT-6.1 Sol", "https://o.example/61", family="oficial", official=True, heat=0),
             it("Gemini 4 Argon vs GPT-6.1 Sol: price and intelligence", "https://blog.example/vs", heat=5)]
    groups = r.cluster(items)
    seeds = sorted(g[0]["title"] for g in groups if g[0]["official"])
    assert seeds == ["Gemini 4 Argon: our next era", "Introducing GPT-6.1 Sol"]
    assert not any(len({i["url"] for i in g if i["official"]}) > 1 for g in groups)


def test_an_old_related_post_does_not_bury_todays_launch():
    fresh = it("Introducing GPT-6.1 Sol", "https://o.example/61", family="oficial", official=True, heat=0, published=NOW - 2 * H)
    old = it("Basis completes a tax workbook with GPT-6 Astra", "https://o.example/basis", family="oficial", official=True,
             heat=0, published=NOW - 80 * H)
    assert r.score([fresh, old], NOW) >= r.score([fresh], NOW)


def test_a_community_link_to_a_lab_domain_counts_as_official():
    def hn(now, fetch, errors):
        return [it("Gemini 4 Argon", "https://blog.google/models/gemini-4", heat=60),
                it("Some model on HF", "https://huggingface.co/someone/model", heat=10)]
    got = r.collect(NOW, fetch=lambda *a, **k: None, collectors=(hn,))["items"]
    assert (got[0]["official"], got[0]["lab"]) == (True, "Google")
    assert got[1]["official"] is False
    (group,) = r.rank(got[:1], NOW)
    assert (group["kind"], group["lab"]) == ("oficial", "Google")
