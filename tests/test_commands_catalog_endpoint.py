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
