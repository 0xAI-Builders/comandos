#!/usr/bin/env python3
import ast
from pathlib import Path


def test_tab_titles_receive_live_verified_model_labels():
    source = Path("bin/cc-app").read_text()
    dash = Path("bin/cc-dash").read_text()
    assert "APP_TAB_MODELS_FILE" in source and "APP_TAB_MODELS_FILE" in dash
    assert "def _refresh_tab_models" in source
    assert 'model_lbl.get_style_context().add_class("tab-model")' in source
    assert "def write_app_tab_models" in dash
    assert "write_app_tab_models(items)" in dash


SRC = Path("bin/cc-app").read_text()


def functions():
    tree = ast.parse(SRC)
    return {
        node.name: ast.get_source_segment(SRC, node)
        for node in tree.body
        if isinstance(node, ast.FunctionDef)
    }


def function_source(name):
    funcs = functions()
    assert name in funcs, f"missing helper: {name}"
    return funcs[name]


def load_order_helper():
    src = function_source("ordered_tab_labels")
    ns = {}
    exec(src, ns)
    return ns


def test_tab_labels_are_saved_in_visual_notebook_order():
    ns = load_order_helper()

    labels = {"alpha": "Alpha", "beta": "Beta", "gamma": "Gamma"}

    assert ns["ordered_tab_labels"](["gamma", "alpha"], labels) == {
        "gamma": "Gamma",
        "alpha": "Alpha",
        "beta": "Beta",
    }


def test_notebook_tabs_are_reorderable_and_saved_after_drag():
    src = SRC

    assert "nb.set_tab_reorderable(box, True)" in src
    assert "nb.set_tab_reorderable(hub, False)" in src
    assert 'nb.connect("page-reordered", on_tab_reordered)' in src
    assert "def current_tab_order()" in src
    assert "ordered_tab_labels(current_tab_order(), labels)" in function_source("save_tabs")


def test_visual_tabs_overview_button_is_present():
    src = SRC

    assert "def open_tabs_overview" in src
    # El botón usa el ícono Lucide "layers" via _icon_btn (set unificado con
    # el dashboard HTML — ver DESIGN.md §3), cableado a open_tabs_overview.
    assert '"layers"' in src
    assert "open_tabs_overview" in src
    # Ya NO se empaqueta en la barra (el usuario solo usa "+"): sigue por atajo.
    assert "_headerbar.pack_end(_tabs" not in src
    assert "_headerbar.pack_end(_plus)" in src


def test_visual_tabs_overview_can_focus_and_close_tabs():
    src = function_source("open_tabs_overview")

    assert "for page in notebook_pages()" in src
    assert "nb.set_current_page(nb.page_num(page))" in src
    assert "close_tab(key)" in src
    assert "Gtk.ScrolledWindow()" in src


def test_ctrl_k_can_close_selected_open_tab():
    src = function_source("open_switcher")

    assert "def close_selected()" in src
    assert "close_tab(key)" in src
    assert "Gdk.KEY_Delete" in src
    assert "Gdk.KEY_w" in src


def test_remote_open_requests_open_background_tab():
    # cc-dash escribe app-tab-open.json cuando el remoto abre/crea una tab;
    # la app la abre SIN robar el foco de la pestana activa
    assert "app-tab-open.json" in SRC
    assert "on_tab_open_request" in SRC
    src = function_source("on_tab_open_request")
    assert "sess in tabs" in src          # no re-abre ni roba foco si ya existe
    assert "set_current_page(cur)" in src  # abre en segundo plano


def test_remote_close_requests_skip_confirm():
    # cc-dash escribe app-tab-close.json tras confirmar en el tablero;
    # la app cierra esa tab sin re-preguntar
    assert "def close_tab(key, confirm=True):" in SRC
    assert "close_tab(sess, confirm=False)" in SRC
    assert "app-tab-close.json" in SRC
    assert "on_tab_close_request" in SRC


