#!/usr/bin/env python3
"""Private account discovery for subscription-backed agent CLIs.

Public helpers return aliases and display identity only. Paths and auth payloads
remain inside this module.
"""
from __future__ import annotations

import base64
import json
import os
import re
from pathlib import Path
from typing import Any

_ALIAS_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")


class AccountError(ValueError):
    pass


def _spec(registry: dict[str, Any], provider: str) -> dict[str, Any]:
    spec = (registry.get("harnesses") or {}).get(provider)
    if not isinstance(spec, dict) or not (spec.get("capabilities") or {}).get("accounts"):
        raise AccountError(f"{provider}: cuentas no soportadas")
    for key in ("defaultHome", "accountsRoot", "authFile", "accountEnv"):
        if not spec.get(key):
            raise AccountError(f"{provider}: registro de cuentas incompleto")
    return spec


def validate_alias(alias: str) -> str:
    alias = str(alias or "main")
    if alias != "main" and (not _ALIAS_RE.fullmatch(alias) or alias.startswith((".", "-"))):
        raise AccountError("alias de cuenta invalido")
    return alias


def account_home(registry: dict[str, Any], provider: str, alias: str = "main") -> Path:
    spec = _spec(registry, provider)
    alias = validate_alias(alias)
    if alias == "main":
        return Path(os.path.expanduser(spec["defaultHome"])).resolve()
    root = Path(os.path.expanduser(spec["accountsRoot"])).resolve()
    candidate = root / alias
    # resolve(strict=False) still resolves existing symlink parents. A named
    # account may not exist yet (login flow), but it may never escape its root.
    resolved = candidate.resolve(strict=False)
    try:
        resolved.relative_to(root)
    except ValueError as exc:
        raise AccountError("cuenta fuera de accountsRoot") from exc
    if candidate.is_symlink():
        raise AccountError("aliases de cuenta no pueden ser symlinks")
    return resolved


def account_environment(registry: dict[str, Any], provider: str, alias: str) -> dict[str, str]:
    spec = _spec(registry, provider)
    return {str(spec["accountEnv"]): str(account_home(registry, provider, alias))}


def _json(path: Path) -> dict[str, Any]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        return data if isinstance(data, dict) else {}
    except Exception:
        return {}


def _jwt_email(token: Any) -> str:
    if not isinstance(token, str) or token.count(".") < 2:
        return ""
    try:
        part = token.split(".")[1]
        part += "=" * (-len(part) % 4)
        payload = json.loads(base64.urlsafe_b64decode(part))
        return str(payload.get("email") or "") if isinstance(payload, dict) else ""
    except Exception:
        return ""


def _auth(provider: str, home: Path, auth_file: str) -> tuple[bool, str, str]:
    data = _json(home / auth_file)
    identity = ""
    subscription = False
    if provider == "claude":
        oauth = data.get("claudeAiOauth") if isinstance(data.get("claudeAiOauth"), dict) else {}
        subscription = bool(oauth.get("accessToken") or oauth.get("refreshToken"))
        meta = _json(home / ".claude.json")
        if not meta:
            meta = _json(home.parent / ".claude.json")
        account = meta.get("oauthAccount") if isinstance(meta.get("oauthAccount"), dict) else {}
        identity = str(account.get("emailAddress") or "")
    elif provider == "codex":
        tokens = data.get("tokens") if isinstance(data.get("tokens"), dict) else {}
        subscription = bool(tokens.get("access_token") or tokens.get("refresh_token"))
        identity = _jwt_email(tokens.get("id_token"))
    elif provider == "grok":
        record = next((value for value in data.values() if isinstance(value, dict)), {})
        subscription = bool(record.get("refresh_token"))
        identity = str(record.get("email") or record.get("first_name") or record.get("user_id") or record.get("principal_id") or "")
    if subscription:
        return True, identity, "ready"
    if data:
        return False, identity, "unsupported_auth"
    return False, "", "login_required"


def list_accounts(registry: dict[str, Any], provider: str) -> list[dict[str, Any]]:
    spec = _spec(registry, provider)
    aliases = ["main"]
    root = Path(os.path.expanduser(spec["accountsRoot"]))
    try:
        aliases.extend(sorted(
            item.name for item in root.iterdir()
            if item.is_dir() and not item.is_symlink() and _ALIAS_RE.fullmatch(item.name)
            and not item.name.startswith((".", "-"))
        ))
    except OSError:
        pass
    output = []
    for alias in aliases:
        try:
            home = account_home(registry, provider, alias)
            authenticated, identity, state = _auth(provider, home, str(spec["authFile"]))
        except AccountError:
            continue
        output.append({
            "provider": provider,
            "alias": alias,
            "identity": identity,
            "authenticated": authenticated,
            "state": state,
            "selectable": authenticated,
        })
    return output


def public_accounts(registry: dict[str, Any]) -> dict[str, list[dict[str, Any]]]:
    output = {}
    for provider, spec in (registry.get("harnesses") or {}).items():
        if (spec.get("capabilities") or {}).get("accounts"):
            try:
                output[provider] = list_accounts(registry, provider)
            except AccountError:
                output[provider] = []
    return output
