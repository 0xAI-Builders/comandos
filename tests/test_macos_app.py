import ast
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
MAC_SOURCE = (ROOT / "bin" / "cc-app-mac").read_text()
ATTACH_SOURCE = (ROOT / "bin" / "cc-webterm-attach").read_text()
DASH_SOURCE = (ROOT / "bin" / "cc-dash").read_text()


def function_source(source, name):
    tree = ast.parse(source)
    node = next(
        item for item in ast.walk(tree)
        if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef))
        and item.name == name
    )
    return ast.get_source_segment(source, node)


def load_plain_function(source, name):
    tree = ast.parse(source)
    node = next(
        item for item in tree.body
        if isinstance(item, ast.FunctionDef) and item.name == name
    )
    module = ast.Module(body=[node], type_ignores=[])
    namespace = {}
    exec(compile(module, "<extracted>", "exec"), namespace)
    return namespace[name]


def test_macos_attach_script_works_with_bash_32():
    assert "mapfile" not in ATTACH_SOURCE


def test_macos_tab_strip_scrolls_when_many_tabs_are_open():
    assert "NSScrollView" in MAC_SOURCE
    assert "setHasHorizontalScroller_(True)" in MAC_SOURCE
    assert "setDocumentView_(self.tab_strip)" in MAC_SOURCE
    assert "scrollRectToVisible_" in MAC_SOURCE


def test_macos_external_links_open_through_nsworkspace():
    assert MAC_SOURCE.count("setUIDelegate_(self)") >= 2
    assert "webView_createWebViewWithConfiguration_forNavigationAction_windowFeatures_" in MAC_SOURCE
    assert "NSWorkspace.sharedWorkspace().openURL_" in MAC_SOURCE


def test_macos_hub_attaches_tmux_local_for_ipc_focus_detection():
    source = function_source(MAC_SOURCE, "_create_hub_and_restore")
    assert 'tmuxc("new-session", "-d", "-s", "local"' in source
    assert 'self._add_tab(HUB_KEY, "local", "local", is_hub=True)' in source


def test_macos_restoration_handles_scratch_and_ssh_sessions():
    source = function_source(MAC_SOURCE, "_restore_saved")
    assert 'sess.startswith("term-")' in source
    assert '"/ssh-connect"' in source
    assert '"/ssh-new-tab"' in source
    assert '"/new"' in source
    assert "continue" in source


def test_macos_ssh_session_name_parser():
    parse = load_plain_function(MAC_SOURCE, "ssh_host_from_session")
    assert parse("ssh-buildbox") == "buildbox"
    assert parse("sshtab-buildbox-12") == "buildbox"
    assert parse("term-123") is None


def test_dashboard_ssh_creation_uses_portable_scope_wrapper():
    source = function_source(DASH_SOURCE, "ssh_connect")
    assert "scope_cmd" in source
    assert '"systemd-run"' not in source
