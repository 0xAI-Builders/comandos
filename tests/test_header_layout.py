"""H1: cabecera «Ordenada» de dos filas; Servidores abre la fila SSH de siempre."""
import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HTML = (ROOT / "dash" / "index.html").read_text(encoding="utf-8")
CSS = (ROOT / "dash" / "workspace.css").read_text(encoding="utf-8")


def js_function(name):
    start = HTML.index(f"function {name}(")
    depth = 0
    for i in range(HTML.index("{", start), len(HTML)):
        depth += {"{": 1, "}": -1}.get(HTML[i], 0)
        if depth == 0:
            return HTML[start:i + 1]
    raise AssertionError(name)


def test_row_one_order_and_servers_button():
    head = HTML[HTML.index('<header class="hdr-ordered"'):HTML.index('</header>')]
    order = re.findall(r'id="(btn-menu|n-waiting|btn-terminal|btn-newsess|btn-switch|btn-snippets|btn-usage|btn-remote|btn-servers|btn-news|btn-notif|btn-pomo|btn-settings|clock)"', head)
    assert order == ["btn-menu", "n-waiting", "btn-terminal", "btn-newsess", "btn-switch", "btn-snippets", "btn-usage", "btn-remote", "btn-servers", "btn-news", "btn-notif", "btn-pomo", "btn-settings", "clock"]
    for tag_id, icon in (("btn-menu", "menu"), ("btn-terminal", "terminal"), ("btn-newsess", "plus"), ("btn-servers", "server")):
        tag = head[head.index(f'id="{tag_id}"') - 40: head.index(f'id="{tag_id}"') + 260]
        assert 'class="hdr-btn' in tag and f'data-icon="{icon}"' in tag and 'class="hdr-lbl"' in tag, tag_id
    topbar = HTML[HTML.index('<div id="topbar">'):HTML.index('<div id="content">')]
    assert 'id="ssh-bar"' not in topbar                       # ya no es fila permanente
    assert 'id="servers-panel"' in HTML and 'id="ssh-bar"' in HTML and 'id="ssh-chips"' in HTML and 'id="ssh-manage"' in HTML


def test_tabs_row_keeps_sort_and_drops_terminal_buttons():
    nav = HTML[HTML.index('<nav id="app-navigation"'):HTML.index('</nav>')]
    assert 'id="tab-sort"' in nav and 'id="tab-panel"' in nav
    assert 'id="tab-terminal"' not in nav and 'id="tab-new"' not in nav


def test_relocated_controls_keep_their_ids_inside_the_menu():
    start = HTML.index('id="menu-panel"'); menu = HTML[start:start + 6000]
    for i in ("btn-theme", "btn-sov", "btn-mute", "vol-top", "limits-strip", "toggle-overview", "open-session-profiles", "open-extension-usage"):
        assert f'id="{i}"' in menu, i
    for i in ("btn-theme", "btn-sov", "btn-mute", "vol-top", "limits-strip", "toggle-overview"):
        assert HTML.count(f'id="{i}"') == 1, f"{i} se mueve, no se duplica"
    assert 'id="workspace-tools"' not in HTML                  # la fila vacía se retira
    assert 'id="session-overview"' in HTML[HTML.index('<div id="side-top">'):]


def test_ssh_functions_untouched_and_styles_cover_new_buttons():
    for fn in ("async function loadSsh(", "async function openSshTab(", "async function connectHost(", "async function setupSshKey(", "function toggleSshManager("):
        assert fn in HTML, fn
    assert "--hdr-key" in CSS and "#btn-servers" in CSS
    assert "max-height:calc(100dvh - 16px);overflow:auto" in CSS[CSS.index("#menu-panel{"):]
    for bid in ("btn-servers", "btn-terminal", "btn-newsess", "btn-menu"):
        assert f'html[data-btn-style="consola"] header #{bid}{{--face:' in CSS, bid


def test_ssh_bar_markup_and_toggle_iife_are_byte_identical():
    bar = ('<div id="ssh-bar">\n'
           '  <button class="lbl" id="ssh-toggle" type="button" aria-expanded="false" title="Mostrar todos los servidores">'
           '<span data-icon="server" data-size="12"></span> <span id="ssh-count">Servidores</span> '
           '<span data-icon="chevron" data-size="12" class="chev"></span></button>\n'
           '  <span id="ssh-chips"></span>\n'
           '  <button id="ssh-manage" title="Agregar, editar o borrar (viven en ~/.ssh/config)">gestionar</button>\n'
           '</div>')
    assert bar in HTML
    assert ('<div id="servers-panel" class="hidden">\n' + bar + '\n</div>') in HTML   # solo cambia el contenedor
    iife = ('(function(){\n'
            '  const bar = $("#ssh-bar"), tg = $("#ssh-toggle");\n'
            '  if(!bar || !tg) return;\n'
            '  const apply = open => { bar.classList.toggle("open", open); tg.setAttribute("aria-expanded", String(open)); };\n'
            '  apply(localStorage.getItem("cc-ssh-open") === "1");\n'
            '  tg.addEventListener("click", ()=>{ const open = !bar.classList.contains("open"); localStorage.setItem("cc-ssh-open", open ? "1" : "0"); apply(open); });\n'
            '})();')
    assert iife in HTML


