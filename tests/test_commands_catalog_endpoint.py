"""GET /commands/catalog: per-CLI catalog + the CLI detected in the pane."""
from dash_harness import dash  # noqa: F401  (pytest fixture)


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


def test_catalog_payload_runs_versions_once_under_concurrency(monkeypatch, tmp_path):
    import threading
    import time
    mod = _load_cc_dash(monkeypatch, tmp_path)
    calls = []

    def slow_versions(catalog, *a, **k):
        calls.append(1)
        time.sleep(0.3)  # long enough that every thread queues behind the first
        return {c["id"]: "0.0.0" for c in catalog["clis"]}

    monkeypatch.setattr(mod.cli_catalog, "installed_versions", slow_versions)
    monkeypatch.setitem(mod._CLI_CATALOG, "catalog", None)
    monkeypatch.setitem(mod._CLI_CATALOG, "snapshot", ({}, 0.0))
    results = []
    threads = [threading.Thread(target=lambda: results.append(mod.cli_catalog_payload())) for _ in range(6)]
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=20)
    assert len(results) == 6
    assert len(calls) == 1
    assert len({at for _view, at in results}) == 1
    assert mod.cli_catalog_payload(refresh=True) and len(calls) == 2
