import json, subprocess
from pathlib import Path
import pytest

ROOT = Path(__file__).resolve().parents[1]
import sys; sys.path.insert(0, str(ROOT / "lib"))
import cli_catalog

VERSIONS = {"claude": "2.1.268", "codex": "0.154.0", "grok": "1.0.25", "opencode": "1.17.18", "agy": "1.1.25"}

def test_catalog_file_is_each_cli_menu_verbatim():
    """El catálogo es el menú `/` de cada CLI tal cual (tools/cli-commands/scraped):
    una lista plana, sin categorías ni descripciones propias, a la versión capturada."""
    sys.path.insert(0, str(ROOT / "tools" / "cli-commands"))
    import build
    cat = cli_catalog.load_catalog()
    roles = json.loads((ROOT / "config" / "agent-roles.json").read_text())["dangerFlags"]
    ids = [c["id"] for c in cat["clis"]]
    assert ids == ["claude", "codex", "grok", "opencode", "agy"]
    assert set(ids) <= set(roles)
    for cli in cat["clis"]:
        src = json.loads((ROOT / "tools" / "cli-commands" / "scraped" / f"{cli['id']}.json").read_text())
        menu = {}
        for name, desc in src["commands"]:
            name = build.ALIASES.get((cli["id"], name), name)
            if name:
                menu[name] = build.clean(desc)
        assert cli["pinnedVersion"] == src["version"], cli["id"]
        assert len(cli["groups"]) == 1 and cli["groups"][0]["title"] == "", cli["id"]
        cmds = cli["groups"][0]["commands"]
        assert [c["text"].strip()[1:] for c in cmds] == sorted(menu), cli["id"]
        for cmd in cmds:
            assert cmd["description"] == menu[cmd["text"].strip()[1:]], (cli["id"], cmd["text"])
    assert build.build(json.loads(cli_catalog.CATALOG_FILE.read_text())) == cat


import cli_help

def _helps():
    """Las cinco ayudas reales (`<cli> --help`) guardadas en tests/fixtures/cli-help."""
    return {c: dict(cli_help.parse_help((ROOT / "tests" / "fixtures" / "cli-help" / f"{c}.txt").read_text(), c),
                    command=f"{c} --help") for c in VERSIONS}

def _start(view, cid):
    return next(c for c in view["clis"] if c["id"] == cid)["start"]

def test_start_rows_come_verbatim_from_each_cli_help():
    view = cli_catalog.catalog_view(cli_catalog.load_catalog(), versions=VERSIONS, accounts={}, helps=_helps())
    codex = _start(view, "codex")
    assert codex["command"] == "codex --help"
    assert codex["rows"] == [{"text": "codex", "description": "Codex CLI", "args": []}]
    # el yolo es lo que el propio CLI describe como saltarse confirmaciones/permisos
    assert [y["text"] for y in codex["yolo"]] == ["codex --dangerously-bypass-approvals-and-sandbox"]
    assert codex["yolo"][0]["description"].startswith("Skip all confirmation prompts and execute commands without sandboxing.")
    opts = next(s for s in codex["sections"] if s["title"] == "Options")["items"]
    assert "codex --full-auto" not in [i["text"].strip() for i in opts]        # ya no existe en 0.159.2
    sandbox = next(i for i in opts if i["text"] == "codex --sandbox ")
    assert sandbox["args"] == ["read-only", "workspace-write", "danger-full-access"]
    assert next(i for i in opts if i["text"] == "codex --ask-for-approval ")["args"] == ["on-request", "never"]
    cmds = next(s for s in codex["sections"] if s["title"] == "Commands")["items"]
    assert any(i["text"] == "codex resume" and i["description"].startswith("Resume a previous interactive session") for i in cmds)
    assert [y["text"] for y in _start(view, "claude")["yolo"]] == [
        "claude --allow-dangerously-skip-permissions", "claude --dangerously-skip-permissions"]
    assert [y["text"] for y in _start(view, "grok")["yolo"]] == ["grok --always-approve"]
    oc = _start(view, "opencode")
    assert [y["text"] for y in oc["yolo"]] == ["opencode --auto"]
    assert oc["rows"][0] == {"text": "opencode", "description": "start opencode tui [default]", "args": []}
    agy = _start(view, "agy")
    assert [y["text"] for y in agy["yolo"]] == ["agy --dangerously-skip-permissions"]
    assert [s["title"] for s in agy["sections"]] == ["Options", "Available subcommands"]
    assert next(i for i in agy["sections"][0]["items"] if i["text"] == "agy --effort ")["args"] == ["low", "medium", "high", "max"]
    grok = next(s for s in _start(view, "grok")["sections"] if s["title"] == "Options")["items"]
    assert next(i for i in grok if i["text"] == "grok --output-format ")["args"] == ["plain", "json", "streaming-json", "streaming-messages-json"]

