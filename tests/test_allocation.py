import json
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
import allocation as al

NOW = 1_800_000_000.0
WEEK = 7 * 24 * 3600

def limit(provider, account, percent, resets_in, window="7d", **extra):
    return dict(id=f"{provider}:{account}:{window}", provider=provider, account=account,
                percent=percent, resets_at=NOW + resets_in, window=window, **extra)

def test_fmt_duration():
    assert al.fmt_duration(45 * 60) == "45 min"
    assert al.fmt_duration(7 * 3600) == "7 h"
    assert al.fmt_duration(46 * 3600) == "1d 22h"

def test_enrich_pace_and_burn_week():
    # 63 % usado con 85.5 h para el reset: transcurrido 49 % de la semana → ritmo 1.29x
    rows = al.enrich_limits([limit("claude", "main", 63.0, 85.5 * 3600)], NOW)
    r = rows[0]
    assert round(r["pace"], 1) == 49.1
    assert round(r["burn"], 2) == 1.28
    assert r["reachesReset"] is False
    assert r["verdict"].startswith("se acaba en ")
    assert 0 < r["runsOutIn"] < 85.5 * 3600

def test_enrich_reaches_reset():
    r = al.enrich_limits([limit("codex", "main", 1.0, 130 * 3600)], NOW)[0]
    assert r["reachesReset"] is True
    assert r["runsOutIn"] is None
    assert r["verdict"] == "llega al reset"
    assert r["burn"] < 0.2

def test_enrich_unknown_window_has_no_projection():
    r = al.enrich_limits([limit("grok", "main", 40.0, 3600, window="")], NOW)[0]
    assert r["burn"] is None and r["verdict"] == "" and r["reachesReset"] is None

def test_enrich_does_not_mutate_input():
    src = [limit("claude", "main", 10.0, 3600, window="5h")]
    al.enrich_limits(src, NOW)
    assert "burn" not in src[0]

REGISTRY = {"motors": {
    "claude": {"models": [{"id": "claude-fable-5-1", "efforts": ["low", "medium", "high", "xhigh", "max"]},
                          {"id": "claude-opus-5", "efforts": ["low", "medium", "high", "max"]},
                          {"id": "claude-sonnet-5", "efforts": ["low", "medium", "high"]}]},
    "codex": {"models": [{"id": "gpt-6-astra", "efforts": ["low", "medium", "high", "xhigh"]},
                         {"id": "gpt-5.6-luna", "efforts": ["low", "medium", "high"]}]},
    "grok": {"models": [{"id": "grok-4.6", "efforts": ["low", "medium", "high"]}]}}}
TIERS = {"patterns": [{"match": "grok-4\\.(6|5)", "tier": "high"}, {"match": "fable|mythos|opus", "tier": "high"},
                      {"match": "gpt-6|codex-max", "tier": "high"},
                      {"match": "sonnet|gpt-5\\.6|terra|sol|luna", "tier": "mid"}, {"match": "haiku|mini|spark|nano", "tier": "low"}]}

def test_layer_from_effort_and_status():
    assert al.layer_of({"effort": "xhigh", "status": "waiting"}) == 3
    assert al.layer_of({"effort": "high", "status": "working"}) == 3
    assert al.layer_of({"effort": "medium", "status": "waiting"}) == 2
    assert al.layer_of({"effort": "low", "status": "waiting"}) == 1
    assert al.layer_of({"effort": "max", "status": "idle"}) == 2      # idle baja una capa
    assert al.layer_of({"effort": "low", "status": "done"}) == 1      # nunca baja de 1
    assert al.layer_of({"effort": "", "status": "waiting"}) == 2      # desconocido → normal

def test_model_for_layer_uses_tiers():
    assert al.model_for_layer("claude", 3, REGISTRY, TIERS) == "claude-fable-5-1"
    assert al.model_for_layer("claude", 2, REGISTRY, TIERS) == "claude-sonnet-5"
    assert al.model_for_layer("claude", 1, REGISTRY, TIERS) == "claude-sonnet-5"   # sin tier low → mid
    assert al.model_for_layer("codex", 1, REGISTRY, TIERS) == "gpt-5.6-luna"
    assert al.model_for_layer("grok", 1, REGISTRY, TIERS) == "grok-4.6"           # único modelo

