import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "lib"))
import tmux_clipboard as tc


def lst(*rows):
    return "\n".join(f"{n}\t{s}" for n, s in rows) + "\n"


def test_newest_auto_ignores_named_internal_buffers():
    out = lst(("cc-type-1", 50), ("buffer453", 28), ("buffer455", 34), ("buffer454", 43))
    assert tc.newest_auto(out) == (455, "buffer455", 34)
    assert tc.newest_auto(lst(("cc-type-1", 50))) is None
    assert tc.newest_auto("") is None


def test_first_poll_only_marks_then_each_new_copy_is_published_once():
    b = tc.Bridge()
    assert b.poll(lst(("buffer453", 28))) is None              # al abrir no pisa el portapapeles
    assert b.poll(lst(("buffer453", 28))) is None
    assert b.poll(lst(("buffer454", 43), ("buffer453", 28))) == "buffer454"
    assert b.poll(lst(("buffer454", 43))) is None              # ya publicado
    # un buffer interno de teclear (con nombre) nunca se publica
    assert b.poll(lst(("cc-type-9", 12), ("buffer454", 43))) is None
    assert b.poll(lst(("buffer456", 5), ("buffer455", 9))) == "buffer456"


def test_restarted_tmux_server_and_oversized_or_empty_buffers():
    b = tc.Bridge()
    b.poll(lst(("buffer900", 10)))
    assert b.poll(lst(("buffer0", 10))) is None                 # servidor nuevo: rebaja la marca
    assert b.poll(lst(("buffer1", 10))) == "buffer1"
    assert b.poll(lst(("buffer2", 0))) is None
    assert b.poll(lst(("buffer3", tc.MAX_BYTES + 1))) is None
    assert b.poll(lst(("buffer4", 7))) == "buffer4"


def test_app_runs_the_bridge_and_publishes_to_the_system_clipboard():
    src = (Path(__file__).resolve().parents[1] / "bin" / "cc-app").read_text()
    body = src[src.index("def tmux_clipboard_loop("):src.index("threading.Thread(target=tmux_clipboard_loop")]
    assert "tmux_clipboard.Bridge()" in body and '"list-buffers"' in body and '"show-buffer", "-b", name' in body
    assert "copy_text_to_clipboard" in body and "GLib.idle_add" in body
    assert "threading.Thread(target=tmux_clipboard_loop, daemon=True).start()" in src
