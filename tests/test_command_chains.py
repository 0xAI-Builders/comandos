import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "lib"))
import pytest
import command_chains as cc

STEPS = [{"kind": "shell", "text": "codex --dangerously-bypass-approvals-and-sandbox"}, {"kind": "pane", "text": "/model"}]

def test_round_trip_is_deterministic(tmp_path):
    saved = cc.save_chain(tmp_path, "Arrancar Codex yolo y fijar modelo", STEPS)
    assert saved["slug"] == "arrancar-codex-yolo-y-fijar-modelo"
    text = (tmp_path / "arrancar-codex-yolo-y-fijar-modelo.md").read_text(encoding="utf-8")
    assert text == "# Arrancar Codex yolo y fijar modelo\n\n1. shell: codex --dangerously-bypass-approvals-and-sandbox\n2. pane: /model\n"
    assert cc.list_chains(tmp_path) == [{"slug": saved["slug"], "name": "Arrancar Codex yolo y fijar modelo", "steps": STEPS}]

def test_hand_edited_file_with_bad_step_is_listed_with_error_and_others_survive(tmp_path):
    cc.save_chain(tmp_path, "Buena", STEPS)
    (tmp_path / "rota.md").write_text("# Rota\n\n- foo: bar\n", encoding="utf-8")
    listed = {c["slug"]: c for c in cc.list_chains(tmp_path)}
    assert "steps" in listed["buena"]
    assert listed["rota"]["error"].startswith("Paso inválido") and "steps" not in listed["rota"]

def test_parse_accepts_dashes_numbers_and_ignores_blank_lines():
    chain = cc.parse("# Dos\n\n- shell: claude\n\n2) pane: /effort max\n")
    assert chain["steps"] == [{"kind": "shell", "text": "claude"}, {"kind": "pane", "text": "/effort max"}]

@pytest.mark.parametrize("bad", [[], [{"kind": "x", "text": "a"}], [{"kind": "pane", "text": "a\nb"}], [{"kind": "pane", "text": ""}],
                                 [{"kind": "pane", "text": "a\x7fb"}], [{"kind": "pane", "text": "a\x1bb"}], [{"kind": "pane", "text": "a\rb"}]])
def test_save_rejects_invalid_steps(tmp_path, bad):
    with pytest.raises(cc.ChainError):
        cc.save_chain(tmp_path, "N", bad)

def test_same_name_twice_gets_a_distinct_slug_and_explicit_slug_overwrites(tmp_path):
    a = cc.save_chain(tmp_path, "Dup", STEPS)
    b = cc.save_chain(tmp_path, "Dup", STEPS)
    assert (a["slug"], b["slug"]) == ("dup", "dup-2")
    c = cc.save_chain(tmp_path, "Dup renombrada", STEPS[:1], slug="dup")
    assert c["slug"] == "dup" and cc.list_chains(tmp_path)[0]["name"] == "Dup renombrada"

def test_delete_only_touches_that_slug(tmp_path):
    cc.save_chain(tmp_path, "A", STEPS); cc.save_chain(tmp_path, "B", STEPS)
    assert cc.delete_chain(tmp_path, "a") and not cc.delete_chain(tmp_path, "a")
    assert [c["slug"] for c in cc.list_chains(tmp_path)] == ["b"]
    with pytest.raises(cc.ChainError):
        cc.delete_chain(tmp_path, "../b")

def test_bom_file_parses(tmp_path):
    (tmp_path / "bom.md").write_bytes("﻿# Con BOM\n\n1. shell: claude\n".encode("utf-8"))
    (chain,) = cc.list_chains(tmp_path)
    assert chain == {"slug": "bom", "name": "Con BOM", "steps": [{"kind": "shell", "text": "claude"}]}

@pytest.mark.parametrize("stem", ["Mi Cadena", "MAYUS", "a" * 61, "-x"])
def test_file_with_invalid_slug_is_listed_as_error(tmp_path, stem):
    (tmp_path / f"{stem}.md").write_text("# X\n\n1. shell: claude\n", encoding="utf-8")
    (entry,) = cc.list_chains(tmp_path)
    assert "steps" not in entry and entry["name"] == f"{stem}.md"
    assert entry["error"].startswith("Nombre de archivo no válido")

def test_slug_with_trailing_newline_is_rejected(tmp_path):
    cc.save_chain(tmp_path, "A", STEPS)
    with pytest.raises(cc.ChainError):
        cc.delete_chain(tmp_path, "a\n")
    with pytest.raises(cc.ChainError):
        cc.save_chain(tmp_path, "A", STEPS, slug="a\n")
