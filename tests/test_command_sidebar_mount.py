from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
HTML = (ROOT / "dash" / "index.html").read_text(encoding="utf-8")

def test_sidebar_mounts_command_module_and_drops_chat_and_old_sections():
    assert 'id="command-sidebar"' in HTML
    assert 'src="command-sidebar.js"' in HTML or "command-sidebar.js" in HTML
    for gone in ('id="op-chat"', 'id="toggle-chat"', 'id="sidebar-insights"', 'id="centro"', 'id="rows"', 'id="op-target"',
                 '/operator/chat/stream', 'opSend(', 'initOpChatSplit('):
        assert gone not in HTML, gone
    assert 'id="newsess"' in HTML          # el formulario de Nueva sesión se conserva

def test_workspace_js_has_no_chat_helpers():
    js = (ROOT / "dash" / "workspace.js").read_text(encoding="utf-8")
    for gone in ("setChatVisible", "openOperatorActions", "opApplyActions", "cc-chat-visible", "cc-chat-draft"):
        assert gone not in js, gone


def _js_function(name):
    start = HTML.index(f"function {name}(")
    depth = 0
    for i in range(HTML.index("{", start), len(HTML)):
        depth += {"{": 1, "}": -1}.get(HTML[i], 0)
        if depth == 0:
            return HTML[start:i + 1]
    raise AssertionError(name)


def test_sidebar_refreshes_on_target_or_agent_change_and_toasts_errors():
    import subprocess
    glue = "\n".join(_js_function(n) for n in (
        "isQuickTermSession", "activePaneTarget", "quickTermEntries", "refreshCommandSidebar", "syncCommandSidebar"))
    init = HTML[HTML.index("const CS = {"):].split("\n", 1)[0] + "\n" + \
        HTML[HTML.index("const CS_CLIS = "):].split("\n", 1)[0]
    script = r"""
const assert = require('node:assert/strict');
const S = {list: []}, openTerms = new Map([['term-qabc', {label: 'Terminal 10:00'}]]);
let sel = null, refreshes = 0, toasts = [], fail = false, release = null;
const pickSel = list => list.find(it => it.alive && it.session === sel) || null;
const toast = (m, err) => toasts.push([m, !!err]);
window = {commandSidebar: {state: {cliInPane: ''}, refresh() {
  refreshes++;
  if (fail) return Promise.reject(new Error('catálogo caído'));
  return new Promise(r => { release = r; });
}}};
""" + init + "\n" + glue + r"""
const tick = () => new Promise(r => setImmediate(r));
(async () => {
  S.list = [{session: 'demo', pane: '%2', alive: true, agent: 'claude', project: 'demo'},
            {session: 'term-qabc', pane: '%9', alive: true, agent: 'shell', project: 'term-qabc'}];
  sel = 'demo';
  assert.deepEqual(activePaneTarget(), {session: 'demo', pane: '%2', kind: 'pane', title: 'demo · %2'});
  syncCommandSidebar(); await tick();
  assert.equal(refreshes, 1);
  syncCommandSidebar(); await tick();                     // mismo destino y agente: sin refetch
  assert.equal(refreshes, 1);
  sel = 'term-qabc'; syncCommandSidebar(); await tick();  // en vuelo: se encola uno
  assert.equal(refreshes, 1);
  release(); await tick(); await tick();
  assert.equal(refreshes, 2);
  assert.equal(activePaneTarget().kind, 'term');
  assert.equal(activePaneTarget().title, 'Terminal 10:00 · %9');
  assert.deepEqual(quickTermEntries(), [{tabId: 'term-qabc', session: 'term-qabc', pane: '%9', label: 'Terminal 10:00'}]);
  release(); await tick();
  S.list[1].agent = 'codex'; syncCommandSidebar(); await tick();   // el pane arrancó codex
  assert.equal(refreshes, 3);
  release(); await tick();
  window.commandSidebar.state.cliInPane = 'codex';
  S.list[1].agent = 'grok'; S.list[1].agent = 'codex'; syncCommandSidebar(); await tick();
  assert.equal(refreshes, 3);                             // ya coincide con cliInPane
  fail = true; sel = 'demo'; syncCommandSidebar(); await tick(); await tick();
  assert.deepEqual(toasts, [['catálogo caído', true]]);
  console.log('ok');
})().catch(e => { console.error(e); process.exitCode = 1; });
"""
    out = subprocess.run(["node", "-e", script], capture_output=True, text=True, timeout=20)
    assert out.returncode == 0, out.stderr
    assert out.stdout.strip() == "ok"


def test_sidebar_is_wired_with_quick_terminal_and_builder_hooks():
    mount = _js_function("mountCommandSidebar")
    assert "ComandosCommandSidebar.createCommandSidebar(" in mount
    assert "openBuilder: () => window.chainBuilder && window.chainBuilder.open()" in mount
    assert "newTerm:" in mount and "quickTerminalInstance()" in mount
    assert "if(quickTerm) window.quickTerminal = quickTerm;" in HTML
    assert "mountCommandSidebar();" in HTML
    render = _js_function("render")
    assert "syncCommandSidebar();" in render
    for kept in ('$("#n-waiting").textContent = counts.waiting;', "refreshDesktopTabs();", "renderTabbar();"):
        assert kept in render, kept
