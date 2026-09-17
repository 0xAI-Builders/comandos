# Reparto de cuota Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pestaña **Reparto** en Analytics que analiza cuotas y sesiones vivas, propone una redistribución determinista de cuenta/motor/modelo/effort, deja curarla arrastrando y tocando, y la aplica en lote sobre el coordinador existente sin que ninguna sesión se rompa.

**Architecture:** Un módulo puro `lib/allocation.py` calcula ritmo y proyección de cada cuota, la propuesta y el impacto en vivo. Un runner `lib/allocation_batch.py` convierte un plan congelado en N llamadas idempotentes a `session_configure` con estado por pane. `bin/cc-dash` expone cinco endpoints `/allocation/*`, enriquece `limits[]` y hereda el trust de carpeta al cambiar de cuenta. El tablero gana `dash/reparto.js` + `dash/reparto.css` (tanques, fichas, arrastre con vista previa, progreso) y la pestaña Optimizar desaparece.

**Tech Stack:** Python 3.10 stdlib (sin dependencias nuevas), pytest, JS vanilla en el tablero, container queries CSS, node solo para el test de parseo.

**Spec:** `docs/superpowers/specs/2026-09-16-reparto-de-cuota-design.md`

## Global Constraints

- Sin dependencias Python nuevas; todo stdlib. Sin frameworks JS.
- El motor es determinista: mismas entradas, misma salida; nada de `random`, `time.time()` ni orden de dict sin ordenar.
- Copia en lenguaje llano: "llega al reset" / "se acaba en 1d 22h" / "ritmo 1.3x". Prohibido "aguanta", F/M/L, iconos sin texto.
- Modelo y effort siempre en texto en cada ficha.
- Los modelos por capa salen de `config/model-tiers.json` (`tiers` + `patterns`), nunca de IDs literales en código.
- Trust: solo se hereda si la carpeta ya estaba aceptada en la cuenta origen (HOME `~/.claude.json` o `{config_dir_origen}/.claude.json`); nunca para `$HOME`.
- Cada pane del lote usa `requestId = "{batchId}:{session}:{pane}"` e `interrupt: true`.
- `docs/superpowers/` está en `.gitignore`: los specs/planes se añaden con `git add -f` (convención del repo).
- Tests: `python3 -m pytest tests/<archivo> -q` desde la raíz del worktree; `bash tests/test_js_parses.sh` para el tablero. cc-dash se carga con `load_dash_module()` de `tests/test_agent_launch.py`.

---

## File Structure

| Archivo | Responsabilidad |
|---|---|
| `lib/allocation.py` (nuevo) | Puro. `enrich_limits`, `layer_of`, `model_for_layer`, `pool_key`, `propose`, `impact`, `plan_hash`, `invert`. Sin I/O. |
| `lib/allocation_batch.py` (nuevo) | Runner de lote: recibe un plan y un callable `configure(data) -> (code, body)`, ejecuta con concurrencia 3, guarda estado por pane, expone `status()`. Persistencia en `~/.claude/hooks/allocation-batches.json`. |
| `lib/claude_trust.py` (modificar) | Nueva `inherit_cwd_trust(cwd, *, source_config_dir, dest_config_dir, home)`. |
| `config/detectors.json` (modificar) | Nuevas claves `dialogPatterns` (lista de regex, en/es) y `verifyAttemptsWithMcp`. |
| `bin/cc-dash` (modificar) | Enriquecer limits en `/usage/state`; endpoints `/allocation/{propose,preview,apply,status,revert}`; trust heredado en `SessionConfiguration.prepare` y en `/session-new`, `/up`, `/recover-tab`; regex de diálogos desde config; `_verify` con más intentos si hay MCPs. |
| `dash/reparto.js` (nuevo) | Toda la UI de la pestaña: estado, render de tanques/fichas/pie/progreso, drag con vista previa, polling. Expone `window.Reparto = {load, render}`. |
| `dash/reparto.css` (nuevo) | Estilos de la pestaña, con container queries sobre `.modal-panel`. |
| `dash/index.html` (modificar) | Pestaña Optimizar → Reparto; `<div data-mpane="reparto">`; carga de `reparto.js`/`reparto.css`; `MTAB_GROUPS.reparto`; borrar `loadOptimize/renderOptimize/applyOptimization` y el markup del pane optimizar. |
| `tests/test_allocation.py` (nuevo) | Motor puro. |
| `tests/test_allocation_batch.py` (nuevo) | Runner con `configure` falso. |
| `tests/test_allocation_dash.py` (nuevo) | Endpoints con `load_dash_module` y monkeypatch. |
| `tests/test_claude_trust.py` (modificar) | Casos de herencia. |
| `tests/test_session_verify_dialogs.py` (nuevo) | Regex desde config y ventana de verificación. |
| `tests/test_dashboard_reparto.py` (nuevo) | Markup, carga de assets, parseo de `reparto.js`, copy prohibida. |
| `docs/reparto.md` (nuevo) | Doc de usuario corta. |

---

### Task 1: `enrich_limits` — ritmo y proyección para toda cuota

**Files:**
- Create: `lib/allocation.py`
- Test: `tests/test_allocation.py`

**Interfaces:**
- Produces: `enrich_limits(limits: list[dict], now: float) -> list[dict]`. Devuelve copias con `pace` (0..100, % de ventana transcurrida), `burn` (float, `percent / pace`), `runsOutIn` (segundos o `None` si no se acaba), `reachesReset` (bool), `verdict` (str: `"llega al reset"` o `"se acaba en 1d 22h"`), `windowSeconds`. Ventana en segundos por `window`: `"5h"→18000`, `"7d"→604800`, otro→`None` (fila sin proyección: `burn=None`, `verdict=""`).
- Produces: `fmt_duration(seconds: float) -> str` (`"45 min"`, `"7 h"`, `"1d 22h"`).

- [ ] **Step 1: Write the failing tests**

```python
# tests/test_allocation.py
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python3 -m pytest tests/test_allocation.py -q`
Expected: FAIL con `ModuleNotFoundError: No module named 'allocation'`

- [ ] **Step 3: Write minimal implementation**

```python
# lib/allocation.py
"""Reparto de cuota: motor puro (sin I/O).

Cuotas → ritmo y proyección; sesiones + cuotas → propuesta determinista;
propuesta + un cambio tentativo → impacto en vivo.
"""
from __future__ import annotations

import copy
import hashlib
import json
import re

WINDOW_SECONDS = {"5h": 5 * 3600, "7d": 7 * 24 * 3600}


def fmt_duration(seconds: float) -> str:
    seconds = max(0.0, float(seconds))
    if seconds < 3600:
        return f"{int(round(seconds / 60))} min"
    if seconds < 48 * 3600:
        return f"{int(round(seconds / 3600))} h"
    days, rest = divmod(int(round(seconds / 3600)), 24)
    return f"{days}d {rest}h"


def enrich_limits(limits: list[dict], now: float) -> list[dict]:
    out = []
    for raw in limits:
        row = dict(raw)
        win = WINDOW_SECONDS.get(str(row.get("window") or ""))
        resets_at = float(row.get("resets_at") or 0)
        pct = row.get("percent")
        row["windowSeconds"] = win
        if not win or resets_at <= now or pct is None:
            row.update(pace=None, burn=None, runsOutIn=None, reachesReset=None, verdict="")
            out.append(row)
            continue
        remaining = resets_at - now
        elapsed = max(1.0, win - remaining)
        pace = min(100.0, elapsed / win * 100.0)
        pct = float(pct)
        burn = pct / max(1.0, pace)
        rate = pct / elapsed  # % por segundo
        runs_out = (100.0 - pct) / rate if rate > 0 else None
        reaches = runs_out is None or runs_out >= remaining
        row.update(pace=pace, burn=burn,
                   runsOutIn=None if reaches else runs_out,
                   reachesReset=reaches,
                   verdict="llega al reset" if reaches else f"se acaba en {fmt_duration(runs_out)}")
        out.append(row)
    return out
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `python3 -m pytest tests/test_allocation.py -q`
Expected: 5 passed

- [ ] **Step 5: Commit**

```bash
git add lib/allocation.py tests/test_allocation.py
git commit -m "feat(allocation): ritmo y proyección de cada cuota (enrich_limits)"
```

---

### Task 2: Capas, modelo por capa y clave de cuota

**Files:**
- Modify: `lib/allocation.py`
- Test: `tests/test_allocation.py`

**Interfaces:**
- Produces: `layer_of(session: dict) -> int` (3/2/1 desde `effort` y `status`).
- Produces: `LAYER_EFFORT = {3: "high", 2: "medium", 1: "low"}`, `WEIGHT = {"low": .5, "medium": 1.0, "high": 1.6, "xhigh": 2.2, "max": 3.0}`.
- Produces: `model_for_layer(motor: str, layer: int, registry: dict, tiers: dict) -> str`. `registry["motors"][motor]["models"]` es una lista de `{"id": ..., "efforts": [...]}`; `tiers` es el JSON de `config/model-tiers.json` (`patterns: [{match, tier}]`). Capa→tier: `3→"high"`, `2→"mid"`, `1→"low"`; si ningún modelo del motor cae en ese tier se prueba `mid`, luego el primero de la lista.
- Produces: `pool_key(motor: str, account: str) -> str` = `f"{motor}:{account}"`.
- Produces: `governing_limit(limits: list[dict], motor: str, account: str) -> dict | None`: la fila `7d` de ese `provider`/`account` (Claude usa la fila `kind == "weekly_all"`), si no la única que haya.

- [ ] **Step 1: Write the failing tests**

```python
REGISTRY = {"motors": {
    "claude": {"models": [{"id": "claude-fable-5-1", "efforts": ["low", "medium", "high", "xhigh", "max"]},
                          {"id": "claude-opus-5", "efforts": ["low", "medium", "high", "max"]},
                          {"id": "claude-sonnet-5", "efforts": ["low", "medium", "high"]}]},
    "codex": {"models": [{"id": "gpt-6-astra", "efforts": ["low", "medium", "high", "xhigh"]},
                         {"id": "gpt-5.6-luna", "efforts": ["low", "medium", "high"]}]},
    "grok": {"models": [{"id": "grok-4.6", "efforts": ["low", "medium", "high"]}]}}}
TIERS = {"patterns": [{"match": "grok-4\\.(6|5)", "tier": "high"}, {"match": "fable|mythos|opus", "tier": "high"},
                      {"match": "gpt-6|gpt-5\\.6|codex-max", "tier": "high"},
                      {"match": "sonnet|terra|sol|luna", "tier": "mid"}, {"match": "haiku|mini|spark|nano", "tier": "low"}]}

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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python3 -m pytest tests/test_allocation.py -q -k "layer or model_for or governing"`
Expected: FAIL con `AttributeError: module 'allocation' has no attribute 'layer_of'`

- [ ] **Step 3: Write minimal implementation** (añadir a `lib/allocation.py`)

```python
LAYER_EFFORT = {3: "high", 2: "medium", 1: "low"}
LAYER_TIER = {3: "high", 2: "mid", 1: "low"}
WEIGHT = {"low": 0.5, "medium": 1.0, "high": 1.6, "xhigh": 2.2, "max": 3.0}
_HIGH_EFFORTS = {"high", "xhigh", "max"}


def layer_of(session: dict) -> int:
    effort = str(session.get("effort") or "").lower()
    base = 3 if effort in _HIGH_EFFORTS else 1 if effort == "low" else 2
    if str(session.get("status") or "") in ("idle", "done"):
        base = max(1, base - 1)
    return base


def _tier_of(model_id: str, tiers: dict) -> str:
    for pat in tiers.get("patterns") or []:
        if re.search(pat.get("match", ""), model_id, re.I):
            return pat.get("tier", "unknown")
    return "unknown"


def model_for_layer(motor: str, layer: int, registry: dict, tiers: dict) -> str:
    models = [m["id"] for m in ((registry.get("motors") or {}).get(motor) or {}).get("models") or []]
    if not models:
        return ""
    for tier in (LAYER_TIER[layer], "mid"):
        for mid in models:
            if _tier_of(mid, tiers) == tier:
                return mid
    return models[0]


def pool_key(motor: str, account: str) -> str:
    return f"{motor}:{account or 'main'}"


