"""Radar de los Resúmenes: qué salió en IA y qué está caliente.

Recolecta tres familias de fuentes y junta las que hablan de lo mismo:

- oficiales: blogs y feeds de los laboratorios, releases de sus CLIs y los
  modelos nuevos que publican en Hugging Face;
- comunidad con métricas: Hacker News (puntos, comentarios), Reddit «hot»,
  GitHub trending (estrellas de hoy), Hugging Face trending, Lobsters;
- prensa especializada, que solo suma cobertura.

Cada grupo recibe una puntuación (oficialidad, calor, amplitud y frescura) y
el generador se queda con los mejores. Después `capture_page` lee cada fuente
completa como bloques (párrafos, títulos, listas, citas, código e imágenes);
las imágenes se descargan a un directorio local para que el lector no dependa
del sitio original ni haga peticiones a terceros.

Todo lo que viene de fuera es DATO: se resume y se muestra escapado, nunca se
ejecuta ni se sigue como instrucción. Las peticiones solo van a hosts públicos.
"""
from __future__ import annotations

import hashlib
import html
import html.parser
import ipaddress
import json
import math
import os
import re
import socket
import time
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone

UA = ("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) "
      "Chrome/128.0 Safari/537.36 ComandOS-radar/2.0")
HOUR = 3600

# (laboratorio, feed). Un release de CLI pesa menos que un anuncio del blog.
OFFICIAL_FEEDS = [
    ("OpenAI", "https://openai.com/news/rss.xml"),
    ("Google", "https://blog.google/innovation-and-ai/technology/ai/rss/"),
    ("Google DeepMind", "https://deepmind.google/blog/rss.xml"),
    ("Hugging Face", "https://huggingface.co/blog/feed.xml"),
    ("Qwen", "https://qwenlm.github.io/blog/index.xml"),
]
OFFICIAL_RELEASES = [
    ("Anthropic · Claude Code", "https://github.com/anthropics/claude-code/releases.atom"),
    ("OpenAI · Codex", "https://github.com/openai/codex/releases.atom"),
    ("Google · Gemini CLI", "https://github.com/google-gemini/gemini-cli/releases.atom"),
]
# Organizaciones cuyos modelos nuevos en Hugging Face son lanzamientos oficiales.
HF_LABS = {"deepseek-ai": "DeepSeek", "Qwen": "Qwen", "mistralai": "Mistral", "meta-llama": "Meta",
           "google": "Google", "openai": "OpenAI", "moonshotai": "Moonshot", "zai-org": "Z.ai",
           "MiniMaxAI": "MiniMax", "nvidia": "NVIDIA", "microsoft": "Microsoft", "ibm-granite": "IBM"}
PRESS_FEEDS = [
    ("TechCrunch", "https://techcrunch.com/category/artificial-intelligence/feed/"),
    ("The Verge", "https://www.theverge.com/rss/ai-artificial-intelligence/index.xml"),
    ("Simon Willison", "https://simonwillison.net/atom/everything/"),
]
# Reddit sin sesión limita fuerte por IP: pocos subreddits, espaciados, con tope de tiempo.
SUBREDDITS = ("LocalLLaMA", "ClaudeAI", "singularity", "OpenAI", "ChatGPTCoding", "artificial")
REDDIT_BUDGET_S = 45
HN_QUERIES = ("AI", "LLM", "OpenAI", "Anthropic", "Claude", "Gemini", "GPT", "model", "agent", "DeepSeek")
X_ACCOUNTS = ("AnthropicAI", "claudeai", "OpenAI", "OpenAIDevs", "GoogleDeepMind", "GeminiApp", "xai",
              "MistralAI", "deepseek_ai", "Alibaba_Qwen", "huggingface", "AIatMeta")

AI_RE = re.compile(
    r"\b(a\.?i\.?|ai|llms?|gpt[\w.\-]*|chatgpt|claude|anthropic|openai|gemini|deepmind|deepseek|qwen|"
    r"llama|mistral|grok|xai|copilot|codex|cursor|agents?|agentic|models?|transformers?|diffusion|"
    r"hugging ?face|ollama|mcp|rag|inference|fine-?tun\w*|neural|sora|veo|midjourney|nvidia|"
    r"reasoning|benchmark|tokens?|embedding\w*|vllm|llama\.cpp|gguf|lora|multimodal|chatbot\w*)\b", re.I)
NOISE_RE = re.compile(r"\b(customers?|case study|partners?(hip)?|scales?|helping|helps|how .{0,30} uses|ebook|webinar|"
                      r"policy|economic|education|students|teachers|grants?|hiring|careers|events?|summit|"
                      r"podcast|newsletter|recap|community spotlight|year in review)\b", re.I)
# Páginas personales (portafolio, CV, «sobre mí»): no son noticia aunque suban en Reddit.
PERSONAL_RE = re.compile(r"\b(portfolio|portafolio|resume|résumé|curriculum|cv|about me|sobre m[ií]|my (personal )?(site|website|homepage|blog)|"
                         r"software (developer|engineer)|full[- ]?stack developer)\b", re.I)
PRERELEASE_RE = re.compile(r"(alpha|beta|rc\d*|nightly|preview|canary|dev)\b", re.I)
RELEASE_RE = re.compile(r"\b(meet|new|our next|next[- ]gen\w*|launch\w*|releas\w*|introduc\w*|announc\w*|now available|open[- ]?sourc\w*|"
                        r"weights|lanza\w*|presenta\w*|v\d+(\.\d+)*|\d+\.\d+)\b", re.I)
