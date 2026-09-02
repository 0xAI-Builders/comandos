"""Watcher de NOTICIAS de skills y MCPs — patrón Radar de LifeOS.

Igual que el scanner de hackathones/bounties (LifeOS radar/scanner.ts):
  1. Golpear APIs JSON públicas (tu SearXNG local, HN Algolia, Reddit JSON,
     GitHub API de los repos oficiales) — sin navegador, sin API keys.
  2. Normalizar a un esquema común {source, kind, title, url, at}.
  3. Dedup por URL contra el snapshot (HOOKS/news-watch.json).
  4. Lo nuevo alimenta el centro de notificaciones (clase skill/mcp/noticia).

Cada fuente es best-effort: si una falla (Reddit bloquea, SearXNG sin
resultados), las demás siguen. Jamás truena el ciclo.
"""
from __future__ import annotations

import json
import os
import re
import time
import urllib.parse
import urllib.request

UA = "ComandOS-news/1.0 (local dashboard; +localhost)"
# El Radar de LifeOS usa UA de Chrome: Superteam redirige a los UA raros
UA_BROWSER = ("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 "
              "(KHTML, like Gecko) Chrome/124.0 Safari/537.36")
SEARX = os.environ.get("COMANDOS_SEARX", "http://localhost:27950")

_SKILL_RE = re.compile(r"\bskills?\b", re.I)
_MCP_RE = re.compile(r"\bmcp\b|model context protocol", re.I)


def _get_json(url, timeout=12, headers=None, browser=False, _hops=0):
    req = urllib.request.Request(url, headers={"User-Agent": UA_BROWSER if browser else UA,
                                               "Accept": "application/json",
                                               **(headers or {})})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return json.loads(r.read().decode(errors="replace"))
    except urllib.error.HTTPError as err:
        # urllib de Python NO sigue 308 (solo 301/302/303/307): seguirlo a mano
        if err.code == 308 and _hops < 3:
            loc = err.headers.get("Location") or ""
            if loc:
                nxt = urllib.parse.urljoin(url, loc)
                return _get_json(nxt, timeout=timeout, headers=headers,
                                 browser=browser, _hops=_hops + 1)
        raise


def _kind(title):
    t = title or ""
    if _MCP_RE.search(t):
        return "mcp"
    if _SKILL_RE.search(t):
        return "skill"
    return "noticia"


def _mk(source, title, url, at):
    title = (title or "").strip()
    if not title or not url:
        return None
    return {"source": source, "kind": _kind(title), "title": title[:180],
            "url": url, "at": int(at or time.time())}


def fetch_searxng(now):
    out = []
    for q in ('"claude code" skill', '"claude" mcp server',
              "model context protocol server nuevo"):
        try:
            params = urllib.parse.urlencode({"q": q, "format": "json",
                                             "time_range": "week", "pageno": 1})
            data = _get_json(f"{SEARX}/search?{params}")
            for r in (data.get("results") or [])[:8]:
                it = _mk("searxng", r.get("title"), r.get("url"), now)
                if it:
                    out.append(it)
        except Exception:
            continue
    return out


def fetch_hn(now):
    out = []
    for q in ('"claude code" skill', '"claude" skills', 'mcp server'):
        try:
            params = urllib.parse.urlencode({"query": q, "tags": "story",
                                             "hitsPerPage": 8})
            data = _get_json(f"https://hn.algolia.com/api/v1/search_by_date?{params}")
            for h in data.get("hits") or []:
                title = h.get("title") or ""
                # el buscador de HN matchea laxo: exigir tema real
                if not re.search(r"claude|anthropic|\bmcp\b|model context", title, re.I):
                    continue
                url = h.get("url") or f"https://news.ycombinator.com/item?id={h.get('objectID')}"
                it = _mk("hackernews", title, url, h.get("created_at_i"))
                if it:
                    out.append(it)
        except Exception:
            continue
    return out


def fetch_reddit(now):
    out = []
    for sub in ("ClaudeAI", "ClaudeCode", "mcp"):
        try:
            data = _get_json(f"https://www.reddit.com/r/{sub}/new.json?limit=15",
                             headers={"Accept": "*/*"})
            for c in (data.get("data") or {}).get("children") or []:
                p = c.get("data") or {}
                title = p.get("title") or ""
                if not (_SKILL_RE.search(title) or _MCP_RE.search(title)):
                    continue
                it = _mk(f"r/{sub}", title,
                         "https://reddit.com" + (p.get("permalink") or ""),
                         p.get("created_utc"))
                if it:
                    out.append(it)
        except Exception:
            continue
    return out


def fetch_github(now):
    """Commits recientes de los repos oficiales: skills de Anthropic y
    servers del Model Context Protocol. API pública sin auth (60 req/h)."""
    out = []
    for repo, kind in (("anthropics/skills", "skill"),
                       ("modelcontextprotocol/servers", "mcp")):
        try:
            data = _get_json(f"https://api.github.com/repos/{repo}/commits?per_page=5",
                             headers={"Accept": "application/vnd.github+json"})
            for c in data if isinstance(data, list) else []:
                msg = ((c.get("commit") or {}).get("message") or "").splitlines()[0]
                when = ((c.get("commit") or {}).get("author") or {}).get("date") or ""
                try:
                    from datetime import datetime
                    at = int(datetime.fromisoformat(when.replace("Z", "+00:00")).timestamp())
                except Exception:
                    at = now
                it = _mk(f"github/{repo.split('/')[0]}", msg, c.get("html_url"), at)
                if it:
                    it["kind"] = kind
                    out.append(it)
        except Exception:
            continue
    return out