def governing_limit(limits: list[dict], motor: str, account: str) -> dict | None:
    rows = [r for r in limits if r.get("provider") == motor and (r.get("account") or "main") == (account or "main")]
    for r in rows:
        if r.get("window") == "7d" and r.get("kind") in ("weekly_all", "window", None) and r.get("kind") != "weekly_scoped":
            return r
    for r in rows:
        if r.get("window") == "7d":
            return r
    return rows[0] if rows else None
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `python3 -m pytest tests/test_allocation.py -q`
Expected: 8 passed

- [ ] **Step 5: Commit**

```bash
git add lib/allocation.py tests/test_allocation.py
git commit -m "feat(allocation): capa por sesión, modelo por capa desde model-tiers, cuota rectora"
```

---

### Task 3: `propose` — propuesta determinista

**Files:**
- Modify: `lib/allocation.py`
- Test: `tests/test_allocation.py`

**Interfaces:**
- Consumes: Task 1 y 2.
- Produces: `propose(sessions, limits, registry, support, accounts, tiers, *, now, overrides=None) -> dict` (el *Plan*):
  - `sessions`: filas de `/state` vivas con `session, pane, agent, motor, model, effort, account, harnessAccount, motorAccount, routeId, status, cwd`.
  - `support`: `dict[routeId] -> {"selectable": bool, "reason": {...}|None}` (salida de `session_change_support`).
  - `accounts`: `dict[motor] -> list[str]` de alias seleccionables.
  - `overrides`: `dict["session|pane"] -> {"layer": int} | {"to": {...}} | {"locked": True}`.
  - Plan: `{"planId", "stateHash", "createdAt", "items": [{"session","pane","key","layer","from":{agent,motor,model,effort,account},"to":{harness,motor,model,effort,harnessAccount,motorAccount,routeId},"same":bool,"reason":str,"risk":"bajo|medio|alto|-","locked":bool}], "impact": {poolKey: {...}}}`.
- Produces: `plan_hash(sessions) -> str` (sha256 corto sobre `session, pane, agent, model, effort, account, harnessAccount, motorAccount` ordenados).
- Produces: `impact(items, limits, now) -> dict[poolKey -> {"n","burnNow","burnAfter","usedAfter","runsOutIn","reachesReset","verdict","label"}]`.

Reglas (del spec): orden por capa desc y luego `key` asc; candidatos = rutas `selectable` cuyo `harness` es el harness actual (`routeId.split(":")[0]`) × cuentas del motor (gateway `harness != motor` fuerza `motorAccount = "main"`) + quedarse igual; carga por peso; `burnAfter = burnNow × pesoNuevo ÷ pesoActual` (`pesoNuevo × 0.06` si `percent < 3`); elegir la cuota con menor `burnAfter`; restricción dura `burnAfter ≤ 1.0` si hay candidato; empates: igual, mismo motor, reset más lejano.

- [ ] **Step 1: Write the failing tests**

```python
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python3 -m pytest tests/test_allocation.py -q -k "plan or strong or medium or override or unselectable or hard or impact"`
Expected: FAIL con `AttributeError: ... 'propose'`

- [ ] **Step 3: Write minimal implementation** (añadir a `lib/allocation.py`)

```python
_HASH_FIELDS = ("session", "pane", "agent", "model", "effort", "account", "harnessAccount", "motorAccount")


def session_key(s: dict) -> str:
    return f"{s.get('session')}|{s.get('pane')}"


def plan_hash(sessions: list[dict]) -> str:
    rows = sorted(tuple(str(s.get(f) or "") for f in _HASH_FIELDS) for s in sessions)
    return hashlib.sha256(json.dumps(rows).encode()).hexdigest()[:16]


def _from(s: dict) -> dict:
    return dict(agent=s.get("agent") or "", motor=s.get("motor") or s.get("agent") or "",
                model=s.get("model") or "", effort=s.get("effort") or "",
                account=s.get("harnessAccount") or s.get("account") or "main",
                motorAccount=s.get("motorAccount") or s.get("account") or "main")


def _load_map(items_to: list[tuple[str, str]]) -> dict:
    """items_to: [(poolKey, effort)] → {poolKey: peso}."""
    load: dict[str, float] = {}
    for pool, effort in items_to:
        load[pool] = load.get(pool, 0.0) + WEIGHT.get(effort, 1.0)
    return load


def _burn_after(row: dict | None, load_now: float, load_after: float) -> float | None:
    if not row or row.get("burn") is None:
        return None
    if float(row.get("percent") or 0) < 3:
        return max(0.1, load_after * 0.06)
    return row["burn"] * (load_after / max(0.5, load_now))


def _candidates(s: dict, registry: dict, support: dict, accounts: dict) -> list[dict]:
    harness = (s.get("routeId") or f"{s.get('agent')}:{s.get('agent')}").split(":")[0]
    out = []
    for route in registry.get("routes") or []:
        if route.get("harness") != harness:
            continue
        if not (support.get(route["id"]) or {}).get("selectable"):
            continue
        motor = route["motor"]
        gateway = motor != harness
        for acct in sorted(accounts.get(harness if gateway else motor) or ["main"]):
            out.append(dict(harness=harness, motor=motor, routeId=route["id"],
                            harnessAccount=acct, motorAccount="main" if gateway else acct))
    return out


def _risk(frm: dict, to: dict) -> str:
    if to["motor"] != frm["motor"] or to["harnessAccount"] != frm["account"]:
        return "alto"
    if to["model"] != frm["model"]:
        return "medio"
    return "bajo"


def _same(frm: dict, to: dict) -> bool:
    return (to["motor"] == frm["motor"] and to["model"] == frm["model"] and to["effort"] == frm["effort"]
            and to["harnessAccount"] == frm["account"] and to["motorAccount"] == frm["motorAccount"])


def impact(items: list[dict], limits: list[dict], now: float) -> dict:
    enriched = enrich_limits(limits, now)
    pools = sorted({pool_key(i["from"]["motor"], i["from"]["account"]) for i in items}
                   | {pool_key(i["to"]["motor"], i["to"]["harnessAccount"]) for i in items}
                   | {pool_key(r["provider"], r.get("account") or "main") for r in enriched})
    load_now = _load_map([(pool_key(i["from"]["motor"], i["from"]["account"]), i["from"]["effort"]) for i in items])
    load_after = _load_map([(pool_key(i["to"]["motor"], i["to"]["harnessAccount"]), i["to"]["effort"]) for i in items])
    out = {}
    for pool in pools:
        motor, acct = pool.split(":", 1)
        row = governing_limit(enriched, motor, acct)
        burn_now = row.get("burn") if row else None
        burn_after = _burn_after(row, load_now.get(pool, 0.0), load_after.get(pool, 0.0))
        entry = dict(n=int(sum(1 for i in items if pool_key(i["to"]["motor"], i["to"]["harnessAccount"]) == pool)),
                     label=f"{motor} {acct}", burnNow=burn_now, burnAfter=burn_after,
                     usedAfter=None, runsOutIn=None, reachesReset=None, verdict="", known=row is not None)
        if row and burn_after is not None:
            remaining = float(row["resets_at"]) - now
            elapsed = max(1.0, row["windowSeconds"] - remaining)
            rate_after = (float(row["percent"]) / elapsed) * (burn_after / max(0.01, burn_now or 0.01)) if burn_now else 0.0
            used_after = min(100.0, float(row["percent"]) + rate_after * remaining)
            runs_out = (100.0 - float(row["percent"])) / rate_after if rate_after > 0 else None
            reaches = runs_out is None or runs_out >= remaining
            entry.update(usedAfter=used_after, runsOutIn=None if reaches else runs_out, reachesReset=reaches,
                         verdict="llega al reset" if reaches else f"se acaba en {fmt_duration(runs_out)}")
        out[pool] = entry
    return out


def propose(sessions, limits, registry, support, accounts, tiers, *, now, overrides=None) -> dict:
    overrides = overrides or {}
    enriched = enrich_limits(limits, now)
    live = [s for s in sessions if s.get("agent") in (registry.get("motors") or {})]
    order = sorted(live, key=lambda s: (-layer_of(s), session_key(s)))
    load = _load_map([(pool_key(_from(s)["motor"], _from(s)["account"]), _from(s)["effort"]) for s in live])
    load_now = dict(load)
    items = []
    for s in order:
        key = session_key(s)
        frm = _from(s)
        ov = overrides.get(key) or {}
        layer = int(ov.get("layer") or layer_of(s))
        locked = bool(ov.get("locked"))
        if locked or "to" in ov:
            to = dict(harness=frm["motor"], motor=frm["motor"], model=frm["model"], effort=frm["effort"],
                      harnessAccount=frm["account"], motorAccount=frm["motorAccount"],
                      routeId=s.get("routeId") or f"{frm['motor']}:{frm['motor']}") if locked else dict(ov["to"])
            reason = "fijada por ti" if locked else "ajustada por ti"
        else:
            effort = LAYER_EFFORT[layer]
            scored = []
            for c in _candidates(s, registry, support, accounts):
                model = model_for_layer(c["motor"], layer, registry, tiers)
                pool = pool_key(c["motor"], c["harnessAccount"])
                row = governing_limit(enriched, c["motor"], c["harnessAccount"])
                src_pool = pool_key(frm["motor"], frm["account"])
                after = dict(load)
                after[src_pool] = after.get(src_pool, 0.0) - WEIGHT.get(frm["effort"], 1.0)
                after[pool] = after.get(pool, 0.0) + WEIGHT.get(effort, 1.0)
                burn_after = _burn_after(row, load_now.get(pool, 0.0), after[pool])
                is_same = c["motor"] == frm["motor"] and c["harnessAccount"] == frm["account"]
                same_motor = c["motor"] == frm["motor"]
                reset_far = -(float(row["resets_at"]) if row else 0.0)
                unknown = row is None or burn_after is None
                over = 0 if (not unknown and burn_after <= 1.0) else 1
                scored.append(((unknown, over, 999.0 if unknown else burn_after, 0 if is_same else 1,
                                0 if same_motor else 1, reset_far, c["routeId"]), c, model, burn_after, row))
            scored.sort(key=lambda t: t[0])
            _, c, model, burn_after, row = scored[0]
            to = dict(c, model=model, effort=effort)
            pct = f"{int(round(float(row['percent'])))} %" if row else "sin cuota conocida"
            reset = f"reset en {fmt_duration(float(row['resets_at']) - now)}" if row else ""
            reason = (f"{to['motor']} {to['harnessAccount']} tiene más margen: {pct} usado, {reset}"
                      if not (to["motor"] == frm["motor"] and to["harnessAccount"] == frm["account"])
                      else f"se queda en {to['motor']} {to['harnessAccount']}: {pct} usado, {reset}")
            if burn_after is not None and burn_after > 1.0:
                reason += " · ninguna cuota llega al reset con esta carga; es la que menos se pasa"
        # aplicar la asignación a la carga acumulada
        src_pool = pool_key(frm["motor"], frm["account"])
        load[src_pool] = load.get(src_pool, 0.0) - WEIGHT.get(frm["effort"], 1.0)
        dst_pool = pool_key(to["motor"], to["harnessAccount"])
        load[dst_pool] = load.get(dst_pool, 0.0) + WEIGHT.get(to["effort"], 1.0)
        same = _same(frm, to)
        items.append(dict(session=s["session"], pane=s["pane"], key=key, layer=layer, cwd=s.get("cwd") or "",
                          **{"from": frm}, to=to, same=same, locked=locked,
                          reason="igual" if same and not locked else reason,
                          risk="-" if same else _risk(frm, to)))
    items.sort(key=lambda i: i["key"])
    state_hash = plan_hash(sessions)
    plan_id = hashlib.sha256(f"{state_hash}:{json.dumps(overrides, sort_keys=True)}".encode()).hexdigest()[:12]
    return dict(planId=plan_id, stateHash=state_hash, createdAt=now, items=items,
                impact=impact(items, limits, now))


def invert(plan: dict) -> dict:
    """Plan inverso: cada item vuelve a su `from`."""
    inv = copy.deepcopy(plan)
    for i in inv["items"]:
        frm, to = i["from"], i["to"]
        i["to"] = dict(harness=frm["motor"], motor=frm["motor"], model=frm["model"], effort=frm["effort"],
                       harnessAccount=frm["account"], motorAccount=frm["motorAccount"],
                       routeId=f"{frm['motor']}:{frm['motor']}")
        i["from"] = dict(agent=to["motor"], motor=to["motor"], model=to["model"], effort=to["effort"],
                         account=to["harnessAccount"], motorAccount=to["motorAccount"])
        i["reason"], i["risk"] = "revertir", "-"
    inv["planId"] = "rev-" + plan["planId"]
    return inv
```

