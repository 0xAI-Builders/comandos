#!/usr/bin/env python3
import ast
import types
from pathlib import Path

SRC = Path("bin/cc-dash").read_text()


def load_functions(*names, extra=None):
    tree = ast.parse(SRC)
    funcs = {
        node.name: ast.get_source_segment(SRC, node)
        for node in tree.body
        if isinstance(node, ast.FunctionDef)
    }
    missing = [n for n in names if n not in funcs]
    assert not missing, f"missing helper(s): {', '.join(missing)}"
    ns = dict(extra or {})
    exec("\n\n".join(funcs[n] for n in names), ns)
    return ns


def test_paste_calls_load_paste_delete_in_order():
    calls = []

    def fake_tmux(*args, stdin=None, timeout=5):
        calls.append((args, stdin))
        return types.SimpleNamespace(returncode=0, stdout="", stderr="")

    ns = load_functions("snippet_paste_to_pane")
    err = ns["snippet_paste_to_pane"]("=demo:", "git status\n", fake_tmux)
    assert err is None
    verbs = [c[0][0] for c in calls]
    assert verbs == ["load-buffer", "paste-buffer", "delete-buffer"]
    # load-buffer receives text via stdin, not argv
    assert calls[0][1] == "git status\n"
    # paste-buffer uses -p (bracketed) and -b comandos-snip -t <pane>
    args = calls[1][0]
    assert "-p" in args
    assert "-b" in args and "comandos-snip" in args
    assert "-t" in args and "=demo:" in args


def test_paste_returns_error_and_still_deletes_buffer_when_paste_fails():
    calls = []

    def fake_tmux(*args, stdin=None, timeout=5):
        calls.append(args[0])
        rc = 1 if args[0] == "paste-buffer" else 0
        stderr = "boom" if rc else ""
        return types.SimpleNamespace(returncode=rc, stdout="", stderr=stderr)

    ns = load_functions("snippet_paste_to_pane")
    err = ns["snippet_paste_to_pane"]("=demo:", "x", fake_tmux)
    assert err is not None
    assert "boom" in err
    # buffer must be cleaned up even after paste failure
    assert calls == ["load-buffer", "paste-buffer", "delete-buffer"]


def test_paste_never_sends_enter():
    calls = []

    def fake_tmux(*args, stdin=None, timeout=5):
        calls.append(args)
        return types.SimpleNamespace(returncode=0, stdout="", stderr="")

    ns = load_functions("snippet_paste_to_pane")
    ns["snippet_paste_to_pane"]("=demo:", "rm -rf /tmp/x", fake_tmux)
    # No send-keys, no Enter — critical safety property
    assert not any("send-keys" in a for c in calls for a in c)
    assert not any("Enter" in a for c in calls for a in c)