def test_governing_limit_prefers_weekly():
    rows = [limit("claude", "main", 30, 3600, window="5h", kind="session"),
            limit("claude", "main", 39, 85 * 3600, window="7d", kind="weekly_all"),
            limit("claude", "main", 63, 85 * 3600, window="7d", kind="weekly_scoped")]
    assert al.governing_limit(rows, "claude", "main")["percent"] == 39
    assert al.governing_limit(rows, "codex", "main") is None
    assert al.pool_key("claude", "relotto") == "claude:relotto"

REGISTRY["routes"] = [{"id": "claude:claude", "harness": "claude", "motor": "claude"},
                      {"id": "claude:codex", "harness": "claude", "motor": "codex"},
                      {"id": "codex:codex", "harness": "codex", "motor": "codex"},
                      {"id": "grok:grok", "harness": "grok", "motor": "grok"},
                      {"id": "acp:claude", "harness": "acp", "motor": "claude"}]
SUPPORT = {"claude:claude": {"selectable": True, "reason": None},
           "claude:codex": {"selectable": True, "reason": None},
           "codex:codex": {"selectable": True, "reason": None},
           "grok:grok": {"selectable": True, "reason": None},
           "acp:claude": {"selectable": False, "reason": {"code": "acp_effort_unobserved"}}}
ACCOUNTS = {"claude": ["main", "relotto"], "codex": ["main"], "grok": ["main"]}

def sess(name, agent, model, effort, account="main", status="waiting", pane="%1"):
    return dict(session=name, pane=pane, agent=agent, motor=agent, model=model, effort=effort,
                account=account, harnessAccount=account, motorAccount=account,
                routeId=f"{agent}:{agent}", status=status, cwd="/tmp/" + name)

LIMITS = [limit("claude", "main", 63.0, 85.5 * 3600, kind="weekly_all"),
          limit("claude", "relotto", 65.0, 76.5 * 3600, kind="weekly_all"),
          limit("codex", "main", 1.0, 130.6 * 3600),
          limit("grok", "main", 6.0, 98.4 * 3600)]

def sessions():
    return [sess("SAVA", "claude", "claude-fable-5-1", "xhigh", pane="%10"),
            sess("MRP", "claude", "claude-opus-5", "max", pane="%12"),
            sess("LifeOS", "claude", "claude-opus-5", "medium", pane="%18"),
            sess("Chips", "grok", "grok-4.6", "high", status="idle", pane="%60"),
            sess("CleanWix", "codex", "gpt-6-astra", "high", status="done", pane="%20")]

def run(**kw):
    return al.propose(sessions(), LIMITS, REGISTRY, SUPPORT, ACCOUNTS, TIERS, now=NOW, **kw)

def test_tier_of_takes_the_first_matching_pattern():
    # Un modelo que casa con DOS patrones se queda con el tier del primero, no con el más barato.
    tiers = {"patterns": [{"match": "gpt-6", "tier": "high"}, {"match": "gpt-6|luna", "tier": "low"}]}
    assert al._tier_of("gpt-6-astra", tiers) == "high"
    assert al._tier_of("gpt-6-luna", tiers) == "high"
    assert al._tier_of("otro-luna", tiers) == "low"

def test_plan_is_deterministic():
    first = json.dumps(run(), sort_keys=True)
    for _ in range(100):
        assert json.dumps(run(), sort_keys=True) == first

def test_plan_shape_and_hash():
    plan = run()
    assert plan["stateHash"] == al.plan_hash(sessions())
    assert {i["key"] for i in plan["items"]} == {"SAVA|%10", "MRP|%12", "LifeOS|%18", "Chips|%60", "CleanWix|%20"}
    item = next(i for i in plan["items"] if i["session"] == "SAVA")
    assert item["layer"] == 3 and item["to"]["effort"] == "high"
    assert item["to"]["model"] == "claude-fable-5-1"
    assert item["risk"] in ("bajo", "medio", "alto", "-") and item["reason"]

