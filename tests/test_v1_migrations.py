"""R3: every 1.0 storage change migrates with a backup, reruns harmlessly and
survives interruption; the installer can run twice on a temporary HOME."""
import json
import os
from pathlib import Path
import sqlite3
import stat
import subprocess
import sys

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import app_state  # noqa: E402

V1_TABLES = {
    "workspace_current", "workspace_previous", "workspace_requests", "workspace_clients", "workspace_meta",
    "events", "event_receipts", "deliveries", "work_marks", "work_mark_applied",
    "pomodoro_state", "pomodoro_blocks", "pomodoro_requests", "pomodoro_records",
    "news_editions", "push_subscriptions", "push_deliveries", "quick_terminal_requests",
    "client_presence", "notice_reads", "notice_prefs",
}


def tables(conn):
    return {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}


def backups(path):
    # Opening a backup creates its own -wal/-shm companions; count the files.
    return sorted(p for p in path.parent.iterdir()
                  if ".pre-migration-" in p.name and not p.name.endswith(("-wal", "-shm")))


def test_versions_are_unique_and_the_latest_is_nine():
    versions = [m[0] for m in app_state.MIGRATIONS]
    assert len(versions) == len(set(versions)) and max(versions) == 9 and sorted(versions) == versions


def test_fresh_database_gets_every_table_without_a_backup(tmp_path):
    db = tmp_path / "state.sqlite3"
    conn = app_state.connect(db)
    assert app_state.migrate(conn) == 9
    assert V1_TABLES <= tables(conn)
    assert backups(db) == []


def test_existing_data_is_backed_up_privately_and_kept(tmp_path, monkeypatch):
    db = tmp_path / "state.sqlite3"
    conn = app_state.connect(db)
    full = list(app_state.MIGRATIONS)
    monkeypatch.setattr(app_state, "MIGRATIONS", full[:1])       # an install that only had W1
    app_state.migrate(conn)
    conn.execute("INSERT INTO workspace_current VALUES (1, 7, ?, 0)", (json.dumps({"schema": 1, "groups": [], "tabs": {}}),))
    monkeypatch.setattr(app_state, "MIGRATIONS", full)
    assert app_state.migrate(conn) == 9
    made = backups(db)
    assert len(made) == 1 and stat.S_IMODE(made[0].stat().st_mode) == 0o600
    assert conn.execute("SELECT revision FROM workspace_current").fetchone()[0] == 7
    old = sqlite3.connect(made[0])
    assert old.execute("SELECT MAX(version) FROM schema_migrations").fetchone()[0] == 1
    # Rerun: nothing pending, no second backup.
    assert app_state.migrate(conn) == 9 and len(backups(db)) == 1


def test_an_interrupted_migration_rolls_back_every_pending_step(tmp_path, monkeypatch):
    db = tmp_path / "state.sqlite3"
    conn = app_state.connect(db)
    full = list(app_state.MIGRATIONS)
    monkeypatch.setattr(app_state, "MIGRATIONS", full[:3])
    app_state.migrate(conn)
    conn.execute("INSERT INTO workspace_meta VALUES ('keep', 'me')")
    broken = [m if m[0] != 6 else (6, "news_editions", m[2] + "\nTHIS IS NOT SQL;") for m in full]
    monkeypatch.setattr(app_state, "MIGRATIONS", broken)
    with pytest.raises(sqlite3.Error):
        app_state.migrate(conn)
    assert app_state.schema_version(conn) == 3
    assert "pomodoro_state" not in tables(conn), "steps before the failure are rolled back too"
    assert conn.execute("SELECT value FROM workspace_meta WHERE key='keep'").fetchone()[0] == "me"
    monkeypatch.setattr(app_state, "MIGRATIONS", full)
    assert app_state.migrate(conn) == 9


def test_an_older_app_leaves_newer_data_untouched(tmp_path, monkeypatch):
    db = tmp_path / "state.sqlite3"
    conn = app_state.connect(db)
    app_state.migrate(conn)
    conn.execute("INSERT INTO notice_prefs VALUES (1, '{\"muted\": true}')")
    monkeypatch.setattr(app_state, "MIGRATIONS", list(app_state.MIGRATIONS)[:1])   # rollback to the W1 app
    old = app_state.connect(db)
    assert app_state.migrate(old) == 9          # it never downgrades or rewrites
    assert old.execute("SELECT value FROM notice_prefs").fetchone()[0] == '{"muted": true}'
    assert backups(db) == []


def test_installer_runs_twice_on_a_temporary_home(tmp_path):
    home, fake_bin = tmp_path / "home", tmp_path / "bin"
    home.mkdir()
    fake_bin.mkdir()
    (fake_bin / "systemctl").write_text("#!/usr/bin/env bash\nexit 0\n")
    (fake_bin / "systemctl").chmod(0o755)
    env = {k: v for k, v in os.environ.items() if k not in ("TMUX", "TMUX_PANE")}
    # Without cargo the installer skips compiling the vendored gateway.
    path = [d for d in env["PATH"].split(os.pathsep) if d and not os.path.exists(os.path.join(d, "cargo"))]
    env.update(HOME=str(home), PATH=os.pathsep.join([str(fake_bin)] + path), CC_MOCK_UNAME="Linux",
               CC_MOCK_OSRELEASE_FILE=str(tmp_path / "none"), CC_MOCK_OS_RELEASE_FILE=str(tmp_path / "none2"),
               XDG_STATE_HOME=str(home / ".local/state"))
    env.pop("COMANDOS_RETIRE_TELEGRAM", None)
    runs = [subprocess.run(["bash", str(ROOT / "install.sh")], env=env, capture_output=True, text=True, timeout=240)
            for _ in range(2)]
    assert [r.returncode for r in runs] == [0, 0], runs[-1].stdout[-2000:] + runs[-1].stderr[-2000:]
    settings = json.loads((home / ".claude/settings.json").read_text())
    for event, entries in settings.get("hooks", {}).items():
        commands = [h.get("command", "") for e in entries for h in e.get("hooks", [])]
        assert sum("cc-notify.sh" in c for c in commands) <= 1, f"{event} hook registered twice"
    for name in ("notifications.js", "device-drafts.js", "pomodoro.js", "work-marks.js", "quick-terminal.js",
                 "workspace-dock.js", "news-reader.js", "push-settings.js"):
        assert (home / ".claude/hooks/dash" / name).exists(), name
