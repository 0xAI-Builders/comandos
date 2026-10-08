"""Run the actual overlay reposition method with counted GTK collaborators."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

def test_unchanged_overlay_skips_draw_but_geometry_theme_and_controls_publish():
    source = (ROOT / "crates/comandos-app/src/ui/overlays.rs").read_text()
    start = source.index("    pub fn reposition(&self, focused: bool)")
    end = source.index("    pub fn paint_ai", start)
    method = source[start:end]
    fixture = (ROOT / "crates/comandos-app/tests/support/overlay_reposition.rs.txt").read_text()
    with tempfile.TemporaryDirectory(prefix="comandos-overlay-reposition-") as tmp:
        rust = Path(tmp) / "probe.rs"
        binary = Path(tmp) / "probe"
        rust.write_text(fixture.replace("// ACTUAL_REPOSITION", method))
        subprocess.run(["rustc", "--edition=2024", str(rust), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    test_unchanged_overlay_skips_draw_but_geometry_theme_and_controls_publish()
    print("overlay reposition regression passed")