def test_strong_sessions_spread_to_account_with_more_margin():
    plan = run()
    strong = [i for i in plan["items"] if i["layer"] == 3]
    accounts = {i["to"]["harnessAccount"] for i in strong}
    assert accounts == {"main", "relotto"}          # no todas en la misma cuenta

def test_medium_layer_moves_to_codex_when_codex_is_empty():
    plan = run()
    lifeos = next(i for i in plan["items"] if i["session"] == "LifeOS")
    assert lifeos["to"]["motor"] == "codex" and lifeos["to"]["routeId"] == "claude:codex"
    assert lifeos["to"]["motorAccount"] == "main"   # gateway fuerza main
    assert lifeos["to"]["effort"] == "medium"

def test_override_layer_and_lock():
    plan = run(overrides={"LifeOS|%18": {"layer": 3}, "Chips|%60": {"locked": True}})
    lifeos = next(i for i in plan["items"] if i["session"] == "LifeOS")
    chips = next(i for i in plan["items"] if i["session"] == "Chips")
    assert lifeos["layer"] == 3 and lifeos["to"]["effort"] == "high"
    assert chips["locked"] and chips["same"]

def test_override_to_is_respected_verbatim():
    plan = run(overrides={"SAVA|%10": {"to": {"harness": "claude", "motor": "claude", "model": "claude-opus-5",
                                              "effort": "medium", "harnessAccount": "main", "motorAccount": "main",
                                              "routeId": "claude:claude"}}})
    sava = next(i for i in plan["items"] if i["session"] == "SAVA")
    assert sava["to"]["model"] == "claude-opus-5" and sava["to"]["effort"] == "medium"

def test_unselectable_routes_never_proposed():
    plan = run()
    assert all(i["to"]["routeId"] != "acp:claude" for i in plan["items"])

def test_hard_constraint_prefers_pool_that_reaches_reset():
    tight = [limit("claude", "main", 90.0, 85 * 3600, kind="weekly_all"),
             limit("claude", "relotto", 20.0, 85 * 3600, kind="weekly_all"),
             limit("codex", "main", 1.0, 130 * 3600), limit("grok", "main", 6.0, 98 * 3600)]
    plan = al.propose(sessions(), tight, REGISTRY, SUPPORT, ACCOUNTS, TIERS, now=NOW)
    strong = [i for i in plan["items"] if i["layer"] == 3]
    assert all(i["to"]["harnessAccount"] == "relotto" for i in strong)
    assert plan["impact"]["claude:relotto"]["reachesReset"] is True

def test_impact_has_before_and_after_per_pool():
    plan = run()
    imp = plan["impact"]["codex:main"]
    assert imp["n"] >= 1 and imp["burnAfter"] > imp["burnNow"]
    assert imp["verdict"] in ("llega al reset",) or imp["verdict"].startswith("se acaba en")

def test_invert_sends_every_item_back_to_its_from():
    plan = run()
    inv = al.invert(plan)
    assert inv["planId"] == "rev-" + plan["planId"] and inv is not plan
    for before, after in zip(plan["items"], inv["items"]):
        assert after["to"]["model"] == before["from"]["model"]
        assert after["to"]["effort"] == before["from"]["effort"]
        assert after["to"]["harnessAccount"] == before["from"]["account"]
        assert after["reason"] == "revertir"
    assert plan["items"][0]["to"]["model"]      # el plan original no se toca

def test_layer_three_spreads_across_its_own_accounts_not_to_a_cheaper_motor():
    # Codex está al 1 % y siempre sería la cuota más barata; la capa 3 igual se queda
    # en su motor y solo se reparte entre las cuentas de ese motor.
    plan = run()
    for item in (i for i in plan["items"] if i["layer"] == 3):
        assert item["to"]["motor"] == item["from"]["motor"]
        assert item["to"]["routeId"] == "claude:claude"

