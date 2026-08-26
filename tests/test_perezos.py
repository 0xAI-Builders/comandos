import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]


def test_perezos_node_suites():
    if not shutil.which("node"):
        pytest.skip("node is not installed")
    files = sorted((ROOT / "tests" / "perezos").glob("test_*.js"))
    assert files, "PerezOS Node suites are missing"
    assert ROOT / "tests" / "perezos" / "test_long_run.js" in files
    subprocess.run(
        ["node", "--test", *map(str, files)], cwd=ROOT, check=True, timeout=420
    )
