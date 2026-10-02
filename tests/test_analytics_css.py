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
