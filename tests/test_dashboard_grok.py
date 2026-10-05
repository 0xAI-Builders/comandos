#!/usr/bin/env python3
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HTML = (ROOT / "dash/index.html").read_text()
PROXY = json.loads((ROOT / "config/proxy.json").read_text())
REGISTRY = json.loads((ROOT / "config/providers.json").read_text())
TIERS = json.loads((ROOT / "config/model-tiers.json").read_text())


def test_xai_logo_and_provider_match_ship_together():
    assert re.search(r"\bxai:\s*'<svg", HTML)
    assert re.search(r"\bgrok:\s*'<svg", HTML)
    assert (ROOT / "dash/icons/grok.svg").is_file()
    assert TIERS["providers"][0]["icon"] == "xai"
    assert "grok" in TIERS["providers"][0]["match"]


def test_grok_models_and_per_model_efforts_are_exposed():
    models = {m["id"]: m for m in REGISTRY["motors"]["grok"]["models"]}
    assert models["grok-4.6"]["efforts"] == ["low", "medium", "high", "xhigh"]
    assert models["grok-4.5"]["efforts"] == ["low", "medium", "high"]
    assert '(nsModel().efforts||[]).map' in HTML


def test_wizard_is_matrix_driven():
    assert "const nsMatrix =" in HTML
    assert 'routeId:NS.routeId' in HTML
    assert '5 disponibles' not in HTML  # counts come from runtime matrix


def test_harness_accounts_are_registry_driven_for_all_providers():
    assert 'function nsAccountItems(provider,role="harness")' in HTML
    assert "nsHarness(provider).accounts" in HTML
    assert 'NS.harness === "grok"' not in HTML
    assert "PROXY.accounts || []" not in HTML


def test_switch_loading_and_results_are_pane_keyed():
    assert "r.operationKey" in HTML
    assert "pend.stageCode" in HTML
    assert 'confirmando modelo y esfuerzo' not in HTML  # stage comes from backend, not a fake optimistic string


def test_native_and_cross_engine_routes_have_explicit_backend_ids():
    assert "const nsMatrix =" in HTML
    assert "routeId: s2.routeId" in HTML   # sugerencia aplicada desde el cajón (la tarjeta Centro se retiró en S2)
    assert 'Harness' in HTML and 'Motor' in HTML and 'Pensamiento' in HTML
