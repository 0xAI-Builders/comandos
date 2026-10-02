#!/usr/bin/env python3
"""Genera dash/analytics.css desde el <style> del mockup aprobado de Analytics.

Cada regla queda bajo la raíz `.an`, así no toca el resto del tablero:
`:root`, `html,body` y `.modal` pasan a ser la propia raíz; `.phone` es una clase
de la raíz; lo que solo existe en el mockup (cabecera falsa, selector de diseños)
se descarta. Las fuentes se renombran a las que sirve el tablero (assets/fonts).

Uso: python3 tools/analytics_scope_css.py /tmp/proto.html > dash/analytics.css
"""
import re
import sys

ROOT = ".an"
MOCK_ONLY = re.compile(r"^(\*|\.app\b|\.hdr\b|\.picker\b|\.grip\b|\.states\b|\.desc\b)")
FONT_FACES = """@font-face{font-family:'ComandOS Inter';font-style:normal;font-weight:400 800;font-display:swap;src:url('assets/fonts/Inter/Inter-latin.woff2') format('woff2')}
@font-face{font-family:'ComandOS Ubuntu Sans Mono';font-style:normal;font-weight:400 700;font-display:swap;src:url('assets/fonts/UbuntuSansMono/UbuntuSansMono-latin.woff2') format('woff2')}
"""


def scope_selector(sel):
    sel = sel.strip()
    if sel in (":root", "html", "body"):
        return ROOT
    if sel == "*":
        return f"{ROOT},{ROOT} *"
    if MOCK_ONLY.match(sel):
        return None
    if sel.startswith(".modal"):
        return ROOT + sel[len(".modal"):]
    if sel.startswith(".phone"):
        return ROOT + sel
    return f"{ROOT} {sel}"


def block_end(css, open_at):
    depth, k = 1, open_at + 1
    while depth:
        depth += {"{": 1, "}": -1}.get(css[k], 0)
        k += 1
    return k


def scope(css):
    out, i = [], 0
    while i < len(css):
        j = css.find("{", i)
        if j < 0:
            break
        head = css[i:j].strip()
        if head.startswith("@media") or head.startswith("@supports"):
            k = block_end(css, j)
            out.append(f"{head}{{{scope(css[j + 1:k - 1])}}}\n")
            i = k
            continue
        if head.startswith("@keyframes"):
            k = block_end(css, j)
            out.append(css[i:k].strip() + "\n")
            i = k
            continue
        k = css.find("}", j)
        sels = [s for s in (scope_selector(x) for x in head.split(",")) if s]
        if sels:
            out.append(",".join(sels) + "{" + css[j + 1:k].strip() + "}\n")
        i = k + 1
    return "".join(out)


def main(path):
    html = open(path, encoding="utf-8").read()
    css = html[html.index("<style>") + 7:html.index("</style>")]
    css = re.sub(r"/\*.*?\*/", "", css, flags=re.S)
    css = css.replace("--sans:'Inter',", "--sans:'ComandOS Inter','Inter',")
    css = css.replace("--mono:'Ubuntu Sans Mono',", "--mono:'ComandOS Ubuntu Sans Mono','Ubuntu Sans Mono',")
    sys.stdout.write("/* GENERADO por tools/analytics_scope_css.py desde el mockup aprobado (rama prototype/analytics-grill). No editar a mano. */\n")
    sys.stdout.write(FONT_FACES)
    sys.stdout.write(scope(css))
    # «Vista celular» del mockup: el marco de teléfono (.app.phone-frame .modal) es el celular de verdad.
    sys.stdout.write(".an.phone{max-width:390px;padding:14px 12px 20px}\n")
    # El SVG trae la fuente como atributo (font-family="Ubuntu Sans Mono,…"); en CSS manda la del tablero.
    sys.stdout.write('.an svg text[font-family^="Ubuntu Sans Mono"]{font-family:var(--mono)}\n')


if __name__ == "__main__":
    main(sys.argv[1])
