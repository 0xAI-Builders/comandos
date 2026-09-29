"""cc-dash /push/* endpoints with a temporary state dir and a fake sender:
nothing is ever sent to a real push service."""
import base64
import http.client
import http.server
import importlib.machinery
import importlib.util
import json
import os
import stat
import sys
import threading
from pathlib import Path

import pytest

pytest.importorskip("pywebpush")
from cryptography.hazmat.primitives import serialization  # noqa: E402
from cryptography.hazmat.primitives.asymmetric import ec  # noqa: E402


def b64(raw):
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()


def subscription(endpoint="https://fcm.googleapis.com/fcm/send/device-1"):
    public = ec.generate_private_key(ec.SECP256R1()).public_key().public_bytes(
        serialization.Encoding.X962, serialization.PublicFormat.UncompressedPoint)
    return {"endpoint": endpoint, "keys": {"p256dh": b64(public), "auth": b64(os.urandom(16))}}


@pytest.fixture
def dash(tmp_path, monkeypatch):
    monkeypatch.setenv("COMANDOS_STATE_DB", str(tmp_path / "state.sqlite3"))
    monkeypatch.delenv("COMANDOS_PUSH_DIR", raising=False)
    bin_dir = str(Path("bin").resolve())
    if bin_dir not in sys.path:
        sys.path.insert(0, bin_dir)
    loader = importlib.machinery.SourceFileLoader("cc_dash_push_under_test", str(Path("bin/cc-dash").resolve()))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    monkeypatch.setattr(module, "HOOKS", str(tmp_path))
    module._PUSH_LOCAL.__dict__.clear()
    module._PUSH_TEST_LAST.clear()
    sent = []

    def fake_send(sub, payload):
        sent.append((sub["endpoint"], payload))
        return {"ok": True, "status": 201, "gone": False, "retryAfterMs": None, "uncertain": False, "error": None}
    monkeypatch.setattr(module, "_push_send", fake_send)
    module.sent = sent
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


def call(srv, method, path, body=None, headers=None):
    client = http.client.HTTPConnection(*srv.server_address, timeout=5)
    try:
        client.request(method, path, body=None if body is None else json.dumps(body),
                       headers={"Content-Type": "application/json", **(headers or {})})
        response = client.getresponse()
        raw = response.read()
        return response.status, (json.loads(raw) if raw else None)
    finally:
        client.close()


def test_key_is_public_and_private_key_stays_private(server, tmp_path):
    status, body = call(server, "GET", "/push/key")
    assert status == 200 and body["available"] is True
    raw = base64.urlsafe_b64decode(body["publicKey"] + "==")
    assert len(raw) == 65 and raw[0] == 4
    pem = tmp_path / "push" / "vapid-private.pem"
    assert stat.S_IMODE(pem.stat().st_mode) == 0o600
    assert "PRIVATE" not in json.dumps(body)
    assert call(server, "GET", "/push/key")[1]["publicKey"] == body["publicKey"]


def test_subscribe_test_and_unsubscribe(dash, server):
    sub = subscription()
    status, body = call(server, "POST", "/push/subscription", {"subscription": sub, "deviceId": "android-1"})
    assert status == 200 and body["ok"] is True
    status, body = call(server, "POST", "/push/test", {"endpoint": sub["endpoint"]})
    assert status == 200 and body["ok"] is True and body["status"] == 201
    assert len(dash.sent) == 1 and dash.sent[0][1]["tag"] == "comandos-test"
    status, body = call(server, "DELETE", "/push/subscription", {"endpoint": sub["endpoint"]})
    assert status == 200 and body["removed"] is True
    status, _ = call(server, "POST", "/push/test", {"endpoint": sub["endpoint"]})
    assert status == 404 and len(dash.sent) == 1


def test_test_push_is_rate_limited(dash, server):
    sub = subscription()
    call(server, "POST", "/push/subscription", {"subscription": sub})
    assert call(server, "POST", "/push/test", {"endpoint": sub["endpoint"]})[0] == 200
    assert call(server, "POST", "/push/test", {"endpoint": sub["endpoint"]})[0] == 429
    assert len(dash.sent) == 1


def test_arbitrary_endpoints_are_refused(dash, server):
    for endpoint in ("https://169.254.169.254/latest/meta-data", "http://fcm.googleapis.com/x",
                     "https://evil.example/push"):
        status, body = call(server, "POST", "/push/subscription", {"subscription": subscription(endpoint)})
        assert status == 400, endpoint
    assert dash.sent == []


def test_gone_answer_to_the_test_retires_the_subscription(dash, server, monkeypatch):
    sub = subscription()
    call(server, "POST", "/push/subscription", {"subscription": sub})
    monkeypatch.setattr(dash, "_push_send", lambda s, p: {"ok": False, "status": 410, "gone": True,
                                                           "retryAfterMs": None, "uncertain": False, "error": "HTTP 410"})
    status, body = call(server, "POST", "/push/test", {"endpoint": sub["endpoint"]})
    assert status == 200 and body["removed"] is True
    dash._PUSH_TEST_LAST.clear()
    assert call(server, "POST", "/push/test", {"endpoint": sub["endpoint"]})[0] == 404


def test_push_routes_require_the_token_remotely(server):
    remote = {"X-Forwarded-For": "100.64.0.9"}
    assert call(server, "GET", "/push/key", headers=remote)[0] == 401
    assert call(server, "POST", "/push/subscription", {"subscription": subscription()}, headers=remote)[0] == 401
    assert call(server, "DELETE", "/push/subscription", {"endpoint": "x"}, headers=remote)[0] == 401
    assert call(server, "POST", "/push/test", {"endpoint": "x"}, headers=remote)[0] == 401


def test_dispatch_helper_uses_the_injected_policy(dash, server):
    call(server, "POST", "/push/subscription", {"subscription": subscription()})
    event = {"eventId": "ev-1", "kind": "turn_completed", "projectKey": "p", "title": "t"}
    assert dash.push_dispatch([event], [], policy=lambda e, c, n: False) == []
    out = dash.push_dispatch([event], [])
    assert [r["state"] for r in out] == ["sent"]
    assert dash.sent[-1][1]["url"] == "/?event=ev-1"
