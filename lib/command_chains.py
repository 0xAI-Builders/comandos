"""Cadenas de comandos: archivos Markdown que el usuario puede editar a mano."""
from __future__ import annotations
import os, re, unicodedata
from pathlib import Path

KINDS = ("shell", "pane")
_STEP_RE = re.compile(r"^\s*(?:\d+[.)]|[-*])\s*(\w+)\s*:\s*(.+?)\s*$")
_SLUG_RE = re.compile(r"^[a-z0-9][a-z0-9-]{0,59}$")


class ChainError(ValueError):
    pass


def default_dir():
    base = os.environ.get("XDG_CONFIG_HOME") or os.path.join(os.path.expanduser("~"), ".config")
    return Path(base) / "comandos" / "cadenas"


def slugify(name):
    s = unicodedata.normalize("NFKD", str(name or "")).encode("ascii", "ignore").decode()
    s = re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-")[:60].strip("-")
    return s or "cadena"


def _has_control(text):
    # Misma regla que lib/pane_typing.py: ningún carácter de control ni DEL.
    return any(ord(ch) < 32 or ord(ch) == 127 for ch in text)


def _check_steps(steps):
    if not isinstance(steps, list) or not steps:
        raise ChainError("La cadena necesita al menos un paso")
    out = []
    for i, st in enumerate(steps, 1):
        st = st if isinstance(st, dict) else {}
        kind, text = st.get("kind"), str(st.get("text") or "")
        if kind not in KINDS:
            raise ChainError(f"Paso inválido {i}: tipo '{kind}' (usa shell o pane)")
        if not text.strip() or _has_control(text):
            raise ChainError(f"Paso inválido {i}: texto vacío o con saltos de línea o caracteres de control")
        out.append({"kind": kind, "text": text.strip()})
    return out


def _check_name(name):
    name = str(name or "").strip()
    if not name or _has_control(name):
        raise ChainError("Nombre inválido: vacío o con saltos de línea")
    return name


def serialize(name, steps):
    name = _check_name(name)
    steps = _check_steps(steps)
    lines = [f"# {name}", ""]
    lines += [f"{i}. {s['kind']}: {s['text']}" for i, s in enumerate(steps, 1)]
    return "\n".join(lines) + "\n"


def parse(text):
    name, steps = "", []
    for raw in str(text or "").splitlines():
        line = raw.rstrip()
        if not line.strip():
            continue
        if not name and line.startswith("# "):
            name = line[2:].strip()
            continue
        m = _STEP_RE.match(line)
        if not m:
            raise ChainError(f"Paso inválido: {line.strip()!r}")
        steps.append({"kind": m.group(1), "text": m.group(2)})
    return {"name": name, "steps": _check_steps(steps)}


def _path(directory, slug):
    if not _SLUG_RE.match(str(slug or "")):
        raise ChainError(f"Slug inválido: {slug!r}")
    return Path(directory) / f"{slug}.md"


def list_chains(directory):
    directory = Path(directory)
    out = []
    if not directory.is_dir():
        return out
    for p in sorted(directory.glob("*.md"), key=lambda q: q.stem):
        slug = p.stem
        try:
            chain = parse(p.read_text(encoding="utf-8"))
            out.append({"slug": slug, "name": chain["name"] or slug, "steps": chain["steps"]})
        except (ChainError, OSError, UnicodeDecodeError) as exc:
            out.append({"slug": slug, "name": p.name, "error": str(exc)})
    return out


def save_chain(directory, name, steps, slug=None):
    directory = Path(directory)
    body = serialize(name, steps)
    directory.mkdir(parents=True, exist_ok=True)
    if slug is None:
        base = slugify(name)
        slug, n = base, 2
        while _path(directory, slug).exists():
            slug, n = f"{base}-{n}", n + 1
    path = _path(directory, slug)
    tmp = path.with_suffix(".md.tmp")
    tmp.write_text(body, encoding="utf-8")
    os.replace(tmp, path)
    return {"slug": slug, "name": str(name).strip(), "steps": _check_steps(steps)}


def delete_chain(directory, slug):
    path = _path(directory, slug)
    if not path.exists():
        return False
    path.unlink()
    return True
