#!/usr/bin/env python3
import ast
import re
import shutil
import subprocess
import sys
import types
from pathlib import Path


SRC = Path("bin/cc-app").read_text()


def load_url_helpers():
    tree = ast.parse(SRC)
    funcs = {
        node.name: ast.get_source_segment(SRC, node)
        for node in tree.body
        if isinstance(node, ast.FunctionDef)
    }
    needed = ("normalize_wrapped_url_text", "url_from_wrapped_text")
    missing = [name for name in needed if name not in funcs]
    assert not missing, f"missing helper(s): {', '.join(missing)}"

    url_re = re.search(r'^URL_RE\s*=\s*(.+)$', SRC, re.M)
    assert url_re, "missing URL_RE"
    ns = {"re": re}
    exec(f"URL_RE = {url_re.group(1)}\n" + "\n\n".join(funcs[name] for name in needed), ns)
    return ns


def load_ssh_session_helpers():
    tree = ast.parse(SRC)
    funcs = {
        node.name: ast.get_source_segment(SRC, node)
        for node in tree.body
        if isinstance(node, ast.FunctionDef)
    }
    needed = ("ssh_host_from_session",)
    missing = [name for name in needed if name not in funcs]
    assert not missing, f"missing helper(s): {', '.join(missing)}"

    ns = {}
    exec("\n\n".join(funcs[name] for name in needed), ns)
    return ns


def load_copy_mode_helpers(fake_tmuxc):
    tree = ast.parse(SRC)
    funcs = {
        node.name: ast.get_source_segment(SRC, node)
        for node in tree.body
        if isinstance(node, ast.FunctionDef)
    }
    needed = ("copy_mode_pane",)
    missing = [name for name in needed if name not in funcs]
    assert not missing, f"missing helper(s): {', '.join(missing)}"

    ns = {"tmuxc": fake_tmuxc}
    exec("\n\n".join(funcs[name] for name in needed), ns)
    return ns


def load_select_pane_helper(fake_tmuxc, fake_pane_at):
    tree = ast.parse(SRC)
    funcs = {
        node.name: ast.get_source_segment(SRC, node)
        for node in tree.body
        if isinstance(node, ast.FunctionDef)
    }
    needed = ("select_pane_at_event",)
    missing = [name for name in needed if name not in funcs]
    assert not missing, f"missing helper(s): {', '.join(missing)}"

    ns = {"tmuxc": fake_tmuxc, "pane_at": fake_pane_at}
    exec("\n\n".join(funcs[name] for name in needed), ns)
    return ns


def load_tmux_copy_helpers(fake_tmuxc, fake_gtk, fake_gdk):
    tree = ast.parse(SRC)
    funcs = {
        node.name: ast.get_source_segment(SRC, node)
        for node in tree.body
        if isinstance(node, ast.FunctionDef)
    }
    needed = ("copy_text_to_clipboard", "copy_tmux_selection_to_clipboard")
    missing = [name for name in needed if name not in funcs]
    assert not missing, f"missing helper(s): {', '.join(missing)}"

    ns = {"tmuxc": fake_tmuxc, "Gtk": fake_gtk, "Gdk": fake_gdk}
    exec("\n\n".join(funcs[name] for name in needed), ns)
    return ns


def load_mouse_helpers(fake_pane_at, fake_tmuxc, fake_open_url,
                       fake_select_pane_at_event):
    ns = {
        "Gdk": types.SimpleNamespace(
            ModifierType=types.SimpleNamespace(CONTROL_MASK=1)
        ),
        "copy_vte_selection": lambda _term: True,
        "open_url": fake_open_url,
        "pane_at": fake_pane_at,
        "select_pane_at_event": fake_select_pane_at_event,
        "tmuxc": fake_tmuxc,
        "url_at_event": lambda _term, _event, match: match or None,
    }
    exec("\n\n".join((function_source("on_term_release"),
                       function_source("on_term_button"))), ns)
    return ns


