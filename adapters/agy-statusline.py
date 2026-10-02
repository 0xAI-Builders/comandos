#!/usr/bin/env python3
"""Barra de estado de agy (Antigravity CLI) que solo guarda su cuota para ComandOS.

agy llama a su comando statusLine con un JSON por stdin cuando cambia su estado; entre otras
cosas trae `quota` (cubetas gemini-5h, gemini-weekly, 3p-5h, 3p-weekly con remaining_fraction y
reset_time) y `plan_tier`. Guardamos SOLO eso (nada de correo, rutas ni modelo) en
~/.claude/hooks/agy-quota.json para las botellas de Analytics. No imprime nada: con
stack_with_default agy sigue pintando su barra de siempre. Nunca falla: agy no debe ver errores.
"""
import json
import os
import sys
import time


def main():
    try:
        data = json.loads(sys.stdin.read() or "null")
    except ValueError:
        return
    quota = data.get("quota") if isinstance(data, dict) else None
    if not isinstance(quota, dict):
        return
    buckets = {}
    for name, b in quota.items():
        if not isinstance(b, dict) or not isinstance(b.get("remaining_fraction"), (int, float)):
            continue
        buckets[str(name)[:40]] = {"remaining_fraction": float(b["remaining_fraction"]),
                                   "reset_time": str(b.get("reset_time") or "")[:40]}
    if not buckets:
        return
    hooks = os.path.join(os.path.expanduser("~"), ".claude", "hooks")
    path = os.path.join(hooks, "agy-quota.json")
    out = {"plan_tier": str(data.get("plan_tier") or "")[:40], "quota": buckets}
    try:
        with open(path) as fh:
            old = json.load(fh)
        if {k: old.get(k) for k in out} == out and time.time() - old.get("captured_at", 0) < 60:
            return  # agy repinta seguido: no reescribir lo mismo
    except (OSError, ValueError, AttributeError):
        pass
    out["captured_at"] = int(time.time())
    os.makedirs(hooks, exist_ok=True)
    tmp = f"{path}.{os.getpid()}.tmp"
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as fh:
        json.dump(out, fh)
    os.replace(tmp, path)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        pass
