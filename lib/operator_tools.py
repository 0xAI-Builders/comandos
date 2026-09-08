#!/usr/bin/env python3
"""Adaptador del catálogo: TOOLS y dispatch van al Dispatcher, no al parser regex."""
from __future__ import annotations

import operator_catalog as _cat
from operator_chat import format_tab_list

THEMES = [
    "noche", "dia", "calido", "termius", "bruno",
    "superglass", "neon", "contraste", "ubuntu",
]

TOOLS = _cat.anthropic_tools()


def anthropic_tools():
    return _cat.anthropic_tools()


def openai_tools():
    return _cat.openai_tools()


def agent_system_prompt(tabs, memory, active):
    names = format_tab_list(tabs)
    mem = (memory or "").strip()
    return (
        "Eres el operador de ComandOS en modo agente. Español latino (tú, no voseo).\n"
        "Para CUALQUIER botón, panel, ajuste o acción de la app DEBES llamar un tool; "
        "si no hay tool_use, no se ejecutó. Nunca digas que hiciste algo sin tool.\n"
        "Acciones con parámetro confirm SOLO tras confirmación explícita del usuario en este chat.\n"
        "Lecturas (list_*, *_state, get_*, *_latest) úsalas libremente para responder con datos reales.\n"
        f"Pestañas: {names}\nActiva: {active or '—'}\nMemoria:\n{mem or '(vacía)'}\n"
        "Temas: " + ", ".join(THEMES) + ".\n"
        "Grupos de acciones disponibles:\n" + _cat.groups_summary() + "\n"
        "Responde corto. No menciones HTTP ni 401. No programes repos: eso va al pane de la IA."
    )


def dispatch_tool(name, args, dispatcher=None, **_legacy):
    if dispatcher is None:
        raise RuntimeError("dispatch_tool necesita dispatcher")
    return dispatcher.run(name, args)
