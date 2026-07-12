#!/usr/bin/env python3
import ast
import re
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
    assert "select_pane_at_event(term, event)" in src


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
    test_tmux_copy_selection_goes_through_gtk_clipboard_not_xclip_only()
    test_vte_selection_copy_is_remembered_for_context_menu_fallback()