_STOP = set("""the a an and or of to in on for with by from at is are was be as it its this that these those
new now how why what when your you our we they their into about over more than not just after vs via can
will has have had using use used out get got make made up one two first all also only like
el la los las un una y o de del en con por para que se su sus es son al lo como más ya nuevo nueva
show hn ask""".split())


# ---------------------------------------------------------------- red segura

def public_host(host):
    """Solo hosts que resuelven a direcciones públicas (nada de red local)."""
    try:
        infos = socket.getaddrinfo(host, None)
    except OSError:
        return False
    for info in infos:
        addr = ipaddress.ip_address(info[4][0].split("%")[0])
        if (addr.is_private or addr.is_loopback or addr.is_link_local or addr.is_multicast
                or addr.is_reserved or addr.is_unspecified):
            return False
    return bool(infos)


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *a, **kw):
        return None


def canonical(url):
    """URL http(s) normalizada para comparar (sin tracking ni fragmento)."""
    try:
        parts = urllib.parse.urlsplit(str(url or "").strip())
    except ValueError:
        return None
    if parts.scheme.lower() not in ("http", "https") or not parts.hostname or parts.username or parts.password:
        return None
    host = parts.hostname.lower()
    if host.startswith("www."):
        host = host[4:]
    if host == "old.reddit.com":
        host = "reddit.com"
    path = re.sub(r"/{2,}", "/", parts.path or "/")
    if len(path) > 1:
        path = path.rstrip("/")
    query = [(k, v) for k, v in urllib.parse.parse_qsl(parts.query, keep_blank_values=True)
             if not re.match(r"^(utm_.*|fbclid|gclid|mc_cid|mc_eid|ref_src|ref|igshid|s|t)$", k, re.I)]
    return urllib.parse.urlunsplit((parts.scheme.lower(), host, path, urllib.parse.urlencode(sorted(query)), ""))


def fetch_public(url, *, accept="text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
                 max_bytes=2_500_000, timeout=15, resolver=public_host, opener=None, headers=None):
    """GET con cada salto de redirección revisado. → (ok, body, content_type, final_url, error)."""
    current = str(url or "")
    do_open = opener or urllib.request.build_opener(_NoRedirect).open
    for _ in range(8):
        try:
            parts = urllib.parse.urlsplit(current)
        except ValueError:
            return False, b"", "", current, "URL no válida"
        if parts.scheme not in ("http", "https") or not parts.hostname:
            return False, b"", "", current, "URL no válida"
        if not resolver(parts.hostname):
            return False, b"", "", current, "host no público"
        req = urllib.request.Request(current, headers={"User-Agent": UA, "Accept": accept,
                                                       "Accept-Language": "en,es;q=0.8", **(headers or {})})
        try:
            with do_open(req, timeout=timeout) as resp:
                kind = resp.headers.get("Content-Type", "") or ""
                body = resp.read(max_bytes + 1)
                if len(body) > max_bytes:
                    return False, b"", kind, current, "demasiado grande"
                return True, body, kind, current, None
        except urllib.error.HTTPError as err:
            if err.code in (301, 302, 303, 307, 308) and err.headers.get("Location"):
                current = urllib.parse.urljoin(current, err.headers["Location"])
                continue
            return False, b"", "", current, f"HTTP {err.code}"
        except (OSError, ValueError) as err:
            return False, b"", "", current, (str(err)[:120] or "error de red")
    return False, b"", "", current, "demasiadas redirecciones"


def _decode(body, kind):
    m = re.search(r"charset=([\w-]+)", kind or "", re.I)
    try:
        return body.decode(m.group(1) if m else "utf-8", errors="replace")
    except LookupError:
        return body.decode("utf-8", errors="replace")


def _get_text(url, fetch, **kw):
    ok, body, kind, _final, err = fetch(url, **kw)
    if not ok:
        raise RuntimeError(err or "sin respuesta")
    return _decode(body, kind)


def _get_json(url, fetch):
    return json.loads(_get_text(url, fetch, accept="application/json"))


# ---------------------------------------------------------------- feeds

def _ts(value):
    """ISO-8601 / RFC-822 / epoch → segundos; None si no se entiende."""
    if value in (None, ""):
        return None
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return int(value / 1000 if value > 10**11 else value)
    text = str(value).strip()
    if re.fullmatch(r"\d{9,13}", text):
        return _ts(int(text))
    try:
        parsed = datetime.fromisoformat(text.replace("Z", "+00:00"))
        if parsed.tzinfo is None:
            parsed = parsed.replace(tzinfo=timezone.utc)
        return int(parsed.timestamp())
    except ValueError:
        pass
    from email.utils import parsedate_to_datetime
    try:
        return int(parsedate_to_datetime(text).timestamp())
    except (TypeError, ValueError, IndexError):
        return None


def _strip_html(text, limit=1200):
    text = re.sub(r"<[^>]+>", " ", html.unescape(str(text or "")))
    return re.sub(r"\s+", " ", html.unescape(text)).strip()[:limit]


def parse_feed(xml_text):
    """RSS 2.0 o Atom → [{title, url, publishedAt, summary, html}]."""
    root = ET.fromstring(xml_text.encode("utf-8") if isinstance(xml_text, str) else xml_text)
    out = []

    def local(tag):
        return tag.rsplit("}", 1)[-1]
    for node in root.iter():
        if local(node.tag) not in ("item", "entry"):
            continue
        item = {"title": "", "url": "", "publishedAt": None, "summary": "", "html": ""}
        for child in node:
            name = local(child.tag)
            if name == "title":
                item["title"] = _strip_html(child.text or "", 300)
            elif name == "link":
                href = child.get("href") or (child.text or "").strip()
                if href and (not item["url"] or child.get("rel") in (None, "alternate")):
                    item["url"] = href
            elif name in ("pubDate", "published", "updated", "date") and not item["publishedAt"]:
                item["publishedAt"] = _ts(child.text)
            elif name in ("description", "summary") and not item["summary"]:
                item["html"] = item["html"] or (child.text or "")
                item["summary"] = _strip_html(child.text or "")
            elif name in ("encoded", "content"):
                item["html"] = child.text or item["html"]
                if not item["summary"]:
                    item["summary"] = _strip_html(child.text or "")
        if item["title"] and item["url"]:
            out.append(item)
    return out


