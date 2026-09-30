import pytest

from dash_harness import dash  # noqa: F401  (pytest fixture)

STEPS = [{"kind": "shell", "text": "claude --dangerously-skip-permissions"}, {"kind": "pane", "text": "/effort max"}]


@pytest.fixture
def dash_env(tmp_path):
    return {"XDG_CONFIG_HOME": str(tmp_path / "cfg")}


def test_chain_crud_round_trip(dash, tmp_path):
    saved = dash.post("/chains", {"name": "Claude yolo máximo", "steps": STEPS})
    assert saved["ok"] is True
    assert saved["chain"]["slug"] == "claude-yolo-maximo"
    assert (tmp_path / "cfg" / "comandos" / "cadenas" / "claude-yolo-maximo.md").exists()
    assert dash.get("/chains")["chains"][0]["steps"] == STEPS
    assert dash.post("/chains/delete", {"slug": "claude-yolo-maximo"})["deleted"] is True
    assert dash.get("/chains")["chains"] == []


def test_chain_delete_missing_reports_false(dash):
    assert dash.post("/chains/delete", {"slug": "no-existe"})["deleted"] is False


def test_chain_save_rejects_bad_steps_with_400(dash):
    assert dash.post_status("/chains", {"name": "x", "steps": [{"kind": "pane", "text": "a\nb"}]}) == 400


def test_chain_delete_rejects_invalid_slug_with_400(dash):
    assert dash.post_status("/chains/delete", {"slug": "../etc/passwd"}) == 400


def test_chain_routes_require_token_for_remote_peers(dash):
    assert dash.get_status("/chains", token=False) == 401
    assert dash.post_status("/chains", {"name": "x", "steps": STEPS}, token=False) == 401
    assert dash.post_status("/chains/delete", {"slug": "x"}, token=False) == 401


def test_chain_save_with_lone_surrogate_returns_500_json_and_leaves_no_tmp(dash, tmp_path):
    status = dash.post_status("/chains", {"name": "a\ud800b", "steps": STEPS})
    assert status == 500
    chains_dir = tmp_path / "cfg" / "comandos" / "cadenas"
    assert list(chains_dir.glob("*.tmp")) == []
    assert dash.get("/chains")["chains"] == []  # el servidor sigue vivo