def test_empty_account_scales_from_the_fleet_reference_load():
    # Una cuenta sin sesiones vivas no se escala desde cero (eso la haría parecer
    # varias veces peor de lo que es y nadie se movería nunca a ella).
    row = al.enrich_limits([limit("claude", "relotto", 65.0, 76.5 * 3600, kind="weekly_all")], NOW)[0]
    ref = al._reference_load({"claude:main": 6.2, "codex:main": 1.6, "grok:main": 1.6})
    assert round(ref, 3) == 3.133
    empty = al._burn_after(row, 0.0, 1.6, ref)
    assert round(empty, 3) == round(row["burn"] * 1.6 / ref, 3)
    assert empty < al._burn_after(row, 0.0, 3.2, ref)        # más carga, más ritmo
    assert al._burn_after(row, 6.2, 1.6, ref) < empty        # con carga propia manda la suya

def test_session_on_an_unlisted_account_can_stay_where_it_is():
    # "vieja" no está en ACCOUNTS: quedarse igual sigue siendo candidato, y con 2 % usado
    # y 160 h para el reset gana a la cuenta al 95 % que resetea en 20 h.
    rows = [limit("claude", "main", 95.0, 20 * 3600, kind="weekly_all"),
            limit("claude", "vieja", 2.0, 160 * 3600, kind="weekly_all")]
    live = [sess("Vieja", "claude", "claude-fable-5-1", "xhigh", account="vieja", pane="%77")]
    plan = al.propose(live, rows, REGISTRY, SUPPORT, ACCOUNTS, TIERS, now=NOW)
    item = plan["items"][0]
    assert item["layer"] == 3
    assert item["to"]["harnessAccount"] == "vieja" and item["to"]["motorAccount"] == "vieja"
    assert item["to"]["routeId"] == "claude:claude"
    assert item["risk"] == "bajo" and "se queda" in item["reason"]

def test_session_on_a_motor_harness_that_is_not_a_motor_counts_and_never_moves():
    # acp:claude gasta la suscripción de Claude: pesa en claude:main, pero la ruta no
    # admite cambio en caliente, así que la propuesta la deja intacta.
    acp = dict(sess("Terminal", "acp", "claude-opus-5", "high", pane="%80"),
               motor="claude", routeId="acp:claude")
    plan = al.propose(sessions() + [acp], LIMITS, REGISTRY, SUPPORT, ACCOUNTS, TIERS, now=NOW)
    item = next(i for i in plan["items"] if i["session"] == "Terminal")
    assert item["locked"] and item["same"] and item["risk"] == "-"
    assert item["reason"] == "esta ruta no admite cambio en caliente"
    assert item["to"]["routeId"] == "acp:claude" and item["to"]["model"] == "claude-opus-5"
    assert item["to"]["effort"] == "high"                   # ni el effort se toca
    sin_acp, con_acp = run()["impact"]["claude:main"], plan["impact"]["claude:main"]
    assert con_acp["n"] == sin_acp["n"] + 1                 # su peso cuenta en la cuota del motor
    assert con_acp["burnAfter"] > sin_acp["burnAfter"]

def test_stale_quota_is_used_and_a_pool_without_quota_never_wins():
    rows = [limit("claude", "main", 40.0, 100 * 3600, kind="weekly_all", stale=True)]  # ni relotto ni grok
    live = [sess("SAVA", "claude", "claude-fable-5-1", "xhigh", pane="%10"),
            sess("Chips", "grok", "grok-4.6", "high", status="idle", pane="%60")]
    plan = al.propose(live, rows, REGISTRY, SUPPORT, ACCOUNTS, TIERS, now=NOW)
    sava = next(i for i in plan["items"] if i["session"] == "SAVA")
    chips = next(i for i in plan["items"] if i["session"] == "Chips")
    assert sava["to"]["harnessAccount"] == "main"           # relotto no tiene cuota: no gana
    assert "40 % usado" in sava["reason"]                   # la fila stale se usa igual
    assert "sin cuota conocida" in chips["reason"]          # grok no tiene fila
    grok = plan["impact"]["grok:main"]
    assert grok["known"] is False and grok["burnNow"] is None and grok["verdict"] == ""
    assert grok["reachesReset"] is None and grok["n"] == 1

