#!/usr/bin/env python3
"""Tests para el schema de preferencias de terminal (fuente, cursor, padding,
opacidad, ligaduras) — extendido en cc-dash /prefs y aplicado en cc-app.
No requiere runtime; parsea el código fuente."""
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CC_DASH = (REPO / "bin" / "cc-dash").read_text()
CC_APP  = (REPO / "bin" / "cc-app").read_text()
INDEX   = (REPO / "dash" / "index.html").read_text()


def test_cc_dash_defines_terminal_prefs_defaults():
    for key in ("font_family", "font_size", "cursor_shape", "cursor_blink",
                "terminal_padding", "terminal_opacity", "ligatures"):
        assert f'"{key}"' in CC_DASH, f"cc-dash no expone {key} en PREFS_DEFAULTS"
    # El default de cursor_shape debe ser uno de los 3 válidos.
    assert '"block"' in CC_DASH and '"ibeam"' in CC_DASH and '"underline"' in CC_DASH


def test_cc_dash_prefs_set_validates_new_fields():
    # /prefs-set debe validar tipos y clamear rangos para no aceptar basura.
    handler = CC_DASH.split('if self.path == "/prefs-set":')[1].split("return")[0]
    assert 'data.get("font_family")' in handler
    assert 'data.get("font_size")' in handler
    assert 'data.get("cursor_shape")' in handler
    assert 'data.get("cursor_blink")' in handler
    assert 'data.get("terminal_padding")' in handler
    assert 'data.get("terminal_opacity")' in handler
    assert 'data.get("ligatures")' in handler


def test_cc_app_reads_and_maps_terminal_prefs():
    # cc-app usa fetch_prefs (no solo fetch_pref_theme) y expone las constantes.
    assert "def fetch_prefs" in CC_APP
    for c in ("TERM_PADDING", "TERM_OPACITY", "_CURSOR_SHAPE", "_CURSOR_BLINK",
              "_CURSOR_SHAPE_MAP", "FONT_FAMILY", "FONT_SIZE"):
        assert c in CC_APP, f"cc-app no expone {c}"
    # Los shapes de VTE deben estar mapeados.
    assert "Vte.CursorShape.BLOCK" in CC_APP
    assert "Vte.CursorShape.IBEAM" in CC_APP
    assert "Vte.CursorShape.UNDERLINE" in CC_APP


def test_cc_app_applies_prefs_to_vte():
    # make_term aplica los prefs — cursor shape, blink, y padding configurable.
    make_term = CC_APP.split("def make_term")[1].split("def ")[0]
    assert "set_cursor_shape" in make_term
    assert "set_cursor_blink_mode" in make_term
    assert "TERM_PADDING" in make_term
    assert "_bg_rgba()" in make_term  # background con opacity


def test_dashboard_settings_has_terminal_section():
    # El modal de Ajustes trae los controles nuevos con ids estables.
    for i in ("pf-font-family", "pf-font-size", "pf-padding", "pf-opacity",
              "sw-cursor-blink", "sw-ligatures"):
        assert f'id="{i}"' in INDEX, f"dash/index.html no tiene el control {i}"
    # Los 3 shapes de cursor son elegibles con botones .cur-shape.
    assert 'class="test cur-shape" data-shape="block"' in INDEX
    assert 'class="test cur-shape" data-shape="ibeam"' in INDEX
    assert 'class="test cur-shape" data-shape="underline"' in INDEX


def test_dashboard_wires_prefs_change_to_setPref():
    # El JS debe postear /prefs-set con el patch correspondiente al cambiar.
    assert "setPref({font_family:" in INDEX
    assert "setPref({font_size:" in INDEX
    assert "setPref({cursor_shape:" in INDEX
    assert "setPref({cursor_blink:" in INDEX
    assert "setPref({terminal_padding:" in INDEX
    assert "setPref({terminal_opacity:" in INDEX
    assert "setPref({ligatures:" in INDEX


def test_cc_app_applies_prefs_live_to_open_terminals():
    # Los cambios de Ajustes deben verse EN TIEMPO REAL en las terminales
    # abiertas: apply_terminal_prefs recorre las paginas del notebook y
    # re-aplica fuente/cursor/padding/opacidad a cada VTE.
    assert "def apply_terminal_prefs" in CC_APP
    body = CC_APP.split("def apply_terminal_prefs", 1)[1].split("\ndef ", 1)[0]
    for marker in ("notebook_pages()", "set_font(", "set_cursor_shape(",
                   "set_margin_start(", "_bg_rgba()"):
        assert marker in body, f"apply_terminal_prefs no re-aplica: {marker}"


def test_cc_app_watches_prefs_for_changes():
    # El poll de estado tambien vigila /prefs: cambiar Ajustes desde la app,
    # Chrome o el celular aplica en <=3s sin reiniciar la app.
    body = CC_APP.split("def poll_state_loop", 1)[1].split("\ndef ", 1)[0]
    assert "/prefs" in body
    assert "apply_terminal_prefs" in body
    assert "apply_theme" in body
    assert "_LIVE_PREF_KEYS" in CC_APP


if __name__ == "__main__":
    for name, fn in list(globals().items()):
        if name.startswith("test_") and callable(fn):
            fn()
            print(f"ok  {name}")
    print("terminal prefs tests pass")


def test_font_selector_only_offers_installed_fonts_with_a11y_flag():
    # cc-dash consulta fc-list y expone p.fonts; el dashboard construye las
    # <option> desde ahí (nada de fonts fantasma que no hacen nada al elegirlas).
    assert "def installed_terminal_fonts" in CC_DASH
    assert '["fc-list", ":", "family"]' in CC_DASH
    assert 'prefs["fonts"] = installed_terminal_fonts()' in CC_DASH
    assert '("Atkinson Hyperlegible Mono", "Atkinson Hyperlegible Mono", True)' in CC_DASH
    assert '("Ubuntu Sans Mono",' in CC_DASH
    assert "Array.isArray(p.fonts)" in INDEX
    assert 'tf("accesible", "accessible")' in INDEX


def test_default_terminal_font_is_ubuntu_sans_mono_everywhere():
    # Tema o no, la mono por defecto es la de Ubuntu (Ptyxis / 24.04+):
    # pref, fallback de cc-app, cadena VTE, ttyd y xterm. JetBrains queda de respaldo.
    assert '"font_family": "Ubuntu Sans Mono"' in CC_DASH
    assert 'or "Ubuntu Sans Mono"' in CC_APP
    assert 'FONT_FAMILY = "Ubuntu Sans Mono,JetBrainsMono Nerd Font Mono,' in CC_APP
    assert 'p.font_family || "Ubuntu Sans Mono"' in INDEX
    assert "fontFamily=Ubuntu Sans Mono, " in (REPO / "bin" / "cc-webterm").read_text()
    assert "'Ubuntu Sans Mono', 'JetBrainsMono Nerd Font Mono'" in (REPO / "dash" / "term.html").read_text()
    # el tema ubuntu también cambia la sans del chrome a Ubuntu Sans
    assert "--sans:'Ubuntu Sans'" in INDEX.split(':root[data-theme="ubuntu"]')[1].split("}")[0]
