#!/usr/bin/env python3
"""End-to-end tests for /snippets routes.

Starts cc-dash on a random loopback port, points HOME at a tmpdir so
`~/.claude/hooks/` lands in isolation, and exercises the routes with urllib.
"""
import json
import os
import socket
import subprocess
import sys
import time
import urllib.request
import urllib.error
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]


def _free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


@pytest.fixture
def dash(tmp_path):
    port = _free_port()
    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    hooks = tmp_path / ".claude" / "hooks"
    hooks.mkdir(parents=True)
    (hooks / "state").mkdir()
    # Empty tabs file so /state works.
    (hooks / "app-tabs.json").write_text("{}")
    proc = subprocess.Popen(
        [sys.executable, str(ROOT / "bin" / "cc-dash"), str(port), "--no-open"],
        env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    for _ in range(80):
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                break
        except OSError:
            time.sleep(0.1)
    else:
        proc.kill()
        out, err = proc.communicate(timeout=2)
        raise RuntimeError(f"cc-dash did not start: {err.decode(errors='ignore')[:500]}")
    yield f"http://127.0.0.1:{port}"
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()


def _req(url, method="GET", body=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method,
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=5) as r:
            return r.status, json.loads(r.read() or b"{}")
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read() or b"{}")


def test_get_snippets_empty(dash):
    status, body = _req(f"{dash}/snippets")
    assert status == 200
    assert body == []


def test_post_snippets_creates(dash):
    status, body = _req(f"{dash}/snippets", "POST",
                        {"name": "docker-ps", "body": "docker ps -a", "tags": ["docker"]})
    assert status == 200
    it = body["item"]
    assert it["name"] == "docker-ps"
    assert it["body"] == "docker ps -a"
    assert it["tags"] == ["docker"]
    assert len(it["id"]) == 16

    status, body = _req(f"{dash}/snippets")
    assert status == 200
    assert len(body) == 1
    assert body[0]["id"] == it["id"]


def test_post_snippets_rejects_empty_name(dash):
    status, body = _req(f"{dash}/snippets", "POST", {"name": "", "body": "x"})
    assert status == 400
    assert "name" in body["error"].lower()


def test_post_snippets_update_changes_fields(dash):
    _, created = _req(f"{dash}/snippets", "POST", {"name": "a", "body": "x"})
    id_ = created["item"]["id"]
    status, body = _req(f"{dash}/snippets/update", "POST",
                        {"id": id_, "name": "b", "body": "y", "tags": ["t"]})
    assert status == 200
    assert body["item"]["name"] == "b"
    assert body["item"]["body"] == "y"
    assert body["item"]["tags"] == ["t"]


def test_post_snippets_update_missing_id(dash):
    status, body = _req(f"{dash}/snippets/update", "POST",
                        {"id": "0" * 16, "name": "b", "body": "y"})
    assert status == 404


def test_post_snippets_delete_removes(dash):
    _, created = _req(f"{dash}/snippets", "POST", {"name": "a", "body": "x"})
    id_ = created["item"]["id"]
    status, body = _req(f"{dash}/snippets/delete", "POST", {"id": id_})
    assert status == 200
    assert body["ok"] is True
    _, listed = _req(f"{dash}/snippets")
    assert listed == []


def test_post_snippets_delete_missing_id(dash):
    status, _ = _req(f"{dash}/snippets/delete", "POST", {"id": "0" * 16})
    assert status == 404
