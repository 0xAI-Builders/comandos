#!/usr/bin/env python3
"""Static assertions about dash/index.html: markup + ICON + JS wiring
required by the snippets feature."""
import re
from pathlib import Path

HTML = Path("dash/index.html").read_text()


def test_icon_snippet_registered():
    # ICON.snippet must exist as an SVG string
    m = re.search(r"snippet\s*:\s*'<svg[^']+</svg>'", HTML)
    assert m, "ICON.snippet not registered in dash/index.html"


def test_header_button_snippets_present():
    m = re.search(
        r'<button\s+class="hdr-btn"\s+id="btn-snippets"[^>]*aria-label="snippets"[^>]*'
        r'title="[^"]*Ctrl\+Shift\+K[^"]*"',
        HTML,
    )
    assert m, "header button #btn-snippets not present with correct title"


def test_dialog_snippets_shell_present():
    assert re.search(r'<dialog\s+id="dlg-snippets"\s+class="modal', HTML)
    assert 'class="snip-list"' in HTML
    assert 'class="snip-pane"' in HTML


def test_ctrl_shift_k_binding_present():
    m = re.search(
        r"e\.ctrlKey\s*&&\s*e\.shiftKey\s*&&\s*[^\n]*(?:'K'|\"K\"|\.key\s*===\s*[\"']K[\"'])",
        HTML,
        re.IGNORECASE,
    )
    assert m, "Ctrl+Shift+K keydown binding not found"
    assert "dlg-snippets" in HTML


def test_dialog_dismisses_on_esc_via_native():
    # Uses <dialog>.showModal() (which enables native ESC-close)
    assert "showModal" in HTML, "must use dialog.showModal() to get native ESC handling"


def test_filter_snippets_function_present():
    assert re.search(r"function\s+filterSnippets\s*\(", HTML)


def test_render_snip_list_function_present():
    assert re.search(r"function\s+renderSnipList\s*\(", HTML)


def test_render_snip_pane_function_present():
    assert re.search(r"function\s+renderSnipPane\s*\(", HTML)


def test_open_snippets_fetches_from_endpoint():
    # openSnippets body must call api('/snippets') to load items
    m = re.search(r"function\s+openSnippets\s*\(\s*\)\s*\{[\s\S]{0,900}?\n\}\n", HTML)
    assert m, "openSnippets body not found"
    body = m.group(0)
    assert "api('/snippets'" in body or 'api("/snippets"' in body


def test_search_input_wired_to_render():
    # snip-q input must have an input listener that updates snipState.query
    assert re.search(r"snip-q[\s\S]{0,600}addEventListener\([\"']input[\"']", HTML)
    assert "snipState.query" in HTML