def test_close_tab_confirms_before_closing():
    src = function_source("close_tab")
    # El confirm es custom (_confirm_must_answer): en WSLg, MessageDialog+run()
    # retornaba NONE si el compositor destruia la ventana. Se valida el
    # COMPORTAMIENTO: pregunta antes y NO cierra sin confirmacion.
    assert "_confirm_must_answer(" in src
    assert "¿Cerrar la pestaña" in src
    # No cierra si el usuario no confirma
    assert "if not resp:" in src
    assert "return" in src


def test_modals_close_on_click_outside():
    # Arquitectura de modales embebidos (Gtk.Overlay + backdrop): click en el
    # backdrop cierra cuando dismissable=True (default). switcher y overview
    # usan show_modal_panel sin dismissable=False; el confirm de cerrar tab
    # SI es must-answer (dismissable=False).
    src_all = SRC
    helper = function_source("show_modal_panel")
    assert "cc-modal-backdrop" in helper
    assert "BUTTON_PRESS_MASK" in helper
    assert "dismissable" in helper
    assert "show_modal_panel(w, on_key=on_sw_key)" in function_source("open_switcher")
    assert "show_modal_panel(w)" in function_source("open_tabs_overview")
    assert "dismissable=False" in function_source("_confirm_must_answer")
    # WebKit/VTE tienen ventana nativa que se comia el click "afuera". El fix
    # NO usa grab de toolkit (rompia seleccion del ListBox y foco del Entry):
    # panel envuelto en EventBox con ventana propia, stacking X elevado a mano
    # (contenido < backdrop < panel) para que el backdrop capte los clicks de
    # afuera aun sobre el webview y los hijos reciban los suyos normal.
    helper = function_source("show_modal_panel")
    assert "grab_add" not in helper          # NUNCA mas: rompia los modales
    assert "set_visible_window(True)" in helper
    assert ".raise_()" in helper
    assert "wrapper" in helper


def test_tab_scroll_arrows_have_padding():
    # Las flechas de scroll del notebook no deben quedar pegadas a la 1a tab
    assert "notebook > header > tabs > arrow" in SRC
    css = SRC.split('APP_CSS = """', 1)[1].split('"""', 1)[0]
    arrow_rule = css.split("> tabs > arrow {", 1)[1].split("}", 1)[0]
    assert "padding" in arrow_rule and "margin" in arrow_rule


def test_terminals_paste_file_paths_on_drop():
    # Soltar archivos en una terminal pega sus rutas shell-quoted (nunca ejecuta)
    src = SRC
    assert "add_uri_targets" in src
    assert "drag-data-received" in src
    assert "shlex.quote" in src
    assert 'pu.scheme == "file"' in src
    assert "feed_child" in src


def test_ssh_terminal_scroll_routes_each_pointer_position_to_tmux_history():
    make_term = function_source("make_term")
    handler = function_source("on_ssh_scroll")
    flush = function_source("_flush_ssh_scroll")
    open_tab = function_source("open_tab")

    assert 'term.connect("scroll-event", on_ssh_scroll)' in make_term
    assert "term.get_has_selection()" in handler
    assert "ssh_host_from_session(sess)" in handler
    assert "terminal_grid_at_event(term, event)" in handler
    assert "http_post" in flush
    assert '"/tmux-scroll"' in flush
    assert '"session": sess' in flush
    assert '"col": col' in flush
    assert '"row": row' in flush
    assert "box._term._session_hint = sess" in open_tab


def test_raise_main_window_uses_keep_above_pulse_for_gnome():
    src = function_source("raise_main_window")
    assert "set_keep_above(True)" in src
    assert "set_keep_above(False)" in src   # se suelta, no queda pineada
    assert "deiconify" in src


