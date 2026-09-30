"""A session with several panes on different models must not alert every poll."""
import ast
from pathlib import Path

SOURCE = Path("bin/cc-dash").read_text()


def load():
    sent = []
    ns = {"load_model_tiers": lambda: {"alertTier": "high"}, "read_conf": lambda: {},
          "tier_style": lambda t: {"symbol": "$$$", "label": "caro"} if t == "high" else {"symbol": "$", "label": "barato"},
          "ui_lang": lambda: "es",
          "threading": type("T", (), {"Thread": staticmethod(lambda target, args=(), kwargs=None, daemon=True:
                                                              type("R", (), {"start": lambda self: target(*args, **(kwargs or {}))})())}),
          "usage_alert_send": lambda msg, **kw: sent.append((msg, kw)), "time": __import__("time")}
    nodes = [n for n in ast.parse(SOURCE).body if isinstance(n, ast.FunctionDef) and n.name == "_maybe_tier_alert"
             or isinstance(n, ast.Assign) and any(getattr(t, "id", "") in ("_TIER_LAST", "_TIER_ALERTED", "TIER_ALERT_COOLDOWN_S") for t in n.targets)]
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "<dash>", "exec"), ns)
    return ns, sent


def test_two_panes_of_one_session_on_different_tiers_alert_once():
    ns, sent = load()
    alert = ns["_maybe_tier_alert"]
    for _ in range(5):                      # polls alternate between the panes
        alert("sava", "claude", "haiku", "low", pane="%11")
        alert("sava", "claude", "opus-5-5", "high", pane="%12")
    assert sent == [], "a pane that never changed tier is not an alert"
    alert("sava", "claude", "opus-5-5", "high", pane="%11")   # %11 really switched to the expensive tier
    assert len(sent) == 1 and "%11" not in sent[0][0] and sent[0][1]["project"] == "sava"
    assert "opus-5-5" in sent[0][0] and sent[0][1]["title"] == "Modelo caro en uso"
    for _ in range(5):
        alert("sava", "claude", "opus-5-5", "high", pane="%11")
    assert len(sent) == 1, "staying on the tier never re-alerts"


def test_the_same_change_within_an_hour_is_not_repeated():
    ns, sent = load()
    alert = ns["_maybe_tier_alert"]
    alert("s", "claude", "sonnet", "mid", pane="%1")
    alert("s", "claude", "opus-5-5", "high", pane="%1")
    alert("s", "claude", "sonnet", "mid", pane="%1")
    alert("s", "claude", "opus-5-5", "high", pane="%1")
    assert len(sent) == 1, "flapping back and forth within the cooldown is one alert"
