#!/usr/bin/env python3
import re
import subprocess
from pathlib import Path


HTML = Path("dash/index.html").read_text()


def test_usage_drawer_markup_exists():
    assert 'id="btn-usage"' in HTML
    assert 'id="usage"' in HTML


def test_usage_state_is_fetched_without_secret_rendering():
    assert 'api("/usage/state")' in HTML
    assert "renderUsage" in HTML
    forbidden = ["OPENAI_ADMIN_KEY", "ANTHROPIC_ADMIN_KEY", "x-api-key"]
    for text in forbidden:
        assert text not in HTML


def test_notifications_v2_prioritize_and_carry_action_buttons():
    # v2: el panel muestra SOLO lo importante (sugerencias,
    # modelos, skills/MCPs) con botones de accion; los turnos van agrupados
    # y el badge cuenta unicamente las clases prioritarias.
    assert "nfPriorityItems" in HTML
    for cls in ('"sugerencia"', '"modelo"', '"skill"', '"mcp"'):
        assert cls in HTML, cls
    # acciones: aplicar + abrir(Brave)/copiar + snooze/pin/dismiss.
    # Las cards de NOTICIA traen sus botones DIRECTOS en la propia card ("leer"
    # abre en el navegador del sistema via /open-url, "copiar" copia el link);
    # la de MODELO expande su detalle in-place ("detalle"). Un solo click.
    for act in ('"aplicar"', '"detalle"',
                '"leer"', '"copiar"', '"snooze"', '"pin"', '"dismiss"'):
        assert act in HTML, act
    # la card de modelo abre su detalle in-place, NUNCA el wizard de sesion nueva
    assert 'nsOpen(); return; }' not in HTML
    # se retiro el navegador MODAL interno: nada abre openUrlModal desde las cards
    assert 'data-inapp' not in HTML and 'openUrlModal:' not in HTML
    assert "cc-nf-snooze" in HTML and "cc-nf-pins" in HTML and "cc-nf-dismiss" in HTML
    # N2: los turnos rutinarios ya no se duplican en la campana; viven en la
    # franja de avisos por proyecto (dash/notifications.js) y la campana enlaza a ella.
    assert 'class="nf2-rest"' not in HTML and 'id="nf-to-strip"' in HTML
    assert '<script src="/notifications.js?v=watch1"></script>' in HTML
    # badge solo prioridades
    assert "el badge cuenta SOLO lo importante" in HTML


def test_switch_result_updates_harness_motor_model_and_effort_everywhere():
    # Tras /model/switch o /harness/switch, el poll debe pintar harness/motor/modelo/esfuerzo
    # en el item vivo (card, chip, popover). Si solo copia model/effort, ACP se queda
    # mostrando Claude y el effort no llega a las pills.
    poll = HTML.split("if(!MOTOR_PENDING.size||MOTOR_STATUS_BUSY) return;", 1)[1]
    poll = poll.split("},450);", 1)[0]
    script = """
const assert=require('node:assert/strict');
let MOTOR_STATUS_BUSY=false, MPOP=null, ticks=0, reply;
const target={session:'local',pane:'%0',agent:'claude',motor:'claude',model:'old',effort:'high',account:'old'};
const other={session:'signara',pane:'%21',model:'untouched'};
const beforeOther=JSON.stringify(other), S={list:[target,other]};
const MOTOR_PENDING=new Map(), SWITCH_SEEN=new Map();
const rowKey=i=>i.session+'|'+i.pane, motorTargetKey=rowKey;
const toast=()=>{}, tick=()=>ticks++, tf=es=>es;
async function api(path){
  const url=new URL(path,'http://test');
  assert.equal(url.pathname,'/model/status');
  assert.equal(url.searchParams.get('operationKey'),'local|%0');
  assert.equal(url.searchParams.get('operationId'),'op-current');
  return reply;
}
async function poll(){
""" + poll + """
}
(async()=>{
  for(const rolledBack of [false,true]){
    MOTOR_PENDING.set('local|%0',{operationId:'op-current'});
    reply={operationId:'op-stale',ok:true,harness:'wrong',model:'wrong'};
    const before=JSON.stringify(target);
    await poll();
    assert.equal(JSON.stringify(target),before);
    assert.equal(MOTOR_PENDING.size,1);
    reply={operationId:'op-current',state:rolledBack?'recovered':'confirmed',ts:rolledBack?2:1,
      ok:!rolledBack,rolledBack,harness:rolledBack?'claude':'acp',motor:'codex',model:'gpt-test',
      effort:'',harnessAccount:'work',motorAccount:'personal',routeId:'acp:codex'};
    await poll();
    assert.equal(target.agent,reply.harness);
    for(const key of ['motor','model','effort','harnessAccount','motorAccount','routeId'])
      assert.equal(target[key],reply[key],key);
    assert.equal(target.account,'work');
    assert.equal(MOTOR_PENDING.size,0);
    assert.equal(JSON.stringify(other),beforeOther);
    assert.equal(MOTOR_STATUS_BUSY,false);
  }
  assert.equal(ticks,2);
})().catch(e=>{console.error(e);process.exitCode=1;});
"""
    subprocess.run(['node', '-e', script], check=True)


def test_usage_chip_text_survives_without_session_rows():
    # Las filas de sesión del panel se retiraron (S2); el helper del chip se queda.
    assert 'id="rows"' not in HTML and 'class="usage-chip hidden"' not in HTML
    assert "function usageChipText" in HTML