Nota para el implementador: `registry` real viene de `load_provider_registry()` y tiene `routes` con `id`, `harness`, `motor`; en el test hay que añadir `"routes"` al `REGISTRY` de Task 2:

```python
REGISTRY["routes"] = [{"id": "claude:claude", "harness": "claude", "motor": "claude"},
                      {"id": "claude:codex", "harness": "claude", "motor": "codex"},
                      {"id": "codex:codex", "harness": "codex", "motor": "codex"},
                      {"id": "grok:grok", "harness": "grok", "motor": "grok"},
                      {"id": "acp:claude", "harness": "acp", "motor": "claude"}]
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `python3 -m pytest tests/test_allocation.py -q`
Expected: 17 passed. Si `test_medium_layer_moves_to_codex_when_codex_is_empty` falla porque LifeOS se queda en Claude: comprobar que `_burn_after` para Codex (1 %) devuelve `load_after × 0.06` y que `governing_limit` encuentra la fila de Codex (sin `kind`, window `7d`).

- [ ] **Step 5: Commit**

```bash
git add lib/allocation.py tests/test_allocation.py
git commit -m "feat(allocation): propuesta determinista, impacto por cuota y plan inverso"
```

---

### Task 4: Runner de lote `lib/allocation_batch.py`

**Files:**
- Create: `lib/allocation_batch.py`
- Test: `tests/test_allocation_batch.py`

**Interfaces:**
- Consumes: plan de Task 3.
- Produces: `class BatchStore(path)` con `create(plan, panes) -> batch`, `get(batch_id)`, `update_item(batch_id, key, **fields)`, `list()`; JSON en `path`, escritura atómica, `threading.Lock`.
- Produces: `run_batch(batch_id, store, configure, status_of, *, concurrency=3, poll=0.5, deadline=600)`: para cada item no `same` ni `locked` llama `configure({session, pane, requestId, interrupt: True, expectedIdentity?, toHarness, motor, model, effort, harnessAccount, motorAccount, routeId})`; con `code == 202` guarda `operationKey/operationId` y sondea `status_of(opkey, opid) -> dict` (mismo shape que `session_operation_status`) hasta estado terminal; traduce a `state ∈ {"cola","aplicando","lista","detenida"}` y `error`. Con `code != 202` → `detenida` con `error = body["error"]`.
- Produces: `batch_view(batch) -> {"batchId","planId","state":"aplicando|terminado","done","total","items":[...]}`; `state == "terminado"` cuando todos son `lista`/`detenida`/`omitida`.
- Estados del coordinador → UI: `confirmed → lista`; `failed|rolled_back|recovery_required → detenida`; `awaiting_confirmation → detenida` con `error = "esperando confirmación: " + motivo` (`dialog` si `status_of` devuelve `screenDialog`, si no `"timeout"`); `validating|waiting|snapshot|applying|verifying|recovering → aplicando`.

- [ ] **Step 1: Write the failing tests**

```python
# tests/test_allocation_batch.py
import sys, threading, time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
import allocation_batch as ab

def item(name, pane, same=False, locked=False):
    return dict(session=name, pane=pane, key=f"{name}|{pane}", same=same, locked=locked, layer=2, cwd="/tmp",
                **{"from": dict(agent="claude", motor="claude", model="a", effort="high", account="main", motorAccount="main")},
                to=dict(harness="claude", motor="codex", model="gpt-6-astra", effort="medium",
                        harnessAccount="main", motorAccount="main", routeId="claude:codex"))

def plan(*items):
    return dict(planId="p1", stateHash="h1", createdAt=0, items=list(items), impact={})

class FakeCoordinator:
    def __init__(self, fail=(), park=()):
        self.calls, self.fail, self.park, self.polls = [], set(fail), set(park), {}
    def configure(self, data):
        self.calls.append(data)
        key = f"{data['session']}|{data['pane']}"
        if key in self.fail:
            return 409, {"ok": False, "error": "pane ocupado"}
        return 202, {"ok": True, "pending": True, "operationKey": key, "operationId": "op-" + key}
    def status_of(self, opkey, opid):
        n = self.polls[opkey] = self.polls.get(opkey, 0) + 1
        if opkey in self.park:
            return {"state": "awaiting_confirmation", "screenDialog": "trust"}
        return {"state": "applying" if n < 2 else "confirmed", "ok": n >= 2}

def test_batch_runs_all_items_with_request_ids_and_interrupt(tmp_path):
    store = ab.BatchStore(tmp_path / "b.json")
    co = FakeCoordinator()
    b = store.create(plan(item("A", "%1"), item("B", "%2"), item("C", "%3", same=True)), panes=None)
    ab.run_batch(b["batchId"], store, co.configure, co.status_of, poll=0.01)
    view = ab.batch_view(store.get(b["batchId"]))
    assert view["state"] == "terminado" and view["total"] == 3 and view["done"] == 3
    states = {i["key"]: i["state"] for i in view["items"]}
    assert states == {"A|%1": "lista", "B|%2": "lista", "C|%3": "omitida"}
    assert {c["requestId"] for c in co.calls} == {f"{b['batchId']}:A:%1", f"{b['batchId']}:B:%2"}
    assert all(c["interrupt"] is True and c["toHarness"] == "claude" and c["motor"] == "codex" for c in co.calls)

def test_rejected_and_parked_items_are_detenida_with_reason(tmp_path):
    store = ab.BatchStore(tmp_path / "b.json")
    co = FakeCoordinator(fail={"A|%1"}, park={"B|%2"})
    b = store.create(plan(item("A", "%1"), item("B", "%2")), panes=None)
    ab.run_batch(b["batchId"], store, co.configure, co.status_of, poll=0.01, deadline=1)
    items = {i["key"]: i for i in ab.batch_view(store.get(b["batchId"]))["items"]}
    assert items["A|%1"]["state"] == "detenida" and "ocupado" in items["A|%1"]["error"]
    assert items["B|%2"]["state"] == "detenida" and "trust" in items["B|%2"]["error"]

def test_panes_subset_and_rerun_is_idempotent(tmp_path):
    store = ab.BatchStore(tmp_path / "b.json")
    co = FakeCoordinator()
    b = store.create(plan(item("A", "%1"), item("B", "%2")), panes=["A|%1"])
    ab.run_batch(b["batchId"], store, co.configure, co.status_of, poll=0.01)
    ab.run_batch(b["batchId"], store, co.configure, co.status_of, poll=0.01)   # segunda pasada: nada nuevo
    assert [c["requestId"] for c in co.calls] == [f"{b['batchId']}:A:%1"]
    view = ab.batch_view(store.get(b["batchId"]))
    assert {i["key"]: i["state"] for i in view["items"]} == {"A|%1": "lista", "B|%2": "omitida"}

def test_retry_item_calls_coordinator_again(tmp_path):
    store = ab.BatchStore(tmp_path / "b.json")
    co = FakeCoordinator(fail={"A|%1"})
    b = store.create(plan(item("A", "%1")), panes=None)
    ab.run_batch(b["batchId"], store, co.configure, co.status_of, poll=0.01)
    co.fail.clear()
    ab.retry_item(b["batchId"], "A|%1", store, co.configure, co.status_of, poll=0.01)
    assert ab.batch_view(store.get(b["batchId"]))["items"][0]["state"] == "lista"
    assert len(co.calls) == 2 and co.calls[1]["requestId"].endswith(":retry1")

def test_concurrency_never_exceeds_limit(tmp_path):
    store = ab.BatchStore(tmp_path / "b.json")
    active, peak, lock = [0], [0], threading.Lock()
    class Slow(FakeCoordinator):
        def configure(self, data):
            with lock:
                active[0] += 1; peak[0] = max(peak[0], active[0])
            time.sleep(0.05)
            with lock:
                active[0] -= 1
            return super().configure(data)
    co = Slow()
    b = store.create(plan(*[item(f"S{i}", f"%{i}") for i in range(8)]), panes=None)
    ab.run_batch(b["batchId"], store, co.configure, co.status_of, poll=0.01, concurrency=3)
    assert peak[0] <= 3
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python3 -m pytest tests/test_allocation_batch.py -q`
Expected: FAIL con `ModuleNotFoundError: No module named 'allocation_batch'`

- [ ] **Step 3: Write minimal implementation**

```python
# lib/allocation_batch.py
"""Lote de reconfiguración: un plan → N operaciones del coordinador, con estado por pane."""
from __future__ import annotations

import json
import os
import secrets
import tempfile
import threading
import time
from concurrent.futures import ThreadPoolExecutor

TERMINAL = {"lista", "detenida", "omitida"}
_UI_STATE = {"confirmed": "lista", "failed": "detenida", "rolled_back": "detenida",
             "recovery_required": "detenida", "awaiting_confirmation": "detenida"}


class BatchStore:
    def __init__(self, path):
        self.path = str(path)
        self.lock = threading.Lock()
        self._data = self._read()

    def _read(self):
        try:
            with open(self.path) as f:
                return json.load(f)
        except Exception:
            return {"batches": {}}

    def _write(self):
        os.makedirs(os.path.dirname(self.path) or ".", exist_ok=True)
        fd, tmp = tempfile.mkstemp(dir=os.path.dirname(self.path) or ".", prefix=".batch-")
        with os.fdopen(fd, "w") as f:
            json.dump(self._data, f)
        os.replace(tmp, self.path)

    def create(self, plan, panes=None):
        wanted = set(panes) if panes else None
        items = []
        for it in plan["items"]:
            skip = it.get("same") or it.get("locked") or (wanted is not None and it["key"] not in wanted)
            items.append(dict(key=it["key"], session=it["session"], pane=it["pane"], to=it["to"],
                              **{"from": it["from"]}, state="omitida" if skip else "cola",
                              error="", operationKey="", operationId="", attempts=0))
        batch = dict(batchId=secrets.token_hex(6), planId=plan["planId"], stateHash=plan["stateHash"],
                     createdAt=time.time(), items=items)
        with self.lock:
            self._data["batches"][batch["batchId"]] = batch
            self._write()
        return batch

    def get(self, batch_id):
        with self.lock:
            b = self._data["batches"].get(batch_id)
            return json.loads(json.dumps(b)) if b else None

    def list(self):
        with self.lock:
            return sorted(self._data["batches"].values(), key=lambda b: b["createdAt"], reverse=True)

    def update_item(self, batch_id, key, **fields):
        with self.lock:
            for it in self._data["batches"][batch_id]["items"]:
                if it["key"] == key:
                    it.update(fields)
            self._write()


def _payload(batch_id, it, suffix=""):
    to = it["to"]
    return dict(session=it["session"], pane=it["pane"], requestId=f"{batch_id}:{it['session']}:{it['pane']}{suffix}",
                interrupt=True, toHarness=to.get("harness") or to["motor"], motor=to["motor"], model=to.get("model", ""),
                effort=to.get("effort", ""), harnessAccount=to.get("harnessAccount", "main"),
                motorAccount=to.get("motorAccount", "main"), routeId=to.get("routeId", ""))


def _drive(batch_id, it, store, configure, status_of, poll, deadline, suffix=""):
    store.update_item(batch_id, it["key"], state="aplicando", error="", attempts=it.get("attempts", 0) + 1)
    code, body = configure(_payload(batch_id, it, suffix))
    if code != 202:
        store.update_item(batch_id, it["key"], state="detenida", error=str(body.get("error") or f"HTTP {code}"))
        return
    opkey, opid = body.get("operationKey", ""), body.get("operationId", "")
    store.update_item(batch_id, it["key"], operationKey=opkey, operationId=opid)
    t0 = time.monotonic()
    while True:
        st = status_of(opkey, opid) or {}
        state = st.get("state", "")
        if state in _UI_STATE:
            ui = _UI_STATE[state]
            err = ""
            if state == "awaiting_confirmation":
                err = "esperando confirmación: " + (st.get("screenDialog") or "timeout")
            elif ui == "detenida":
                err = str(st.get("error") or state)
            store.update_item(batch_id, it["key"], state=ui, error=err)
            return
        if time.monotonic() - t0 > deadline:
            store.update_item(batch_id, it["key"], state="detenida", error="sin respuesta del coordinador")
            return
        time.sleep(poll)


