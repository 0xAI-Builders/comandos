#!/usr/bin/env python3
import ast
import os
import re
import tempfile
import types
from pathlib import Path


SRC = Path("bin/cc-dash").read_text()


def load_remote_helpers():
    tree = ast.parse(SRC)
    funcs = {
        node.name: ast.get_source_segment(SRC, node)
        for node in tree.body
        if isinstance(node, ast.FunctionDef)
    }
    needed = ("remote_urls", "remote_status_from_text")
    missing = [name for name in needed if name not in funcs]
    assert not missing, f"missing helper(s): {', '.join(missing)}"

    ns = {"re": re}
    exec("\n\n".join(funcs[name] for name in needed), ns)
    return ns


def load_tmux_mouse_helper():
    tree = ast.parse(SRC)
    funcs = {
        node.name: ast.get_source_segment(SRC, node)
        for node in tree.body
        if isinstance(node, ast.FunctionDef)
    }
    assert "set_tmux_mouse" in funcs

    calls = []

    def fake_tmux(*args, timeout=5):
        calls.append(args)
        return types.SimpleNamespace(returncode=0, stdout="", stderr="")

    ns = {"tmux": fake_tmux}
    exec(funcs["set_tmux_mouse"], ns)
    return ns, calls


def load_functions(*names, extra=None):
    tree = ast.parse(SRC)
    funcs = {
        node.name: ast.get_source_segment(SRC, node)
        for node in tree.body
        if isinstance(node, ast.FunctionDef)
    }
    missing = [name for name in names if name not in funcs]
    assert not missing, f"missing helper(s): {', '.join(missing)}"
    ns = dict(extra or {})
    exec("\n\n".join(funcs[name] for name in names), ns)
    return ns


def test_remote_urls_include_existing_access_token():
    ns = load_remote_helpers()

    urls = ns["remote_urls"]("zion.tail63a117.ts.net", "abc123")

    assert urls["dashboard"] == "https://zion.tail63a117.ts.net/?token=abc123"
    assert urls["terminal"] == "https://zion.tail63a117.ts.net/term?token=abc123"
    assert urls["terminalFallback"] == "https://zion.tail63a117.ts.net:8443/?token=abc123"


def test_remote_status_detects_dashboard_and_terminal_routes():
    ns = load_remote_helpers()
    serve = """
https://zion.tail63a117.ts.net (tailnet only)
|-- /     proxy http://127.0.0.1:4777
|-- /term proxy http://127.0.0.1:4780/term

https://zion.tail63a117.ts.net:8443 (tailnet only)
|-- / proxy http://127.0.0.1:4779
"""

    status = ns["remote_status_from_text"](serve, True)

    assert status["remoteOn"] is True
    assert status["webtermOn"] is True
    assert status["termRouteOn"] is True
    assert status["fallbackRouteOn"] is True


def test_tab_history_is_treated_as_authenticated_live_api():
    assert '"/tab-history"' in SRC
    api_get = SRC.split("API_GET = ", 1)[1].split("def do_GET", 1)[0]
    assert '"/tab-history"' in api_get


def test_static_shell_also_uses_no_store_headers():
    assert "def end_headers(self):" in SRC
    assert "Cache-Control" in SRC
    assert "no-store" in SRC


def test_tmux_mouse_helper_changes_only_the_target_session():
    ns, calls = load_tmux_mouse_helper()

    err = ns["set_tmux_mouse"]("term-6629-3", False)

    assert err is None
    assert calls == [
        ("has-session", "-t", "=term-6629-3"),
        ("set-option", "-t", "term-6629-3", "mouse", "off"),
    ]


def test_tmux_mouse_endpoint_is_available_to_remote_terminal_ui():
    assert 'self.path == "/tmux-mouse"' in SRC


def test_parse_ssh_config_preserves_identity_file_for_existing_ui():
    ns = load_functions("parse_ssh_config", extra={"os": os})
    old_home = os.environ.get("HOME", "")
    with tempfile.TemporaryDirectory() as home:
        os.environ["HOME"] = home
        ssh_dir = Path(home) / ".ssh"
        ssh_dir.mkdir()
        (ssh_dir / "config").write_text(
            "Host prod\n"
            "    HostName 203.0.113.10\n"
            "    User root\n"
            "    Port 2222\n"
            "    IdentityFile ~/.ssh/prod_ed25519\n"
        )

        hosts = ns["parse_ssh_config"]()

    os.environ["HOME"] = old_home
    assert hosts == [{
        "host": "prod",
        "hostname": "203.0.113.10",
        "user": "root",
        "port": "2222",
        "identity": "~/.ssh/prod_ed25519",
    }]


def test_ssh_key_setup_starts_copy_id_tmux_session_for_saved_host():
    calls = []

    def fake_tmux(*args, timeout=5):
        calls.append(("tmux", args))
        if args[:2] == ("has-session", "-t"):
            return types.SimpleNamespace(returncode=1, stdout="", stderr="")
        return types.SimpleNamespace(returncode=0, stdout="", stderr="")

    def fake_run(args, capture_output=True, text=True, timeout=15):
        calls.append(("run", tuple(args)))
        return types.SimpleNamespace(returncode=0, stdout="", stderr="")

    def fake_which(name):
        if name == "ssh-copy-id":
            return "/usr/bin/ssh-copy-id"
        if name == "systemd-run":
            return "/usr/bin/systemd-run"
        return None

    extra = {
        "os": os,
        "shlex": __import__("shlex"),
        "shutil": types.SimpleNamespace(which=fake_which),
        "subprocess": types.SimpleNamespace(run=fake_run),
        "tmux": fake_tmux,
        "SSH_HOST_RE": re.compile(r"^[A-Za-z0-9._-]{1,80}$"),
    }
    ns = load_functions(
        "parse_ssh_config",
        "ssh_host_entry",
        "ssh_public_key_for_host",
        "ssh_key_setup",
        extra=extra,
    )
    old_home = os.environ.get("HOME", "")
    with tempfile.TemporaryDirectory() as home:
        os.environ["HOME"] = home
        ssh_dir = Path(home) / ".ssh"
        ssh_dir.mkdir()
        (ssh_dir / "id_ed25519.pub").write_text("ssh-ed25519 AAA test\n")
        (ssh_dir / "config").write_text("Host prod\n    HostName 203.0.113.10\n    User root\n")

        sess, err = ns["ssh_key_setup"]("prod")

    os.environ["HOME"] = old_home
    assert err is None
    assert sess == "ssh-key-prod"
    run_args = next(args for kind, args in calls if kind == "run")
    assert run_args[:7] == (
        "systemd-run", "--user", "--scope", "--collect", "--quiet", "tmux", "new-session"
    )
    command = run_args[-1]
    assert "ssh-copy-id" in command
    assert "prod" in command
    assert "id_ed25519.pub" in command


def test_ssh_key_setup_endpoint_is_available_from_existing_server_ui():
    assert 'self.path == "/ssh-key-setup"' in SRC


if __name__ == "__main__":
    test_remote_urls_include_existing_access_token()
    test_remote_status_detects_dashboard_and_terminal_routes()
    test_tab_history_is_treated_as_authenticated_live_api()
    test_static_shell_also_uses_no_store_headers()
    test_tmux_mouse_helper_changes_only_the_target_session()
    test_tmux_mouse_endpoint_is_available_to_remote_terminal_ui()
    test_parse_ssh_config_preserves_identity_file_for_existing_ui()
    test_ssh_key_setup_starts_copy_id_tmux_session_for_saved_host()
    test_ssh_key_setup_endpoint_is_available_from_existing_server_ui()
