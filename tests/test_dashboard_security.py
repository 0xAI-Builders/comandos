#!/usr/bin/env python3
import importlib.machinery
import importlib.util
import http.client
import http.server
import sys
import threading
from email.message import Message
from pathlib import Path

import pytest


@pytest.mark.parametrize("endpoint", ["/terminal-history", "/terminal-panes"])
@pytest.mark.parametrize("headers,expected", [
    ({"X-Forwarded-For": "203.0.113.8", "X-Comandos-Token": "wrong"}, 401),
    ({"Origin": "https://invalid.example"}, 403),
    ({"Content-Length": "20000001"}, 413),
    ({"Content-Length": "invalid"}, 400),
    ({"Content-Length": "-1"}, 400),
])
def test_rejected_post_cannot_corrupt_following_asset_request(
        dash, monkeypatch, tmp_path, headers, expected, endpoint):
    monkeypatch.setattr(dash, "DASH", str(tmp_path))
    monkeypatch.setattr(dash, "access_token", lambda: "correct-token")
    (tmp_path / "terminal.css").write_text(".xterm { color: white; }")
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), dash.Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    client = http.client.HTTPConnection(*server.server_address, timeout=2)
    try:
        client.request("POST", endpoint, body="{}", headers=headers)
        response = client.getresponse()
        assert response.status == expected
        response.read()
        # Unread POST bytes used to become the next method: '{}GET' -> 501.
        client.request("GET", "/terminal.css")
        asset = client.getresponse()
        assert asset.status == 200
        assert asset.read() == b".xterm { color: white; }"
        assert response.getheader("Connection") == "close"
    finally:
        client.close()
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)


@pytest.fixture(scope="module")
def dash():
    bin_dir = str(Path("bin").resolve())
    if bin_dir not in sys.path:
        sys.path.insert(0, bin_dir)
    loader = importlib.machinery.SourceFileLoader(
        "cc_dash_security_under_test", str(Path("bin/cc-dash").resolve()))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


def test_host_allowlist_accepts_safe_localhost_subdomains_and_rejects_malformed(dash):
    allowed = [
        "localhost",
        "localhost:4777",
        "comandos-perezos.localhost",
        "comandos-perezos.localhost:7383",
        "api.comandos-perezos.localhost",
        "A1-b.localhost:80",
        "localhost:65535",
    ]
    rejected = [
        "comandos-perezos.localhost.evil.test",
        "-bad.localhost",
        "bad-.localhost",
        "bad..localhost",
        ".localhost",
        "*.localhost",
        "comandos_perezos.localhost",
        "comandos-perezos.localhost:abc",
        "localhost:65536",
        "localhost:99999",
        "localhost:123456",
        "comandos-perezos.localhost/path",
    ]

    assert all(dash.HOST_OK_RE.fullmatch(host) for host in allowed)
    assert not any(dash.HOST_OK_RE.fullmatch(host) for host in rejected)


def security_gate(dash, monkeypatch, *, host, peer="127.0.0.1", xff=None,
                  origin="", token=""):
    monkeypatch.setattr(dash, "access_token", lambda: "correct-token")
    handler = object.__new__(dash.Handler)
    handler.client_address = (peer, 54321)
    handler.path = "/state"
    handler.headers = Message()
    hosts = host if isinstance(host, list) else [host]
    for value in hosts:
        handler.headers["Host"] = value
    if xff is not None:
        values = xff if isinstance(xff, list) else [xff]
        for value in values:
            handler.headers["X-Forwarded-For"] = value
    if origin:
        origins = origin if isinstance(origin, list) else [origin]
        for value in origins:
            handler.headers["Origin"] = value
    if token:
        handler.headers["X-Comandos-Token"] = token
    return handler._security_gate()


def test_security_gate_allows_tokenless_devhost_when_peer_and_xff_chain_are_loopback(
        dash, monkeypatch):
    assert security_gate(
        dash,
        monkeypatch,
        host="comandos-perezos.localhost:80",
        peer="127.0.0.1",
        xff="127.0.0.1, ::1, ::ffff:127.0.0.1",
        origin="http://comandos-perezos.localhost",
    ) is None


