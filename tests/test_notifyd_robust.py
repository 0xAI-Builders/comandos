"""cc-notifyd: los popups del sistema nunca se traban (1-oct).

- Al llegar al tope ya no se descarta el aviso NUEVO: se cierra el más viejo
  (primero los que no esperan respuesta).
- Un popup «te espera» se cierra solo cuando esa sesión ya no espera (respondiste
  en la terminal) o ya no existe; nunca durante sus primeros segundos.
"""
import importlib.machinery
import importlib.util
from pathlib import Path
from types import SimpleNamespace as W

ROOT = Path(__file__).resolve().parents[1]
loader = importlib.machinery.SourceFileLoader("cc_notifyd_under_test", str(ROOT / "bin" / "cc-notifyd"))
spec = importlib.util.spec_from_loader(loader.name, loader)
nd = importlib.util.module_from_spec(spec)
loader.exec_module(nd)


def test_full_stack_evicts_the_oldest_and_keeps_waiting_ones_longest():
    a = W(_kind="waiting", _born=1)
    b = W(_kind="done", _born=2)
    c = W(_kind="done", _born=3)
    assert nd.evict_candidate([a, b, c]) is b
    only_waiting = [W(_kind="waiting", _born=5), W(_kind="waiting", _born=4)]
    assert nd.evict_candidate(only_waiting) is only_waiting[1]   # si todos esperan, el más viejo
    assert nd.evict_candidate([]) is None


def test_waiting_popup_closes_once_the_session_no_longer_waits():
    now = 100.0
    w = W(_kind="waiting", _session="alpha", _pane="%3", _born=now - 10)
    fresh = W(_kind="waiting", _session="beta", _pane="", _born=now - 1)
    done = W(_kind="done", _session="alpha", _pane="%3", _born=now - 30)
    state = [{"session": "alpha", "pane": "%3", "status": "waiting", "alive": True}]
    assert nd.stale_waiting([w, fresh, done], state, now) == []          # sigue esperando; el nuevo tiene gracia
    state = [{"session": "alpha", "pane": "%3", "status": "working", "alive": True}]
    assert nd.stale_waiting([w, fresh, done], state, now) == [w]         # respondiste: se cierra
    assert nd.stale_waiting([w], [], now) == [w]                          # la sesión ya no existe
    other_pane = [{"session": "alpha", "pane": "%9", "status": "waiting", "alive": True}]
    assert nd.stale_waiting([w], other_pane, now) == [w]                  # espera OTRO pane, no este
    no_pane = W(_kind="waiting", _session="alpha", _pane="", _born=now - 10)
    assert nd.stale_waiting([no_pane], other_pane, now) == []             # sin pane: basta con que la sesión espere


def test_daemon_wires_both_rules():
    src = (ROOT / "bin" / "cc-notifyd").read_text()
    assert "len(popups) >= 8" not in src, "a full stack never drops the new notice"
    assert "evict_candidate(popups)" in src
    assert "threading.Thread(target=waiting_sweep_loop, daemon=True).start()" in src
