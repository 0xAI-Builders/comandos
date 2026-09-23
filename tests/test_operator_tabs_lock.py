"""Renombrar y mandar al final desde el operador comparten candado con cc-app."""
import json
import threading
from importlib.machinery import SourceFileLoader
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]


@pytest.fixture
def dash(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    (tmp_path / ".claude" / "hooks").mkdir(parents=True)
    mod = SourceFileLoader("ccdash_tabs_lock", str(ROOT / "bin" / "cc-dash")).load_module()
    monkeypatch.setattr(mod, "TABS_FILE", str(tmp_path / "app-tabs.json"))
    monkeypatch.setattr(mod, "HOOKS", str(tmp_path))
    return mod


def test_rename_waits_for_the_shared_lock(dash):
    Path(dash.TABS_FILE).write_text(json.dumps({"a": "A", "b": "B"}))
    done = threading.Event()
    with dash.file_lock(dash.TABS_FILE):
        t = threading.Thread(target=lambda: (dash.operator_rename_tab("a", "Nuevo"), done.set()))
        t.start()
        assert not done.wait(0.3), "renombró sin esperar el candado"
    t.join(2)
    assert json.loads(Path(dash.TABS_FILE).read_text())["a"] == "Nuevo"


def test_send_back_moves_tab_last_and_writes_signal(dash):
    Path(dash.TABS_FILE).write_text(json.dumps({"a": "A", "b": "B"}))
    assert dash.operator_send_back("a") is None
    assert list(json.loads(Path(dash.TABS_FILE).read_text())) == ["b", "a"]
    assert json.loads((Path(dash.HOOKS) / "app-tab-back.json").read_text())["session"] == "a"