@pytest.mark.parametrize("case", [
    {"host": "comandos-perezos.localhost", "xff": "203.0.113.8"},
    {"host": "comandos-perezos.localhost", "xff": "127.0.0.1, 10.0.0.7"},
    {"host": "comandos-perezos.localhost", "xff": "127.0.0.1, "},
    {"host": "comandos-perezos.localhost", "xff": "localhost"},
    {"host": "comandos-perezos.localhost",
     "xff": ["127.0.0.1", "203.0.113.8"]},
    {"host": "comandos-perezos.localhost", "peer": "192.0.2.4",
     "xff": "127.0.0.1"},
    {"host": "127.0.0.1:4777", "xff": "127.0.0.1"},
    {"host": "zion.tail63a117.ts.net", "xff": "127.0.0.1"},
])
def test_security_gate_keeps_token_for_untrusted_or_nonlocal_proxy_chains(
        dash, monkeypatch, case):
    code, body = security_gate(dash, monkeypatch, **case)
    assert code == 401
    assert "token" in body["error"]


def test_security_gate_preserves_host_origin_and_remote_token_protections(
        dash, monkeypatch):
    assert security_gate(
        dash, monkeypatch, host="comandos-perezos.localhost", xff="127.0.0.1",
        origin="https://evil.example",
    ) == (403, {"error": "Origen no permitido"})
    assert security_gate(
        dash, monkeypatch, host="comandos-perezos.localhost.evil.example",
    ) == (403, {"error": "Host no permitido"})
    assert security_gate(
        dash, monkeypatch, host="zion.tail63a117.ts.net", xff="203.0.113.8",
        token="correct-token",
    ) is None


def test_security_gate_never_treats_ambiguous_or_remote_authority_as_local(
        dash, monkeypatch):
    for xff in ["", "   ", ["", ""], ["127.0.0.1", ""]]:
        assert security_gate(
            dash, monkeypatch, host="comandos-perezos.localhost", xff=xff,
        )[0] == 401
    assert security_gate(
        dash, monkeypatch, host="zion.tail63a117.ts.net",
    )[0] == 401
    assert security_gate(
        dash, monkeypatch,
        host=["comandos-perezos.localhost", "evil.example"], xff="127.0.0.1",
    ) == (403, {"error": "Host no permitido"})
    assert security_gate(
        dash, monkeypatch, host="comandos-perezos.localhost", xff="127.0.0.1",
        origin=["http://comandos-perezos.localhost", "https://evil.example"],
    ) == (403, {"error": "Origen no permitido"})


@pytest.mark.parametrize("host", [
    "localhost:4777",
    "comandos-perezos.localhost",
    "127.0.0.1:4777",
    "[::1]:4777",
])
def test_security_gate_keeps_direct_loopback_authorities_tokenless(
        dash, monkeypatch, host):
    assert security_gate(dash, monkeypatch, host=host) is None


def test_security_gate_enforces_rfc_hostname_total_length_boundary(
        dash, monkeypatch):
    valid = ".".join(["a" * 63, "b" * 63, "c" * 63, "d" * 51, "localhost"])
    too_long = ".".join(["a" * 63, "b" * 63, "c" * 63, "d" * 63, "localhost"])
    assert len(valid) == 253
    assert len(too_long) == 265
    assert security_gate(dash, monkeypatch, host=valid) is None
    assert security_gate(dash, monkeypatch, host=too_long) == (
        403, {"error": "Host no permitido"})


