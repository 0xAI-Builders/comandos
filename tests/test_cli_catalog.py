import json, subprocess
from pathlib import Path
import pytest

ROOT = Path(__file__).resolve().parents[1]
import sys; sys.path.insert(0, str(ROOT / "lib"))
import cli_catalog

DANGER = {"claude": "--dangerously-skip-permissions", "codex": "--dangerously-bypass-approvals-and-sandbox",
          "opencode": "", "grok": "--always-approve", "agy": "--dangerously-skip-permissions"}
VERSIONS = {"claude": "2.1.268", "codex": "0.154.0", "grok": "1.0.25", "opencode": "1.17.18", "agy": "1.1.25"}

def test_catalog_file_matches_repo_danger_flags_and_map_versions():
    cat = cli_catalog.load_catalog()
    roles = json.loads((ROOT / "config" / "agent-roles.json").read_text())["dangerFlags"]
    ids = [c["id"] for c in cat["clis"]]
    assert ids == ["claude", "codex", "grok", "opencode", "agy"]
    assert set(ids) <= set(roles)
    text = (ROOT / "docs" / "tui-command-map.md").read_text()
    for cli in cat["clis"]:
        assert cli["pinnedVersion"] in text, cli["id"]
        for grp in cli["groups"]:
            for cmd in grp["commands"]:
                assert cmd["text"].strip() in text, (cli["id"], cmd["text"])
                assert "\n" not in cmd["text"] and cmd["description"]

def test_yolo_launches_come_first_and_use_danger_flag():
    cat = cli_catalog.load_catalog()
    view = cli_catalog.catalog_view(cat, danger_flags=DANGER, versions=VERSIONS, accounts={})
    claude = next(c for c in view["clis"] if c["id"] == "claude")
    assert claude["launch"]["yolo"][:2] == [
        "claude --dangerously-skip-permissions",
        "claude --dangerously-skip-permissions --resume <id>"]
    assert claude["launch"]["normal"][:2] == ["claude", "claude --resume <id>"]
    codex = next(c for c in view["clis"] if c["id"] == "codex")
    assert codex["launch"]["yolo"][1] == "codex --dangerously-bypass-approvals-and-sandbox resume <id>"
    opencode = next(c for c in view["clis"] if c["id"] == "opencode")
    assert opencode["launch"]["yolo"] == [] and "sin flag" in opencode["launch"]["yoloNote"]

def test_account_launches_use_config_dir_env():
    cat = cli_catalog.load_catalog()
    accounts = {"claude": [{"alias": "relotto", "env": {"CLAUDE_CONFIG_DIR": "/home/u/.claude-accounts/relotto"}}]}
    view = cli_catalog.catalog_view(cat, danger_flags=DANGER, versions=VERSIONS, accounts=accounts)
    claude = next(c for c in view["clis"] if c["id"] == "claude")
    assert "CLAUDE_CONFIG_DIR=/home/u/.claude-accounts/relotto claude --dangerously-skip-permissions" in claude["launch"]["normal"]

@pytest.mark.parametrize("pinned,installed,verified,expected", [
    ("2.1.268", "2.1.268", True, "ok"), ("2.1.268", "2.1.270", True, "drift"),
    ("2.1.268", None, True, "missing"), ("1.1.25", "1.1.25", False, "unverified")])
def test_version_status(pinned, installed, verified, expected):
    assert cli_catalog.version_status(pinned, installed, verified) == expected

def test_installed_versions_runs_resolved_path_and_tolerates_failures():
    cat = cli_catalog.load_catalog()
    calls = []
    def which(b): return {"claude": "/opt/claude", "codex": "/opt/codex"}.get(b)
    def run(cmd, **kw):
        calls.append(cmd)
        if cmd[0] == "/opt/codex": raise subprocess.TimeoutExpired(cmd, 1)
        return subprocess.CompletedProcess(cmd, 0, stdout="2.1.268 (Claude Code)\n", stderr="")
    versions = cli_catalog.installed_versions(cat, which=which, run=run)
    assert versions == {"claude": "2.1.268", "codex": "?", "grok": None, "opencode": None, "agy": None}
    assert calls[0] == ["/opt/claude", "--version"]

def test_view_never_contains_enter_or_newlines():
    cat = cli_catalog.load_catalog()
    view = cli_catalog.catalog_view(cat, danger_flags=DANGER, versions=VERSIONS, accounts={})
    for cli in view["clis"]:
        for text in cli["launch"]["yolo"] + cli["launch"]["normal"]:
            assert "\n" not in text and "\r" not in text


def _cmd(view, cli, text):
    c = next(c for c in view["clis"] if c["id"] == cli)
    return next(cmd for g in c["groups"] for cmd in g["commands"] if cmd["text"] == text)

