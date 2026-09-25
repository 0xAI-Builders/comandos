import sys
from pathlib import Path
from types import SimpleNamespace
import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "lib"))
from terminal_history import capture


def fake_tmux(calls):
    def run(*args):
        calls.append(args)
        text = "%1\t1\t0\t0\t40\t24\tcodex\n%2\t0\t41\t0\t39\t24\tgrok\n" if args[0] == "list-panes" else "á🙂 same same\n"
        return SimpleNamespace(returncode=0, stdout=text)
    return run


def test_pointer_selects_correct_split_without_mutating_focus():
    calls = []
    result = capture(fake_tmux(calls), {"session": "term-1", "col": 50, "row": 2})
    assert result["pane"] == "%2"
    assert result["text"] == "á🙂 same same\n"
    assert [c[0] for c in calls] == ["list-panes", "capture-pane"]


def test_foreign_pane_cannot_be_captured():
    calls = []
    with pytest.raises(ValueError, match="pertenece"):
        capture(fake_tmux(calls), {"session": "term-1", "pane": "%999"})
    assert len(calls) == 1


@pytest.mark.parametrize("data", [{"session": "-a" ,"lines": 99999}, {"session": "x", "col": -1, "row": 0}, {"session": "x", "lines": True}, {"session": "x;kill"}])
def test_invalid_history_requests_are_rejected(data):
    with pytest.raises(ValueError):
        capture(fake_tmux([]), data)


def test_panes_carry_a_friendly_folder_path(monkeypatch):
    monkeypatch.setenv("HOME", "/home/dev")
    def run(*args):
        if args[0] == "list-panes":
            return SimpleNamespace(returncode=0, stdout="%1\t1\t0\t0\t40\t24\tnode\t/home/dev/codebase/App\n"
                                                        "%2\t0\t41\t0\t39\t24\tzsh\t/srv/data\n")
        return SimpleNamespace(returncode=0, stdout="x\n")
    result = capture(run, {"session": "term-1"})
    assert [(p["id"], p["title"], p["path"]) for p in result["panes"]] == [
        ("%1", "node", "~/codebase/App"), ("%2", "zsh", "/srv/data")]