# CSRF: un Origin presente debe ser el MISMO origen que el Host. Antes bastaba
# con que cayera en la allowlist (*.localhost, *.ts.net), asi que cualquier
# proyecto devhost o sitio del tailnet podia teclear en los agentes.
@pytest.mark.parametrize("host,origin", [
    ("127.0.0.1:4777", "http://evil.localhost"),
    ("127.0.0.1:4777", "http://x.ts.net"),
    ("127.0.0.1:4777", "https://x.tail63a117.ts.net"),
    ("127.0.0.1:4777", "null"),
    ("127.0.0.1:4777", "http://127.0.0.1:4778"),
    ("127.0.0.1:4777", "http://localhost:4777"),
    ("127.0.0.1:4777", "ftp://127.0.0.1:4777"),
    ("127.0.0.1:4777", "http://127.0.0.1:4777/path"),
    ("127.0.0.1:4777", "http://user@127.0.0.1:4777"),
    ("localhost:4777", "http://evil.localhost:4777"),
    ("comandos-perezos.localhost", "http://other.localhost"),
    ("zion.tail63a117.ts.net", "https://evil.tail63a117.ts.net"),
])
def test_security_gate_rejects_cross_origin_even_inside_allowlist(
        dash, monkeypatch, host, origin):
    assert security_gate(dash, monkeypatch, host=host, origin=origin) == (
        403, {"error": "Origen no permitido"})
    # El token no convierte un cross-origin en legitimo.
    assert security_gate(dash, monkeypatch, host=host, origin=origin,
                         token="correct-token")[0] == 403


@pytest.mark.parametrize("host,origin", [
    ("127.0.0.1:4777", "http://127.0.0.1:4777"),
    ("localhost:4777", "http://LOCALHOST:4777"),
    ("[::1]:4777", "http://[::1]:4777"),
    ("comandos-perezos.localhost", "http://comandos-perezos.localhost"),
    ("comandos-perezos.localhost:80", "http://comandos-perezos.localhost"),
])
def test_security_gate_allows_same_origin_locally(dash, monkeypatch, host, origin):
    assert security_gate(dash, monkeypatch, host=host, origin=origin) is None


def test_security_gate_same_origin_tailnet_still_needs_token(dash, monkeypatch):
    host, origin = "zion.tail63a117.ts.net", "https://zion.tail63a117.ts.net"
    assert security_gate(dash, monkeypatch, host=host, origin=origin,
                         xff="100.64.0.2")[0] == 401
    assert security_gate(dash, monkeypatch, host=host, origin=origin,
                         xff="100.64.0.2", token="correct-token") is None


def test_security_gate_without_origin_keeps_local_clients_tokenless(dash, monkeypatch):
    # curl de los hooks, cc-app, cc-notifyd, cc-telegram: sin Origin.
    assert security_gate(dash, monkeypatch, host="127.0.0.1:4777") is None


@pytest.mark.parametrize("token", ["ñandú", "é" * 43, "tok\udcff"])
def test_security_gate_non_ascii_token_is_unauthorized_not_crash(
        dash, monkeypatch, token):
    monkeypatch.setattr(dash, "access_token", lambda: "correct-token")
    handler = object.__new__(dash.Handler)
    handler.client_address = ("203.0.113.8", 1)
    handler.path = "/state"
    handler.headers = Message()
    handler.headers["Host"] = "zion.tail63a117.ts.net"
    monkeypatch.setattr(handler, "_presented_token", lambda: token, raising=False)
    assert handler._security_gate()[0] == 401


def test_allocation_status_requires_token_remotely(dash):
    assert any("/allocation/status".startswith(p) for p in dash.Handler.API_GET)


def _serve(dash):
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), dash.Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, thread


def test_cross_origin_no_cors_post_to_send_is_rejected_before_typing(
        dash, monkeypatch):
    typed = []
    monkeypatch.setattr(dash, "tmux", lambda *a, **k: typed.append(a))
    monkeypatch.setattr(dash, "tmux_stdin", lambda *a, **k: typed.append(a))
    server, thread = _serve(dash)
    port = server.server_address[1]
    client = http.client.HTTPConnection("127.0.0.1", port, timeout=3)
    try:
        client.request("POST", "/send", body='{"session":"x","text":"rm -rf ~"}',
                       headers={"Host": f"127.0.0.1:{port}",
                                "Origin": "http://evil.localhost",
                                "Content-Type": "text/plain"})
        response = client.getresponse()
        assert response.status == 403
        response.read()
    finally:
        client.close()
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)
    assert typed == []