def _item(*, family, origin, title, url, published=None, official=False, lab=None, discussion=None,
          signals=None, summary="", heat=0.0, release=False):
    return {"family": family, "origin": origin, "title": _strip_html(title, 300), "url": url,
            "publishedAt": published, "official": bool(official), "lab": lab, "discussion": discussion,
            "signals": signals or {}, "summary": summary, "heat": float(heat), "release": bool(release)}


def collect_official(now, fetch, errors):
    out = []
    for lab, url in OFFICIAL_FEEDS:
        try:
            for e in parse_feed(_get_text(url, fetch, accept="application/rss+xml,application/xml,*/*"))[:12]:
                out.append(_item(family="oficial", origin=lab, title=e["title"], url=e["url"],
                                 published=e["publishedAt"], official=True, lab=lab, summary=e["summary"]))
        except Exception as exc:
            errors.append({"source": lab, "error": str(exc)[:200]})
    for lab, url in OFFICIAL_RELEASES:
        try:
            for e in parse_feed(_get_text(url, fetch, accept="application/atom+xml,*/*"))[:10]:
                if PRERELEASE_RE.search(e["title"] or ""):
                    continue
                version = re.search(r"\d+\.\d+(?:\.\d+)?", e["title"] or "")
                minor = bool(version and re.fullmatch(r"\d+\.\d+(?:\.0)?", version.group(0)))
                out.append(_item(family="release", origin=lab, title=f"{lab.split(' · ')[-1]} {e['title']}",
                                 url=e["url"], published=e["publishedAt"], official=True,
                                 lab=lab.split(" · ")[0], summary=e["summary"], release=minor))
        except Exception as exc:
            errors.append({"source": lab, "error": str(exc)[:200]})
    try:
        out += _anthropic_news(_get_text("https://www.anthropic.com/news", fetch))
    except Exception as exc:
        errors.append({"source": "Anthropic", "error": str(exc)[:200]})
    for author, lab in HF_LABS.items():
        try:
            rows = _get_json(f"https://huggingface.co/api/models?author={urllib.parse.quote(author)}"
                             "&sort=createdAt&direction=-1&limit=6", fetch)
            for m in rows if isinstance(rows, list) else []:
                created = _ts(m.get("createdAt"))
                if not created or now - created > 72 * HOUR:
                    continue
                out.append(_item(family="hf-org", origin=f"Hugging Face · {lab}", title=m.get("id", ""),
                                 url=f"https://huggingface.co/{m.get('id')}", published=created, official=True,
                                 lab=lab, signals={"likes": m.get("likes") or 0, "downloads": m.get("downloads") or 0},
                                 release=True))
        except Exception as exc:
            errors.append({"source": f"Hugging Face · {lab}", "error": str(exc)[:200]})
    return out


def _anthropic_news(page):
    """La portada de noticias trae un JSON con publishedOn + slug + título."""
    text = page.replace('\\"', '"')
    out, seen = [], set()
    for m in re.finditer(r'"publishedOn":"([^"]+)","slug":\{"_type":"slug","current":"([a-z0-9-]+)"\}'
                         r'(.{0,1500}?)"title":"([^"]{4,240})"', text, re.S):
        slug = m.group(2)
        if slug in seen:
            continue
        seen.add(slug)
        summary = re.search(r'"summary":"([^"]{0,600})"', m.group(3))
        out.append(_item(family="oficial", origin="Anthropic", title=html.unescape(m.group(4)),
                         url=f"https://www.anthropic.com/news/{slug}", published=_ts(m.group(1)),
                         official=True, lab="Anthropic", summary=html.unescape(summary.group(1)) if summary else ""))
        if len(out) >= 12:
            break
    return out


def collect_hn(now, fetch, errors):
    since = now - 36 * HOUR
    hits, seen = [], set()
    for q in HN_QUERIES:
        try:
            data = _get_json("https://hn.algolia.com/api/v1/search?" + urllib.parse.urlencode(
                {"query": q, "tags": "story", "hitsPerPage": 40,
                 "numericFilters": f"created_at_i>{since},points>40"}), fetch)
            for h in data.get("hits") or []:
                if h.get("objectID") in seen:
                    continue
                seen.add(h.get("objectID"))
                hits.append(h)
        except Exception as exc:
            errors.append({"source": "Hacker News", "error": str(exc)[:200]})
            break
    out = []
    for h in hits:
        title = h.get("title") or ""
        if not AI_RE.search(title):
            continue
        points, comments = int(h.get("points") or 0), int(h.get("num_comments") or 0)
        disc = f"https://news.ycombinator.com/item?id={h.get('objectID')}"
        heat = 12 * math.log2(1 + points / 50) + 4 * math.log2(1 + comments / 40)
        out.append(_item(family="hn", origin="Hacker News", title=title, url=h.get("url") or disc,
                         published=h.get("created_at_i"), discussion=disc,
                         signals={"points": points, "comments": comments, "hnId": h.get("objectID")}, heat=heat))
    return out


