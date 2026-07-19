import ast
from pathlib import Path
from types import SimpleNamespace


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


def load_plain_functions(source, *names):
    tree = ast.parse(source)
    wanted = set(names)
    nodes = [
        node for node in tree.body
        if isinstance(node, ast.FunctionDef) and node.name in wanted
    ]
    assert {node.name for node in nodes} == wanted
    module = ast.Module(body=nodes, type_ignores=[])
    namespace = {}
    exec(compile(module, "<extracted>", "exec"), namespace)
    return namespace


def load_plain_methods(source, class_name, *names, extra=None):
    tree = ast.parse(source)
    cls = next(
        node for node in tree.body
        if isinstance(node, ast.ClassDef) and node.name == class_name
    )
    wanted = set(names)
    nodes = [
        node for node in cls.body
        if isinstance(node, ast.FunctionDef) and node.name in wanted
    ]
    assert {node.name for node in nodes} == wanted
    for node in nodes:
        node.decorator_list = []
    module = ast.Module(body=nodes, type_ignores=[])
    namespace = dict(extra or {})
    exec(compile(module, "<extracted-methods>", "exec"), namespace)
    return namespace


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
    assert 'kind in ("scratch", "shell")' in source
    assert 'kind == "ssh"' in source
    assert 'kind == "ssh-tab"' in source
    assert '"/ssh-connect"' in source
    assert '"/ssh-new-tab"' in source
    assert '"/ensure"' in source
    assert "continue" in source


def test_macos_restoration_prefers_persisted_tab_kind_over_name_prefixes():
    ns = load_plain_functions(
        MAC_SOURCE, "ssh_host_from_session", "restore_tab_spec")
    restore = ns["restore_tab_spec"]

    project = restore("ssh-client", {
        "ssh-client": {
            "kind": "project",
            "cwd": "/code/ssh-client",
        },
    })
    scratch_named_project = restore("term-dashboard", {
        "term-dashboard": {
            "kind": "project",
            "cwd": "/code/term-dashboard",
        },
    })
    xterm_named_project = restore("xterm-tools", {
        "xterm-tools": {
            "kind": "project",
            "cwd": "/code/xterm-tools",
        },
    })

    assert project == {
        "session": "ssh-client", "kind": "project",
        "host": "", "cwd": "/code/ssh-client",
    }
    assert scratch_named_project["kind"] == "project"
    assert xterm_named_project["kind"] == "project"


def test_macos_restoration_backfills_prefixed_projects_on_first_upgrade():
    ns = load_plain_functions(
        MAC_SOURCE, "ssh_host_from_session", "restore_tab_spec")
    restore = ns["restore_tab_spec"]

    for session in ("ssh-client", "term-dashboard", "xterm-tools"):
        spec = restore(session, {}, f"/code/{session}")
        assert spec == {
            "session": session,
            "kind": "project",
            "host": "",
            "cwd": f"/code/{session}",
        }


def test_macos_queues_tab_opens_until_authenticated_terminal_boot():
    launch = function_source(MAC_SOURCE, "applicationDidFinishLaunching_")
    create = function_source(MAC_SOURCE, "_create_hub_and_restore")
    add_tab = function_source(MAC_SOURCE, "_add_tab")
    open_tab = function_source(MAC_SOURCE, "openOrFocus_win_label_")
    new_local = function_source(MAC_SOURCE, "newLocalTab_")
    ipc_open = function_source(MAC_SOURCE, "_on_open")

    assert "self.terminals_ready = False" in launch
    assert "self._pending_terminal_actions = []" in launch
    assert "self.terminals_ready = True" in create
    assert "_drain_pending_terminal_actions" in create
    assert "_defer_terminal_action" in open_tab
    assert "_defer_terminal_action" in new_local
    assert "_defer_terminal_action" in ipc_open
    assert "if not self.webterm_token" in add_tab


