"""Quick terminals: one uniquely dated folder and one shell per requestId.

Every test uses a temporary database and base directory and an injected fake
shell creator; nothing here talks to the user's tmux server.
"""
import os
import stat
import sys
import threading
from datetime import datetime, timezone
from pathlib import Path
from zoneinfo import ZoneInfo

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "lib"))
import app_state  # noqa: E402
import quick_terminal as qt  # noqa: E402

MX = ZoneInfo("America/Mexico_City")
AT = datetime(2026, 9, 29, 10, 11, 12, tzinfo=MX)


class FakeShells:
    """Records launches; a session 'exists' once a launch succeeded."""

    def __init__(self, fail=0, create_then_fail=False, delay=0.0):
        self.live, self.launches, self.registered = {}, [], []
        self.fail, self.create_then_fail, self.delay = fail, create_then_fail, delay
        self.lock = threading.Lock()

    def launch(self, session, cwd, pane_key):
        if self.delay:
            threading.Event().wait(self.delay)
        with self.lock:
            self.launches.append((session, cwd, pane_key))
            if self.create_then_fail:
                self.create_then_fail = False
                self.live[session] = cwd
                raise RuntimeError("timeout after creating the session")
            if self.fail:
                self.fail -= 1
                raise RuntimeError("tmux: no server")
            self.live[session] = cwd

    def exists(self, session):
        with self.lock:
            return session in self.live

    def register(self, session, label, cwd):
        self.registered.append((session, label, cwd))


def db(tmp_path):
    conn = app_state.connect(tmp_path / "state.sqlite3")
    app_state.migrate(conn)
    return conn


def open_(conn, shells, base, request_id="req-1", now=AT, **kw):
    return qt.open_quick_terminal(conn, request_id, base=base, launch=shells.launch,
                                  exists=shells.exists, register=shells.register, now=now, **kw)


# ---- directory reservation ------------------------------------------------

def test_fixed_time_names_the_folder_in_mexico_city(tmp_path):
    utc = datetime(2026, 9, 29, 16, 11, 12, tzinfo=timezone.utc)
    assert qt.reserve_directory(tmp_path, utc).name == "T-2026-09-29-10-11-12"


def test_naive_time_is_rejected(tmp_path):
    with pytest.raises(ValueError):
        qt.reserve_directory(tmp_path, datetime(2026, 9, 29, 10, 0, 0))


def test_two_reservations_in_the_same_second_get_distinct_folders(tmp_path):
    first = qt.reserve_directory(tmp_path, AT)
    second = qt.reserve_directory(tmp_path, AT)
    assert (first.name, second.name) == ("T-2026-09-29-10-11-12", "T-2026-09-29-10-11-12-2")


def test_concurrent_reservations_never_share_a_folder(tmp_path):
    out, barrier = [], threading.Barrier(12)

    def work():
        barrier.wait()
        out.append(qt.reserve_directory(tmp_path, AT))

    threads = [threading.Thread(target=work) for _ in range(12)]
    [t.start() for t in threads]
    [t.join() for t in threads]
    assert len({p.name for p in out}) == 12
    assert all(p.is_dir() for p in out)


def test_existing_folder_with_content_is_never_reused(tmp_path):
    taken = tmp_path / "T-2026-09-29-10-11-12"
    taken.mkdir()
    (taken / "notes.txt").write_text("keep")
    assert qt.reserve_directory(tmp_path, AT).name == "T-2026-09-29-10-11-12-2"
    assert (taken / "notes.txt").read_text() == "keep"


# ---- request lifecycle ----------------------------------------------------

def test_first_request_creates_folder_shell_and_tab(tmp_path):
    shells, conn = FakeShells(), db(tmp_path)
    out = open_(conn, shells, tmp_path / "Terminal")
    assert out["cwd"] == str(tmp_path / "Terminal" / "T-2026-09-29-10-11-12")
    assert Path(out["cwd"]).is_dir()
    assert out["tabId"].startswith("term-q") and out["paneKey"].startswith("pane-q")
    assert shells.launches == [(out["tabId"], out["cwd"], out["paneKey"])]
    assert shells.registered == [(out["tabId"], "T-2026-09-29-10-11-12", out["cwd"])]
    row = conn.execute("SELECT state, cwd FROM quick_terminal_requests").fetchone()
    assert row == ("ready", out["cwd"])


