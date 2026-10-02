"""/usage/state enriquece cada límite con ritmo y proyección (lib/allocation.enrich_limits)."""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
from test_agent_launch import load_dash_module
from test_allocation import LIMITS, NOW


def test_limits_salen_enriquecidos(tmp_path, monkeypatch):
    dash = load_dash_module()
    monkeypatch.setattr(dash.time, "time", lambda: NOW)
    filas = dash.enriched_limits(LIMITS)
    semanal = next(f for f in filas if f["window"] == "7d")
    assert semanal["burn"] and semanal["verdict"]
    assert semanal["verdict"].startswith(("llega al reset", "se acaba en"))
