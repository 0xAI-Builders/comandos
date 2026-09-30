"""Arranques de cada CLI leídos de su propio `<cli> --help`.

Nada de etiquetas propias: cada fila es un flag o subcomando con el texto exacto
que imprime el CLI, y cada bloque lleva el título de sección que el CLI usa
(«Options», «Commands», «Available subcommands»…). Entiende los formatos de
commander (Claude), clap (Codex, Grok), yargs (OpenCode) y flag de Go (agy).
"""
from __future__ import annotations
import os, re, subprocess

_HELP_CACHE: dict = {}                    # exe -> ((path, mtime_ns, size), parsed)
_ANSI = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")
_SECTION = re.compile(r"^([A-Z][\w ]*?(?: of [\w-]+)?):\s*$")
_SKIP_SECTIONS = {"arguments", "positionals"}         # no se teclean
# Arranque sin permisos, según la propia descripción del CLI («Bypass all permission
# checks», «Skip all confirmation prompts», «Auto-approve all tool executions»…).
# Solo decide el color ámbar y que la fila salga arriba; el texto es el del CLI.
_YOLO = re.compile(r"\b(?:bypass\w*|skip) all (?:permission|confirmation)|\bauto-approve (?:all|permissions)", re.I)
# «Enable bypassing all permission checks as an option, without it being enabled by
# default» (claude --allow-dangerously-skip-permissions) solo lo permite: arranca en
# el modo normal (verificado en un tmux privado: «auto mode on»).
_ONLY_ALLOWS = re.compile(r"\bas an option\b|without it being enabled", re.I)
_VALUES = [
    re.compile(r"\[possible values: ([^\]]+)\]"),
    re.compile(r"[\[(]choices: ([^\])]+)[\])]"),
    re.compile(r"\(([\w-]+(?:\|[\w-]+)+)\)"),          # agy: (low|medium|high|max)
]
_YARGS_TAIL = re.compile(r"(\s*\[(?:boolean|string|number|array|count)\])?(\s*\[(?:default: \[\]|(?:default|choices|aliases)[^\]]*)\])*\s*$")


def _indent(line):
    return len(line) - len(line.lstrip(" "))


def _values(desc, bullets):
    for rx in _VALUES:
        m = rx.search(desc)
        if m:
            vals = [v.strip().strip('"') for v in re.split(r"[,|]", m.group(1)) if v.strip()]
            return [v for v in vals if ":" not in v and " " not in v]     # fuera «default: "host"»
    return bullets


def _item(head, desc_lines, binary, section):
    """head = «-s, --sandbox <MODE>» o «resume» o «opencode run [message..]»."""
    head = head.strip()
    bullets = [m.group(1) for d in desc_lines for m in [re.match(r"^-\s+([\w.-]+)(?::|$)", d.strip())] if m]
    desc = re.sub(r"\s{2,}", " ", " ".join(d.strip() for d in desc_lines if d.strip()))
    if head.startswith("-"):
        names = [n.strip() for n in re.split(r",\s*", re.split(r"\s+[<\[]", head)[0])]
        flag = next((n for n in names if n.startswith("--")), names[0]).split("=")[0]
        takes_arg = bool(re.search(r"[<\[]", head)) or bool(re.search(r"\[(?:string|number|array)\]", desc))
        text = f"{binary} {flag}"
    else:
        words = head.split()
        if words and words[0] == binary:
            words = words[1:]
        if not words or words[0].startswith(("[", "<")):
            text, takes_arg = binary, False            # «opencode [project]» = arranque normal
        else:
            text = f"{binary} {words[0].split('|')[0]}"     # «plugin|plugins»: el primero
            takes_arg = len(words) > 1
    desc_shown = _YARGS_TAIL.sub("", desc).strip() if re.search(r"\[(?:boolean|string|number|array)\]", desc) else desc
    vals = _values(desc, bullets)
    return {"text": text + (" " if takes_arg or vals else ""), "head": head, "description": desc_shown,
            "args": vals, "yolo": bool(_YOLO.search(desc_shown)) and not _ONLY_ALLOWS.search(desc_shown),
            "section": section}


def parse_help(text, binary):
    """{'summary': str, 'sections': [{'title', 'items': [...]}]} en el orden del CLI."""
    lines = _ANSI.sub("", text).replace("\t", "    ").splitlines()
    summary, sections, cur, item = [], [], None, None
    para_done = False

    def flush():
        nonlocal item
        if item and cur is not None:
            head, desc = item
            if not re.match(r"^\s*(help|completions?)\b", head) or head.strip().startswith("-"):
                cur["items"].append(_item(head, desc, binary, cur["title"]))
        item = None

    for raw in lines:
        line = raw.rstrip()
        m = _SECTION.match(line)
        if m and not line.startswith(" "):
            flush()
            title = m.group(1)
            cur = {"title": title, "items": []}
            sections.append(cur)
            continue
        if cur is None:
            s = line.strip()
            if not s:
                para_done = para_done or bool(summary)
            elif not para_done and not s.lower().startswith("usage") and not re.search(r"[█▀▄⠀]", s) \
                    and not s.startswith(binary + " "):
                summary.append(s)                      # primer párrafo de la ayuda
            continue
        if not line.strip():
            continue
        ind = _indent(line)
        s = line.strip()
        starts_item = bool(re.match(r"^--?[A-Za-z0-9]", s)) and (item is None or ind <= item_ind + 4)
        if item is not None and not starts_item and ind > item_ind:
            item[1].append(s)                          # continuación de la descripción
            continue
        if starts_item or ind <= 4:
            flush()
            parts = re.split(r"\s{2,}", s, maxsplit=1)
            item = (parts[0], [parts[1]] if len(parts) > 1 else [])
            item_ind = ind
            continue
        if item is not None:
            item[1].append(s)
    flush()
    out = []
    for sec in sections:
        if sec["title"].lower() in _SKIP_SECTIONS or not sec["items"]:
            continue
        title = sec["title"]
        if title.lower().startswith("usage of"):
            title = "Options"                          # flag de Go no titula la sección; «Usage of agy»
        out.append({"title": title, "items": sec["items"]})
    return {"summary": " ".join(summary).strip(), "sections": out}


def help_for(binary, which=None, run=subprocess.run):
    """`<binary> --help` interpretado, cacheado por (ruta, mtime, tamaño) del ejecutable."""
    if which is None:
        from providers import which as _which
        which = _which
    exe = which(binary)
    if not exe:
        return None
    try:
        real = os.path.realpath(exe)
        st = os.stat(real)
        key = (real, st.st_mtime_ns, st.st_size)
    except OSError:
        return None
    hit = _HELP_CACHE.get(exe)
    if hit and hit[0] == key:
        return hit[1]
    env = dict(os.environ, NO_COLOR="1", TERM="dumb", COLUMNS="100")
    try:
        p = run([exe, "--help"], capture_output=True, text=True, timeout=20, env=env, stdin=subprocess.DEVNULL)
    except (OSError, subprocess.SubprocessError):
        return None
    text = (p.stdout or "") + ("\n" + p.stderr if p.stderr and not p.stdout.strip() else "")
    if not text.strip():
        return None
    parsed = parse_help(text, binary)
    parsed["command"] = f"{binary} --help"
    _HELP_CACHE[exe] = (key, parsed)
    return parsed
