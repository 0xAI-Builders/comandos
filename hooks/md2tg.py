#!/usr/bin/env python3
"""markdown (stdin) -> HTML de Telegram (stdout).

Telegram solo acepta un subset de HTML: <b> <i> <u> <code> <pre> <a>.
Las tablas van como <pre> con columnas alineadas (Telegram usa monoespaciada).
Salida capada a 3500 chars (limite de sendMessage: 4096, menos el titulo).
"""
import html
import re
import sys


def fmt_table(rows):
    parsed = []
    for r in rows:
        cells = [re.sub(r"\*\*(.+?)\*\*", r"\1", c.strip()).replace("`", "")
                 for c in r.strip().strip("|").split("|")]
        if all(re.match(r"^[\s\-:]*$", c) for c in cells):
            continue  # fila separadora |---|---|
        parsed.append(cells)
    if not parsed:
        return ""
    ncol = max(len(p) for p in parsed)
    w = [max((len(p[i]) if i < len(p) else 0) for p in parsed) for i in range(ncol)]
    lines = ["  ".join((p[i] if i < len(p) else "").ljust(w[i])
                       for i in range(ncol)).rstrip() for p in parsed]
    return "<pre>" + html.escape("\n".join(lines)) + "</pre>"


def inline(s):
    s = html.escape(s)
    s = re.sub(r"\*\*(.+?)\*\*", r"<b>\1</b>", s)
    s = re.sub(r"(^|[\s(])\*([^*\n]+)\*(?=$|[\s).,;:!?])", r"\1<i>\2</i>", s)
    s = re.sub(r"`([^`]+)`", r"<code>\1</code>", s)
    s = re.sub(r"\[([^\]]+)\]\((https?://[^)\s]+)\)", r'<a href="\2">\1</a>', s)
    return s


def main():
    out, code_buf, tbl, in_code = [], [], [], False
    for line in sys.stdin.read().splitlines():
        if line.strip().startswith("```"):
            if tbl:
                out.append(fmt_table(tbl))
                tbl = []
            if in_code:
                out.append("<pre>" + html.escape("\n".join(code_buf)) + "</pre>")
                code_buf = []
            in_code = not in_code
            continue
        if in_code:
            code_buf.append(line)
            continue
        if re.match(r"^\s*\|.*\|\s*$", line):
            tbl.append(line)
            continue
        if tbl:
            out.append(fmt_table(tbl))
            tbl = []
        m = re.match(r"^#{1,6} (.*)", line)
        if m:
            out.append("<b>" + inline(m.group(1)) + "</b>")
            continue
        if re.match(r"^\s*---+\s*$", line):
            out.append("————————")
            continue
        line = re.sub(r"^(\s*)[-*] ", "\\1• ", line)
        out.append(inline(line))
    if in_code and code_buf:
        out.append("<pre>" + html.escape("\n".join(code_buf)) + "</pre>")
    if tbl:
        out.append(fmt_table(tbl))
    sys.stdout.write("\n".join(out)[:3500])


if __name__ == "__main__":
    main()
