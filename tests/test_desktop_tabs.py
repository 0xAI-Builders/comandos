#!/usr/bin/env python3
import ast
from pathlib import Path


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
    assert "_headerbar.pack_end(_tabs" in src   # ahora vive en el headerbar custom


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