def _reddit_get(url, fetch, sleep=time.sleep):
    """Reddit sin sesión limita a ~1 petición cada 2 s: un reintento tras un 429."""
    for attempt in range(2):
        ok, body, kind, _final, err = fetch(url, accept="application/atom+xml,*/*")
        if ok:
            return _decode(body, kind)
        if err != "HTTP 429" or attempt == 1:
            raise RuntimeError(err or "sin respuesta")
        sleep(5)


def collect_reddit(now, fetch, errors, sleep=time.sleep, clock=time.monotonic):
    out, start, limited = [], clock(), 0
    for n, sub in enumerate(SUBREDDITS):
        if clock() - start > REDDIT_BUDGET_S or limited >= 2:
            errors.append({"source": f"r/{sub}", "error": "omitido: Reddit limitó las peticiones"})
            continue
        if n:
            sleep(2.5)
        try:
            entries = parse_feed(_reddit_get(f"https://www.reddit.com/r/{sub}/hot/.rss?limit=25", fetch, sleep))
        except Exception as exc:
            limited += "429" in str(exc)
            errors.append({"source": f"r/{sub}", "error": str(exc)[:200]})
            continue
        for rank, e in enumerate(entries[:25]):
            if e["publishedAt"] and now - e["publishedAt"] > 48 * HOUR:
                continue
            body = e["html"] or ""
            links = [html.unescape(u) for u in re.findall(r'href="([^"]+)"', html.unescape(body))]
            target = next((u for u in links if u.startswith("http") and not re.search(
                r"//(www\.|old\.)?reddit\.com|//redd\.it|//i\.redd\.it|//v\.redd\.it|//preview\.redd", u)), None)
            if sub in ("ChatGPT", "artificial", "singularity") and not AI_RE.search(e["title"]):
                continue
            heat = 9 * (1 - rank / 25) + (3 if sub in ("LocalLLaMA", "ClaudeAI", "singularity") else 0)
            text = _strip_html(re.sub(r"submitted by.*", "", html.unescape(body), flags=re.S), 2000)
            out.append(_item(family="reddit", origin=f"r/{sub}", title=e["title"], url=target or e["url"],
                             published=e["publishedAt"], discussion=e["url"],
                             signals={"hotRank": rank + 1, "subreddit": sub, "selftext": text}, heat=heat))
    return out


def collect_github_trending(now, fetch, errors):
    try:
        page = _get_text("https://github.com/trending?since=daily", fetch)
    except Exception as exc:
        errors.append({"source": "GitHub trending", "error": str(exc)[:200]})
        return []
    out = []
    for rank, block in enumerate(re.findall(r'<article class="Box-row">(.*?)</article>', page, re.S)):
        repo = re.search(r'<h2[^>]*>\s*<a[^>]*href="/([^"]+)"', block)
        if not repo:
            continue
        name = repo.group(1).strip()
        desc = _strip_html((re.search(r'<p class="col-9[^"]*">(.*?)</p>', block, re.S) or [None, ""])[1], 400)
        stars = re.search(r"([\d,]+)\s+stars? today", block)
        today = int(stars.group(1).replace(",", "")) if stars else 0
        if not AI_RE.search(f"{name} {desc}"):
            continue
        heat = 10 * math.log2(1 + today / 150)
        out.append(_item(family="github", origin="GitHub trending", title=f"{name}: {desc}" if desc else name,
                         url=f"https://github.com/{name}", published=None,
                         signals={"starsToday": today, "trendingRank": rank + 1}, heat=heat, summary=desc))
    return out


def collect_hf(now, fetch, errors):
    out = []
    try:
        rows = _get_json("https://huggingface.co/api/models?sort=trendingScore&direction=-1&limit=20", fetch)
        for rank, m in enumerate(rows if isinstance(rows, list) else []):
            created = _ts(m.get("createdAt"))
            if created and now - created > 21 * 24 * HOUR:
                continue                                  # tendencia vieja: ya se contó
            out.append(_item(family="hf", origin="Hugging Face trending", title=m.get("id", ""),
                             url=f"https://huggingface.co/{m.get('id')}", published=created,
                             signals={"trendingRank": rank + 1, "likes": m.get("likes") or 0,
                                      "downloads": m.get("downloads") or 0},
                             heat=9 * (1 - rank / 20), release=True))
    except Exception as exc:
        errors.append({"source": "Hugging Face trending", "error": str(exc)[:200]})
    try:
        rows = _get_json("https://huggingface.co/api/daily_papers?limit=15", fetch)
        for p in rows if isinstance(rows, list) else []:
            paper = p.get("paper") or {}
            up = int(paper.get("upvotes") or 0)
            if up < 15:
                continue
            out.append(_item(family="papers", origin="Hugging Face papers", title=paper.get("title") or p.get("title", ""),
                             url=f"https://huggingface.co/papers/{paper.get('id')}", published=_ts(p.get("publishedAt")),
                             signals={"upvotes": up}, heat=5 * math.log2(1 + up / 15),
                             summary=_strip_html(paper.get("summary"), 800)))
    except Exception as exc:
        errors.append({"source": "Hugging Face papers", "error": str(exc)[:200]})
    return out


def collect_lobsters(now, fetch, errors):
    try:
        rows = _get_json("https://lobste.rs/t/ai.json", fetch)
    except Exception as exc:
        errors.append({"source": "Lobsters", "error": str(exc)[:200]})
        return []
    out = []
    for s in rows if isinstance(rows, list) else []:
        created = _ts(s.get("created_at"))
        if created and now - created > 48 * HOUR:
            continue
        score = int(s.get("score") or 0)
        out.append(_item(family="lobsters", origin="Lobsters", title=s.get("title", ""),
                         url=s.get("url") or s.get("comments_url"), published=created,
                         discussion=s.get("comments_url"),
                         signals={"points": score, "comments": s.get("comment_count") or 0},
                         heat=5 * math.log2(1 + score / 10)))
    return out