def run_batch(batch_id, store, configure, status_of, *, concurrency=3, poll=0.5, deadline=600):
    batch = store.get(batch_id)
    todo = [it for it in batch["items"] if it["state"] == "cola"]
    with ThreadPoolExecutor(max_workers=max(1, concurrency)) as ex:
        list(ex.map(lambda it: _drive(batch_id, it, store, configure, status_of, poll, deadline), todo))
    return store.get(batch_id)


def retry_item(batch_id, key, store, configure, status_of, *, poll=0.5, deadline=600):
    batch = store.get(batch_id)
    it = next(i for i in batch["items"] if i["key"] == key)
    _drive(batch_id, it, store, configure, status_of, poll, deadline, suffix=f":retry{it.get('attempts', 0)}")
    return store.get(batch_id)


def batch_view(batch):
    items = [dict(key=i["key"], session=i["session"], pane=i["pane"], state=i["state"], error=i.get("error", ""),
                  to=i["to"], **{"from": i["from"]}, operationKey=i.get("operationKey", ""),
                  operationId=i.get("operationId", "")) for i in batch["items"]]
    done = sum(1 for i in items if i["state"] in TERMINAL)
    return dict(batchId=batch["batchId"], planId=batch["planId"], stateHash=batch["stateHash"],
                state="terminado" if done == len(items) else "aplicando", done=done, total=len(items), items=items)
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `python3 -m pytest tests/test_allocation_batch.py -q`
Expected: 5 passed

- [ ] **Step 5: Commit**

```bash
git add lib/allocation_batch.py tests/test_allocation_batch.py
git commit -m "feat(allocation): runner de lote idempotente con estado por pane"
```

---

### Task 5: Trust heredado en `lib/claude_trust.py`

**Files:**
- Modify: `lib/claude_trust.py`
- Test: `tests/test_claude_trust.py`

**Interfaces:**
- Produces: `cwd_trusted_in(cwd: str, *, config_dir: str | None, home: str) -> bool`: `True` si `projects[cwd].hasTrustDialogAccepted` es `True` en `{home}/.claude.json` o en `{config_dir}/.claude.json`. Compara la ruta exacta y sus ancestros hasta el toplevel git (la regla que ya documenta el módulo).
- Produces: `inherit_cwd_trust(cwd: str, *, source_config_dir: str | None, dest_config_dir: str, home: str) -> bool`: si `cwd_trusted_in(cwd, config_dir=source_config_dir, home=home)` → `ensure_cwd_trusted(cwd, config_dir=dest_config_dir, home=home)` y devuelve `True`; si no, no escribe nada y devuelve `False`. Nunca para `cwd == home`.

- [ ] **Step 1: Write the failing tests** (añadir a `tests/test_claude_trust.py`)

```python
def _write_trust(path, cwd):
    path.parent.mkdir(parents=True, exist_ok=True)
    data = json.loads(path.read_text()) if path.exists() else {}
    data.setdefault("projects", {})[cwd] = {"hasTrustDialogAccepted": True}
    path.write_text(json.dumps(data))

def test_inherit_copies_trust_only_when_source_had_it(tmp_path):
    home, src, dst = tmp_path / "home", tmp_path / "home/.claude-accounts/main", tmp_path / "home/.claude-accounts/relotto"
    cwd = str(tmp_path / "repo"); (tmp_path / "repo").mkdir()
    for d in (home, src, dst): d.mkdir(parents=True, exist_ok=True)
    assert claude_trust.inherit_cwd_trust(cwd, source_config_dir=str(src), dest_config_dir=str(dst), home=str(home)) is False
    assert not (dst / ".claude.json").exists()
    _write_trust(src / ".claude.json", cwd)
    assert claude_trust.inherit_cwd_trust(cwd, source_config_dir=str(src), dest_config_dir=str(dst), home=str(home)) is True
    assert json.loads((dst / ".claude.json").read_text())["projects"][cwd]["hasTrustDialogAccepted"] is True
    assert json.loads((home / ".claude.json").read_text())["projects"][cwd]["hasTrustDialogAccepted"] is True

def test_inherit_accepts_home_trust_as_source(tmp_path):
    home, dst = tmp_path / "home", tmp_path / "home/.claude-accounts/relotto"
    cwd = str(tmp_path / "repo"); (tmp_path / "repo").mkdir(); dst.mkdir(parents=True)
    _write_trust(home / ".claude.json", cwd)
    assert claude_trust.inherit_cwd_trust(cwd, source_config_dir=None, dest_config_dir=str(dst), home=str(home)) is True

def test_inherit_never_for_home_itself(tmp_path):
    home = tmp_path / "home"; home.mkdir()
    _write_trust(home / ".claude.json", str(home))
    assert claude_trust.inherit_cwd_trust(str(home), source_config_dir=None, dest_config_dir=str(home / "x"), home=str(home)) is False
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python3 -m pytest tests/test_claude_trust.py -q -k inherit`
Expected: FAIL con `AttributeError: ... 'inherit_cwd_trust'`

- [ ] **Step 3: Write minimal implementation** (añadir a `lib/claude_trust.py`; usa `_candidates_for(cwd)` si el módulo ya tiene la lista de ancestros; si no, replicar: la ruta y sus padres hasta el que contiene `.git`, sin pasar de `home`)

```python
def _ancestors_until_git(cwd: str, home: str) -> list[str]:
    out, cur = [], os.path.abspath(cwd)
    home = os.path.abspath(home)
    while True:
        out.append(cur)
        if os.path.isdir(os.path.join(cur, ".git")) or cur in (home, os.path.dirname(cur)):
            return out
        cur = os.path.dirname(cur)


def _trusted_in_file(path: str, cwd: str, home: str) -> bool:
    try:
        with open(path) as f:
            projects = (json.load(f) or {}).get("projects") or {}
    except Exception:
        return False
    return any(bool((projects.get(p) or {}).get("hasTrustDialogAccepted")) for p in _ancestors_until_git(cwd, home))


def cwd_trusted_in(cwd: str, *, config_dir: str | None, home: str) -> bool:
    files = [os.path.join(home, ".claude.json")]
    if config_dir:
        files.append(os.path.join(config_dir, ".claude.json"))
    return any(_trusted_in_file(p, cwd, home) for p in files)


def inherit_cwd_trust(cwd: str, *, source_config_dir: str | None, dest_config_dir: str, home: str) -> bool:
    if os.path.abspath(cwd) == os.path.abspath(home):
        return False
    if not cwd_trusted_in(cwd, config_dir=source_config_dir, home=home):
        return False
    ensure_cwd_trusted(cwd, config_dir=dest_config_dir, home=home)
    return True
```

Comprobar que `ensure_cwd_trusted` escribe en `{home}/.claude.json` **y** en `{config_dir}/.claude.json` cuando `config_dir` se pasa (así está documentado en el módulo); si solo escribe HOME, añadir la escritura en el config dir con `_stamp`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `python3 -m pytest tests/test_claude_trust.py -q`
Expected: todos pasan (los 10 existentes + 3 nuevos)

- [ ] **Step 5: Commit**

```bash
git add lib/claude_trust.py tests/test_claude_trust.py
git commit -m "feat(trust): inherit_cwd_trust copia la aceptación de carpeta de la cuenta origen a la destino"
```

---

### Task 6: Cablear trust heredado en el coordinador y en los lanzadores

**Files:**
- Modify: `bin/cc-dash` — `SessionConfiguration.apply` (`bin/cc-dash:2037`, justo antes de `_send_configuration_command`), `/session-new` (`bin/cc-dash:~8686`, antes de `threading.Thread(target=_send...)`), `tmux_new_session` (`bin/cc-dash:4663`, usado por `/up` y `/recover-tab`).
- Test: `tests/test_allocation_dash.py` (nuevo) con `load_dash_module`.

**Interfaces:**
- Consumes: `claude_trust.inherit_cwd_trust` (Task 5), `account_environment(...)`/`accounts_root` del registro para resolver `config_dir` de un alias: usar la función existente que devuelve el home de una cuenta (buscar `def account_home` o el uso de `accountsRoot` en `list_accounts`; si no existe, añadir `def _claude_config_dir(alias) -> str | None` que devuelve `None` para `main` y `{accountsRoot}/{alias}` para el resto).
- Produces: `def inherit_trust_for_switch(cwd, from_alias, to_alias) -> bool` en cc-dash, que solo actúa cuando el harness es `claude`, `to_alias != from_alias`, y registra `usage_changes` con `kind="trust_inherited"` vía `cc_usage.record_change` si existe (si no, `log("trust heredado ...")`).

- [ ] **Step 1: Write the failing tests**

```python
# tests/test_allocation_dash.py
import json, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
from test_agent_launch import load_dash_module

def test_inherit_trust_for_switch_only_between_claude_accounts(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    dash = load_dash_module()
    calls = []
    monkeypatch.setattr(dash.claude_trust, "inherit_cwd_trust",
                        lambda cwd, **kw: calls.append((cwd, kw)) or True)
    monkeypatch.setattr(dash, "_claude_config_dir", lambda alias: None if alias == "main" else f"{tmp_path}/.claude-accounts/{alias}")
    assert dash.inherit_trust_for_switch("/repo", "main", "relotto", harness="claude") is True
    assert calls[0][0] == "/repo" and calls[0][1]["dest_config_dir"].endswith("/relotto") and calls[0][1]["source_config_dir"] is None
    assert dash.inherit_trust_for_switch("/repo", "main", "main", harness="claude") is False
    assert dash.inherit_trust_for_switch("/repo", "main", "work", harness="codex") is False
    assert len(calls) == 1
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python3 -m pytest tests/test_allocation_dash.py -q`
Expected: FAIL con `AttributeError: ... 'inherit_trust_for_switch'`

- [ ] **Step 3: Write minimal implementation** (en `bin/cc-dash`, junto a los imports de `lib`: `import claude_trust`; y cerca de `session_change_support`)

```python
def _claude_config_dir(alias):
    if not alias or alias == "main":
        return None
    spec = (load_provider_registry().get("harnesses") or {}).get("claude") or {}
    root = os.path.expanduser(spec.get("accountsRoot") or "~/.claude-accounts")
    return os.path.join(root, alias)


def inherit_trust_for_switch(cwd, from_alias, to_alias, harness="claude"):
    """Copia la aceptación de carpeta de la cuenta origen a la destino (solo Claude, solo si cambia)."""
    if harness != "claude" or not cwd or (to_alias or "main") == (from_alias or "main"):
        return False
    dest = _claude_config_dir(to_alias)
    if not dest:
        return False
    ok = claude_trust.inherit_cwd_trust(cwd, source_config_dir=_claude_config_dir(from_alias),
                                        dest_config_dir=dest, home=os.path.expanduser("~"))
    if ok:
        log(f"trust heredado {from_alias or 'main'} → {to_alias} en {cwd}")
    return ok
```

Y los tres cableados:

1. En `SessionConfiguration.apply(self, plan, snapshot)`, antes de `_send_configuration_command(...)`:
   ```python
   inherit_trust_for_switch(self.cwd, (snapshot.get('origin') or {}).get('harnessAccount') or self.current.get('harnessAccount'),
                            plan.get('harnessAccount'), harness=plan.get('toHarness') or self.to)
   ```
   (usar los nombres reales de atributos: leer `prepare()` para saber dónde guarda `cwd`, `current` y `to`; si no hay `self.cwd`, usar `self.identity.get('pane_current_path')`).
2. En `/session-new`, antes del `threading.Thread(target=_send...)`: `if route and route["harness"] == "claude": inherit_trust_for_switch(final_cwd, "main", alias, harness="claude")`.
3. En `tmux_new_session(sess, cwd, agent)`: si `agent == "claude"` y la sesión se revive con una cuenta distinta de `main` (leer el alias de `read_tab_metadata(sess)` si existe; si no hay alias, no hacer nada).

- [ ] **Step 4: Run tests to verify they pass**

