#!/usr/bin/env python3
import copy
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import providers


ALL_HARNESSES = ("claude", "codex", "grok", "acp", "opencode", "agy")
ALL_MOTORS = ("claude", "codex", "grok", "opencode", "agy")
CORE_LIVE = {"claude:claude", "claude:codex", "claude:grok", "codex:codex", "grok:grok"}
ACP_LIVE = {"acp:claude", "acp:codex", "acp:grok", "acp:opencode", "acp:agy"}
NATIVE_EXTRA = {"opencode:opencode", "agy:agy"}


def facts(**overrides):
    value = {
        "harnesses": {x: {"available": True, "authenticated": True} for x in ALL_HARNESSES},
        "motors": {x: {"authenticated": True} for x in ALL_MOTORS},
        "gateway": {"installed": True, "alive": True},
    }
    value.update(overrides)
    return value


def test_matrix_covers_exactly_nine_cells_and_five_are_live_subscription_routes():
    registry = providers.load_registry(ROOT / "config/providers.json")
    matrix = providers.evaluate_capability_matrix(registry, facts())
    # nucleo 3x3 intacto + filas acp/opencode/agy: la matriz cubre TODO
    # harness x motor, y cada celda dice por que si o por que no
    assert {(x["harness"], x["motor"]) for x in matrix} == {
        (h, m) for h in ALL_HARNESSES for m in ALL_MOTORS
    }
    assert {x["id"] for x in matrix if x["selectable"]} == CORE_LIVE | ACP_LIVE | NATIVE_EXTRA


def test_unavailable_cells_are_explanatory_and_never_selectable():
    registry = providers.load_registry(ROOT / "config/providers.json")
    matrix = providers.evaluate_capability_matrix(registry, facts())
    reasons = {x["id"]: x["reason"]["code"] for x in matrix if not x["selectable"]}
    core = {k: v for k, v in reasons.items() if k.split(":")[0] in ("claude", "codex", "grok") and k.split(":")[1] in ("claude", "codex", "grok")}
    assert core == {
        "codex:claude": "unsupported_protocol",
        "codex:grok": "route_not_live_verified",
        "grok:claude": "subscription_not_accepted",
        "grok:codex": "route_not_live_verified",
    }
    # fuera del nucleo, toda celda sin ruta se sintetiza como not_routed (visible, no seleccionable)
    assert {v for k, v in reasons.items() if k not in core} == {"not_routed"}
    assert all(x["reason"]["message"] for x in matrix if not x["selectable"])


def test_gateway_down_disables_only_external_claude_routes():
    registry = providers.load_registry(ROOT / "config/providers.json")
    runtime = facts(gateway={"installed": True, "alive": False})
    matrix = providers.evaluate_capability_matrix(registry, runtime)
    selected = {x["id"] for x in matrix if x["selectable"]}
    # sin gateway sobreviven las nativas y TODAS las ACP (no pasan por cc-proxy)
    assert selected == {"claude:claude", "codex:codex", "grok:grok"} | ACP_LIVE | NATIVE_EXTRA
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


def test_acp_routes_need_only_the_motor_login_never_the_gateway():
    """Las rutas ACP corren el agente del vendor con SU suscripcion: sin
    cc-proxy y sin API keys. Que caiga el gateway no las apaga."""
    registry = providers.load_registry(ROOT / "config/providers.json")
    runtime = facts(gateway={"installed": False, "alive": False})
    matrix = providers.evaluate_capability_matrix(registry, runtime)
    assert ACP_LIVE.issubset({x["id"] for x in matrix if x["selectable"]})
    runtime = facts(motors={**{x: {"authenticated": True} for x in ALL_MOTORS}, "codex": {"authenticated": False}})
    matrix = providers.evaluate_capability_matrix(registry, runtime)
    assert {x["reason"]["code"] for x in matrix if x["id"] == "acp:codex"} == {"motor_login_missing"}
    assert "acp:claude" in {x["id"] for x in matrix if x["selectable"]}


def test_acp_agents_are_declared_for_every_acp_route():
    registry = providers.load_registry(ROOT / "config/providers.json")
    agents = registry["acpAgents"]
    for route in registry["routes"]:
        if route["harness"] == "acp":
            assert route["motor"] in agents
            assert agents[route["motor"]]["command"]
