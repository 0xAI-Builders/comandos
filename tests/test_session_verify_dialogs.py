import json
import re
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
    assert dash.screen_dialog("Please sign in to continue") == "login"


def test_an_invalid_regex_is_dropped_not_fatal(tmp_path, monkeypatch):
    dash = _cfg_with(monkeypatch, tmp_path, {"trust": ["(sin cerrar", "Do you trust"]})
    assert dash.dialog_patterns()["trust"] == ["Do you trust"]
    assert dash.screen_dialog("Do you trust the files?") == "trust"


def test_a_missing_kind_falls_back_per_kind(tmp_path, monkeypatch):
    """Si la config solo trae 'trust', las otras categorías no desaparecen."""
    dash = _cfg_with(monkeypatch, tmp_path, {"trust": ["Do you trust"]})
    assert dash.screen_dialog("error: algo") == "error"
    assert dash.screen_dialog("Please sign in to continue") == "login"


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


def test_login_patterns_do_not_fire_on_ordinary_pane_content():
    """El detector es una red de seguridad, no la única defensa: _verify exige
    además prompt de shell, modelo, effort, cuentas y conversación. Así que se
    prefieren patrones específicos y se aceptan falsos negativos; un falso
    positivo congelaría el sondeo hasta 90 s sin motivo."""
    dash = load_dash_module()
    dash._DIALOG_CACHE.update(mtime=None, value=None)
    inocentes = [
        "dile al usuario que debe sign in antes de seguir",
        "  await user.login({ retry: true })",
        "# TODO: log in the request duration",
        "el commit dice: log into the new format",
        # prosa reflowed por el ancho del terminal: la frase cae a principio de línea
        "the user must\nsign in before the deploy can continue",
        # cuerpo indentado de un git log
        "commit 9fd2\n\n    log into the staging environment\n",
        "Getting started\n\nlogin flows are documented below",
        # testigos que aportó la revisión: la frase existe, pero a media línea
        "previously you had to sign in to use offline mode; no longer required",
        "note: authentication required for the admin api since v3",
        "warning: authentication required for endpoint /admin/users",
        "dev: yeah login required for that page now",
        "nota: se requiere iniciar sesión para usar la api de administración",
        # testigos de la ronda 4: viñetas y tablas de markdown, que NO son marco de TUI
        "- Login required for admin API calls",
        "* Login required for team workspaces (previously optional)",
        "- Sign in to continue using advanced search (new in this release)",
        "| Login required | for admin panel only |",
        "• Sign in to use the beta channel",
    ]
    for texto in inocentes:
        assert dash.screen_dialog(texto) == "", texto
    reales = [
        "Please sign in to continue",
        "│ Sign in to continue",
        "login required",
        "authentication required",
        "> sign in required",   # cadena real que ya fijaba test_session_route_matrix
        "  /login",
        "Inicia sesión para continuar",
        "Se requiere iniciar sesión",
        "┃ Login required",
    ]
    for texto in reales:
        assert dash.screen_dialog(texto) == "login", texto


def test_an_operator_can_disable_a_kind_by_emptying_it(tmp_path, monkeypatch):
    """`"trust": []` es intención explícita: el fallback no puede resucitarla."""
    dash = _cfg_with(monkeypatch, tmp_path, {"trust": [], "login": ["login required"]})
    assert dash.dialog_patterns()["trust"] == []
    assert dash.screen_dialog("Do you trust the files in this folder?") == ""
    assert dash.screen_dialog("login required") == "login"


def test_a_kind_with_a_wrong_type_still_falls_back(tmp_path, monkeypatch):
    """Un tipo equivocado es un error, no una intención: ahí sí vale el defecto."""
    dash = _cfg_with(monkeypatch, tmp_path, {"trust": {"mal": "escrito"}})
    assert dash.screen_dialog("Do you trust the files in this folder?") == "trust"


def test_a_missing_config_file_falls_back_to_code(tmp_path, monkeypatch):
    """_detectors_cfg debe degradar, no propagar FileNotFoundError."""
    dash = load_dash_module()
    monkeypatch.setattr(dash, "REPO_ROOT", str(tmp_path / "no-existe"))
    dash._DIALOG_CACHE.update(mtime=None, value=None)
    assert dash._detectors_cfg() == {}
    assert dash.screen_dialog("Do you trust the files in this folder?") == "trust"
    assert dash.screen_dialog("error: algo") == "error"