Run: `python3 -m pytest tests/test_allocation_dash.py tests/test_session_route_matrix.py tests/test_claude_trust.py -q`
Expected: todos pasan. Si `test_harness_switch_leaves_trust_dialog_unconfirmed_without_enter` o `test_launch_commands_do_not_persist_workspace_trust` fallan porque ahora sí se escribe trust: esos tests fijan que **no se acepta trust que el origen no tenía**; comprobar que en su fixture el origen no tiene la carpeta aceptada (entonces `inherit` devuelve `False` y no escribe). Si el fixture sí la tiene en origen, actualizar el test para afirmar la herencia (es el nuevo comportamiento).

- [ ] **Step 5: Commit**

```bash
git add bin/cc-dash tests/test_allocation_dash.py tests/test_claude_trust.py tests/test_session_route_matrix.py
git commit -m "feat(coordinator): hereda el trust de carpeta al cambiar de cuenta y en los lanzadores"
```

---

### Task 7: Diálogos desde config y ventana de verificación

**Files:**
- Modify: `config/detectors.json` (añadir claves), `bin/cc-dash:2099` (`_verify`) y `pending_confirmation` (`bin/cc-dash:~2119`), firma `_verify(..., attempts=60)`.
- Test: `tests/test_session_verify_dialogs.py`

**Interfaces:**
- Produces: `dialog_patterns() -> list[str]` (lee `config/detectors.json["dialogPatterns"]`, con fallback a la lista actual), `screen_dialog(screen: str) -> str` (`"trust"|"login"|"onboarding"|"error"|""`), `verify_attempts(cwd, config_dir) -> int` (`detectors["verifyAttemptsWithMcp"]` (180) si hay `.mcp.json` en `cwd` o `mcpServers` no vacío en `{config_dir or HOME}/.claude.json`; si no, 60).
- `session_operation_status` añade `screenDialog` cuando el estado es `awaiting_confirmation` (captura la pantalla y aplica `screen_dialog`).

- [ ] **Step 1: Write the failing tests**

```python
# tests/test_session_verify_dialogs.py
import json, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
from test_agent_launch import load_dash_module

def test_screen_dialog_detects_english_and_spanish():
    dash = load_dash_module()
    assert dash.screen_dialog("Do you trust the files in this folder?") == "trust"
    assert dash.screen_dialog("¿Confías en los archivos de esta carpeta?") == "trust"
    assert dash.screen_dialog("Please sign in to continue") == "login"
    assert dash.screen_dialog("Choose the text style that looks best") == "onboarding"
    assert dash.screen_dialog("error: cannot resume session") == "error"
    assert dash.screen_dialog("❯ ") == ""

def test_verify_attempts_grow_with_mcp(tmp_path, monkeypatch):
    dash = load_dash_module()
    monkeypatch.setenv("HOME", str(tmp_path))
    repo = tmp_path / "repo"; repo.mkdir()
    assert dash.verify_attempts(str(repo), None) == 60
    (repo / ".mcp.json").write_text("{}")
    assert dash.verify_attempts(str(repo), None) == 180
    (repo / ".mcp.json").unlink()
    (tmp_path / ".claude.json").write_text(json.dumps({"mcpServers": {"x": {}}}))
    assert dash.verify_attempts(str(repo), None) == 180
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python3 -m pytest tests/test_session_verify_dialogs.py -q`
Expected: FAIL con `AttributeError: ... 'screen_dialog'`

- [ ] **Step 3: Write minimal implementation**

`config/detectors.json`: añadir al objeto raíz

```json
"dialogPatterns": {
  "trust": ["trust this folder", "Do you trust", "Accessing workspace", "Conf[ií]as en los archivos", "carpeta de confianza"],
  "login": ["sign in", "log in", "login required", "iniciar sesi[oó]n", "inicia sesi[oó]n"],
  "onboarding": ["Choose the text style", "Elige el estilo de texto", "Welcome to Claude Code", "Let's get started"],
  "error": ["^\\s*error:", "fatal:"]
},
"verifyAttemptsWithMcp": 180
```

`bin/cc-dash` (cerca de `load_model_tiers`):

```python
_DEFAULT_DIALOGS = {"trust": [r"trust this folder", r"Do you trust", r"Accessing workspace"],
                    "login": [r"sign in", r"log in", r"login required"],
                    "onboarding": [r"Choose the text style"], "error": [r"^\s*error:", r"fatal:"]}


def dialog_patterns():
    try:
        with open(os.path.join(REPO_DIR, "config", "detectors.json")) as f:
            pats = json.load(f).get("dialogPatterns") or {}
        return {k: list(v) for k, v in pats.items()} or _DEFAULT_DIALOGS
    except Exception:
        return _DEFAULT_DIALOGS


def screen_dialog(screen):
    for kind, pats in dialog_patterns().items():
        for p in pats:
            if re.search(p, screen or "", re.I | re.M):
                return kind
    return ""


def verify_attempts(cwd, config_dir):
    try:
        with open(os.path.join(REPO_DIR, "config", "detectors.json")) as f:
            with_mcp = int(json.load(f).get("verifyAttemptsWithMcp") or 180)
    except Exception:
        with_mcp = 180
    if cwd and os.path.exists(os.path.join(cwd, ".mcp.json")):
        return with_mcp
    try:
        with open(os.path.join(config_dir or os.path.expanduser("~"), ".claude.json")) as f:
            if (json.load(f).get("mcpServers") or {}):
                return with_mcp
    except Exception:
        pass
    return 60
```

(`REPO_DIR` es como cc-dash localiza `config/`; usar la misma variable que usa `load_model_tiers`/`optimization_plans`.)

En `_verify` (`bin/cc-dash:2099`): sustituir el `re.search(r'trust this folder|...', screen, re.I)` por `if screen_dialog(screen): continue`. En `verify(self, plan, snapshot)`: `return self._verify(plan, plan.get('expectedSid',''), attempts=verify_attempts(self.cwd, _claude_config_dir(plan.get('harnessAccount'))))`. En `pending_confirmation`: capturar pantalla y, si `screen_dialog(screen) in ("trust","login","onboarding")`, devolver el dict de `awaiting_confirmation` con `screenDialog=<kind>` en vez de solo mirar `error:|fatal:`. En `session_operation_status`, cuando el estado sea `awaiting_confirmation`, añadir `screenDialog` capturando el pane.

- [ ] **Step 4: Run tests to verify they pass**

Run: `python3 -m pytest tests/test_session_verify_dialogs.py tests/test_session_route_matrix.py tests/test_session_operations.py -q`
Expected: todos pasan (el test `test_trust_or_login_dialog_never_confirms_or_receives_input` debe seguir verde: `screen_dialog` cubre sus tres cadenas).

- [ ] **Step 5: Commit**

```bash
git add config/detectors.json bin/cc-dash tests/test_session_verify_dialogs.py
git commit -m "feat(coordinator): diálogos de trust/login/onboarding desde config (en/es) y verificación de 90 s con MCPs"
```

---

### Task 8: Endpoints `/allocation/propose` y `/allocation/preview`, limits enriquecidos

**Files:**
- Modify: `bin/cc-dash` — `/usage/state` (`bin/cc-dash:7902`), nuevos handlers en `do_POST` (junto a `"/optimization/default"`, `bin/cc-dash:8116`) y `do_GET`.
- Test: `tests/test_allocation_dash.py`

**Interfaces:**
- Produces en cc-dash: `allocation_inputs() -> dict(sessions, limits, registry, support, accounts, tiers, now)` que junta `read_states_cached()` (filtrar `alive` y `agent in registry["motors"]`), `usage_provider_limits()["limits"]`, `load_provider_registry()`, `session_change_support(registry)`, cuentas seleccionables por motor (`{m: [a["alias"] for a in list_accounts(registry, m) if a.get("selectable")]}`; buscar la firma real de `list_accounts` en `lib/accounts.py`), `load_model_tiers()`, `time.time()`.
- Produces: `POST /allocation/propose {overrides?}` → `{ok, plan}`; guarda el plan en `ALLOCATION_PLANS[planId]` (dict en memoria, máx. 20).
- Produces: `POST /allocation/preview {planId, key, target: {layer}|{to}}` → `{ok, impact}`: recalcula `propose` con el override extra sin guardar.
- Produces: `/usage/state.limits[]` llevan `pace, burn, runsOutIn, reachesReset, verdict` (`allocation.enrich_limits`).

- [ ] **Step 1: Write the failing tests** (añadir a `tests/test_allocation_dash.py`)

```python
def _fake_inputs(dash, monkeypatch, tmp_path):
    from test_allocation import REGISTRY, TIERS, SUPPORT, ACCOUNTS, LIMITS, sessions, NOW
    monkeypatch.setattr(dash, "read_states_cached", lambda *a, **k: [dict(s, alive=True) for s in sessions()])
    monkeypatch.setattr(dash, "usage_provider_limits", lambda *a, **k: {"limits": LIMITS, "health": {}})
    monkeypatch.setattr(dash, "load_provider_registry", lambda: REGISTRY)
    monkeypatch.setattr(dash, "session_change_support", lambda *a, **k: SUPPORT)
    monkeypatch.setattr(dash, "load_model_tiers", lambda: TIERS)
    monkeypatch.setattr(dash, "selectable_accounts_by_motor", lambda registry: ACCOUNTS)
    monkeypatch.setattr(dash.time, "time", lambda: NOW)

def test_propose_endpoint_builds_plan_from_live_inputs(tmp_path, monkeypatch):
    dash = load_dash_module(); _fake_inputs(dash, monkeypatch, tmp_path)
    code, body = dash.allocation_propose({})
    assert code == 200 and body["ok"] and body["plan"]["items"]
    assert body["plan"]["planId"] in dash.ALLOCATION_PLANS
    assert all(i["to"]["routeId"] != "acp:claude" for i in body["plan"]["items"])

def test_preview_recomputes_impact_without_storing(tmp_path, monkeypatch):
    dash = load_dash_module(); _fake_inputs(dash, monkeypatch, tmp_path)
    _, body = dash.allocation_propose({})
    pid = body["plan"]["planId"]
    code, prev = dash.allocation_preview({"planId": pid, "key": "LifeOS|%18", "target": {"layer": 3}})
    assert code == 200 and "impact" in prev and "claude:main" in prev["impact"]
    assert len(dash.ALLOCATION_PLANS) == 1

def test_usage_state_limits_are_enriched(tmp_path, monkeypatch):
    dash = load_dash_module(); _fake_inputs(dash, monkeypatch, tmp_path)
    from test_allocation import LIMITS, NOW
    rows = dash.enriched_limits(LIMITS)
    assert rows[0]["burn"] and rows[0]["verdict"]
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python3 -m pytest tests/test_allocation_dash.py -q`
Expected: FAIL con `AttributeError: ... 'allocation_propose'`

- [ ] **Step 3: Write minimal implementation** (en `bin/cc-dash`; `import allocation` junto a los otros imports de `lib`)

```python
ALLOCATION_PLANS = {}


def selectable_accounts_by_motor(registry):
    out = {}
    for motor in (registry.get("motors") or {}):
        try:
            out[motor] = [a["alias"] for a in list_accounts(registry, motor) if a.get("selectable")] or ["main"]
        except Exception:
            out[motor] = ["main"]
    return out


def enriched_limits(limits):
    return allocation.enrich_limits(limits, time.time())


def allocation_inputs():
    registry = load_provider_registry()
    motors = registry.get("motors") or {}
    sessions = [s for s in read_states_cached() if s.get("alive") and s.get("agent") in motors]
    return dict(sessions=sessions, limits=usage_provider_limits()["limits"], registry=registry,
                support=session_change_support(registry), accounts=selectable_accounts_by_motor(registry),
                tiers=load_model_tiers(), now=time.time())


def allocation_propose(data):
    inp = allocation_inputs()
    plan = allocation.propose(inp["sessions"], inp["limits"], inp["registry"], inp["support"], inp["accounts"],
                              inp["tiers"], now=inp["now"], overrides=data.get("overrides") or {})
    plan["overrides"] = data.get("overrides") or {}
    ALLOCATION_PLANS[plan["planId"]] = plan
    while len(ALLOCATION_PLANS) > 20:
        ALLOCATION_PLANS.pop(next(iter(ALLOCATION_PLANS)))
    return 200, {"ok": True, "plan": plan}


def allocation_preview(data):
    plan = ALLOCATION_PLANS.get(str(data.get("planId") or ""))
    if not plan:
        return 404, {"ok": False, "error": "plan desconocido; vuelve a analizar"}
    overrides = dict(plan.get("overrides") or {})
    overrides[str(data.get("key") or "")] = data.get("target") or {}
    inp = allocation_inputs()
    tmp = allocation.propose(inp["sessions"], inp["limits"], inp["registry"], inp["support"], inp["accounts"],
                             inp["tiers"], now=inp["now"], overrides=overrides)
    return 200, {"ok": True, "impact": tmp["impact"], "items": tmp["items"]}
```

