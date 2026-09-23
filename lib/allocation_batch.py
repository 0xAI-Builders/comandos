# lib/allocation_batch.py
"""Lote de reconfiguración: un plan → N operaciones del coordinador, con estado por pane."""
from __future__ import annotations

import hashlib
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
                data = json.load(f)
        except Exception:
            return {"batches": {}}
        # Lo que estaba en vuelo al cargar era de un proceso que ya murió: nadie lo
        # terminará, y un lote "aplicando" para siempre bloquea el revert.
        for b in (data.get("batches") or {}).values():
            for it in b.get("items") or []:
                if it.get("state") in ("cola", "aplicando"):
                    it["state"], it["error"] = "detenida", "interrumpida por reinicio del tablero; revisa el pane"
        return data

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
            items = [json.loads(json.dumps(b)) for b in self._data["batches"].values()]
            return sorted(items, key=lambda b: b["createdAt"], reverse=True)

    def update_item(self, batch_id, key, **fields):
        with self.lock:
            for it in self._data["batches"][batch_id]["items"]:
                if it["key"] == key:
                    it.update(fields)
            self._write()


def request_id(batch_id, session, pane, attempt=0):
    """Id determinista por item (dedupe en el coordinador) que pasa su validador
    [A-Za-z0-9_-]{8,100}; cada reintento lleva uno nuevo."""
    h = hashlib.sha1(f"{session}|{pane}".encode()).hexdigest()[:10]
    return f"{batch_id}-{h}" + (f"-r{attempt}" if attempt else "")


def _payload(batch_id, it, attempt=0):
    to = it["to"]
    return dict(session=it["session"], pane=it["pane"], requestId=request_id(batch_id, it["session"], it["pane"], attempt),
                interrupt=True, toHarness=to.get("harness") or to["motor"], motor=to["motor"], model=to.get("model", ""),
                effort=to.get("effort", ""), harnessAccount=to.get("harnessAccount", "main"),
                motorAccount=to.get("motorAccount", "main"), routeId=to.get("routeId", ""))


def _drive(batch_id, it, store, configure, status_of, poll, deadline, attempt=0):
    try:
        store.update_item(batch_id, it["key"], state="aplicando", error="", attempts=it.get("attempts", 0) + 1)
        code, body = configure(_payload(batch_id, it, attempt))
        # 200 = el coordinador ya conocía este requestId y la operación terminó.
        if code == 200 and body.get("state") in _UI_STATE:
            code = 202
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
    except Exception as exc:
        store.update_item(batch_id, it["key"], state="detenida", error=str(exc))


def run_batch(batch_id, store, configure, status_of, *, concurrency=3, poll=0.5, deadline=600):
    batch = store.get(batch_id)
    todo = [it for it in batch["items"] if it["state"] == "cola"]
    with ThreadPoolExecutor(max_workers=max(1, concurrency)) as ex:
        list(ex.map(lambda it: _drive(batch_id, it, store, configure, status_of, poll, deadline), todo))
    return store.get(batch_id)


def retry_item(batch_id, key, store, configure, status_of, *, poll=0.5, deadline=600):
    batch = store.get(batch_id)
    it = next(i for i in batch["items"] if i["key"] == key)
    if it["state"] != "detenida":
        return batch
    _drive(batch_id, it, store, configure, status_of, poll, deadline, attempt=it.get("attempts", 0))
    return store.get(batch_id)


def batch_view(batch):
    items = [dict(key=i["key"], session=i["session"], pane=i["pane"], state=i["state"], error=i.get("error", ""),
                  to=i["to"], **{"from": i["from"]}, operationKey=i.get("operationKey", ""),
                  operationId=i.get("operationId", "")) for i in batch["items"]]
    done = sum(1 for i in items if i["state"] in TERMINAL)
    return dict(batchId=batch["batchId"], planId=batch["planId"], stateHash=batch["stateHash"],
                state="terminado" if done == len(items) else "aplicando", done=done, total=len(items), items=items)
