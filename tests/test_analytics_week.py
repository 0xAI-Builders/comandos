import json
import subprocess
import sys
from datetime import datetime
from pathlib import Path
from zoneinfo import ZoneInfo

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import analytics_week as aw  # noqa: E402

TZ = ZoneInfo("America/Mexico_City")


def ts(text):
    return datetime.fromisoformat(text).replace(tzinfo=TZ).timestamp()


NOW = ts("2026-10-01T08:16:00")
LIMITS = [
    {"provider": "claude", "account": "main", "window": "7d", "scope": "", "percent": 36.0, "resets_at": ts("2026-10-03T08:00:00")},
    {"provider": "claude", "account": "main", "window": "7d", "scope": "Fable", "percent": 31.0, "resets_at": ts("2026-10-03T08:00:00")},
    {"provider": "claude", "account": "main", "window": "5h", "scope": "", "percent": 6.0, "resets_at": ts("2026-10-01T10:40:00")},
    {"provider": "claude", "account": "relotto", "window": "7d", "scope": "", "percent": 59.0, "resets_at": ts("2026-10-02T23:00:00")},
    {"provider": "codex", "account": "main", "window": "7d", "scope": "", "percent": 53.0, "resets_at": ts("2026-10-03T21:58:00")},
    {"provider": "grok", "account": "main", "window": "7d", "scope": "", "percent": None, "resets_at": 0},
]


def turn(provider, account, root, at, tokens=1_000_000, started=None):
    return {"provider": provider, "account": account, "git_root": root, "pane_pwd": root,
            "started": started if started is not None else ts(at), "finished": ts(at), "tokens": tokens}


def test_days_and_label_follow_the_mockup():
    week = aw.build_week(now=NOW, offset=0, limits=[], turns=[], spans=[], snapshots=[], records=[])
    assert week["days"][0] == ["2026-10-01", "jue", "1 oct"]
    assert week["days"][1] == ["2026-09-30", "mié", "30"]
    assert week["days"][-1] == ["2026-09-24", "jue", "24"]
    assert week["week"]["label"] == "24 sep – 1 oct"
    assert week["week"]["today"] == "2026-10-01" and round(week["week"]["now"], 2) == 8.27
    prev = aw.build_week(now=NOW, offset=-1, limits=[], turns=[], spans=[], snapshots=[], records=[])
    assert prev["days"][0] == ["2026-09-23", "mié", "23 sep"] and prev["week"]["label"] == "16 – 23 sep"
    assert prev["week"]["today"] is None and prev["week"]["now"] is None


def test_accounts_have_one_bottle_per_limit_and_never_merge():
    week = aw.build_week(now=NOW, offset=0, limits=LIMITS, turns=[], spans=[], snapshots=[], records=[])
    by = {a["id"]: a for a in week["accounts"]}
    assert list(by) == ["claude:main", "claude:relotto", "codex:main"]
    main = by["claude:main"]
    assert (main["week"], main["h5"], main["model"]["n"], main["model"]["v"]) == (36, 6, "Fable", 31)
    assert main["reset"] == "sáb 3 oct, 08:00" and main["left"] == "1d 23h"
    assert main["h5Left"] == "2h 24m" and main["color"] == "#8B7CFF"
    assert by["claude:relotto"]["week"] == 59 and by["claude:relotto"]["h5"] is None
    assert by["codex:main"]["model"] is None


def test_sessions_merge_close_work_and_split_at_midnight():
    turns = [
        turn("claude", "main", "/x/ComandOS", "2026-09-30T23:40:00"),
        turn("claude", "main", "/x/ComandOS", "2026-10-01T00:20:00", tokens=3_000_000, started=ts("2026-09-30T23:50:00")),
        turn("claude", "relotto", "/x/ComandOS", "2026-10-01T00:10:00"),
        turn("claude", "main", "/x/ComandOS", "2026-10-01T02:00:00"),
    ]
    spans = [{"provider": "claude", "account": "main", "git_root": "/x/ComandOS",
              "started": ts("2026-09-30T23:30:00"), "finished": ts("2026-09-30T23:40:00")}]
    week = aw.build_week(now=NOW, offset=0, limits=LIMITS, turns=turns, spans=spans, snapshots=[], records=[])
    main = [s for s in week["sessions"] if s["acc"] == "claude:main"]
    assert [(s["d"], round(s["st"], 2), round(s["en"], 2)) for s in main] == [
        ("2026-09-30", 23.5, 24.0), ("2026-10-01", 0.0, 0.33), ("2026-10-01", 2.0, 2.0)]
    assert [round(s["tok"], 1) for s in main] == [1.0, 3.0, 1.0]
    assert [s["acc"] for s in week["sessions"] if s["acc"] != "claude:main"] == ["claude:relotto"]
    acc = {a["id"]: a for a in week["accounts"]}
    assert acc["claude:main"]["hoy"]["ses"] == 2 and acc["claude:relotto"]["sem"]["ses"] == 1


def test_unknown_account_counts_as_main_and_unlimited_accounts_still_exist():
    week = aw.build_week(now=NOW, offset=0, limits=[], turns=[turn("grok", "unknown", "/x/SAVA", "2026-10-01T01:00:00")],
                         spans=[], snapshots=[], records=[])
    assert [a["id"] for a in week["accounts"]] == ["grok:main"]
    assert week["accounts"][0]["week"] is None and week["sessions"][0]["acc"] == "grok:main"