def collect_press(now, fetch, errors):
    out = []
    for name, url in PRESS_FEEDS:
        try:
            for e in parse_feed(_get_text(url, fetch, accept="application/rss+xml,application/atom+xml,*/*"))[:15]:
                if e["publishedAt"] and now - e["publishedAt"] > 48 * HOUR:
                    continue
                if name == "Simon Willison" and not AI_RE.search(e["title"] + " " + e["summary"][:300]):
                    continue
                out.append(_item(family="prensa", origin=name, title=e["title"], url=e["url"],
                                 published=e["publishedAt"], summary=e["summary"], heat=4.0))
        except Exception as exc:
            errors.append({"source": name, "error": str(exc)[:200]})
    return out


def collect_x(now, fetch, errors, env=None):
    """Opcional: solo con X_BEARER_TOKEN. La API de X cobra por lectura."""
    env = os.environ if env is None else env
    token = env.get("X_BEARER_TOKEN")
    if not token:
        return []
    query = "(" + " OR ".join(f"from:{a}" for a in X_ACCOUNTS) + ") -is:retweet -is:reply"
    url = "https://api.x.com/2/tweets/search/recent?" + urllib.parse.urlencode(
        {"query": query, "max_results": 50, "tweet.fields": "created_at,public_metrics,entities,author_id",
         "expansions": "author_id", "user.fields": "username,name"})
    try:
        data = json.loads(_get_text(url, fetch, accept="application/json",
                                    headers={"Authorization": f"Bearer {token}"}))
    except Exception as exc:
        errors.append({"source": "X", "error": str(exc)[:200]})
        return []
    users = {u["id"]: u for u in (data.get("includes") or {}).get("users") or []}
    out = []
    for t in data.get("data") or []:
        user = users.get(t.get("author_id")) or {}
        m = t.get("public_metrics") or {}
        likes, reposts = int(m.get("like_count") or 0), int(m.get("retweet_count") or 0)
        link = next((u.get("expanded_url") for u in (t.get("entities") or {}).get("urls") or []
                     if u.get("expanded_url") and "x.com" not in u.get("expanded_url", "")), None)
        post = f"https://x.com/{user.get('username', 'i')}/status/{t.get('id')}"
        out.append(_item(family="x", origin=f"X · @{user.get('username', '?')}", title=t.get("text", "")[:280],
                         url=link or post, published=_ts(t.get("created_at")), official=True,
                         lab=user.get("name"), discussion=post,
                         signals={"likes": likes, "reposts": reposts},
                         heat=8 * math.log2(1 + likes / 500) + 4 * math.log2(1 + reposts / 100)))
    return out


# Dominios de anuncios oficiales: una nota de HN o Reddit que enlaza aquí
# apunta al anuncio mismo, aunque el feed del laboratorio haya fallado.
OFFICIAL_DOMAINS = [
    (r"(^|\.)anthropic\.com$|^claude\.(ai|com)$", "Anthropic"), (r"(^|\.)openai\.com$", "OpenAI"),
    (r"^deepmind\.google$", "Google DeepMind"), (r"^blog\.google$", "Google"), (r"^developers\.googleblog\.com$", "Google"),
    (r"(^|\.)x\.ai$", "xAI"), (r"^mistral\.ai$", "Mistral"), (r"^ai\.meta\.com$", "Meta"),
    (r"^qwenlm\.github\.io$|^qwen\.ai$", "Qwen"), (r"(^|\.)deepseek\.com$", "DeepSeek"),
    (r"^blogs\.nvidia\.com$", "NVIDIA"), (r"^huggingface\.co$", "Hugging Face"),
]


def official_lab(url):
    host = (urllib.parse.urlsplit(str(url or "")).hostname or "").lower().removeprefix("www.")
    path = urllib.parse.urlsplit(str(url or "")).path
    if host == "huggingface.co" and not path.startswith("/blog"):
        return None                                   # un repo de modelo cualquiera no es anuncio
    return next((lab for pattern, lab in OFFICIAL_DOMAINS if re.search(pattern, host)), None)


COLLECTORS = (collect_official, collect_hn, collect_reddit, collect_github_trending, collect_hf,
              collect_lobsters, collect_press, collect_x)


def collect(now=None, *, fetch=fetch_public, collectors=COLLECTORS):
    """Todas las familias en paralelo. → {items, failures}. Un fallo no es «sin noticias»."""
    now = int(now or time.time())
    errors, items = [], []

    def run(fn):
        local = []
        try:
            return fn(now, fetch, local), local
        except Exception as exc:                          # un recolector roto no tumba la edición
            return [], local + [{"source": fn.__name__.replace("collect_", ""), "error": str(exc)[:200]}]
    with ThreadPoolExecutor(max_workers=len(collectors)) as pool:
        for got, errs in pool.map(run, collectors):
            items += [i for i in got if i.get("title") and canonical(i.get("url"))]
            errors += errs
    for i in items:
        lab = None if i["official"] else official_lab(i["url"])
        if lab:
            i.update(official=True, lab=lab, officialUrl=True)
    return {"items": items, "failures": errors}


# ---------------------------------------------------------------- agrupar y puntuar

_TOKEN_RE = re.compile(r"[a-z0-9]+(?:\.[0-9]+)*")


