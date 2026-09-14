#!/usr/bin/env python3
"""Adaptador del catálogo: TOOLS y dispatch van al Dispatcher, no al parser regex."""
from __future__ import annotations

import operator_catalog as _cat
import json
import re
from copy import deepcopy
from operator_chat import format_tab_list
from operator_chat import fold

THEMES = [
    "noche", "dia", "calido", "termius", "bruno",
    "superglass", "neon", "contraste", "ubuntu",
]

TOOLS = _cat.anthropic_tools()


def anthropic_tools():
    return _cat.anthropic_tools()


def openai_tools():
    return _cat.openai_tools()


def provider_failure_message(failures):
    """Keep actionable provider failures in both streaming and saved replies."""
    messages = []
    for model, error in failures:
        detail = str(error or 'No respondió el proveedor.').strip()[:240]
        if re.search(r'out of extra usage|quota (?:exhausted|exceeded)|insufficient credits|credit balance.*(?:low|empty)', detail, re.I):
            detail = 'Cuota agotada; revisa el uso de esa cuenta o elige otro proveedor.'
        message = f'{model}: {detail}'
        if message not in messages:
            messages.append(message)
    return '\n'.join(messages) or 'No pude hablar con el modelo.'


def analysis_request(text):
    """Recognize requests for advice/data, never turn them into UI commands."""
    text = fold(text)
    # An explicit action remains actionable, even when its purpose is savings.
    if re.search(r'\b(aplica|cambia|activa|desactiva|abre|ejecuta|envia|enviale|manda|mata|cierra|guarda|apply|change|open|run|send|close)\b', text):
        return False
    return bool(re.search(r'\b(recomiend\w*|recomend\w*|sugier\w*|recommend\w*|suggest\w*|analiza\w*|analisis|analytics|estadisticas|consumo|cuotas?|ahorrar|ahorro|usage|status|estado|estados|tokens?|cost[eo]?s?|skills?|mcps?)\b', text)
                or re.search(r'\b(como van|como estan|que pasa|how are)\b', text))


def provider_messages(messages, family):
    """Translate tool history when a retry switches API protocol families."""
    out = []
    for message in messages:
        role, content = message.get('role'), message.get('content')
        if family == 'openai' and isinstance(content, list):
            text = ''.join(block.get('text', '') for block in content if block.get('type') == 'text')
            calls = [dict(id=b['id'], type='function', function={'name':b['name'], 'arguments':json.dumps(b['input'])})
                     for b in content if b.get('type') == 'tool_use']
            results = [b for b in content if b.get('type') == 'tool_result']
            if calls:
                out.append({'role':'assistant', 'content':text or None, 'tool_calls':calls})
            elif text:
                out.append({'role':role, 'content':text})
            for result in results:
                out.append({'role':'tool', 'tool_call_id':result['tool_use_id'], 'content':result.get('content', '')})
        elif family == 'anthropic' and role == 'tool':
            block = {'type':'tool_result', 'tool_use_id':message['tool_call_id'], 'content':content or ''}
            if out and out[-1]['role'] == 'user' and isinstance(out[-1]['content'], list):
                out[-1]['content'].append(block)
            else:
                out.append({'role':'user', 'content':[block]})
        elif family == 'anthropic' and message.get('tool_calls'):
            blocks = [{'type':'text', 'text':content}] if content else []
            for call in message['tool_calls']:
                blocks.append({'type':'tool_use', 'id':call['id'], 'name':call['function']['name'],
                               'input':json.loads(call['function']['arguments'])})
            out.append({'role':'assistant', 'content':blocks})
        else:
            out.append(deepcopy(message))
    return out


def agent_system_prompt(tabs, memory, active, context=None):
    names = format_tab_list(tabs)
    mem = (memory or "").strip()
    return (
        "Eres el operador de ComandOS en modo agente. Español latino (tú, no voseo).\n"
        "Para CUALQUIER botón, panel, ajuste o acción de la app DEBES llamar un tool; "
        "si no hay tool_use, no se ejecutó. Nunca digas que hiciste algo sin tool.\n"
        "Acciones con parámetro confirm SOLO tras confirmación explícita del usuario en este chat.\n"
        "Lecturas (list_*, *_state, get_*, *_latest) úsalas libremente para responder con datos reales.\n"
        "Para estado usa session_status; para consumo de esta sesión usa session_usage; "
        "para skills/MCPs usa session_brain y extension_usage. scope=all en extension_usage es global. "
        "list_sessions, usage_state, usage_analytics, usage_provider_compare y dedication_stats son globales: no atribuyas sus totales al panel seleccionado. "
        "Consulta herramientas actuales antes de afirmar estado, métricas o recomendar cambios. "
        "Compara con usage_analytics y sus tamaños de muestra, evidence y disclaimer; optimization_plans contiene perfiles propuestos, no ahorros demostrados. "
        "Distingue observaciones, estimaciones de precio API y coste pagado. No inventes tokens, costes o ahorros por skill/MCP. "
        "Datos ausentes o sin atribución son desconocidos, no cero; configuración no demuestra activación ni uso. "
        "Para recomendaciones explica evidencia, ámbito, ventana temporal y límites; si faltan datos, propone cómo medir. "
        "Recomendar no autoriza ejecutar cambios. Solo aplica acciones solicitadas expresamente. "
        "Una acción pendiente/enviada no está confirmada. Un fallo de lectura no es éxito.\n"
        f"Pestañas: {names}\nActiva: {active or '—'}\nMemoria:\n{mem or '(vacía)'}\n"
        "Contexto seleccionado: " + json.dumps(context or {'session': active}, ensure_ascii=False) + "\n"
        "Este contexto prevalece sobre proyectos mencionados en el historial. No uses get_active_tab para sustituirlo: ese tool devuelve el foco del escritorio.\n"
        "Temas: " + ", ".join(THEMES) + ".\n"
        "Grupos de acciones disponibles:\n" + _cat.groups_summary() + "\n"
        "Responde corto. No menciones HTTP ni 401. No programes repos: eso va al pane de la IA."
    )


def dispatch_tool(name, args, dispatcher=None, **_legacy):
    if dispatcher is None:
        raise RuntimeError("dispatch_tool necesita dispatcher")
    return dispatcher.run(name, args)
