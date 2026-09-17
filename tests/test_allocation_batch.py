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