def test_input_order_does_not_change_the_plan():
    base = al.propose(sessions(), LIMITS, REGISTRY, SUPPORT, ACCOUNTS, TIERS, now=NOW)
    want = json.dumps(base, sort_keys=True)
    for perm in ((4, 3, 2, 1, 0), (3, 0, 4, 2, 1), (1, 4, 0, 3, 2)):
        rows = sessions()
        shuffled = [rows[i] for i in perm]
        got = al.propose(shuffled, LIMITS, REGISTRY, SUPPORT, ACCOUNTS, TIERS, now=NOW)
        assert json.dumps(got, sort_keys=True) == want


def test_el_item_lleva_la_etiqueta_legible_de_la_sesion():
    """El nombre de tmux (term-3692-1) no identifica nada para quien decide.
    El item arrastra el `project` que el resto del tablero ya muestra."""
    ss = [dict(sess("term-3692-1", "claude", "claude-opus-5", "high"), project="Signara ⫽2")]
    plan = al.propose(ss, LIMITS, REGISTRY, SUPPORT, ACCOUNTS, TIERS, now=NOW)
    it = plan["items"][0]
    assert it["project"] == "Signara ⫽2"
    assert it["session"] == "term-3692-1", "el nombre interno se conserva para el coordinador"


def test_una_sesion_sin_project_no_revienta_el_item():
    plan = al.propose([sess("term-9", "claude", "claude-opus-5", "high")],
                      LIMITS, REGISTRY, SUPPORT, ACCOUNTS, TIERS, now=NOW)
    assert plan["items"][0]["project"] == ""


# ---------- perfil de proyecto, interrupciones y antigüedad ----------

def _plan(ss, **kw):
    return al.propose(ss, LIMITS, REGISTRY, SUPPORT, ACCOUNTS, TIERS, now=NOW, **kw)


def test_sin_perfil_nada_cambia():
    """Los fixtures no traen perfil ni marca de tiempo: la propuesta es la de siempre."""
    a = _plan(sessions())
    b = _plan(sessions(), profiles={}, signals={})
    assert a["items"] == b["items"]


def test_valor_alto_y_complejo_se_queda_con_lo_mejor_aunque_este_parada():
    s = dict(sess("big", "claude", "claude-opus-5", "low", status="done"), cwd="/repo/big")
    assert al.layer_of(s) == 1
    assert al.layer_of(s, {"value": "alto", "complexity": "alta"}) == 3
    it = _plan([s], profiles={"/repo/big": {"value": "alto", "complexity": "alta"}})["items"][0]
    assert it["to"]["effort"] == "high"
    assert "se queda con lo mejor" in it["reason"]


def test_valor_alto_que_interrumpe_sube_de_nivel():
    s = dict(sess("x", "claude", "claude-opus-5", "medium"), cwd="/repo/x")
    assert al.layer_of(s, {"value": "alto"}, {"interruptionsPerHour": 1.0}) == 2
    assert al.layer_of(s, {"value": "alto"}, {"interruptionsPerHour": 4.0}) == 3


def test_valor_bajo_absorbe_lo_barato():
    s = dict(sess("cheap", "claude", "claude-opus-5", "max"), cwd="/repo/cheap")
    assert al.layer_of(s, {"value": "bajo"}) == 2
    assert al.layer_of(dict(s, status="idle"), {"value": "bajo"}) == 1
    it = _plan([s], profiles={"/repo/cheap": {"value": "bajo"}})["items"][0]
    assert it["to"]["effort"] == "medium" and "valor bajo" in it["reason"]


