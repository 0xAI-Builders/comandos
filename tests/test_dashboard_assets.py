#!/usr/bin/env python3
import os
import socket
import subprocess
import sys
import time
import urllib.request
from contextlib import contextmanager
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def _free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


@contextmanager
def running_dash(home, *, override=None):
    port = _free_port()
    env = os.environ.copy()
    env["HOME"] = str(home)
    env.pop("COMANDOS_DASH_DIR", None)
    if override is not None:
        env["COMANDOS_DASH_DIR"] = str(override)
    proc = subprocess.Popen(
        [sys.executable, str(ROOT / "bin" / "cc-dash"), str(port), "--no-open"],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        for _ in range(80):
            if proc.poll() is not None:
                _, stderr = proc.communicate(timeout=2)
                raise RuntimeError(stderr.decode(errors="replace"))
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                    break
            except OSError:
                time.sleep(0.05)
        else:
            raise RuntimeError("cc-dash did not start")
        yield f"http://127.0.0.1:{port}"
    finally:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()


def _write_dashboard(path, marker):
    (path / "assets").mkdir(parents=True)
    (path / "index.html").write_text(marker)
    (path / "assets" / "app.js").write_text(f"// {marker}\n")


def test_development_override_serves_fixture_dashboard_and_nested_assets(tmp_path):
    home = tmp_path / "home"
    installed = home / ".claude" / "hooks" / "dash"
    development = tmp_path / "worktree" / "dash"
    _write_dashboard(installed, "installed-dashboard")
    _write_dashboard(development, "development-dashboard")

    with running_dash(home, override=development) as base_url:
        assert urllib.request.urlopen(f"{base_url}/", timeout=5).read() == (
            b"development-dashboard")
        assert urllib.request.urlopen(
            f"{base_url}/assets/app.js", timeout=5).read() == (
                b"// development-dashboard\n")


def test_default_still_serves_dashboard_installed_under_hooks(tmp_path):
    home = tmp_path / "home"
    installed = home / ".claude" / "hooks" / "dash"
    source_assets = tmp_path / "repository" / "dash" / "assets"
    installed.mkdir(parents=True)
    source_assets.mkdir(parents=True)
    (installed / "index.html").write_text("installed-dashboard")
    (source_assets / "app.js").write_text("// installed-dashboard\n")
    (installed / "assets").symlink_to(source_assets, target_is_directory=True)

    with running_dash(home) as base_url:
        assert urllib.request.urlopen(f"{base_url}/", timeout=5).read() == (
            b"installed-dashboard")
        assert urllib.request.urlopen(
            f"{base_url}/assets/app.js", timeout=5).read() == (
                b"// installed-dashboard\n")


def test_explicit_override_fails_clearly_for_unusable_directory(tmp_path):
    regular_file = tmp_path / "not-a-directory"
    regular_file.write_text("not a dashboard")
    unreadable = tmp_path / "unreadable-dashboard"
    unreadable.mkdir()
    unreadable.chmod(0)
    candidates = [tmp_path / "missing-dashboard", regular_file, unreadable]

    try:
        for candidate in candidates:
            env = os.environ.copy()
            env["HOME"] = str(tmp_path / "isolated-home")
            env["COMANDOS_DASH_DIR"] = str(candidate)
            result = subprocess.run(
                [
                    sys.executable,
                    "-c",
                    ("import runpy,sys; sys.path.insert(0,sys.argv[2]); "
                     "runpy.run_path(sys.argv[1])"),
                    str(ROOT / "bin" / "cc-dash"),
                    str(ROOT / "bin"),
                ],
                env=env,
                capture_output=True,
                text=True,
                timeout=10,
            )
            assert result.returncode != 0
            assert "COMANDOS_DASH_DIR" in result.stderr
    finally:
        unreadable.chmod(0o700)
