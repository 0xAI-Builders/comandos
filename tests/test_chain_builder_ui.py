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
    builder = html.index('<script src="/chain-builder.js?v=1"></script>')
    assert builder > sidebar
    start = html.index("function mountChainBuilder(")
    body = html[start:html.index("\n}\n", start)]
    assert "if(ONLY_PANEL) return;" in body
    assert "ComandosChainBuilder.createChainBuilder(" in body
    assert "commandSidebar.state.catalog" in body and "commandSidebar.state.chains" in body
    assert "commandSidebar.refresh()" in body and "commandSidebar.startChain(" in body
    assert "mountChainBuilder();" in html[html.index("function mountCommandSidebar("):start]
