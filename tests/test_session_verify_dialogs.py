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
