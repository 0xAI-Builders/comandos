#!/usr/bin/env python3
import importlib.machinery
import importlib.util
import sys
from email.message import Message
from pathlib import Path

import pytest


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
