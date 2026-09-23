"""Ctrl+C en Grok/Codex: el CLI nunca muere por SIGKILL y solo se limpian sus
hijos despegados cuando el propio CLI ya salió."""
import ast
import os
import signal
import sys
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "lib"))

import agent_stop

SOURCE = Path(__file__).resolve().parents[1].joinpath("bin", "cc-app").read_text()


def proc(pid, ppid, pgid, comm, start=1, tpgid=-1, sid=None):
    return {"pid": pid, "ppid": ppid, "pgid": pgid, "comm": comm, "start": start,
            "tpgid": tpgid, "sid": sid if sid is not None else pgid}


def no_cmdline(_pid):
    return []


# ---- decisión del teclado ----

def test_single_ctrl_c_only_passes_through():
    assert agent_stop.ctrl_c_action(False, False, 10.0, None) == "pass"
    assert agent_stop.ctrl_c_action(False, False, 10.0, 8.0) == "pass"


def test_second_ctrl_c_within_window_arms_cleanup():
    assert agent_stop.ctrl_c_action(False, False, 10.5, 10.0) == "arm"


def test_selection_copies_and_never_arms():
    assert agent_stop.ctrl_c_action(True, False, 10.5, 10.0) == "copy"


def test_copy_mode_context_never_arms():
    assert agent_stop.ctrl_c_action(False, True, 10.5, 10.0) == "pass"


def test_client_state_blocks_copy_mode_prefix_and_other_key_tables():
    out = "/dev/pts/9|root|0|0|77\n/dev/pts/3|root|0|1|55\n"
    assert agent_stop.parse_client_state(out, "/dev/pts/9")["pane_pid"] == 77
    assert not agent_stop.client_blocks_stop(agent_stop.parse_client_state(out, "/dev/pts/9"))
    assert agent_stop.client_blocks_stop(agent_stop.parse_client_state(out, "/dev/pts/3"))
    assert agent_stop.client_blocks_stop(agent_stop.parse_client_state("/dev/pts/9|prefix|1|0|77", "/dev/pts/9"))
    assert agent_stop.client_blocks_stop(agent_stop.parse_client_state("/dev/pts/9|copy-mode|0|0|77", "/dev/pts/9"))
    assert agent_stop.client_blocks_stop(None)
    assert agent_stop.parse_client_state(out, "/dev/pts/1") is None


# ---- a quién apunta ----

def test_foreground_grok_is_the_target():
    processes = [proc(10, 1, 10, "zsh", tpgid=20), proc(20, 10, 20, "grok"), proc(21, 20, 21, "node")]
    assert agent_stop.foreground_agent(10, processes, no_cmdline)["pid"] == 20


def test_grok_launched_by_claude_is_not_targeted():
    # claude -> pytest -> grok: el primer plano es claude, no grok.
    processes = [
        proc(10, 1, 10, "zsh", tpgid=30),
        proc(30, 10, 30, "claude"),
        proc(31, 30, 31, "pytest"),
        proc(32, 31, 31, "grok"),
    ]
    assert agent_stop.foreground_agent(10, processes, no_cmdline) is None
    assert agent_stop.cleanup_after_exit(10, processes, cmdline=no_cmdline,
                                         kill=lambda *a: (_ for _ in ()).throw(AssertionError(a))) == []


def test_background_codex_is_not_targeted():
    processes = [proc(10, 1, 10, "bash", tpgid=10), proc(40, 10, 40, "codex")]
    assert agent_stop.foreground_agent(10, processes, no_cmdline) is None


def test_agent_names_include_platform_binaries_and_npm_wrapper():
    assert agent_stop.is_agent_name("grok-linux-x86_64")
    assert agent_stop.is_agent_name("/usr/lib/codex-x86_64-unknown")
    assert not agent_stop.is_agent_name("grokker")
    processes = [proc(10, 1, 10, "zsh", tpgid=50), proc(50, 10, 50, "node")]
    argv = {50: ["node", "/usr/local/bin/codex"]}
    assert agent_stop.foreground_agent(10, processes, lambda pid: argv.get(pid, []))["pid"] == 50


def test_only_detached_descendants_are_cleanup_targets():
    agent = proc(20, 10, 20, "grok")
    processes = [
        proc(10, 1, 10, "zsh"), agent,
        proc(21, 20, 20, "rg"),      # mismo grupo: ya le llega el ^C
        proc(22, 20, 22, "node"),    # MCP con sesión propia
        proc(23, 22, 22, "sh"),
    ]
    assert [p["pid"] for p in agent_stop.detached_descendants(agent, processes)] == [22, 23]


# ---- limpieza ----

def _fake(processes_alive):
    return lambda recs: [r for r in recs if r["pid"] in processes_alive]


def test_cli_still_running_means_nothing_is_signalled():
    processes = [proc(10, 1, 10, "zsh", tpgid=20), proc(20, 10, 20, "grok"), proc(22, 20, 22, "node")]
    sent = []
    clock = iter(range(100))
    result = agent_stop.cleanup_after_exit(
        10, processes, alive=_fake({20, 22}), kill=lambda pid, sig: sent.append((pid, sig)),
        sleep=lambda s: None, clock=lambda: next(clock), wait_s=3, cmdline=no_cmdline)
    assert result == [] and sent == []