def test_same_request_returns_the_same_terminal_without_a_second_shell(tmp_path):
    shells, conn = FakeShells(), db(tmp_path)
    first = open_(conn, shells, tmp_path)
    later = datetime(2026, 9, 29, 11, 0, 0, tzinfo=MX)
    again = open_(conn, shells, tmp_path, now=later)
    assert {k: again[k] for k in ("tabId", "paneKey", "cwd")} == \
        {k: first[k] for k in ("tabId", "paneKey", "cwd")}
    assert len(shells.launches) == 1
    assert len(list(tmp_path.glob("T-*"))) == 1


def test_another_request_gets_another_folder_and_shell(tmp_path):
    shells, conn = FakeShells(), db(tmp_path)
    a = open_(conn, shells, tmp_path, "req-a")
    b = open_(conn, shells, tmp_path, "req-b")
    assert a["cwd"] != b["cwd"] and a["tabId"] != b["tabId"]
    assert len(shells.launches) == 2


def test_active_pane_cwd_is_not_inherited(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    shells, conn = FakeShells(), db(tmp_path)
    out = open_(conn, shells, tmp_path / "base")
    assert Path(out["cwd"]).parent == tmp_path / "base"


def test_permission_denied_reports_and_stores_nothing(tmp_path):
    base = tmp_path / "locked"
    base.mkdir()
    os.chmod(base, stat.S_IRUSR | stat.S_IXUSR)
    try:
        if os.access(base, os.W_OK):
            pytest.skip("running as a user that ignores directory permissions")
        shells, conn = FakeShells(), db(tmp_path)
        with pytest.raises(qt.QuickTerminalError) as err:
            open_(conn, shells, base)
        assert err.value.code == "folder" and err.value.retryable
        assert shells.launches == []
        assert conn.execute("SELECT COUNT(*) FROM quick_terminal_requests").fetchone()[0] == 0
    finally:
        os.chmod(base, stat.S_IRWXU)


def test_launch_failure_keeps_folder_and_retry_reuses_it(tmp_path):
    shells, conn = FakeShells(fail=1), db(tmp_path)
    with pytest.raises(qt.QuickTerminalError) as err:
        open_(conn, shells, tmp_path)
    assert err.value.code == "launch" and err.value.retryable
    cwd = err.value.cwd
    (Path(cwd) / "draft.txt").write_text("user content")
    state = conn.execute("SELECT state, error FROM quick_terminal_requests").fetchone()
    assert state[0] == "failed" and "no server" in state[1]
    out = open_(conn, shells, tmp_path, now=datetime(2026, 9, 29, 12, 0, 0, tzinfo=MX))
    assert out["cwd"] == cwd and (Path(cwd) / "draft.txt").read_text() == "user content"
    assert len(shells.live) == 1 and len(shells.launches) == 2
    assert len(list(tmp_path.glob("T-*"))) == 1


def test_retry_after_ambiguous_failure_does_not_open_a_second_shell(tmp_path):
    shells, conn = FakeShells(create_then_fail=True), db(tmp_path)
    with pytest.raises(qt.QuickTerminalError):
        open_(conn, shells, tmp_path)
    out = open_(conn, shells, tmp_path)
    assert len(shells.launches) == 1          # the session already existed
    assert shells.live == {out["tabId"]: out["cwd"]}


def test_retry_recreates_a_folder_removed_after_failure(tmp_path):
    shells, conn = FakeShells(fail=1), db(tmp_path)
    with pytest.raises(qt.QuickTerminalError) as err:
        open_(conn, shells, tmp_path)
    os.rmdir(err.value.cwd)
    out = open_(conn, shells, tmp_path)
    assert out["cwd"] == err.value.cwd and Path(out["cwd"]).is_dir()


def test_concurrent_same_request_opens_exactly_one_shell(tmp_path):
    shells = FakeShells(delay=0.2)
    path = tmp_path / "state.sqlite3"
    app_state.migrate(app_state.connect(path))
    results, errors, barrier = [], [], threading.Barrier(6)

    def work():
        conn = app_state.connect(path)
        barrier.wait()
        try:
            results.append(qt.open_quick_terminal(
                conn, "same", base=tmp_path / "T", launch=shells.launch,
                exists=shells.exists, register=shells.register, now=AT))
        except Exception as exc:  # pragma: no cover - reported below
            errors.append(exc)

    threads = [threading.Thread(target=work) for _ in range(6)]
    [t.start() for t in threads]
    [t.join() for t in threads]
    assert errors == []
    assert len(shells.launches) == 1
    assert len({(r["tabId"], r["cwd"]) for r in results}) == 1
    assert len(list((tmp_path / "T").iterdir())) == 1


def test_a_waiter_gives_up_while_another_launch_holds_the_lease(tmp_path):
    shells, conn = FakeShells(), db(tmp_path)
    conn.execute("INSERT INTO quick_terminal_requests VALUES "
                 "('busy','launching','/x','term-qx','pane-qx',NULL,?,0,0)", (10 ** 12,))
    with pytest.raises(qt.QuickTerminalError) as err:
        open_(conn, shells, tmp_path, "busy", wait=0.05)
    assert err.value.code == "busy" and shells.launches == []


def test_an_expired_lease_is_taken_over_once(tmp_path):
    shells, conn = FakeShells(), db(tmp_path)
    cwd = tmp_path / "T-old"
    conn.execute("INSERT INTO quick_terminal_requests VALUES "
                 "('stale','launching',?,'term-qs','pane-qs',NULL,1,0,0)", (str(cwd),))
    out = open_(conn, shells, tmp_path, "stale")
    assert out["cwd"] == str(cwd) and len(shells.launches) == 1


@pytest.mark.parametrize("bad", ["", "x" * 129, "a b", "../x", None, 7])
def test_invalid_request_ids_are_rejected(tmp_path, bad):
    shells, conn = FakeShells(), db(tmp_path)
    with pytest.raises(qt.QuickTerminalError) as err:
        open_(conn, shells, tmp_path, bad)
    assert err.value.code == "request"


def test_default_base_is_the_approved_folder_and_injectable(monkeypatch):
    monkeypatch.delenv("COMANDOS_QUICK_TERMINAL_BASE", raising=False)
    monkeypatch.setenv("HOME", "/home/someguy")
    assert str(qt.default_base()) == "/home/someguy/codebase/0xJesus/Terminal"
    monkeypatch.setenv("COMANDOS_QUICK_TERMINAL_BASE", "/tmp/elsewhere")
    assert str(qt.default_base()) == "/tmp/elsewhere"


def test_migration_seven_is_registered():
    assert any(v == 7 and "quick_terminal_requests" in sql for v, _, sql in app_state.MIGRATIONS)


# ---- cc-dash endpoint -----------------------------------------------------

import http.client  # noqa: E402
import http.server  # noqa: E402
import importlib.machinery  # noqa: E402
import importlib.util  # noqa: E402
import json  # noqa: E402
import shutil  # noqa: E402
import subprocess  # noqa: E402
import uuid  # noqa: E402


@pytest.fixture
def dash(tmp_path, monkeypatch):
    monkeypatch.setenv("COMANDOS_STATE_DB", str(tmp_path / "state.sqlite3"))
    monkeypatch.setenv("COMANDOS_QUICK_TERMINAL_BASE", str(tmp_path / "Terminal"))
    bin_dir = str(Path("bin").resolve())
    if bin_dir not in sys.path:
        sys.path.insert(0, bin_dir)
    loader = importlib.machinery.SourceFileLoader(
        "cc_dash_quick_terminal_under_test", str(Path("bin/cc-dash").resolve()))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    tabs = tmp_path / "app-tabs.json"
    tabs.write_text(json.dumps({"alpha": "Alpha"}))
    monkeypatch.setattr(module, "TABS_FILE", str(tabs))
    monkeypatch.setattr(module, "HOOKS", str(tmp_path))
    monkeypatch.setattr(module, "TAB_OPEN_FILE", str(tmp_path / "app-tab-open.json"))
    monkeypatch.setattr(module, "TABS_META_FILE", str(tmp_path / "meta.json"), raising=False)
    monkeypatch.setattr(module, "LAYOUT_SNAPSHOT_FILE", str(tmp_path / "app-sessions-v2.json"))
    module._WORKSPACE_LOCAL.__dict__.clear()
    module._QUICK_TERMINAL_LOCAL.__dict__.clear()
    shells = FakeShells()
    module._real_quick_terminal_launch = module.quick_terminal_launch
    monkeypatch.setattr(module, "quick_terminal_launch", shells.launch)
    monkeypatch.setattr(module, "quick_terminal_exists", shells.exists)
    module._fake_shells = shells
    return module


@pytest.fixture
def server(dash):
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), dash.Handler)
    thread = threading.Thread(target=srv.serve_forever, daemon=True)
    thread.start()
    yield srv
    srv.shutdown()
    srv.server_close()
    thread.join(timeout=2)


