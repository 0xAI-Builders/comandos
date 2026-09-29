"""W4: per-device drafts and reading anchors survive a reopen without PTY input."""
import json
from pathlib import Path
import shutil
import subprocess
import sys

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import app_state  # noqa: E402
import workspace_state as ws  # noqa: E402


@pytest.fixture
def store(tmp_path):
    conn = app_state.connect(tmp_path / "state.sqlite3")
    app_state.migrate(conn)
    return ws.WorkspaceStore(conn)


def test_draft_patches_merge_per_key_and_null_removes(store):
    store.save_client("phone", {"draftsPatch": {"tab:alpha": {"text": "hola", "selStart": 2, "selEnd": 4}}})
    store.save_client("phone", {"draftsPatch": {"tab:beta": {"text": "otro"}}})
    drafts = store.client("phone")["drafts"]
    assert drafts["tab:alpha"]["text"] == "hola" and drafts["tab:beta"]["text"] == "otro"
    store.save_client("phone", {"draftsPatch": {"tab:alpha": None}})
    assert set(store.client("phone")["drafts"]) == {"tab:beta"}
    assert store.client("desk") is None      # other devices never see it


def test_anchor_patches_merge_and_focus_is_kept(store):
    store.save_client("phone", {"activeTabId": "alpha"})
    store.save_client("phone", {"anchorsPatch": {"tab:alpha": {"text": "línea 40", "ratio": 0.3}}})
    state = store.client("phone")
    assert state["activeTabId"] == "alpha" and state["readingAnchors"]["tab:alpha"]["text"] == "línea 40"


def test_drafts_are_bounded(store):
    with pytest.raises(ValueError):
        store.save_client("phone", {"draftsPatch": {"tab:a": {"text": "x" * (ws.MAX_DRAFT_CHARS + 1)}}})
    for i in range(ws.MAX_DRAFTS + 5):
        store.save_client("phone", {"draftsPatch": {f"tab:{i}": {"text": str(i), "updatedAt": i}}})
    drafts = store.client("phone")["drafts"]
    assert len(drafts) == ws.MAX_DRAFTS and "tab:0" not in drafts      # oldest dropped first


def test_bad_patch_shapes_are_rejected(store):
    for bad in ({"draftsPatch": []}, {"draftsPatch": {"tab:a": {"text": 3}}}, {"anchorsPatch": {"": {}}}):
        with pytest.raises(ValueError):
            store.save_client("phone", bad)


@pytest.mark.skipif(not shutil.which("node"), reason="node required")
def test_restoring_a_draft_never_sends_bytes_to_the_terminal():
    result = subprocess.run(["node", str(ROOT / "tests/device_drafts_checks.cjs")], capture_output=True, text=True, timeout=30)
    assert result.returncode == 0, result.stdout + result.stderr
