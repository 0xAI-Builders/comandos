"""Crear una sesion desde el wizard lleva a ella.

cc-dash abre la pestana nueva en la app EN SEGUNDO PLANO (register_app_tab ->
app-tab-open.json restaura la pestana previa), asi que si el tablero no pide
foco despues de /session-new, el usuario se queda donde estaba.
"""
from pathlib import Path

HTML = (Path(__file__).resolve().parents[1] / "dash/index.html").read_text()


def test_wizard_navigates_to_created_session():
    start = HTML.index('$("#ns-go").addEventListener')
    call = HTML.index('api("/session-new"', start)
    end = HTML.index("}catch(err){", call)
    after = HTML[call:end]
    assert 'openInApp(r.session' in after
    assert 'openTerm(r.session' in after
    assert '"/focus", {session: r.session}' in after


def test_tab_bar_plus_opens_the_same_wizard_and_terminal_is_separate():
    init = HTML.split("function initTabNavigation(){", 1)[1].split("\n}\n", 1)[0]
    plus = init.split('const newBtn = document.getElementById("tab-new");', 1)[1].split("const termBtn", 1)[0]
    assert 'showView("panel")' in plus and 'getElementById("btn-newsess").click()' in plus
    terminal = init.split("const termBtn", 1)[1]
    assert "quickTerm.open()" in terminal and "/tab-new" not in terminal
