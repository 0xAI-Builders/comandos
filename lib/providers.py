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


def validate_registry(data: dict[str, Any]) -> dict[str, Any]:
    if data.get("version") != 1:
        raise ProviderRegistryError("providers.json version must be 1")
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
            for model in item.get("models") or []:
                if not isinstance(model, dict) or not str(model.get("id") or ""):
                    raise ProviderRegistryError(f"bad model in {section}.{ident}")
                efforts = model.get("efforts") or []
                if len(set(efforts)) != len(efforts):
                    raise ProviderRegistryError(f"duplicate effort in {ident}/{model.get('id')}")
                if model.get("defaultEffort") and model["defaultEffort"] not in efforts:
                    raise ProviderRegistryError(f"default effort not allowed in {ident}/{model.get('id')}")
    return data


def load_registry(path: str | os.PathLike[str] | None = None) -> dict[str, Any]:
    source = Path(path) if path else DEFAULT_PATH
    with source.open(encoding="utf-8") as handle:
        return validate_registry(json.load(handle))


def harness(registry: dict[str, Any], ident: str) -> dict[str, Any] | None:
    item = (registry.get("harnesses") or {}).get(ident)
    return copy.deepcopy(item) if item else None


def engine(registry: dict[str, Any], ident: str) -> dict[str, Any] | None:
    item = (registry.get("claudeEngines") or {}).get(ident)
    return copy.deepcopy(item) if item else None


def engine_for_model(registry: dict[str, Any], model: str) -> str:
    value = str(model or "")
    for ident, item in (registry.get("claudeEngines") or {}).items():
        if re.search(str(item.get("modelMatch") or r"$^"), value, re.I):
            return ident
    return ""


def model_spec(registry: dict[str, Any], owner: str, model_id: str, *, section: str = "harnesses") -> dict[str, Any] | None:
    item = (registry.get(section) or {}).get(owner) or {}
    for model in item.get("models") or []:
        if model.get("id") == model_id:
            return copy.deepcopy(model)
    return None


def effort_allowed(registry: dict[str, Any], owner: str, model_id: str, effort: str, *, section: str = "harnesses") -> bool:
    spec = model_spec(registry, owner, model_id, section=section)
    return bool(spec and effort in (spec.get("efforts") or []))


def public_state(registry: dict[str, Any]) -> dict[str, Any]:
    """Return public runtime state. No auth document or secret values."""
    out = copy.deepcopy(registry)
    for ident, item in (out.get("harnesses") or {}).items():
        binary = item.get("binary")
        item["available"] = binary is None or shutil.which(str(binary)) is not None
        item["home"] = _expand(item.get("defaultHome")) if item.get("defaultHome") else ""
        auth = item.get("authFile")
        item["authenticated"] = bool(auth and item.get("home") and os.path.isfile(os.path.join(item["home"], auth))) if auth else None
        item.pop("authFile", None)
        item.pop("defaultHome", None)
        item.pop("accountsRoot", None)
        item.pop("accountEnv", None)
    for item in (out.get("claudeEngines") or {}).values():
        item.pop("authSource", None)
    return out
