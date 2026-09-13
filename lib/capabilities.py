#!/usr/bin/env python3
"""Sanitized per-harness capability inventory.

Only capability names, descriptions and provenance leave this module. Commands, URLs,
environment values, arguments and tool payloads are never returned.
"""
from __future__ import annotations

import json
import ast
import os
import re
from pathlib import Path
from typing import Any

from accounts import account_home
from mcp_descriptions import metadata as description_metadata

_SECTION_RE = re.compile(r'''^\s*\[\s*mcp_servers\s*\.\s*("(?:\\.|[^"\\])*"|'[^']*'|[A-Za-z0-9_-]+)\s*\]\s*(?:\#.*)?$''')
_SAFE_NAME_RE = re.compile(r"^[^\x00-\x1f]{1,160}$")


def _json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
        return value if isinstance(value, dict) else {}
    except Exception:
        return {}


def _clean_name(value: str) -> str:
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
        value = value[1:-1]
    value = value.replace('\\"', '"').strip()
    return value if _SAFE_NAME_RE.fullmatch(value) else ""


def _toml_mcps(path: Path, provider: str, scope: str, source: str) -> list[dict[str, Any]]:
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return []
    output = []
    current = None
    enabled = True
    description = None
    def append_current():
        if current:
            output.append(_entry(current, provider, scope, source, enabled, description))
    index = 0
    while index < len(lines):
        line = lines[index]
        index += 1
        match = _SECTION_RE.match(line)
        if match:
            append_current()
            current = _clean_name(match.group(1))
            enabled = True
            description = None
            continue
        if current and re.match(r"^\s*(enabled\s*=\s*false|disabled\s*=\s*true)\b", line, re.I):
            enabled = False
        elif current and re.match(r"^\s*\[", line):
            append_current()
            current = None
        # Read only a declared public description. Consume multiline string
        # values so their contents cannot manufacture another server section.
        field = re.match(r'^\s*([A-Za-z0-9_-]+)\s*=\s*(.*)$', line)
        if field:
            value = field.group(2).strip()
            delimiter = value[:3]
            if delimiter in ('"""', "'''"):
                while delimiter not in value[3:] and index < len(lines):
                    value += '\n' + lines[index]
                    index += 1
                end = value.find(delimiter, 3)
                value = value[:end + 3] if end >= 0 else ''
            if current and field.group(1) == 'description':
                try:
                    description = ast.literal_eval(value)
                except (ValueError, SyntaxError):
                    description = None
    append_current()
    return output


def _json_mcps(path: Path, provider: str, scope: str, source: str, disabled=()) -> list[dict[str, Any]]:
    servers = _json(path).get("mcpServers")
    if not isinstance(servers, dict):
        return []
    disabled = set(str(x) for x in disabled)
    return [_entry(str(name), provider, scope, source, str(name) not in disabled,
                   spec.get('description') if isinstance(spec, dict) else None)
            for name, spec in sorted(servers.items()) if _SAFE_NAME_RE.fullmatch(str(name))]


def _entry(name: str, provider: str, scope: str, source: str, enabled: bool, description=None) -> dict[str, Any]:
    return {
        "name": name,
        "provider": provider,
        "scope": scope,
        "source": source,
        "sources": [source],
        "enabled": bool(enabled),
        "status": "configured" if enabled else "disabled",
        "confidence": "configured",
        **description_metadata(name, description),
    }


def _parents_to_root(cwd: str) -> list[Path]:
    if not cwd or not os.path.isabs(cwd):
        return []
    current = Path(cwd).resolve()
    output = []
    while True:
        output.append(current)
        if (current / ".git").exists() or current.parent == current:
            break
        current = current.parent
    output.reverse()
    return output


def _plugin_mcps(root: Path, provider: str, scope: str, source: str) -> list[dict[str, Any]]:
    if not root.is_dir():
        return []
    output = []
    try:
        manifests = list(root.glob("*/.mcp.json")) + list(root.glob("*/*/.mcp.json"))
    except OSError:
        return []
    for manifest in manifests[:500]:
        output.extend(_json_mcps(manifest, provider, scope, source))
    return output


def _merge(entries: list[dict[str, Any]]) -> list[dict[str, Any]]:
    merged: dict[str, dict[str, Any]] = {}
    for item in entries:
        key = item["name"].casefold()
        previous = merged.get(key)
        if not previous:
            merged[key] = dict(item)
            continue
        previous["enabled"] = previous["enabled"] or item["enabled"]
        previous["status"] = "configured" if previous["enabled"] else "disabled"
        for source in item["sources"]:
            if source not in previous["sources"]:
                previous["sources"].append(source)
        if previous["scope"] != item["scope"]:
            previous["scope"] = "mixed"
        if item.get('descriptionSource') == 'configuration':
            previous.update(description=item['description'], descriptionSource='configuration')
    return sorted(merged.values(), key=lambda item: item["name"].casefold())


def session_capabilities(registry: dict[str, Any], harness: str, alias: str, cwd: str) -> dict[str, Any]:
    """Return an effective, privacy-safe capability inventory for one pane."""
    try:
        home = account_home(registry, harness, alias)
    except Exception:
        return {"harness": harness, "account": alias, "mcps": [], "status": "unknown", "confidence": "unknown"}

    entries: list[dict[str, Any]] = []
    parents = _parents_to_root(cwd)
    project = Path(cwd).resolve() if cwd and os.path.isabs(cwd) else None

    if harness == "claude":
        settings = {}
        if project:
            for filename in ("settings.json", "settings.local.json"):
                settings.update(_json(project / ".claude" / filename))
        disabled = settings.get("disabledMcpjsonServers") if isinstance(settings.get("disabledMcpjsonServers"), list) else []
        global_json = home / ".claude.json"
        if not global_json.exists():
            global_json = home.parent / ".claude.json"
        entries.extend(_json_mcps(global_json, harness, "user", "claude-user"))
        if project:
            entries.extend(_json_mcps(project / ".mcp.json", harness, "project", "mcp-json", disabled))
    elif harness == "codex":
        entries.extend(_toml_mcps(home / "config.toml", harness, "user", "codex-user"))
        for parent in parents:
            if (parent / '.codex' / 'config.toml').resolve() == (home / 'config.toml').resolve():
                continue
            entries.extend(_toml_mcps(parent / ".codex" / "config.toml", harness, "project", "codex-project"))
    elif harness == "grok":
        entries.extend(_toml_mcps(home / "config.toml", harness, "user", "grok-user"))
        for parent in parents:
            if (parent / '.grok' / 'config.toml').resolve() == (home / 'config.toml').resolve():
                continue
            entries.extend(_toml_mcps(parent / ".grok" / "config.toml", harness, "project", "grok-project"))
        # Grok Build intentionally loads Claude-compatible MCP declarations too.
        claude_spec = (registry.get("harnesses") or {}).get("claude") or {}
        claude_home = Path(os.path.expanduser(str(claude_spec.get("defaultHome") or "~/.claude"))).resolve()
        claude_json = claude_home.parent / ".claude.json"
        entries.extend(_json_mcps(claude_json, harness, "compatible", "claude-compatible"))
        if project:
            entries.extend(_json_mcps(project / ".mcp.json", harness, "project", "mcp-json"))
            entries.extend(_plugin_mcps(project / ".grok" / "plugins", harness, "project", "grok-plugin"))
        entries.extend(_plugin_mcps(home / "plugins", harness, "user", "grok-plugin"))

    mcps = _merge(entries)
    return {
        "harness": harness,
        "account": alias,
        "mcps": mcps,
        "status": "configured" if mcps else "empty",
        "confidence": "configured",
    }
