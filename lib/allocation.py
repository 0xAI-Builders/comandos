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
LAYER_EFFORT = {3: "high", 2: "medium", 1: "low"}
LAYER_TIER = {3: "high", 2: "mid", 1: "low"}
WEIGHT = {"low": 0.5, "medium": 1.0, "high": 1.6, "xhigh": 2.2, "max": 3.0}
_HIGH_EFFORTS = {"high", "xhigh", "max"}


def _t(lang: str, es: str, en: str) -> str:
    """Copia bilingue. El idioma viaja como argumento y no como estado del modulo:
    este fichero es puro y dos peticiones seguidas pueden pedir idiomas distintos."""
    return en if lang == "en" else es


def fmt_duration(seconds: float) -> str:
    seconds = max(0.0, float(seconds))
    if seconds < 3600:
        return f"{int(round(seconds / 60))} min"
    if seconds < 24 * 3600:
        return f"{int(round(seconds / 3600))} h"
    days, rest = divmod(int(round(seconds / 3600)), 24)
    return f"{days}d {rest}h"


def enrich_limits(limits: list[dict], now: float, lang: str = "es") -> list[dict]:
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
                   verdict=_t(lang, "llega al reset", "lasts to reset") if reaches
                   else _t(lang, f"se acaba en {fmt_duration(runs_out)}",
                                 f"runs out in {fmt_duration(runs_out)}"))
        out.append(row)
    return out


VALUE_RANK = {"alto": 0, "medio": 1, "bajo": 2}
INTERRUPTIONS_HIGH = 3.0   # esperas por hora a partir de las que un modelo mas pesado se paga solo


def profile_for(session: dict, profiles: dict | None) -> dict:
    """Perfil del proyecto de la sesion: por su raiz git, o por prefijo de ruta si
    la sesion vive en un subdirectorio. Sin perfil devuelve {}."""
    if not profiles:
        return {}
    root = str(session.get("gitRoot") or session.get("cwd") or "").rstrip("/")
    if not root:
        return {}
    if root in profiles:
        return dict(profiles[root])
    best = ""
    for k in profiles:
        kk = str(k).rstrip("/")
        if kk and (root == kk or root.startswith(kk + "/")) and len(kk) > len(best):
            best = kk
    return dict(profiles[best]) if best else {}


def signal_for(session: dict, signals: dict | None) -> dict:
    return profile_for(session, signals) if signals else {}


def layer_of(session: dict, profile: dict | None = None, signal: dict | None = None) -> int:
    """Capa 1-3 de la sesion. Sin perfil: por effort y estado, como siempre.
    Con perfil, el valor del proyecto manda sobre el effort que tenga puesto:

      valor alto + complejidad alta  -> 3 siempre: se queda con lo mejor
      valor alto + te interrumpe     -> sube una: cada espera evitada vale mas que los tokens
      complejidad baja               -> tope 2: no necesita lo mas pesado
      valor bajo                     -> tope 2, y 1 si esta parada: absorbe lo barato
    """
    effort = str(session.get("effort") or "").lower()
    base = 3 if effort in _HIGH_EFFORTS else 1 if effort == "low" else 2
    resting = str(session.get("status") or "") in ("idle", "done")
    if resting:
        base = max(1, base - 1)
    prof = profile or {}
    if not prof:
        return base
    value = str(prof.get("value") or "medio")
    complexity = str(prof.get("complexity") or "media")
    per_hour = float((signal or {}).get("interruptionsPerHour") or 0.0)
    if value == "alto" and complexity == "alta":
        return 3
    if value == "alto" and per_hour >= INTERRUPTIONS_HIGH:
        base = min(3, base + 1)
    if complexity == "baja":
        base = min(base, 2)
    if value == "bajo":
        base = min(base, 1 if resting else 2)
    return base