def load_open_url_helper(gio_launcher, popen, gtk_show_uri=None):
    fake_gio = types.SimpleNamespace(
        AppInfo=types.SimpleNamespace(launch_default_for_uri=gio_launcher)
    )
    fake_subprocess = types.SimpleNamespace(
        DEVNULL=subprocess.DEVNULL,
        Popen=popen,
    )
    ns = {
        "Gio": fake_gio,
        "Gtk": types.SimpleNamespace(
            show_uri_on_window=gtk_show_uri or (
                lambda _parent, _url, _timestamp: False
            )
        ),
        "shutil": shutil,
        "subprocess": fake_subprocess,
        "sys": types.SimpleNamespace(platform="linux"),
    }
    exec(function_source("open_url"), ns)
    return ns


class FakeTerm:
    def __init__(self, url=None, selected=False):
        self.url = url
        self.selected = selected

    def match_check_event(self, _event):
        return self.url, 1

    def get_has_selection(self):
        return self.selected

    def get_toplevel(self):
        return self


class MouseEvent:
    def __init__(self, button, x, y, state=0, timestamp=4242):
        self.button = button
        self.x = x
        self.y = y
        self.state = state
        self.time = timestamp


def function_source(name):
    tree = ast.parse(SRC)
    for node in tree.body:
        if isinstance(node, ast.FunctionDef) and node.name == name:
            return ast.get_source_segment(SRC, node)
    raise AssertionError(f"missing helper: {name}")


def test_wrapped_url_text_is_joined_before_opening():
    ns = load_url_helpers()
    text = "abre https://example.com/a/very/\nlong/path?x=1&y=2 ahora"

    assert ns["normalize_wrapped_url_text"](text) == (
        "abre https://example.com/a/very/long/path?x=1&y=2 ahora"
    )


def test_wrapped_url_can_be_selected_from_first_or_second_visual_line():
    ns = load_url_helpers()
    text = "abre https://example.com/a/very/\nlong/path?x=1&y=2 ahora"
    full = "https://example.com/a/very/long/path?x=1&y=2"

    assert ns["url_from_wrapped_text"](text, "https://example.com/a/very/") == full
    assert ns["url_from_wrapped_text"](text, "long/path?x=1&y=2") == full
    assert ns["url_from_wrapped_text"](text, "") == full


def test_fresh_ssh_tab_sessions_are_recognized_as_ssh():
    ns = load_ssh_session_helpers()

    assert ns["ssh_host_from_session"]("ssh-prod") == "prod"
    assert ns["ssh_host_from_session"]("sshtab-prod-2") == "prod"
    assert ns["ssh_host_from_session"]("sshtab-prod-east-12") == "prod-east"
    assert ns["ssh_host_from_session"]("term-123") is None


def test_opening_ssh_tabs_disables_tmux_mouse_for_text_selection():
    src = function_source("open_tab")

    assert "ssh_host_from_session(sess)" in src
    assert '"set-option", "-t", sess, "mouse", "off"' in src


def test_copy_mode_pane_scans_split_panes_for_selection():
    calls = []

    def fake_tmuxc(*args):
        calls.append(args)
        if args[:4] == ("display-message", "-p", "-t", "%1"):
            return types.SimpleNamespace(returncode=0, stdout="0\n", stderr="")
        if args[:3] == ("list-panes", "-t", "=ssh-prod:"):
            return types.SimpleNamespace(returncode=0, stdout="%1|0\n%2|1\n", stderr="")
        return types.SimpleNamespace(returncode=1, stdout="", stderr="")

    ns = load_copy_mode_helpers(fake_tmuxc)

    assert ns["copy_mode_pane"]("ssh-prod", "%1") == "%2"
    assert ("list-panes", "-t", "=ssh-prod:", "-F", "#{pane_id}|#{pane_in_mode}") in calls


def test_terminal_copy_menu_remains_clickable_when_selection_detection_is_flaky():
    src = function_source("on_term_button")

    assert "copy_mode_pane(cur_sess, pane_id)" in src
    assert 'sensitive=True' in src


def test_clicking_split_pane_selects_it_without_stealing_terminal_mouse_event():
    calls = []

    def fake_tmuxc(*args):
        calls.append(args)
        return types.SimpleNamespace(returncode=0, stdout="", stderr="")

    def fake_pane_at(term, event):
        return "%2", 2

    ns = load_select_pane_helper(fake_tmuxc, fake_pane_at)

    assert ns["select_pane_at_event"](object(), object()) == "%2"
    assert calls == [("select-pane", "-t", "%2")]

    src = function_source("on_term_button")
    assert "pane_at(term, event)" in src
    assert "select_pane_at_event(term, event)" not in src


