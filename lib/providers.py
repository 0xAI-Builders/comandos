#!/usr/bin/env python3
"""Registro declarativo de harnesses y motores de ComandOS.

El JSON solo describe datos. Este módulo valida IDs/regex/capacidades y ofrece
lookups seguros; nunca ejecuta templates arbitrarios ni devuelve credenciales.
"""
from __future__ import annotations

import copy
import json
import os
import re
import shutil
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PATH = ROOT / "config" / "providers.json"
_ID = re.compile(r"^[a-z][a-z0-9._-]{0,39}$")

class ProviderRegistryError(ValueError):
    pass


def _expand(value: str | None) -> str:
    return os.path.expanduser(value or "")


# Los daemons (cc-dash bajo systemd) heredan un PATH mínimo sin los bin de
# usuario donde viven los CLIs (claude/grok en ~/.local/bin, codex en
# ~/.bun/bin). La detección de instalado/no-instalado no puede depender del
# entorno del proceso: which() busca en PATH y además en estos directorios.
_USER_BIN_DIRS = (
    "~/.local/bin",
    "~/.bun/bin",
    "~/.cargo/bin",
    "~/.npm-global/bin",
    "~/.opencode/bin",
    "~/bin",
    "/usr/local/bin",
)


def which(binary: str | None) -> str | None:
    """shutil.which + directorios estándar de binarios de usuario."""
    name = str(binary or "")
    if not name:
        return None
    hit = shutil.which(name)
    if hit:
        return hit
    for directory in _USER_BIN_DIRS:
        candidate = os.path.join(os.path.expanduser(directory), name)
        if os.path.isfile(candidate) and os.access(candidate, os.X_OK):
            return candidate
    return None


def _validate_models(section: str, ident: str, item: dict[str, Any]) -> None:
    for model in item.get("models") or []:
        if not isinstance(model, dict) or not str(model.get("id") or ""):
            raise ProviderRegistryError(f"bad model in {section}.{ident}")
        efforts = model.get("efforts") or []
        if len(set(efforts)) != len(efforts):
            raise ProviderRegistryError(f"duplicate effort in {ident}/{model.get('id')}")
        if model.get("defaultEffort") and model["defaultEffort"] not in efforts:
            raise ProviderRegistryError(f"default effort not allowed in {ident}/{model.get('id')}")


def validate_registry(data: dict[str, Any]) -> dict[str, Any]:
    version = data.get("version")
    if version not in (1, 2):
        raise ProviderRegistryError("providers.json version must be 1 or 2")
    for section in ("harnesses", "claudeEngines"):
        entries = data.get(section)
        if not isinstance(entries, dict) or not entries:
            raise ProviderRegistryError(f"{section} must be a non-empty object")
        for ident, item in entries.items():
            if not _ID.fullmatch(ident) or not isinstance(item, dict):
                raise ProviderRegistryError(f"invalid {section} id: {ident!r}")
            if not str(item.get("label") or "").strip():
                raise ProviderRegistryError(f"{section}.{ident} needs label")
            if section == "claudeEngines":
                try:
                    re.compile(str(item.get("modelMatch") or ""), re.I)
                except re.error as exc:
                    raise ProviderRegistryError(f"bad modelMatch for {ident}: {exc}") from exc
            _validate_models(section, ident, item)
    if version == 2:
        motors = data.get("motors")
        if not isinstance(motors, dict) or not motors:
            raise ProviderRegistryError("motors must be a non-empty object")
        for ident, item in motors.items():
            if ident not in data["harnesses"] or not isinstance(item, dict):
                raise ProviderRegistryError(f"invalid motor: {ident}")
            try:
                re.compile(str(item.get("modelMatch") or ""), re.I)
            except re.error as exc:
                raise ProviderRegistryError(f"bad motor regex {ident}: {exc}") from exc
            _validate_models("motors", ident, item)
        matrix = data.get("matrixHarnesses") or []
        if set(matrix) != {"claude", "codex", "grok"}:
            raise ProviderRegistryError("matrixHarnesses must cover claude/codex/grok")
        cells, route_ids = set(), set()
        scopes = {"new_session", "session_motor", "session_model"}
        for route in data.get("routes") or []:
            ident = route.get("id")
            cell = (route.get("harness"), route.get("motor"))
            if not _ID.fullmatch(str(ident or "")) and ":" not in str(ident or ""):
                raise ProviderRegistryError(f"bad route id: {ident}")
            if ident in route_ids or cell in cells:
                raise ProviderRegistryError(f"duplicate route/cell: {ident}")
            if cell[0] not in data["harnesses"] or cell[1] not in motors:
                raise ProviderRegistryError(f"bad route reference: {ident}")
            if not set(route.get("actionScopes") or []).issubset(scopes):
                raise ProviderRegistryError(f"bad route scope: {ident}")
            route_ids.add(ident); cells.add(cell)
        for exclusion in data.get("exclusions") or []:
            cell = (exclusion.get("harness"), exclusion.get("motor"))
            if cell in cells:
                raise ProviderRegistryError(f"excluded cell is routed: {cell}")
            cells.add(cell)
        expected = {(h, m) for h in matrix for m in matrix}
        if cells != expected:
            raise ProviderRegistryError(f"matrix coverage mismatch: missing={sorted(expected-cells)} extra={sorted(cells-expected)}")
    return data


