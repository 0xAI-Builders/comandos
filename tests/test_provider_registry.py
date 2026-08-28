#!/usr/bin/env python3
import json
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import providers


def test_registry_validates_and_classifies_all_three_claude_engines():
    registry = providers.load_registry(ROOT / "config" / "providers.json")
    assert providers.engine_for_model(registry, "claude-fable-5") == "claude"
    assert providers.engine_for_model(registry, "gpt-5.6-sol") == "codex"
    assert providers.engine_for_model(registry, "grok-4.6") == "grok"


def test_grok_models_expose_only_valid_efforts():
    registry = providers.load_registry(ROOT / "config" / "providers.json")
    assert providers.effort_allowed(registry, "grok", "grok-4.6", "xhigh")
    assert not providers.effort_allowed(registry, "grok", "grok-4.5", "xhigh")
    assert providers.effort_allowed(registry, "grok", "grok-4.5", "high")


def test_public_registry_redacts_auth_locations_and_detects_grok():
    registry = providers.load_registry(ROOT / "config" / "providers.json")
    public = providers.public_state(registry)
    grok = public["harnesses"]["grok"]
    assert grok["available"] is True
    assert "authFile" not in grok
    assert "accountsRoot" not in grok
    assert "authSource" not in public["claudeEngines"]["grok"]


def test_registry_rejects_bad_engine_regex(tmp_path):
    registry = providers.load_registry(ROOT / "config" / "providers.json")
    registry["claudeEngines"]["grok"]["modelMatch"] = "["
    path = tmp_path / "providers.json"
    path.write_text(json.dumps(registry))
    with pytest.raises(providers.ProviderRegistryError):
        providers.load_registry(path)
