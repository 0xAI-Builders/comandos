"""Watcher de modelos: descubre los modelos MAS RECIENTES de cada CLI local.

Fuentes (subscription-only, cero API keys):
  - claude: strings del binario versionado (~/.local/share/claude/versions/N)
  - codex:  strings del binario vendor (los ids del picker viajan en el binario)
  - grok:   `grok models` (salida de texto del CLI)

El watcher NUNCA edita config/providers.json solo: reporta novedades para que
el humano decida. Snapshot durable en HOOKS/model-watch.json.
"""
from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import time

try:
    from providers import which as _which   # shutil.which + ~/.local/bin, ~/.bun/bin…
except Exception:  # pragma: no cover
    _which = shutil.which

_ANSI = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")

# Formas canonicas: solo ids que un humano reconoceria como modelo completo.
_CLAUDE_RE = re.compile(r"claude-(?:fable|opus|sonnet|haiku|mythos)-\d(?:-\d)?"
                        r"(?:-\d{8})?(?:\[1m\])?$")
# Sufijo con nombre abierto (sol, luna, terra, astra, mini, max…): OpenAI
# bautiza cada generación y una lista cerrada se queda ciega (Astra no cabía).
_CODEX_RE = re.compile(r"gpt-\d+(?:\.\d+)?(?:-codex)?(?:-[a-z]{2,12})?$")
_GROK_RE = re.compile(r"grok-[\w.]+$")


def _run(cmd, timeout=20, env=None):
    try:
        r = subprocess.run(cmd, capture_output=True, text=True,
                           timeout=timeout, env=env, stdin=subprocess.DEVNULL)
        return _ANSI.sub("", (r.stdout or "") + (r.stderr or ""))
    except Exception:
        return ""


def installed_versions():
    out = {}
    for prov, cmd in (("claude", ["claude", "--version"]),
                      ("codex", ["codex", "--version"]),
                      ("grok", ["grok", "--version"])):
        exe = _which(cmd[0])
        if exe:
            # ejecutar la RUTA resuelta: bajo systemd `codex` a pelo no está en PATH
            m = re.search(r"\d+\.\d+[\w.-]*", _run([exe] + cmd[1:], timeout=15))
            out[prov] = m.group(0) if m else "?"
    return out


def _binary_ids(path, pattern, canon):
    ids = set()
    try:
        with open(path, "rb") as fh:
            data = fh.read()
    except OSError:
        return ids
    for m in re.finditer(pattern, data):
        cand = m.group(0).decode(errors="replace")
        if canon.fullmatch(cand):
            ids.add(cand)
    return ids


def _claude_binary():
    base = os.path.expanduser("~/.local/share/claude/versions")
    try:
        versions = sorted(os.listdir(base), key=lambda v: [int(x) for x in re.findall(r"\d+", v)])
        return os.path.join(base, versions[-1]) if versions else ""
    except OSError:
        real = _which("claude")
        return os.path.realpath(real) if real else ""


def _codex_binary():
    real = _which("codex")
    if not real:
        return ""
    root = os.path.dirname(os.path.dirname(os.path.realpath(real)))
    for cand in (os.path.join(os.path.dirname(root), "codex-linux-x64", "vendor",
                              "x86_64-unknown-linux-musl", "bin", "codex"),):
        if os.path.isfile(cand):
            return cand
    return os.path.realpath(real)


def discover_models(grok_home=None):
    found = {"claude": set(), "codex": set(), "grok": set()}
    cb = _claude_binary()
    if cb:
        found["claude"] = _binary_ids(
            cb, rb"claude-(?:fable|opus|sonnet|haiku|mythos)-[\w.\[\]-]{1,24}", _CLAUDE_RE)
    xb = _codex_binary()
    if xb:
        found["codex"] = _binary_ids(xb, rb"gpt-\d[\w.-]{1,24}", _CODEX_RE)
    grok_exe = _which("grok")
    if grok_exe:
        env = dict(os.environ)
        if grok_home:
            env["GROK_HOME"] = os.fspath(grok_home)
        out = _run([grok_exe, "models"], timeout=25, env=env)
        for line in out.splitlines():
            m = re.match(r"\s*[-*]\s+([\w.-]+)", line)
            if m and _GROK_RE.fullmatch(m.group(1)):
                found["grok"].add(m.group(1))
    # SIN heuristicas de recorte aqui: fable-5 y fable-5-1 COEXISTEN como
    # modelos reales (igual que opus-4 y opus-4-1) — cualquier regla de
    # "parece substring" mata modelos legitimos. El ruido de versiones
    # viejas lo absorbe el techo por familia en watch_models().
    return {k: sorted(v) for k, v in found.items()}