def load_registry(path: str | os.PathLike[str] | None = None) -> dict[str, Any]:
    source = Path(path) if path else DEFAULT_PATH
    with source.open(encoding="utf-8") as handle:
        return validate_registry(json.load(handle))


def harness(registry: dict[str, Any], ident: str) -> dict[str, Any] | None:
    item = (registry.get("harnesses") or {}).get(ident)
    return copy.deepcopy(item) if item else None


def engine(registry: dict[str, Any], ident: str) -> dict[str, Any] | None:
    item = (registry.get("motors") or registry.get("claudeEngines") or {}).get(ident)
    return copy.deepcopy(item) if item else None


def engine_for_model(registry: dict[str, Any], model: str) -> str:
    value = str(model or "")
    for ident, item in (registry.get("motors") or registry.get("claudeEngines") or {}).items():
        if re.search(str(item.get("modelMatch") or r"$^"), value, re.I):
            return ident
    return ""


def _model_key(model_id: str) -> str:
    """Clave tolerante: los CLIs reportan variantes del mismo modelo
    (claude-fable-5, fable-5[1m], fable) — todas deben resolver al spec."""
    key = str(model_id or "").lower()
    key = re.sub(r"\[.*$", "", key)              # sufijo de contexto [1m]
    key = re.sub(r"-\d{8}$", "", key)            # fecha -20260901
    key = key.removeprefix("claude-")
    return key.rstrip("-")


def model_spec(registry: dict[str, Any], owner: str, model_id: str, *, section: str = "harnesses") -> dict[str, Any] | None:
    item = (registry.get(section) or {}).get(owner) or {}
    for model in item.get("models") or []:
        if model.get("id") == model_id:
            return copy.deepcopy(model)
    # Sin match exacto: match por clave normalizada. Un "cambiar esfuerzo"
    # con el id que reporta el CLI jamas debe morir en model_unavailable.
    want = _model_key(model_id)
    if want:
        for model in item.get("models") or []:
            if _model_key(model.get("id", "")) == want:
                return copy.deepcopy(model)
        # alias de familia ("fable", "opus"): como el CLI, resuelve al MAS
        # NUEVO de la familia (fable -> fable-5-1 aunque exista fable-5)
        def _ver(mid: str) -> tuple:
            nums = re.findall(r"\d+", _model_key(mid))
            return tuple(int(n) for n in nums)
        hits = [m for m in item.get("models") or []
                if _model_key(m.get("id", "")).startswith(want + "-")
                or _model_key(m.get("id", "")).split("-")[0] == want]
        if hits:
            return copy.deepcopy(max(hits, key=lambda m: _ver(m.get("id", ""))))
    return None


def effort_allowed(registry: dict[str, Any], owner: str, model_id: str, effort: str, *, section: str = "harnesses") -> bool:
    spec = model_spec(registry, owner, model_id, section=section)
    return bool(spec and effort in (spec.get("efforts") or []))


def route_for(registry: dict[str, Any], route_id: str) -> dict[str, Any] | None:
    return next((copy.deepcopy(r) for r in registry.get("routes") or [] if r.get("id") == route_id), None)


