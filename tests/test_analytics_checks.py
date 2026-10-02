"""Corre desde pytest los checks node de Analytics (paridad con el mockup y la capa del tablero)."""
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
CHECKS = ["analytics_parity_checks.cjs"]


@pytest.mark.parametrize("name", CHECKS)
def test_node_check(name):
    if not shutil.which("node"):
        pytest.skip("sin node")
    subprocess.run(["node", str(ROOT / "tests" / name)], check=True, cwd=ROOT)
