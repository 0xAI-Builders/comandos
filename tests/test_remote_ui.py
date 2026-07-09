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


def run_node_json(script):
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


def test_remote_terminal_iframe_does_not_hijack_tmux_wheel_events():
    fn = extract_js_function(HTML, "wireTermFrameScroll")
    assert "scrollLines" not in fn
    assert 'addEventListener("wheel"' not in fn
    assert "WheelEvent" in fn
    assert "elementFromPoint" in fn
    assert "clientX" in fn
    assert "clientY" in fn
    assert "touchstart" in fn
    assert "touchmove" in fn
    assert "frame.dataset.selecting" in fn
    assert "passive:false" in fn
    style_fn = extract_js_function(HTML, "styleTermFrame")
    assert "touch-action:pan-y" in style_fn
    assert "-webkit-overflow-scrolling:touch" in style_fn
    assert "wireTermFrameScroll(frame)" in HTML


def test_remote_touch_scroll_is_throttled_for_tmux_wheel_ticks():
    fn = extract_js_function(HTML, "wireTermFrameScroll")
    script = f"""
{fn}
const listeners = {{}};
let wheels = [];
const target = {{
  dispatchEvent(ev){{
    if(ev.type === "wheel") wheels.push({{deltaY: ev.deltaY, clientY: ev.clientY}});
    return true;
  }}
}};
class FakeWheelEvent {{
  constructor(type, init){{ this.type = type; Object.assign(this, init); }}
}}
const doc = {{
  body: target,
  addEventListener(type, cb){{ listeners[type] = cb; }},
  elementFromPoint(){{ return target; }},
  querySelector(){{ return target; }}
}};
const frame = {{ dataset: {{}}, contentWindow: {{ WheelEvent: FakeWheelEvent }}, contentDocument: doc }};
wireTermFrameScroll(frame);
const ev = y => ({{
  touches: [{{clientX: 10, clientY: y, screenX: 10, screenY: y}}],
  target,
  preventDefault(){{ this.prevented = true; }},
  stopPropagation(){{ this.stopped = true; }}
}});
listeners.touchstart(ev(100));
listeners.touchmove(ev(80));
const afterSmallMove = wheels.length;
listeners.touchmove(ev(60));
const afterSecondSmallMove = wheels.length;
listeners.touchmove(ev(40));
const afterThreshold = wheels.length;
listeners.touchmove(ev(-90));
console.log(JSON.stringify({{
  afterSmallMove,
  afterSecondSmallMove,
  afterThreshold,
  finalCount: wheels.length,
  deltas: wheels.map(w => w.deltaY)
}}));
"""
    result = run_node_json(script)

    assert result["afterSmallMove"] == 0
    assert result["afterSecondSmallMove"] == 0
    assert result["afterThreshold"] == 1
    assert result["finalCount"] <= 4


def test_remote_terminal_has_explicit_text_selection_mode():
    assert 'id="term-select-toggle"' in HTML
    assert "/tmux-mouse" in HTML
    assert "setTermSelectionMode" in HTML
    assert "restoreTermInteraction" in HTML
    fn = extract_js_function(HTML, "setTermSelectionMode")
    assert 'api("/tmux-mouse"' in fn
    assert "enabled: !selecting" in fn
    assert "selectingTerms.add(sess)" in fn
    assert "selectingTerms.delete(sess)" in fn
    assert 'frame.dataset.selecting = selecting ? "1" : ""' in fn
    assert "updateTermSelectButton()" in fn
    assert "updateTermSelectButton();" in extract_js_function(HTML, "showView")
    assert "restoreTermInteraction(sess)" in extract_js_function(HTML, "closeTerm")


def test_existing_ssh_manager_can_setup_passwordless_key_access():
    assert "/ssh-key-setup" in HTML
    assert "setupSshKey" in HTML
    assert 'button class="key"' in HTML
    assert "row.querySelector(\".key\")" in HTML
    fn = extract_js_function(HTML, "setupSshKey")
    assert 'api("/ssh-key-setup"' in fn
    assert "openInApp(r.session" in fn
    assert "openTerm(r.session" in fn


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
    test_remote_terminal_iframe_does_not_hijack_tmux_wheel_events()
    test_remote_touch_scroll_is_throttled_for_tmux_wheel_ticks()
    test_remote_terminal_has_explicit_text_selection_mode()
    test_existing_ssh_manager_can_setup_passwordless_key_access()
    test_remote_buttons_reflect_actual_backend_state()
    test_remote_routes_are_never_served_from_stale_shell_cache()
    test_dashboard_declares_standard_favicon_to_avoid_remote_404_noise()
