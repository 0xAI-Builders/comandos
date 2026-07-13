#!/usr/bin/env python3
import ast
import os
import re
import subprocess
import tempfile
import textwrap
import types
from pathlib import Path


SRC = Path("bin/cc-dash").read_text()
CC_MOBILE = Path("bin/cc-mobile").resolve()


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


def load_tmux_mouse_helpers(responses=None):
    tree = ast.parse(SRC)
    funcs = {
        node.name: ast.get_source_segment(SRC, node)
        for node in tree.body
        if isinstance(node, ast.FunctionDef)
    }
    needed = ("get_tmux_mouse", "set_tmux_mouse")
    missing = [name for name in needed if name not in funcs]
    assert not missing, f"missing helper(s): {', '.join(missing)}"

    calls = []
    replies = iter(responses or [])

    def fake_tmux(*args, timeout=5):
        calls.append(args)
        try:
            reply = next(replies)
            if isinstance(reply, BaseException):
                raise reply
            return reply
        except StopIteration:
            pass
        return types.SimpleNamespace(returncode=0, stdout="", stderr="")

    ns = {"tmux": fake_tmux}
    exec("\n\n".join(funcs[name] for name in needed), ns)
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


def load_handler_method(name, extra=None):
    tree = ast.parse(SRC)
    handler = next(
        node for node in tree.body
        if isinstance(node, ast.ClassDef) and node.name == "Handler"
    )
    method = next(
        node for node in handler.body
        if isinstance(node, ast.FunctionDef) and node.name == name
    )
    ns = dict(extra or {})
    exec(textwrap.dedent(ast.get_source_segment(SRC, method)), ns)
    return ns[name]


def test_remote_urls_keep_dashboard_token_out_of_ttyd_urls():
    ns = load_remote_helpers()

    urls = ns["remote_urls"]("zion.tail63a117.ts.net", "abc123")

    assert urls["dashboard"] == "https://zion.tail63a117.ts.net/?token=abc123"
    assert urls["terminal"] == "https://zion.tail63a117.ts.net/term"
    assert urls["terminalFallback"] == "https://zion.tail63a117.ts.net:8443/"


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


def test_cc_mobile_off_disables_both_dashboard_and_terminal_https_routes(
        tmp_path):
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    calls = tmp_path / "tailscale-calls"
    tailscale = fake_bin / "tailscale"
    tailscale.write_text(
        "#!/bin/sh\n"
        "if [ \"$*\" = \"serve status\" ]; then\n"
        "  printf '%b' \"$SERVE_STATUS\"\n"
        "  exit \"$STATUS_RC\"\n"
        "fi\n"
        "printf '%s\\n' \"$*\" >> \"$TAILSCALE_CALLS\"\n"
        "case \"$*\" in\n"
        "  *\"--https=$FAIL_HTTPS\"*) exit 1 ;;\n"
        "esac\n"
    )
    tailscale.chmod(0o755)
    env = {
        **os.environ,
        "PATH": f"{fake_bin}:{os.environ['PATH']}",
        "TAILSCALE_CALLS": str(calls),
        "FAIL_HTTPS": "never",
        "SERVE_STATUS": "",
        "STATUS_RC": "0",
    }

    subprocess.run(
        [str(CC_MOBILE), "off"], env=env, check=True,
        text=True, capture_output=True,
    )

    assert calls.read_text().splitlines() == [
        "serve --https=443 off",
        "serve --https=8443 off",
    ]


