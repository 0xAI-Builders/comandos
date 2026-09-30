"""Catálogo curado de comandos por CLI, verificado contra el binario instalado.

El catálogo nunca teclea nada: solo describe. Los arranques yolo salen de
config/agent-roles.json; las cuentas, de los directorios reales de cuentas.
"""
from __future__ import annotations
import json, os, re, subprocess
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
CATALOG_FILE = REPO_ROOT / "config" / "cli-commands.json"
_VERSION_RE = re.compile(r"\d+\.\d+[\w.-]*")


class CatalogError(ValueError):
    pass


def load_catalog(path=None):
    p = Path(path) if path else CATALOG_FILE
    with open(p, encoding="utf-8") as fh:
        cat = json.load(fh)
    if not isinstance(cat, dict) or cat.get("version") != 1 or not isinstance(cat.get("clis"), list):
        raise CatalogError("catálogo inválido: se esperaba {version: 1, clis: [...]}")
    seen = set()
    for cli in cat["clis"]:
        for key in ("id", "label", "binary", "pinnedVersion", "launch", "groups"):
            if key not in cli:
                raise CatalogError(f"cli sin '{key}': {cli.get('id')}")
        if cli["id"] in seen:
            raise CatalogError(f"cli repetido: {cli['id']}")
        seen.add(cli["id"])
        for grp in cli["groups"]:
            for cmd in grp.get("commands", []):
                if not cmd.get("text") or not cmd.get("description") or "\n" in cmd["text"]:
                    raise CatalogError(f"comando inválido en {cli['id']}: {cmd}")
    return cat


def version_status(pinned, installed, verified=True):
    if installed is None:
        return "missing"
    if not verified:
        return "unverified"
    return "ok" if str(installed) == str(pinned) else "drift"


def installed_versions(catalog, which=None, run=subprocess.run):
    if which is None:
        from providers import which as _which
        which = _which
    out = {}
    for cli in catalog["clis"]:
        exe = which(cli["binary"])
        if not exe:
            out[cli["id"]] = None
            continue
        try:
            r = run([exe, "--version"], capture_output=True, text=True, timeout=15)
            m = _VERSION_RE.search((r.stdout or "") + (r.stderr or ""))
            out[cli["id"]] = m.group(0) if m else "?"
        except (subprocess.SubprocessError, OSError):
            out[cli["id"]] = "?"
    return out


# ---- comandos detectados en el binario instalado ----
# Un comando `/x` del catálogo solo se muestra si `/x` aparece literalmente en el
# ejecutable real del CLI (los cinco embeben sus slash-commands como cadenas).
# Sin binario legible no se afirma nada y el catálogo curado se muestra entero.
_DETECT_CACHE = {}          # cli_id -> ((path, mtime_ns, size), frozenset)
_NATIVE_GLOB = "../../{name}-*/vendor/*/bin/{name}"


def native_binary(exe):
    """Ruta del ejecutable real: resuelve enlaces y los envoltorios de node
    (`@openai/codex/bin/codex.js` -> `@openai/codex-linux-x64/vendor/.../bin/codex`)."""
    if not exe:
        return None
    try:
        real = os.path.realpath(exe)
        with open(real, "rb") as fh:
            head = fh.read(4)
    except OSError:
        return None
    if head == b"\x7fELF":
        return real
    name = os.path.basename(real).split(".")[0]
    import glob
    hits = sorted(glob.glob(os.path.join(os.path.dirname(real), _NATIVE_GLOB.format(name=name))))
    for h in hits:
        try:
            with open(h, "rb") as fh:
                if fh.read(4) == b"\x7fELF":
                    return os.path.realpath(h)
        except OSError:
            continue
    return None


def _catalog_slash_names(cli):
    return {c["text"].split()[0] for g in cli["groups"] for c in g.get("commands", [])
            if c.get("text", "").startswith("/")}


