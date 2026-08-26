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
    assert contract["unavailableClassification"] == {
        "missingPackage": True,
        "missingExecutable": True,
        "launchCrash": False,
        "invalidFlags": False,
    }


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
    assert report["performance"]["stableBufferReplacements"] == 0
    assert "steadyAllocations" not in report["performance"]
    assert report["performance"]["sourceAudit"]["preallocated"] is True
    assert report["performance"]["heap"]["bounded"] is True
    assert report["performance"]["heap"]["growthBytes"] <= report["performance"]["heap"]["budgetBytes"]
    assert report["performance"]["warmupMs"] == 10_000
    assert report["performance"]["sampleMsPerScenario"] == 30_000
    for scenario in ("idle", "action"):
        assert report["performance"][scenario]["samples"] > 0
        assert report["performance"][scenario]["complete"] is True
        assert report["performance"][scenario]["coverageMs"] >= 29_700
        assert 0 <= report["performance"][scenario]["startLagMs"] <= 150
        assert 0 <= report["performance"][scenario]["endLagMs"] <= 150
        assert report["performance"][scenario]["allFull"] is True
        assert report["performance"][scenario]["qualityTransitions"] == 0
        assert report["performance"][scenario]["governorTransitions"] == 0
    assert report["performance"]["action"]["allActive"] is True
    assert 28 <= report["performance"]["action"]["cadenceHz"] <= 31.5
    assert report["performance"]["action"]["expectedCadenceHz"] == 30
    for screenshot in report["screenshots"].values():
        artifact = Path(screenshot)
        assert artifact.is_file() and artifact.stat().st_size > 0
