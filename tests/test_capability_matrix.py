#!/usr/bin/env python3
import copy
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import providers


def facts(**overrides):
    value = {
        "harnesses": {x: {"available": True, "authenticated": True} for x in ("claude", "codex", "grok")},
        "motors": {x: {"authenticated": True} for x in ("claude", "codex", "grok")},
        "gateway": {"installed": True, "alive": True},
    }
    value.update(overrides)
    return value


def test_matrix_covers_exactly_nine_cells_and_five_are_live_subscription_routes():
    registry = providers.load_registry(ROOT / "config/providers.json")
    matrix = providers.evaluate_capability_matrix(registry, facts())
    assert {(x["harness"], x["motor"]) for x in matrix} == {
        (h, m) for h in ("claude", "codex", "grok") for m in ("claude", "codex", "grok")
    }
    assert {x["id"] for x in matrix if x["selectable"]} == {
        "claude:claude", "claude:codex", "claude:grok", "codex:codex", "grok:grok"
    }


def test_unavailable_cells_are_explanatory_and_never_selectable():
    registry = providers.load_registry(ROOT / "config/providers.json")
    matrix = providers.evaluate_capability_matrix(registry, facts())
    reasons = {x["id"]: x["reason"]["code"] for x in matrix if not x["selectable"]}
    assert reasons == {
        "codex:claude": "unsupported_protocol",
        "codex:grok": "route_not_live_verified",
        "grok:claude": "subscription_not_accepted",
        "grok:codex": "route_not_live_verified",
    }


def test_gateway_down_disables_only_external_claude_routes():
    registry = providers.load_registry(ROOT / "config/providers.json")
    runtime = facts(gateway={"installed": True, "alive": False})
    matrix = providers.evaluate_capability_matrix(registry, runtime)
    selected = {x["id"] for x in matrix if x["selectable"]}
    assert selected == {"claude:claude", "codex:codex", "grok:grok"}
    assert {x["reason"]["code"] for x in matrix if x["id"] in ("claude:codex", "claude:grok")} == {"gateway_down"}


def test_scope_and_selection_validation_use_route_catalog():
    registry = providers.load_registry(ROOT / "config/providers.json")
    matrix = providers.evaluate_capability_matrix(registry, facts())
    assert {x["id"] for x in providers.allowed_routes(matrix, "session_motor", "claude")} == {
        "claude:claude", "claude:codex", "claude:grok"
    }
    assert providers.validate_selection(
        registry, matrix,
        {"routeId": "claude:grok", "model": "grok-4.6", "effort": "xhigh"},
        "session_motor",
    )["id"] == "claude:grok"
    with pytest.raises(providers.ProviderRegistryError):
        providers.validate_selection(
            registry, matrix,
            {"routeId": "grok:grok", "model": "grok-4.5", "effort": "xhigh"},
            "session_model",
        )


def test_invalid_registry_with_missing_cell_is_rejected():
    registry = providers.load_registry(ROOT / "config/providers.json")
    broken = copy.deepcopy(registry)
    broken["exclusions"].pop()
    with pytest.raises(providers.ProviderRegistryError):
        providers.validate_registry(broken)


def test_binary_detection_survives_minimal_daemon_path(tmp_path, monkeypatch):
    """cc-dash corre bajo systemd con PATH mínimo (sin ~/.local/bin, ~/.bun/bin):
    la detección de claude/codex/grok no puede depender del PATH heredado."""
    for rel in (".local/bin/claude", ".bun/bin/codex", ".local/bin/grok"):
        binary = tmp_path / rel
        binary.parent.mkdir(parents=True, exist_ok=True)
        binary.write_text("#!/bin/sh\n")
        binary.chmod(0o755)
    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setenv("PATH", "/usr/bin:/bin")  # PATH de systemd user unit
    registry = providers.load_registry(ROOT / "config/providers.json")
    public = providers.public_state(registry)
    availability = {k: public["harnesses"][k]["available"] for k in ("claude", "codex", "grok")}
    assert availability == {"claude": True, "codex": True, "grok": True}
