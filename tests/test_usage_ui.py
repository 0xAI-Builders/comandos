#!/usr/bin/env python3
from pathlib import Path


HTML = Path("dash/index.html").read_text()


def test_usage_drawer_markup_exists():
    assert 'id="btn-usage"' in HTML
    assert 'id="usage"' in HTML
    assert 'id="usage-limits"' in HTML
    assert 'id="usage-providers"' in HTML
    assert 'id="usage-projects"' in HTML
    assert 'id="usage-alerts"' in HTML


def test_usage_state_is_fetched_without_secret_rendering():
    assert 'api("/usage/state")' in HTML
    assert "renderUsage" in HTML
    forbidden = ["OPENAI_ADMIN_KEY", "ANTHROPIC_ADMIN_KEY", "x-api-key"]
    for text in forbidden:
        assert text not in HTML


def test_session_cards_have_usage_chip_container():
    assert 'class="usage-chip hidden"' in HTML
    assert "usageChipText" in HTML


def test_usage_ui_exposes_model_selector_with_preset_names():
    # Un solo control por concepto: menu de modelo con la intencion como etiqueta
    for preset in ("Ahorro", "Diario", "Difícil", "Máximo"):
        assert preset in HTML
    assert "MODEL_CHOICES" in HTML
    assert 'api("/model/switch"' in HTML
    assert "model: " in HTML


def test_usage_ui_labels_detected_panes_without_unattributed_noise():
    assert "function confidenceLabel" in HTML
    assert '"detected": "detectado"' in HTML
    assert "confidenceLabel(u.confidence)" in HTML


def test_usage_ui_switches_models_inline_per_pane():
    # Menu propio (no <select> nativo: el WebKitGTK viejo de la app no lo abre)
    assert "mdl-menu" in HTML
    assert "mdl-btn" in HTML
    assert "modelSelectEl" not in HTML
    assert "session: btn.dataset.session" in HTML
    assert "pane: btn.dataset.pane || undefined" in HTML
    assert 'role="listbox"' in HTML
    assert 'role="option"' in HTML


def test_session_cards_have_model_selector():
    assert "modelMenuEl" in HTML
    assert "function wireModelMenu" in HTML
    assert "mdl-cur" in HTML


def test_header_has_agent_toggle_for_new_sessions():
    assert 'id="agent-pick"' in HTML
    for agent in ("claude", "codex", "opencode", "gemini", "agy"):
        assert f'data-a="{agent}"' in HTML
    assert "pickedAgent" in HTML
    assert "cc-new-agent" in HTML


def test_card_usage_chip_is_full_width_line():
    # Texto COMPLETO siempre: el chip vive en su propia linea, sin ellipsis
    assert ".card .usage-chip" in HTML
    assert 'white-space:normal' in HTML


def test_usage_drawer_collapses_historic_sessions():
    # Las sesiones historicas (transcripts) no se listan una por una:
    # se ven los panes vivos + una linea de historial (adios duplicados)
    assert "historial" in HTML
    assert 'startsWith("%")' in HTML


def test_usage_ui_renders_exact_limit_bars():
    # Porcentajes exactos del proveedor (OAuth Claude / rollouts Codex),
    # sin inputs manuales de limites de tokens.
    assert 'id="usage-limits"' in HTML
    assert "renderUsageLimits" in HTML
    assert "resetea" in HTML
    assert 'id="limit-codex-daily"' not in HTML
    assert 'id="usage-limits-save"' not in HTML


def test_header_shows_global_limit_percentages():
    # Los porcentajes globales viven SIEMPRE visibles en el header,
    # no solo dentro del drawer
    assert 'id="limits-strip"' in HTML
    assert "renderLimitsStrip" in HTML


if __name__ == "__main__":
    test_usage_drawer_markup_exists()
    test_usage_state_is_fetched_without_secret_rendering()
    test_session_cards_have_usage_chip_container()
    test_usage_ui_exposes_model_selector_with_preset_names()
    test_usage_ui_labels_detected_panes_without_unattributed_noise()
    test_usage_ui_switches_models_inline_per_pane()
    test_session_cards_have_model_selector()
    test_usage_ui_renders_exact_limit_bars()
    test_header_shows_global_limit_percentages()
    test_header_has_agent_toggle_for_new_sessions()
    test_card_usage_chip_is_full_width_line()
    test_usage_drawer_collapses_historic_sessions()
