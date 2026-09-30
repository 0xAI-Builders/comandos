"""Runs the Node behaviour checks of dash/chain-builder.js and verifies it ships."""
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]


@pytest.mark.skipif(shutil.which("node") is None, reason="node no disponible")
def test_chain_builder_node_checks():
    subprocess.run(["node", "--check", str(ROOT / "dash" / "chain-builder.js")], check=True)
    out = subprocess.run(["node", str(ROOT / "tests" / "chain_builder_checks.cjs")],
                         cwd=ROOT, capture_output=True, text=True, timeout=30)
    assert out.returncode == 0, out.stderr
    assert out.stdout.strip().endswith("chain-builder checks ok")


def test_install_links_the_module():
    assert "chain-builder.js" in (ROOT / "install.sh").read_text()


def test_builder_never_types_into_a_pane():
    src = (ROOT / "dash" / "chain-builder.js").read_text()
    for forbidden in ("/pane/type", "/send", "send-keys", "Enter"):
        assert forbidden not in src, forbidden


def test_builder_is_loaded_and_mounted_after_the_sidebar():
    html = (ROOT / "dash" / "index.html").read_text(encoding="utf-8")
    sidebar = html.index('<script src="/command-sidebar.js')
    builder = html.index('<script src="/chain-builder.js?v=')
    assert builder > sidebar
    start = html.index("function mountChainBuilder(")
    body = html[start:html.index("\n}\n", start)]
    assert "if(ONLY_PANEL) return;" in body
    assert "ComandosChainBuilder.createChainBuilder(" in body
    assert "commandSidebar.state.catalog" in body and "commandSidebar.state.chains" in body
    assert "commandSidebar.refresh()" in body and "commandSidebar.startChain(" in body
    assert "mountChainBuilder();" in html[html.index("function mountCommandSidebar("):start]


def test_every_icon_used_by_the_sidebar_and_builder_exists_in_the_icon_map():
    import json
    import re
    html = (ROOT / "dash" / "index.html").read_text(encoding="utf-8")
    block = html[html.index("const ICON = {"):html.index("const svg = (name")]
    have = set(re.findall(r'^\s{2}"?([\w-]+)"?\s*:', block, re.M))
    used = set()
    for f in ("command-sidebar.js", "chain-builder.js"):
        used |= set(re.findall(r'data-icon="([\w-]+)"', (ROOT / "dash" / f).read_text()))
    cat = json.loads((ROOT / "config" / "cli-commands.json").read_text())
    used |= {g["icon"] for c in cat["clis"] for g in c["groups"]}
    assert {"brain", "cycle", "key", "map"} <= used
    assert used - have == set(), sorted(used - have)


def test_sidebar_hydrates_icons_after_every_render():
    html = (ROOT / "dash" / "index.html").read_text(encoding="utf-8")
    mount = html[html.index("function mountCommandSidebar("):html.index("function mountChainBuilder(")]
    assert "hydrate: hydrateIcons" in mount


def test_builder_touch_rules_share_the_sidebar_specificity():
    css = (ROOT / "dash" / "workspace.css").read_text()
    # estilos 1:1 del mockup: tokens compartidos, filas y chips con la misma especificidad en barra y modal
    assert "#command-sidebar,.chain-only{--cs-panel:#171b24;" in css
    assert ":is(#command-sidebar,.chain-only) .cmds .cmd{padding-block:10px}" in css
    assert ":is(#command-sidebar,.chain-only) .cmds{border-top:1px solid var(--cs-line2)}" in css
    assert ".chain-only .cmd button[data-flat].add{min-height:32px}" in css


def test_desktop_opens_the_chain_modal_as_a_centered_gtk_window():
    """Mockup aprobado: el modal Cadenas va centrado sobre la app. El tablero del
    escritorio es la columna izquierda, así que la página ?panel=chains se abre
    en una ventana GTK modal centrada y avisa por el puente al cerrar/guardar/correr."""
    html = (ROOT / "dash" / "index.html").read_text(encoding="utf-8")
    page = html[html.index("function mountChainPage("):html.index("function mountChainBuilder(")]
    assert 'ONLY_PANEL !== "chains"' in page
    assert 'onClose: () => tell({chainModal: "close"})' in page
    assert 'tell({chainModal: o && o.run ? "run" : "saved", slug: chain.slug})' in page
    assert ".then(() => window.chainBuilder.open())" in page
    assert 'if(ONLY_PANEL==="chains") return;' in html
    assert 'html[data-only-panel="chains"] body.only-panel > .backdrop[data-mclose]{display:flex!important' in html
    assert ':not(.backdrop){display:none !important}' in html
    app = (ROOT / "bin" / "cc-app").read_text(encoding="utf-8")
    assert 'HEADER_ACTIONS["chains"] = open_chain_modal' in app
    assert 'set_position(Gtk.WindowPosition.CENTER_ON_PARENT)' in app[app.index("def open_chain_modal("):app.index("def _chain_modal_msg(")]
    assert 'if d.get("chainModal") in ("close", "saved", "run"):' in app
    js = (ROOT / "dash" / "chain-builder.js").read_text(encoding="utf-8")
    assert "const userClose = () => { if (close()) onClose(); };" in js
