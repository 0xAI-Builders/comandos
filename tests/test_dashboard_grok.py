#!/usr/bin/env python3
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HTML = (ROOT / "dash/index.html").read_text()
PROXY = json.loads((ROOT / "config/proxy.json").read_text())
TIERS = json.loads((ROOT / "config/model-tiers.json").read_text())


def test_xai_logo_and_provider_match_ship_together():
    assert re.search(r"\bxai:\s*'<svg", HTML)
    assert re.search(r"\bgrok:\s*'<svg", HTML)
    assert (ROOT / "dash/icons/grok.svg").is_file()
    assert TIERS["providers"][0]["icon"] == "xai"
    assert "grok" in TIERS["providers"][0]["match"]


def test_grok_models_and_per_model_efforts_are_exposed():
    models = {m["id"]: m for m in PROXY["grok"]["menu"]}
    assert models["grok-4.6"]["efforts"] == ["low", "medium", "high", "xhigh"]
    assert models["grok-4.5"]["efforts"] == ["low", "medium", "high"]
    assert 'function effortsFor(prov, model)' in HTML
    assert "tileEfforts.map" in HTML


def test_claude_picker_is_three_engine_and_native_grok_is_single_engine():
    assert 'const ENGINE_IDS = ["claude", "codex", "grok"]' in HTML
    assert 'item && ["grok","codex"].includes(item.agent) ? [item.agent] : ENGINE_IDS' in HTML
    assert 'driver = ["grok","codex"].includes(item.agent) ? item.agent : "claude"' in HTML


def test_grok_wizard_modes_and_accounts_are_visible():
    assert '{id: "claude-grok", label: "Claude + Grok"' in HTML
    assert '{id: "grok", label: "Grok Build"' in HTML
    assert 'provider: item && item.agent === "grok" ? "grok" : "claude"' in HTML


def test_switch_loading_and_results_are_pane_keyed():
    assert "const motorTargetKey = it => it ? rowKey(it)" in HTML
    assert "r.operationKey" in HTML
    assert 'stageTxt: r && r.queued ?' in HTML
    assert 'confirmando modelo y esfuerzo' not in HTML  # stage comes from backend, not a fake optimistic string


def test_native_harness_cards_have_their_own_model_picker_and_driver():
    assert '["claude","codex","grok"].includes(it.agent)' in HTML
    assert 'item && ["grok","codex"].includes(item.agent) ? [item.agent] : ENGINE_IDS' in HTML
    assert 'driver = ["grok","codex"].includes(item.agent) ? item.agent : "claude"' in HTML
    assert 'Harness / motor' in HTML
    for label in ("Claude Code", "Claude + Codex", "Claude + Grok", "Codex CLI", "Grok Build"):
        assert label in HTML