def test_cc_mobile_off_fails_closed_without_resetting_unrelated_serve_config(
        tmp_path):
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    calls = tmp_path / "tailscale-calls"
    tailscale = fake_bin / "tailscale"
    tailscale.write_text(
        "#!/bin/sh\n"
        "if [ \"$*\" = \"serve status\" ]; then\n"
        "  printf '%b' \"$SERVE_STATUS\"\n"
        "  exit \"$STATUS_RC\"\n"
        "fi\n"
        "printf '%s\\n' \"$*\" >> \"$TAILSCALE_CALLS\"\n"
        "case \"$*\" in\n"
        "  *\"--https=$FAIL_HTTPS\"*) exit 1 ;;\n"
        "esac\n"
    )
    tailscale.chmod(0o755)
    env = {
        **os.environ,
        "PATH": f"{fake_bin}:{os.environ['PATH']}",
        "TAILSCALE_CALLS": str(calls),
        "FAIL_HTTPS": "8443",
        "SERVE_STATUS": (
            "https://node.example.ts.net:8443\\n"
            "|-- / proxy http://127.0.0.1:4779\\n"
        ),
        "STATUS_RC": "0",
    }

    result = subprocess.run(
        [str(CC_MOBILE), "off"], env=env, check=False,
        text=True, capture_output=True,
    )

    assert result.returncode != 0
    assert "No pude deshabilitar" in result.stderr
    assert calls.read_text().splitlines() == [
        "serve --https=443 off",
        "serve --https=8443 off",
    ]


def test_cc_mobile_off_accepts_an_already_absent_terminal_handler(tmp_path):
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    calls = tmp_path / "tailscale-calls"
    tailscale = fake_bin / "tailscale"
    tailscale.write_text(
        "#!/bin/sh\n"
        "if [ \"$*\" = \"serve status\" ]; then exit 0; fi\n"
        "printf '%s\\n' \"$*\" >> \"$TAILSCALE_CALLS\"\n"
        "case \"$*\" in\n"
        "  *\"--https=8443\"*) exit 1 ;;\n"
        "esac\n"
    )
    tailscale.chmod(0o755)
    env = {
        **os.environ,
        "PATH": f"{fake_bin}:{os.environ['PATH']}",
        "TAILSCALE_CALLS": str(calls),
    }

    result = subprocess.run(
        [str(CC_MOBILE), "off"], env=env, check=False,
        text=True, capture_output=True,
    )

    assert result.returncode == 0
    assert calls.read_text().splitlines() == [
        "serve --https=443 off",
        "serve --https=8443 off",
    ]


def test_tab_history_is_treated_as_authenticated_live_api():
    assert '"/tab-history"' in SRC
    api_get = SRC.split("API_GET = ", 1)[1].split("def do_GET", 1)[0]
    assert '"/tab-history"' in api_get


def test_static_shell_also_uses_no_store_headers():
    assert "def end_headers(self):" in SRC
    assert "Cache-Control" in SRC
    assert "no-store" in SRC


def test_tmux_mouse_helper_changes_only_the_target_session():
    ns, calls = load_tmux_mouse_helpers()

    err = ns["set_tmux_mouse"]("term-6629-3", False)

    assert err is None
    assert calls == [
        ("has-session", "-t", "=term-6629-3"),
        ("set-option", "-t", "term-6629-3", "mouse", "off"),
    ]


def test_get_tmux_mouse_helper_reads_target_session():
    ns, calls = load_tmux_mouse_helpers([
        types.SimpleNamespace(returncode=0, stdout="", stderr=""),
        types.SimpleNamespace(returncode=0, stdout="off\n", stderr=""),
    ])

    enabled, err = ns["get_tmux_mouse"]("ssh-prod")

    assert err is None
    assert enabled is False
    assert calls == [
        ("has-session", "-t", "=ssh-prod"),
        ("show-options", "-A", "-v", "-t", "ssh-prod", "mouse"),
    ]


def test_get_tmux_mouse_helper_parses_on_and_reports_tmux_errors():
    ok_ns, _ = load_tmux_mouse_helpers([
        types.SimpleNamespace(returncode=0, stdout="", stderr=""),
        types.SimpleNamespace(returncode=0, stdout="on\n", stderr=""),
    ])
    assert ok_ns["get_tmux_mouse"]("dev") == (True, None)

    missing_ns, missing_calls = load_tmux_mouse_helpers([
        types.SimpleNamespace(returncode=1, stdout="", stderr="missing"),
    ])
    assert missing_ns["get_tmux_mouse"]("gone") == (
        None, "No hay sesion tmux 'gone'"
    )
    assert missing_calls == [("has-session", "-t", "=gone")]

    failed_ns, _ = load_tmux_mouse_helpers([
        types.SimpleNamespace(returncode=0, stdout="", stderr=""),
        types.SimpleNamespace(returncode=1, stdout="", stderr="show failed\n"),
    ])
    assert failed_ns["get_tmux_mouse"]("dev") == (None, "show failed")