Dispatch en `do_POST` (junto a `/optimization/default`):

```python
        if self.path == "/allocation/propose":
            return self._json(*allocation_propose(data))
        if self.path == "/allocation/preview":
            return self._json(*allocation_preview(data))
```

En `/usage/state`: `state["limits"] = enriched_limits(state.get("limits") or prov["limits"])` justo después de `cached_usage_state(...)`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `python3 -m pytest tests/test_allocation_dash.py tests/test_usage_dash.py -q`
Expected: todos pasan.

- [ ] **Step 5: Commit**

```bash
git add bin/cc-dash tests/test_allocation_dash.py
git commit -m "feat(dash): /allocation/propose y /allocation/preview; limits con ritmo y proyección"
```

---

### Task 9: Endpoints `/allocation/apply`, `/allocation/status`, `/allocation/retry`, `/allocation/revert`

**Files:**
- Modify: `bin/cc-dash`
- Test: `tests/test_allocation_dash.py`

**Interfaces:**
- Consumes: `allocation_batch.BatchStore`, `run_batch`, `retry_item`, `batch_view`; `session_configure(data) -> (code, body)`; `session_operation_status(opkey, opid)` (firma real en `bin/cc-dash:6024`: comprobar y adaptar el lambda).
- Produces: `allocation_apply(data) -> (code, body)`: `409 {"error": "plan_stale", "changed": [...]}` si `allocation.plan_hash(sessions vivas) != plan["stateHash"]`; si no crea el batch en `ALLOCATION_BATCHES` (`BatchStore(HOOKS_DIR/"allocation-batches.json")`) y lanza `run_batch` en un `threading.Thread(daemon=True)`; devuelve `202 {"ok": True, "batchId"}`.
- Produces: `allocation_status(batch_id) -> (200, batch_view)`; `allocation_retry({batchId, key})` → `202`; `allocation_revert({batchId})` → crea plan `allocation.invert(plan)` solo con los items `lista` del batch, lo guarda en `ALLOCATION_PLANS` y aplica igual que `apply` (sin comprobar `stateHash`, porque el estado ya cambió a propósito).

- [ ] **Step 1: Write the failing tests** (añadir a `tests/test_allocation_dash.py`)

```python
def _fake_coordinator(dash, monkeypatch):
    calls = []
    def configure(data):
        calls.append(data)
        key = f"{data['session']}|{data['pane']}"
        return 202, {"ok": True, "pending": True, "operationKey": key, "operationId": "op"}
    monkeypatch.setattr(dash, "session_configure", configure)
    monkeypatch.setattr(dash, "session_operation_status", lambda opkey, opid=None, **k: {"state": "confirmed", "ok": True})
    return calls

def test_apply_runs_batch_and_status_reports_items(tmp_path, monkeypatch):
    dash = load_dash_module(); _fake_inputs(dash, monkeypatch, tmp_path)
    calls = _fake_coordinator(dash, monkeypatch)
    monkeypatch.setattr(dash, "ALLOCATION_BATCHES", dash.allocation_batch.BatchStore(tmp_path / "b.json"))
    _, body = dash.allocation_propose({})
    code, res = dash.allocation_apply({"planId": body["plan"]["planId"]}, wait=True)
    assert code == 202 and res["batchId"]
    _, st = dash.allocation_status(res["batchId"])
    assert st["state"] == "terminado" and all(i["state"] in ("lista", "omitida") for i in st["items"])
    assert all(c["interrupt"] is True and c["requestId"].startswith(res["batchId"] + ":") for c in calls)

def test_apply_rejects_stale_plan(tmp_path, monkeypatch):
    dash = load_dash_module(); _fake_inputs(dash, monkeypatch, tmp_path)
    _fake_coordinator(dash, monkeypatch)
    _, body = dash.allocation_propose({})
    from test_allocation import sessions
    changed = [dict(s, alive=True) for s in sessions()]; changed[0]["effort"] = "low"
    monkeypatch.setattr(dash, "read_states_cached", lambda *a, **k: changed)
    code, res = dash.allocation_apply({"planId": body["plan"]["planId"]}, wait=True)
    assert code == 409 and res["error"] == "plan_stale" and changed[0]["session"] in " ".join(res["changed"])

def test_revert_applies_inverse_of_confirmed_items(tmp_path, monkeypatch):
    dash = load_dash_module(); _fake_inputs(dash, monkeypatch, tmp_path)
    calls = _fake_coordinator(dash, monkeypatch)
    monkeypatch.setattr(dash, "ALLOCATION_BATCHES", dash.allocation_batch.BatchStore(tmp_path / "b.json"))
    _, body = dash.allocation_propose({})
    _, res = dash.allocation_apply({"planId": body["plan"]["planId"]}, wait=True)
    n_forward = len(calls)
    code, rev = dash.allocation_revert({"batchId": res["batchId"]}, wait=True)
    assert code == 202
    back = calls[n_forward:]
    assert back and all(c["requestId"].startswith(rev["batchId"] + ":") for c in back)
    forward_by_key = {f"{c['session']}|{c['pane']}": c for c in calls[:n_forward]}
    for c in back:
        f = forward_by_key[f"{c['session']}|{c['pane']}"]
        assert (c["model"], c["effort"]) != (f["model"], f["effort"]) or c["harnessAccount"] != f["harnessAccount"]
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python3 -m pytest tests/test_allocation_dash.py -q -k "apply or revert"`
Expected: FAIL con `AttributeError: ... 'allocation_apply'`

- [ ] **Step 3: Write minimal implementation** (en `bin/cc-dash`; `import allocation_batch`)

```python
ALLOCATION_BATCHES = None  # BatchStore, se crea perezosamente


def _batches():
    global ALLOCATION_BATCHES
    if ALLOCATION_BATCHES is None:
        ALLOCATION_BATCHES = allocation_batch.BatchStore(os.path.join(HOOKS_DIR, "allocation-batches.json"))
    return ALLOCATION_BATCHES


def _status_of(opkey, opid):
    try:
        return session_operation_status(opkey, opid)
    except TypeError:
        return session_operation_status(opkey)


def _run_batch_bg(batch_id, wait):
    def go():
        allocation_batch.run_batch(batch_id, _batches(), session_configure, _status_of, concurrency=3)
    if wait:
        go()
    else:
        threading.Thread(target=go, daemon=True).start()


def allocation_apply(data, wait=False):
    plan = ALLOCATION_PLANS.get(str(data.get("planId") or ""))
    if not plan:
        return 404, {"ok": False, "error": "plan desconocido; vuelve a analizar"}
    inp = allocation_inputs()
    if allocation.plan_hash(inp["sessions"]) != plan["stateHash"]:
        before = {i["key"]: i["from"] for i in plan["items"]}
        changed = [allocation.session_key(s) for s in inp["sessions"]
                   if allocation.session_key(s) not in before
                   or any(str(s.get(f) or "") != str(before[allocation.session_key(s)].get(g) or "")
                          for f, g in (("model", "model"), ("effort", "effort"), ("harnessAccount", "account")))]
        gone = [k for k in before if k not in {allocation.session_key(s) for s in inp["sessions"]}]
        return 409, {"ok": False, "error": "plan_stale", "changed": changed + gone}
    batch = _batches().create(plan, panes=data.get("panes"))
    _run_batch_bg(batch["batchId"], wait)
    return 202, {"ok": True, "batchId": batch["batchId"], "total": len(batch["items"])}


def allocation_status(batch_id):
    b = _batches().get(str(batch_id or ""))
    if not b:
        return 404, {"ok": False, "error": "lote desconocido"}
    return 200, dict(ok=True, **allocation_batch.batch_view(b))


def allocation_retry(data, wait=False):
    bid, key = str(data.get("batchId") or ""), str(data.get("key") or "")
    if not _batches().get(bid):
        return 404, {"ok": False, "error": "lote desconocido"}
    def go():
        allocation_batch.retry_item(bid, key, _batches(), session_configure, _status_of)
    (go() if wait else threading.Thread(target=go, daemon=True).start())
    return 202, {"ok": True, "batchId": bid, "key": key}


def allocation_revert(data, wait=False):
    b = _batches().get(str(data.get("batchId") or ""))
    if not b:
        return 404, {"ok": False, "error": "lote desconocido"}
    plan = ALLOCATION_PLANS.get(b["planId"])
    if not plan:
        return 409, {"ok": False, "error": "el plan original ya no está en memoria"}
    done = {i["key"] for i in b["items"] if i["state"] == "lista"}
    inv = allocation.invert(plan)
    inv["items"] = [i for i in inv["items"] if i["key"] in done]
    for i in inv["items"]:
        i["same"] = False
    ALLOCATION_PLANS[inv["planId"]] = inv
    batch = _batches().create(inv, panes=None)
    _run_batch_bg(batch["batchId"], wait)
    return 202, {"ok": True, "batchId": batch["batchId"], "total": len(batch["items"])}
```

Dispatch: en `do_POST` añadir `/allocation/apply`, `/allocation/retry`, `/allocation/revert`; en `do_GET` `if self.path.startswith("/allocation/status"): return self._json(*allocation_status(urllib.parse.parse_qs(...).get("batchId", [""])[0]))`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `python3 -m pytest tests/test_allocation_dash.py tests/test_allocation_batch.py -q`
Expected: todos pasan.

- [ ] **Step 5: Commit**

```bash
git add bin/cc-dash tests/test_allocation_dash.py
git commit -m "feat(dash): aplicar, estado, reintento y revert del reparto en lote"
```

---

### Task 10: Tablero — pestaña Reparto (tanques, fichas, arrastre, pie)

**Files:**
- Create: `dash/reparto.js`, `dash/reparto.css`
- Modify: `dash/index.html` — línea 2250 (`data-mtab="optimizar"` → `reparto`, icono `scale`), líneas 2311–2317 (pane optimizar → `<div class="mtab-pane" data-mpane="reparto"><div id="reparto"></div></div>`), 1907 (`<link rel="stylesheet" href="/reparto.css">`), 9019 (`<script src="/reparto.js"></script>` antes de workspace.js), 7809 (`MTAB_GROUPS.reparto: ["reparto"]`), `activateMtab` (llamar `window.Reparto?.load()` al activar `reparto`), borrar `loadOptimize`, `optimizeSessions`, `renderOptimize`, `applyOptimization` y sus listeners (3818–3855) y el objeto `OPTIMIZE`.
- Test: `tests/test_dashboard_reparto.py`, `tests/test_js_parses.sh` (añadir `node --check dash/reparto.js`).

**Interfaces:**
- Consumes: `api(path, body)` global del tablero (`dash/index.html:2595`), `S.list` (filas de `/state`), endpoints de Task 8 y 9.
- Produces: `window.Reparto = { load(), render(), state }`. Estados de la pestaña: `idle` (sin plan: solo tanques con la carga actual y botón **Analizar**), `curar` (plan cargado), `aplicando` (batch en curso), `terminado`.
- DOM: `#reparto` contiene `.rp-tanks` (4 `.tank[data-pool]`), `.rp-bins` (un `.bin[data-pool]` por tanque con `.tk[data-key]`), `.rp-foot`, `.rp-progress`, `.rp-dropbar`.

- [ ] **Step 1: Write the failing tests**