def discover_addons():
    """Nombres de SKILLS y MCPs configurados por cuenta (solo nombres,
    jamas comandos/env/urls). Para avisar altas/bajas en el centro de
    notificaciones."""
    skills = {}
    for label, base in (("claude", "~/.claude/skills"),
                        ("claude:relotto", "~/.claude-accounts/relotto/skills"),
                        ("grok", "~/.grok/skills")):
        try:
            names = sorted(d for d in os.listdir(os.path.expanduser(base))
                           if not d.startswith("."))
        except OSError:
            names = []
        if names:
            skills[label] = names
    try:
        names = sorted(f[:-3] for f in os.listdir(os.path.expanduser("~/.codex/prompts"))
                       if f.endswith(".md"))
        if names:
            skills["codex"] = names
    except OSError:
        pass
    mcps = {}
    try:
        cfg = json.load(open(os.path.expanduser("~/.claude.json")))
        names = sorted((cfg.get("mcpServers") or {}).keys())
        if names:
            mcps["claude"] = names
    except Exception:
        pass
    try:
        import re as _re
        toml = open(os.path.expanduser("~/.codex/config.toml")).read()
        names = sorted(set(_re.findall(r"^\[mcp_servers\.([\w-]+)\]", toml, _re.M)))
        if names:
            mcps["codex"] = names
    except Exception:
        pass
    try:
        toml = open(os.path.expanduser("~/.grok/config.toml")).read()
        import re as _re
        names = sorted(set(_re.findall(r"^\[mcp_servers\.([\w-]+)\]", toml, _re.M)))
        if names:
            mcps["grok"] = names
    except Exception:
        pass
    return {"skills": skills, "mcps": mcps}


def registry_model_ids(registry_path):
    try:
        with open(registry_path) as fh:
            reg = json.load(fh)
    except (OSError, ValueError):
        return {}
    out = {}
    for motor, spec in (reg.get("motors") or {}).items():
        out[motor] = {m.get("id", "") for m in spec.get("models") or []}
    return out


def _norm(mid):
    return re.sub(r"\[.*$", "", re.sub(r"-\d{8}$", "", mid.lower()))


def _family_ver(mid):
    """('opus', (4,8)) / ('gpt', (5,6)) / ('grok', (4,6)) — para comparar
    novedad REAL: solo interesa lo mas nuevo que lo ya registrado."""
    n = _norm(mid)
    m = re.match(r"claude-([a-z]+)-(\d+(?:-\d+)*)", n)
    if m:
        return m.group(1), tuple(int(x) for x in m.group(2).split("-"))
    m = re.match(r"(gpt)-(\d+(?:\.\d+)?)", n)
    if m:
        return m.group(1), tuple(int(x) for x in m.group(2).split("."))
    m = re.match(r"(grok)-(\d+(?:\.\d+)?)", n)
    if m:
        return m.group(1), tuple(int(x) for x in m.group(2).split("."))
    return n, ()


def watch_models(hooks_dir, registry_path, grok_home=None, now=None):
    """Un ciclo del watcher: descubre, compara contra registry + snapshot
    previo, persiste, y devuelve las NOVEDADES (para notificar)."""
    ts = int(now if now is not None else time.time())
    snap_path = os.path.join(os.fspath(hooks_dir), "model-watch.json")
    try:
        with open(snap_path) as fh:
            prev = json.load(fh)
    except (OSError, ValueError):
        prev = {}
    versions = installed_versions()
    discovered = discover_models(grok_home)
    registry = registry_model_ids(registry_path)
    reg_norm = {p: {_norm(m) for m in ids} for p, ids in registry.items()}
    prev_seen = {p: set(v) for p, v in (prev.get("discovered") or {}).items()}
    # techo por familia segun el registry: solo es NOTICIA lo que supera
    # (o estrena familia); los modelos viejos embebidos en el binario no.
    reg_ceiling = {}
    for prov, ids in registry.items():
        for mid in ids:
            fam, ver = _family_ver(mid)
            key = (prov, fam)
            if ver > reg_ceiling.get(key, ()):  # tuple compare
                reg_ceiling[key] = ver
    news = {}
    pending = {}
    for prov, ids in discovered.items():
        best_per_family = {}
        for m in ids:
            if _norm(m) in reg_norm.get(prov, set()):
                continue
            fam, ver = _family_ver(m)
            ceil = reg_ceiling.get((prov, fam))
            if ceil is not None and ver <= ceil:
                continue
            cur = best_per_family.get(fam)
            if cur is None or ver > cur[0]:
                best_per_family[fam] = (ver, m)
        pend = sorted(m for _v, m in best_per_family.values())
        if pend:
            pending[prov] = pend
        fresh = sorted(m for m in pend if m not in prev_seen.get(prov, set()))
        if fresh:
            news[prov] = fresh
    # newSince se RE-deriva cada ciclo (nada de arrastres viejos): es lo que
    # HOY supera al registry; se vacia solo cuando el humano lo integra.
    addons = discover_addons()
    prev_addons = prev.get("addons") or {}
    addon_news = {}
    for kind in ("skills", "mcps"):
        added = {}
        for owner, names in (addons.get(kind) or {}).items():
            before = set((prev_addons.get(kind) or {}).get(owner) or [])
            plus = [n for n in names if n not in before]
            if plus and prev_addons:      # primer snapshot no es noticia
                added[owner] = plus
        if added:
            addon_news[kind] = {"added": added, "at": ts}
    snap = {"checkedAt": ts, "versions": versions, "discovered": discovered,
            "addons": addons,
            "addonNews": addon_news or (prev.get("addonNews") if not prev_addons else {}),
            "newSince": {p: {"models": ms, "at": ts, "cli": versions.get(p, "?")}
                         for p, ms in pending.items()}}
    tmp = snap_path + ".tmp"
    with open(tmp, "w") as fh:
        json.dump(snap, fh, indent=1)
    os.replace(tmp, snap_path)
    return {"news": news, "snapshot": snap}
