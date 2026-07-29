import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "bin" / "cc-webterm"
SOURCE = SCRIPT.read_text()


def write_executable(path, body):
    path.write_text("#!/bin/sh\n" + body)
    path.chmod(0o755)


def status_with(tmp_path, healthy):
    fake = tmp_path / "bin"
    fake.mkdir(exist_ok=True)
    write_executable(
        fake / "curl",
        'case "$*" in\n'
        '  *"4779/token"*) [ "$FALLBACK_HEALTHY" = 1 ] ;;\n'
        '  *"4780/term/token"*) [ "$PRIMARY_HEALTHY" = 1 ] ;;\n'
        '  *) exit 1 ;;\n'
        'esac\n',
    )
    env = {
        **os.environ,
        "PATH": f"{fake}:{os.environ['PATH']}",
        "FALLBACK_HEALTHY": "1" if "fallback" in healthy else "0",
        "PRIMARY_HEALTHY": "1" if "primary" in healthy else "0",
    }
    run = subprocess.run(
        [str(SCRIPT), "status"], env=env, text=True,
        capture_output=True, check=True,
    )
    return run.stdout.strip().splitlines()[-1]


def test_status_requires_both_probes_for_active(tmp_path):
    assert status_with(tmp_path, {"primary", "fallback"}) == "active"
    assert status_with(tmp_path, {"primary"}) == "degraded"
    assert status_with(tmp_path, {"fallback"}) == "degraded"
    assert status_with(tmp_path, set()) == "off"


def test_transient_units_restart_only_after_failure():
    assert "--property=Restart=on-failure" in SOURCE
    assert "--property=RestartSec=1s" in SOURCE
    assert SOURCE.count('"${SYSTEMD_RESTART[@]}"') == 2
    assert 'current_state="$(health_state)"' in SOURCE
    assert '[ "$current_state" = active ]' in SOURCE
    assert "systemctl --user stop cc-webterm.service" in SOURCE
    assert "systemctl --user stop cc-webterm-path.service" in SOURCE


def test_enabled_state_survives_reboot_until_user_turns_terminal_off():
    assert 'ENABLED_FILE="$HOOKS/webterm-enabled"' in SOURCE
    assert "mark_enabled()" in SOURCE
    assert 'rm -f "$ENABLED_FILE"' in SOURCE
    active = SOURCE.split('if [ "$current_state" = active ]; then', 1)[1]
    active = active.split("fi", 1)[0]
    assert "mark_enabled" in active
    successful = SOURCE.split(
        'wait_for_endpoint "http://127.0.0.1:$PATH_PORT/term/token"; then', 1)[1]
    successful = successful.split("else", 1)[0]
    assert "mark_enabled" in successful


def test_mobile_and_doctor_do_not_use_process_matches_as_health():
    mobile = (ROOT / "bin" / "cc-mobile").read_text()
    doctor = (ROOT / "bin" / "cc-doctor").read_text()
    assert '4780/term/token' in mobile and '4779/token' in mobile
    assert 'pgrep -f "ttyd.*cc-webterm-attach"' not in mobile
    section = doctor.split("_section_remote()", 1)[1].split("\n}", 1)[0]
    assert '4780/term/token' in section and '4779/token' in section
    assert "pgrep" not in section


def test_launcher_does_not_require_gnu_readlink():
    assert "readlink -f" not in SOURCE
    assert "os.path.realpath" in SOURCE
