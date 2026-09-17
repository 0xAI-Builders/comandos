import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
import allocation as al

NOW = 1_800_000_000.0
WEEK = 7 * 24 * 3600

def limit(provider, account, percent, resets_in, window="7d", **extra):
    return dict(id=f"{provider}:{account}:{window}", provider=provider, account=account,
                percent=percent, resets_at=NOW + resets_in, window=window, **extra)

def test_fmt_duration():
    assert al.fmt_duration(45 * 60) == "45 min"
    assert al.fmt_duration(7 * 3600) == "7 h"
    assert al.fmt_duration(46 * 3600) == "1d 22h"

def test_enrich_pace_and_burn_week():
    # 63 % usado con 85.5 h para el reset: transcurrido 49 % de la semana → ritmo 1.29x
    rows = al.enrich_limits([limit("claude", "main", 63.0, 85.5 * 3600)], NOW)
    r = rows[0]
    assert round(r["pace"], 1) == 49.1
    assert round(r["burn"], 2) == 1.28
    assert r["reachesReset"] is False
    assert r["verdict"].startswith("se acaba en ")
    assert 0 < r["runsOutIn"] < 85.5 * 3600

def test_enrich_reaches_reset():
    r = al.enrich_limits([limit("codex", "main", 1.0, 130 * 3600)], NOW)[0]
    assert r["reachesReset"] is True
    assert r["runsOutIn"] is None
    assert r["verdict"] == "llega al reset"
    assert r["burn"] < 0.2

def test_enrich_unknown_window_has_no_projection():
    r = al.enrich_limits([limit("grok", "main", 40.0, 3600, window="")], NOW)[0]
    assert r["burn"] is None and r["verdict"] == "" and r["reachesReset"] is None

def test_enrich_does_not_mutate_input():
    src = [limit("claude", "main", 10.0, 3600, window="5h")]
    al.enrich_limits(src, NOW)
    assert "burn" not in src[0]