def stale_factor(session: dict, now: float) -> float:
    """Cuanto pesa HOY una sesion en la cuota. Una que trabaja pesa entera; una
    parada hace horas apenas gasta hasta que la despierten. Sin marca de tiempo
    no se descuenta nada, para que el calculo siga siendo reproducible."""
    ts = session.get("ts")
    try:
        ts = float(ts)
    except (TypeError, ValueError):
        return 1.0
    if not ts or str(session.get("status") or "") not in ("idle", "done"):
        return 1.0
    age = max(0.0, float(now) - ts)
    return 0.1 if age >= 24 * 3600 else 0.25 if age >= 3 * 3600 else 1.0


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
    """La fila de cuota que manda para (motor, cuenta). Una fila sin porcentaje
    (Grok medido en local sin limite declarado) no gobierna nada: se trata como
    cuota desconocida en vez de reventar la propuesta entera con float(None)."""
    rows = [r for r in limits if r.get("provider") == motor and (r.get("account") or "main") == (account or "main")
            and r.get("percent") is not None]
    for r in rows:
        if r.get("window") == "7d" and r.get("kind") in ("weekly_all", "window", None) and r.get("kind") != "weekly_scoped":
            return r
    for r in rows:
        if r.get("window") == "7d":
            return r
    return rows[0] if rows else None


_HASH_FIELDS = ("session", "pane", "agent", "model", "effort", "account", "harnessAccount", "motorAccount")


def session_key(s: dict) -> str:
    return f"{s.get('session')}|{s.get('pane')}"


def plan_hash(sessions: list[dict]) -> str:
    rows = sorted(tuple(str(s.get(f) or "") for f in _HASH_FIELDS) for s in sessions)
    return hashlib.sha256(json.dumps(rows).encode()).hexdigest()[:16]


def _from(s: dict) -> dict:
    motor = s.get("motor") or s.get("agent") or ""
    route = s.get("routeId") or f"{s.get('agent') or motor}:{motor}"
    return dict(agent=s.get("agent") or "", motor=motor,
                model=s.get("model") or "", effort=s.get("effort") or "",
                account=s.get("harnessAccount") or s.get("account") or "main",
                motorAccount=s.get("motorAccount") or s.get("account") or "main",
                harness=route.split(":")[0], routeId=route)


def _pool_of(to: dict) -> str:
    """La cuota que paga es la del motor, con la cuenta del motor (en gateway, `main`)."""
    return pool_key(to.get("motor") or "", to.get("motorAccount") or to.get("harnessAccount") or "main")


def _load_map(items_to: list[tuple[str, object]]) -> dict:
    """items_to: [(poolKey, effort | peso)] → {poolKey: peso}. El segundo elemento
    puede ser el nombre del effort o un peso ya calculado (effort × antigüedad)."""
    load: dict[str, float] = {}
    for pool, w in items_to:
        load[pool] = load.get(pool, 0.0) + (float(w) if isinstance(w, (int, float)) else WEIGHT.get(w, 1.0))
    return load


def _reference_load(load: dict) -> float:
    """Carga típica de las cuotas que hoy llevan sesiones.

    Una cuenta sin sesiones vivas no tiene escala propia: su `burn` se midió con una
    carga que ya no está. Escalarlo desde cero la haría parecer varias veces peor de
    lo que es y ninguna sesión se movería nunca a ella. Se toma como vara la carga
    media de las cuotas que sí tienen sesiones.
    """
    live = [w for w in load.values() if w > 0]
    return max(0.5, sum(live) / len(live)) if live else 0.5


def _burn_after(row: dict | None, load_now: float, load_after: float, ref_load: float) -> float | None:
    if not row or row.get("burn") is None:
        return None
    if float(row.get("percent") or 0) < 3:
        return max(0.1, load_after * 0.06)
    base = load_now if load_now > 0 else ref_load
    return row["burn"] * (load_after / max(0.5, base))


def _candidates(s: dict, registry: dict, support: dict, accounts: dict) -> list[dict]:
    frm = _from(s)
    harness, here = frm["harness"], frm["account"]
    out = []
    for route in registry.get("routes") or []:
        if route.get("harness") != harness:
            continue
        if not (support.get(route.get("id")) or {}).get("selectable"):
            continue
        motor = route["motor"]
        if motor != harness:  # gateway: la cuenta del motor no es seleccionable (no hay exportador)
            out.append(dict(harness=harness, motor=motor, routeId=route["id"],
                            harnessAccount=here, motorAccount="main"))
            continue
        accts = set(accounts.get(motor) or [])
        if motor == frm["motor"]:
            accts.add(here)  # quedarse igual es candidato aunque la cuenta no esté listada
        for acct in sorted(accts):
            out.append(dict(harness=harness, motor=motor, routeId=route["id"],
                            harnessAccount=acct, motorAccount=acct))
    if not out:  # ruta actual no cambiable en caliente: solo queda quedarse igual
        out.append(dict(harness=harness, motor=frm["motor"], routeId=frm["routeId"],
                        harnessAccount=here, motorAccount=frm["motorAccount"]))
    return out


