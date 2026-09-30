"""N3: CommandOS no longer ships, installs or starts its Telegram bot.

Everything runs against a temporary HOME with a fake systemctl: the real
cc-telegram.service is never stopped by these tests."""
import os
import stat
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
RETIRE = ROOT / "lib" / "retire-telegram.sh"

SYSTEMCTL = """#!/usr/bin/env bash
printf '%s\\n' "$*" >> "$SYSTEMCTL_LOG"
exit 0
"""


def _exe(path, text):
    path.write_text(text)
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


@pytest.fixture
def home(tmp_path):
    h = tmp_path / "home"
    (h / ".config" / "systemd" / "user" / "default.target.wants").mkdir(parents=True)
    (h / ".local" / "bin").mkdir(parents=True)
    (h / ".claude" / "hooks" / "tg-targets").mkdir(parents=True)
    fake = tmp_path / "fakebin"
    fake.mkdir()
    _exe(fake / "systemctl", SYSTEMCTL)
    return h


def run_retire(home, tmp_path):
    log = tmp_path / "systemctl.log"
    env = {"HOME": str(home), "PATH": f"{tmp_path / 'fakebin'}:/usr/bin:/bin", "SYSTEMCTL_LOG": str(log)}
    proc = subprocess.run(["bash", "-c", f'. "{RETIRE}"; cc_retire_telegram'], env=env,
                          capture_output=True, text=True, timeout=30)
    assert proc.returncode == 0, proc.stderr
    return log.read_text().splitlines() if log.exists() else [], proc.stdout


def test_distribution_no_longer_ships_the_bot():
    for gone in ("bin/cc-telegram", "systemd/cc-telegram.service", "hooks/telegram.env.example",
                 "hooks/md2tg.py"):
        assert not (ROOT / gone).exists(), gone


def test_retirement_stops_only_the_comandos_unit_and_keeps_secrets(home, tmp_path):
    units = home / ".config" / "systemd" / "user"
    (units / "cc-telegram.service").symlink_to(ROOT / "systemd" / "cc-telegram.service")
    (units / "other-bot.service").symlink_to("/opt/elsewhere/other-bot.service")
    (units / "default.target.wants" / "other-bot.service").symlink_to(units / "other-bot.service")
    (home / ".local" / "bin" / "cc-telegram").symlink_to(ROOT / "bin" / "cc-telegram")
    (home / ".local" / "bin" / "cc-dash").symlink_to(ROOT / "bin" / "cc-dash")
    (home / ".claude" / "hooks" / "md2tg.py").symlink_to(ROOT / "hooks" / "md2tg.py")
    secrets = home / ".claude" / "hooks" / "telegram.env"
    secrets.write_text("CC_TELEGRAM_BOT_TOKEN=keep-me\n")
    target = home / ".claude" / "hooks" / "tg-targets" / "msg-1.json"
    target.write_text("{}")

    calls, _ = run_retire(home, tmp_path)
    assert calls == ["--user disable --now cc-telegram.service", "--user daemon-reload"]
    assert all("other-bot" not in c for c in calls), "a foreign unit reached systemctl"
    assert not os.path.lexists(units / "cc-telegram.service")
    assert not os.path.lexists(home / ".local" / "bin" / "cc-telegram")
    assert not os.path.lexists(home / ".claude" / "hooks" / "md2tg.py")
    assert (units / "other-bot.service").is_symlink()
    assert (home / ".local" / "bin" / "cc-dash").is_symlink()
    assert secrets.read_text() == "CC_TELEGRAM_BOT_TOKEN=keep-me\n" and target.exists()

    again, _ = run_retire(home, tmp_path)
    assert again == calls, "second run must not call systemctl again"


def test_a_unit_that_is_not_from_comandos_is_left_alone(home, tmp_path):
    units = home / ".config" / "systemd" / "user"
    foreign_repo = tmp_path / "someone-else" / "systemd"
    foreign_repo.mkdir(parents=True)
    (foreign_repo / "cc-telegram.service").write_text("[Service]\n")
    (units / "cc-telegram.service").symlink_to(foreign_repo / "cc-telegram.service")
    calls, out = run_retire(home, tmp_path)
    assert calls == []
    assert (units / "cc-telegram.service").is_symlink() and "no lo toco" in out
    (units / "cc-telegram.service").unlink()
    (units / "cc-telegram.service").write_text("[Service]\n")
    assert run_retire(home, tmp_path)[0] == []
    assert (units / "cc-telegram.service").exists()


def install_env(tmp_path, home, **extra):
    fake = tmp_path / "fakebin"
    return {"HOME": str(home), "PATH": f"{fake}:/usr/bin:/bin", "SYSTEMCTL_LOG": str(tmp_path / "systemctl.log"),
            "CC_MOCK_UNAME": "Linux", "CC_MOCK_OSRELEASE_FILE": str(tmp_path / "none"),
            "CC_MOCK_OS_RELEASE_FILE": str(tmp_path / "none"), "LANG": "C.UTF-8", **extra}


def test_installer_never_registers_the_bot_and_migrates_only_on_request(home, tmp_path):
    units = home / ".config" / "systemd" / "user"
    (units / "cc-telegram.service").symlink_to(ROOT / "systemd" / "cc-telegram.service")
    proc = subprocess.run(["bash", str(ROOT / "install.sh")], env=install_env(tmp_path, home),
                          capture_output=True, text=True, timeout=300)
    assert proc.returncode == 0, proc.stdout[-2000:] + proc.stderr[-2000:]
    log = (tmp_path / "systemctl.log").read_text()
    assert "telegram" not in log.lower(), "the default install touched cc-telegram"
    assert not (home / ".claude" / "hooks" / "telegram.env").exists()
    assert not os.path.lexists(home / ".claude" / "hooks" / "md2tg.py")
    assert (units / "cc-telegram.service").is_symlink(), "migration must wait for its flag"
    assert "telegram" not in proc.stdout.lower()

    (tmp_path / "systemctl.log").unlink()
    proc = subprocess.run(["bash", str(ROOT / "install.sh")],
                          env=install_env(tmp_path, home, COMANDOS_RETIRE_TELEGRAM="1"),
                          capture_output=True, text=True, timeout=300)
    assert proc.returncode == 0, proc.stderr[-2000:]
    calls = (tmp_path / "systemctl.log").read_text().splitlines()
    assert "--user disable --now cc-telegram.service" in calls
    assert [c for c in calls if "telegram" in c] == ["--user disable --now cc-telegram.service"]
    assert not os.path.lexists(units / "cc-telegram.service")


def test_hook_and_dashboard_sources_have_no_telegram_channel():
    notify = (ROOT / "hooks" / "cc-notify.sh").read_text()
    dash = (ROOT / "bin" / "cc-dash").read_text()
    html = (ROOT / "dash" / "index.html").read_text()
    conf = (ROOT / "hooks" / "cc-notify.conf.example").read_text()
    assert "api.telegram.org" not in notify + dash
    assert "TELEGRAM_ENABLED" not in notify + dash + html + conf
    assert "telegram.env" not in notify
    # the operator chat was retired later too (S4)
    assert "operator_chat" not in dash and not (ROOT / "lib" / "operator_chat.py").exists()
