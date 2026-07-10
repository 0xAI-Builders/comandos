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
    assert '_tabs = Gtk.Button(label="▦")' in src
    assert '_tabs.connect("clicked", open_tabs_overview)' in src
    assert "_actions.pack_start(_tabs" in src


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


def test_close_tab_confirms_before_closing():
    src = function_source("close_tab")
    assert "Gtk.MessageDialog" in src
    assert "ButtonsType.YES_NO" in src
    # No cierra si el usuario no confirma
    assert "if resp != Gtk.ResponseType.YES:" in src
    assert "return" in src


if __name__ == "__main__":
    test_tab_labels_are_saved_in_visual_notebook_order()
    test_notebook_tabs_are_reorderable_and_saved_after_drag()
    test_visual_tabs_overview_button_is_present()
    test_visual_tabs_overview_can_focus_and_close_tabs()
    test_ctrl_k_can_close_selected_open_tab()
    test_close_tab_confirms_before_closing()
