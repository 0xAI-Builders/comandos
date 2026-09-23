"""Nada que pueda tardar segundos (HTTP a cc-dash, systemd-run, tmux en
serie) corre en el hilo de GTK: va a un hilo y el resultado vuelve por idle_add."""
import ast
import json
from pathlib import Path
from types import SimpleNamespace

SOURCE = Path(__file__).resolve().parents[1].joinpath("bin", "cc-app").read_text()


def load(names, ns):
    tree = ast.parse(SOURCE)
    nodes = [n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name in names]
    assert len(nodes) == len(names), names
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "<app>", "exec"), ns)
    return ns


class Deferred:
    """threading/GLib falsos: nada corre hasta que el test lo pide."""
    def __init__(self):
        self.threads, self.idle = [], []
        outer = self

        class Thread:
            def __init__(self, target, args=(), daemon=None):
                self.target, self.args = target, args

            def start(self):
                outer.threads.append(self)
        self.threading = SimpleNamespace(Thread=Thread)
        self.GLib = SimpleNamespace(idle_add=lambda fn, *a: self.idle.append((fn, a)),
                                    timeout_add=lambda ms, fn, *a: self.idle.append((fn, a)))

    def run_threads(self):
        while self.threads:
            t = self.threads.pop(0)
            t.target(*t.args)

    def run_idle(self):
        while self.idle:
            fn, a = self.idle.pop(0)
            fn(*a)

    def drain(self):
        while self.threads or self.idle:
            self.run_threads()
            self.run_idle()


def test_export_reply_posts_off_the_ui_thread():
    d = Deferred()
    posts, popups = [], []
    ns = {"threading": d.threading, "GLib": d.GLib, "ES": True,
          "term_session": lambda t: "term-1",
          "http_post": lambda *a, **k: (posts.append(a), ({"path": "/x.pdf"}, None))[1],
          "notify_popup": lambda *a: popups.append(a)}
    load(["export_claude_reply"], ns)["export_claude_reply"](object(), "pdf")
    assert posts == []
    d.drain()
    assert posts and popups[-1][1] == "/x.pdf"


def test_notify_popup_never_blocks_the_caller():
    d = Deferred()
    opened = []
    ns = {"threading": d.threading, "json": json,
          "urllib": SimpleNamespace(request=SimpleNamespace(Request=lambda *a, **k: a,
                                                            urlopen=lambda *a, **k: opened.append(a)))}
    load(["notify_popup"], ns)["notify_popup"]("t", "b")
    assert opened == []
    d.drain()
    assert opened


def test_copy_reply_fetches_state_in_a_thread_and_sets_clipboard_on_idle():
    d = Deferred()
    clip = []
    body = json.dumps([{"session": "term-1", "detail": "hola", "project": "p"}]).encode()

    class Resp:
        def __enter__(self):
            return self

        def __exit__(self, *a):
            return False

        def read(self):
            return body
    fetched = []
    ns = {"threading": d.threading, "GLib": d.GLib, "ES": True, "json": json,
          "term_session": lambda t: "term-1", "notify_popup": lambda *a: None,
          "urllib": SimpleNamespace(request=SimpleNamespace(urlopen=lambda *a, **k: (fetched.append(a), Resp())[1])),
          "Gtk": SimpleNamespace(Clipboard=SimpleNamespace(get=lambda sel: SimpleNamespace(set_text=lambda t, n: clip.append(t)))),
          "Gdk": SimpleNamespace(SELECTION_CLIPBOARD=1)}
    load(["copy_claude_reply"], ns)["copy_claude_reply"](object())
    assert fetched == [] and clip == []
    d.drain()
    assert clip == ["hola"]


def test_restore_tabs_revives_sessions_off_the_ui_thread_and_opens_in_order(tmp_path):
    d = Deferred()
    tabs_file = tmp_path / "app-tabs.json"
    tabs_file.write_text(json.dumps({"term-a": "A", "srv": "B", "xterm-local": None}))
    blocking, opened, ready = [], [], []

    def run(argv, **kw):
        blocking.append(argv)
        return SimpleNamespace(returncode=1 if argv[:2] == ["tmux", "has-session"] else 0)
    ns = {"threading": d.threading, "GLib": d.GLib, "json": json, "os": __import__("os"),
          "TABS_FILE": str(tabs_file), "tabs": {}, "load_tab_snapshot": lambda: {},
          "subprocess": SimpleNamespace(run=run), "ssh_host_from_session": lambda s: None,
          "restore_saved_layout": lambda s: blocking.append(("layout", s)) or True,
          "http_post": lambda *a, **k: (blocking.append(("post",) + a), ({}, None))[1],
          "tmuxc": lambda *a: SimpleNamespace(returncode=0, stdout="1\n"),
          "resume_command": lambda snap: None, "_send_resume": lambda *a: None,
          "open_tab": lambda sess, win, label=None: opened.append((sess, label)),
          "open_xterm_tab": lambda sess: opened.append(("xterm", sess)),
          "print": lambda *a, **k: None}
    load(["restore_tabs", "_restore_one"], ns)["restore_tabs"](on_done=lambda: ready.append(True))
    assert blocking == [] and opened == [] and ready == []
    d.drain()
    assert ("layout", "term-a") in blocking and ("post", "/new", {"session": "srv"}) in blocking
    assert opened == [("term-a", "A"), ("srv", "B"), ("xterm", "local")]
    assert ready == [True]


def test_startup_marks_ready_only_after_restore_finishes():
    fn = next(n for n in ast.parse(SOURCE).body if isinstance(n, ast.FunctionDef) and n.name == "finish_startup_restore")
    text = ast.get_source_segment(SOURCE, fn)
    assert "restore_tabs(on_done=" in text


def test_new_session_dialog_and_xterm_tab_do_not_block_in_gtk():
    for name in ("new_session_dialog", "open_xterm_tab"):
        fn = next(n for n in ast.parse(SOURCE).body if isinstance(n, ast.FunctionDef) and n.name == name)
        text = ast.get_source_segment(SOURCE, fn)
        assert "threading.Thread" in text, name
    dialog = ast.get_source_segment(SOURCE, next(n for n in ast.parse(SOURCE).body
                                                 if isinstance(n, ast.FunctionDef) and n.name == "new_session_dialog"))
    assert "dlg.run()" not in dialog
