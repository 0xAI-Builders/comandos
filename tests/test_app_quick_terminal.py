"""GTK Terminal button: same backend as the web, one requestId until success."""
import ast
from pathlib import Path

SOURCE = Path(__file__).resolve().parents[1].joinpath("bin", "cc-app").read_text()


def load(names, ns):
    tree = ast.parse(SOURCE)
    nodes = [n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name in names]
    assert len(nodes) == len(names), names
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "<app>", "exec"), ns)
    return ns


def test_gtk_quick_terminal_retries_with_the_same_request_id():
    calls, answers = [], [(None, "tmux: no server"),
                          ({"tabId": "term-q1", "paneKey": "pane-q1", "cwd": "/b/T-1", "label": "T-1"}, None)]

    def post(path, payload, timeout=15):
        calls.append((path, dict(payload)))
        return answers.pop(0)

    ids = iter(["gtk-a", "gtk-b"])
    ns = load(["request_quick_terminal"], {"_QUICK_TERMINAL_PENDING": [None], "http_post": post})
    result, err = ns["request_quick_terminal"](make_id=lambda: next(ids))
    assert result is None and "no server" in err
    result, err = ns["request_quick_terminal"](make_id=lambda: next(ids))
    assert err is None and result["tabId"] == "term-q1"
    assert calls == [("/terminal/quick", {"requestId": "gtk-a"})] * 2   # no cwd, same id
    assert ns["_QUICK_TERMINAL_PENDING"] == [None]
    answers.append(({"tabId": "term-q2", "cwd": "/b/T-2"}, None))
    ns["request_quick_terminal"](make_id=lambda: next(ids))
    assert calls[-1] == ("/terminal/quick", {"requestId": "gtk-b"})


def test_gtk_terminal_button_is_separate_from_new_session():
    assert '_quick_term_btn = _icon_btn(\n    "terminal"' in SOURCE
    assert "open_quick_terminal)" in SOURCE
    # Fix 2 (30-sep): the action lives in the web header, which asks GTK through the bridge.
    assert "_headerbar.pack_end(_quick_term_btn)" not in SOURCE
    assert '"quickTerminal": open_quick_terminal' in SOURCE
    plus = SOURCE.split("_plus = _icon_btn(", 1)[1].split(")", 1)[0]
    assert "_open_wizard" in plus                        # "+" keeps the wizard