def test_menu_icon_and_mobile_counters():
    assert re.search(r"\n  menu:\s+'<svg viewBox=\"0 0 24 24\"", HTML)
    for full, short in (("esperan", "esp."), ("listos", "list."), ("trabajando", "trab.")):
        assert f'<span class="cnt-long">{full}</span><span class="cnt-short">{short}</span>' in HTML
    mobile = CSS[CSS.index("@media (max-width:640px){header.hdr-ordered"):]
    assert ".cnt-long{display:none}" in mobile and ".cnt-short{display:inline}" in mobile


def test_header_buttons_are_wired_outside_the_remote_only_tab_init():
    init = js_function("initTabNavigation")
    assert "tab-terminal" not in init and "tab-new" not in init and "btn-terminal\")" not in init
    assert "ComandosQuickTerminal.createQuickTerminal" in init and "!window.quickTerminal" in init
    actions = js_function("initHeaderActions")
    assert "quickTerminalInstance()" in actions and ".open()" in actions
    assert "NS.intoPane = null; nsOpen();" in actions and 'showView("panel")' in actions
    assert "initHeaderActions();" in HTML


NODE = r'''
const assert = require('node:assert/strict');
const src = process.argv[1];
function el(id){
  const cls = new Set(), attrs = {}, listeners = {};
  const e = {id, style:{}, parentNode:null, offsetWidth:200,
    classList:{contains:c=>cls.has(c), add:c=>cls.add(c), remove:c=>cls.delete(c),
      toggle:(c,f)=>{ const on = f === undefined ? !cls.has(c) : f; on ? cls.add(c) : cls.delete(c); return on; }},
    setAttribute:(k,v)=>{ attrs[k]=String(v); }, getAttribute:k=>attrs[k],
    addEventListener:(t,f)=>{ (listeners[t] ||= []).push(f); },
    fire:(t,ev={})=>{ for(const f of listeners[t]||[]) f({target:e, composedPath:()=>[e], stopPropagation(){}, ...ev}); },
    contains:x=>x===e, closest:()=>null, focus(){ e.focused=true; },
    getBoundingClientRect:()=>({left:20,bottom:50,height:36})};
  return e;
}
const ids = {}; for(const id of ['menu-panel','btn-menu','servers-panel','btn-servers','ssh-bar','ssh-toggle']) ids[id]=el(id);
ids['menu-panel'].classList.add('hidden'); ids['servers-panel'].classList.add('hidden');
const docL = {}, bodyKids = [];
global.innerWidth = 1200;
global.document = {body:{appendChild:x=>{ bodyKids.push(x); x.parentNode = global.document.body; }},
  addEventListener:(t,f)=>{ (docL[t] ||= []).push(f); }};
global.$ = s => ids[s.slice(1)];
let loads = 0; global.loadSsh = () => { loads++; };
eval(src + '\ninitHeaderPopovers();');
const menu = ids['menu-panel'], mb = ids['btn-menu'], sp = ids['servers-panel'], sb = ids['btn-servers'];
assert.deepEqual(bodyKids, [menu]);                         // popover fijo en <body>, como #notif-panel
mb.fire('click');
assert.equal(menu.classList.contains('hidden'), false);
assert.equal(mb.getAttribute('aria-expanded'), 'true');
assert.equal(menu.style.top, '58px');
// clic dentro del menú no lo cierra; clic afuera sí
for(const f of docL.click) f({target:menu, composedPath:()=>[menu]});
assert.equal(menu.classList.contains('hidden'), false);
const outside = el('x');
for(const f of docL.click) f({target:outside, composedPath:()=>[outside]});
assert.equal(menu.classList.contains('hidden'), true);
assert.equal(mb.getAttribute('aria-expanded'), 'false');
// Escape cierra y devuelve el foco a ☰
mb.fire('click');
for(const f of docL.keydown) f({key:'Escape'});
assert.equal(menu.classList.contains('hidden'), true);
assert.equal(mb.focused, true);
// lo que abre otra vista cierra el menú (Ajustes, Analytics, tarjetas, perfiles); el volumen no
const closers = ['#toggle-overview', '#open-session-profiles', '#open-extension-usage', '#btn-sov', '#btn-theme', '#limits-strip .pill'];
const inMenu = sel => ({target:{closest:q => q.split(',').includes(sel) ? {} : null}});
for(const sel of closers){
  mb.fire('click'); assert.equal(menu.classList.contains('hidden'), false);
  menu.fire('click', inMenu(sel));
  assert.equal(menu.classList.contains('hidden'), true, sel);
  assert.equal(mb.getAttribute('aria-expanded'), 'false');
}
mb.fire('click'); menu.fire('click', inMenu('#vol-top'));
assert.equal(menu.classList.contains('hidden'), false);
for(const f of docL.keydown) f({key:'Escape'});
// Servidores: alterna el panel, abre la fila SSH real y la recarga
sb.fire('click');
assert.equal(sp.classList.contains('hidden'), false);
assert.equal(sb.getAttribute('aria-expanded'), 'true');
assert.equal(ids['ssh-bar'].classList.contains('open'), true);
assert.equal(loads, 1);
sb.fire('click');
assert.equal(sp.classList.contains('hidden'), true);
assert.equal(sb.getAttribute('aria-expanded'), 'false');
assert.equal(loads, 1);
console.log('ok');
'''


def test_menu_and_servers_popovers_behave():
    out = subprocess.run(["node", "-e", NODE, js_function("initHeaderPopovers")],
                         capture_output=True, text=True, timeout=20)
    assert out.returncode == 0, out.stderr
    assert out.stdout.strip() == "ok"
