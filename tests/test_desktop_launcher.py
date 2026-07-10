#!/usr/bin/env python3
from pathlib import Path


APP = Path("bin/cc-app").read_text()
DESKTOP = Path("dash/comandos.desktop.in").read_text()


def test_desktop_launcher_wm_class_matches_gtk_window():
    assert "StartupWMClass=comandos" in DESKTOP
    assert 'win.set_wmclass("comandos", "comandos")' in APP


def test_desktop_launcher_uses_startup_notification():
    assert "StartupNotify=true" in DESKTOP


if __name__ == "__main__":
    test_desktop_launcher_wm_class_matches_gtk_window()
    test_desktop_launcher_uses_startup_notification()
