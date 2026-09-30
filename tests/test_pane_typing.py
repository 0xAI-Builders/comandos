import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "lib"))
import pytest
import pane_typing

class FakeTmux:
    def __init__(self, fail_at=None):
        self.calls, self.fail_at = [], fail_at
    def __call__(self, *args, **kw):
        self.calls.append(args)
        class R: returncode = 0; stderr = ""
        if self.fail_at is not None and len(self.calls) == self.fail_at:
            R.returncode, R.stderr = 1, "no such pane"
        return R()

def test_types_every_character_literally_and_never_sends_enter():
    t, slept = FakeTmux(), []
    out = pane_typing.type_literal(t, "%3", "/model claude-fable-5-1", sleep=slept.append, delay=0.02, budget=10)
    typed = "".join(c[-1] for c in t.calls)
    assert typed == "/model claude-fable-5-1"
    assert all(c[:5] == ("send-keys", "-t", "%3", "-l", "--") for c in t.calls)
    assert not any("Enter" in c for c in t.calls)
    assert out == {"typed": 23}
    assert len(slept) == 22 and slept[0] == pytest.approx(0.02)

def test_long_text_speeds_up_to_fit_the_budget():
    t, slept = FakeTmux(), []
    pane_typing.type_literal(t, "%3", "x" * 200, sleep=slept.append, delay=0.022, budget=1.2)
    assert sum(slept) <= 1.2 + 1e-6 and len(t.calls) == 200

@pytest.mark.parametrize("bad", ["", "  ", "ls\n", "a\rb", "\x1b[A", "a\tb", "x" * 2001])
def test_rejects_newlines_control_chars_empty_and_too_long(bad):
    with pytest.raises(pane_typing.TypingError):
        pane_typing.type_literal(FakeTmux(), "%3", bad, sleep=lambda s: None)

def test_stops_at_first_tmux_failure_and_reports_progress():
    t = FakeTmux(fail_at=3)
    with pytest.raises(pane_typing.TypingError) as exc:
        pane_typing.type_literal(t, "%3", "abcdef", sleep=lambda s: None)
    assert exc.value.typed == 2 and "no such pane" in str(exc.value)

def test_lock_refuses_a_second_typing_on_the_same_pane_only():
    locks = pane_typing.PaneTypingLocks()
    assert locks.acquire("%3") and not locks.acquire("%3") and locks.acquire("%4")
    locks.release("%3")
    assert locks.acquire("%3")


def test_semicolon_is_escaped_so_tmux_does_not_read_it_as_a_command_separator():
    t = FakeTmux()
    out = pane_typing.type_literal(t, "%3", "a;b", sleep=lambda s: None)
    assert [c[-1] for c in t.calls] == ["a", "\\;", "b"]   # backslash + semicolon
    assert out == {"typed": 3}