def _within_layer(cands: list[dict], layer: int, frm: dict) -> list[dict]:
    """La capa 3 se reparte entre las cuentas de su motor, no cambia de motor.

    La capa es la importancia que declaró el usuario. Cambiar de cuenta es repartir
    la misma familia de modelos; cambiar de motor cambia el modelo debajo del trabajo
    que el usuario marcó como el más importante, y eso lo decide él arrastrando la
    ficha. Las capas 2 y 1 son la válvula de escape del reparto.
    """
    if layer < 3:
        return cands
    return [c for c in cands if c["motor"] == frm["motor"]] or cands


def _risk(frm: dict, to: dict) -> str:
    if to["motor"] != frm["motor"] or to["harnessAccount"] != frm["account"]:
        return "alto"
    if to["model"] != frm["model"]:
        return "medio"
    return "bajo"


def _same(frm: dict, to: dict) -> bool:
    return (to["motor"] == frm["motor"] and to["model"] == frm["model"] and to["effort"] == frm["effort"]
            and to["harnessAccount"] == frm["account"] and to["motorAccount"] == frm["motorAccount"])


def impact(items: list[dict], limits: list[dict], now: float, lang: str = "es") -> dict:
    enriched = enrich_limits(limits, now, lang)
    pools = sorted({pool_key(i["from"]["motor"], i["from"]["motorAccount"]) for i in items}
                   | {_pool_of(i["to"]) for i in items}
                   | {pool_key(r["provider"], r.get("account") or "main") for r in enriched})
    load_now = _load_map([(pool_key(i["from"]["motor"], i["from"]["motorAccount"]),
                           i.get("weightFrom", i["from"]["effort"])) for i in items])
    load_after = _load_map([(_pool_of(i["to"]), i.get("weightTo", i["to"]["effort"])) for i in items])
    ref_load = _reference_load(load_now)
    out = {}
    for pool in pools:
        motor, acct = pool.split(":", 1)
        row = governing_limit(enriched, motor, acct)
        burn_now = row.get("burn") if row else None
        burn_after = _burn_after(row, load_now.get(pool, 0.0), load_after.get(pool, 0.0), ref_load)
        entry = dict(n=int(sum(1 for i in items if _pool_of(i["to"]) == pool)),
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
                         verdict=_t(lang, "llega al reset", "lasts to reset") if reaches
                         else _t(lang, f"se acaba en {fmt_duration(runs_out)}",
                                       f"runs out in {fmt_duration(runs_out)}"))
        out[pool] = entry
    return out