def test_complejidad_baja_no_necesita_lo_mas_pesado():
    s = dict(sess("simple", "claude", "claude-opus-5", "xhigh"), cwd="/repo/simple")
    assert al.layer_of(s, {"value": "alto", "complexity": "baja"}) == 2


def test_el_perfil_se_hereda_por_prefijo_de_ruta():
    s = dict(sess("sub", "claude", "claude-opus-5", "high"), cwd="/repo/big/packages/api")
    assert al.profile_for(s, {"/repo/big": {"value": "alto"}}) == {"value": "alto"}
    assert al.profile_for(s, {"/repo/bigger": {"value": "alto"}}) == {}   # prefijo de texto no basta
    assert al.profile_for(dict(s, gitRoot="/repo/big"), {"/repo/big": {"value": "bajo"}}) == {"value": "bajo"}


def test_una_sesion_parada_hace_horas_casi_no_pesa():
    working = dict(sess("w", "claude", "claude-opus-5", "high", status="working"), ts=NOW - 30 * 3600)
    assert al.stale_factor(working, NOW) == 1.0                       # trabajando: pesa entera
    assert al.stale_factor(dict(working, status="done"), NOW) == 0.1  # parada > 24 h
    assert al.stale_factor(dict(working, status="idle", ts=NOW - 4 * 3600), NOW) == 0.25
    assert al.stale_factor(dict(working, status="idle", ts=NOW - 600), NOW) == 1.0
    assert al.stale_factor(dict(working, status="done", ts=None), NOW) == 1.0  # sin marca: reproducible
    it = _plan([dict(working, status="done")])["items"][0]
    assert it["weightFrom"] == round(al.WEIGHT["high"] * 0.1, 3)
    assert "parada" in it["reason"]


def test_las_de_mas_valor_se_reparten_primero():
    lo = dict(sess("lo", "claude", "claude-opus-5", "high"), cwd="/lo")
    hi = dict(sess("hi", "claude", "claude-opus-5", "high"), cwd="/hi")
    plan = _plan([lo, hi], profiles={"/lo": {"value": "bajo"}, "/hi": {"value": "alto"}})
    # El orden de reparto no es visible en items (van ordenados por clave), pero si
    # la consecuencia: la de valor alto conserva su capa y la baja cae a 2.
    by = {i["session"]: i for i in plan["items"]}
    assert by["hi"]["to"]["effort"] == "high" and by["lo"]["to"]["effort"] == "medium"
    assert by["hi"]["profile"]["value"] == "alto"


def test_valor_alto_y_complejo_no_baja_de_xhigh():
    """"Se queda con lo mejor" tiene que ser literal: si ya corre en xhigh no se
    le propone high por la tabla de capas."""
    s = dict(sess("big", "claude", "claude-opus-5", "xhigh", status="working"), cwd="/repo/big")
    it = _plan([s], profiles={"/repo/big": {"value": "alto", "complexity": "alta"}})["items"][0]
    assert it["to"]["effort"] == "xhigh"
    # Sin perfil, la tabla de siempre: capa 3 propone high.
    assert _plan([s])["items"][0]["to"]["effort"] == "high"


def test_una_cuota_sin_porcentaje_no_revienta_la_propuesta():
    """Visto en produccion: Grok medido en local sin limite declarado trae
    percent=None y la propuesta entera moria en float(None)."""
    lim = LIMITS + [dict(id="grok:main:7d:measured", provider="grok", account="main", percent=None,
                         resets_at=NOW + 3600, window="7d", kind="measured")]
    lim = [l for l in lim if not (l["provider"] == "grok" and l.get("kind") != "measured")]
    assert al.governing_limit(al.enrich_limits(lim, NOW), "grok", "main") is None
    plan = al.propose([sess("g", "grok", "grok-4.6", "high")], lim, REGISTRY, SUPPORT, ACCOUNTS, TIERS, now=NOW)
    assert plan["items"][0]["to"]["motor"] and plan["items"][0]["reason"]  # se propone algo, no se rompe
    assert plan["impact"]["grok:main"]["known"] is False
