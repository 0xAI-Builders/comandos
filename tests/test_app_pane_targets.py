"""Comandos tmux dirigidos a un pane nunca van con un target vacío:
`tmux kill-pane -t ""` mata el pane "actual" de OTRA sesión."""
import ast
from pathlib import Path
from types import SimpleNamespace

SOURCE = Path(__file__).resolve().parents[1].joinpath("bin", "cc-app").read_text()


def load(names, ns):
    tree = ast.parse(SOURCE)
    nodes = [n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name in names]
    assert len(nodes) == len(names)
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "<app>", "exec"), ns)
    return ns


def ns_for(key, sessions, term_sess=None):
    calls = []

    def tmuxc(*args):
        calls.append(args)
        if args[0] == "display-message":
            target = args[args.index("-t") + 1]
            name = target.strip("=:")
            if name in sessions:
                return SimpleNamespace(returncode=0, stdout=sessions[name] + "\n")
            return SimpleNamespace(returncode=1, stdout="")
        return SimpleNamespace(returncode=0, stdout="")
    page = SimpleNamespace(_key=key, _term=object() if term_sess else None)
    import re
    ns = {"tmuxc": tmuxc, "_cur_page": lambda: page, "re": re,
          "term_session": lambda term: term_sess}
    load(["_cur_pane_id", "_kill_cur_pane", "_select_pane_cmd"], ns)
    return ns, calls


def test_xterm_tab_never_kills_a_pane():
    ns, calls = ns_for("xterm-local", {"local": "%3"})
    assert ns["_cur_pane_id"]() == ""
    assert ns["_kill_cur_pane"]() is None
    assert not [c for c in calls if c[0] == "kill-pane"]


def test_missing_session_never_kills_a_pane():
    ns, calls = ns_for("term-gone", {})
    ns["_kill_cur_pane"]()
    assert not [c for c in calls if c[0] == "kill-pane"]


def test_kill_targets_the_resolved_pane_id():
    ns, calls = ns_for("term-1", {"term-1": "%7"})
    ns["_kill_cur_pane"]()
    assert ("kill-pane", "-t", "%7") in calls


def test_terminal_session_wins_over_tab_key():
    ns, calls = ns_for("term-1", {"term-1": "%7", "other": "%9"}, term_sess="other")
    assert ns["_cur_pane_id"]() == "%9"


def test_select_pane_rejects_empty_or_non_pane_targets():
    ns, calls = ns_for("term-1", {})
    ns["_select_pane_cmd"]({"pane": ""})
    ns["_select_pane_cmd"]({"pane": "=other:"})
    assert not calls
    ns["_select_pane_cmd"]({"pane": "%4"})
    assert calls == [("select-pane", "-t", "%4")]


def test_app_commands_use_guarded_helpers():
    assert "\"kill_pane\": lambda a: _kill_cur_pane(a.get(\"session\"))" in SOURCE
    assert '"select_pane": lambda a: _select_pane_cmd(a)' in SOURCE


def test_kill_pane_command_honours_explicit_session():
    ns, calls = ns_for("term-1", {"term-1": "%7", "term-2": "%8"})
    ns["_kill_cur_pane"]("term-2")
    assert ("kill-pane", "-t", "%8") in calls