def test_without_help_only_the_binary_row_is_offered():
    view = cli_catalog.catalog_view(cli_catalog.load_catalog(), versions=VERSIONS, accounts={})
    assert _start(view, "codex") == {"command": "codex --help", "rows": [{"text": "codex", "description": "", "args": []}],
                                     "yolo": [], "sections": []}

def test_account_rows_use_config_dir_env():
    accounts = {"claude": [{"alias": "relotto", "env": {"CLAUDE_CONFIG_DIR": "/home/u/.claude-accounts/relotto"}}]}
    view = cli_catalog.catalog_view(cli_catalog.load_catalog(), versions=VERSIONS, accounts=accounts, helps=_helps())
    assert [r["text"] for r in _start(view, "claude")["rows"]] == ["claude", "CLAUDE_CONFIG_DIR=/home/u/.claude-accounts/relotto claude"]

def test_help_for_caches_by_binary_and_tolerates_failures(tmp_path):
    exe = tmp_path / "codex"; exe.write_text("#!/bin/sh\n"); exe.chmod(0o755)
    calls = []
    def run(cmd, **kw):
        calls.append(cmd)
        return subprocess.CompletedProcess(cmd, 0, stdout=(ROOT / "tests" / "fixtures" / "cli-help" / "codex.txt").read_text(), stderr="")
    cli_help._HELP_CACHE.clear()
    h = cli_help.help_for("codex", which=lambda b: str(exe), run=run)
    assert h["command"] == "codex --help" and h["summary"] == "Codex CLI"
    assert cli_help.help_for("codex", which=lambda b: str(exe), run=run) is h and calls == [[str(exe), "--help"]]
    def boom(cmd, **kw): raise subprocess.TimeoutExpired(cmd, 1)
    assert cli_help.help_for("grok", which=lambda b: None, run=boom) is None
    cli_help._HELP_CACHE.clear()
    assert cli_help.help_for("codex", which=lambda b: str(exe), run=boom) is None

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
    view = cli_catalog.catalog_view(cat, versions=VERSIONS, accounts={}, helps=_helps())
    for cli in view["clis"]:
        st = cli["start"]
        for row in st["rows"] + st["yolo"] + [i for sec in st["sections"] for i in sec["items"]]:
            assert "\n" not in row["text"] and "\r" not in row["text"]


def _cmd(view, cli, text):
    c = next(c for c in view["clis"] if c["id"] == cli)
    return next(cmd for g in c["groups"] for cmd in g["commands"] if cmd["text"] == text)

def test_argsfrom_models_uses_watcher_models_and_marks_new():
    cat = cli_catalog.load_catalog()
    models = {"claude": ["claude-opus-5-5", "claude-sonnet-5-5"], "grok": ["grok-4.7"], "codex": ["gpt-6.1-sol"]}
    new = {"claude": ["claude-sonnet-5-5", "claude-haiku-4-5"], "grok": []}
    view = cli_catalog.catalog_view(cat, versions=VERSIONS, accounts={},
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
        view = cli_catalog.catalog_view(cat, versions=VERSIONS, accounts={}, **kw)
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
    view = cli_catalog.catalog_view(cat, versions=VERSIONS, accounts={}, detected=det)
    claude = next(c for c in view["clis"] if c["id"] == "claude")
    names = [c["text"].split()[0] for g in claude["groups"] for c in g["commands"]]
    assert names == ["/model", "/compact"] or sorted(names) == ["/compact", "/model"]
    assert claude["detected"] == {"found": 2, "total": len(names) + claude["detectedMissing"]}
    codex = next(c for c in view["clis"] if c["id"] == "codex")
    assert codex["detected"] is None                             # sin binario legible: catálogo completo
    assert sum(len(g["commands"]) for g in codex["groups"]) == sum(
        len(g.get("commands", [])) for g in next(c for c in cat["clis"] if c["id"] == "codex")["groups"])
    # sin `detected` no cambia nada
    full = cli_catalog.catalog_view(cat, versions=VERSIONS, accounts={})
    assert "detected" not in next(c for c in full["clis"] if c["id"] == "claude")
