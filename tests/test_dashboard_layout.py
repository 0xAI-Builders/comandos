#!/usr/bin/env python3
import re
from pathlib import Path


CSS = Path("dash/index.html").read_text()


def rule(selector: str) -> str:
    pattern = re.compile(re.escape(selector) + r"\{([^}]*)\}", re.S)
    match = pattern.search(CSS)
    assert match, f"missing CSS rule: {selector}"
    return re.sub(r"\s+", " ", match.group(1)).strip()


def block(selector: str) -> str:
    marker = selector + "{"
    start = CSS.index(marker) + len(marker)
    depth = 1
    for index in range(start, len(CSS)):
        if CSS[index] == "{":
            depth += 1
        elif CSS[index] == "}":
            depth -= 1
            if depth == 0:
                return CSS[start:index]
    raise AssertionError(f"unclosed CSS block: {selector}")


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


def test_phone_dashboard_keeps_servers_compact_and_session_actions_visible():
    mobile = block("@media (max-width:640px)")
    compact = re.sub(r"\s+", "", mobile)

    assert "#ssh-bar{flex-wrap:nowrap;overflow-x:auto;overflow-y:hidden" in compact
    assert ".ssh-chip,#ssh-manage{flex:none}" in compact
    assert ".row.rpath{display:none}" in compact
    assert ".row.name{flex-basis:140px}" in compact
    assert "#servers.modal-panel{padding:24px18px}" in compact
    assert ".srv-row{flex-wrap:wrap}" in compact
    assert ".srv-info{flex-basis:100%}" in compact
    assert ".srv-rowbutton{flex:110;min-height:36px}" in compact


def test_tablet_dashboard_wraps_session_rows_before_they_overflow():
    tablet = block("@media (max-width:900px)")
    compact = re.sub(r"\s+", "", tablet)

    assert ".row{flex-wrap:wrap" in compact
    assert ".row.rpath{max-width:35vw}" in compact
    assert ".row.acts{order:3;flex:10" in compact
    assert "width:100%;justify-content:flex-end;flex-wrap:wrap;opacity:1" in compact
    assert ".row.actsbutton{min-height:36px}" in compact
    assert ".row.actsbutton.term,.row.actsbutton.kill{width:36px}" in compact


def test_ssh_connection_list_is_an_independent_touch_scroller():
    saved = rule("#srv-list")

    assert "max-height:min(42dvh,420px)" in saved
    assert "overflow-y:auto" in saved
    assert "overflow-x:hidden" in saved
    assert "overscroll-behavior-y:contain" in saved
    assert "-webkit-overflow-scrolling:touch" in saved
    assert "touch-action:pan-y" in saved
    assert "scrollbar-gutter:stable" in saved
    assert "scrollbar-width:auto" in saved
    assert "width:10px" in rule("#srv-list::-webkit-scrollbar")
    assert (
        '<div id="srv-list" role="region" '
        'aria-label="Conexiones guardadas" tabindex="0"></div>'
    ) in CSS


if __name__ == "__main__":
    test_split_left_panel_is_the_scroll_container()
    test_desktop_panel_has_a_real_content_scroller()
    test_remote_shell_uses_dynamic_viewport_grid_without_fixed_tab_offset()
    test_remote_touch_targets_have_stable_minimums()