def test_usage_ui_exposes_model_selector_with_preset_names():
    # Un solo control por concepto: menu de modelo con la intencion como etiqueta
    for preset in ("Ahorro", "Diario", "Difícil", "Máximo"):
        assert preset in HTML
    assert "MODEL_CHOICES" in HTML
    assert 'api("/model/switch"' in HTML
    assert "model: " in HTML


def test_usage_ui_labels_detected_panes_without_unattributed_noise():
    assert "function confidenceLabel" in HTML
    assert '"detected": "detectado"' in HTML
    assert "confidenceLabel(u.confidence)" in HTML


def test_usage_ui_switches_models_inline_per_pane():
    # Menu propio (no <select> nativo: el WebKitGTK viejo de la app no lo abre)
    assert "mdl-menu" in HTML
    assert "mdl-btn" in HTML
    assert "modelSelectEl" not in HTML
    assert "session: btn.dataset.session" in HTML
    assert "pane: btn.dataset.pane || undefined" in HTML
    assert 'role="listbox"' in HTML
    assert 'role="option"' in HTML


def test_session_cards_have_model_selector():
    assert "modelMenuEl" in HTML
    assert "function wireModelMenu" in HTML
    assert "mdl-cur" in HTML


def test_no_agent_badges_in_header():
    # Los badges de agente se eliminaron a peticion del usuario
    assert 'id="agent-pick"' not in HTML


def test_header_has_no_search_nor_open_project():
    # The old inline search was removed; Ctrl+K opens the session switcher and
    # the plus button sits beside the Sessions heading.
    assert 'id="new-form"' not in HTML
    assert 'id="new-name"' not in HTML
    assert 'id="q"' not in HTML
    switch_button = re.search(
        r'<button\b(?=[^>]*\bid="btn-switch")'
        r'(?=[^>]*\btitle="[^"]*Ctrl\+K)[^>]*>', HTML
    )
    assert switch_button
    # La cabecera "Sesiones" y su + salieron del panel con la barra de comandos
    # (S2); "Nueva sesión" vuelve a la cabecera en H1 con el id btn-newsess.
    assert 'id="sessions-title"' not in HTML


def test_command_sidebar_replaces_the_operator_chat_dock():
    assert 'id="command-sidebar"' in HTML
    for gone in ('id="op-chat"', 'id="tl-toggle"', 'id="tl-wrap"', 'Dile a ComandOS', 'id="op-model"', "op-compose",
                 'opStreamXhr("/operator/chat/stream"'):
        assert gone not in HTML, gone
    assert "nsOpen()" in HTML
    assert "Elegí un snippet" not in HTML
    assert "creá uno nuevo" not in HTML
    assert "Reabrí las existentes" not in HTML


def test_recent_closed_sessions_are_recoverable():
    # La sección "Recientes (cerradas)" salió del panel (S2); recuperar sigue
    # en el conmutador Ctrl+K, que lee /tab-history y llama /recover-tab.
    assert 'id="recent-wrap"' not in HTML and "renderRecent" not in HTML
    assert 'api("/tab-history")' in HTML
    assert '"/recover-tab"' in HTML.replace("'", '"')


def test_switcher_closes_on_outside_click():
    assert 'if(e.target === $("#sw-ov")) swClose();' in HTML


def test_terminal_tab_close_asks_with_modal_and_syncs_desktop():
    # La × abre un modal de confirmación; al aceptar cierra local Y en el
    # escritorio via /tab-close (sin esto el poll /tabs resucitaba la tab)
    assert 'id="tabclose"' in HTML
    assert "function askCloseTab" in HTML
    assert 'api("/tab-close"' in HTML
    # el prefijo "term:" se recorta antes de cerrar (bug: closeTerm no matcheaba)
    assert 'key.replace(/^term:/, "")' in HTML


def test_remote_tab_mirror_prunes_closed_desktop_tabs():
    # Tabs espejadas que ya no están en /tabs se PODAN (antes quedaban
    # huérfanas "solo en remoto")
    assert "o.mirrored && !want.has(s)" in HTML
    assert "addTermTab(t.session, t.label, true)" in HTML


def test_codex_dropdown_offers_models_with_reasoning():
    # Modelos vigentes de codex-cli 0.144.x (gpt-5.6 sol/terra/luna)
    assert "gpt-5.6-sol" in HTML
    assert "gpt-5.6-luna" in HTML
    assert "gpt-5.3-codex-spark" in HTML
    assert "data-effort" in HTML
    assert "effort: effort || undefined" in HTML


def test_opencode_menu_offers_providers_and_models():
    # OpenCode es el UNICO agente con seleccion de provider desde la UI
    assert 'api("/opencode/models")' in HTML
    assert "opencodeMenuHtml" in HTML
    assert "mdl-head" in HTML
    assert "model_name" in HTML


def test_card_usage_chip_is_full_width_line():
    # Texto COMPLETO siempre: el chip vive en su propia linea, sin ellipsis
    assert ".card .usage-chip" in HTML
    assert 'white-space:normal' in HTML


def test_header_shows_global_limit_percentages():
    # Los porcentajes globales viven SIEMPRE visibles en el header,
    # no solo dentro del drawer
    assert 'id="limits-strip"' in HTML
    assert "renderLimitsStrip" in HTML


def test_wizard_hides_not_routed_cells_from_unavailable_list():
    ns = HTML.split("function nsRender", 1)[1][:2800]
    assert "not_routed" in ns
    assert "nsSelectableMotors" in HTML or "r.selectable" in ns