def tokens(title):
    words = _TOKEN_RE.findall(str(title or "").lower())
    return {w for w in words if (len(w) >= 3 or any(c.isdigit() for c in w)) and w not in _STOP}


_GENERIC = set("""announces announced announcing releases released release launch launches launched model models
available update updates version versions introducing today open source weights preview official users
says said report reports ships shipped adds added support supports better faster cheaper""".split())


def _similar(a, b):
    """Mismo tema: la mitad de las palabras en común, o la misma versión
    (un token con dígito) más un nombre propio en común (no «announces»)."""
    if not a or not b:
        return False
    shared = a & b
    if len(shared) / len(a | b) >= 0.5:
        return True
    keyed = [w for w in shared if any(c.isdigit() for c in w)]
    named = [w for w in shared if not any(c.isdigit() for c in w) and w not in _GENERIC]
    return bool(keyed) and bool(named)


def cluster(items):
    """Une lo que habla de lo mismo sin encadenar: cada grupo crece alrededor
    de su semilla (lo oficial y más caliente primero). Una nota entra si
    comparte URL destino con el grupo o si se parece a la semilla; así un
    título «puente» no junta dos lanzamientos distintos."""
    order = sorted(range(len(items)), key=lambda k: (not items[k]["official"], -items[k]["heat"]))
    groups, seeds, by_url = [], [], {}

    def urls(it):
        out = set()
        for u in (it["url"], it.get("discussion")):
            key = canonical(u)
            if key and "news.ycombinator.com" not in key and "reddit.com" not in key:
                out.add(key)
        return out
    for k in order:
        it, toks = items[k], tokens(items[k]["title"])
        home = next((by_url[u] for u in urls(it) if u in by_url), None)
        if home is None:
            home = next((g for g, seed in enumerate(seeds) if _similar(toks, seed)), None)
        if home is None:
            home = len(groups)
            groups.append([])
            seeds.append(toks)
        groups[home].append(it)
        for u in urls(it):
            by_url.setdefault(u, home)
    return groups


def _age_factor(hours):
    if hours is None:
        return 0.7
    for limit, factor in ((12, 1.0), (24, 0.85), (36, 0.6), (72, 0.3)):
        if hours < limit:
            return factor
    return 0.08


def score(group, now):
    official = [i for i in group if i["official"]]
    families = {i["family"] for i in group}
    heat = sum(i["heat"] for i in group)
    base = 0.0
    launch = any(i["release"] or RELEASE_RE.search(i["title"]) for i in group)
    if any(i["family"] in ("oficial", "x") or i.get("officialUrl") for i in official):
        base += 22 + (18 if launch else 0)
        if not launch and all(NOISE_RE.search(i["title"]) for i in official):
            base -= 16                                    # caso de cliente, política, evento: no es lanzamiento
    elif any(i["family"] == "hf-org" for i in official):
        base += 26
    elif any(i["family"] == "release" and i["release"] for i in official):
        base += 24
    elif official:
        base += 6
    if launch:
        base += 6
    breadth = 8 * (len(families) - 1)
    if not official and not launch and all(_personal(i) for i in group):
        base -= 0.6 * (heat + breadth)                    # portafolio o portada personal: casi fuera
    # La frescura es la del anuncio principal; si no trae fecha, la más reciente.
    lead = _primary(group).get("publishedAt") or max((i["publishedAt"] for i in group if i.get("publishedAt")), default=None)
    age = (now - lead) / HOUR if lead else None
    return round((base + heat + breadth) * _age_factor(age), 2)


def _personal(item):
    """Enlace de comunidad a una página personal: título de portafolio o la portada de un sitio."""
    if PERSONAL_RE.search(item.get("title") or ""):
        return True
    path = urllib.parse.urlsplit(item.get("url") or "").path
    return item["family"] in ("reddit", "hn", "lobsters") and path in ("", "/")


def _primary(group):
    order = {"oficial": 0, "x": 1, "hf-org": 2, "release": 3, "prensa": 5}
    return sorted(group, key=lambda i: (0 if i.get("officialUrl") else order.get(i["family"], 4), -i["heat"]))[0]


def rank(items, now, *, limit=6, seen_urls=(), min_score=10.0):
    """Los mejores grupos, sin repetir lo ya publicado en las últimas ediciones."""
    seen = {canonical(u) for u in seen_urls if canonical(u)}
    ranked = []
    for group in cluster(items):
        urls = {canonical(i["url"]) for i in group} | {canonical(i.get("discussion")) for i in group}
        if seen & {u for u in urls if u}:
            continue
        s = score(group, now)
        if s < min_score:
            continue
        p = _primary(group)
        lab = p.get("lab") if p["official"] else None
        ranked.append({"key": "radar:" + hashlib.sha1(canonical(p["url"]).encode()).hexdigest()[:16],
                       "score": s, "kind": "oficial" if lab else "hot", "lab": lab or "Comunidad",
                       "title": p["title"], "items": group})
    ranked.sort(key=lambda g: -g["score"])
    return ranked[:limit]


def heat_label(item):
    """Texto corto del calor de una fuente: «612 pts · 240 comentarios»."""
    s = item.get("signals") or {}
    if item["family"] == "hn" or item["family"] == "lobsters":
        return f"{s.get('points', 0)} pts · {s.get('comments', 0)} comentarios"
    if item["family"] == "reddit":
        return f"#{s.get('hotRank')} en hot de r/{s.get('subreddit')}"
    if item["family"] == "github":
        return f"{s.get('starsToday', 0):,} ★ hoy · #{s.get('trendingRank')} en trending"
    if item["family"] in ("hf", "hf-org"):
        bits = [f"#{s['trendingRank']} trending"] if s.get("trendingRank") else []
        return " · ".join(bits + [f"{s.get('likes', 0):,} ♥", f"{s.get('downloads', 0):,} descargas"])
    if item["family"] == "papers":
        return f"{s.get('upvotes', 0)} votos en papers del día"
    if item["family"] == "x":
        return f"{s.get('likes', 0):,} ♥ · {s.get('reposts', 0):,} reposts"
    return "oficial" if item["official"] else ""