def test_clean_url_click_opens_despite_preexisting_selection():
    opened = []
    selected_on_press = []
    tmux_calls = []

    ns = load_mouse_helpers(
        lambda _term, _event: ("%2", 2),
        lambda *args: tmux_calls.append(args),
        lambda url, *_context: opened.append(url),
        lambda term, event: selected_on_press.append((term, event)),
    )
    term = FakeTerm("https://example.com/path", selected=True)
    press = MouseEvent(1, 10, 20)
    release = MouseEvent(1, 11, 21)

    assert ns["on_term_button"](term, press) is False
    assert selected_on_press == []
    assert tmux_calls == []
    assert ns["on_term_release"](term, release) is False

    assert tmux_calls == [("select-pane", "-t", "%2")]
    assert opened == ["https://example.com/path"]


def test_primary_drag_neither_opens_url_nor_selects_pane():
    opened = []
    selected_on_press = []
    tmux_calls = []
    ns = load_mouse_helpers(
        lambda _term, _event: ("%2", 2),
        lambda *args: tmux_calls.append(args),
        lambda url, *_context: opened.append(url),
        lambda term, event: selected_on_press.append((term, event)),
    )
    term = FakeTerm("https://example.com/path")

    ns["on_term_button"](term, MouseEvent(1, 10, 20))
    ns["on_term_release"](term, MouseEvent(1, 30, 20))

    assert selected_on_press == []
    assert tmux_calls == []
    assert opened == []


def test_clean_non_link_click_selects_recorded_split_only_on_release():
    selected_on_press = []
    tmux_calls = []
    ns = load_mouse_helpers(
        lambda _term, _event: ("%7", 2),
        lambda *args: tmux_calls.append(args),
        lambda _url, *_context: None,
        lambda term, event: selected_on_press.append((term, event)),
    )
    term = FakeTerm()

    ns["on_term_button"](term, MouseEvent(1, 8, 9))
    assert selected_on_press == []
    assert tmux_calls == []
    ns["on_term_release"](term, MouseEvent(1, 8, 9))

    assert tmux_calls == [("select-pane", "-t", "%7")]


def test_ctrl_click_remains_an_immediate_link_command():
    opened = []
    ns = load_mouse_helpers(
        lambda _term, _event: ("%2", 2),
        lambda *_args: None,
        lambda url, *_context: opened.append(url),
        lambda _term, _event: None,
    )
    term = FakeTerm("https://example.com/ctrl")

    handled = ns["on_term_button"](term, MouseEvent(1, 5, 6, state=1))

    assert handled is True
    assert opened == ["https://example.com/ctrl"]


def test_clean_link_click_passes_window_and_event_timestamp():
    opened = []
    ns = load_mouse_helpers(
        lambda _term, _event: ("%2", 2),
        lambda *_args: None,
        lambda *args: opened.append(args),
        lambda _term, _event: None,
    )
    term = FakeTerm("https://example.com/focus")

    ns["on_term_button"](term, MouseEvent(1, 5, 6, timestamp=7001))
    ns["on_term_release"](term, MouseEvent(1, 5, 6, timestamp=7002))

    assert opened == [("https://example.com/focus", term, 7002)]


def test_linux_open_url_prefers_gtk_click_context_for_focus():
    gtk_calls = []
    gio_calls = []
    popen_calls = []
    ns = load_open_url_helper(
        lambda url, context: gio_calls.append((url, context)) or True,
        lambda argv, **kwargs: popen_calls.append((argv, kwargs)),
        lambda parent, url, timestamp: (
            gtk_calls.append((parent, url, timestamp)) or True
        ),
    )
    parent = object()

    assert ns["open_url"]("https://example.com/focus", parent, 7002) is True
    assert gtk_calls == [(parent, "https://example.com/focus", 7002)]
    assert gio_calls == []
    assert popen_calls == []


def test_linux_open_url_prefers_gio_without_spawning_xdg_open():
    gio_calls = []
    popen_calls = []
    ns = load_open_url_helper(
        lambda url, context: gio_calls.append((url, context)) or True,
        lambda argv, **kwargs: popen_calls.append((argv, kwargs)),
    )

    assert ns["open_url"]("https://example.com/gio") is True
    assert gio_calls == [("https://example.com/gio", None)]
    assert popen_calls == []


