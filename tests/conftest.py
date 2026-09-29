"""Suite-wide isolation of CommandOS state.

Code under test opens the shared SQLite state through app_state.connect(),
whose default is the user's real ~/.local/state/comandos/app-state.sqlite3.
Every test gets its own database unless it chose one, and the session fails
if the real file was touched anyway.
"""
import os
from pathlib import Path

import pytest

REAL_STATE = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "comandos" / "app-state.sqlite3"


def _fingerprint():
    # The -wal file holds writes not yet checkpointed into the main file.
    out = []
    for path in (REAL_STATE, REAL_STATE.with_name(REAL_STATE.name + "-wal")):
        try:
            st = path.stat()
            out.append((st.st_size, st.st_mtime_ns))
        except FileNotFoundError:
            out.append(None)
    return tuple(out)


def pytest_sessionstart(session):
    session.config._comandos_real_state = _fingerprint()


def pytest_sessionfinish(session, exitstatus):
    before = getattr(session.config, "_comandos_real_state", None)
    if _fingerprint() != before:
        session.exitstatus = pytest.ExitCode.TESTS_FAILED
        print(f"\nERROR: the test session modified the real state database {REAL_STATE}")


@pytest.fixture(autouse=True)
def _isolated_state_db(tmp_path, monkeypatch):
    if not os.environ.get("COMANDOS_STATE_DB"):
        monkeypatch.setenv("COMANDOS_STATE_DB", str(tmp_path / "isolated-state.sqlite3"))
    yield