def test_tmux_mouse_helpers_convert_process_exceptions_to_api_errors():
    get_ns, _ = load_tmux_mouse_helpers([TimeoutError("tmux timed out")])
    enabled, get_error = get_ns["get_tmux_mouse"]("dev")
    assert enabled is None
    assert "tmux timed out" in get_error

    set_ns, _ = load_tmux_mouse_helpers([TimeoutError("tmux timed out")])
    set_error = set_ns["set_tmux_mouse"]("dev", True)
    assert "tmux timed out" in set_error


def test_tmux_mouse_get_is_authenticated_and_post_stays_available():
    assert 'self.path == "/tmux-mouse"' in SRC
    api_get = SRC.split("API_GET = ", 1)[1].split("def do_GET", 1)[0]
    assert '"/tmux-mouse"' in api_get
    get_route = SRC.split("def _do_GET(self):", 1)[1].split("def do_POST(self):", 1)[0]
    assert 'self.path.startswith("/tmux-mouse?")' in get_route
    assert "get_tmux_mouse(sess)" in get_route
    assert '"mouse": "on" if enabled else "off"' in get_route


def test_tmux_mouse_get_route_validates_session_and_maps_failures():
    session_re = re.compile(r"^[A-Za-z0-9._-]{1,80}$")
    replies = {
        "missing": (None, "No hay sesion tmux 'missing'"),
        "broken": (None, "show failed"),
        "enabled": (True, None),
        "disabled": (False, None),
    }
    route = load_handler_method(
        "_do_GET",
        extra={"SESSION_RE": session_re, "get_tmux_mouse": replies.__getitem__},
    )

    class Request:
        def __init__(self, path):
            self.path = path

        @staticmethod
        def _json(code, body):
            return code, body

    assert route(Request("/tmux-mouse")) == (
        400, {"error": "Nombre de sesion invalido"}
    )
    assert route(Request("/tmux-mouse?session=bad%2Fname"))[0] == 400
    assert route(Request("/tmux-mouse?session=missing"))[0] == 404
    assert route(Request("/tmux-mouse?session=broken"))[0] == 500
    assert route(Request("/tmux-mouse?session=enabled")) == (200, {"mouse": "on"})
    assert route(Request("/tmux-mouse?session=disabled")) == (200, {"mouse": "off"})


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
    run_args = next(args for kind, args in calls if kind == "run" and "new-session" in args)
    assert run_args[:7] == (
        "systemd-run", "--user", "--scope", "--collect", "--quiet", "tmux", "new-session"
    )
    command = run_args[-1]
    assert "ssh-copy-id" in command
    assert "prod" in command
    assert "id_ed25519.pub" in command


def test_ssh_key_setup_endpoint_is_available_from_existing_server_ui():
    assert 'self.path == "/ssh-key-setup"' in SRC


def test_ssh_connect_disables_tmux_mouse_for_native_text_selection():
    calls = []

    def fake_tmux(*args, timeout=5):
        calls.append(("tmux", args))
        if args == ("has-session", "-t", "=ssh-prod"):
            return types.SimpleNamespace(returncode=1, stdout="", stderr="")
        return types.SimpleNamespace(returncode=0, stdout="", stderr="")

    def fake_run(args, capture_output=True, text=True, timeout=15):
        calls.append(("run", tuple(args)))
        return types.SimpleNamespace(returncode=0, stdout="", stderr="")

    extra = {
        "os": os,
        "shlex": __import__("shlex"),
        "subprocess": types.SimpleNamespace(run=fake_run),
        "tmux": fake_tmux,
        "SSH_HOST_RE": re.compile(r"^[A-Za-z0-9._-]{1,80}$"),
    }
    ns = load_functions("parse_ssh_config", "ssh_connect", extra=extra)
    old_home = os.environ.get("HOME", "")
    with tempfile.TemporaryDirectory() as home:
        os.environ["HOME"] = home
        ssh_dir = Path(home) / ".ssh"
        ssh_dir.mkdir()
        (ssh_dir / "config").write_text("Host prod\n    HostName 203.0.113.10\n    User root\n")

        sess, connected, note = ns["ssh_connect"]("prod")

    os.environ["HOME"] = old_home
    assert sess == "ssh-prod"
    assert connected is True
    assert note is None
    assert ("tmux", ("set-option", "-t", "ssh-prod", "mouse", "off")) in calls