def call(srv, path, body):
    client = http.client.HTTPConnection(*srv.server_address, timeout=10)
    try:
        client.request("POST", path, body=json.dumps(body), headers={"Content-Type": "application/json"})
        response = client.getresponse()
        return response.status, json.loads(response.read() or b"null")
    finally:
        client.close()


def test_endpoint_opens_registers_and_repeats_idempotently(dash, server, tmp_path):
    status, body = call(server, "/terminal/quick", {"requestId": "web-1"})
    assert status == 200, body
    assert set(body) >= {"tabId", "paneKey", "cwd"}
    assert Path(body["cwd"]).parent == tmp_path / "Terminal" and Path(body["cwd"]).is_dir()
    registry = json.loads(Path(dash.TABS_FILE).read_text())
    assert registry[body["tabId"]] == Path(body["cwd"]).name
    meta = json.loads(Path(dash.TABS_META_FILE).read_text())[body["tabId"]]
    assert meta == {"kind": "scratch", "cwd": body["cwd"]}
    opened = json.loads((tmp_path / "app-tab-open.json").read_text())
    assert opened["session"] == body["tabId"]            # the desktop opens the same tab
    workspace = dash.workspace_store().current()["document"]
    assert body["tabId"] in workspace["tabs"]            # and the remote workspace lists it
    again = call(server, "/terminal/quick", {"requestId": "web-1"})
    assert again[0] == 200 and again[1]["tabId"] == body["tabId"] and again[1]["cwd"] == body["cwd"]
    assert len(dash._fake_shells.launches) == 1


