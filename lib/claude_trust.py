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


def _ancestors_until_git(cwd: str, home: str) -> list[str]:
    """Return cwd and its ancestors up to and including the git toplevel.

    Stops at the first ancestor with a `.git` entry — a directory in a normal
    clone, but a plain FILE in a linked worktree, so this checks existence
    rather than isdir — or at `home`/filesystem root, whichever comes first.
    """
    out, cur = [], os.path.abspath(cwd)
    home = os.path.abspath(home)
    while True:
        out.append(cur)
        if os.path.exists(os.path.join(cur, ".git")) or cur in (home, os.path.dirname(cur)):
            return out
        cur = os.path.dirname(cur)


def _trusted_in_file(path: str, cwd: str, home: str) -> bool:
    try:
        with open(path) as f:
            projects = (json.load(f) or {}).get("projects") or {}
    except Exception:
        return False
    return any(
        bool((projects.get(p) or {}).get("hasTrustDialogAccepted"))
        for p in _ancestors_until_git(cwd, home)
    )


def cwd_trusted_in(cwd: str, *, config_dir: str | None, home: str) -> bool:
    """True if hasTrustDialogAccepted is set for cwd (or a git ancestor) in HOME or config_dir."""
    files = [os.path.join(home, ".claude.json")]
    if config_dir:
        files.append(os.path.join(config_dir, ".claude.json"))
    return any(_trusted_in_file(p, cwd, home) for p in files)


def inherit_cwd_trust(cwd: str, *, source_config_dir: str | None, dest_config_dir: str, home: str) -> bool:
    """Copy cwd's trust acceptance from a source account's config to a destination account's.

    Never invents trust: only stamps dest when the source (HOME or
    source_config_dir) already accepted it. Never persists trust for HOME.
    """
    if os.path.abspath(cwd) == os.path.abspath(home):
        return False
    if not cwd_trusted_in(cwd, config_dir=source_config_dir, home=home):
        return False
    ensure_cwd_trusted(cwd, config_dir=dest_config_dir, home=home)
    return True
