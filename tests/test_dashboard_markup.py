"""El marcado del tablero no puede cerrar etiquetas en orden cruzado.

Un `</details></div>` donde tocaba `</div></details>` hizo que WebKit cerrara
el panel de Comparar antes de tiempo. Consecuencia: los paneles Reparto y
Alertas quedaron FUERA de .modal-panel, que es el ambito sobre el que
activateMtab busca los paneles, asi que nunca recibian .active y no se veian
nunca; y el bloque de calificaciones se salio de su panel y quedaba visible con
cualquier pestana. El sintoma era una pestana que se marca al pulsarla y no
muestra nada, que es dificil de atribuir al marcado.
"""
from html.parser import HTMLParser
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HTML = (ROOT / "dash/index.html").read_text()

# Elementos sin etiqueta de cierre: no entran en la pila.
VOID = {"area", "base", "br", "col", "embed", "hr", "img", "input", "link",
        "meta", "param", "source", "track", "wbr"}
# Elementos cuyo cierre el parser de HTML puede inferir; no se vigilan.
OPTIONAL = {"p", "li", "tr", "td", "th", "thead", "tbody", "tfoot", "option",
            "dt", "dd", "rt", "rp", "optgroup", "colgroup", "caption"}
WATCHED = {"div", "details", "section", "nav", "span", "button", "label", "table", "form"}


class Nesting(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.stack = []
        self.crossed = []

    def handle_starttag(self, tag, attrs):
        if tag not in VOID:
            self.stack.append((tag, self.getpos()[0]))

    def handle_startendtag(self, tag, attrs):
        pass

    def handle_endtag(self, tag):
        if tag in VOID:
            return
        for depth in range(len(self.stack) - 1, -1, -1):
            if self.stack[depth][0] != tag:
                continue
            skipped = [t for t, _ in self.stack[depth + 1:]
                       if t in WATCHED and t not in OPTIONAL]
            if skipped:
                self.crossed.append((tag, self.getpos()[0], skipped))
            del self.stack[depth:]
            return


def _parse():
    p = Nesting()
    p.feed(HTML)
    return p


def test_ninguna_etiqueta_se_cierra_en_orden_cruzado():
    p = _parse()
    detalle = "; ".join(f"</{t}> en linea {ln} deja abiertos {sk}" for t, ln, sk in p.crossed)
    assert not p.crossed, f"cierre cruzado en dash/index.html: {detalle}"


def test_cada_panel_de_pestana_vive_dentro_de_su_modal_panel():
    """activateMtab busca los paneles dentro de .modal-panel: uno que caiga fuera
    se marca en la pestana y nunca se muestra."""
    import re
    depth_modal = None
    stack = []
    fuera = []
    for n, line in enumerate(HTML.split("\n"), 1):
        for m in re.finditer(r'<(/?)(\w+)([^>]*)>', line):
            close, tag, attrs = m.group(1), m.group(2).lower(), m.group(3)
            if tag in VOID or m.group(0).endswith("/>"):
                continue
            if close:
                if stack and tag in [t for t, _ in stack]:
                    for d in range(len(stack) - 1, -1, -1):
                        if stack[d][0] == tag:
                            if depth_modal is not None and d <= depth_modal:
                                depth_modal = None
                            del stack[d:]
                            break
                continue
            if "modal-panel" in attrs:
                depth_modal = len(stack)
            if "data-mpane=" in attrs and depth_modal is None:
                fuera.append((re.search(r'data-mpane="([^"]+)"', attrs).group(1), n))
            stack.append((tag, n))
    assert not fuera, f"paneles fuera de .modal-panel: {fuera}"