def test_macos_terminal_action_queue_runs_only_after_ready():
    helpers = load_plain_functions(MAC_SOURCE, "terminal_action_session")
    ns = load_plain_methods(
        MAC_SOURCE, "AppController", "_defer_terminal_action",
        "_drain_pending_terminal_actions",
        extra={"terminal_action_session": helpers["terminal_action_session"]})
    controller = SimpleNamespace(
        terminals_ready=False,
        webterm_token="",
        _pending_terminal_actions=[],
    )
    calls = []

    assert ns["_defer_terminal_action"](
        controller, calls.append, "queued") is True
    assert calls == []
    controller.terminals_ready = True
    controller.webterm_token = "token"
    ns["_drain_pending_terminal_actions"](controller)

    assert calls == ["queued"]
    assert controller._pending_terminal_actions == []
    assert ns["_defer_terminal_action"](
        controller, calls.append, "immediate") is False


def test_macos_remote_close_cancels_queued_open_before_token_ready():
    helpers = load_plain_functions(MAC_SOURCE, "terminal_action_session")
    ns = load_plain_methods(
        MAC_SOURCE, "AppController", "_defer_terminal_action",
        "_cancel_pending_terminal_actions", "_drain_pending_terminal_actions",
        extra={"terminal_action_session": helpers["terminal_action_session"]})
    calls = []
    controller = SimpleNamespace(
        terminals_ready=False,
        webterm_token="",
        _pending_terminal_actions=[],
    )
    ns["_defer_terminal_action"](
        controller, calls.append, {"session": "old-b", "event": "open"})
    ns["_defer_terminal_action"](
        controller, calls.append, {"session": "old-a", "event": "open"})

    assert ns["_cancel_pending_terminal_actions"](controller, "old-b") is True
    controller.terminals_ready = True
    controller.webterm_token = "token"
    ns["_drain_pending_terminal_actions"](controller)

    assert calls == [{"session": "old-a", "event": "open"}]


def test_macos_early_tab_save_preserves_restore_snapshot(tmp_path):
    functions = load_plain_functions(MAC_SOURCE, "merge_tab_labels")
    tabs_file = tmp_path / "app-tabs.json"
    methods = load_plain_methods(
        MAC_SOURCE, "AppController", "_save_tabs",
        extra={
            "json": __import__("json"),
            "os": __import__("os"),
            "TABS_FILE": str(tabs_file),
            "merge_tab_labels": functions["merge_tab_labels"],
        },
    )
    controller = SimpleNamespace(
        _restore_snapshot=[("old-a", "Old A"), ("old-b", "Old B")],
        tabs=[{
            "key": "early", "label": "Early", "is_hub": False,
        }],
    )

    methods["_save_tabs"](controller)

    assert __import__("json").loads(tabs_file.read_text()) == {
        "old-a": "Old A", "old-b": "Old B", "early": "Early",
    }


def test_macos_pending_remote_close_removes_tab_from_restore_snapshot(tmp_path):
    functions = load_plain_functions(
        MAC_SOURCE, "cancel_restore_snapshot", "merge_tab_labels")
    remaining, removed = functions["cancel_restore_snapshot"](
        [("old-a", "Old A"), ("old-b:claude", "Old B")], "old-b")
    assert removed is True

    tabs_file = tmp_path / "app-tabs.json"
    methods = load_plain_methods(
        MAC_SOURCE, "AppController", "_save_tabs",
        extra={
            "json": __import__("json"),
            "os": __import__("os"),
            "TABS_FILE": str(tabs_file),
            "merge_tab_labels": functions["merge_tab_labels"],
        },
    )
    controller = SimpleNamespace(_restore_snapshot=remaining, tabs=[])
    methods["_save_tabs"](controller)

    assert __import__("json").loads(tabs_file.read_text()) == {"old-a": "Old A"}


def test_macos_cancelled_restore_never_materializes_tab():
    restore_one = load_plain_methods(
        MAC_SOURCE, "AppController", "_restore_one")["_restore_one"]
    calls = []
    controller = SimpleNamespace(
        _restore_is_cancelled=lambda _sess: True,
        _tab_for_key=lambda _sess: None,
        _select_win=lambda *_args: calls.append("select"),
        _add_tab=lambda *_args, **_kwargs: calls.append("add"),
        _save_tabs=lambda: calls.append("save"),
    )

    restore_one(controller, "old-b", "Old B", "project", "", "/code/old-b")

    assert calls == []