def test_after_cli_exit_detached_children_get_sigint_then_sigterm_never_sigkill():
    processes = [proc(10, 1, 10, "zsh", tpgid=20), proc(20, 10, 20, "grok"),
                 proc(21, 20, 20, "rg"), proc(22, 20, 22, "node"), proc(23, 20, 23, "mcp")]
    alive = {21, 22, 23}          # el CLI (20) ya salió
    sent = []

    def kill(pid, sig):
        sent.append((pid, sig))
        if pid == 23 and sig == signal.SIGINT:
            alive.discard(23)       # este sí respeta SIGINT
    agent_stop.cleanup_after_exit(10, processes, alive=_fake(alive), kill=kill,
                                  sleep=lambda s: None, clock=lambda: 0, cmdline=no_cmdline)
    assert sent == [(22, signal.SIGINT), (23, signal.SIGINT), (22, signal.SIGTERM)]
    assert all(pid != 20 and sig != signal.SIGKILL for pid, sig in sent)


def test_still_same_skips_unparseable_and_reused_records():
    me = next(p for p in agent_stop.read_processes() if p["pid"] == os.getpid())
    reused = dict(me, start=me["start"] + 1)
    gone = {"pid": 2 ** 22 + 7, "start": 1}
    assert agent_stop.still_same([reused, gone, me]) == [me]
    assert agent_stop._parse_stat(b"1 (x) S 1 2 3 4 5 a b c d e f g h i j k l m n") is None


def test_live_process_table_sees_this_python():
    processes = agent_stop.read_processes()
    me = next(item for item in processes if item["pid"] == os.getpid())
    assert me["ppid"] == os.getppid()
    assert me["comm"]
    assert me["start"] > 0
    assert "tpgid" in me and "sid" in me


# ---- cableado en la app ----

def load_app(name, ns):
    tree = ast.parse(SOURCE)
    nodes = [n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == name]
    assert nodes, name
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "<app>", "exec"), ns)
    return ns[name]


class Term:
    def __init__(self, selection=False):
        self.selection = selection
        self.copied = False
        self._tty = "/dev/pts/9"

    def get_has_selection(self):
        return self.selection

    def copy_clipboard_format(self, _fmt):
        self.copied = True

    def unselect_all(self):
        self.selection = False


def app_ns(client_row="/dev/pts/9|root|0|0|77"):
    calls = {"tmux": 0, "threads": []}

    def tmuxc(*args):
        calls["tmux"] += 1
        return SimpleNamespace(returncode=0, stdout=client_row)

    class Thread:
        def __init__(self, target, args=(), daemon=None):
            calls["threads"].append((target, args))

        def start(self):
            pass
    ns = {"agent_stop": agent_stop, "tmuxc": tmuxc, "Vte": SimpleNamespace(Format=SimpleNamespace(TEXT=1)),
          "threading": SimpleNamespace(Thread=Thread), "print": lambda *a, **k: None}
    load_app("_agent_cleanup_worker", ns)
    load_app("_arm_agent_cleanup", ns)
    return load_app("handle_ctrl_c", ns), calls


def test_app_single_ctrl_c_passes_without_tmux_or_proc_work():
    handle, calls = app_ns()
    term = Term()
    assert handle(term, False, 10.0) is False
    assert calls == {"tmux": 0, "threads": []}


def test_app_selection_copies_and_skips_stop_logic():
    handle, calls = app_ns()
    term = Term(selection=True)
    term._last_ctrl_c = 9.8
    assert handle(term, False, 10.0) is True
    assert term.copied and calls == {"tmux": 0, "threads": []}


def test_app_copy_mode_context_skips_stop_logic():
    handle, calls = app_ns()
    term = Term()
    term._last_ctrl_c = 9.8
    assert handle(term, True, 10.0) is False
    assert calls == {"tmux": 0, "threads": []}


def test_app_double_ctrl_c_in_tmux_copy_mode_does_not_arm():
    handle, calls = app_ns("/dev/pts/9|root|0|1|77")
    term = Term()
    handle(term, False, 10.0)
    assert handle(term, False, 10.4) is False
    assert calls["tmux"] == 1 and calls["threads"] == []


def test_app_double_ctrl_c_arms_worker_with_pane_pid():
    handle, calls = app_ns()
    term = Term()
    handle(term, False, 10.0)
    assert handle(term, False, 10.4) is False     # el ^C sigue llegando al CLI
    assert [args for _target, args in calls["threads"]] == [(77,)]
    handle(term, False, 10.6)                     # el tercero no re-arma
    assert len(calls["threads"]) == 1


def test_app_stop_path_never_sigkills():
    for name in ("handle_ctrl_c", "_arm_agent_cleanup", "_agent_cleanup_worker"):
        tree = next(n for n in ast.parse(SOURCE).body if isinstance(n, ast.FunctionDef) and n.name == name)
        text = ast.get_source_segment(SOURCE, tree)
        assert "SIGKILL" not in text and "kill_survivors" not in text
    assert "SIGKILL" not in Path(agent_stop.__file__).read_text().split('"""', 2)[2]
