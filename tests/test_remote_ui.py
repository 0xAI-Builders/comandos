#!/usr/bin/env python3
import json
import subprocess
from pathlib import Path


HTML = Path("dash/index.html").read_text()
SW = Path("dash/sw.js").read_text()


def extract_js_function(source, name):
    needles = (f"function {name}(", f"async function {name}(")
    start = None
    for needle in needles:
        try:
            start = source.index(needle)
            break
        except ValueError:
            pass
    if start is None:
        raise ValueError(f"{name} not found")
    brace = source.index("{", start)
    depth = 0
    for i in range(brace, len(source)):
        ch = source[i]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return source[start : i + 1]
    raise AssertionError(f"Could not extract {name}")


def remote_button_state(state, busy=False):
    fn = extract_js_function(HTML, "remoteButtonState")
    script = f"""
{fn}
console.log(JSON.stringify(remoteButtonState({json.dumps(state)}, {str(busy).lower()})));
"""
    out = subprocess.check_output(["node", "-e", script], text=True)
    return json.loads(out)


def test_remote_drawer_controls_are_present():
    assert 'id="btn-remote"' in HTML
    assert 'id="remote"' in HTML
    assert 'id="remote-status"' in HTML
    assert 'id="remote-qr"' in HTML
    assert 'id="remote-dashboard-url"' in HTML
    assert 'id="remote-term-url"' in HTML
    assert 'id="remote-on"' in HTML
    assert 'id="remote-off"' in HTML
    assert 'id="remote-webterm-on"' in HTML
    assert 'id="remote-webterm-off"' in HTML
    assert 'id="remote-open-terminal"' in HTML


def test_remote_ui_calls_backend_endpoints():
    for endpoint in (
        "/remote-state",
        "/remote-on",
        "/remote-off",
        "/remote-webterm-on",
        "/remote-webterm-off",
        "/remote-qr.png",
    ):
        assert endpoint in HTML


def test_remote_polling_slows_down_when_remote_webterm_is_enabled():
    assert "remotePollSeconds()" in HTML
    assert "document.hidden" in HTML


def test_remote_terminal_can_be_opened_from_remote_drawer():
    assert "ensureRemoteTerminalVisible" in HTML
    assert 'remoteAction("/remote-webterm-on"' in HTML
    assert 'ensureRemoteTerminalVisible();' in HTML


def test_open_terminal_button_dismisses_remote_drawer():
    fn = extract_js_function(HTML, "ensureRemoteTerminalVisible")
    assert '$("#remote").classList.remove("open")' in fn


def test_remote_terminal_iframe_wires_wheel_and_touch_to_xterm_scrollback():
    fn = extract_js_function(HTML, "wireTermFrameScroll")
    assert "scrollLines" in fn
    assert "wheel" in fn
    assert "touchstart" in fn
    assert "touchmove" in fn
    assert "passive:false" in fn
    style_fn = extract_js_function(HTML, "styleTermFrame")
    assert "touch-action:pan-y" in style_fn
    assert "-webkit-overflow-scrolling:touch" in style_fn
    assert "wireTermFrameScroll(frame)" in HTML


def test_remote_buttons_reflect_actual_backend_state():
    off = remote_button_state({"remoteOn": False, "webtermOn": False})
    assert off["remoteOnDisabled"] is False
    assert off["remoteOffDisabled"] is True
    assert off["webtermOnDisabled"] is False
    assert off["webtermOffDisabled"] is True
    assert off["openTerminalDisabled"] is True

    on = remote_button_state({"remoteOn": True, "webtermOn": True})
    assert on["remoteOnDisabled"] is True
    assert on["remoteOffDisabled"] is False
    assert on["webtermOnDisabled"] is True
    assert on["webtermOffDisabled"] is False
    assert on["openTerminalDisabled"] is False

    busy = remote_button_state({"remoteOn": True, "webtermOn": False}, busy=True)
    assert all(v is True for k, v in busy.items() if k.endswith("Disabled"))


def test_remote_routes_are_never_served_from_stale_shell_cache():
    assert 'const SHELL = "comandos-shell-v2"' in SW
    for endpoint in (
        "/remote-state",
        "/remote-qr.png",
        "/tabs",
        "/tab-history",
    ):
        assert endpoint in SW
    assert "live.some" in SW


def test_dashboard_declares_standard_favicon_to_avoid_remote_404_noise():
    assert '<link rel="icon" href="/icon-192.png">' in HTML


if __name__ == "__main__":
    test_remote_drawer_controls_are_present()
    test_remote_ui_calls_backend_endpoints()
    test_remote_polling_slows_down_when_remote_webterm_is_enabled()
    test_remote_terminal_can_be_opened_from_remote_drawer()
    test_open_terminal_button_dismisses_remote_drawer()
    test_remote_terminal_iframe_wires_wheel_and_touch_to_xterm_scrollback()
    test_remote_buttons_reflect_actual_backend_state()
    test_remote_routes_are_never_served_from_stale_shell_cache()
    test_dashboard_declares_standard_favicon_to_avoid_remote_404_noise()