# ---------------------------------------------------------------- captura completa

class _Blocks(html.parser.HTMLParser):
    SKIP = {"script", "style", "noscript", "svg", "nav", "footer", "header", "form", "aside", "button",
            "iframe", "template", "select", "dialog", "canvas"}
    BLOCK = {"p": "p", "h1": "h", "h2": "h", "h3": "h", "h4": "h", "li": "li", "blockquote": "quote",
             "pre": "code", "figcaption": "caption", "td": "p", "dd": "p", "dt": "h"}

    def __init__(self, base, scope):
        super().__init__(convert_charrefs=True)
        self.base, self.scope = base, scope
        self.blocks, self.meta = [], {}
        self.skip = 0
        self.inside = 0 if scope else 1
        self.stack = []                  # bloque abierto: [tipo, partes]
        self.inline_code = 0

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        if tag == "meta":
            key = (a.get("property") or a.get("name") or "").lower()
            if key in ("og:title", "og:image", "article:published_time", "author", "og:site_name",
                       "description", "og:description", "twitter:image") and a.get("content"):
                self.meta.setdefault(key, a["content"])
            return
        if tag == "html" and a.get("lang"):
            self.meta["lang"] = a["lang"][:8]
        if tag == "time" and a.get("datetime"):
            self.meta.setdefault("time", a["datetime"])
        if self.scope and tag == self.scope:
            self.inside += 1
        if tag in self.SKIP:
            self.skip += 1
            return
        if self.skip or not self.inside:
            return
        if tag == "img":
            src = a.get("src") or a.get("data-src") or ""
            if (not src or src.startswith("data:")) and a.get("srcset"):
                src = a["srcset"].split(",")[0].strip().split(" ")[0]
            if src and not src.startswith("data:"):
                self._close()
                self.blocks.append({"type": "img", "src": urllib.parse.urljoin(self.base, src),
                                    "alt": (a.get("alt") or "")[:200]})
            return
        if tag == "br" and self.stack:
            self.stack[-1][1].append("\n")
        if tag == "code" and self.stack and self.stack[-1][0] != "code":
            self.inline_code += 1
            self.stack[-1][1].append("`")
        if tag in self.BLOCK:
            self._close()
            self.stack.append([self.BLOCK[tag], []])

    def handle_endtag(self, tag):
        if self.scope and tag == self.scope and self.inside:
            self.inside -= 1
        if tag in self.SKIP:
            self.skip = max(0, self.skip - 1)
            return
        if tag == "code" and self.inline_code and self.stack:
            self.inline_code -= 1
            self.stack[-1][1].append("`")
        if tag in self.BLOCK:
            self._close()

    def handle_data(self, data):
        if self.skip or not self.inside or not self.stack:
            return
        self.stack[-1][1].append(data)

    def _close(self):
        while self.stack:
            kind, parts = self.stack.pop()
            text = "".join(parts)
            text = text if kind == "code" else re.sub(r"\s+", " ", text).strip()
            text = text.replace("``", "")
            if kind == "code":
                text = text.strip("\n")
            if text and len(text) > 1:
                self.blocks.append({"type": kind, "text": text[:4000]})


_SHARE = re.compile(r"^(x(\.com)?|twitter|facebook|linkedin|e-?mail|mail|threads|bluesky|reddit|whatsapp|"
                    r"copy( link)?|print|share( this)?|hacker news|youtube|instagram|rss)$", re.I)
_BOILER = re.compile(r"^(share|subscribe|sign up|log in|cookie|accept|related|read more|advertisement|"
                     r"follow us|copy link|table of contents|skip to)", re.I)


def page_blocks(page, base):
    """HTML → (bloques, meta). Prefiere <article>, luego <main>, luego todo."""
    scope = "article" if re.search(r"<article[\s>]", page, re.I) else "main" if re.search(r"<main[\s>]", page, re.I) else None
    parser = _Blocks(base, scope)
    parser.feed(page)
    parser._close()
    blocks = parser.blocks
    if scope and sum(len(b.get("text", "")) for b in blocks) < 400:
        parser = _Blocks(base, None)
        parser.feed(page)
        parser._close()
        blocks = parser.blocks
    clean, last = [], None
    for b in blocks:
        if b["type"] != "img":
            if (_BOILER.match(b["text"]) and len(b["text"]) < 80) or _SHARE.match(b["text"].strip()):
                continue
            if b["type"] == "h" and b["text"] == (parser.meta.get("og:title") or "").strip() and not clean:
                continue                                  # el título ya va arriba
        key = b.get("text") or b.get("src")
        if key == last:
            continue
        last = key
        clean.append(b)
    out, chars = [], 0
    for b in clean:
        chars += len(b.get("text", ""))
        out.append(b)
        if len(out) >= 120 or chars > 30000:
            break
    return out, parser.meta


_IMAGE_TYPES = {"image/png": "png", "image/jpeg": "jpg", "image/webp": "webp", "image/gif": "gif",
                "image/avif": "avif"}


