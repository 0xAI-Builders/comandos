import re
from pathlib import Path

CSS = Path("dash/analytics.css")


def test_analytics_css_is_scoped_and_ships_its_fonts():
    css = CSS.read_text()
    body = re.sub(r"@font-face\{[^}]*\}", "", css)
    body = re.sub(r"@keyframes[^{]*\{(?:[^{}]*\{[^}]*\})*[^}]*\}", "", body)
    selectors = []
    for head in re.findall(r"([^{}]+)\{", body):
        head = head.strip()
        if head.startswith(("@media", "@supports", "/*")) or not head:
            continue
        selectors += [s.strip() for s in head.split(",")]
    assert selectors and all(s == ".an" or s.startswith((".an ", ".an.", ".an,")) for s in selectors), \
        [s for s in selectors if not (s == ".an" or s.startswith((".an ", ".an.", ".an,")))][:5]
    assert "--sans:'ComandOS Inter','Inter'" in css
    for font in ("assets/fonts/Inter/Inter-latin.woff2", "assets/fonts/UbuntuSansMono/UbuntuSansMono-latin.woff2"):
        assert font in css and Path(font).stat().st_size > 10_000


def test_svg_labels_use_the_bundled_mono_font():
    # Las etiquetas de las botellas son <text font-family="Ubuntu Sans Mono,…">: el atributo pide la fuente de
    # Google del mockup; en el tablero se llama «ComandOS Ubuntu Sans Mono» y sin esta regla cae en otra fuente.
    css = Path("dash/analytics.css").read_text()
    assert '.an svg text[font-family^="Ubuntu Sans Mono"]{font-family:var(--mono)}' in css


def test_phone_keeps_the_mockup_phone_frame():
    # En el mockup, «Vista celular» mete Analytics en un marco de 390 px con relleno 14/12/20.
    css = Path("dash/analytics.css").read_text()
    assert ".an.phone{max-width:390px;padding:14px 12px 20px}" in css
    html = Path("dash/index.html").read_text()
    assert "@media (max-width:600px){html[data-only-panel=\"usage\"] body.only-panel #usage{padding:0!important}}" in html
