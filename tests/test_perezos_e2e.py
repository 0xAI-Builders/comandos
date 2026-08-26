import json
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]


def test_perezos_browser_harness_imports_without_launching():
    if not shutil.which("node"):
        pytest.skip("node is not installed")
    run = subprocess.run(
        [
            "node",
            "-e",
            (
                "const h=require('./tests/e2e_perezos.js');"
                "process.stdout.write(JSON.stringify(h.assertImportContract()))"
            ),
        ],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=10,
    )
    assert run.returncode == 0, run.stdout + run.stderr
    contract = json.loads(run.stdout)
    assert contract["scripts"] == [
        "core", "art", "rig", "behaviors", "motion", "renderer", "engine"
    ]
    assert contract["viewport"] == {"width": 1400, "height": 900}


def test_perezos_browser_contract():
    if not shutil.which("node"):
        pytest.skip("node is not installed")
    run = subprocess.run(
        ["node", "tests/e2e_perezos.js"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=150,
    )
    if run.returncode == 77:
        pytest.skip(run.stderr.strip() or "Playwright/Chromium is unavailable")
    assert run.returncode == 0, run.stdout + run.stderr
    report = json.loads(run.stdout)
    assert report["visualFailures"] == []
    assert report["lifecycleFailures"] == []
    assert report["accessibilityFailures"] == []
    assert report["performance"]["averageMs"] < 1.0
    assert report["performance"]["p95Ms"] < 2.0
    assert report["performance"]["decodedBytes"] < 16 * 1024 * 1024
    assert report["performance"]["steadyAllocations"] == 0
    assert report["performance"]["warmupMs"] == 10_000
    assert report["performance"]["sampleMsPerScenario"] == 30_000
    for screenshot in report["screenshots"].values():
        artifact = Path(screenshot)
        assert artifact.is_file() and artifact.stat().st_size > 0