def model_spec_for_route(registry: dict[str, Any], route_id: str, model_id: str) -> dict[str, Any] | None:
    route = route_for(registry, route_id)
    return model_spec(registry, route.get("motor", ""), model_id, section="motors") if route else None


def evaluate_capability_matrix(registry: dict[str, Any], facts: dict[str, Any]) -> list[dict[str, Any]]:
    """Evaluate product routes against presence-only runtime facts."""
    out = []
    binaries = facts.get("harnesses") or {}
    motors = facts.get("motors") or {}
    gateway = facts.get("gateway") or {}
    for route in registry.get("routes") or []:
        cell = copy.deepcopy(route)
        reason = None
        hf = binaries.get(route["harness"], {})
        mf = motors.get(route["motor"], {})
        if not hf.get("available", False):
            reason = ("harness_binary_missing", f"{route['harness']} no está instalado")
        elif not hf.get("authenticated", False):
            reason = ("harness_login_missing", f"{route['harness']} no tiene login")
        elif any(req == "gateway" for req in route.get("authRequirements") or []) and not gateway.get("alive", False):
            reason = ("gateway_down", "cc-model-proxy no responde")
        elif any(req == f"motor:{route['motor']}" for req in route.get("authRequirements") or []) and not mf.get("authenticated", False):
            reason = ("motor_login_missing", f"{route['motor']} no tiene login")
        elif route.get("experimental") and not route.get("liveVerified", False):
            reason = (route.get("reasonCode") or "route_not_live_verified", "puente pendiente de verificación E2E")
        cell["productState"] = "supported"
        cell["runtimeState"] = "available" if reason is None else "unavailable"
        cell["selectable"] = reason is None
        cell["reason"] = None if reason is None else {"code": reason[0], "message": reason[1]}
        out.append(cell)
    for exclusion in registry.get("exclusions") or []:
        cell = copy.deepcopy(exclusion)
        cell.update(runtimeState="unavailable", selectable=False, actionScopes=[])
        cell["reason"] = {"code": cell.get("reasonCode"), "message": cell.get("message")}
        out.append(cell)
    order = {name: i for i, name in enumerate(registry.get("matrixHarnesses") or [])}
    return sorted(out, key=lambda x: (order.get(x.get("harness"), 99), order.get(x.get("motor"), 99)))


def allowed_routes(matrix: list[dict[str, Any]], scope: str, current_harness: str | None = None) -> list[dict[str, Any]]:
    return [copy.deepcopy(r) for r in matrix if r.get("selectable") and scope in (r.get("actionScopes") or []) and (not current_harness or r.get("harness") == current_harness)]


def validate_selection(registry: dict[str, Any], matrix: list[dict[str, Any]], selection: dict[str, Any], scope: str) -> dict[str, Any]:
    route_id = str(selection.get("routeId") or "")
    cell = next((r for r in matrix if r.get("id") == route_id), None)
    if not cell or not cell.get("selectable") or scope not in (cell.get("actionScopes") or []):
        raise ProviderRegistryError((cell or {}).get("reason", {}).get("code") or "route_scope_not_supported")
    model_id = str(selection.get("model") or "")
    spec = model_spec_for_route(registry, route_id, model_id)
    if not spec:
        raise ProviderRegistryError("model_unavailable")
    effort = str(selection.get("effort") or "")
    if effort and effort not in (spec.get("efforts") or []):
        raise ProviderRegistryError("effort_unavailable")
    return copy.deepcopy(cell)


def public_state(registry: dict[str, Any]) -> dict[str, Any]:
    """Return public runtime state. No auth document or secret values."""
    out = copy.deepcopy(registry)
    for ident, item in (out.get("harnesses") or {}).items():
        binary = item.get("binary")
        item["available"] = binary is None or which(binary) is not None
        home = _expand(item.get("defaultHome")) if item.get("defaultHome") else ""
        auth = item.get("authFile")
        item["authenticated"] = bool(auth and home and os.path.isfile(os.path.join(home, auth))) if auth else None
        item.pop("authFile", None)
        item.pop("defaultHome", None)
        item.pop("accountsRoot", None)
        item.pop("accountEnv", None)
    for item in (out.get("claudeEngines") or {}).values():
        item.pop("authSource", None)
    return out
