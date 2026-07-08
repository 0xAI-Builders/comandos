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


if __name__ == "__main__":
    test_split_left_panel_is_the_scroll_container()
    test_desktop_panel_has_a_real_content_scroller()
