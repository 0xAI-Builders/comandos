"""Nuevas pestanas locales: nombre sin colision, exito verificado y
sesion citada en el sh -c del attach."""
import ast
import re
import shlex
from pathlib import Path
from types import SimpleNamespace

SOURCE = Path(__file__).resolve().parents[1].joinpath("bin", "cc-app").read_text()


def load(names, ns):
    tree = ast.parse(SOURCE)
    nodes = [n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name in names]
    assert len(nodes) == len(names), names
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "<app>", "exec"), ns)
    return ns


def fake_tmux(existing, fail=False):
    created = []

    def run(argv, **kw):
        tmux = argv[argv.index("tmux"):]
        name = tmux[tmux.index("-s") + 1] if "-s" in tmux else tmux[tmux.index("-t") + 1].lstrip("=")
        if tmux[1] == "has-session":
            return SimpleNamespace(returncode=0 if name in existing else 1, stdout="", stderr="")
        if tmux[1] == "new-session":
            if fail or name in existing:
                return SimpleNamespace(returncode=1, stdout="", stderr="duplicate session")
            existing.add(name)
            created.append(name)
            return SimpleNamespace(returncode=0, stdout=name + "\n", stderr="")
        raise AssertionError(argv)
    return run, created


def ns_for(existing, fail=False):
    import threading
    run, created = fake_tmux(existing, fail)
    ns = {"subprocess": SimpleNamespace(run=run), "os": SimpleNamespace(getpid=lambda: 41234),
          "TERM_N": [0], "_TERM_N_LOCK": threading.Lock()}
    load(["_create_term_session"], ns)
    return ns, created


def test_new_local_tab_skips_names_of_live_sessions():
    ns, created = ns_for({"term-1234-1", "term-1234-2"})
    assert ns["_create_term_session"]("/tmp") == "term-1234-3"
    assert created == ["term-1234-3"]


def test_new_local_tab_reports_failure_instead_of_attaching():
    ns, created = ns_for(set(), fail=True)
    assert ns["_create_term_session"]("/tmp") is None


def test_open_tab_rejects_bad_session_and_quotes_attach():
    src = ast.get_source_segment(SOURCE, next(n for n in ast.parse(SOURCE).body
                                              if isinstance(n, ast.FunctionDef) and n.name == "open_tab"))
    assert "SESSION_RE.match(sess" in src
    assert 'shlex.quote("=" + sess)' in src
    assert "'={sess}'" not in src
    m = re.search(r'SESSION_RE = re\.compile\((r"[^"]+")\)', SOURCE)
    rx = re.compile(eval(m.group(1)))
    assert rx.match("term-1234-3") and rx.match("sshtab-prod-east-2")
    assert not rx.match("x'; rm -rf ~; '") and not rx.match("")