def test_linux_open_url_falls_back_to_xdg_open_exactly_once():
    gio_calls = []
    popen_calls = []

    def fail_gio(url, context):
        gio_calls.append((url, context))
        raise RuntimeError("no default GIO handler")

    ns = load_open_url_helper(
        fail_gio,
        lambda argv, **kwargs: popen_calls.append((argv, kwargs)),
    )

    assert ns["open_url"]("https://example.com/fallback") is True
    assert gio_calls == [("https://example.com/fallback", None)]
    assert [call[0] for call in popen_calls] == [
        ["xdg-open", "https://example.com/fallback"]
    ]


def test_linux_open_url_falls_back_when_gio_returns_false():
    popen_calls = []
    ns = load_open_url_helper(
        lambda _url, _context: False,
        lambda argv, **kwargs: popen_calls.append((argv, kwargs)),
    )

    assert ns["open_url"]("https://example.com/gio-false") is True
    assert [call[0] for call in popen_calls] == [
        ["xdg-open", "https://example.com/gio-false"]
    ]


def test_linux_open_url_reports_total_dispatch_failure():
    def fail_gio(_url, _context):
        raise RuntimeError("gio failed")

    def fail_xdg(_argv, **_kwargs):
        raise OSError("xdg-open missing")

    ns = load_open_url_helper(fail_gio, fail_xdg)

    assert ns["open_url"]("https://example.com/unavailable") is False


def test_tmux_copy_selection_goes_through_gtk_clipboard_not_xclip_only():
    calls = []
    copied = []

    def fake_tmuxc(*args):
        calls.append(args)
        if args == ("send-keys", "-t", "%2", "-X", "copy-selection-no-clear"):
            return types.SimpleNamespace(returncode=0, stdout="", stderr="")
        if args == ("save-buffer", "-"):
            return types.SimpleNamespace(returncode=0, stdout="selected text", stderr="")
        return types.SimpleNamespace(returncode=1, stdout="", stderr="")

    class FakeClipboard:
        def set_text(self, text, length):
            copied.append((text, length))

        def store(self):
            copied.append(("stored", None))

    fake_gtk = types.SimpleNamespace(Clipboard=types.SimpleNamespace(get=lambda selection: FakeClipboard()))
    fake_gdk = types.SimpleNamespace(SELECTION_CLIPBOARD="clipboard")
    ns = load_tmux_copy_helpers(fake_tmuxc, fake_gtk, fake_gdk)

    assert ns["copy_tmux_selection_to_clipboard"]("%2") is True
    assert ("save-buffer", "-") in calls
    assert copied == [("selected text", -1), ("stored", None)]


def test_vte_selection_copy_is_remembered_for_context_menu_fallback():
    copy_src = function_source("copy_vte_selection")
    menu_src = function_source("on_term_button")

    assert "_last_selection_copy" in copy_src
    assert "_last_selection_copy" in menu_src


if __name__ == "__main__":
    test_wrapped_url_text_is_joined_before_opening()
    test_wrapped_url_can_be_selected_from_first_or_second_visual_line()
    test_fresh_ssh_tab_sessions_are_recognized_as_ssh()
    test_opening_ssh_tabs_disables_tmux_mouse_for_text_selection()
    test_copy_mode_pane_scans_split_panes_for_selection()
    test_terminal_copy_menu_remains_clickable_when_selection_detection_is_flaky()
    test_clicking_split_pane_selects_it_without_stealing_terminal_mouse_event()
    test_clean_url_click_opens_despite_preexisting_selection()
    test_primary_drag_neither_opens_url_nor_selects_pane()
    test_clean_non_link_click_selects_recorded_split_only_on_release()
    test_ctrl_click_remains_an_immediate_link_command()
    test_clean_link_click_passes_window_and_event_timestamp()
    test_linux_open_url_prefers_gtk_click_context_for_focus()
    test_linux_open_url_prefers_gio_without_spawning_xdg_open()
    test_linux_open_url_falls_back_to_xdg_open_exactly_once()
    test_linux_open_url_falls_back_when_gio_returns_false()
    test_linux_open_url_reports_total_dispatch_failure()
    test_tmux_copy_selection_goes_through_gtk_clipboard_not_xclip_only()
    test_vte_selection_copy_is_remembered_for_context_menu_fallback()
