#!/usr/bin/env python3
"""Pre-accept Claude Code's workspace trust dialog.

Claude 2.1.263 shows "Accessing workspace / Yes, I trust this folder" unless
`projects[<cwd>].hasTrustDialogAccepted` is true in **~/.claude.json** (HOME).
It does NOT read `{CLAUDE_CONFIG_DIR}/.claude.json` for that flag, and it only
walks ancestors until the git toplevel — so a trusted `~/codebase` does not
cover `~/codebase/0xJesus/ServerMacMini`.

Home itself is session-only: Claude refuses to persist trust for `$HOME`.
"""
from __future__ import annotations

import fcntl
import json
import os


def ensure_cwd_trusted(cwd: str, *, config_dir: str | None = None, home: str | None = None) -> bool:
    """Stamp hasTrustDialogAccepted on HOME ~/.claude.json (and optional config-dir copy).

    Returns True if the HOME file now has the flag for this cwd.
    Never persists trust for $HOME itself. Never raises.
    """
    cwd = os.path.expanduser(str(cwd or ""))
    if not cwd.startswith("/") or not os.path.isdir(cwd):
        return False
    try:
        cwd = os.path.realpath(cwd)
        home_root = os.path.realpath(os.path.expanduser(home if home is not None else "~"))
    except Exception:
        return False
    if cwd == home_root:
        return False
    home_json = os.path.join(home_root, ".claude.json")
    ok = _stamp(home_json, cwd)
    if config_dir:
        try:
            cfg_json = os.path.join(os.path.realpath(os.path.expanduser(config_dir)), ".claude.json")
        except Exception:
            cfg_json = ""
        if cfg_json and os.path.realpath(cfg_json) != os.path.realpath(home_json):
            _stamp(cfg_json, cwd)
    return ok


def _stamp(path: str, cwd: str) -> bool:
    directory = os.path.dirname(path)
    try:
        if directory:
            os.makedirs(directory, exist_ok=True)
    except Exception:
        return False
    lock_path = path + ".lock"
    try:
        with open(lock_path, "a+") as lock:
            fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
            try:
                return _stamp_locked(path, cwd)
            finally:
                fcntl.flock(lock.fileno(), fcntl.LOCK_UN)
    except Exception:
        return False


def _stamp_locked(path: str, cwd: str) -> bool:
    try:
        with open(path) as handle:
            data = json.load(handle)
        if not isinstance(data, dict):
            return False
    except FileNotFoundError:
        data = {}
    except Exception:
        return False
    projects = data.get("projects")
    if not isinstance(projects, dict):
        projects = {}
        data["projects"] = projects
    entry = projects.get(cwd)
    if not isinstance(entry, dict):
        entry = {}
    if entry.get("hasTrustDialogAccepted") is True:
        projects[cwd] = entry
        return True
    entry["hasTrustDialogAccepted"] = True
    projects[cwd] = entry
    tmp = path + ".tmp"
    with open(tmp, "w") as handle:
        json.dump(data, handle, indent=2)
        handle.write("\n")
    os.replace(tmp, path)
    return True
