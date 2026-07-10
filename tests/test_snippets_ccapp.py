#!/usr/bin/env python3
"""Static assertions about bin/cc-app: shows the snippets button, dialog,
accelerator, and the tmux commands used to paste (bracketed, no Enter)."""
import re
from pathlib import Path

SRC = Path("bin/cc-app").read_text()


def test_snippets_helpers_defined():
    assert re.search(r"def\s+snip_load\s*\(", SRC)
    assert re.search(r"def\s+snip_save\s*\(", SRC)
    assert re.search(r"def\s+snip_paste\s*\(", SRC)


def test_snippets_dialog_class():
    assert re.search(r"class\s+SnippetsDialog\s*\(", SRC)


def test_header_button_snippets():
    # A button/widget whose tooltip mentions "Snippets"
    assert re.search(
        r'set_tooltip_text\(["\'][^"\']*Snippets[^"\']*["\']',
        SRC,
    ) or re.search(
        r'_icon_btn\(\s*["\']snippet["\'][^)]*Snippets',
        SRC,
    )


def test_shortcut_ctrl_shift_k():
    # <Control><Shift>K accelerator wired OR Gdk.KEY_K with Shift+Ctrl in a key-press handler
    assert re.search(r'<Control><Shift>K', SRC) or \
           re.search(r'Gdk\.KEY_K[\s\S]{0,400}(SHIFT|CONTROL)', SRC) or \
           re.search(r'(SHIFT|CONTROL)[\s\S]{0,400}Gdk\.KEY_K', SRC)


def test_paste_uses_bracketed_paste_and_no_enter():
    # snip_paste MUST call tmux with load-buffer + paste-buffer -p, and MUST NOT call send-keys with Enter
    m = re.search(r"def\s+snip_paste\s*\([\s\S]{0,2500}?\n(?=def\s|class\s|\Z)", SRC)
    assert m, "snip_paste body not found"
    body = m.group(0)
    assert "load-buffer" in body
    assert "paste-buffer" in body
    assert "-p" in body, "must use bracketed paste (-p)"
    assert "Enter" not in body, "snip_paste must NOT send Enter"
    assert "send-keys" not in body, "snip_paste must NOT use send-keys"