def test_ssh_new_tab_creates_unique_tmux_session_for_saved_host():
    calls = []

    def fake_tmux(*args, timeout=5):
        calls.append(("tmux", args))
        if args == ("has-session", "-t", "=sshtab-prod-1"):
            return types.SimpleNamespace(returncode=0, stdout="", stderr="")
        if args[:2] == ("has-session", "-t"):
            return types.SimpleNamespace(returncode=1, stdout="", stderr="")
        return types.SimpleNamespace(returncode=0, stdout="", stderr="")

    def fake_run(args, capture_output=True, text=True, timeout=15):
        calls.append(("run", tuple(args)))
        return types.SimpleNamespace(returncode=0, stdout="", stderr="")

    labels = {}

    extra = {
        "os": os,
        "shlex": __import__("shlex"),
        "subprocess": types.SimpleNamespace(run=fake_run),
        "tmux": fake_tmux,
        "scope_cmd": lambda argv: ["scope", *argv],
        "write_app_tab": lambda sess, label: labels.setdefault(sess, label),
        "SSH_HOST_RE": re.compile(r"^[A-Za-z0-9._-]{1,80}$"),
    }
    ns = load_functions(
        "parse_ssh_config",
        "ssh_host_entry",
        "ssh_new_tab_session",
        "ssh_open_new_tab",
        extra=extra,
    )
    old_home = os.environ.get("HOME", "")
    with tempfile.TemporaryDirectory() as home:
        os.environ["HOME"] = home
        ssh_dir = Path(home) / ".ssh"
        ssh_dir.mkdir()
        (ssh_dir / "config").write_text("Host prod\n    HostName 203.0.113.10\n    User root\n")

        sess, connected, note = ns["ssh_open_new_tab"]("prod")

    os.environ["HOME"] = old_home
    assert sess == "sshtab-prod-2"
    assert connected is True
    assert note is None
    assert labels == {"sshtab-prod-2": "prod"}
    run_args = next(args for kind, args in calls if kind == "run" and "new-session" in args)
    assert run_args[:5] == ("scope", "tmux", "new-session", "-d", "-s")
    assert run_args[5] == "sshtab-prod-2"
    assert "ssh prod" in run_args[-1]
    assert ("tmux", ("set-option", "-t", "sshtab-prod-2", "mouse", "off")) in calls


def test_ssh_new_tab_endpoint_is_available_for_left_click_chips():
    assert 'self.path == "/ssh-new-tab"' in SRC


if __name__ == "__main__":
    test_remote_urls_keep_dashboard_token_out_of_ttyd_urls()
    test_remote_status_detects_dashboard_and_terminal_routes()
    test_tab_history_is_treated_as_authenticated_live_api()
    test_static_shell_also_uses_no_store_headers()
    test_tmux_mouse_helper_changes_only_the_target_session()
    test_get_tmux_mouse_helper_reads_target_session()
    test_get_tmux_mouse_helper_parses_on_and_reports_tmux_errors()
    test_tmux_mouse_helpers_convert_process_exceptions_to_api_errors()
    test_tmux_mouse_get_is_authenticated_and_post_stays_available()
    test_tmux_mouse_get_route_validates_session_and_maps_failures()
    test_parse_ssh_config_preserves_identity_file_for_existing_ui()
    test_ssh_key_setup_starts_copy_id_tmux_session_for_saved_host()
    test_ssh_key_setup_endpoint_is_available_from_existing_server_ui()
    test_ssh_connect_disables_tmux_mouse_for_native_text_selection()
    test_ssh_new_tab_creates_unique_tmux_session_for_saved_host()
    test_ssh_new_tab_endpoint_is_available_for_left_click_chips()