def fetch_devpost(now):
    """Hackathones ONLINE abiertos — port de LifeOS radar scrapeDevpost
    (1 pagina basta para novedades)."""
    out = []
    try:
        data = _get_json("https://devpost.com/api/hackathons?page=1", timeout=15, browser=True)
        for h in (data.get("hackathons") or [])[:20]:
            if h.get("open_state") not in (None, "open", "upcoming"):
                continue
            if h.get("invite_only"):
                continue
            loc = ((h.get("displayed_location") or {}).get("location")) or ""
            if "online" not in loc.lower():
                continue
            prize = re.sub(r"<[^>]+>", "", h.get("prize_amount") or "").strip()
            it = _mk("devpost", h.get("title"), h.get("url"), now)
            if it:
                it["kind"] = "hackathon"
                it["meta"] = {"prize": prize,
                              "participants": h.get("registrations_count"),
                              "deadline": h.get("submission_period_dates")}
                out.append(it)
    except Exception:
        pass
    return out


def fetch_superteam(now):
    """Bounties de Superteam Earn — port del index del Radar (sin el fetch
    de detalle por slug: para NOTICIA basta el listado)."""
    out = []
    try:
        data = _get_json("https://earn.superteam.fun/api/listings", timeout=15, browser=True)
        for it0 in (data if isinstance(data, list) else [])[:20]:
            token = it0.get("token") or "USDC"
            if it0.get("compensationType") == "range":
                prize = f"{it0.get('minRewardAsk')}-{it0.get('maxRewardAsk')} {token}"
            else:
                prize = f"{it0.get('rewardAmount') or '?'} {token}"
            slug = it0.get("slug") or ""
            it = _mk("superteam", it0.get("title"),
                     f"https://earn.superteam.fun/listings/{slug}" if slug else "", now)
            if it:
                it["kind"] = "bounty"
                it["meta"] = {"prize": prize, "deadline": it0.get("deadline")}
                out.append(it)
    except Exception:
        pass
    return out


def fetch_dorahacks(now):
    out = []
    try:
        data = _get_json("https://dorahacks.io/api/hackathon/?page=1", timeout=15, browser=True)
        rows = data.get("results") if isinstance(data, dict) else data
        for h in (rows or [])[:15]:
            end = h.get("end_time") or h.get("deadline")
            if end and int(end) < now:      # epoch SEGUNDOS (nota del Radar)
                continue
            uname = h.get("uname") or h.get("slug") or ""
            it = _mk("dorahacks", h.get("name") or h.get("title"),
                     f"https://dorahacks.io/hackathon/{uname}" if uname else h.get("url"), now)
            if it:
                it["kind"] = "hackathon"
                it["meta"] = {"prize": h.get("bounty_prize") or h.get("prize_pool"),
                              "deadline": end}
                out.append(it)
    except Exception:
        pass
    return out


def _history_append(hooks_dir, items, ts):
    """Historial append-only en SQLite: NADA de data de los API calls se
    pierde — sirve para analytics y deteccion de patrones despues."""
    import sqlite3
    path = os.path.join(os.fspath(hooks_dir), "news-history.sqlite")
    con = sqlite3.connect(path, timeout=10)
    try:
        con.execute("""create table if not exists events(
            url text primary key, source text not null, kind text not null,
            title text not null, at integer not null,
            first_seen integer not null, last_seen integer not null,
            meta text not null default '{}')""")
        for it in items:
            con.execute("""insert into events(url,source,kind,title,at,first_seen,last_seen,meta)
                values(?,?,?,?,?,?,?,?)
                on conflict(url) do update set last_seen=excluded.last_seen,
                  title=excluded.title, meta=excluded.meta""",
                (it["url"], it["source"], it["kind"], it["title"], it["at"],
                 ts, ts, json.dumps(it.get("meta") or {}, ensure_ascii=False)))
        con.commit()
        return con.execute("select count(*) from events").fetchone()[0]
    finally:
        con.close()


def watch_news(hooks_dir, now=None, max_age_days=7):
    ts = int(now if now is not None else time.time())
    snap_path = os.path.join(os.fspath(hooks_dir), "news-watch.json")
    try:
        with open(snap_path) as fh:
            prev = json.load(fh)
    except (OSError, ValueError):
        prev = {}
    seen = set(prev.get("seen") or [])
    first_run = not prev
    items = []
    for fn in (fetch_searxng, fetch_hn, fetch_reddit, fetch_github,
               fetch_devpost, fetch_superteam, fetch_dorahacks):
        items.extend(fn(ts))
    cutoff = ts - max_age_days * 86400
    fresh, kept_urls = [], set()
    for it in items:
        if it["url"] in kept_urls or it["at"] < cutoff:
            continue
        kept_urls.add(it["url"])
        if it["url"] not in seen:
            fresh.append(it)
    seen |= kept_urls
    # lo NUEVO de cada ciclo se acumula en 'recent' (el panel lo muestra).
    # Primer arranque: siembra el panel con lo de las ultimas 48 h (sin
    # popup — news va vacio), no con 40 noticias viejas ni con nada.
    if first_run:
        recent = [it for it in fresh if it["at"] >= ts - 48 * 3600][:10]
    else:
        recent = (prev.get("recent") or []) + fresh
    recent = sorted(recent, key=lambda x: -x["at"])[:40]
    try:
        history_count = _history_append(hooks_dir, items, ts)
    except Exception:
        history_count = None
    snap = {"checkedAt": ts, "recent": recent,
            "seen": sorted(seen)[-800:],
            "historyCount": history_count,
            "sources": {"searxng": SEARX, "note": "patrón Radar de LifeOS"}}
    tmp = snap_path + ".tmp"
    with open(tmp, "w") as fh:
        json.dump(snap, fh, indent=1)
    os.replace(tmp, snap_path)
    return {"news": [] if first_run else fresh, "snapshot": snap}