def test_argsfrom_models_uses_watcher_models_and_marks_new():
    cat = cli_catalog.load_catalog()
    models = {"claude": ["claude-opus-5-5", "claude-sonnet-5-5"], "grok": ["grok-4.7"], "codex": ["gpt-6.1-sol"]}
    new = {"claude": ["claude-sonnet-5-5", "claude-haiku-4-5"], "grok": []}
    view = cli_catalog.catalog_view(cat, danger_flags=DANGER, versions=VERSIONS, accounts={},
                                    models=models, new_models=new)
    cm = _cmd(view, "claude", "/model ")
    assert cm["args"] == ["claude-opus-5-5", "claude-sonnet-5-5"]
    assert cm["newArgs"] == ["claude-sonnet-5-5"]          # interseccion con newSince
    gm = _cmd(view, "grok", "/model ")
    assert gm["args"] == ["grok-4.7"] and "newArgs" not in gm
    # codex /model abre un selector sin argumentos: no toma modelos
    assert "args" in _cmd(view, "codex", "/model") and _cmd(view, "codex", "/model")["args"] == []
    # otros comandos con args fijos no cambian
    assert _cmd(view, "claude", "/effort ")["args"] == ["low", "medium", "high", "max"]

def test_argsfrom_models_keeps_json_args_without_watcher_data():
    cat = cli_catalog.load_catalog()
    for kw in ({}, {"models": {}}, {"models": {"claude": [], "grok": []}, "new_models": {}}):
        view = cli_catalog.catalog_view(cat, danger_flags=DANGER, versions=VERSIONS, accounts={}, **kw)
        assert _cmd(view, "claude", "/model ")["args"] == ["claude-fable-5-1", "claude-opus-5-5", "claude-sonnet-5-5"]
        assert _cmd(view, "grok", "/model ")["args"] == ["grok-4.6", "grok-4.5"]
        assert "newArgs" not in _cmd(view, "claude", "/model ")

def test_model_commands_are_marked_argsfrom_models_in_the_json():
    cat = cli_catalog.load_catalog()
    for cid in ("claude", "grok"):
        cmd = next(c for g in next(x for x in cat["clis"] if x["id"] == cid)["groups"]
                   for c in g["commands"] if c["text"] == "/model ")
        assert cmd["argsFrom"] == "models" and cmd["args"]
    codex = next(x for x in cat["clis"] if x["id"] == "codex")
    assert all("argsFrom" not in c for g in codex["groups"] for c in g["commands"])


# ---- comandos detectados de verdad en el binario instalado ----

def _elf(*names):
    return b"\x7fELF" + b"\0" * 20 + b" ".join(n.encode() for n in names) + b"\0"


def test_native_binary_resolves_node_wrapper_to_platform_package(tmp_path):
    pkg = tmp_path / "node_modules" / "@openai"
    (pkg / "codex" / "bin").mkdir(parents=True)
    wrapper = pkg / "codex" / "bin" / "codex.js"
    wrapper.write_bytes(b"#!/usr/bin/env node\nconsole.log(1)\n")
    native = pkg / "codex-linux-x64" / "vendor" / "x86_64-unknown-linux-musl" / "bin" / "codex"
    native.parent.mkdir(parents=True)
    native.write_bytes(_elf("/model"))
    assert cli_catalog.native_binary(str(wrapper)) == str(native)
    elf = tmp_path / "claude"
    elf.write_bytes(_elf("/model"))
    assert cli_catalog.native_binary(str(elf)) == str(elf)
    assert cli_catalog.native_binary("") is None
    assert cli_catalog.native_binary(str(tmp_path / "missing")) is None


def test_detected_commands_reads_slash_names_from_each_binary(tmp_path):
    cat = cli_catalog.load_catalog()
    claude = tmp_path / "claude"
    claude.write_bytes(_elf("/model", "/compact", "/effort"))
    which = lambda b: str(claude) if b == "claude" else ""
    det = cli_catalog.detected_commands(cat, which=which)
    assert det["claude"] == {"/model", "/compact", "/effort"}
    assert det["codex"] is None                                  # no instalado: no se afirma nada
    # cache por (ruta, mtime, tamaño): el mismo archivo no se relee
    det2 = cli_catalog.detected_commands(cat, which=which)
    assert det2["claude"] == det["claude"]


def test_view_only_keeps_detected_commands_and_is_flat_when_asked():
    cat = cli_catalog.load_catalog()
    det = {"claude": {"/model", "/compact"}, "codex": None}
    view = cli_catalog.catalog_view(cat, danger_flags=DANGER, versions=VERSIONS, accounts={}, detected=det)
    claude = next(c for c in view["clis"] if c["id"] == "claude")
    names = [c["text"].split()[0] for g in claude["groups"] for c in g["commands"]]
    assert names == ["/model", "/compact"] or sorted(names) == ["/compact", "/model"]
    assert claude["detected"] == {"found": 2, "total": len(names) + claude["detectedMissing"]}
    codex = next(c for c in view["clis"] if c["id"] == "codex")
    assert codex["detected"] is None                             # sin binario legible: catálogo completo
    assert sum(len(g["commands"]) for g in codex["groups"]) == sum(
        len(g.get("commands", [])) for g in next(c for c in cat["clis"] if c["id"] == "codex")["groups"])
    # sin `detected` no cambia nada
    full = cli_catalog.catalog_view(cat, danger_flags=DANGER, versions=VERSIONS, accounts={})
    assert "detected" not in next(c for c in full["clis"] if c["id"] == "claude")
