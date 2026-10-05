import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "lib"))
import analytics_week as aw


def test_sidebar_measures_opencode_without_changing_analytics_accounts():
    now = 1_800_000_000
    accounts = [{"id": "claude:main", "provider": "claude", "alias": "main", "week": 70}]
    limits = [{"provider": "claude", "account": "main", "plan": "Max"}]
    def turn(session, tokens, cost, age=10, account="unknown"):
        return {"provider": "opencode", "account": account, "session": session,
                "finished": now - age, "tokens": tokens, "cost": cost,
                "model": "opencode/longcat-2.5-preview-free"}
    turns = [turn("one", 100, 0), turn("one", 200, .5), turn("two", 300, .25),
             turn("old", 999, 1, age=7 * 86400 + 1), turn("future", 999, 1, age=-1),
             turn("work", 400, 2, account="work")]
    result = aw.sidebar_accounts(accounts, limits, turns, now)
    assert accounts == [{"id": "claude:main", "provider": "claude", "alias": "main", "week": 70}]
    assert result[0]["plan"] == "Max"
    by = {a["id"]: a for a in result}
    assert by["opencode:main"]["week"] is None
    assert by["opencode:main"]["measured"] == {
        "sessions": 2, "tokens": 600, "costUsd": .75,
        "models": ["opencode/longcat-2.5-preview-free"],
    }
    assert by["opencode:work"]["measured"]["tokens"] == 400
    assert by["opencode:work"]["measured"]["costUsd"] == 2


def test_sidebar_includes_opencode_with_an_external_motor():
    result = aw.sidebar_accounts([], [], [{"provider": "groq", "agent": "opencode",
        "account": "main", "session": "s", "finished": 99, "tokens": 50,
        "cost": 0, "model": "groq/model"}], 100)
    assert result[0]["id"] == "opencode:main"
    assert result[0]["measured"]["tokens"] == 50


def test_sidebar_uses_provider_plan_type():
    accounts = [{"id": "codex:main", "provider": "codex", "alias": "main", "week": 10}]
    result = aw.sidebar_accounts(accounts, [{"provider": "codex", "account": "main",
                                          "plan_type": "Pro"}], [], 100)
    assert result[0]["plan"] == "Pro"