def detected_commands(catalog, which=None):
    """{cli_id: {'/model', ...}} con los comandos del catálogo presentes en el
    binario; None cuando el CLI no está o su ejecutable no se puede leer."""
    if which is None:
        from providers import which as _which
        which = _which
    out = {}
    for cli in catalog["clis"]:
        path = native_binary(which(cli["binary"]))
        if not path:
            out[cli["id"]] = None
            continue
        try:
            st = os.stat(path)
            key = (path, st.st_mtime_ns, st.st_size)
        except OSError:
            out[cli["id"]] = None
            continue
        hit = _DETECT_CACHE.get(cli["id"])
        if hit and hit[0] == key:
            out[cli["id"]] = set(hit[1])
            continue
        try:
            with open(path, "rb") as fh:
                data = fh.read()
        except OSError:
            out[cli["id"]] = None
            continue
        found = frozenset(n for n in _catalog_slash_names(cli) if n.encode() in data)
        _DETECT_CACHE[cli["id"]] = (key, found)
        out[cli["id"]] = set(found)
    return out


def _launches(cli, danger, accounts):
    b, resume = cli["binary"], cli["launch"].get("resume", "--resume <id>")
    yolo, normal = [], [f"{b}", f"{b} {resume}"]
    if danger:
        yolo = [f"{b} {danger}", f"{b} {danger} {resume}"]
    for extra in cli["launch"].get("extra", []):
        text = extra.replace("{bin}", b).replace("{danger}", danger or "").replace("{resume}", resume)
        (yolo if danger and "{danger}" in extra else normal).append(re.sub(r"\s{2,}", " ", text).strip())
    for acc in accounts.get(cli["id"], []):
        env = " ".join(f"{k}={v}" for k, v in sorted(acc.get("env", {}).items()))
        if env:
            normal.append(f"{env} {b} {danger}".strip() if danger else f"{env} {b}")
    return {"yolo": yolo, "normal": normal, "yoloNote": cli["launch"].get("yoloNote", "")}


def _command_view(cli_id, cmd, models, new_models):
    args = list(cmd.get("args", []))
    out = {"text": cmd["text"], "description": cmd["description"], "args": args}
    if cmd.get("argsFrom") == "models":
        live = list((models or {}).get(cli_id) or [])
        if live:                       # sin datos del watcher quedan los args curados
            out["args"] = live
            fresh = set((new_models or {}).get(cli_id) or [])
            new = [a for a in live if a in fresh]
            if new:
                out["newArgs"] = new
    return out


def catalog_view(catalog, *, danger_flags, versions, accounts, models=None, new_models=None, detected=None):
    """`models` = {cli: [ids]} (más nuevo por familia, del watcher) y
    `new_models` = {cli: [ids]} (newSince del watcher). `detected` = salida de
    detected_commands(): con un conjunto, solo quedan los `/x` presentes en el
    binario; con None (o sin `detected`) el catálogo va entero. Todos opcionales."""
    clis = []
    for cli in catalog["clis"]:
        installed = versions.get(cli["id"])
        found = (detected or {}).get(cli["id"]) if detected is not None else None
        keep = lambda c: found is None or not c["text"].startswith("/") or c["text"].split()[0] in found
        groups = [{"title": g["title"], "icon": g.get("icon", ""),
                   "commands": [_command_view(cli["id"], c, models, new_models)
                                for c in g.get("commands", []) if keep(c)]} for g in cli["groups"]]
        view = {
            "id": cli["id"], "label": cli["label"], "binary": cli["binary"],
            "version": {"pinned": cli["pinnedVersion"], "installed": installed,
                        "status": version_status(cli["pinnedVersion"], installed, cli.get("verified", True))},
            "launch": _launches(cli, danger_flags.get(cli["id"], ""), accounts),
            "groups": groups,
        }
        if detected is not None:
            total = len(_catalog_slash_names(cli))
            n = sum(len(g["commands"]) for g in groups) if found is not None else None
            view["detected"] = {"found": n, "total": total} if found is not None else None
            view["detectedMissing"] = (total - n) if found is not None else 0
        clis.append(view)
    return {"version": 1, "clis": clis}
