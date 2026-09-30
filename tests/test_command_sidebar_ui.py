"""Runs the Node behaviour checks of dash/command-sidebar.js and verifies it ships."""
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]


@pytest.mark.skipif(shutil.which("node") is None, reason="node no disponible")
def test_command_sidebar_node_checks():
    subprocess.run(["node", "--check", str(ROOT / "dash" / "command-sidebar.js")], check=True)
    out = subprocess.run(["node", str(ROOT / "tests" / "command_sidebar_checks.cjs")],
                         cwd=ROOT, capture_output=True, text=True, timeout=30)
    assert out.returncode == 0, out.stderr
    assert out.stdout.strip().endswith("command-sidebar checks ok")


def test_install_links_the_module():
    assert "command-sidebar.js" in (ROOT / "install.sh").read_text()


def test_fixture_matches_the_real_catalog_view():
    import json
    import sys
    sys.path.insert(0, str(ROOT / "lib"))
    import cli_catalog
    cat = cli_catalog.load_catalog()
    import cli_help
    versions = {c["id"]: c["pinnedVersion"] for c in cat["clis"]}
    helps = {c["id"]: dict(cli_help.parse_help((ROOT / "tests" / "fixtures" / "cli-help" / f"{c['id']}.txt").read_text(),
                                               c["binary"]), command=f"{c['binary']} --help") for c in cat["clis"]}
    view = cli_catalog.catalog_view(cat, versions=versions, accounts={}, helps=helps)
    assert json.loads((ROOT / "tests" / "fixtures" / "command-catalog.json").read_text()) == view
