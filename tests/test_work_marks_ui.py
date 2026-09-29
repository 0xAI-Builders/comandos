"""Runs the Node behaviour checks of dash/work-marks.js and verifies it ships."""
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]


@pytest.mark.skipif(shutil.which("node") is None, reason="node no disponible")
def test_work_marks_node_checks():
    subprocess.run(["node", "--check", str(ROOT / "dash" / "work-marks.js")], check=True)
    out = subprocess.run(["node", str(ROOT / "tests" / "work_marks_checks.cjs")],
                         capture_output=True, text=True, timeout=30)
    assert out.returncode == 0, out.stderr
    assert "work marks checks passed" in out.stdout


def test_dashboard_loads_and_installer_links_the_script():
    html = (ROOT / "dash" / "index.html").read_text()
    assert '<script src="/work-marks.js"></script>' in html
    install = (ROOT / "install.sh").read_text()
    assert " work-marks.js " in install


@pytest.mark.skipif(shutil.which("node") is None, reason="node no disponible")
def test_desktop_and_web_draw_the_same_icons():
    import json
    import sys
    sys.path.insert(0, str(ROOT / "lib"))
    import work_marks
    out = subprocess.run(["node", "-e", "process.stdout.write(JSON.stringify(require(process.argv[1]).ICONS))",
                          str(ROOT / "dash" / "work-marks.js")], capture_output=True, text=True, check=True)
    assert json.loads(out.stdout) == work_marks.ICONS
    assert set(work_marks.MARKS) <= set(work_marks.ICONS)
    svg = work_marks.icon_svg("frozen")
    assert svg.startswith('<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"')
    assert 'stroke="#7CC4FF"' in svg


def test_pane_key_lookup_never_guesses():
    import sys
    sys.path.insert(0, str(ROOT / "lib"))
    import work_marks
    panes = [{"paneKey": "a", "session": "s", "paneId": "%1"}, {"paneKey": "b", "session": "s", "paneId": "%2"},
             {"paneKey": "c", "session": "t", "paneId": "%9"}, {"paneKey": "d", "session": "t", "paneId": "%9"}]
    assert work_marks.pane_key_for(panes, "s", "%2") == "b"
    assert work_marks.pane_key_for(panes, "t", "%9") is None
    assert work_marks.pane_key_for(panes, "s", "%7") is None