def propose(sessions, limits, registry, support, accounts, tiers, *, now, overrides=None,
             lang: str = "es", profiles: dict | None = None, signals: dict | None = None) -> dict:
    overrides = overrides or {}
    enriched = enrich_limits(limits, now, lang)
    motors = registry.get("motors") or {}
    # el harness puede no ser un motor (acp, gemini, shell); lo que gasta cuota es el motor
    live = [s for s in sessions if _from(s)["motor"] in motors]
    prof = {session_key(s): profile_for(s, profiles) for s in live}
    sig = {session_key(s): signal_for(s, signals) for s in live}
    layers = {session_key(s): max(1, min(3, int((overrides.get(session_key(s)) or {}).get("layer")
                                                or layer_of(s, prof[session_key(s)], sig[session_key(s)]))))
              for s in live}
    # Peso de cada sesion hoy: effort × antigüedad. Una parada hace un dia casi no cuenta.
    stale = {session_key(s): stale_factor(s, now) for s in live}
    # Las de mas valor se reparten primero: se quedan con la cuota mas fresca.
    order = sorted(live, key=lambda s: (VALUE_RANK.get(str(prof[session_key(s)].get("value") or "medio"), 1),
                                        -layers[session_key(s)], session_key(s)))
    load = _load_map([(pool_key(f["motor"], f["motorAccount"]), WEIGHT.get(f["effort"], 1.0) * stale[k])
                      for k, f in ((session_key(s), _from(s)) for s in live)])
    load_now = dict(load)
    ref_load = _reference_load(load_now)
    items = []
    for s in order:
        key = session_key(s)
        frm = _from(s)
        ov = overrides.get(key) or {}
        layer = layers[key]
        src = pool_key(frm["motor"], frm["motorAccount"])
        frozen = not (support.get(frm["routeId"]) or {}).get("selectable")
        locked = bool(ov.get("locked")) or frozen
        if locked or "to" in ov:
            # el `to` del usuario manda, pero lo que no diga se hereda de donde está
            here = dict(harness=frm["harness"], motor=frm["motor"], model=frm["model"], effort=frm["effort"],
                        harnessAccount=frm["account"], motorAccount=frm["motorAccount"], routeId=frm["routeId"])
            to = here if locked else {**here, **dict(ov["to"])}
            reason = (_t(lang, "esta ruta no admite cambio en caliente",
                             "this route cannot be switched live") if frozen
                      else _t(lang, "fijada por ti", "pinned by you") if locked
                      else _t(lang, "ajustada por ti", "tuned by you"))
        else:
            effort = LAYER_EFFORT[layer]
            # "Se queda con lo mejor" tiene que ser verdad: un proyecto de valor alto
            # y complejo que ya corre en xhigh o max no baja a high por la tabla.
            pf0 = prof[key]
            if (layer == 3 and frm["effort"] in _HIGH_EFFORTS
                    and str(pf0.get("value") or "") == "alto" and str(pf0.get("complexity") or "") == "alta"):
                effort = frm["effort"]
            scored = []
            for c in _within_layer(_candidates(s, registry, support, accounts), layer, frm):
                pool = _pool_of(c)
                row = governing_limit(enriched, *pool.split(":", 1))
                after = dict(load)
                after[src] = after.get(src, 0.0) - WEIGHT.get(frm["effort"], 1.0) * stale[key]
                after[pool] = after.get(pool, 0.0) + WEIGHT.get(effort, 1.0) * stale[key]
                burn_after = _burn_after(row, load_now.get(pool, 0.0), after[pool], ref_load)
                is_same = c["motor"] == frm["motor"] and c["harnessAccount"] == frm["account"]
                reset_far = -(float(row["resets_at"]) if row else 0.0)
                unknown = row is None or burn_after is None
                over = 0 if (not unknown and burn_after <= 1.0) else 1
                scored.append(((unknown, over, 999.0 if unknown else burn_after, 0 if is_same else 1,
                                0 if c["motor"] == frm["motor"] else 1, reset_far,
                                c["routeId"], c["harnessAccount"]), c, burn_after, row))
            scored.sort(key=lambda t: t[0])
            _, c, burn_after, row = scored[0]
            to = dict(c, model=model_for_layer(c["motor"], layer, registry, tiers), effort=effort)
            pct = (_t(lang, f"{int(round(float(row['percent'])))} % usado",
                            f"{int(round(float(row['percent'])))} % used")
                   if row else _t(lang, "sin cuota conocida", "no known quota"))
            reset = (_t(lang, f", reset en {fmt_duration(float(row['resets_at']) - now)}",
                              f", resets in {fmt_duration(float(row['resets_at']) - now)}")
                     if row else "")
            moved = not (to["motor"] == frm["motor"] and to["harnessAccount"] == frm["account"])
            where = f"{to['motor']} {to['harnessAccount']}"
            reason = (_t(lang, f"{where} tiene más margen: {pct}{reset}",
                               f"{where} has more room: {pct}{reset}") if moved
                      else _t(lang, f"se queda en {where}: {pct}{reset}",
                                    f"stays on {where}: {pct}{reset}"))
            if burn_after is not None and burn_after > 1.0:
                reason += _t(lang,
                             " · ninguna cuota llega al reset con esta carga; es la que menos se pasa",
                             " · no quota lasts to reset under this load; this one overshoots least")
        dst = _pool_of(to)  # la asignación ya cuenta para quien venga detrás
        w_from = WEIGHT.get(frm["effort"], 1.0) * stale[key]
        w_to = WEIGHT.get(to["effort"], 1.0) * stale[key]
        load[src] = load.get(src, 0.0) - w_from
        load[dst] = load.get(dst, 0.0) + w_to
        same = _same(frm, to)
        # Por que esta capa: el perfil se explica en la ficha, no se adivina.
        pf, sg = prof[key], sig[key]
        why = []
        if pf:
            v, c = str(pf.get("value") or "medio"), str(pf.get("complexity") or "media")
            if v == "alto" and c == "alta":
                why.append(_t(lang, "valor alto y complejo: se queda con lo mejor",
                                    "high value and complex: keeps the best"))
            elif v == "alto" and float(sg.get("interruptionsPerHour") or 0) >= INTERRUPTIONS_HIGH:
                why.append(_t(lang, f"valor alto y te interrumpe {sg.get('interruptionsPerHour')}/h: sube de nivel",
                                    f"high value, interrupts you {sg.get('interruptionsPerHour')}/h: goes up a tier"))
            elif v == "bajo":
                why.append(_t(lang, "valor bajo: absorbe lo barato", "low value: takes the cheap tier"))
            elif c == "baja":
                why.append(_t(lang, "tarea sencilla: no necesita lo más pesado",
                                    "simple task: no need for the heaviest"))
        if stale[key] < 1.0 and not locked:
            why.append(_t(lang, "parada: casi no pesa en la cuota", "stopped: barely counts against quota"))
        if why and not (same and not locked):
            reason = " · ".join(why) + " · " + reason
        items.append(dict(session=s.get("session"), pane=s.get("pane"), key=key, layer=layer,
                          cwd=s.get("cwd") or "",
                          # Etiqueta que el resto del tablero ya usa para esta sesion
                          # ("SAVA", "MRP", "PaginasWeb ⫽28"). El nombre interno de tmux
                          # (term-3692-1) no le dice nada a nadie.
                          project=s.get("project") or "",
                          gitRoot=s.get("gitRoot") or "",
                          profile=pf or None, signal=sg or None,
                          weightFrom=round(w_from, 3), weightTo=round(w_to, 3),
                          **{"from": frm}, to=to, same=same, locked=locked,
                          reason=_t(lang, "igual", "unchanged") if same and not locked else reason,
                          risk="-" if same else _risk(frm, to)))
    items.sort(key=lambda i: i["key"])
    state_hash = plan_hash(sessions)
    plan_id = hashlib.sha256(f"{state_hash}:{json.dumps(overrides, sort_keys=True)}".encode()).hexdigest()[:12]
    return dict(planId=plan_id, stateHash=state_hash, createdAt=now, items=items,
                impact=impact(items, limits, now, lang))


def invert(plan: dict) -> dict:
    """Plan inverso: cada item vuelve a su `from`. El `impact` sigue siendo el del plan
    original; quien revierte recalcula con las cuotas del momento."""
    inv = copy.deepcopy(plan)
    for i in inv["items"]:
        frm, to = i["from"], i["to"]
        i["to"] = dict(harness=frm.get("harness") or frm["motor"], motor=frm["motor"], model=frm["model"],
                       effort=frm["effort"], harnessAccount=frm["account"], motorAccount=frm["motorAccount"],
                       routeId=frm.get("routeId") or f"{frm['motor']}:{frm['motor']}")
        i["from"] = dict(agent=to.get("motor") or "", motor=to.get("motor") or "", model=to.get("model") or "",
                         effort=to.get("effort") or "", account=to.get("harnessAccount") or "main",
                         motorAccount=to.get("motorAccount") or "main",
                         harness=to.get("harness") or to.get("motor") or "",
                         routeId=to.get("routeId") or "")
        i["same"] = _same(i["from"], i["to"])
        i["reason"], i["risk"] = "revertir", "-" if i["same"] else _risk(i["from"], i["to"])
    inv["planId"] = "rev-" + plan["planId"]
    return inv