```python
# tests/test_dashboard_reparto.py
import re, subprocess
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
HTML = (ROOT / "dash/index.html").read_text()
JS = (ROOT / "dash/reparto.js").read_text() if (ROOT / "dash/reparto.js").exists() else ""

def test_reparto_tab_replaces_optimizar():
    assert 'data-mtab="reparto"' in HTML and 'data-mpane="reparto"' in HTML
    assert 'data-mtab="optimizar"' not in HTML and "applyOptimization" not in HTML
    assert 'src="/reparto.js"' in HTML and 'href="/reparto.css"' in HTML
    assert re.search(r"MTAB_GROUPS\s*=\s*\{[^}]*reparto:\s*\[\"reparto\"\]", HTML)

def test_reparto_js_parses_and_exposes_api():
    assert "window.Reparto" in JS
    subprocess.run(["node", "--check", str(ROOT / "dash/reparto.js")], check=True)

def test_copy_is_plain_language():
    banned = ["aguanta", ">F<", ">M<", ">L<"]
    for word in banned:
        assert word not in JS, word
    for phrase in ["llega al reset", "se acaba en", "ritmo", "Analizar", "Aplicar", "Propuesta original"]:
        assert phrase in JS, phrase

def test_reparto_css_uses_container_queries():
    css = (ROOT / "dash/reparto.css").read_text()
    assert "container-type" in css and "@container" in css and ".rp-dropbar" in css
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python3 -m pytest tests/test_dashboard_reparto.py -q`
Expected: FAIL (archivos inexistentes / markup viejo).

- [ ] **Step 3: Write the implementation**

`dash/reparto.css` — portar del prototipo `dash/prototypes/reparto-round8-final.html` (rama `prototype/reparto-tanques`) los bloques: `.tank` con `.base/.liq.layer/.pre/.foam/.cap/.lvl/.reset`, `.tk` (ficha con `.diff.sel`, `.mini`, `.dot`, `.moved`, `.armed`, `.locked`), `.rp-foot`, `.go`, `.ghostb`, `.rp-progress` (`.pb`, `.steps`), `.rp-dropbar` y las `@container panel (max-width:620px)` / `(max-width:440px)`. Prefijar clases con `rp-` donde choquen con las del tablero (`.tank`, `.tk`, `.go` no existen en index.html; comprobar con `grep -c "\.go{" dash/index.html`). Usar solo variables del tema (`--bg --panel --panel2 --line --line2 --text --dim --faint --brand --on-brand --ok --warn --err --working`); el líquido usa `var(--brand)` con `opacity:.55`.

`dash/reparto.js` — estructura (código completo; los renders portan el HTML del prototipo):

```js
// dash/reparto.js — pestaña Reparto: tanques de cuota, fichas arrastrables, propuesta, lote.
(function(){
  const $ = (s, r=document) => r.querySelector(s);
  const state = { phase:"idle", plan:null, overrides:{}, batch:null, armed:null, drag:null, poll:null, limits:[] };
  const WEIGHT = {low:.5, medium:1, high:1.6, xhigh:2.2, max:3};
  const fmtH = s => s==null ? "" : s<3600 ? `${Math.round(s/60)} min` : s<48*3600 ? `${Math.round(s/3600)} h` : `${Math.floor(s/86400)}d ${Math.round((s%86400)/3600)}h`;
  const esc = s => String(s??"").replace(/[&<>"']/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c]));
  const pv = a => `<span class="rp-pv ${esc(a)}">${{claude:"C",codex:"X",grok:"G"}[a]||"?"}</span>`;
  const key = it => it.key;
  const same = it => it.same;

  async function load(){
    const st = await api("/usage/state");
    state.limits = st.limits || [];
    if(!state.plan){ state.phase = "idle"; }
    render();
  }
  async function analyze(){
    const r = await api("/allocation/propose", {overrides: state.overrides});
    state.plan = r.plan; state.phase = "curar"; render();
  }
  async function preview(k, target){
    if(!state.plan) return;
    const r = await api("/allocation/preview", {planId: state.plan.planId, key: k, target});
    paintImpact(r.impact);
  }
  async function setOverride(k, target){
    state.overrides[k] = target;
    const r = await api("/allocation/propose", {overrides: state.overrides});
    state.plan = r.plan; render();
  }
  async function apply(){
    try{
      const r = await api("/allocation/apply", {planId: state.plan.planId});
      state.batch = {batchId: r.batchId}; state.phase = "aplicando"; render(); pollBatch();
    }catch(e){
      if(/plan_stale/.test(e.message)){ toast("Algo cambió en las sesiones; vuelve a analizar", true); state.plan=null; state.phase="idle"; render(); }
      else toast(e.message, true);
    }
  }
  function pollBatch(){
    clearInterval(state.poll);
    state.poll = setInterval(async()=>{
      const b = await api(`/allocation/status?batchId=${encodeURIComponent(state.batch.batchId)}`);
      state.batch = b; if(b.state === "terminado"){ clearInterval(state.poll); state.phase = "terminado"; }
      render();
    }, 800);
  }
  async function retry(k){ await api("/allocation/retry", {batchId: state.batch.batchId, key: k}); pollBatch(); }
  async function revert(){ const r = await api("/allocation/revert", {batchId: state.batch.batchId}); state.batch = {batchId:r.batchId}; state.phase="aplicando"; render(); pollBatch(); }

  // ---- impacto por cuota (pools) ----
  function pools(){
    const imp = state.plan ? state.plan.impact : {};
    const byPool = {};
    for(const l of state.limits){ if(l.window!=="7d") continue; const k = `${l.provider}:${l.account||"main"}`; if(!byPool[k] || l.kind==="weekly_all") byPool[k] = l; }
    return Object.entries(byPool).map(([k,l]) => ({k, label: `${l.provider} ${l.account||"main"}`, used: l.percent, burn: l.burn, verdict: l.verdict,
      resetsIn: (l.resets_at||0) - Date.now()/1000, after: imp[k] || null}));
  }
  function tankHtml(p){
    const a = p.after; const usedAfter = a && a.usedAfter!=null ? a.usedAfter : p.used;
    const sev = (a ? a.burnAfter : p.burn) > 1.15 ? "err" : (a ? a.burnAfter : p.burn) > .9 ? "warn" : "";
    const verdict = a ? a.verdict : p.verdict;
    return `<div class="rp-tank rp-drop ${usedAfter>=100?"full":""}" data-pool="${esc(p.k)}">
      <div class="cap">${esc(p.label)}<small data-verdict="${esc(p.k)}">${esc(verdict)} · ritmo <span data-burn="${esc(p.k)}">${(a?a.burnAfter:p.burn||0).toFixed(1)}x</span></small></div>
      <div class="base" style="--n:${p.used||0}%"></div>
      <div class="pre" data-pre="${esc(p.k)}" style="--h:${usedAfter}%"></div>
      <div class="liq layer ${sev}" data-liq="${esc(p.k)}" style="--h:${usedAfter}%"></div><div class="foam"></div>
      <div class="lvl num" data-lvl="${esc(p.k)}">${Math.round(usedAfter)}%</div>
      <div class="reset">reset en ${fmtH(p.resetsIn)}</div></div>`;
  }
  function paintImpact(imp){
    for(const [k, a] of Object.entries(imp||{})){
      const root = $("#reparto"); const liq = root.querySelector(`[data-liq="${CSS.escape(k)}"]`); if(!liq) continue;
      const sev = a.burnAfter>1.15?"err":a.burnAfter>.9?"warn":"";
      liq.style.setProperty("--h", (a.usedAfter??0)+"%"); liq.className = "liq layer "+sev;
      root.querySelector(`[data-pre="${CSS.escape(k)}"]`).style.setProperty("--h", (a.usedAfter??0)+"%");
      root.querySelector(`[data-lvl="${CSS.escape(k)}"]`).textContent = Math.round(a.usedAfter??0)+"%";
      root.querySelector(`[data-burn="${CSS.escape(k)}"]`).textContent = (a.burnAfter||0).toFixed(1)+"x";
      const v = root.querySelector(`[data-verdict="${CSS.escape(k)}"]`); v.firstChild.textContent = a.verdict+" · ritmo ";
      liq.closest(".rp-tank").classList.toggle("full", (a.usedAfter??0)>=100);
    }
  }

  // ---- fichas ----
  function tileHtml(it, i){
    const f = it.from, t = it.to; const bstate = batchState(it.key);
    const models = (state.plan.options||{})[t.motor]?.models || [t.model];
    const efforts = (state.plan.options||{})[t.motor]?.efforts || ["low","medium","high"];
    const sel = (list, cur, kind) => `<span class="rp-mini ${kind==="model"?(t.model!==f.model?"chg":""):(t.effort!==f.effort?"chg":"")}">${list.map(v=>`<button class="${v===cur?"on":""}" data-set="${esc(it.key)}|${kind}|${esc(v)}">${esc(v.replace("claude-","").replace("gpt-5.6-","").replace("gpt-6-",""))}</button>`).join("")}</span>`;
    return `<div class="rp-tk ${it.locked?"locked":""} ${state.overrides[it.key]?"moved":""} ${state.armed===it.key?"armed":""}" draggable="${it.locked?"false":"true"}" data-key="${esc(it.key)}">
      <span class="dot ${esc(it.status||"")}"></span>${pv(t.motor)}
      <span class="nm"><b>${esc(it.session)} <span class="rp-pane">${esc(it.pane)}</span></b>
        <span class="diff sel"><span class="old">${esc(f.model)} · ${esc(f.effort)}${f.account!==t.harnessAccount?" · "+esc(f.account):""}</span><span class="faint">→</span>
        <span class="new">${state.phase==="curar" ? sel(models, t.model, "model")+" "+sel(efforts, t.effort, "effort") : `${esc(t.model)} · ${esc(t.effort)}`}</span></span></span>
      ${state.phase==="curar" ? `<button class="lk ${it.locked?"on":""}" data-lock="${esc(it.key)}" title="fijar: no tocar esta sesión">🔒</button>` : ""}
      ${bstate ? `<span class="rp-st ${bstate.state}">${esc(stateLabel(bstate))}</span>` : ""}</div>`;
  }
  function batchState(k){ return state.batch && state.batch.items ? state.batch.items.find(i=>i.key===k) : null; }
  function stateLabel(b){ return {cola:"en cola", aplicando:"aplicando", lista:"lista", detenida:"detenida · "+(b.error||""), omitida:"sin cambio"}[b.state] || b.state; }

  // ---- render ----
  function render(){
    const root = $("#reparto"); if(!root) return;
    const P = pools(); const items = state.plan ? state.plan.items : [];
    const n = items.filter(i=>!same(i)&&!i.locked).length;
    let h = `<div class="rp-hint">Cada tanque es una cuota; la capa clara es lo que añadirán las sesiones hasta el reset. Suelta sesiones dentro para moverlas de cuenta o motor.</div>`;
    h += `<div class="rp-tanks">${P.map(tankHtml).join("")}</div>`;
    if(state.phase==="idle"){
      h += `<div class="rp-foot"><span class="rp-note">${(S.list||[]).filter(x=>x.alive).length} sesiones vivas</span><span class="sp"></span><button class="rp-go" data-analyze>Analizar</button></div>`;
    }else{
      h += `<div class="rp-bins">${P.map(p=>`<div class="rp-bin rp-drop" data-pool="${esc(p.k)}" data-label="${esc(p.label)}">${items.map((it,i)=>`${it.to.motor}:${it.to.harnessAccount}`===p.k?tileHtml(it,i):"").join("")}</div>`).join("")}</div>`;
      const unknown = items.filter(it => !P.some(p=>p.k===`${it.to.motor}:${it.to.harnessAccount}`));
      if(unknown.length) h += `<div class="rp-bin" data-label="sin cuota conocida">${unknown.map(tileHtml).join("")}</div>`;
      if(state.phase==="curar"){
        h += `<div class="rp-foot"><span class="rp-note">${n} cambios · ${items.length-n} igual${Object.keys(state.overrides).length?` · <b class="warn">${Object.keys(state.overrides).length} ajustadas por ti</b>`:""}</span><span class="sp"></span><button class="rp-ghost" data-reset>Propuesta original</button><button class="rp-go" data-apply ${n?"":"disabled"}>Aplicar ${n}</button></div>`;
      }else{
        const b = state.batch || {items:[], done:0, total:0};
        const det = (b.items||[]).filter(i=>i.state==="detenida");
        h += `<div class="rp-progress"><div class="rp-note">${state.phase==="terminado"?`Listo: ${(b.items||[]).filter(i=>i.state==="lista").length} sesiones reconfiguradas`:`Aplicando ${b.total} cambios · reinicio con la conversación conservada`}</div>
          <div class="pb"><i style="--p:${b.total?Math.round(b.done/b.total*100):0}%"></i></div>
          <div class="steps">${(b.items||[]).filter(i=>i.state!=="omitida").map(i=>`<span class="${i.state}">${esc(i.session)} ${esc(i.pane)}${i.state==="detenida"?` · ${esc(i.error)} <button data-retry="${esc(i.key)}">reintentar</button>`:""}</span>`).join("")}</div></div>`;
        h += `<div class="rp-foot"><span class="rp-note">${b.done}/${b.total} · ${det.length} detenidas</span><span class="sp"></span>${state.phase==="terminado"?`<button class="rp-ghost" data-revert>Revertir todo</button><button class="rp-go" data-again>Volver a analizar</button>`:""}</div>`;
      }
      h += `<div class="rp-dropbar">${P.map(p=>`<div class="dz rp-drop" data-pool="${esc(p.k)}"><span class="t">${esc(p.label)}<br><span data-verdict-bar="${esc(p.k)}">${esc(p.after?p.after.verdict:p.verdict)}</span></span></div>`).join("")}</div>`;
    }
    root.innerHTML = h; root.classList.toggle("dragging", !!state.armed); wire(root);
  }
  function wire(root){
    root.querySelector("[data-analyze]")?.addEventListener("click", analyze);
    root.querySelector("[data-apply]")?.addEventListener("click", apply);
    root.querySelector("[data-revert]")?.addEventListener("click", revert);
    root.querySelector("[data-again]")?.addEventListener("click", ()=>{ state.plan=null; state.batch=null; state.overrides={}; state.phase="idle"; render(); });
    root.querySelector("[data-reset]")?.addEventListener("click", ()=>{ state.overrides={}; analyze(); });
    root.querySelectorAll("[data-set]").forEach(b=>b.addEventListener("click", e=>{ e.stopPropagation(); const [k,kind,v]=b.dataset.set.split("|"); const it=state.plan.items.find(i=>i.key===k); const to={...it.to, [kind]:v}; setOverride(k,{to}); }));
    root.querySelectorAll("[data-lock]").forEach(b=>b.addEventListener("click", e=>{ e.stopPropagation(); const k=b.dataset.lock; const it=state.plan.items.find(i=>i.key===k); if(it.locked){ delete state.overrides[k]; analyze(); } else setOverride(k,{locked:true}); }));
    root.querySelectorAll("[data-retry]").forEach(b=>b.addEventListener("click", ()=>retry(b.dataset.retry)));
    root.querySelectorAll(".rp-tk[draggable=true]").forEach(el=>{
      el.addEventListener("dragstart", e=>{ state.drag=el.dataset.key; el.classList.add("lift"); root.classList.add("dragging"); try{e.dataTransfer.setData("text/plain", state.drag);}catch(_){} });
      el.addEventListener("dragend", ()=>{ el.classList.remove("lift"); state.drag=null; root.classList.remove("dragging"); if(state.plan) paintImpact(state.plan.impact); root.querySelectorAll(".rp-drop.over").forEach(z=>z.classList.remove("over")); });
      el.addEventListener("click", e=>{ if(e.target.closest("button")) return; state.armed = state.armed===el.dataset.key ? null : el.dataset.key; render(); });
    });
    root.querySelectorAll(".rp-drop").forEach(z=>{
      const target = ()=>{ const [motor, acct] = z.dataset.pool.split(":"); const it = state.plan.items.find(i=>i.key===(state.drag||state.armed)); return {to:{...it.to, motor, harness: it.to.harness, harnessAccount: acct, motorAccount: it.to.harness!==motor ? "main" : acct, routeId: `${it.to.harness}:${motor}`}}; };
      z.addEventListener("dragover", e=>{ e.preventDefault(); if(!z.classList.contains("over")){ root.querySelectorAll(".rp-drop.over").forEach(o=>o.classList.remove("over")); z.classList.add("over"); preview(state.drag, target()); } });
      z.addEventListener("dragleave", e=>{ if(!z.contains(e.relatedTarget)) z.classList.remove("over"); });
      z.addEventListener("drop", e=>{ e.preventDefault(); const k = state.drag || e.dataTransfer.getData("text/plain"); if(k) setOverride(k, target()); });
      z.addEventListener("click", e=>{ if(!state.armed || e.target.closest(".rp-tk")) return; const k=state.armed; state.armed=null; setOverride(k, target()); });
    });
  }
  window.Reparto = { load, render, state };
})();
```