if __name__ == "__main__":
    test_remote_open_requests_open_background_tab()
    test_terminals_paste_file_paths_on_drop()
    test_remote_close_requests_skip_confirm()
    test_tab_labels_are_saved_in_visual_notebook_order()
    test_notebook_tabs_are_reorderable_and_saved_after_drag()
    test_visual_tabs_overview_button_is_present()
    test_visual_tabs_overview_can_focus_and_close_tabs()
    test_ctrl_k_can_close_selected_open_tab()
    test_close_tab_confirms_before_closing()
    test_modals_close_on_click_outside()
    test_tab_scroll_arrows_have_padding()
    test_raise_main_window_uses_keep_above_pulse_for_gnome()


def test_ctrl_o_is_not_captured_by_the_app():
    # Ctrl+O pertenece al CLI/terminal; no debe abrir un overlay de tabs.
    assert 'e.keyval in (Gdk.KEY_o, Gdk.KEY_O)' not in SRC


def test_dashboard_url_is_cache_busted_on_app_start():
    assert 'BASE_URL = "http://127.0.0.1:4777"' in SRC
    assert '_DASH_V' in SRC
    assert 'URL = f"{BASE_URL}/?app=1&v={_DASH_V}"' in SRC


def test_grok_tabs_snapshot_exact_session_and_resume_with_grok_home():
    assert 'ent["agent"] == "grok"' in SRC
    assert 'it.get("agentSessionId")' in SRC
    assert 'GROK_HOME_DEFAULT' in SRC
    assert 'grok --resume ' in SRC
    assert 'grok --continue' in SRC


def test_claude_snapshot_and_resume_scan_all_account_config_dirs():
    assert '~/.claude-accounts/*/sessions' in SRC
    assert '"config_dir": os.path.dirname(directory)' in SRC
    assert 'ent["claude_config_dir"]' in SRC
    assert 'projects = os.path.join(cfg, "projects")' in SRC
    assert 'CLAUDE_CONFIG_DIR=' in functions()["resume_command"]


def test_right_click_shell_can_start_ai_session_like_plus():
    """Click derecho en una terminal NORMAL (zsh, sin CLI de IA) debe ofrecer
    arrancar una sesión de IA en ESE pane — el mismo wizard que el +. Sin esto
    hay que ir al tablero y crear otra pestaña."""
    src = function_source("on_term_button")
    assert "Iniciar sesión de IA" in src or "Start AI session" in src
    assert "open_ai_session_here" in SRC or "nsOpenForPane" in SRC
    assert "openMotorFor" in SRC or "nsOpen" in SRC
    assert "/harness/switch" in SRC or "nsOpenForPane" in SRC or "openAiHere" in SRC


def test_terminal_model_bar_shows_certainty_and_actions():
    # Pildoras POR PANE, ancladas a la linea de titulo tmux (geometria real
    # de list-panes): modelo verificado con color de marca y boton de motor
    # que abre el picker PROBADO del tablero. Sin boton harness (feedback).
    src = Path("bin/cc-app").read_text()
    assert "_attach_model_bar" in src and "_pane_pill" in src
    assert "_pane_geometry" in src and "Gtk.Overlay" in src
    for state in ("verified", "changing", "detecting"):
        assert state in src
    assert "openMotorFor" in src and "openHarnessFor" not in src
    assert "pane-pill" in src and "pp-claude" in src
    # la pestana local no se registra en tabs (rompia save_tabs y el strip)
    assert 'tabs.setdefault("local", hub)' not in src
    # la VTE nunca se empuja de lado: el overlay envuelve SOLO la terminal
    assert "ov.add(term)" in src
    # el riel tmux queda VACIO: el titulo @ccmodel no se pinta detras de la pildora
    assert '"pane-border-format"' in src
    assert 'opacity:1' in src


def test_dash_publishes_per_pane_model_detail_for_the_bar():
    dash_src = Path("bin/cc-dash").read_text()
    seg = dash_src.split("def write_app_tab_models", 1)[1][:4000]
    for field in ('"panes"', '"state"', '"changing"', '"verified"', '"detecting"',
                  '"hAcct"', '"mAcct"', '"target"'):
        assert field in seg, field
    # mismo criterio determinista que el borde tmux (MOTOR_RESULT pendiente)
    assert "MOTOR_RESULT" in seg
    html = Path("dash/index.html").read_text()
    assert "window.openMotorFor" in html and "window.openHarnessFor" in html


