"""Los cinco endpoints de /allocation, contra el motor y el lote reales."""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
from test_agent_launch import load_dash_module
from test_allocation import ACCOUNTS, LIMITS, NOW, REGISTRY, SUPPORT, TIERS, sessions


def _wire(dash, monkeypatch, tmp_path, live=None):
    monkeypatch.setattr(dash, "read_states_cached", lambda *a, **k: [dict(s, alive=True) for s in (live or sessions())])
    monkeypatch.setattr(dash, "usage_provider_limits", lambda *a, **k: {"limits": LIMITS, "health": {}})
    monkeypatch.setattr(dash, "load_provider_registry", lambda: REGISTRY)
    monkeypatch.setattr(dash, "session_change_support", lambda *a, **k: SUPPORT)
    monkeypatch.setattr(dash, "load_model_tiers", lambda: TIERS)
    monkeypatch.setattr(dash, "selectable_accounts_by_motor", lambda registry: ACCOUNTS)
    monkeypatch.setattr(dash.time, "time", lambda: NOW)
    monkeypatch.setattr(dash, "_ALLOCATION_BATCHES", dash.allocation_batch.BatchStore(tmp_path / "b.json"))
    dash.ALLOCATION_PLANS.clear()
    llamadas = []
    monkeypatch.setattr(dash, "session_configure",
                        lambda d: (llamadas.append(d), (202, {"ok": True, "pending": True,
                                                              "operationKey": f"{d['session']}|{d['pane']}",
                                                              "operationId": "op"}))[1])
    monkeypatch.setattr(dash, "session_operation_status",
                        lambda opkey, opid='', **k: {"state": "confirmed", "ok": True})
    return llamadas


def test_propose_devuelve_un_plan_utilizable(tmp_path, monkeypatch):
    dash = load_dash_module(); _wire(dash, monkeypatch, tmp_path)
    code, body = dash.allocation_propose({})
    assert code == 200 and body["ok"]
    plan = body["plan"]
    assert plan["items"] and plan["planId"] in dash.ALLOCATION_PLANS
    assert plan["options"]["claude"]["models"], "la ficha necesita los modelos válidos"
    assert "low" in plan["options"]["claude"]["efforts"]
    assert all("status" in i for i in plan["items"]), "la ficha pinta el estado de la sesión"
    assert all(i["to"]["routeId"] != "acp:claude" for i in plan["items"])


def test_propose_es_determinista(tmp_path, monkeypatch):
    dash = load_dash_module(); _wire(dash, monkeypatch, tmp_path)
    uno = dash.allocation_propose({})[1]["plan"]
    otro = dash.allocation_propose({})[1]["plan"]
    assert uno["planId"] == otro["planId"] and uno["items"] == otro["items"]


def test_overrides_invalidos_dan_400_no_500(tmp_path, monkeypatch):
    dash = load_dash_module(); _wire(dash, monkeypatch, tmp_path)
    assert dash.allocation_propose({"overrides": {"SAVA|%10": {"layer": "alta"}}})[0] == 400
    assert dash.allocation_propose({"overrides": {"SAVA|%10": {"to": "claude"}}})[0] == 400
    assert dash.allocation_propose({"overrides": "nada"})[0] == 200


def test_preview_recalcula_sin_guardar(tmp_path, monkeypatch):
    dash = load_dash_module(); _wire(dash, monkeypatch, tmp_path)
    pid = dash.allocation_propose({})[1]["plan"]["planId"]
    code, prev = dash.allocation_preview({"planId": pid, "key": "LifeOS|%18", "target": {"layer": 3}})
    assert code == 200 and prev["impact"] and len(dash.ALLOCATION_PLANS) == 1
    assert dash.allocation_preview({"planId": "noexiste", "key": "x", "target": {}})[0] == 404


def test_apply_lanza_el_lote_y_status_lo_cuenta(tmp_path, monkeypatch):
    dash = load_dash_module(); llamadas = _wire(dash, monkeypatch, tmp_path)
    pid = dash.allocation_propose({})[1]["plan"]["planId"]
    code, res = dash.allocation_apply({"planId": pid}, wait=True)
    assert code == 202 and res["batchId"]
    code, st = dash.allocation_status(res["batchId"])
    assert code == 200 and st["state"] == "terminado"
    assert all(i["state"] in ("lista", "omitida") for i in st["items"])
    assert llamadas and all(c["interrupt"] is True for c in llamadas)
    assert all(c["requestId"].startswith(res["batchId"] + ":") for c in llamadas)


def test_apply_rechaza_un_plan_caducado(tmp_path, monkeypatch):
    dash = load_dash_module(); _wire(dash, monkeypatch, tmp_path)
    pid = dash.allocation_propose({})[1]["plan"]["planId"]
    movidas = [dict(s, alive=True) for s in sessions()]
    movidas[0]["effort"] = "low"
    monkeypatch.setattr(dash, "read_states_cached", lambda *a, **k: movidas)
    code, res = dash.allocation_apply({"planId": pid}, wait=True)
    assert code == 409 and res["error"] == "plan_stale"
    assert any(movidas[0]["session"] in c for c in res["changed"])


def test_status_y_retry_de_un_lote_desconocido(tmp_path, monkeypatch):
    dash = load_dash_module(); _wire(dash, monkeypatch, tmp_path)
    assert dash.allocation_status("noexiste")[0] == 404
    assert dash.allocation_retry({"batchId": "noexiste", "key": "x"})[0] == 404
    assert dash.allocation_revert({"batchId": "noexiste"})[0] == 404


def test_revert_deshace_solo_lo_confirmado(tmp_path, monkeypatch):
    dash = load_dash_module(); llamadas = _wire(dash, monkeypatch, tmp_path)
    pid = dash.allocation_propose({})[1]["plan"]["planId"]
    res = dash.allocation_apply({"planId": pid}, wait=True)[1]
    ida = len(llamadas)
    code, rev = dash.allocation_revert({"batchId": res["batchId"]}, wait=True)
    assert code == 202
    vuelta = llamadas[ida:]
    assert vuelta, "el revert no llamó al coordinador"
    origen = {f"{c['session']}|{c['pane']}": c for c in llamadas[:ida]}
    for c in vuelta:
        antes = origen[f"{c['session']}|{c['pane']}"]
        assert (c["model"], c["effort"], c["harnessAccount"]) != (antes["model"], antes["effort"], antes["harnessAccount"])


def test_limits_salen_enriquecidos(tmp_path, monkeypatch):
    dash = load_dash_module(); _wire(dash, monkeypatch, tmp_path)
    filas = dash.enriched_limits(LIMITS)
    semanal = next(f for f in filas if f["window"] == "7d")
    assert semanal["burn"] and semanal["verdict"]
    assert semanal["verdict"].startswith(("llega al reset", "se acaba en"))
