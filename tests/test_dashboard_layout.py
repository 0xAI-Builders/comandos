#!/usr/bin/env python3
import re
from pathlib import Path


CSS = Path("dash/index.html").read_text()


def rule(selector: str) -> str:
    pattern = re.compile(re.escape(selector) + r"\{([^}]*)\}", re.S)
    match = pattern.search(CSS)
    assert match, f"missing CSS rule: {selector}"
    return re.sub(r"\s+", " ", match.group(1)).strip()


def test_split_left_panel_is_the_scroll_container():
    panel = rule("body.app.split #view-panel")
    assert "min-height:0" in panel
    assert "overflow-y:auto" in panel
    assert "overflow-x:hidden" in panel

    content = rule("body.app.split #view-panel #content")
    assert "overflow:visible" in content
    assert "flex:none" in content


def test_desktop_panel_has_a_real_content_scroller():
    panes = rule("#panes")
    assert "display:flex" in panes
    assert "flex-direction:column" in panes

    panel = rule("#view-panel")
    assert "display:flex" in panel
    assert "flex-direction:column" in panel
    assert "min-height:0" in panel


def test_remote_shell_uses_dynamic_viewport_grid_without_fixed_tab_offset():
    assert "interactive-widget=resizes-content" in CSS
    app = rule("body.app")
    assert "var(--app-height,100dvh)" in app
    panes = rule("body.app #panes")
    assert "grid-template-rows:auto minmax(0,1fr)" in panes
    assert "safe-area-inset-top" in panes
    assert "safe-area-inset-bottom" in panes
    narrow = rule("body.app #view-panel,body.app #term-area")
    assert "top:44px" not in narrow
    assert "position:absolute" not in narrow


def test_remote_touch_targets_have_stable_minimums():
    tab = rule(".apptab")
    assert "min-height:44px" in tab
    splitter = rule("body.app.split #splitter::before")
    assert "inset:0 -12px" in splitter


if __name__ == "__main__":
    test_split_left_panel_is_the_scroll_container()
    test_desktop_panel_has_a_real_content_scroller()
    test_remote_shell_uses_dynamic_viewport_grid_without_fixed_tab_offset()
    test_remote_touch_targets_have_stable_minimums()
