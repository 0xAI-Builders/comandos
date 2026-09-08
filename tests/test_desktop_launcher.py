#!/usr/bin/env python3
from pathlib import Path


APP = Path("bin/cc-app").read_text()
DESKTOP = Path("dash/comandos.desktop.in").read_text()


def test_desktop_launcher_wm_class_matches_gtk_window():
    assert "StartupWMClass=comandos" in DESKTOP
    assert 'win.set_wmclass("comandos", "comandos")' in APP


def test_desktop_launcher_uses_startup_notification():
    assert "StartupNotify=true" in DESKTOP


def test_shift_question_does_not_open_help():
    assert "e.keyval == Gdk.KEY_question" not in APP
    assert "KEY_question and not isinstance" not in APP
    on_key = APP.split("def on_key(w, e):", 1)[1].split("\ndef ", 1)[0]
    assert "KEY_F1" in on_key
    assert "KEY_question" not in on_key


if __name__ == "__main__":
    test_desktop_launcher_wm_class_matches_gtk_window()
    test_desktop_launcher_uses_startup_notification()