def test_invalid_json_falls_back_to_code(tmp_path, monkeypatch):
    """Un JSON roto tampoco puede tumbar el bucle de sondeo."""
    dash = load_dash_module()
    cfg = tmp_path / "config"
    cfg.mkdir()
    (cfg / "detectors.json").write_text('{"dialogPatterns": {"trust": [  <- esto no es JSON')
    monkeypatch.setattr(dash, "REPO_ROOT", str(tmp_path))
    dash._DIALOG_CACHE.update(mtime=None, value=None)
    assert dash._detectors_cfg() == {}
    assert dash.screen_dialog("Do you trust the files in this folder?") == "trust"


def test_default_dialogs_are_bilingual_like_the_config():
    """El defecto en código es la red de seguridad: no puede ser más pobre que la config."""
    dash = load_dash_module()
    onboarding = " ".join(dash._DEFAULT_DIALOGS["onboarding"])
    assert "Elige" in onboarding, "el fallback de bienvenida no cubre español"
    login = " ".join(dash._DEFAULT_DIALOGS["login"])
    assert "sesi" in login, "el fallback de login no cubre español"
    # cada patrón de login lleva las DOS defensas: anclado a línea y con contexto
    for pat in dash._DEFAULT_DIALOGS["login"]:
        assert pat.startswith("(^|"), f"patrón de login sin anclar: {pat}"
        assert any(w in pat for w in ("required", "continue|use|proceed", "sesi", "/login")), pat


def test_a_list_whose_patterns_are_all_invalid_falls_back(tmp_path, monkeypatch):
    """`"trust": [123]` es un error de escritura, no "apaga la categoría": si se
    tratara como intención, un typo desactivaría en silencio un detector de seguridad.
    Apagar se escribe con [] vacío, y eso sí se respeta."""
    dash = _cfg_with(monkeypatch, tmp_path, {"trust": [123, None]})
    assert dash.screen_dialog("Do you trust the files in this folder?") == "trust"
    dash2 = _cfg_with(monkeypatch, tmp_path, {"trust": []})
    assert dash2.screen_dialog("Do you trust the files in this folder?") == ""


def test_the_cache_key_is_read_under_the_same_lock_as_the_value(tmp_path, monkeypatch):
    """El stat fuera del lock permitía cachear un valor correcto bajo una mtime vieja,
    forzando un recálculo extra en la siguiente llamada."""
    src = Path(__file__).resolve().parents[1].joinpath("bin/cc-dash").read_text()
    cuerpo = src[src.index("def dialog_patterns():"):src.index("def screen_dialog(")]
    assert cuerpo.index("_DIALOG_CACHE_LOCK") < cuerpo.index("os.stat("), (
        "os.stat debe quedar dentro del lock")


def test_screen_dialog_is_case_insensitive_and_multiline():
    """Los patrones se aplican con re.I | re.M: las mayúsculas del inicio de frase
    y el anclaje por línea dependen de ello."""
    dash = load_dash_module()
    dash._DIALOG_CACHE.update(mtime=None, value=None)
    assert dash.screen_dialog("LOGIN REQUIRED") == "login"
    assert dash.screen_dialog("salida previa\nDo you trust the files?") == "trust"


def test_a_dialog_sentence_inside_a_box_is_a_known_accepted_miss():
    """Límite aceptado a propósito: una frase dentro de una caja ("│ You need to
    log in to use Claude") lleva el término a media línea y no se detecta. El coste
    es un mensaje peor en pending_confirmation, nunca una confirmación de más:
    _verify exige además prompt de shell, modelo, effort, cuentas y conversación.
    El coste contrario —un changelog que diga "authentication required" y congele
    el sondeo 90 s— sí es real, y por eso se elige este lado."""
    dash = load_dash_module()
    dash._DIALOG_CACHE.update(mtime=None, value=None)
    assert dash.screen_dialog("│ You need to log in to use Claude") == ""
    assert dash.screen_dialog("│ Sign in to continue") == "login"


def test_only_real_tui_frame_counts_as_decoration():
    """Las viñetas y celdas de markdown (-, *, |, ·, •) no son marco de terminal:
    si contaran, "- Login required for admin API calls" pasaría las dos defensas."""
    dash = load_dash_module()
    clase = re.search(r"\[([^\]]*)\]", dash._LOGIN_LINE).group(1)
    for marcador in ("*", "|", "-", "·", "•"):
        assert marcador not in clase, f"{marcador} sigue contando como marco: {clase}"
    assert "│" in clase and ">" in clase, "falta marco real de TUI"
