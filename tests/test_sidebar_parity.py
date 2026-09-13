"""Exercise rendered sidebar parity, rather than matching implementation strings."""
import os
from pathlib import Path
import shutil
import subprocess

import pytest

ROOT = Path(__file__).resolve().parents[1]


def test_shared_sidebar_behavior_in_isolated_browser():
    node = shutil.which('node')
    if not node or not Path('/usr/bin/google-chrome').exists():
        pytest.skip('Headless sidebar matrix requires Node and Google Chrome')
    probe = subprocess.run(
        [node, '-e', "require('./tests/e2e_mobile_remote').loadPlaywright()"],
        cwd=ROOT, capture_output=True, text=True, timeout=20,
    )
    if probe.returncode:
        pytest.skip('Install Playwright or set NODE_PATH to run the sidebar browser matrix')
    result = subprocess.run(
        [node, 'tests/e2e_sidebar_parity.js'], cwd=ROOT,
        capture_output=True, text=True, timeout=120, env=os.environ.copy(),
    )
    assert result.returncode == 0, result.stdout + result.stderr