def save_image(url, media_dir, *, fetch=fetch_public):
    """Descarga una imagen pública a media_dir. → nombre de archivo o None. Nunca SVG."""
    ok, body, kind, _final, _err = fetch(url, accept="image/avif,image/webp,image/png,image/jpeg,image/gif",
                                         max_bytes=4_000_000, timeout=12)
    ext = _IMAGE_TYPES.get((kind or "").split(";")[0].strip().lower())
    if not ok or not ext or len(body) < 1500:
        return None
    name = hashlib.sha256(body).hexdigest()[:32] + "." + ext
    os.makedirs(media_dir, mode=0o700, exist_ok=True)
    path = os.path.join(media_dir, name)
    if not os.path.exists(path):
        tmp = path + ".part"
        with open(tmp, "wb") as fh:
            fh.write(body)
        os.replace(tmp, path)
    return name


def blocks_text(blocks, limit=14000):
    """Bloques → texto plano para el modelo (marca títulos, listas, citas e imágenes)."""
    lines = []
    for b in blocks:
        t = b.get("type")
        if t == "img":
            if b.get("alt"):
                lines.append(f"[imagen: {b['alt']}]")
            continue
        prefix = {"h": "## ", "li": "- ", "quote": "> ", "caption": "(pie) "}.get(t, "")
        lines.append(prefix + b["text"])
    text = "\n".join(lines)
    return text[:limit]


def capture_page(url, media_dir, *, fetch=fetch_public, max_images=6, fallback_blocks=None):
    """Lee una página completa. → (ok, capture, error). capture = {finalUrl, title,
    byline, lang, publishedAt, blocks, partial}. Las imágenes quedan locales; si
    una no baja, se omite (no se enlaza la remota)."""
    ok, body, kind, final, err = fetch(url)
    capture = {"finalUrl": final or url, "title": None, "byline": None, "lang": None,
               "publishedAt": None, "blocks": [], "partial": False}
    if ok and re.match(r"text/(html|plain)|application/xhtml", kind or "text/html"):
        page = _decode(body, kind)
        if "html" in (kind or "html"):
            blocks, meta = page_blocks(page, final or url)
        else:
            blocks, meta = [{"type": "p", "text": p.strip()} for p in page.split("\n\n") if p.strip()][:120], {}
        capture.update(title=_strip_html(meta.get("og:title"), 300) or None, lang=meta.get("lang"),
                       byline=_strip_html(meta.get("author") or meta.get("og:site_name"), 120) or None,
                       publishedAt=_ts(meta.get("article:published_time") or meta.get("time")))
        hero = meta.get("og:image") or meta.get("twitter:image")
        if hero and not any(b["type"] == "img" for b in blocks[:4]):
            blocks.insert(0, {"type": "img", "src": urllib.parse.urljoin(final or url, hero), "alt": capture["title"] or ""})
        text_chars = sum(len(b.get("text", "")) for b in blocks)
        if text_chars < 300 and fallback_blocks:
            blocks = [b for b in blocks if b["type"] == "img"][:1] + fallback_blocks
            capture["partial"] = True
    elif fallback_blocks:
        blocks, capture["partial"] = list(fallback_blocks), True
    else:
        return False, capture, err or f"tipo no legible: {(kind or '')[:40]}"
    kept, images = [], 0
    for b in blocks:
        if b["type"] == "img":
            if images >= max_images:
                continue
            name = save_image(b["src"], media_dir, fetch=fetch) if canonical(b["src"]) else None
            if not name:
                continue
            images += 1
            kept.append({"type": "img", "media": name, "alt": b.get("alt") or ""})
        else:
            kept.append(b)
    capture["blocks"] = kept
    if not any(b["type"] != "img" for b in kept):
        return False, capture, err or "página sin texto legible"
    return True, capture, None


def discussion_capture(item, *, fetch=fetch_public):
    """El hilo de HN o Reddit como bloques: el post y sus comentarios de arriba."""
    blocks = []
    try:
        if item["family"] == "hn" and item["signals"].get("hnId"):
            data = _get_json(f"https://hn.algolia.com/api/v1/items/{item['signals']['hnId']}", fetch)
            if data.get("text"):
                blocks.append({"type": "p", "text": _strip_html(data["text"], 3000)})
            for c in (data.get("children") or [])[:10]:
                text = _strip_html(c.get("text"), 1200)
                if text:
                    blocks.append({"type": "quote", "text": f"{c.get('author') or 'anónimo'}: {text}"})
        elif item["family"] == "reddit":
            if item["signals"].get("selftext"):
                blocks.append({"type": "p", "text": item["signals"]["selftext"]})
            xml_text = _reddit_get(item["discussion"].rstrip("/") + "/.rss?limit=12", fetch)
            for e in parse_feed(xml_text)[1:11]:
                text = _strip_html(re.sub(r"<!-- SC_OFF -->|<!-- SC_ON -->", "", html.unescape(e["html"] or "")), 1200)
                author = re.sub(r"^/u/", "", e["title"].split(" on ")[0]) if " on " in e["title"] else "comentario"
                if text:
                    blocks.append({"type": "quote", "text": f"{author}: {text}"})
        elif item.get("summary"):
            blocks.append({"type": "p", "text": item["summary"]})
    except Exception as exc:
        return False, {"finalUrl": item.get("discussion"), "title": item["title"], "byline": item["origin"],
                       "lang": None, "publishedAt": item.get("publishedAt"), "blocks": blocks, "partial": True}, str(exc)[:120]
    capture = {"finalUrl": item.get("discussion") or item["url"], "title": item["title"], "byline": item["origin"],
               "lang": "en", "publishedAt": item.get("publishedAt"), "blocks": blocks, "partial": False}
    return bool(blocks), capture, None if blocks else "hilo sin texto"