Notas:
- `state.plan.options`: en Task 8 añadir al plan `options = {motor: {models: [...ids], efforts: [...]}}` desde `registry["motors"]` para que las fichas muestren solo opciones válidas (una línea en `allocation_propose`: `plan["options"] = {m: {"models": [x["id"] for x in spec["models"]], "efforts": sorted({e for x in spec["models"] for e in x.get("efforts", [])}, key=["low","medium","high","xhigh","max"].index)} for m, spec in registry["motors"].items()}`).
- `it.status`: en Task 8 añadir `status` a cada item del plan desde la sesión (`s.get("status")`).
- El bloqueo (`🔒`) debe ser un icono monolínea del tablero (`svg("lock",12)` si la función `svg` es global), no un emoji: comprobar con `grep -n "function svg(" dash/index.html` y usar esa función.

`dash/index.html`: los cambios listados en **Files**. En `activateMtab(scope, name)` añadir `if(name==="reparto") window.Reparto?.load();`.

`tests/test_js_parses.sh`: añadir `node --check dash/reparto.js` antes del `echo OK`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `python3 -m pytest tests/test_dashboard_reparto.py tests/test_dashboard_assets.py tests/test_dashboard_layout.py -q && bash tests/test_js_parses.sh`
Expected: todos pasan, `OK`. Si `test_dashboard_assets.py` exige que cada asset servido esté en una lista blanca de cc-dash, añadir `reparto.js` y `reparto.css` donde estén `workspace.js`/`workspace.css`.

- [ ] **Step 5: Verificación manual mínima** (sin browser automation local; el usuario o `chrome-bg`): con cc-dash corriendo, abrir Analytics → Reparto: cuatro tanques con líquido, "Analizar" rellena fichas, arrastrar una ficha a otro tanque cambia la capa clara mientras está encima, y "Aplicar N" muestra el panel de progreso. Anotar en el commit qué se probó.

- [ ] **Step 6: Commit**

```bash
git add dash/reparto.js dash/reparto.css dash/index.html tests/test_dashboard_reparto.py tests/test_js_parses.sh
git commit -m "feat(dash): pestaña Reparto — tanques de cuota, fichas antes→después con selectores, arrastre con vista previa, progreso"
```

---

### Task 11: Doc de usuario, catálogo del operador y limpieza

**Files:**
- Create: `docs/reparto.md`
- Modify: `lib/operator_catalog.py` (la acción `open_analytics_tab` acepta `reparto`; si existe una acción `optimization_apply`, marcarla `deprecated` con nota "usa /allocation/apply"), `README.md` y `README.es.md` (una línea en "What you get" / "Qué obtienes"), `docs/releases/v1.6.1.md` (nuevo, formato de `v1.6.0.md`).
- Test: `tests/test_operator_analytics.py` si cubre `open_analytics_tab` con la lista de pestañas.

- [ ] **Step 1: Write the failing test** (solo si `open_analytics_tab` valida la pestaña)

```python
def test_open_analytics_tab_accepts_reparto():
    from operator_catalog import CATALOG
    spec = next(t for t in CATALOG if t.name == "open_analytics_tab")
    assert "reparto" in json.dumps(spec.schema)
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python3 -m pytest tests/test_operator_analytics.py -q -k reparto`
Expected: FAIL (si aplica; si `open_analytics_tab` no valida pestañas, saltar a Step 3).

- [ ] **Step 3: Write docs and catalog changes**

`docs/reparto.md`:

```markdown
# Reparto de cuota

Analytics → **Reparto**. Cuatro tanques, uno por cuenta con cuota (Claude main, Claude relotto, Codex, Grok).
La capa oscura es lo gastado; la clara, lo que tus sesiones añadirán hasta que la cuota se renueve.
Cada tanque dice "llega al reset" o "se acaba en 1d 22h", y su ritmo: 1.0x es gastar justo lo que llega al reset.

1. **Analizar** reparte tus sesiones vivas entre cuentas y motores para que ninguna cuota se acabe antes de tiempo.
2. **Cura** la propuesta: arrastra una sesión a otro tanque (o tócala y toca el tanque), cambia modelo y effort en los
   selectores de la ficha, o fija una sesión con el candado. Las cuotas reaccionan mientras arrastras.
3. **Aplicar N** reinicia esas sesiones con la nueva configuración conservando la conversación. El panel de progreso
   muestra cada sesión; si una se detiene (trust, login), reintenta desde ahí. **Revertir todo** aparece al terminar.

Al cambiar de cuenta, ComandOS copia la aceptación de la carpeta desde la cuenta origen: nunca acepta una carpeta que
tú no hayas aceptado antes. Las rutas que no admiten cambio en caliente (OpenCode, Antigravity, ACP salvo Claude) no
aparecen como destino.
```

`docs/releases/v1.6.1.md`: una entrada "Reparto de cuota" con los tres pasos y la nota de trust heredado. README: una viñeta.

- [ ] **Step 4: Run the whole suite**

Run: `python3 -m pytest tests -q -x --ignore=tests/test_acp_client.py && bash tests/test_js_parses.sh && bash tests/test_codex_adapters.sh`
Expected: todo verde (los E2E `COMANDOS_E2E` siguen opt-in).

- [ ] **Step 5: Commit**

```bash
git add docs/reparto.md docs/releases/v1.6.1.md README.md README.es.md lib/operator_catalog.py tests/test_operator_analytics.py
git commit -m "docs(reparto): guía de usuario, nota de release y catálogo del operador"
```

---

## Self-Review

**Spec coverage.** Flujo en cuatro pasos → Task 10. Importancia del usuario / reparto del motor → Task 3 (`overrides`, capas). Aplicar reinicia ya → Task 4 (`interrupt: True`) y Task 9. Trust heredado → Task 5, 6. Tanques dos capas, fichas, arrastre con vista previa, angosto con barra de destinos → Task 10 (CSS portado del prototipo, `@container`). Lenguaje llano → Task 1 (`verdict`) y test de copy en Task 10. Determinista + plan congelado + `plan_stale` → Task 3, 9. Endpoints propose/preview/apply/status/revert → Task 8, 9 (más `retry`, que el panel de progreso necesita). Diálogos configurables y ventana de 90 s → Task 7. Cuotas con ritmo para todas + `stale` de Codex/Grok → Task 1 y 8 cubren el ritmo; **hueco:** el spec pide que Codex y Grok pasen a `stale: true` en vez de desaparecer (`bin/cc_usage.py:1339`). Añadido como paso extra en Task 8: en `read_codex_rate_limits`, sustituir el descarte por `item["stale"] = age > 1.5 * window` y conservar la fila; el tanque muestra "cuota de hace X" cuando `stale`. Test: en `tests/test_usage_core.py` (o el que cubra `read_codex_rate_limits`) un caso con rollout viejo que espera la fila con `stale: True`. Errores (plan caducado, pane bloqueado, ruta no cambiable, fallo a mitad) → Task 3 (candidatos), 4, 9, 10. Pruebas → cada task. Fuera de alcance → sin tasks, correcto.

**Placeholder scan.** Sin TBD/TODO. El único "comprobar" es de adaptación a nombres reales del código (`self.cwd`, `list_accounts`, `svg`), con la instrucción de cómo resolverlo.

**Type consistency.** `plan["items"][i]` usa `from`/`to`/`key`/`same`/`locked`/`layer`/`reason`/`risk` en Task 3, 4, 9, 10. `to` siempre lleva `harness, motor, model, effort, harnessAccount, motorAccount, routeId` (Task 3 `_candidates` + `model/effort`; Task 4 `_payload` los lee; Task 10 los construye igual en `target()`). `batch_view` devuelve `state ∈ {cola, aplicando, lista, detenida, omitida}` y `batchId/planId/done/total/items`, que es lo que `reparto.js` consume. `enrich_limits` produce `burn/verdict/reachesReset/runsOutIn/pace/windowSeconds`, que `impact` y `pools()` leen.
