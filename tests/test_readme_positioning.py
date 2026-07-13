from pathlib import Path


README = Path("README.md").read_text()
README_ES = Path("README.es.md").read_text()
RELEASE = Path("docs/releases/v1.5.0.md")
WEBTERM = Path("bin/cc-webterm").read_text()
MOBILE = Path("bin/cc-mobile").read_text()
SECURITY = Path("SECURITY.md").read_text()
PLAN = Path(
    "docs/superpowers/plans/"
    "2026-07-13-v1.5.0-smooth-local-first-release.md"
)


def test_primary_readme_positions_supported_agents_and_local_first():
    first = README[:1800].lower()
    for phrase in (
        "local-first",
        "claude code",
        "codex cli",
        "gemini cli",
        "opencode",
        "tmux",
        "ssh",
    ):
        assert phrase in first


def test_privacy_claims_are_precise():
    for text in (README, README_ES):
        lowered = text.lower()
        assert "no cloud, no db" not in lowered
        assert "sin nube, sin db" not in lowered
    assert "no hosted comandos backend" in README.lower()
    assert "local sqlite" in README.lower()
    assert "~/.ssh/config" in README
    assert "openssh" in README.lower()
    assert "telegram" in README.lower()
    assert "tailscale" in README.lower()


def test_remote_security_claims_distinguish_dashboard_token_from_ttyd():
    for text in (README, README_ES):
        lowered = text.lower()
        assert "behind the same token" not in lowered
        assert "con el mismo token" not in lowered
        assert "no token → no access" not in lowered
        assert "sin token no opera" not in lowered

    english = " ".join(README.lower().split())
    spanish = " ".join(README_ES.lower().split())
    assert "dashboard and cc-dash api" in english
    assert "does not add separate comandos authentication" in english
    assert "tablero remoto y la api de cc-dash" in spanish
    assert "no agrega autenticación separada de comandos" in spanish
    assert "token del dashboard/api no autentica ttyd" in WEBTERM.lower()
    assert "dash-token" not in WEBTERM
    assert "ttyd depende de la política de acceso de tailscale" in MOBILE.lower()
    assert "tailscale policy can operate ttyd" in SECURITY.lower()
    assert "tailnet + a valid token" not in SECURITY.lower()


def test_stale_agent_roadmap_is_removed():
    roadmap = README.partition("## Roadmap")[2]
    assert "Codex CLI" not in roadmap
    assert "Antigravity" not in roadmap


def test_release_notes_cover_v150_upgrade_and_brave_compatibility():
    assert RELEASE.exists()
    notes = RELEASE.read_text()
    for heading in (
        "## Performance",
        "## Resize and touch",
        "## Remote interaction",
        "## Local-first privacy",
        "## Compatibility",
        "## Verification",
        "## Upgrade",
    ):
        assert heading in notes
    assert "Brave" in notes
    assert "git pull --ff-only" in notes
    assert "systemctl --user restart cc-dash.service cc-notifyd.service" in notes


def test_release_plan_commands_publish_the_complete_current_branch_payload():
    plan = PLAN.read_text()

    assert "+      " not in plan
    for path in (
        "bin/cc-mobile",
        "bin/cc-webterm",
        "SECURITY.md",
        "tests/test_usage_dash.py",
        "tests/test_webterm_links.py",
        "tests/test_readme_positioning.py",
        "docs/releases/v1.5.0.md",
    ):
        assert path in plan

    assert "git fetch origin --prune" in plan
    assert "git push origin HEAD:main" in plan
    assert "repos/0xAI-Builders/comandos/topics --input -" in plan
    assert 'do bash "$test_file" || exit; done' in plan
    assert '("show-options", "-A", "-v", "-t", "ssh-prod", "mouse")' in plan
    assert '("show-options", "-v", "-t", sess, "mouse")' not in plan