def test_endpoint_rejects_missing_request_id(server):
    status, body = call(server, "/terminal/quick", {})
    assert status == 400 and body["code"] == "request"


def test_endpoint_reports_launch_failure_and_retry_succeeds(dash, server):
    dash._fake_shells.fail = 1
    status, body = call(server, "/terminal/quick", {"requestId": "web-2"})
    assert status == 502 and body["retryable"] is True and body["code"] == "launch"
    assert Path(body["cwd"]).is_dir()
    status, again = call(server, "/terminal/quick", {"requestId": "web-2"})
    assert status == 200 and again["cwd"] == body["cwd"]
    assert len(dash._fake_shells.live) == 1


def test_endpoint_does_not_read_the_active_pane(dash, server, monkeypatch):
    def forbidden(*args, **kwargs):
        raise AssertionError("quick terminal must not ask tmux for the active pane cwd")
    monkeypatch.setattr(dash, "tmux", forbidden)
    status, body = call(server, "/terminal/quick", {"requestId": "web-3"})
    assert status == 200 and "Terminal" in body["cwd"]


# ---- real tmux, private socket only ----------------------------------------

@pytest.mark.skipif(shutil.which("tmux") is None, reason="tmux not installed")
def test_launch_starts_a_plain_shell_on_a_private_tmux_server(dash, tmp_path, monkeypatch):
    private = tmp_path / "tmux"
    private.mkdir(mode=0o700)
    sock = "cq-" + uuid.uuid4().hex[:10]
    env = {k: v for k, v in os.environ.items() if k not in ("TMUX", "TMUX_PANE")}
    env["TMUX_TMPDIR"] = str(private)
    socket_path = private / f"tmux-{os.getuid()}" / sock

    real_run = subprocess.run

    def private_tmux(*args, timeout=5):
        return real_run(["tmux", "-L", sock, *args], capture_output=True, text=True,
                        timeout=timeout, env=env)

    def run(argv, **kw):
        assert argv[0] == "tmux", argv
        return real_run(["tmux", "-L", sock, *argv[1:]], env=env, **kw)

    monkeypatch.setattr(dash, "tmux", private_tmux)
    monkeypatch.setattr(dash, "scope_cmd", lambda argv: argv)
    monkeypatch.setattr(dash.subprocess, "run", run)
    cwd = tmp_path / "T-2026-09-29-10-11-12"
    cwd.mkdir()
    try:
        dash._real_quick_terminal_launch("term-qprivate", str(cwd), "pane-qprivate")
        assert socket_path.exists()                     # the private server, not the user's
        assert private_tmux("has-session", "-t", "=term-qprivate").returncode == 0
        info = private_tmux("display-message", "-p", "-t", "=term-qprivate:",
                            "#{pane_current_path} #{@comandos-pane-key}").stdout.split()
        assert info == [str(cwd), "pane-qprivate"]
        command = private_tmux("display-message", "-p", "-t", "=term-qprivate:",
                               "#{pane_current_command}").stdout.strip()
        assert command in {"zsh", "bash", "sh", "fish", "dash"}
    finally:
        if socket_path.exists():
            real_run(["tmux", "-L", sock, "kill-server"], env=env, capture_output=True)
