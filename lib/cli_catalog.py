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


def catalog_view(catalog, *, danger_flags, versions, accounts, models=None, new_models=None):
    """`models` = {cli: [ids]} (más nuevo por familia, del watcher) y
    `new_models` = {cli: [ids]} (newSince del watcher). Ambos opcionales."""
    clis = []
    for cli in catalog["clis"]:
        installed = versions.get(cli["id"])
        clis.append({
            "id": cli["id"], "label": cli["label"], "binary": cli["binary"],
            "version": {"pinned": cli["pinnedVersion"], "installed": installed,
                        "status": version_status(cli["pinnedVersion"], installed, cli.get("verified", True))},
            "launch": _launches(cli, danger_flags.get(cli["id"], ""), accounts),
            "groups": [{"title": g["title"], "icon": g.get("icon", ""),
                        "commands": [_command_view(cli["id"], c, models, new_models)
                                     for c in g.get("commands", [])]} for g in cli["groups"]],
        })
    return {"version": 1, "clis": clis}