def test_waste_and_past_week_use_closed_cycles_only():
    snaps = [
        {"provider": "grok", "account": "main", "window": "7d", "scope": "", "resets_at": ts("2026-09-26T20:00:00"), "percent": 12.0},
        {"provider": "grok", "account": "main", "window": "7d", "scope": "", "resets_at": ts("2026-09-19T20:00:00"), "percent": 8.0},
        {"provider": "grok", "account": "main", "window": "7d", "scope": "", "resets_at": ts("2026-10-03T20:00:00"), "percent": 6.0},
    ]
    limits = [{"provider": "grok", "account": "main", "window": "7d", "scope": "", "percent": 6.0, "resets_at": ts("2026-10-03T20:00:00")}]
    week = aw.build_week(now=NOW, offset=0, limits=limits, turns=[], spans=[], snapshots=snaps, records=[])
    assert week["waste"] == [{"id": "grok:main", "cyc": [88, 92]}]
    assert week["accounts"][0]["weekUsed"] == 6
    prev = aw.build_week(now=NOW, offset=-1, limits=limits, turns=[], spans=[], snapshots=snaps, records=[])
    assert prev["accounts"][0]["weekUsed"] == 12
    assert aw.build_week(now=NOW, offset=-1, limits=limits, turns=[], spans=[], snapshots=[], records=[])["accounts"][0]["weekUsed"] is None


def test_last_week_hours_come_from_the_previous_window():
    turns = [turn("codex", "main", "/x/SAVA", "2026-09-20T10:00:00", started=ts("2026-09-20T09:00:00")),
             turn("codex", "main", "/x/SAVA", "2026-09-30T10:00:00", started=ts("2026-09-30T09:30:00"))]
    week = aw.build_week(now=NOW, offset=0, limits=[], turns=turns, spans=[], snapshots=[], records=[])
    assert week["lastWeek"] == {"SAVA": 1.0}
    assert [s["d"] for s in week["sessions"]] == ["2026-09-30"]


def test_pomodoros_are_focus_blocks_with_clean_project_names():
    rec = lambda status, at, project, mode="focus": {  # noqa: E731
        "mode": mode, "status": status, "startedAtMs": ts(at) * 1000, "endedAtMs": ts(at) * 1000 + 25 * 60000,
        "targetMs": 25 * 60000, "plannedMs": 25 * 60000, "activeMs": 25 * 60000 if status == "completed" else 90000,
        "project": project}
    records = [rec("completed", "2026-10-01T01:29:00", "ComandOS ⎇ ⫽30"), rec("cancelled", "2026-09-30T14:15:00", "Signara ⫽42"),
               rec("completed", "2026-09-30T15:00:00", "x", mode="break"), rec("completed", "2026-09-01T10:00:00", "old")]
    week = aw.build_week(now=NOW, offset=0, limits=[], turns=[], spans=[], snapshots=[], records=records)
    assert [(p["d"], p["proj"], p["status"], p["plan"], p["act"]) for p in week["pomodoros"]] == [
        ("2026-09-30", "Signara", "cancelled", 25, 2), ("2026-10-01", "ComandOS", "completed", 25, 25)]


def test_model_has_the_fixture_shape_and_renders_every_tab():
    fixture = json.loads((ROOT / "tests/fixtures/analytics/week-normal.json").read_text())
    week = aw.build_week(now=NOW, offset=0, limits=LIMITS, turns=[turn("claude", "main", "/x/A", "2026-10-01T01:00:00")],
                         spans=[], snapshots=[], records=[])
    assert set(week) == set(fixture) and set(week["week"]) == set(fixture["week"])
    assert set(week["accounts"][0]) >= set(fixture["accounts"][0])
    assert set(week["sessions"][0]) == set(fixture["sessions"][0])
    script = ("const {create}=require('./dash/analytics-render.js');const m=JSON.parse(require('fs').readFileSync(0,'utf8'));"
              "for(const t of ['cuentas','comparar','pomodoro'])for(const phone of [false,true])create(m,{phone}).html(t);")
    for offset in (0, -1):
        model = aw.build_week(now=NOW, offset=offset, limits=LIMITS, turns=[], spans=[], snapshots=[], records=[])
        subprocess.run(["node", "-e", script], input=json.dumps(model), text=True, check=True, cwd=ROOT)


def test_a_limit_without_a_real_reset_shows_no_reset():
    # Grok con cuota declarada y sin lectura oficial: trae porcentaje pero resets_at 0.
    limits = [{"provider": "grok", "account": "main", "window": "7d", "scope": "", "percent": 40.0, "resets_at": 0}]
    week = aw.build_week(now=NOW, offset=0, limits=limits, turns=[], spans=[], snapshots=[], records=[])
    grok = next(a for a in week["accounts"] if a["id"] == "grok:main")
    assert grok["week"] == 40 and grok["reset"] is None and grok["left"] is None


def test_past_week_ignores_a_cycle_far_after_the_window():
    # Falta la foto del ciclo que cerraba la ventana: no se usa la de dos semanas después.
    snaps = [{"provider": "claude", "account": "main", "window": "7d", "scope": "", "percent": 88.0,
              "resets_at": int(NOW - 3600)}]
    limits = [{"provider": "claude", "account": "main", "window": "7d", "scope": "", "percent": 50.0,
               "resets_at": int(NOW + 86400)}]
    week = aw.build_week(now=NOW, offset=-1, limits=limits, turns=[], spans=[], snapshots=snaps, records=[])
    [acc] = week["accounts"]
    assert acc["weekUsed"] is None


def test_a_pomodoro_crossing_midnight_ends_at_midnight():
    tz = aw.ZoneInfo(aw.TZ)
    start = aw.datetime(2026, 9, 30, 23, 50, tzinfo=tz).timestamp() * 1000
    rec = {"mode": "focus", "status": "completed", "startedAtMs": start, "endedAtMs": start + 25 * 60000,
           "activeMs": 25 * 60000, "targetMs": 25 * 60000, "project": "Relotto"}
    [f] = aw.pomodoros([rec], ["2026-09-30"], tz)
    assert f["en"] == 24
