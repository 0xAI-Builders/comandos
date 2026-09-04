import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "lib"))
import model_watch  # noqa: E402

SRC = (ROOT / "lib" / "model_watch.py").read_text()


def test_codex_regex_accepts_named_generations_like_astra():
    # OpenAI bautiza cada generación (sol, luna, terra, astra…): una lista
    # cerrada de sufijos dejaba fuera gpt-6-astra y el watcher no avisaba.
    for ident in ("gpt-6-astra", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.4-mini", "gpt-5.3-codex", "gpt-5.2"):
        assert model_watch._CODEX_RE.fullmatch(ident), ident
    assert not model_watch._CODEX_RE.fullmatch("gpt-5.6-lunacodex-auto-review")


def test_watcher_finds_binaries_outside_systemd_path():
    # cc-dash corre bajo systemd con PATH pelón: codex vive en ~/.bun/bin y
    # claude/grok en ~/.local/bin. shutil.which a secas devolvía nada y el
    # watcher reportaba codex: [] sin versión.
    assert "shutil.which(" not in SRC.split("_which = shutil.which", 1)[1]
    assert "from providers import which as _which" in SRC


def test_watcher_runs_resolved_paths_not_bare_names():
    # Con PATH pelón, `codex --version` a pelo devolvía nada y la versión era "?".
    assert "_run([exe] + cmd[1:]" in SRC
    assert '_run([grok_exe, "models"]' in SRC
