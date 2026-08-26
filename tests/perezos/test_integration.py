import re
from pathlib import Path


HTML = Path("dash/index.html").read_text()
CSS = Path("dash/perezos/perezos.css").read_text() if Path("dash/perezos/perezos.css").exists() else ""


def js_function(name: str) -> str:
    match = re.search(rf"function {re.escape(name)}\([^)]*\)\{{", HTML)
    assert match, f"missing JS function: {name}"
    start = match.start()
    depth = 0
    for pos in range(match.end() - 1, len(HTML)):
        if HTML[pos] == "{":
            depth += 1
        elif HTML[pos] == "}":
            depth -= 1
            if depth == 0:
                return HTML[start:pos + 1]
    raise AssertionError(f"unterminated JS function: {name}")


def test_perezos_runtime_loads_in_dependency_order():
    names = ["core", "art", "rig", "behaviors", "motion", "renderer", "engine"]
    positions = [HTML.index(f'/perezos/{name}.js') for name in names]
    assert positions == sorted(positions)
    assert positions[-1] < HTML.index("<script>")
    assert '<link rel="stylesheet" href="/perezos/perezos.css">' in HTML


def test_stage_is_one_semantic_persistent_canvas():
    assert '<button type="button" class="perezos-stage"' in HTML
    assert "PerezOS, mascota de la sesión seleccionada" in HTML
    assert "PerezOS, selected session mascot" in HTML
    assert '<canvas class="perezos-canvas" width="224" height="192" aria-hidden="true"></canvas>' in HTML
    assert HTML.count('class="perezos-canvas"') == 1
    assert "const CENTRO_VIEW = {sessionId:\"\", item:null, mascot:null, roleSig:\"\"}" in HTML


def test_same_session_updates_context_without_rebuilding_shell():
    render = js_function("renderCentro")
    assert "const sameSession = CENTRO_VIEW.sessionId === it.session" in render
    assert "if(!sameSession)" in render
    rebuild = render.split("if(!sameSession)", 1)[1].split("CENTRO_VIEW.mascot.setContext", 1)[0]
    assert "box.innerHTML" in rebuild
    update = render.split("CENTRO_VIEW.mascot.setContext", 1)[1]
    assert "box.innerHTML" not in update
    assert "CENTRO_VIEW.item = it" in render
    assert "perezosContext(CENTRO_VIEW.item" in render


def test_handlers_use_live_item_and_lifecycle_destroys_without_selection():
    render = js_function("renderCentro")
    assert "CENTRO_VIEW.mascot?.destroy()" in render
    assert 'CENTRO_VIEW.sessionId = ""' in render
    assert "openSession(CENTRO_VIEW.item" in render
    assert 'notifyInteraction("activate", 0, 0)' in render
    assert 'notifyInteraction("pointer", x, y)' in render
    assert "getBoundingClientRect()" in render
    assert "setPointerCapture" not in render


def test_context_maps_session_ui_and_theme_without_recreating_controller():
    context = js_function("perezosContext")
    for token in ("sessionId", "status", "role", "costume", "contextPressure",
                  "expanded", "theme", "timestamp", "colors"):
        assert token in context
    costume = js_function("perezosCostumeFor")
    assert "it.costume" in costume
    assert "role.hat" in costume
    theme = js_function("applyTheme")
    assert "CENTRO_VIEW.mascot?.setContext" in theme
    assert "createPerezOS" not in theme


def test_preference_migration_reads_old_key_once_and_new_code_uses_cc_mascot():
    migration = js_function("migrateMascotPreference")
    assert 'localStorage.getItem("cc-axo")' in migration
    assert HTML.count('"cc-axo"') == 1
    assert 'localStorage.getItem("cc-mascot")' in migration
    assert 'localStorage.setItem("cc-mascot"' in migration
    assert "function mascotVisible()" in HTML
    assert "function applyMascotPreference()" in HTML
    assert 'id="sw-mascot"' in HTML
    assert "Mascota PerezOS" in HTML


def test_old_axolotl_runtime_is_fully_removed():
    forbidden = [
        "AXO_PIX", "AXO_MOVES", "AXO_PAL", "AXO_THEMED", "_axoBuild",
        "axoAscii", "axoDraw", "axoIdleLoop", "axoVisible", "applyAxoPref",
        "axo-ascii", "axobreathe", "no-axo", 'class="aqua"',
        "Mascota ajolote", "ComandOS axolotl", "glub", "blub",
    ]
    assert not [token for token in forbidden if token in HTML]


def test_engine_has_no_network_or_per_part_dom_api():
    source = "\n".join(path.read_text() for path in sorted(Path("dash/perezos").glob("*.js")))
    for token in ["fetch(", "XMLHttpRequest", "WebSocket", "appendChild(", "insertBefore("]:
        assert token not in source


def test_stage_css_is_responsive_accessible_and_animation_free():
    assert ".perezos-stage" in CSS
    assert "width:256px" in CSS and "height:208px" in CSS
    assert "width:180px" in CSS and "height:148px" in CSS
    assert "image-rendering:pixelated" in CSS
    assert "body.no-mascot .perezos-stage" in CSS
    assert ".perezos-stage::before" in CSS and ".perezos-stage::after" in CSS
    assert "2px solid var(--brand)" in CSS
    assert "@keyframes" not in CSS
    assert "radial-gradient" not in CSS


def test_service_worker_and_backend_copy_name_perezos():
    assert 'const SHELL = "comandos-shell-v3"' in Path("dash/sw.js").read_text()
    assert "disfraces de PerezOS" in Path("bin/cc-dash").read_text()
    assert "PerezOS sea siempre" in Path("bin/cc-app").read_text()
