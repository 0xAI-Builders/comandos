#!/usr/bin/env python3
from pathlib import Path


NOTIFY = Path("hooks/cc-notify.sh").read_text()


def test_cc_notify_has_best_effort_usage_capture_hook():
    assert "usage_capture()" in NOTIFY
    assert "capture-hook" in NOTIFY
    assert ">/dev/null 2>&1 &" in NOTIFY


if __name__ == "__main__":
    test_cc_notify_has_best_effort_usage_capture_hook()
