"""GET /commands/catalog: per-CLI catalog + the CLI detected in the pane."""
import json

import pytest

from dash_harness import dash  # noqa: F401  (pytest fixture)

SNAPSHOT = {
    "checkedAt": 1790783243, "heartbeatAt": 1790783250,
    "versions": {"claude": "9.9.9", "grok": "1.0.44"},   # sin codex/opencode/agy
    "discovered": {"claude": ["claude-opus-5-5", "claude-opus-5-5[1m]", "claude-opus-4-8",
                              "claude-sonnet-4-5-20250929", "claude-sonnet-5-5", "claude-fable-5-1"],
                   "codex": [], "grok": ["grok-4.7", "grok-4.7-build-fast"]},
    "newSince": {"claude": {"models": ["claude-sonnet-5-5"], "at": 1790783243, "cli": "9.9.9"}},
}


@pytest.fixture
def dash_env(tmp_path):
    hooks = tmp_path / ".claude" / "hooks"
    hooks.mkdir(parents=True, exist_ok=True)
    (hooks / "model-watch.json").write_text(json.dumps(SNAPSHOT))
    return {}


def test_catalog_route_reports_pane_cli_and_versions(dash):
    body = dash.get("/commands/catalog?session=demo&pane=%1")
    assert body["target"] == {"session": "demo", "pane": "%1"}
    assert body["cliInPane"] in ("", "claude", "codex", "grok", "opencode", "agy")
    ids = [c["id"] for c in body["catalog"]["clis"]]
    assert ids == ["claude", "codex", "grok", "opencode", "agy"]
    for cli in body["catalog"]["clis"]:
        assert cli["version"]["status"] in ("ok", "drift", "missing", "unverified")
    again = dash.get("/commands/catalog?session=demo&pane=%1")
    assert again["versionsAt"] == body["versionsAt"]  # cache: --version is not re-run


def _cmd(body, cli, text):
    c = next(c for c in body["catalog"]["clis"] if c["id"] == cli)
    return c, next(cmd for g in c["groups"] for cmd in g["commands"] if cmd["text"] == text)


def test_catalog_route_takes_versions_and_model_chips_from_the_watcher_snapshot(dash):
    body = dash.get("/commands/catalog?session=demo&pane=%1")
    claude, model = _cmd(body, "claude", "/model ")
    # 9.9.9 no existe en la maquina: solo sale del snapshot, nunca de `--version`
    assert claude["version"]["installed"] == "9.9.9" and claude["version"]["status"] == "drift"
    assert model["args"] == ["claude-opus-5-5", "claude-sonnet-5-5", "claude-fable-5-1"]
    assert model["newArgs"] == ["claude-sonnet-5-5"]
    grok, gmodel = _cmd(body, "grok", "/model ")
    assert grok["version"]["installed"] == "1.0.44"
    assert gmodel["args"] == ["grok-4.7"]
    # el snapshot no lista codex: sin binario conocido -> missing (el watcher ya lo resolvio)
    codex, _m = _cmd(body, "codex", "/model")
    assert codex["version"]["installed"] is None and codex["version"]["status"] == "missing"
    assert body["versionsAt"] == SNAPSHOT["checkedAt"]


def test_catalog_route_requires_token(dash):
    assert dash.get_status("/commands/catalog", token=False) == 401


def _load_cc_dash(monkeypatch, tmp_path):
    import importlib.machinery
    import importlib.util
    import sys
    from pathlib import Path
    root = Path(__file__).resolve().parents[1]
    monkeypatch.setenv("HOME", str(tmp_path))
    for sub in ("bin", "lib"):
        if str(root / sub) not in sys.path:
            monkeypatch.syspath_prepend(str(root / sub))
    loader = importlib.machinery.SourceFileLoader("cc_dash_catalog_under_test", str(root / "bin" / "cc-dash"))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


def _write_snapshot(mod, tmp_path, **over):
    snap = json.loads(json.dumps(SNAPSHOT))
    snap.update(over)
    hooks = tmp_path / ".claude" / "hooks"
    hooks.mkdir(parents=True, exist_ok=True)
    (hooks / "model-watch.json").write_text(json.dumps(snap))
    return snap


def _fresh(mod):
    mod._CLI_CATALOG.update({"catalog": None, "derived": None, "fallback": None})


def test_catalog_cache_is_invalidated_when_checked_at_changes(monkeypatch, tmp_path):
    mod = _load_cc_dash(monkeypatch, tmp_path)
    monkeypatch.setattr(mod.cli_catalog, "installed_versions",
                        lambda *a, **k: pytest.fail("el catalogo no ejecuta --version"))
    _fresh(mod)
    _write_snapshot(mod, tmp_path)
    calls = []
    real = mod.model_watch_lib.latest_models
    monkeypatch.setattr(mod.model_watch_lib, "latest_models", lambda *a, **k: calls.append(1) or real(*a, **k))
    v1, at1 = mod.cli_catalog_payload()
    v2, at2 = mod.cli_catalog_payload()
    assert at1 == at2 == SNAPSHOT["checkedAt"] and len(calls) == 1      # cache hit
    _write_snapshot(mod, tmp_path, checkedAt=SNAPSHOT["checkedAt"] + 600,
                    versions={"claude": "9.9.10"})
    v3, at3 = mod.cli_catalog_payload()
    assert len(calls) == 2 and at3 == SNAPSHOT["checkedAt"] + 600
    claude = next(c for c in v3["clis"] if c["id"] == "claude")
    assert claude["version"]["installed"] == "9.9.10"


def test_catalog_falls_back_to_one_version_probe_only_without_a_snapshot(monkeypatch, tmp_path):
    mod = _load_cc_dash(monkeypatch, tmp_path)
    probes = []
    monkeypatch.setattr(mod.cli_catalog, "installed_versions",
                        lambda catalog, *a, **k: probes.append(1) or {c["id"]: "0.0.1" for c in catalog["clis"]})
    _fresh(mod)
    view, _at = mod.cli_catalog_payload()
    mod.cli_catalog_payload()
    assert len(probes) == 1                       # cacheado hasta que el watcher escriba
    assert all(c["version"]["installed"] == "0.0.1" for c in view["clis"])
    # con snapshot ya no se ejecuta nada
    _write_snapshot(mod, tmp_path)
    mod.cli_catalog_payload()
    assert len(probes) == 1


def test_refresh_forces_one_watcher_cycle_under_concurrency(monkeypatch, tmp_path):
    import threading
    import time
    mod = _load_cc_dash(monkeypatch, tmp_path)
    _fresh(mod)
    _write_snapshot(mod, tmp_path, checkedAt=1)
    cycles = []

    def slow_cycle(force=False):
        cycles.append(force)
        time.sleep(0.3)   # every other thread queues behind the first
        _write_snapshot(mod, tmp_path, checkedAt=int(time.time()), versions={"claude": "7.7.7"})

    monkeypatch.setattr(mod, "_model_watch_cycle", slow_cycle)
    results = []
    threads = [threading.Thread(target=lambda: results.append(mod.cli_catalog_payload(refresh=True))) for _ in range(6)]
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=20)
    assert len(results) == 6
    assert cycles == [True]                       # una sola vuelta forzada, sin pisarse
    assert len({at for _v, at in results}) == 1
    for view, _at in results:
        assert next(c for c in view["clis"] if c["id"] == "claude")["version"]["installed"] == "7.7.7"
