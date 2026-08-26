import json
import re
import subprocess
import textwrap
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


def render_harness(render_source: str) -> subprocess.CompletedProcess:
    script = textwrap.dedent(f"""
        const assert = require("node:assert/strict");
        global.window = global;
        function classes(initial = []){{
          const values = new Set(initial);
          return {{
            add(name){{ values.add(name); }},
            remove(name){{ values.delete(name); }},
            contains(name){{ return values.has(name); }},
            toggle(name, force){{
              const enabled = force === undefined ? !values.has(name) : !!force;
              if(enabled) values.add(name); else values.delete(name);
              return enabled;
            }},
          }};
        }}
        function element(initial = []){{
          return {{dataset:{{}}, style:{{}}, textContent:"", classList:classes(initial),
            listeners:new Map(),
            addEventListener(type, listener){{ this.listeners.set(type, listener); }},
            setAttribute(name, value){{ this[name] = String(value); }},
          }};
        }}
        const canvas = {{identity:"persistent-canvas"}};
        const stage = element();
        stage.querySelector = selector => selector === ".perezos-canvas" ? canvas : null;
        stage.getBoundingClientRect = () => ({{left:0, top:0, width:256, height:208}});
        const nodes = {{
          ".perezos-stage":stage, ".cx-hero":element(), ".cx-name":element(),
          ".cx-sub":element(), ".cx-model":element(), ".cx-acct":element(),
          ".cx-open":element(), ".cx-more":element(), ".cx-extra":element(["hidden"]),
          ".cx-pause":element(), ".cx-resume":element(["hidden"]),
          ".cx-kill":element(), ".cx-roles":element(),
        }};
        let shellWrites = 0;
        const box = {{dataset:{{}}, querySelector:selector => nodes[selector] || null,
          querySelectorAll(){{ return []; }}, replaceChildren(){{ this.replaced = true; }},
          get innerHTML(){{ return ""; }},
          set innerHTML(value){{ shellWrites += 1; this.lastMarkup = value; }},
        }};
        const wrap = {{classList:classes()}};
        const CENTRO_VIEW = {{sessionId:"", item:null, mascot:null, roleSig:""}};
        let controllersCreated = 0;
        window.ComandOSPerezOS = {{createPerezOS(receivedCanvas){{
          assert.strictEqual(receivedCanvas, canvas);
          controllersCreated += 1;
          return {{identity:`controller-${{controllersCreated}}`, contexts:[],
            setContext(value){{ this.contexts.push(value); }}, setVisible(){{}}, destroy(){{}},
            notifyInteraction(){{ return true; }},
          }};
        }}}};
        const $ = selector => selector === "#centro-wrap" ? wrap : selector === "#centro" ? box : null;
        const pickSel = list => list[0] || null;
        const usageForItem = () => ({{model:"gpt-5.4"}});
        const shortModel = value => value;
        const tierOf = () => "daily";
        const tierStyleOf = () => null;
        const roleOfModel = () => ({{id:"constructor", hat:"casco"}});
        const perezosCostumeFor = (item, role) => item.costume || role.hat;
        const perezosContext = item => ({{sessionId:item.session, status:item.status}});
        const attrEsc = value => String(value || "");
        const svg = () => "";
        const t = value => value;
        const tf = (es, en) => es;
        const toast = () => {{}};
        const openSession = async () => "";
        const api = async () => ({{}});
        const tick = () => {{}};
        const renderCentroRoles = () => {{}};
        const renderAdvice = () => {{}};
        const renderBrain = () => {{}};
        const mascotVisible = () => true;
        const applyMascotPreference = () => {{}};
        const PEREZOS_PHRASES = [["hola", "hello"]];
        let perezosPhraseIndex = 0;
        const renderSource = {json.dumps(render_source)};
        const renderCentro = eval("(" + renderSource + ")");
        const first = {{session:"same-session", project:"Uno", status:"idle", costume:"",
          contextPct:10, account:"main", ts:1}};
        renderCentro([first]);
        const firstCanvas = box.querySelector(".perezos-stage").querySelector(".perezos-canvas");
        const firstController = CENTRO_VIEW.mascot;
        renderCentro([{{...first, status:"working", costume:"fuego", contextPct:80, ts:2}}]);
        assert.strictEqual(box.querySelector(".perezos-stage").querySelector(".perezos-canvas"),
          firstCanvas, "same-session canvas identity changed");
        assert.strictEqual(CENTRO_VIEW.mascot, firstController,
          "same-session controller identity changed");
        assert.equal(shellWrites, 1, "same-session Control Center shell rebuilt");
        assert.equal(controllersCreated, 1, "same-session controller was recreated");
        assert.equal(firstController.contexts.length, 2);
        assert.equal(firstController.contexts[1].status, "working");
    """)
    return subprocess.run(["node", "-e", script], text=True, capture_output=True)


def test_same_session_updates_keep_exact_canvas_and_controller_identity():
    result = render_harness(js_function("renderCentro"))
    assert result.returncode == 0, result.stderr


def test_same_session_harness_rejects_reviewers_unconditional_innerhtml_mutation():
    render = js_function("renderCentro")
    mutated = render.replace(
        "const sameSession = CENTRO_VIEW.sessionId === it.session;",
        'box.innerHTML = "<div>forced rebuild</div>";\n'
        "  const sameSession = CENTRO_VIEW.sessionId === it.session;",
    )
    assert mutated != render
    result = render_harness(mutated)
    assert result.returncode != 0
    assert "same-session Control Center shell rebuilt" in result.stderr


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


def test_real_model_regexes_select_expected_role_props_and_ignore_invalid_patterns():
    roles = json.loads(Path("config/agent-roles.json").read_text())
    script = textwrap.dedent(f"""
        const assert = require("node:assert/strict");
        let ROLES = {json.dumps(roles)};
        const MT = {{alertTier:"high"}};
        {js_function("roleOfModel")}
        {js_function("perezosCostumeFor")}
        ROLES.roles.unshift({{id:"broken", hat:"none", matches:["["]}});
        const expected = [
          ["gpt-5.4", "constructor", "casco"],
          ["gpt-5.5", "arquitecto", "corona"],
          ["gpt-5.6-sol", "arquitecto", "corona"],
          ["glm-4.6", "constructor", "casco"],
        ];
        for(const [model, roleId, prop] of expected){{
          let role;
          assert.doesNotThrow(() => {{ role = roleOfModel(model); }});
          assert.equal(role && role.id, roleId, model);
          assert.equal(perezosCostumeFor({{}}, role, ""), prop, model);
        }}
    """)
    result = subprocess.run(["node", "-e", script], text=True, capture_output=True)
    assert result.returncode == 0, result.stderr


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