def test_macos_remote_close_cancels_unmaterialized_restore():
    on_close = load_plain_methods(
        MAC_SOURCE, "AppController", "_on_close",
        extra={"http_post": lambda path, payload, timeout=0: calls.append(
            (path, payload, timeout))},
    )["_on_close"]
    controller = SimpleNamespace(
        _cancel_pending_terminal_actions=lambda _sess: False,
        _restore_current_session=lambda sess: sess,
        _tab_for_key=lambda _sess: None,
        _cancel_pending_restore=lambda sess: calls.append(("cancel", sess)) or True,
        _save_tabs=lambda: calls.append("save"),
    )

    calls = []
    on_close(controller, {"session": "old-b"})

    assert calls == [
        ("cancel", "old-b"),
        "save",
        ("/tab-metadata-remove", {"session": "old-b"}, 3),
    ]


def test_macos_restore_checks_cancellation_before_recreating_session():
    source = function_source(MAC_SOURCE, "_restore_saved")
    assert source.index("self._restore_is_cancelled(restore_sess)") < source.index(
        'tmuxc("has-session"')


def test_macos_visual_close_cancels_pending_restore_before_saving():
    source = function_source(MAC_SOURCE, "_close_tab")
    assert source.index("self._cancel_pending_restore(key)") < source.index(
        "self._save_tabs()")


def test_macos_restore_alias_can_be_cancelled_by_new_session_name():
    helpers = load_plain_functions(MAC_SOURCE, "cancel_restore_snapshot")
    methods = load_plain_methods(
        MAC_SOURCE, "AppController", "_record_restore_alias",
        "_cancel_pending_restore", "_restore_current_session",
        extra={"cancel_restore_snapshot": helpers["cancel_restore_snapshot"]})
    controller = SimpleNamespace(
        _restore_snapshot=[("sshtab-prod-2", "prod")],
        _restore_cancelled=set(),
        _restore_original_by_current={},
        _restore_current_by_original={},
        _restore_lock=__import__("threading").RLock(),
    )

    methods["_record_restore_alias"](
        controller, "sshtab-prod-2", "sshtab-prod-1")

    assert methods["_restore_current_session"](
        controller, "sshtab-prod-2") == "sshtab-prod-1"
    assert methods["_cancel_pending_restore"](
        controller, "sshtab-prod-1") is True
    assert controller._restore_snapshot == []
    assert controller._restore_cancelled == {"sshtab-prod-1", "sshtab-prod-2"}


def test_macos_metadata_is_read_only_and_written_through_dashboard():
    save = function_source(MAC_SOURCE, "_save_tabs")
    add_tab = function_source(MAC_SOURCE, "_add_tab")
    close = function_source(MAC_SOURCE, "_close_tab")

    assert "TABS_META_FILE" not in save
    assert '"/tab-metadata"' in add_tab
    assert '"/tab-metadata-remove"' in close


def test_macos_restoration_uses_exact_persisted_host_for_truncated_legacy_session():
    ns = load_plain_functions(
        MAC_SOURCE, "ssh_host_from_session", "restore_tab_spec")
    restore = ns["restore_tab_spec"]
    host = "build-" + "x" * 54
    legacy_session = ("ssh-" + host)[:60]

    spec = restore(legacy_session, {
        legacy_session: {"kind": "ssh", "host": host},
    })

    assert spec["kind"] == "ssh"
    assert spec["host"] == host


def test_macos_ssh_session_name_parser():
    parse = load_plain_function(MAC_SOURCE, "ssh_host_from_session")
    assert parse("ssh-buildbox") == "buildbox"
    assert parse("sshtab-buildbox-12") == "buildbox"
    assert parse("term-123") is None


def test_dashboard_ssh_creation_uses_portable_scope_wrapper():
    source = function_source(DASH_SOURCE, "ssh_connect")
    assert "scope_cmd" in source
    assert '"systemd-run"' not in source
