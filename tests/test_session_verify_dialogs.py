import json
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
from test_agent_launch import load_dash_module


def test_screen_dialog_detects_english_and_spanish():
    dash = load_dash_module()
    assert dash.screen_dialog("Do you trust the files in this folder?") == "trust"
    assert dash.screen_dialog("¿Confías en los archivos de esta carpeta?") == "trust"
    assert dash.screen_dialog("Please sign in to continue") == "login"
    assert dash.screen_dialog("Por favor inicia sesión para continuar") == "login"
    assert dash.screen_dialog("Choose the text style that looks best") == "onboarding"
    assert dash.screen_dialog("error: cannot resume session") == "error"
    assert dash.screen_dialog("❯ ") == ""


def test_trust_wins_over_an_incidental_error_line():
    dash = load_dash_module()
    screen = "error: something odd\nDo you trust the files in this folder?"
    assert dash.screen_dialog(screen) == "trust"


def test_dialog_patterns_come_from_config_not_code():
    dash = load_dash_module()
    pats = dash.dialog_patterns()
    assert set(pats) >= {"trust", "login", "onboarding", "error"}
    assert any("Conf" in p for p in pats["trust"]), "faltan los patrones en español"


def test_verify_attempts_grow_with_mcp(tmp_path, monkeypatch):
    dash = load_dash_module()
    monkeypatch.setenv("HOME", str(tmp_path))
    repo = tmp_path / "repo"
    repo.mkdir()
    assert dash.verify_attempts(str(repo), None) == 60
    (repo / ".mcp.json").write_text("{}")
    assert dash.verify_attempts(str(repo), None) == 180
    (repo / ".mcp.json").unlink()
    (tmp_path / ".claude.json").write_text(json.dumps({"mcpServers": {"x": {}}}))
    assert dash.verify_attempts(str(repo), None) == 180


def test_verify_uses_the_configured_detector_not_a_hard_coded_regex():
    """_verify ya no lleva el regex de diálogos incrustado: lo decide screen_dialog."""
    src = Path(__file__).resolve().parents[1].joinpath("bin/cc-dash").read_text()
    assert "trust this folder|Do you trust|Accessing workspace" not in src, (
        "el regex fijo sigue en _verify; debe usar screen_dialog(screen)")
    assert "if screen_dialog(screen):" in src


def test_verify_widens_the_window_with_verify_attempts():
    """verify() pasa attempts=verify_attempts(...), si no una sesión con MCPs no confirma a tiempo."""
    src = Path(__file__).resolve().parents[1].joinpath("bin/cc-dash").read_text()
    assert "attempts=verify_attempts(" in src


def test_pending_confirmation_records_which_dialog_stopped_the_start():
    """Un diálogo de trust/login/bienvenida no invalida la observación, pero queda anotado."""
    src = Path(__file__).resolve().parents[1].joinpath("bin/cc-dash").read_text()
    assert "self.screen_dialog_kind = kind" in src
    assert "kind == 'error'" in src


# --- la config puede venir mal escrita: nunca debe casar con todo ni reventar ---

def _cfg_with(monkeypatch, tmp_path, dialog_patterns):
    """Redirige REPO_ROOT a un config/detectors.json de pega."""
    dash = load_dash_module()
    cfg = tmp_path / "config"
    cfg.mkdir(exist_ok=True)
    (cfg / "detectors.json").write_text(json.dumps({"dialogPatterns": dialog_patterns}))
    monkeypatch.setattr(dash, "REPO_ROOT", str(tmp_path))
    dash._DIALOG_CACHE.update(mtime=None, value=None)
    return dash


def test_a_bare_string_pattern_is_not_exploded_into_letters(tmp_path, monkeypatch):
    """`"trust": "Do you trust"` (sin lista) no puede volverse ['D','o',' ',...],
    que casaría con cualquier pantalla y dejaría el cambio sin confirmar nunca."""
    dash = _cfg_with(monkeypatch, tmp_path, {"trust": "Do you trust"})
    assert dash.dialog_patterns()["trust"] == ["Do you trust"]
    assert dash.screen_dialog("compilando el proyecto...") == ""
    assert dash.screen_dialog("Do you trust the files?") == "trust"


def test_a_non_string_pattern_never_raises(tmp_path, monkeypatch):
    """`"trust": 5` reventaba con TypeError dentro del bucle de sondeo."""
    dash = _cfg_with(monkeypatch, tmp_path, {"trust": 5, "login": [7, "sign in"]})
    assert dash.screen_dialog("texto cualquiera") == ""
    assert dash.screen_dialog("please sign in") == "login"


def test_an_invalid_regex_is_dropped_not_fatal(tmp_path, monkeypatch):
    dash = _cfg_with(monkeypatch, tmp_path, {"trust": ["(sin cerrar", "Do you trust"]})
    assert dash.dialog_patterns()["trust"] == ["Do you trust"]
    assert dash.screen_dialog("Do you trust the files?") == "trust"


def test_a_missing_kind_falls_back_per_kind(tmp_path, monkeypatch):
    """Si la config solo trae 'trust', las otras categorías no desaparecen."""
    dash = _cfg_with(monkeypatch, tmp_path, {"trust": ["Do you trust"]})
    assert dash.screen_dialog("error: algo") == "error"
    assert dash.screen_dialog("please sign in") == "login"


def test_patterns_are_cached_between_calls(tmp_path, monkeypatch):
    """dialog_patterns corre hasta 180 veces por cambio: no puede releer el disco cada vez."""
    dash = _cfg_with(monkeypatch, tmp_path, {"trust": ["Do you trust"]})
    reads = []
    real = dash._detectors_cfg
    monkeypatch.setattr(dash, "_detectors_cfg", lambda: reads.append(1) or real())
    dash._DIALOG_CACHE.update(mtime=None, value=None)
    for _ in range(50):
        dash.screen_dialog("nada que ver")
    assert len(reads) == 1, f"releyó la config {len(reads)} veces"


def test_onboarding_patterns_are_specific_enough(tmp_path, monkeypatch):
    """Frases genéricas como "Let's get started" casan con código o transcripciones
    del propio usuario y bloquearían el sondeo hasta 90 s."""
    dash = load_dash_module()
    dash._DIALOG_CACHE.update(mtime=None, value=None)
    for innocent in ("Let's get started with the migration",
                     "# Welcome to Claude Code, our internal guide",
                     "README: getting started"):
        assert dash.screen_dialog(innocent) == "", innocent
    assert dash.screen_dialog("Choose the text style that looks best") == "onboarding"
    assert dash.screen_dialog("Elige el estilo de texto") == "onboarding"
