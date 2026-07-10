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
    # Un solo control por concepto: select de modelo con la intencion como etiqueta
    for preset in ("Ahorro", "Diario", "Difícil", "Máximo"):
        assert preset in HTML
    assert "function modelOptions" in HTML
    assert 'api("/model/switch"' in HTML
    assert "model: " in HTML


def test_usage_ui_labels_detected_panes_without_unattributed_noise():
    assert "function confidenceLabel" in HTML
    assert '"detected": "detectado"' in HTML
    assert "confidenceLabel(u.confidence)" in HTML


def test_usage_ui_switches_models_inline_per_pane():
    assert 'class="pane-model-controls"' in HTML
    assert "pane-model" in HTML
    assert "session: sel.dataset.session" in HTML
    assert "pane: sel.dataset.pane || undefined" in HTML


def test_session_cards_have_model_selector():
    assert '"mdl")' in HTML          # cardEl inyecta modelSelectEl(..., "mdl")
    assert 'el.querySelectorAll("select.mdl").forEach(wireModelSelect)' in HTML
    assert "function wireModelSelect" in HTML


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