def test_archive_and_snapshot_recognize_cc_acp_as_acp_harness():
    assert '"cc-acp"' in SRC
    archive = function_source("archive_tab")
    assert "cc-acp" in archive
    assert 'ent["agent"] == "acp"' in function_source("snapshot_tabs")
    assert 'agent == "acp"' in function_source("resume_command")


def test_sane_flags_keeps_values_and_drops_orphan_value_flags():
    """Regresión del apagón 2-sep: el snapshot guardaba ["--model","--effort"]
    sin valores y la resurrección lanzaba `claude --resume id --model --effort`
    (claude tomaba "--effort" como modelo; todas las sesiones revivían rotas)."""
    namespace = {}
    exec(function_source("_sane_flags"), namespace)  # helper puro, sin deps GTK
    exec("_RESUME_SKIP_FLAGS = " + repr({"--resume", "--continue", "-r", "-c", "--last"}), namespace)
    exec(SRC.split("_VALUE_FLAGS = ")[1].split("\n\n")[0].join(["_VALUE_FLAGS = ", ""]), namespace)
    sane = namespace["_sane_flags"]
    # captura pares flag+valor y omite resume+id
    assert sane(["--resume", "abc-123", "--model", "fable-5", "--effort", "high",
                 "--dangerously-skip-permissions"]) == \
        ["--model", "fable-5", "--effort", "high", "--dangerously-skip-permissions"]
    # snapshot viejo roto: value-flags huérfanos se descartan, no se propagan
    assert sane(["--model", "--effort", "--dangerously-skip-permissions"]) == \
        ["--dangerously-skip-permissions"]
    assert sane(["--continue", "--model"]) == []


def test_resume_command_and_proc_flags_use_the_sane_helpers():
    assert "_sane_flags(argv[1:])" in function_source("_proc_flags")
    resume = function_source("resume_command")
    assert "_sane_flags(" in resume
    for cli in ("claude", "codex", "grok"):
        assert f'_which_cli("{cli}")' in resume, f"resume de {cli} debe detectar el binario sin depender del PATH"


def test_shell_panes_get_a_visible_start_ai_pill_and_ctrl_shift_a():
    # Los panes sin agente no tenian pildora: la unica via era el click derecho.
    # Ahora: boton 'Iniciar IA aqui' sobre cada shell, atajo Ctrl+Shift+A, y
    # el menu contextual resuelve la sesion aunque el tty no este registrado.
    src = SRC
    assert "def _shell_pill(sess, pane_id)" in src
    assert '"#{pane_id} #{pane_left} #{pane_top} #{pane_width} #{pane_current_command}"' in src
    assert "_PANE_CMD.get(pane_id, \"\") not in _SHELLS" in src
    assert 'open_ai_session_here(cur_sess or str(key or "").partition(":")[0], pane_id)' in src
    assert "if ctrl and shift and e.keyval in (Gdk.KEY_A, Gdk.KEY_a):" in src
    assert "console.error('dash_js: '" in src
    assert 'tf("Iniciar IA aquí","Start AI here")' in Path("dash/index.html").read_text()


def test_pills_sit_on_the_reserved_tmux_border_row_not_on_content():
    # pane-border-status top + formato vacio reserva la fila pane_top-1.
    # La pildora va AHI, no en la primera fila de contenido (tapaba el prompt).
    assert "def _pill_row_y(m_t, top, chh)" in SRC
    assert "row = top - 1 if top >= 1 else 0" in SRC
    assert SRC.count("_pill_row_y(m_t, top, chh)") == 2
    assert "max(0, m_t + max(0, top) * chh)" not in SRC
