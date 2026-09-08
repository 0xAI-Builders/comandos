# Operador con streaming y catálogo completo de acciones — Plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Que el chat del operador responda en streaming, en menos de 2 s al primer token, y pueda ejecutar CADA acción que existe en la UX de ComandOS (tablero remoto, terminal web y app GTK) mediante function calling.

**Architecture:** Un catálogo declarativo (`lib/operator_catalog.py`) describe cada acción como `ToolSpec` con un target (`api` loopback a cc-dash, `ui` acción al navegador, `app` comando a cc-app por archivo, `local` callback existente). Un despachador genérico (`lib/operator_dispatch.py`) ejecuta cualquier target. `POST /operator/chat/stream` hace SSE reenviando deltas del proveedor y eventos de tool. El tablero consume el stream con `fetch` + `ReadableStream`. cc-app gana un observador de `app-command.json` para las acciones que solo existen en GTK.

**Tech Stack:** Python 3.10+ (stdlib: urllib, json, threading), pytest, cc-dash (`socketserver.ThreadingTCPServer`), JS vanilla en `dash/index.html` y `dash/term.html`, GTK/PyGObject en `bin/cc-app`, tmux.

**Spec:** `docs/superpowers/specs/2026-09-07-operator-full-catalog-design.md`

## Global Constraints

- Sin dependencias nuevas de pip ni npm. Solo stdlib.
- Nada escucha fuera de `127.0.0.1`; el loopback a cc-dash lleva `X-Comandos-Token`.
- Toda acción destructiva exige `confirm: true` en sus parámetros (lista en Task 2).
- Copy en español latino (tú, no voseo), igual que el resto del tablero.
- Los tests corren con `uv run --with pytest python -m pytest -q tests/<archivo>` (no hay pytest global). Los tests JS de sintaxis: `bash tests/test_js_parses.sh`.
- Cada tarea termina con commit. No se commitea `~/`, `.playwright-mcp/` ni los PNG sueltos del working tree.
- El chat sigue funcionando con `/operator/chat` (sin stream) hasta que Task 6 lo migra; ningún commit deja el chat roto.

---

## Estructura de archivos

| Archivo | Responsabilidad |
|---|---|
| `lib/operator_catalog.py` (nuevo) | `ToolSpec`, `CATALOG` (todas las acciones), `anthropic_tools()`, `openai_tools()`, `DESTRUCTIVE`, `READONLY`. |
| `lib/operator_dispatch.py` (nuevo) | `Dispatcher`: ejecuta un `ToolSpec` según su target. Sin conocimiento del LLM. |
| `lib/operator_stream.py` (nuevo) | Cliente SSE hacia Anthropic/OpenAI: genera eventos `delta`, `tool_call`, `done`. Caché de credenciales. |
| `lib/operator_tools.py` (modificar) | Pasa a ser un adaptador fino: `dispatch_tool` delega en `operator_dispatch`; `TOOLS` se genera desde el catálogo. |
| `bin/cc-dash` (modificar) | Nuevo `POST /operator/chat/stream`, `operator_agent_turn` usa `Dispatcher` y el cliente stream. Nuevo `POST /app/command`. |
| `bin/cc-app` (modificar) | Observador de `app-command.json` → handlers GTK existentes. |
| `dash/index.html` (modificar) | `opSend` en streaming, render incremental, `opApplyActions` genérico, funciones globales por control. |
| `dash/term.html` (modificar) | Acepta `postMessage {type:"toolbar"}` del padre. |
| `tests/test_operator_catalog.py` (nuevo) | Cobertura del catálogo contra el código fuente. |
| `tests/test_operator_dispatch.py` (nuevo) | Despacho por target con servidores falsos. |
| `tests/test_operator_stream.py` (nuevo) | Parser SSE y caché de credenciales. |
| `tests/test_operator_agent_loop.py` (nuevo) | Loop de agente con proveedor falso, formato SSE, paralelismo. |
| `tests/test_operator_chat.py`, `tests/test_operator_tools.py`, `tests/test_remote_ui.py`, `tests/test_desktop_tabs.py` (modificar) | Adaptar a la nueva fuente de tools; asserts de UI y de cc-app. |

---

### Task 1: Catálogo declarativo (`ToolSpec` + generación de esquemas)

**Files:**
- Create: `lib/operator_catalog.py`
- Test: `tests/test_operator_catalog.py`

**Interfaces:**
- Produces:
  ```python
  @dataclass(frozen=True)
  class ToolSpec:
      name: str            # snake_case único
      group: str           # "sessions" | "panes" | "models" | "usage" | "pomodoro" | "prefs" | "notifs" | "remote" | "snippets" | "fs" | "news" | "nav" | "term" | "app" | "chat"
      description: str
      params: dict         # JSON schema "properties"
      required: tuple[str, ...]
      target: dict         # {"kind":"api","method","path","body","query"} | {"kind":"ui","op":"click|call|term",...} | {"kind":"app","command"} | {"kind":"local","intent"}
      destructive: bool = False
      readonly: bool = False
  CATALOG: list[ToolSpec]
  def by_name(name: str) -> ToolSpec | None
  def anthropic_tools() -> list[dict]
  def openai_tools() -> list[dict]
  def groups_summary() -> str
  DESTRUCTIVE: frozenset[str]; READONLY: frozenset[str]
  ```
- Convención de `body`/`query` en targets `api`: los valores `"$param"` se sustituyen por el argumento del mismo nombre; los literales se envían tal cual; las claves cuyo `$param` no vino se omiten.

- [ ] **Step 1: Escribir el test de estructura**

```python
# tests/test_operator_catalog.py
import json
import re
from pathlib import Path

import pytest

from lib import operator_catalog as cat

ROOT = Path(__file__).resolve().parents[1]
DASH = (ROOT / "bin" / "cc-dash").read_text()
HTML = (ROOT / "dash" / "index.html").read_text()
TERM = (ROOT / "dash" / "term.html").read_text()
APP = (ROOT / "bin" / "cc-app").read_text()


def test_catalog_names_are_unique_snake_case():
    names = [t.name for t in cat.CATALOG]
    assert len(names) == len(set(names))
    for n in names:
        assert re.fullmatch(r"[a-z][a-z0-9_]{2,48}", n), n


def test_every_tool_has_group_description_and_valid_target():
    kinds = {"api", "ui", "app", "local"}
    for t in cat.CATALOG:
        assert t.group in cat.GROUPS, t.name
        assert len(t.description) >= 12, t.name
        assert t.target["kind"] in kinds, t.name
        for r in t.required:
            assert r in t.params, (t.name, r)
        if t.destructive:
            assert "confirm" in t.params and "confirm" in t.required, t.name


def test_api_targets_point_to_real_cc_dash_paths():
    for t in cat.CATALOG:
        if t.target["kind"] != "api":
            continue
        path = t.target["path"]
        assert f'"{path}"' in DASH, (t.name, path)


@pytest.mark.xfail(strict=True, reason="Task 7 crea las globales/selectores")
def test_ui_targets_point_to_real_selectors_or_functions():
    for t in cat.CATALOG:
        if t.target["kind"] != "ui":
            continue
        op = t.target["op"]
        if op == "click":
            sel = t.target["selector"]
            key = sel.lstrip("#.").split("[")[0]
            assert key in HTML, (t.name, sel)
        elif op == "call":
            fn = t.target["fn"]
            assert f"function {fn}(" in HTML or f"window.{fn} = " in HTML, (t.name, fn)
        elif op == "term":
            assert t.target["type"] in ("toolbar", "paste", "mode", "ctrl"), t.name
        else:
            raise AssertionError((t.name, op))


@pytest.mark.xfail(strict=True, reason="Task 9 crea APP_COMMANDS en cc-app")
def test_app_targets_are_handled_by_cc_app():
    for t in cat.CATALOG:
        if t.target["kind"] != "app":
            continue
        assert f'"{t.target["command"]}":' in APP, (t.name, t.target["command"])


def test_schemas_generate_for_both_providers():
    a = cat.anthropic_tools()
    o = cat.openai_tools()
    assert len(a) == len(o) == len(cat.CATALOG)
    json.dumps(a)
    json.dumps(o)
    assert all(x["input_schema"]["type"] == "object" for x in a)
    assert all(x["function"]["parameters"]["type"] == "object" for x in o)


def test_minimum_coverage_per_group():
    counts = {}
    for t in cat.CATALOG:
        counts[t.group] = counts.get(t.group, 0) + 1
    floor = {"sessions": 20, "panes": 10, "models": 18, "usage": 16, "pomodoro": 7,
             "prefs": 18, "notifs": 6, "remote": 12, "snippets": 5, "fs": 4,
             "news": 4, "nav": 14, "term": 5, "app": 26, "chat": 6}
    for g, n in floor.items():
        assert counts.get(g, 0) >= n, (g, counts.get(g, 0))
```

Si `from lib import operator_catalog` no resuelve, copia el encabezado de `tests/test_operator_tools.py` (cómo inserta `ROOT` o `ROOT/lib` en `sys.path`) y usa el mismo import que ese archivo.

- [ ] **Step 2: Correrlo y ver que falla**

Run: `uv run --with pytest python -m pytest -q tests/test_operator_catalog.py`
Expected: FAIL con `ModuleNotFoundError` / `ImportError` de `operator_catalog`.

- [ ] **Step 3: Escribir el catálogo**

`lib/operator_catalog.py` completo. `T(...)` construye un `ToolSpec`; `P(...)` construye propiedades JSON schema: un `str` es descripción de string; `(tipo, desc)`; `(tipo, desc, enum)`; o un dict literal.

(El código está en la Parte 2 de este documento, sección "Código de `lib/operator_catalog.py`".)

- [ ] **Step 4: Correr el test**

Run: `uv run --with pytest python -m pytest -q tests/test_operator_catalog.py`
Expected: 5 passed, 2 xfailed (los de `ui` y `app`, que se destapan en Tasks 7 y 9).

- [ ] **Step 5: Commit**

```bash
git add lib/operator_catalog.py tests/test_operator_catalog.py docs/superpowers/specs/2026-09-07-operator-full-catalog-design.md docs/superpowers/plans/2026-09-07-operator-full-catalog.md
git commit -m "feat(operator): catálogo declarativo de TODAS las acciones de la UX (ToolSpec + esquemas)"
```

---

## Parte 2 — Código de `lib/operator_catalog.py` (Task 1, Step 3)

```python
# lib/operator_catalog.py
"""Catálogo ÚNICO de acciones de ComandOS accesibles desde el operador.

Cada control de la UX (tablero remoto, terminal web, app GTK) tiene aquí un
ToolSpec. Los esquemas para Anthropic/OpenAI se generan de esta lista. Un
test cruza cada target con el código fuente para que no haya deriva.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

GROUPS = ("sessions", "panes", "models", "usage", "pomodoro", "prefs", "notifs",
          "remote", "snippets", "fs", "news", "nav", "term", "app", "chat")


@dataclass(frozen=True)
class ToolSpec:
    name: str
    group: str
    description: str
    params: dict = field(default_factory=dict)
    required: tuple[str, ...] = ()
    target: dict = field(default_factory=dict)
    destructive: bool = False
    readonly: bool = False


def P(**kw: Any) -> dict:
    out = {}
    for k, v in kw.items():
        if isinstance(v, str):
            out[k] = {"type": "string", "description": v}
        elif isinstance(v, tuple) and len(v) == 2:
            out[k] = {"type": v[0], "description": v[1]}
        elif isinstance(v, tuple) and len(v) == 3:
            out[k] = {"type": v[0], "description": v[1], "enum": list(v[2])}
        else:
            out[k] = dict(v)
    return out


CONFIRM = {"confirm": {"type": "boolean",
                       "description": "Debe ser true; pide confirmación al usuario en el chat antes de llamar."}}
TAB = "Nombre o etiqueta de la pestaña/sesión (se resuelve difuso)."
PANE = "Id de pane tmux (%N). Opcional: por defecto el pane vivo de la sesión."
ARR = lambda desc: {"type": "array", "items": {"type": "string"}, "description": desc}


def api(method, path, body=None, query=None):
    return {"kind": "api", "method": method, "path": path, "body": body or {}, "query": query or {}}


def ui_click(selector):
    return {"kind": "ui", "op": "click", "selector": selector}


def ui_call(fn, *args):
    return {"kind": "ui", "op": "call", "fn": fn, "args": list(args)}


def ui_term(type_):
    return {"kind": "ui", "op": "term", "type": type_}


def app(command):
    return {"kind": "app", "command": command}


def local(intent):
    return {"kind": "local", "intent": intent}


def T(name, group, description, params=None, required=(), target=None, destructive=False, readonly=False):
    params = dict(params or {})
    required = tuple(required)
    if destructive:
        params.update(CONFIRM)
        required = tuple(dict.fromkeys(required + ("confirm",)))
    return ToolSpec(name, group, description, params, required, target or {}, destructive, readonly)


EFFORT = ("string", "Esfuerzo", ("low", "medium", "high", "xhigh", "max"))

CATALOG: list[ToolSpec] = [
    # ───────────── sessions / tabs ─────────────
    T("list_tabs", "sessions", "Lista las pestañas abiertas (mismas que el escritorio).", target=api("GET", "/tabs"), readonly=True),
    T("list_sessions", "sessions", "Estado de todas las sesiones/agentes: status, proyecto, último mensaje.", target=api("GET", "/state"), readonly=True),
    T("get_active_tab", "sessions", "Qué pestaña está activa en la app y su pane vivo.", target=api("GET", "/active-tab"), readonly=True),
    T("session_brain", "sessions", "MCPs, skills, cuentas y CLAUDE.md de un proyecto.", P(cwd="Carpeta absoluta del proyecto", pane=PANE, harness="claude|codex|grok|opencode"), ("cwd",), api("GET", "/session-brain", query={"cwd": "$cwd", "pane": "$pane", "harness": "$harness"}), readonly=True),
    T("events_log", "sessions", "Últimos 80 eventos de actividad (working/waiting/done).", target=api("GET", "/events"), readonly=True),
    T("dedication_stats", "sessions", "Tiempo dedicado por proyecto (hoy y semana).", target=api("GET", "/dedication"), readonly=True),
    T("tab_history", "sessions", "Pestañas cerradas recientemente (recuperables).", target=api("GET", "/tab-history"), readonly=True),
    T("focus_tab", "sessions", "Trae al frente una pestaña/sesión.", P(tab=TAB), ("tab",), local("focus")),
    T("next_tab", "sessions", "Pasa a la pestaña siguiente.", target=local("step_next")),
    T("prev_tab", "sessions", "Pasa a la pestaña anterior.", target=local("step_prev")),
    T("close_tab", "sessions", "Cierra la pestaña (la sesión tmux sigue viva).", P(tab=TAB), ("tab",), local("close")),
    T("rename_tab", "sessions", "Renombra una pestaña.", P(tab=TAB, name="Nuevo nombre"), ("tab", "name"), local("rename")),
    T("send_tab_back", "sessions", "Manda la pestaña al final de la barra.", P(tab=TAB), ("tab",), local("send_back")),
    T("new_terminal_tab", "sessions", "Nueva terminal (shell) como pestaña en ambos lados.", P(label="Etiqueta opcional"), (), api("POST", "/tab-new", {"label": "$label"})),
    T("recover_tab", "sessions", "Recupera una pestaña cerrada por nombre.", P(name="Nombre de la pestaña cerrada"), ("name",), local("recover")),
    T("new_ai_session", "sessions", "Crea una sesión de IA nueva en una carpeta (wizard +): harness, motor, modelo, esfuerzo, cuenta, modo sin aprobaciones.", P(cwd="Carpeta absoluta", agent=("string", "Harness", ("claude", "codex", "grok", "opencode", "agy", "acp")), model="Modelo", effort=EFFORT, routeId="Ruta de la matriz de capacidades", harnessAccount="Alias de cuenta del harness", motorAccount="Alias de cuenta del motor", danger=("boolean", "Sin aprobaciones (dangerously)")), ("cwd",), api("POST", "/session-new", {"cwd": "$cwd", "agent": "$agent", "model": "$model", "effort": "$effort", "routeId": "$routeId", "harnessAccount": "$harnessAccount", "motorAccount": "$motorAccount", "danger": "$danger"})),
    T("open_project", "sessions", "Abre/crea la sesión de un proyecto por nombre (busca en ~/codebase).", P(session="Nombre del proyecto", agent="Harness opcional"), ("session",), api("POST", "/new", {"session": "$session", "agent": "$agent"})),
    T("open_with_account", "sessions", "Nueva sesión Claude en una carpeta con una cuenta concreta.", P(cwd="Carpeta", account="Alias de cuenta", danger=("boolean", "Sin aprobaciones")), ("cwd", "account"), api("POST", "/open-with-account", {"cwd": "$cwd", "account": "$account", "danger": "$danger"})),
    T("session_open", "sessions", "Revive si hace falta y enfoca la ventana de una sesión.", P(session="Sesión tmux", cwd="Carpeta", agent="Harness"), ("session",), api("POST", "/up", {"session": "$session", "cwd": "$cwd", "agent": "$agent"})),
    T("session_ensure", "sessions", "Asegura que la sesión y su ventana existen, sin robar el foco.", P(session="Sesión", cwd="Carpeta", win=("string", "Ventana", ("claude", "shell")), agent="Harness"), ("session",), api("POST", "/ensure", {"session": "$session", "cwd": "$cwd", "win": "$win", "agent": "$agent"})),
    T("session_shell", "sessions", "Abre/enfoca la ventana shell de la sesión.", P(session="Sesión", cwd="Carpeta"), ("session",), api("POST", "/shell", {"session": "$session", "cwd": "$cwd"})),
    T("toggle_shell", "sessions", "Alterna entre la ventana de la IA y el shell (Ctrl-b l).", target=local("toggle_shell")),
    T("kill_session", "sessions", "MATA la sesión tmux (irreversible).", P(tab=TAB), ("tab",), local("kill"), destructive=True),
    T("export_reply", "sessions", "Exporta la última respuesta de la IA a txt o pdf y la abre.", P(session="Sesión", format=("string", "Formato", ("txt", "pdf"))), ("session", "format"), api("POST", "/export", {"session": "$session", "format": "$format"})),
    T("favorite_toggle", "sessions", "Marca/desmarca una sesión como favorita (★).", P(session="Sesión", on=("boolean", "true=favorita")), ("session", "on"), ui_call("opFavorite", "$session", "$on")),
    # ───────────── panes / input ─────────────
    T("send_text", "panes", "Escribe texto en el pane y pulsa Enter (responder a la IA).", P(tab=TAB, text="Texto a enviar", pane=PANE), ("tab", "text"), local("send")),
    T("paste_text", "panes", "Pega texto (bracketed paste) SIN Enter.", P(session="Sesión", text="Texto", pane=PANE), ("session", "text"), api("POST", "/paste", {"session": "$session", "text": "$text", "pane": "$pane"})),
    T("send_key", "panes", "Envía una tecla: Enter, Escape, Up, Down, Tab, y, n, 1-9.", P(key="Tecla", tab=TAB, pane=PANE), ("key",), local("key")),
    T("choose_option", "panes", "Elige la opción N de un menú numerado de la IA (manda el número y Enter).", P(tab=TAB, option=("integer", "1-9")), ("tab", "option"), local("choose_option")),
    T("interrupt_turn", "panes", "Para el turno actual de la IA (cancela cambio en cola y manda Escape).", P(tab=TAB, pane=PANE), ("tab",), local("interrupt")),
    T("pause_agent", "panes", "Pausa (SIGSTOP) el proceso de la IA.", P(tab=TAB), ("tab",), local("pause_on")),
    T("resume_agent", "panes", "Reanuda (SIGCONT) el proceso de la IA.", P(tab=TAB), ("tab",), local("pause_off")),
    T("split_pane", "panes", "Divide el pane: derecha, izquierda, abajo o arriba.", P(side=("string", "Lado", ("derecha", "izquierda", "abajo", "arriba")), tab=TAB), ("side",), local("split")),
    T("close_split", "panes", "Cierra el split activo (kill-pane).", P(tab=TAB), (), local("close_split"), destructive=True),
    T("tmux_mouse_get", "panes", "Lee si el modo ratón de tmux está activo en la sesión.", P(session="Sesión"), ("session",), api("GET", "/tmux-mouse", query={"session": "$session"}), readonly=True),
    T("tmux_mouse_set", "panes", "Activa/desactiva el modo ratón (seleccionar texto vs interactuar).", P(session="Sesión", enabled=("boolean", "true=interactuar")), ("session", "enabled"), api("POST", "/tmux-mouse", {"session": "$session", "enabled": "$enabled"})),
    T("tmux_scroll", "panes", "Desplaza el historial del pane N líneas (negativo=arriba).", P(session="Sesión", delta=("integer", "Líneas")), ("session", "delta"), api("POST", "/tmux-scroll", {"session": "$session", "delta": "$delta"})),
    # ───────────── models / harness / accounts ─────────────
    T("proxy_state", "models", "Estado del gateway: motor y modelo global, cuentas, logins.", target=api("GET", "/proxy"), readonly=True),
    T("list_providers", "models", "Registro de proveedores, harnesses, motores y rutas disponibles.", target=api("GET", "/providers"), readonly=True),
    T("list_model_tiers", "models", "Tabla de tiers de modelos (barato/rápido/potente).", target=api("GET", "/model-tiers"), readonly=True),
    T("list_opencode_models", "models", "Catálogo de modelos de OpenCode.", target=api("GET", "/opencode/models"), readonly=True),
    T("sovereignty_report", "models", "Reporte de soberanía (qué corre local vs nube).", target=api("GET", "/sovereignty"), readonly=True),
    T("model_switch", "models", "Cambia modelo/motor/esfuerzo del pane de una sesión en vivo.", P(session="Sesión", pane=PANE, routeId="Ruta de la matriz", provider="claude|codex|grok|opencode", model="Modelo", effort=EFFORT, motor="Motor", harnessAccount="Cuenta harness", motorAccount="Cuenta motor", interrupt=("boolean", "Detener y cambiar ya")), ("session",), api("POST", "/model/switch", {"session": "$session", "pane": "$pane", "routeId": "$routeId", "provider": "$provider", "model": "$model", "effort": "$effort", "motor": "$motor", "harnessAccount": "$harnessAccount", "motorAccount": "$motorAccount", "interrupt": "$interrupt"})),
    T("model_switch_cancel", "models", "Cancela un cambio de modelo en cola.", P(session="Sesión", pane=PANE), ("session",), api("POST", "/model/switch-cancel", {"session": "$session", "pane": "$pane"})),
    T("model_switch_status", "models", "Progreso de un cambio de modelo/motor en curso.", P(operationKey="Clave devuelta por model_switch"), ("operationKey",), api("GET", "/model/status", query={"operationKey": "$operationKey"}), readonly=True),
    T("harness_switch", "models", "Cambia el CLI (Claude Code, Codex, Grok Build, OpenCode, ACP) de un pane con traspaso de contexto.", P(session="Sesión", pane=PANE, toHarness="Harness destino", model="Modelo", effort=EFFORT, account="Cuenta", motor="Motor", danger=("boolean", "Sin aprobaciones"), interrupt=("boolean", "Detener y cambiar ya")), ("session", "toHarness"), api("POST", "/harness/switch", {"session": "$session", "pane": "$pane", "toHarness": "$toHarness", "model": "$model", "effort": "$effort", "account": "$account", "motor": "$motor", "danger": "$danger", "interrupt": "$interrupt"})),
    T("set_global_motor", "models", "Fija motor y modelo GLOBAL del gateway.", P(motor=("string", "Motor", ("claude", "codex", "grok")), model="Modelo"), ("motor",), api("POST", "/proxy", {"motor": "$motor", "model": "$model"})),
    T("gateway_enable", "models", "Enciende/apaga el gateway (proxy) de modelos.", P(enable=("boolean", "true=encender")), ("enable",), api("POST", "/proxy", {"enable": "$enable"})),
    T("account_add", "models", "Añade una cuenta (alias) de un proveedor y abre su login.", P(provider=("string", "Proveedor", ("claude", "codex", "grok")), alias="Alias", cwd="Carpeta"), ("provider", "alias"), api("POST", "/account/add", {"provider": "$provider", "alias": "$alias", "cwd": "$cwd"})),
    T("account_switch", "models", "Mueve la conversación viva a otra cuenta Claude.", P(session="Sesión", pane=PANE, alias="Alias", interrupt=("boolean", "Detener y cambiar ya")), ("session", "alias"), api("POST", "/account/switch", {"session": "$session", "pane": "$pane", "alias": "$alias", "interrupt": "$interrupt"})),
    T("skill_toggle", "models", "Activa/desactiva una skill (aplica al reciclar la sesión).", P(path="Ruta de la skill", on=("boolean", "true=activar")), ("path", "on"), api("POST", "/skill-toggle", {"path": "$path", "on": "$on"})),
    T("mcp_toggle", "models", "Activa/desactiva un MCP del proyecto.", P(cwd="Carpeta", name="Nombre del MCP", on=("boolean", "true=activar")), ("cwd", "name", "on"), api("POST", "/mcp-toggle", {"cwd": "$cwd", "name": "$name", "on": "$on"})),
    T("optimization_plans", "models", "Perfiles de optimización (ahorro, equilibrio, potencia).", target=api("GET", "/optimization/plans"), readonly=True),
    T("optimization_set_default", "models", "Perfil de optimización por defecto.", P(profile="Perfil"), ("profile",), api("POST", "/optimization/default", {"profile": "$profile"})),
    T("optimization_apply", "models", "Aplica un perfil a varias sesiones (un model_switch por sesión).", P(profile="Perfil", sessions=ARR("Sesiones")), ("profile", "sessions"), local("optimization_apply")),
    T("undo_guard_switch", "models", "Deshace el último cambio de modelo hecho por la guardia (vuelve al anterior).", P(session="Sesión", model="Modelo previo"), ("session", "model"), api("POST", "/model/switch", {"session": "$session", "model": "$model"})),
    T("open_motor_picker", "models", "Abre el selector de motor/modelo/cuenta de un pane en el tablero.", P(session="Sesión", pane=PANE), ("session",), ui_call("openMotorFor", "$session", "$pane")),
    T("open_global_motor_picker", "models", "Abre el selector de motor GLOBAL.", target=ui_click("#motor-global")),
    # ───────────── usage / guard / analytics ─────────────
    T("usage_state", "usage", "Uso y cuotas por proveedor/cuenta, alertas, salud de credenciales.", target=api("GET", "/usage/state"), readonly=True),
    T("usage_guard", "usage", "Guardia anti-desborde con pronóstico por proyecto.", target=api("GET", "/usage/guard"), readonly=True),
    T("usage_changes", "usage", "Ledger de cambios de modelo hechos por la guardia.", target=api("GET", "/usage/changes"), readonly=True),
    T("usage_provider_compare", "usage", "Comparativa de costo/uso entre proveedores.", P(days=("integer", "Ventana en días (14)")), (), api("GET", "/usage/provider-compare", query={"days": "$days"}), readonly=True),
    T("usage_analytics", "usage", "Analytics de experimentos A/B por tipo de tarea.", P(days=("integer", "Días"), taskType="Tipo de tarea"), (), api("GET", "/usage/analytics", query={"days": "$days", "taskType": "$taskType"}), readonly=True),
    T("usage_interactions", "usage", "Últimas interacciones registradas.", P(limit=("integer", "Máximo (20)")), (), api("GET", "/usage/interactions", query={"limit": "$limit"}), readonly=True),
    T("usage_experiments", "usage", "Lista de experimentos A/B.", target=api("GET", "/usage/experiments"), readonly=True),
    T("usage_experiment_create", "usage", "Crea un experimento A/B.", P(label="Nombre", taskType="Tipo de tarea", variants=ARR("Variantes"), minPairs=("integer", "Pares mínimos")), ("label", "taskType", "variants"), api("POST", "/usage/experiment", {"action": "create", "label": "$label", "taskType": "$taskType", "variants": "$variants", "minPairs": "$minPairs"})),
    T("usage_experiment_pair", "usage", "Registra un par de comparación en un experimento.", P(experimentId="Id", projectId="Proyecto"), ("experimentId",), api("POST", "/usage/experiment", {"action": "pair", "experimentId": "$experimentId", "projectId": "$projectId"})),
    T("usage_rate", "usage", "Califica una interacción: Mal, Parcial o Resuelto.", P(interactionId="Id", outcome=("string", "Resultado", ("bad", "partial", "solved")), rating=("integer", "1-5"), note="Nota", taskType="Tipo"), ("interactionId", "outcome"), api("POST", "/usage/rating", {"interactionId": "$interactionId", "outcome": "$outcome", "rating": "$rating", "note": "$note", "taskType": "$taskType"})),
    T("usage_refresh", "usage", "Vuelve a bajar uso y costos de todos los proveedores.", target=api("POST", "/usage/refresh")),
    T("usage_set_quota", "usage", "Declara la cuota de 7 días de un proveedor (0 la borra).", P(provider="Proveedor", tokens7d=("integer", "Tokens/7d")), ("provider", "tokens7d"), api("POST", "/usage/quota", {"provider": "$provider", "tokens7d": "$tokens7d"})),
    T("usage_set_subscription", "usage", "Declara el costo mensual/moneda de un proveedor para el ROI.", P(provider="Proveedor", monthly=("number", "Costo mensual"), currency="Moneda", display="Moneda de display"), ("provider",), api("POST", "/usage/subscription", {"provider": "$provider", "monthly": "$monthly", "currency": "$currency", "display": "$display"})),
    T("usage_alert_rule_set", "usage", "Crea/actualiza una alerta de presupuesto (proyecto, pane o proveedor).", P(id="Id opcional", scope=("string", "Ámbito", ("project", "pane", "provider")), target="Objetivo", label="Etiqueta", threshold=("number", "Umbral")), ("scope", "target", "threshold"), api("POST", "/usage/alert-rule", {"action": "set", "id": "$id", "scope": "$scope", "target": "$target", "label": "$label", "threshold": "$threshold"})),
    T("usage_alert_rule_delete", "usage", "Borra una alerta de presupuesto.", P(id="Id de la regla"), ("id",), api("POST", "/usage/alert-rule", {"action": "delete", "id": "$id"})),
    T("usage_set_thresholds", "usage", "Umbrales globales de alerta (p. ej. 70,85,95).", P(thresholds={"type": "array", "items": {"type": "integer"}, "description": "Porcentajes"}), ("thresholds",), api("POST", "/usage/settings", {"settings": {"COMANDOS_ALERT_THRESHOLDS": "$thresholds"}})),
    T("usage_settings_set", "usage", "Escribe ajustes de uso arbitrarios.", P(settings={"type": "object", "description": "Ajustes"}), ("settings",), api("POST", "/usage/settings", {"settings": "$settings"})),
    T("ui_log_summary", "usage", "Resumen de telemetría de la UI.", target=api("GET", "/ui-log/summary"), readonly=True),
    # ───────────── pomodoro ─────────────
    T("pomodoro_state", "pomodoro", "Bloque de foco actual, cola y analytics de 7 días.", target=api("GET", "/pomodoro"), readonly=True),
    T("pomodoro_start", "pomodoro", "Inicia un bloque de foco/descanso.", P(mins=("integer", "Minutos"), mode=("string", "Modo", ("focus", "break")), project="Proyecto", session="Sesión", cycleIndex=("integer", "Ciclo"), cycleTotal=("integer", "Total")), ("mins", "mode"), api("POST", "/pomodoro", {"mins": "$mins", "mode": "$mode", "project": "$project", "session": "$session", "cycleIndex": "$cycleIndex", "cycleTotal": "$cycleTotal"})),
    T("pomodoro_stop", "pomodoro", "Termina/salta el bloque actual.", P(status=("string", "Estado", ("done", "skipped", "cancelled"))), ("status",), api("POST", "/pomodoro", {"stop": True, "status": "$status"})),
    T("pomodoro_ack", "pomodoro", "Vacía la cola de notificaciones del pomodoro.", target=api("POST", "/pomodoro", {"ack": True})),
    T("pomodoro_settings", "pomodoro", "Duración, descanso, ciclos y auto-descanso.", P(mins=("integer", "Foco"), breakMins=("integer", "Descanso"), cycles=("integer", "Ciclos"), auto=("boolean", "Auto-descanso")), (), api("POST", "/pomodoro", {"settings": {"mins": "$mins", "breakMins": "$breakMins", "cycles": "$cycles", "auto": "$auto"}})),
    T("pomodoro_extend", "pomodoro", "Añade 5 minutos al bloque en marcha.", target=ui_click("#pp-extend")),
    T("pomodoro_skip", "pomodoro", "Salta el bloque en marcha.", target=ui_click("#pp-skip")),
    T("open_pomodoro", "pomodoro", "Abre el panel de pomodoro.", target=ui_click("#btn-pomo")),
    # ───────────── prefs / settings ─────────────
    T("get_conf", "prefs", "Configuración (volumen, notificaciones, idioma, worktrees…).", target=api("GET", "/conf"), readonly=True),
    T("get_prefs", "prefs", "Preferencias (tema, fuente, cursor, favoritos) y fuentes instaladas.", target=api("GET", "/prefs"), readonly=True),
    T("set_pref", "prefs", "Switch de configuración on/off.", P(key=("string", "Clave", ("AUTO_WORKTREE", "NOTIFY_ON_DONE", "NOTIFY_ON_ATTENTION", "SOUND_ENABLED", "DESKTOP_NOTIFY", "TELEGRAM_ENABLED", "SPEAK_DONE", "SPEAK_ATTENTION")), on=("boolean", "true=on")), ("key", "on"), local("pref")),
    T("set_voice", "prefs", "Voz que anuncia el proyecto (SPEAK_DONE + SPEAK_ATTENTION).", P(on=("boolean", "true=on")), ("on",), local("voice")),
    T("set_volume", "prefs", "Volumen de voz y chime 0-100.", P(percent=("integer", "0-100")), ("percent",), api("POST", "/conf-set", {"key": "VOLUME", "value": "$percent"})),
    T("set_language", "prefs", "Idioma del tablero: auto, es, en.", P(lang=("string", "Idioma", ("auto", "es", "en"))), ("lang",), local("lang")),
    T("set_theme", "prefs", "Tema visual del tablero y terminales.", P(theme="Nombre del tema"), ("theme",), local("theme")),
    T("set_notify_corner", "prefs", "Esquina de los avisos: tl, tr, bl, br o free.", P(corner=("string", "Esquina", ("tl", "tr", "bl", "br", "free"))), ("corner",), local("notify_pos")),
    T("set_terminal_font", "prefs", "Familia y/o tamaño de fuente del terminal.", P(family="Familia instalada", size=("integer", "Tamaño px")), (), api("POST", "/prefs-set", {"font_family": "$family", "font_size": "$size"})),
    T("set_cursor", "prefs", "Forma y parpadeo del cursor.", P(shape=("string", "Forma", ("block", "bar", "underline")), blink=("boolean", "Parpadeo")), (), api("POST", "/prefs-set", {"cursor_shape": "$shape", "cursor_blink": "$blink"})),
    T("set_ligatures", "prefs", "Ligaduras tipográficas en el terminal.", P(on=("boolean", "true=on")), ("on",), api("POST", "/prefs-set", {"ligatures": "$on"})),
    T("set_terminal_padding", "prefs", "Margen interno del terminal.", P(px=("integer", "Píxeles")), ("px",), api("POST", "/prefs-set", {"terminal_padding": "$px"})),
    T("set_terminal_opacity", "prefs", "Opacidad del fondo del terminal 0-100.", P(percent=("integer", "0-100")), ("percent",), api("POST", "/prefs-set", {"terminal_opacity": "$percent"})),
    T("set_poll_seconds", "prefs", "Segundos de refresco del tablero remoto.", P(seconds=("integer", "Segundos")), ("seconds",), ui_call("setPollSeconds", "$seconds")),
    T("set_browser_notifications", "prefs", "Notificaciones del navegador on/off.", P(on=("boolean", "true=on")), ("on",), ui_call("setBrowserNotifications", "$on")),
    T("test_notification", "prefs", "Prueba un aviso: voz, chime o terminado.", P(kind=("string", "Tipo", ("voice", "chime", "done"))), ("kind",), api("POST", "/test", {"kind": "$kind"})),
    T("open_settings", "prefs", "Abre Ajustes en una tab: Apariencia, Notificaciones, Terminal o Tablero.", P(tab=("string", "Tab", ("appearance", "notif", "term", "dash"))), (), local("ui_settings")),
    T("set_limit_style", "prefs", "Estilo de la barra de límites en Analytics.", P(style="Estilo"), ("style",), ui_call("setLimitStyle", "$style")),
    T("notif_dismiss_ids", "prefs", "Marca como leídas notificaciones por id (persistente).", P(ids=ARR("Ids")), ("ids",), api("POST", "/prefs-set", {"nfDismiss": "$ids"})),
    T("notif_snooze_ids", "prefs", "Pospone notificaciones por id hasta una marca de tiempo.", P(snooze={"type": "object", "description": "{id: epoch_ms}"}), ("snooze",), api("POST", "/prefs-set", {"nfSnooze": "$snooze"})),
    # ───────────── notifications ─────────────
    T("notifs_count", "notifs", "Cuántas notificaciones vivas hay.", target=api("GET", "/notifs/count"), readonly=True),
    T("open_notifications", "notifs", "Abre la campana de notificaciones.", target=ui_click("#btn-notif")),
    T("notif_clear_all", "notifs", "Limpia todas las notificaciones.", target=ui_click("#nf-clearall")),
    T("notif_dismiss", "notifs", "Quita una notificación por id.", P(id="Id"), ("id",), ui_call("nfDismiss", "$id")),
    T("notif_pin", "notifs", "Guarda (fija) una notificación.", P(id="Id"), ("id",), ui_call("nfPin", "$id")),
    T("notif_unpin", "notifs", "Quita una notificación guardada.", P(id="Id"), ("id",), ui_call("nfUnpin", "$id")),
    T("notif_snooze_1h", "notifs", "Recordar una notificación en 1 hora.", P(id="Id"), ("id",), ui_call("nfSnooze", "$id")),
    T("show_timeline", "notifs", "Muestra/oculta la actividad reciente.", P(on=("boolean", "true=mostrar")), ("on",), ui_call("setTimeline", "$on")),
    # ───────────── remote / ssh ─────────────
    T("remote_state", "remote", "Estado del acceso remoto (Tailscale) y URLs.", target=api("GET", "/remote-state"), readonly=True),
    T("remote_on", "remote", "Prende el tablero remoto en el tailnet.", target=api("POST", "/remote-on")),
    T("remote_off", "remote", "Apaga el tablero remoto.", target=api("POST", "/remote-off"), destructive=True),
    T("webterm_on", "remote", "Prende el terminal web remoto.", target=api("POST", "/remote-webterm-on")),
    T("webterm_off", "remote", "Apaga el terminal web remoto.", target=api("POST", "/remote-webterm-off"), destructive=True),
    T("open_remote_panel", "remote", "Abre el panel Remoto (QR y URLs).", target=ui_click("#btn-remote")),
    T("ssh_list", "remote", "Servidores de ~/.ssh/config.", target=api("GET", "/ssh"), readonly=True),
    T("ssh_add", "remote", "Añade un servidor SSH.", P(host="Alias", hostname="Host/IP", user="Usuario", port=("integer", "Puerto"), identity="Ruta de llave"), ("host", "hostname"), api("POST", "/ssh-add", {"host": "$host", "hostname": "$hostname", "user": "$user", "port": "$port", "identity": "$identity"})),
    T("ssh_update", "remote", "Edita/renombra un servidor SSH.", P(orig="Alias actual", host="Alias nuevo", hostname="Host/IP", user="Usuario", port=("integer", "Puerto"), identity="Llave"), ("orig",), api("POST", "/ssh-update", {"orig": "$orig", "host": "$host", "hostname": "$hostname", "user": "$user", "port": "$port", "identity": "$identity"})),
    T("ssh_delete", "remote", "Borra un servidor SSH.", P(host="Alias"), ("host",), api("POST", "/ssh-del", {"host": "$host"}), destructive=True),
    T("ssh_connect", "remote", "Conecta a un servidor en la sesión actual.", P(host="Alias"), ("host",), api("POST", "/ssh-connect", {"host": "$host"})),
    T("ssh_new_tab", "remote", "Abre una pestaña nueva conectada a un servidor.", P(host="Alias"), ("host",), api("POST", "/ssh-new-tab", {"host": "$host"})),
    T("ssh_key_setup", "remote", "Instala tu llave en el servidor (acceso sin contraseña).", P(host="Alias"), ("host",), api("POST", "/ssh-key-setup", {"host": "$host"})),
    T("open_servers_panel", "remote", "Abre el gestor de servidores SSH.", target=ui_click("#ssh-manage")),
    # ───────────── snippets ─────────────
    T("snippets_list", "snippets", "Lista los snippets guardados.", target=api("GET", "/snippets"), readonly=True),
    T("snippet_create", "snippets", "Crea un snippet.", P(name="Nombre", body="Contenido", tags=ARR("Etiquetas")), ("name", "body"), api("POST", "/snippets", {"name": "$name", "body": "$body", "tags": "$tags"})),
    T("snippet_update", "snippets", "Edita un snippet.", P(id="Id", name="Nombre", body="Contenido", tags=ARR("Etiquetas")), ("id",), api("POST", "/snippets/update", {"id": "$id", "name": "$name", "body": "$body", "tags": "$tags"})),
    T("snippet_delete", "snippets", "Borra un snippet.", P(id="Id"), ("id",), api("POST", "/snippets/delete", {"id": "$id"}), destructive=True),
    T("snippet_send", "snippets", "Pega un snippet en una sesión (sin ejecutar).", P(id="Id del snippet", session="Sesión"), ("id", "session"), local("snippet_send")),
    T("open_snippets", "snippets", "Abre el panel de snippets.", target=ui_click("#btn-snippets")),
    # ───────────── fs / links ─────────────
    T("fs_list_dirs", "fs", "Lista subcarpetas de una ruta (selector de carpeta).", P(path="Ruta (~ por defecto)"), (), api("GET", "/fs/dirs", query={"path": "$path"}), readonly=True),
    T("fs_mkdir", "fs", "Crea una carpeta.", P(path="Ruta"), ("path",), api("POST", "/fs/mkdir", {"path": "$path"})),
    T("open_path", "fs", "Abre una ruta local con la app por defecto.", P(path="Ruta absoluta o ~"), ("path",), api("POST", "/open-path", {"path": "$path"})),
    T("open_url", "fs", "Abre una URL http(s) en el navegador del sistema.", P(url="URL"), ("url",), api("POST", "/open-url", {"url": "$url"})),
    # ───────────── news / model watch ─────────────
    T("news_latest", "news", "Últimas noticias de IA vigiladas.", target=api("GET", "/news/latest"), readonly=True),
    T("models_latest", "news", "Modelos nuevos detectados por el vigilante.", target=api("GET", "/models/latest"), readonly=True),
    T("news_refresh", "news", "Fuerza un ciclo del vigilante de noticias.", target=api("POST", "/news/refresh")),
    T("models_refresh", "news", "Fuerza un ciclo del vigilante de modelos.", target=api("POST", "/models/refresh")),
    # ───────────── nav (tablero) ─────────────
    T("open_panel", "nav", "Abre un panel del tablero: analytics (con tab), sov, switcher, timeline, wizard, centro.", P(panel=("string", "Panel", ("analytics", "sov", "switcher", "timeline", "wizard", "centro")), tab=("string", "Tab de analytics", ("resumen", "comparar", "optimizar"))), ("panel",), local("ui_panel")),
    T("close_panels", "nav", "Cierra todos los paneles/modales abiertos.", target=ui_call("closeAllPanels")),
    T("show_view", "nav", "Muestra el panel o una terminal: 'panel' o 'term:<sesión>'.", P(view="panel | term:<sesión>"), ("view",), ui_call("showView", "$view")),
    T("switcher_search", "nav", "Abre el conmutador con un texto de búsqueda.", P(query="Texto"), (), ui_call("swOpenWith", "$query")),
    T("wizard_prefill", "nav", "Abre el wizard de nueva sesión precargado.", P(cwd="Carpeta", harness="Harness", motor="Motor", model="Modelo", effort="Esfuerzo", danger=("boolean", "Sin aprobaciones")), (), ui_call("nsOpenPrefilled", "$cwd", "$harness", "$motor", "$model", "$effort", "$danger")),
    T("select_session_card", "nav", "Selecciona la fila de una sesión en el panel (centro de control).", P(session="Sesión"), ("session",), ui_call("selectSessionCard", "$session")),
    T("expand_reply", "nav", "Expande/colapsa la respuesta de una sesión en el panel.", P(session="Sesión", on=("boolean", "true=expandir")), ("session",), ui_call("expandReply", "$session", "$on")),
    T("toggle_ssh_chips", "nav", "Expande/colapsa la fila de servidores.", target=ui_click("#ssh-toggle")),
    T("open_analytics_tab", "nav", "Abre Analytics en una tab concreta.", P(tab=("string", "Tab", ("resumen", "comparar", "optimizar", "proveedores", "alertas", "guardia"))), ("tab",), ui_call("openAnalyticsTab", "$tab")),
    T("compare_set_window", "nav", "Ventana de días del comparador.", P(days=("integer", "Días")), ("days",), ui_call("compareSetDays", "$days")),
    T("set_split_left", "nav", "Ancho del panel izquierdo en layout ancho (px).", P(px=("integer", "Píxeles")), ("px",), ui_call("setSplitLeft", "$px")),
    T("set_chat_height", "nav", "Alto del chat del operador (px).", P(px=("integer", "Píxeles")), ("px",), ui_call("setOpChatHeight", "$px")),
    T("copy_text", "nav", "Copia un texto al portapapeles del navegador.", P(text="Texto"), ("text",), ui_call("copyText", "$text")),
    T("reload_dashboard", "nav", "Recarga el tablero.", target=ui_call("reloadDashboard")),
    T("dashboard_toast", "nav", "Muestra un aviso breve en el tablero.", P(text="Texto", error=("boolean", "Estilo error")), ("text",), ui_call("toast", "$text", "$error")),
    T("close_tab_modal", "nav", "Abre el modal de cerrar pestaña de una terminal remota.", P(session="Sesión"), ("session",), ui_call("askCloseTab", "$session")),
    # ───────────── term (toolbar del terminal web) ─────────────
    T("term_key", "term", "Pulsa una tecla de la toolbar del terminal web: escape, enter, tab, left, up, down, right.", P(key=("string", "Tecla", ("escape", "enter", "tab", "left", "up", "down", "right")), session="Sesión (terminal visible por defecto)"), ("key",), ui_term("toolbar")),
    T("term_paste", "term", "Pega texto en el terminal web visible.", P(text="Texto", session="Sesión"), ("text",), ui_term("paste")),
    T("term_selection_mode", "term", "Modo seleccionar texto (true) o interactuar (false) en el terminal web.", P(selecting=("boolean", "true=seleccionar"), session="Sesión"), ("selecting",), ui_term("mode")),
    T("term_ctrl_arm", "term", "Arma Ctrl para la siguiente tecla en el terminal web.", P(session="Sesión"), (), ui_term("ctrl")),
    T("term_focus", "term", "Da foco al terminal web visible (abre teclado en móvil).", P(session="Sesión"), (), ui_call("focusVisibleTerm")),
    # ───────────── app (escritorio GTK) ─────────────
    T("app_split", "app", "Split del pane actual en la app: right, left, down, up.", P(side=("string", "Lado", ("right", "left", "down", "up")), session="Sesión"), ("side",), app("split")),
    T("app_split_ssh", "app", "Split con otra conexión SSH al mismo servidor.", P(side=("string", "Lado", ("right", "left", "down", "up")), session="Sesión"), ("side",), app("split_ssh")),
    T("app_kill_pane", "app", "Cierra ESTE split en la app.", P(session="Sesión"), (), app("kill_pane"), destructive=True),
    T("app_toggle_window", "app", "Alterna ventana 1/2 (Ctrl-b l) en la app.", P(session="Sesión"), (), app("toggle_window")),
    T("app_select_pane", "app", "Selecciona un pane por id en la app.", P(pane="Id %N"), ("pane",), app("select_pane")),
    T("app_next_tab", "app", "Pestaña siguiente (Ctrl+PgDn).", target=app("next_tab")),
    T("app_prev_tab", "app", "Pestaña anterior (Ctrl+PgUp).", target=app("prev_tab")),
    T("app_mru_toggle", "app", "Alterna con la última pestaña usada (Ctrl+Tab).", target=app("mru_toggle")),
    T("app_focus_page", "app", "Enfoca una pestaña GTK ya abierta sin re-attachear.", P(session="Sesión"), ("session",), app("focus_page")),
    T("app_tab_reorder", "app", "Mueve una pestaña a una posición (0 = primera).", P(session="Sesión", index=("integer", "Posición")), ("session", "index"), app("tab_reorder")),
    T("app_mosaic", "app", "Mosaico de todas las sesiones: on, off o toggle (Ctrl+G).", P(state=("string", "Estado", ("on", "off", "toggle"))), ("state",), app("mosaic")),
    T("app_mosaic_zoom", "app", "En mosaico, enfoca en grande una sesión.", P(session="Sesión"), ("session",), app("mosaic_zoom")),
    T("app_side_panel", "app", "Muestra/oculta el tablero lateral (en mosaico).", P(on=("boolean", "true=mostrar")), ("on",), app("side_panel")),
    T("app_terminals_visible", "app", "Muestra/oculta las terminales (F12).", P(on=("boolean", "true=mostrar")), ("on",), app("terminals_visible")),
    T("app_reload_dashboard", "app", "Recarga el tablero embebido (F5).", target=app("reload_dashboard")),
    T("app_font_scale", "app", "Zoom de fuente del terminal: +0.1, -0.1 o reset (0).", P(delta=("number", "Delta; 0 = reset")), ("delta",), app("font_scale")),
    T("app_open_switcher", "app", "Abre el conmutador nativo (Ctrl+K).", target=app("open_switcher")),
    T("app_tabs_overview", "app", "Abre la vista de todas las pestañas.", target=app("tabs_overview")),
    T("app_help", "app", "Abre la ayuda de atajos (F1).", target=app("help")),
    T("app_snippets", "app", "Abre el panel nativo de snippets (Ctrl+Shift+K).", target=app("snippets")),
    T("app_new_local_tab", "app", "Nueva terminal VTE aquí (Ctrl+T).", target=app("new_local_tab")),
    T("app_open_xterm_tab", "app", "Abre una sesión en el terminal xterm.js (Ctrl+Shift+T).", P(session="Sesión (local por defecto)"), (), app("open_xterm_tab")),
    T("app_open_wizard", "app", "Abre el wizard de nueva sesión desde la app (+).", target=app("open_wizard")),
    T("app_start_ai_here", "app", "Inicia IA en el pane actual (Ctrl+Shift+A).", P(session="Sesión", pane=PANE), (), app("start_ai_here")),
    T("app_copy_selection", "app", "Copia la selección del terminal al portapapeles del sistema.", target=app("copy_selection")),
    T("app_paste_clipboard", "app", "Pega el portapapeles del sistema en el terminal (Ctrl+V).", target=app("paste_clipboard")),
    T("app_copy_reply", "app", "Copia la última respuesta de la IA de la pestaña actual.", target=app("copy_reply")),
    T("app_window", "app", "Ventana de la app: minimize, maximize, restore, raise.", P(action=("string", "Acción", ("minimize", "maximize", "restore", "raise"))), ("action",), app("window")),
    T("app_side_dashboard_width", "app", "Posición del divisor tablero/terminales (px).", P(px=("integer", "Píxeles")), ("px",), app("paned_position")),
    T("app_quit", "app", "Cierra la app de escritorio (Ctrl+Q).", target=app("quit"), destructive=True),
    # ───────────── chat / operador ─────────────
    T("remember", "chat", "Guarda un hecho en la memoria del operador.", P(fact="Hecho"), ("fact",), local("remember")),
    T("memory_read", "chat", "Lee la memoria completa del operador.", target=local("memory_read"), readonly=True),
    T("memory_replace", "chat", "Reescribe la memoria completa (para borrar o corregir).", P(text="Texto completo"), ("text",), local("memory_replace"), destructive=True),
    T("new_conversation", "chat", "Empieza una conversación nueva del operador.", target=ui_click("#op-new")),
    T("set_chat_model", "chat", "Modelo del chat: haiku, gpt-5.3-codex-spark, grok-4.5.", P(model=("string", "Modelo", ("haiku", "gpt-5.3-codex-spark", "grok-4.5"))), ("model",), api("POST", "/operator/model", {"model": "$model"})),
    T("copy_last_reply", "chat", "Copia la última respuesta del chat.", target=local("copy_reply")),
    T("copy_session_reply", "chat", "Copia la última respuesta de la IA de una sesión.", P(tab=TAB), ("tab",), local("copy_session")),
]

DESTRUCTIVE = frozenset(t.name for t in CATALOG if t.destructive)
READONLY = frozenset(t.name for t in CATALOG if t.readonly)
APP_TOOL_BY_COMMAND = {t.target["command"]: t.name for t in CATALOG if t.target["kind"] == "app"}
APP_COMMAND_NAMES = frozenset(APP_TOOL_BY_COMMAND)
_BY_NAME = {t.name: t for t in CATALOG}


def by_name(name):
    return _BY_NAME.get(str(name or ""))


def _schema(t):
    return {"type": "object", "properties": t.params, "required": list(t.required)}


def anthropic_tools():
    return [{"name": t.name, "description": t.description, "input_schema": _schema(t)} for t in CATALOG]


def openai_tools():
    return [{"type": "function", "function": {"name": t.name, "description": t.description, "parameters": _schema(t)}} for t in CATALOG]


def groups_summary():
    groups = {}
    for t in CATALOG:
        groups.setdefault(t.group, []).append(t.name)
    return "\n".join(f"- {g}: " + ", ".join(names) for g, names in groups.items())
```

---

### Task 2: Despachador genérico por target (`api`, `ui`, `app`, `local`)

**Files:**
- Create: `lib/operator_dispatch.py`
- Test: `tests/test_operator_dispatch.py`

**Interfaces:**
- Consumes: `operator_catalog.by_name`, `ToolSpec`.
- Produces:
  ```python
  LOCAL_INTENTS: tuple[str, ...]
  def substitute(template, args) -> Any        # "$x" -> args["x"]; claves sin valor se omiten
  class Dispatcher:
      def __init__(self, *, base_url, token, hooks_dir, local_handlers, http_post=None, http_get=None)
      def run(self, name, args) -> {"ok": bool, "reply": str, "actions": list[dict], "data": Any}
  ```
- Acciones para el navegador (las consume Task 7): `{"type":"ui","op":"click","selector"}`, `{"type":"ui","op":"call","fn","args"}`, `{"type":"ui","op":"term","term":{...}}`.
- Comando para cc-app (lo consume Task 9): `~/.claude/hooks/app-command.json` = `{"command","args","ts"}`.

- [ ] **Step 1: Test**

```python
# tests/test_operator_dispatch.py
import json

from lib import operator_catalog as cat
from lib import operator_dispatch as od


def test_substitute_replaces_and_drops_missing():
    body = {"session": "$session", "pane": "$pane", "literal": True, "nested": {"a": "$a"}}
    assert od.substitute(body, {"session": "s1", "a": 3}) == {"session": "s1", "literal": True, "nested": {"a": 3}}


def _dispatcher(tmp_path, posts, gets, local=None):
    def post(path, body):
        posts.append((path, body)); return {"ok": True, "echo": body}
    def get(path, query):
        gets.append((path, query)); return [{"session": "a", "label": "A"}]
    return od.Dispatcher(base_url="http://127.0.0.1:1", token="t", hooks_dir=str(tmp_path),
                         local_handlers=local or {}, http_post=post, http_get=get)


def test_api_post_target_builds_body_from_args(tmp_path):
    posts, gets = [], []
    out = _dispatcher(tmp_path, posts, gets).run("paste_text", {"session": "s1", "text": "hola"})
    assert out["ok"] and posts == [("/paste", {"session": "s1", "text": "hola"})]


def test_api_get_target_builds_query(tmp_path):
    posts, gets = [], []
    out = _dispatcher(tmp_path, posts, gets).run("tmux_mouse_get", {"session": "s1"})
    assert gets == [("/tmux-mouse", {"session": "s1"})] and out["data"]


def test_destructive_requires_confirm(tmp_path):
    posts, gets = [], []
    d = _dispatcher(tmp_path, posts, gets)
    out = d.run("ssh_delete", {"host": "x"})
    assert out["ok"] is False and "confirm" in out["reply"] and posts == []
    assert d.run("ssh_delete", {"host": "x", "confirm": True})["ok"] and posts == [("/ssh-del", {"host": "x"})]


def test_ui_targets_return_browser_actions(tmp_path):
    d = _dispatcher(tmp_path, [], [])
    assert d.run("open_pomodoro", {})["actions"] == [{"type": "ui", "op": "click", "selector": "#btn-pomo"}]
    assert d.run("show_view", {"view": "term:x"})["actions"] == [{"type": "ui", "op": "call", "fn": "showView", "args": ["term:x"]}]
    assert d.run("term_key", {"key": "enter", "session": "x"})["actions"] == [{"type": "ui", "op": "term", "term": {"type": "toolbar", "key": "enter", "session": "x"}}]


def test_app_target_writes_command_file(tmp_path):
    out = _dispatcher(tmp_path, [], []).run("app_mosaic", {"state": "on"})
    data = json.loads((tmp_path / "app-command.json").read_text())
    assert out["ok"] and data["command"] == "mosaic" and data["args"] == {"state": "on"} and data["ts"] > 0


def test_local_target_calls_handler(tmp_path):
    seen = []
    d = _dispatcher(tmp_path, [], [], local={"focus": lambda a: (seen.append(a) or {"ok": True, "reply": "Foco en " + a["tab"]})})
    assert d.run("focus_tab", {"tab": "Signara"})["reply"] == "Foco en Signara" and seen == [{"tab": "Signara"}]


def test_unknown_tool_and_missing_required(tmp_path):
    d = _dispatcher(tmp_path, [], [])
    assert d.run("nope", {})["ok"] is False
    out = d.run("paste_text", {"session": "s"})
    assert out["ok"] is False and "text" in out["reply"]


def test_every_local_intent_in_catalog_is_declared():
    intents = {t.target["intent"] for t in cat.CATALOG if t.target["kind"] == "local"}
    assert intents <= set(od.LOCAL_INTENTS), intents - set(od.LOCAL_INTENTS)
```

- [ ] **Step 2: Correr y ver fallar** — `uv run --with pytest python -m pytest -q tests/test_operator_dispatch.py` → `ModuleNotFoundError`.

- [ ] **Step 3: Implementar**

```python
# lib/operator_dispatch.py
"""Ejecuta un ToolSpec del catálogo según su target. No sabe de LLMs."""
from __future__ import annotations

import json
import os
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Callable

from . import operator_catalog as cat

LOCAL_INTENTS = (
    "focus", "step_next", "step_prev", "close", "rename", "send_back", "recover",
    "toggle_shell", "kill", "send", "key", "choose_option", "interrupt", "pause_on",
    "pause_off", "split", "close_split", "optimization_apply", "pref", "voice", "lang",
    "theme", "notify_pos", "ui_settings", "ui_panel", "snippet_send", "remember",
    "memory_read", "memory_replace", "copy_reply", "copy_session",
)

_MISSING = object()


def substitute(template: Any, args: dict) -> Any:
    if isinstance(template, str) and template.startswith("$"):
        return args.get(template[1:], _MISSING)
    if isinstance(template, dict):
        out = {}
        for k, v in template.items():
            val = substitute(v, args)
            if val is not _MISSING and val is not None:
                out[k] = val
        return out
    if isinstance(template, list):
        return [x for x in (substitute(v, args) for v in template) if x is not _MISSING]
    return template


def _summarize(data: Any, limit: int = 1800) -> str:
    try:
        text = json.dumps(data, ensure_ascii=False)
    except Exception:
        text = str(data)
    return text if len(text) <= limit else text[:limit] + "…"


def _fail(msg: str) -> dict:
    return {"ok": False, "reply": msg, "actions": [], "data": None}


class Dispatcher:
    def __init__(self, *, base_url: str, token: str, hooks_dir: str,
                 local_handlers: dict[str, Callable[[dict], dict]],
                 http_post=None, http_get=None):
        self.base_url = base_url.rstrip("/")
        self.token = token
        self.hooks_dir = hooks_dir
        self.local_handlers = local_handlers
        self._post = http_post or self._default_post
        self._get = http_get or self._default_get

    def _default_post(self, path, body):
        req = urllib.request.Request(self.base_url + path, data=json.dumps(body).encode(), method="POST",
                                     headers={"Content-Type": "application/json", "X-Comandos-Token": self.token})
        with urllib.request.urlopen(req, timeout=20) as r:
            return json.loads(r.read().decode("utf-8", "replace") or "{}")

    def _default_get(self, path, query):
        url = self.base_url + path
        clean = {k: v for k, v in (query or {}).items() if v not in (None, "")}
        if clean:
            url += "?" + urllib.parse.urlencode(clean)
        req = urllib.request.Request(url, headers={"X-Comandos-Token": self.token})
        with urllib.request.urlopen(req, timeout=20) as r:
            return json.loads(r.read().decode("utf-8", "replace") or "{}")

    def run(self, name: str, args: dict | None) -> dict:
        args = dict(args or {})
        spec = cat.by_name(name)
        if spec is None:
            return _fail(f"No existe la acción '{name}'.")
        if spec.destructive and args.get("confirm") is not True:
            return _fail(f"'{name}' es destructiva: pide confirmación al usuario y vuelve a llamar con confirm=true.")
        missing = [r for r in spec.required if args.get(r) in (None, "")]
        if missing:
            return _fail(f"Faltan parámetros: {', '.join(missing)}.")
        kind = spec.target["kind"]
        try:
            if kind == "api":
                return self._run_api(spec, args)
            if kind == "ui":
                return self._run_ui(spec, args)
            if kind == "app":
                return self._run_app(spec, args)
            return self._run_local(spec, args)
        except urllib.error.HTTPError as e:
            try:
                detail = json.loads(e.read().decode("utf-8", "replace")).get("error") or str(e)
            except Exception:
                detail = str(e)
            return _fail(f"No pude ({name}): {detail}")
        except Exception as e:
            return _fail(f"No pude ({name}): {str(e)[:200]}")

    def _run_api(self, spec, args):
        t = spec.target
        if t["method"] == "GET":
            data = self._get(t["path"], substitute(t.get("query") or {}, args))
        else:
            data = self._post(t["path"], substitute(t.get("body") or {}, args))
        err = data.get("error") if isinstance(data, dict) else None
        reply = _summarize(data) if spec.readonly else (str(err) if err else "Hecho.")
        return {"ok": not err, "reply": reply, "actions": [], "data": data}

    def _run_ui(self, spec, args):
        t = spec.target
        if t["op"] == "click":
            action = {"type": "ui", "op": "click", "selector": t["selector"]}
        elif t["op"] == "call":
            vals = [substitute(x, args) for x in t.get("args", [])]
            action = {"type": "ui", "op": "call", "fn": t["fn"], "args": [v if v is not _MISSING else None for v in vals]}
        else:
            action = {"type": "ui", "op": "term", "term": {"type": t["type"], **{k: v for k, v in args.items() if k != "confirm"}}}
        return {"ok": True, "reply": "Hecho en el tablero.", "actions": [action], "data": None}

    def _run_app(self, spec, args):
        path = os.path.join(self.hooks_dir, "app-command.json")
        payload = {"command": spec.target["command"], "args": {k: v for k, v in args.items() if k != "confirm"}, "ts": time.time()}
        tmp = path + ".tmp"
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump(payload, f)
        os.replace(tmp, path)
        return {"ok": True, "reply": "Enviado a la app de escritorio.", "actions": [], "data": None}

    def _run_local(self, spec, args):
        fn = self.local_handlers.get(spec.target["intent"])
        if fn is None:
            return _fail(f"Acción local '{spec.target['intent']}' no disponible.")
        out = dict(fn(args) or {})
        out.setdefault("ok", True)
        out.setdefault("reply", "Hecho.")
        out.setdefault("actions", [])
        out.setdefault("data", None)
        return out
```

Nota: `show_view` con `args: ["term:x"]` — en `_run_ui` los `None` de argumentos ausentes se conservan posicionalmente para no desplazar parámetros (`nsOpenPrefilled(cwd, harness, …)`). El test `test_ui_targets_return_browser_actions` usa tools de un solo argumento, así que no ve `None`.

- [ ] **Step 4: Correr** — 9 passed.

- [ ] **Step 5: Commit**

```bash
git add lib/operator_dispatch.py tests/test_operator_dispatch.py
git commit -m "feat(operator): despachador genérico api/ui/app/local con confirmación para destructivas"
```

---

### Task 3: Cliente SSE al proveedor (Anthropic y OpenAI) + caché de credenciales

**Files:**
- Create: `lib/operator_stream.py`
- Test: `tests/test_operator_stream.py`

**Interfaces:**
- Produces:
  ```python
  RETRYABLE = (401, 403, 429, 500, 502, 503, 504)
  class Credentials: __init__(claude_token: Callable, grok_key: Callable, ttl=60); claude(); grok(); invalidate()
  def parse_sse(lines: Iterable[bytes]) -> Iterator[dict]
  def open_stream(url, payload, headers, timeout=30) -> Iterator[bytes]
  def stream_anthropic(events) -> Iterator[dict]   # {"t":"delta","text"} | {"t":"tool_call","id","name","input"} | {"t":"error","text"} | {"t":"done","stop"}
  def stream_openai(events) -> Iterator[dict]      # mismo contrato
  ```

- [ ] **Step 1: Test**

```python
# tests/test_operator_stream.py
import json
import time

from lib import operator_stream as osr


def _sse(*objs):
    return [f"data: {json.dumps(o)}\n".encode() for o in objs] + [b"data: [DONE]\n"]


def test_parse_sse_yields_json_and_skips_done_and_comments():
    lines = [b": ping\n", b"event: x\n"] + _sse({"a": 1}, {"b": 2})
    assert list(osr.parse_sse(lines)) == [{"a": 1}, {"b": 2}]


def test_anthropic_stream_text_and_tool_call():
    ev = [
        {"type": "message_start"},
        {"type": "content_block_start", "index": 0, "content_block": {"type": "text"}},
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Ho"}},
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "la"}},
        {"type": "content_block_stop", "index": 0},
        {"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": "t1", "name": "focus_tab"}},
        {"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{\"tab\": \"Sig"}},
        {"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "nara\"}"}},
        {"type": "content_block_stop", "index": 1},
        {"type": "message_delta", "delta": {"stop_reason": "tool_use"}},
        {"type": "message_stop"},
    ]
    out = list(osr.stream_anthropic(iter(ev)))
    assert out[:2] == [{"t": "delta", "text": "Ho"}, {"t": "delta", "text": "la"}]
    assert {"t": "tool_call", "id": "t1", "name": "focus_tab", "input": {"tab": "Signara"}} in out
    assert out[-1] == {"t": "done", "stop": "tool_use"}


def test_openai_stream_text_and_tool_call():
    ev = [
        {"choices": [{"delta": {"content": "Ho"}}]},
        {"choices": [{"delta": {"content": "la"}}]},
        {"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "c1", "function": {"name": "focus_tab", "arguments": "{\"tab\":"}}]}}]},
        {"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": " \"Signara\"}"}}]}}]},
        {"choices": [{"delta": {}, "finish_reason": "tool_calls"}]},
    ]
    out = list(osr.stream_openai(iter(ev)))
    assert out[:2] == [{"t": "delta", "text": "Ho"}, {"t": "delta", "text": "la"}]
    assert {"t": "tool_call", "id": "c1", "name": "focus_tab", "input": {"tab": "Signara"}} in out
    assert out[-1] == {"t": "done", "stop": "tool_calls"}


def test_credentials_cache_reads_disk_once_per_ttl():
    calls = []
    c = osr.Credentials(lambda: (calls.append("c") or "tokC"), lambda: (calls.append("g") or "keyG"), ttl=0.2)
    assert c.claude() == "tokC" and c.claude() == "tokC" and calls == ["c"]
    time.sleep(0.25)
    c.claude()
    assert calls == ["c", "c"]
    c.invalidate(); c.claude()
    assert calls == ["c", "c", "c"]
```

- [ ] **Step 2: Correr y ver fallar** → import error.

- [ ] **Step 3: Implementar**

```python
# lib/operator_stream.py
"""Streaming SSE hacia Anthropic (/v1/messages) y OpenAI-compatible (/v1/chat/completions)."""
from __future__ import annotations

import json
import time
import urllib.request
from typing import Callable, Iterable, Iterator

RETRYABLE = (401, 403, 429, 500, 502, 503, 504)


class Credentials:
    def __init__(self, claude_token: Callable[[], str], grok_key: Callable[[], str], ttl: float = 60.0):
        self._c, self._g, self._ttl = claude_token, grok_key, ttl
        self._cache: dict[str, tuple[float, str]] = {}

    def _get(self, key, fn):
        now = time.monotonic()
        hit = self._cache.get(key)
        if hit and now - hit[0] < self._ttl:
            return hit[1]
        val = str(fn() or "")
        self._cache[key] = (now, val)
        return val

    def claude(self):
        return self._get("claude", self._c)

    def grok(self):
        return self._get("grok", self._g)

    def invalidate(self):
        self._cache.clear()


def parse_sse(lines: Iterable[bytes]) -> Iterator[dict]:
    for raw in lines:
        line = raw.decode("utf-8", "replace").rstrip("\r\n")
        if not line.startswith("data:"):
            continue
        body = line[5:].strip()
        if not body or body == "[DONE]":
            continue
        try:
            yield json.loads(body)
        except json.JSONDecodeError:
            continue


def open_stream(url: str, payload: dict, headers: dict, timeout: float = 30.0) -> Iterator[bytes]:
    req = urllib.request.Request(url, data=json.dumps(payload).encode(), headers=headers, method="POST")
    resp = urllib.request.urlopen(req, timeout=timeout)   # HTTPError sube al llamador
    try:
        for line in resp:
            yield line
    finally:
        resp.close()


def stream_anthropic(events: Iterator[dict]) -> Iterator[dict]:
    blocks: dict[int, dict] = {}
    stop = None
    for ev in events:
        t = ev.get("type")
        if t == "content_block_start":
            blocks[ev["index"]] = {"block": dict(ev.get("content_block") or {}), "json": ""}
        elif t == "content_block_delta":
            d = ev.get("delta") or {}
            if d.get("type") == "text_delta" and d.get("text"):
                yield {"t": "delta", "text": d["text"]}
            elif d.get("type") == "input_json_delta":
                blocks.setdefault(ev["index"], {"block": {}, "json": ""})["json"] += d.get("partial_json") or ""
        elif t == "content_block_stop":
            b = blocks.pop(ev["index"], None)
            if b and b["block"].get("type") == "tool_use":
                try:
                    inp = json.loads(b["json"] or "{}")
                except json.JSONDecodeError:
                    inp = {}
                yield {"t": "tool_call", "id": b["block"].get("id"), "name": b["block"].get("name"), "input": inp}
        elif t == "message_delta":
            stop = (ev.get("delta") or {}).get("stop_reason") or stop
        elif t == "error":
            yield {"t": "error", "text": str((ev.get("error") or {}).get("message") or "error")}
    yield {"t": "done", "stop": stop}


def stream_openai(events: Iterator[dict]) -> Iterator[dict]:
    calls: dict[int, dict] = {}
    stop = None
    for ev in events:
        for ch in ev.get("choices") or []:
            d = ch.get("delta") or {}
            if d.get("content"):
                yield {"t": "delta", "text": d["content"]}
            for tc in d.get("tool_calls") or []:
                slot = calls.setdefault(tc.get("index", 0), {"id": None, "name": None, "args": ""})
                slot["id"] = tc.get("id") or slot["id"]
                fn = tc.get("function") or {}
                slot["name"] = fn.get("name") or slot["name"]
                slot["args"] += fn.get("arguments") or ""
            if ch.get("finish_reason"):
                stop = ch["finish_reason"]
    for slot in calls.values():
        try:
            inp = json.loads(slot["args"] or "{}")
        except json.JSONDecodeError:
            inp = {}
        yield {"t": "tool_call", "id": slot["id"], "name": slot["name"], "input": inp}
    yield {"t": "done", "stop": stop}
```

- [ ] **Step 4: Correr** → 4 passed.

- [ ] **Step 5: Commit**

```bash
git add lib/operator_stream.py tests/test_operator_stream.py
git commit -m "feat(operator): parser SSE Anthropic/OpenAI y caché de credenciales"
```

---

### Task 4: Loop de agente en streaming en cc-dash + respaldo solo ante errores reales

**Files:**
- Modify: `bin/cc-dash` (`_operator_try_model` :4383, `_operator_post_json` :4337, `operator_llm_chat` :4440, `operator_agent_turn` :4468)
- Create: `tests/test_operator_agent_loop.py`

**Interfaces:**
- Consumes: `operator_stream`, `operator_catalog`, un `dispatcher` con `.run(name, args)`.
- Produces en cc-dash:
  ```python
  OPERATOR_CREDS: operator_stream.Credentials; OPERATOR_TIMEOUT = 30
  def operator_build_payload(model, sys_txt, conv, tools=True) -> (family, payload, headers, urls)   # family: "anthropic" | "openai"
  def operator_provider_stream(model, sys_txt, conv, tools=True) -> (family, Iterator[dict])          # raise RuntimeError si todos fallan
  def operator_agent_stream(text, *, model, tabs, memory, active, convo, dispatcher, max_rounds=6) -> Iterator[dict]
      # {"t":"delta","text"} {"t":"tool","name","args"} {"t":"tool_result","name","ok","reply"} {"t":"action",...} {"t":"error","text"} {"t":"final","reply","actions"}
  def operator_agent_turn(text, *, model, tabs, memory, active, convo, dispatcher) -> {"reply","actions"}
  ```
- Política: otro modelo solo si hubo red o HTTP en `RETRYABLE`; una respuesta sin texto y sin tools cierra el turno con "Listo.". `max_tokens` 1024, timeout 30 s, `cache_control` en `system` y en la última tool (Anthropic).
- Cómo importan los módulos de `lib`: mira cómo cc-dash importa hoy `operator_chat` y `operator_tools` (`grep -n "operator_tools\b" bin/cc-dash | head -3`) y usa exactamente ese mecanismo para `operator_catalog`, `operator_dispatch`, `operator_stream`.

- [ ] **Step 1: Test del loop con proveedor falso**

```python
# tests/test_operator_agent_loop.py
import ast
import json
import types
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = (ROOT / "bin" / "cc-dash").read_text()


def _load(names):
    """Ejecuta solo las defs pedidas de cc-dash en un namespace controlado."""
    from lib import operator_stream, operator_catalog
    tree = ast.parse(SRC)
    wanted = [n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name in names]
    assert {n.name for n in wanted} == set(names), set(names) - {n.name for n in wanted}
    ns = {"operator_stream": operator_stream, "operator_catalog": operator_catalog, "json": json}
    exec(compile(ast.Module(body=wanted, type_ignores=[]), "cc-dash", "exec"), ns)
    return ns


def _fake_chat():
    return types.SimpleNamespace(model_entry=lambda m: {"model": m},
                                 MODELS=[{"model": "haiku"}, {"model": "gpt-5.3-codex-spark"}, {"model": "grok-4.5"}])


def test_build_payload_anthropic_has_cache_control_and_tools():
    ns = _load({"operator_build_payload"})
    ns["OPERATOR_CREDS"] = types.SimpleNamespace(claude=lambda: "tok", grok=lambda: "key")
    ns["load_proxy_cfg"] = lambda: {"port": 18765}
    fam, payload, headers, urls = ns["operator_build_payload"]("haiku", "SYS", [{"role": "user", "content": "hola"}], tools=True)
    assert fam == "anthropic" and payload["stream"] is True and payload["max_tokens"] == 1024
    assert payload["system"][0]["cache_control"] == {"type": "ephemeral"}
    assert payload["tools"][-1]["cache_control"] == {"type": "ephemeral"}
    assert len(payload["tools"]) == len(ns["operator_catalog"].CATALOG)
    assert "api.anthropic.com" in urls[0] and headers["Authorization"] == "Bearer tok"


def test_build_payload_openai_for_grok():
    ns = _load({"operator_build_payload"})
    ns["OPERATOR_CREDS"] = types.SimpleNamespace(claude=lambda: "tok", grok=lambda: "key")
    ns["load_proxy_cfg"] = lambda: {"port": 18765}
    fam, payload, headers, urls = ns["operator_build_payload"]("grok-4.5", "SYS", [{"role": "user", "content": "hola"}])
    assert fam == "openai" and payload["messages"][0] == {"role": "system", "content": "SYS"}
    assert payload["tools"][0]["type"] == "function" and "x.ai" in urls[0]


def test_agent_stream_runs_tool_then_final_text():
    ns = _load({"operator_agent_stream", "_llm_poison"})
    rounds = [
        [{"t": "delta", "text": "Voy."}, {"t": "tool_call", "id": "1", "name": "focus_tab", "input": {"tab": "Signara"}}, {"t": "done", "stop": "tool_use"}],
        [{"t": "delta", "text": "Listo, "}, {"t": "delta", "text": "enfocada."}, {"t": "done", "stop": "end_turn"}],
    ]
    ns["operator_provider_stream"] = lambda model, sys_txt, conv, tools=True: ("anthropic", iter(rounds.pop(0)))
    ns["operator_tools"] = types.SimpleNamespace(agent_system_prompt=lambda *a, **k: "SYS")
    ns["operator_chat"] = _fake_chat()
    calls = []
    dispatcher = types.SimpleNamespace(run=lambda name, args: (calls.append((name, args)) or {"ok": True, "reply": "Foco en Signara", "actions": [{"type": "ui", "op": "click", "selector": "#x"}], "data": None}))
    events = list(ns["operator_agent_stream"]("enfoca signara", model="haiku", tabs=[], memory="", active=None, convo={"messages": []}, dispatcher=dispatcher))
    kinds = [e["t"] for e in events]
    assert kinds[:2] == ["delta", "tool"]
    assert {"t": "tool_result", "name": "focus_tab", "ok": True, "reply": "Foco en Signara"} in events
    assert calls == [("focus_tab", {"tab": "Signara"})]
    assert events[-1] == {"t": "final", "reply": "Listo, enfocada.", "actions": [{"type": "ui", "op": "click", "selector": "#x"}]}


def test_agent_stream_stops_after_max_rounds():
    ns = _load({"operator_agent_stream", "_llm_poison"})
    ns["operator_provider_stream"] = lambda *a, **k: ("anthropic", iter([{"t": "tool_call", "id": "1", "name": "list_tabs", "input": {}}, {"t": "done", "stop": "tool_use"}]))
    ns["operator_tools"] = types.SimpleNamespace(agent_system_prompt=lambda *a, **k: "SYS")
    ns["operator_chat"] = _fake_chat()
    dispatcher = types.SimpleNamespace(run=lambda n, a: {"ok": True, "reply": "[]", "actions": [], "data": []})
    events = list(ns["operator_agent_stream"]("x", model="haiku", tabs=[], memory="", active=None, convo={"messages": []}, dispatcher=dispatcher, max_rounds=3))
    assert sum(1 for e in events if e["t"] == "tool") == 3 and events[-1]["t"] == "final"


def test_agent_stream_reports_provider_failure_without_fallback_storm():
    ns = _load({"operator_agent_stream", "_llm_poison"})
    tried = []
    def boom(model, *a, **k):
        tried.append(model); raise RuntimeError("HTTP 500")
    ns["operator_provider_stream"] = boom
    ns["operator_tools"] = types.SimpleNamespace(agent_system_prompt=lambda *a, **k: "SYS")
    ns["operator_chat"] = _fake_chat()
    events = list(ns["operator_agent_stream"]("x", model="haiku", tabs=[], memory="", active=None, convo={"messages": []}, dispatcher=None))
    assert tried == ["haiku", "gpt-5.3-codex-spark", "grok-4.5"]      # una pasada por modelo, no 4 rondas × 3
    assert events[0]["t"] == "error" and events[-1]["t"] == "final"
```

- [ ] **Step 2: Correr y ver fallar** — `uv run --with pytest python -m pytest -q tests/test_operator_agent_loop.py` → AssertionError en `_load` (las funciones no existen).

- [ ] **Step 3: Implementar en cc-dash** (reemplaza `_operator_try_model`, `_operator_post_json` y `operator_llm_chat`; conserva `_operator_http_error`, `_llm_poison`, `operator_claude_token`, `operator_grok_key`)

```python
OPERATOR_CREDS = operator_stream.Credentials(lambda: operator_claude_token(), lambda: operator_grok_key(), ttl=60)
OPERATOR_TIMEOUT = 30


def operator_build_payload(model, sys_txt, conv, tools=True):
    port = int(load_proxy_cfg().get("port") or 18765)
    proxy_url = f"http://127.0.0.1:{port}/v1/messages"
    if str(model).startswith("grok"):
        key = OPERATOR_CREDS.grok()
        if not key:
            raise RuntimeError("No hay login de Grok.")
        payload = {"model": model, "stream": True,
                   "messages": ([{"role": "system", "content": sys_txt}] if sys_txt else []) + conv}
        if tools:
            payload["tools"] = operator_catalog.openai_tools()
        headers = {"Authorization": "Bearer " + key, "Content-Type": "application/json", "Accept": "text/event-stream"}
        return "openai", payload, headers, ("https://api.x.ai/v1/chat/completions",
                                            "https://cli-chat-proxy.grok.com/v1/chat/completions")
    payload = {"model": model, "max_tokens": 1024, "stream": True, "messages": conv}
    if sys_txt:
        payload["system"] = [{"type": "text", "text": sys_txt, "cache_control": {"type": "ephemeral"}}]
    if tools:
        tl = operator_catalog.anthropic_tools()
        tl[-1] = dict(tl[-1], cache_control={"type": "ephemeral"})
        payload["tools"] = tl
    if model == "haiku" or str(model).startswith("claude"):
        token = OPERATOR_CREDS.claude()
        if not token:
            raise RuntimeError("No hay login de Claude.")
        payload["model"] = "claude-haiku-4-5-20251001" if model == "haiku" else model
        headers = {"Content-Type": "application/json", "Authorization": "Bearer " + token,
                   "anthropic-version": "2023-06-01", "anthropic-beta": "oauth-2025-04-20", "Accept": "text/event-stream"}
        return "anthropic", payload, headers, ("https://api.anthropic.com/v1/messages", proxy_url)
    headers = {"Content-Type": "application/json", "anthropic-version": "2023-06-01", "Accept": "text/event-stream"}
    return "anthropic", payload, headers, (proxy_url,)


def operator_provider_stream(model, sys_txt, conv, tools=True):
    family, payload, headers, urls = operator_build_payload(model, sys_txt, conv, tools)
    last = "sin respuesta"
    for url in urls:
        try:
            lines = operator_stream.open_stream(url, payload, headers, timeout=OPERATOR_TIMEOUT)
            events = operator_stream.parse_sse(lines)
            norm = operator_stream.stream_anthropic(events) if family == "anthropic" else operator_stream.stream_openai(events)
            return family, norm
        except urllib.error.HTTPError as e:
            if e.code == 401:
                OPERATOR_CREDS.invalidate()
            last = _operator_http_error(e)
            if e.code not in operator_stream.RETRYABLE:
                break
        except Exception as e:
            last = str(e)[:240]
    raise RuntimeError(last)


def operator_agent_stream(text, *, model, tabs, memory, active, convo, dispatcher, max_rounds=6):
    sys_txt = operator_tools.agent_system_prompt(tabs, memory, active)
    conv = []
    for msg in (convo.get("messages") or [])[-16:]:
        blob = str(msg.get("text") or "")
        if _llm_poison(blob):
            continue
        conv.append({"role": "assistant" if msg.get("role") != "user" else "user", "content": blob[:4000]})
    if not conv or conv[-1].get("content") != text:
        conv.append({"role": "user", "content": text})
    want = operator_chat.model_entry(model)["model"]
    order = [want] + [m["model"] for m in operator_chat.MODELS if m["model"] != want]
    actions, notes = [], []
    for _round in range(max_rounds):
        family, events, err = None, None, ""
        for cand in order:
            try:
                family, events = operator_provider_stream(cand, sys_txt, conv, tools=True)
                break
            except RuntimeError as e:
                err = str(e)
        if events is None:
            yield {"t": "error", "text": err or "No pude hablar con el modelo."}
            yield {"t": "final", "reply": "No pude hablar con el modelo. Prueba otra vez.", "actions": actions}
            return
        text_acc, calls = "", []
        for ev in events:
            if ev["t"] == "delta":
                text_acc += ev["text"]
                yield ev
            elif ev["t"] == "tool_call":
                calls.append(ev)
            elif ev["t"] == "error":
                yield ev
        if not calls:
            reply = text_acc.strip() or (notes[-1] if notes else "Listo.")
            if _llm_poison(reply):
                reply = "Listo."
            yield {"t": "final", "reply": reply, "actions": actions}
            return
        if family == "openai":
            conv.append({"role": "assistant", "content": text_acc or "",
                         "tool_calls": [{"id": c["id"], "type": "function", "function": {"name": c["name"], "arguments": json.dumps(c["input"])}} for c in calls]})
        else:
            content = ([{"type": "text", "text": text_acc}] if text_acc else []) + \
                      [{"type": "tool_use", "id": c["id"], "name": c["name"], "input": c["input"]} for c in calls]
            conv.append({"role": "assistant", "content": content})
        results = []
        for c in calls:
            yield {"t": "tool", "name": c["name"], "args": c["input"]}
            out = dispatcher.run(c["name"], c["input"])
            actions.extend(out.get("actions") or [])
            for a in out.get("actions") or []:
                yield {"t": "action", **a}
            note = str(out.get("reply") or "ok")
            notes.append(note)
            yield {"t": "tool_result", "name": c["name"], "ok": bool(out.get("ok")), "reply": note}
            results.append((c, note))
        if family == "openai":
            for c, note in results:
                conv.append({"role": "tool", "tool_call_id": c["id"], "content": note})
        else:
            conv.append({"role": "user", "content": [{"type": "tool_result", "tool_use_id": c["id"], "content": note} for c, note in results]})
    yield {"t": "final", "reply": notes[-1] if notes else "Listo.", "actions": actions}


def operator_agent_turn(text, *, model, tabs, memory, active, convo, dispatcher, **_ignored):
    final = {"reply": "Listo.", "actions": []}
    for ev in operator_agent_stream(text, model=model, tabs=tabs, memory=memory, active=active, convo=convo, dispatcher=dispatcher):
        if ev["t"] == "final":
            final = {"reply": ev["reply"], "actions": ev["actions"]}
    return final
```

`operator_handle_chat` sigue llamando a `operator_agent_turn` con los callbacks viejos (`focus=…, close=…`) hasta Task 5; `**_ignored` los absorbe, pero `dispatcher` es obligatorio, así que Task 4 y Task 5 se implementan seguidas y se commitean en el mismo estado verde. Si prefieres dos commits, en Task 4 pasa temporalmente `dispatcher=None` y deja `operator_agent_stream` devolver `{"t":"final","reply":"(operador en migración)"}` cuando `dispatcher is None`; quita eso en Task 5.

- [ ] **Step 4: Correr** — `uv run --with pytest python -m pytest -q tests/test_operator_agent_loop.py` → 5 passed.

- [ ] **Step 5: Commit** (ver nota; normalmente junto con Task 5)

```bash
git add bin/cc-dash tests/test_operator_agent_loop.py
git commit -m "feat(operator): loop de agente en streaming, prompt caching y respaldo solo ante errores reales"
```

---

### Task 5: Handlers `local` + `Dispatcher` en cc-dash; `operator_tools` como adaptador

**Files:**
- Modify: `bin/cc-dash` (`operator_handle_chat` :4540; callbacks `operator_*` :4066-4276; handlers de `/optimization/plans`, `/snippets`, `/paste`, `/model/switch` para extraer funciones si están inline)
- Modify: `lib/operator_tools.py` (`TOOLS` :75, `anthropic_tools` :133, `openai_tools` :137, `agent_system_prompt` :151, `tool_to_intent` :166, `dispatch_tool` :305)
- Modify: `tests/test_operator_tools.py`, `tests/test_operator_chat.py`, `tests/test_operator_agent_loop.py`

**Interfaces:**
- Produces en cc-dash: `operator_build_dispatcher() -> operator_dispatch.Dispatcher` con handlers para TODOS los `LOCAL_INTENTS`; helpers `_ok(err, reply, actions=None)`, `_op_tab(args, tabs, active)`, `operator_optimization_apply(profile, sessions)`, `operator_snippet_send(snip_id, sess)`, `operator_interrupt(sess, pane)`.
- `operator_tools.TOOLS = operator_catalog.anthropic_tools()`; `dispatch_tool(name, args, dispatcher)`.

- [ ] **Step 1: Test** (añadir a `tests/test_operator_agent_loop.py`)

```python
def test_cc_dash_registers_every_local_intent():
    from lib import operator_dispatch as od
    body = SRC.split("def operator_build_dispatcher(", 1)[1].split("\ndef ", 1)[0]
    for intent in od.LOCAL_INTENTS:
        assert f'"{intent}":' in body, intent


def test_handle_chat_uses_dispatcher_not_legacy_callbacks():
    body = SRC.split("def operator_handle_chat(", 1)[1].split("\ndef ", 1)[0]
    assert "dispatcher=operator_build_dispatcher()" in body
    assert "focus=focus_session" not in body
```

- [ ] **Step 2: Correr y ver fallar** → `IndexError` en el split.

- [ ] **Step 3: Implementar en cc-dash** (encima de `operator_handle_chat`)

```python
def _ok(err, reply, actions=None):
    if err:
        return {"ok": False, "reply": str(err)}
    return {"reply": reply, "actions": actions or []}


def _op_tab(args, tabs, active):
    name = str(args.get("tab") or args.get("session") or "")
    if not name:
        return active, None
    hit = operator_chat._resolve_tab(name, tabs, active)
    if hit is None:
        return None, {"ok": False, "reply": f"No encuentro la pestaña '{name}'. Pestañas: {operator_chat.format_tab_list(tabs)}"}
    return hit.get("session") or active, None


def operator_interrupt(sess, pane):
    err = model_switch_cancel(sess, pane)      # extraer del handler "/model/switch-cancel" (:7621) si está inline
    return err or operator_key(sess, "Escape", pane)


def operator_optimization_apply(profile, sessions):
    plans = optimization_plans()               # extraer del handler "/optimization/plans" (:6474) si está inline
    plan = next((p for p in plans if profile in (p.get("id"), p.get("name"))), None)
    if not plan:
        return {"ok": False, "reply": "Perfil desconocido: " + ", ".join(str(p.get("id", "")) for p in plans)}
    done = []
    for sess in sessions:
        err = model_switch_for_session(sess, plan)   # extraer del handler "/model/switch" (:7702): misma lógica que aplica el tablero en applyOptimization
        done.append(f"{sess}: {'ok' if not err else err}")
    return {"reply": "\n".join(done) or "Nada que aplicar"}


def operator_snippet_send(snip_id, sess):
    it = next((s for s in read_snippets() if str(s.get("id")) == snip_id), None)   # read_snippets: la función que alimenta GET /snippets (:6737)
    if not it:
        return {"ok": False, "reply": "Snippet no encontrado"}
    err = tmux_paste(sess, str(it.get("body") or ""))   # tmux_paste: la función del handler "/paste" (:7991)
    return _ok(err, f"Pegado en {sess}")


def operator_build_dispatcher():
    tabs = operator_tabs_payload()
    active = operator_active_session()
    root = operator_store_root()

    def with_tab(fn):
        def inner(args):
            sess, err = _op_tab(args, tabs, active)
            return err or fn(sess, args)
        return inner

    def step(delta):
        def inner(_args):
            names = [t["session"] for t in tabs]
            if not names:
                return {"ok": False, "reply": "No hay pestañas."}
            i = names.index(active) if active in names else -1
            target = names[(i + delta) % len(names)]
            return _ok(focus_session(target), f"Pestaña: {target}")
        return inner

    def ui(action, reply="Hecho en el tablero."):
        return {"reply": reply, "actions": [action]}

    handlers = {
        "focus": with_tab(lambda s, a: _ok(focus_session(s), f"Foco en {s}")),
        "step_next": step(+1),
        "step_prev": step(-1),
        "close": with_tab(lambda s, a: _ok(close_app_tab(s), f"Cerré {s}")),
        "rename": with_tab(lambda s, a: _ok(operator_rename_tab(s, str(a.get("name") or "")), "Renombrada", [{"type": "rename", "session": s, "label": a.get("name")}])),
        "send_back": with_tab(lambda s, a: _ok(operator_send_back(s), "Al final", [{"type": "send_back", "session": s}])),
        "recover": lambda a: _ok(operator_recover(str(a.get("name") or "")), "Recuperada"),
        "toggle_shell": lambda a: _ok(operator_toggle_shell(active), "Alternado"),
        "kill": with_tab(lambda s, a: _ok(operator_kill(s), f"Maté {s}")),
        "send": with_tab(lambda s, a: _ok(operator_send_text(s, str(a.get("text") or ""), a.get("pane")), "Enviado")),
        "key": with_tab(lambda s, a: _ok(operator_key(s, str(a.get("key") or ""), a.get("pane")), "Tecla enviada")),
        "choose_option": with_tab(lambda s, a: _ok(operator_key(s, str(int(a.get("option") or 1)), a.get("pane")) or (time.sleep(0.3), operator_key(s, "Enter", a.get("pane")))[1], f"Opción {a.get('option')}")),
        "interrupt": with_tab(lambda s, a: _ok(operator_interrupt(s, a.get("pane")), "Turno interrumpido")),
        "pause_on": with_tab(lambda s, a: _ok(operator_pause(s, True), "Pausada")),
        "pause_off": with_tab(lambda s, a: _ok(operator_pause(s, False), "Reanudada")),
        "split": with_tab(lambda s, a: _ok(operator_split(s, str(a.get("side") or "derecha")), "Dividido")),
        "close_split": with_tab(lambda s, a: _ok(operator_close_split(s), "Split cerrado")),
        "optimization_apply": lambda a: operator_optimization_apply(str(a.get("profile") or ""), list(a.get("sessions") or [])),
        "pref": lambda a: ui({"type": "pref", "key": a.get("key"), "on": bool(a.get("on"))}),
        "voice": lambda a: ui({"type": "voice", "on": bool(a.get("on"))}),
        "lang": lambda a: ui({"type": "ui", "target": "lang", "lang": a.get("lang")}),
        "theme": lambda a: ui({"type": "ui", "target": "theme", "theme": operator_chat.match_theme_name(str(a.get("theme") or ""))}) if operator_chat.match_theme_name(str(a.get("theme") or "")) else {"ok": False, "reply": "Tema desconocido. Temas: " + ", ".join(operator_tools.THEMES)},
        "notify_pos": lambda a: ui({"type": "notify_pos", "corner": a.get("corner")}),
        "ui_settings": lambda a: ui({"type": "ui", "target": "settings", "tab": a.get("tab")}),
        "ui_panel": lambda a: ui({"type": "ui", "target": a.get("panel"), "tab": a.get("tab")}),
        "snippet_send": lambda a: operator_snippet_send(str(a.get("id") or ""), str(a.get("session") or "")),
        "remember": lambda a: (operator_chat.append_memory(root, str(a.get("fact") or "")), {"reply": "Lo recordaré."})[1],
        "memory_read": lambda a: {"reply": operator_chat.load_memory(root) or "(memoria vacía)"},
        "memory_replace": lambda a: (operator_chat.save_memory(root, str(a.get("text") or "")), {"reply": "Memoria reescrita."})[1],
        "copy_reply": lambda a: ui({"type": "copy_reply"}),
        "copy_session": with_tab(lambda s, a: ui({"type": "copy_session", "session": s})),
    }
    return operator_dispatch.Dispatcher(base_url=f"http://127.0.0.1:{PORT}", token=access_token(), hooks_dir=HOOKS, local_handlers=handlers)
```

Cada `operator_*` existente devuelve `str` de error o `None` (revisa :4066-4276); `_ok` lo convierte. Si alguno devuelve otra cosa, envuélvelo. Los nombres `PORT`, `HOOKS`, `access_token`, `focus_session`, `close_app_tab` ya existen en cc-dash.

En `operator_handle_chat`, sustituir la llamada con callbacks por:

```python
    result = operator_agent_turn(text, model=model, tabs=tabs, memory=memory, active=active,
                                 convo=convo, dispatcher=operator_build_dispatcher())
```

En `lib/operator_tools.py`, dejar solo:

```python
from __future__ import annotations
from . import operator_catalog as _cat        # mismo mecanismo de import que use cc-dash para lib/
from .operator_chat import THEMES, format_tab_list   # si THEMES vive aquí, consérvalo

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
```

Borrar `tool_to_intent` y el literal `TOOLS: list[dict] = [...]`. En `tests/test_operator_tools.py`: reemplazar los asserts sobre `tool_to_intent(...)` por `cat.by_name("focus_tab").target == {"kind": "local", "intent": "focus"}` y similares; el test de clamp de volumen pasa a comprobar que `set_volume` apunta a `/conf-set` con `key: VOLUME` (el clamp real lo hace cc-dash en `/conf-set` :6848). En `tests/test_operator_chat.py`: conservar tests de `parse_intent`/regex solo si pasan; ese parser no está en la ruta viva. Si `apply_intent` deja de tener llamadores, borrarlo junto con sus tests.

- [ ] **Step 4: Correr todo lo del operador**

Run: `uv run --with pytest python -m pytest -q tests/test_operator_agent_loop.py tests/test_operator_tools.py tests/test_operator_chat.py tests/test_operator_catalog.py tests/test_operator_dispatch.py tests/test_operator_stream.py`
Expected: todo verde salvo los 2 `xfail` de Task 1.

- [ ] **Step 5: Probar en vivo**

```bash
systemctl --user restart cc-dash && sleep 2
curl -s -H "X-Comandos-Token: $(cat ~/.claude/hooks/dash-token)" -H 'Content-Type: application/json' \
  -d '{"text":"lista mis pestañas y dime cuál está activa"}' http://127.0.0.1:4777/operator/chat | python3 -m json.tool | head -30
```
Expected: `reply` con la lista real; `actions: []`; menos de 6 s.

- [ ] **Step 6: Commit**

```bash
git add bin/cc-dash lib/operator_tools.py tests/test_operator_tools.py tests/test_operator_chat.py tests/test_operator_agent_loop.py
git commit -m "feat(operator): cc-dash usa el catálogo + despachador; handlers locales registrados"
```

---

### Task 6: Endpoint SSE `POST /operator/chat/stream` y consumo en el tablero

**Files:**
- Modify: `bin/cc-dash` (`do_POST` junto a `/operator/chat` :6757)
- Modify: `dash/index.html` (`opSend` :8541, `opRender` :8422), `dash/sw.js` (lista `live`)
- Modify: `tests/test_operator_agent_loop.py`, `tests/test_remote_ui.py`

**Interfaces:**
- Produces: `POST /operator/chat/stream` con body `{id, text, model, effort}` → `text/event-stream`; frames `data: <json>\n\n` con los eventos de `operator_agent_stream` y un último `{"t":"payload", ...snapshot idéntico al de /operator/chat}`.
- `operator_sse_frame(obj) -> bytes`; `operator_handle_chat_stream(data, write)`.

- [ ] **Step 1: Tests**

En `tests/test_operator_agent_loop.py`:

```python
def test_sse_frame_helper_formats_events():
    ns = _load({"operator_sse_frame"})
    assert ns["operator_sse_frame"]({"t": "delta", "text": "ho\nla"}) == b'data: {"t": "delta", "text": "ho\\nla"}\n\n'
```

En `tests/test_remote_ui.py`:

```python
def test_operator_chat_streams_over_sse():
    dash = open("bin/cc-dash").read()
    assert '"/operator/chat/stream"' in dash and "text/event-stream" in dash
    assert "/operator/chat/stream" in HTML and "getReader()" in HTML and "op-tool" in HTML
    assert "/operator/chat/stream" in SW
```

- [ ] **Step 2: Correr y ver fallar** → FAIL.

- [ ] **Step 3: Backend** (encima de `operator_handle_chat`)

```python
def operator_sse_frame(obj):
    return b"data: " + json.dumps(obj, ensure_ascii=False).encode("utf-8") + b"\n\n"


def operator_handle_chat_stream(data, write):
    root = operator_store_root()
    store = operator_chat.load_store(root)
    cid = str(data.get("id") or store.get("current") or "")
    convo = operator_chat.get_conversation(store, cid) or operator_chat.new_conversation(store)
    text = str(data.get("text") or "")[:4000]
    tabs = operator_tabs_payload()
    memory = operator_chat.load_memory(root)
    active = operator_active_session()
    if data.get("model"):
        operator_chat.set_model(store, str(data.get("model") or "haiku"), str(data.get("effort") or ""))
    model = str(store.get("model") or "haiku")
    operator_chat.append_message(convo, "user", text)
    final = {"reply": "Listo.", "actions": []}
    for ev in operator_agent_stream(text, model=model, tabs=tabs, memory=memory, active=active,
                                    convo=convo, dispatcher=operator_build_dispatcher()):
        if ev["t"] == "final":
            final = {"reply": ev["reply"], "actions": ev["actions"]}
        write(operator_sse_frame(ev))
    operator_chat.append_message(convo, "assistant", final["reply"], final["actions"])
    store["current"] = convo["id"]
    operator_chat.save_store(root, store)
    pub = operator_chat.public_store(store)
    write(operator_sse_frame({"t": "payload", "ok": True, "id": convo["id"], "reply": final["reply"],
                              "actions": final["actions"], "messages": convo.get("messages") or [],
                              "conversations": pub["conversations"], "memory": memory,
                              "model": pub.get("model") or "haiku", "effort": pub.get("effort") or "",
                              "modelName": pub.get("modelName") or "Haiku",
                              "models": pub.get("models") or operator_chat.MODELS}))
```

En `do_POST`, antes de `if self.path == "/operator/chat":`:

```python
        if self.path == "/operator/chat/stream":
            if not str(data.get("text") or "").strip():
                return self._json(400, {"error": "Escribe un mensaje"})
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream; charset=utf-8")
            self.send_header("Cache-Control", "no-cache")
            self.send_header("X-Accel-Buffering", "no")
            self.end_headers()
            def write(chunk):
                self.wfile.write(chunk)
                self.wfile.flush()
            try:
                operator_handle_chat_stream(data, write)
            except (BrokenPipeError, ConnectionResetError):
                pass
            except Exception as e:
                try:
                    write(operator_sse_frame({"t": "error", "text": str(e)[:300]}))
                except Exception:
                    pass
            return
```

En `dash/sw.js`, añadir `"/operator"` a la lista `live` (cubre `/operator`, `/operator/chat` y `/operator/chat/stream`).

- [ ] **Step 4: Frontend** — reemplazar `opSend` en `dash/index.html`:

```javascript
function opToolLine(name, args){
  const short = Object.entries(args||{}).filter(([k])=>k!=="confirm")
    .map(([k,v])=>`${k}=${typeof v==="string"?v:JSON.stringify(v)}`).join(", ");
  return `⚙ ${name}(${short})`;
}
let opLiveRaf = 0;
function opRenderLive(){ if(!opLiveRaf) opLiveRaf = requestAnimationFrame(() => { opLiveRaf = 0; opRender(); }); }
async function opSend(text){
  const t = String(text||"").trim();
  if(!t || OP.ac) return;
  OP.messages = [...(OP.messages||[]), {role:"user", text:t}];
  const live = {role:"assistant", text:"", tools:[]};
  OP.messages.push(live);
  opRender();
  OP.ac = new AbortController();
  opBusy(true);
  try{
    const headers = {"Content-Type":"application/json"};
    const tok = authToken(); if(tok) headers["X-Comandos-Token"] = tok;
    const res = await fetch("/operator/chat/stream", {method:"POST", headers, signal: OP.ac.signal,
      body: JSON.stringify({id: OP.id, text: t, model: OP.model, effort: OP.effort})});
    if(!res.ok || !res.body) throw new Error((await res.json().catch(()=>({}))).error || res.statusText);
    const reader = res.body.getReader();
    const dec = new TextDecoder();
    let buf = "", payload = null;
    for(;;){
      const {value, done} = await reader.read();
      if(done) break;
      buf += dec.decode(value, {stream:true});
      let i;
      while((i = buf.indexOf("\n\n")) >= 0){
        const frame = buf.slice(0, i); buf = buf.slice(i+2);
        const line = frame.split("\n").find(l => l.startsWith("data:"));
        if(!line) continue;
        let ev; try{ ev = JSON.parse(line.slice(5)); }catch(e){ continue; }
        if(ev.t === "delta"){ live.text += ev.text; opRenderLive(); }
        else if(ev.t === "tool"){ live.tools.push({name:ev.name, args:ev.args, ok:null}); opRenderLive(); }
        else if(ev.t === "tool_result"){ const x = [...live.tools].reverse().find(z => z.name===ev.name && z.ok===null); if(x){ x.ok = ev.ok; x.reply = ev.reply; } opRenderLive(); }
        else if(ev.t === "action"){ opApplyActions([ev]); }
        else if(ev.t === "error"){ live.text += (live.text?"\n":"") + "⚠ " + ev.text; opRenderLive(); }
        else if(ev.t === "final"){ if(!live.text) live.text = ev.reply; }
        else if(ev.t === "payload"){ payload = ev; }
      }
    }
    if(payload){ opApplyPayload(payload); opRender(); }
  }catch(e){
    live.text = e.name === "AbortError" ? tf("Paré la respuesta.","Stopped.") : (e.message || tf("No pude hablar con ComandOS","Could not talk to ComandOS"));
    opRender();
  } finally {
    OP.ac = null;
    opBusy(false);
  }
}
```

En `opRender`, para cada mensaje del asistente, anteponer al texto:

```javascript
      const tools = (m.tools||[]).map(x => `<div class="op-tool ${x.ok===false?"bad":x.ok?"ok":"run"}">${opEsc(opToolLine(x.name, x.args))}${x.reply?` <span>${opEsc(String(x.reply).slice(0,140))}</span>`:""}</div>`).join("");
```

y CSS junto a los estilos de `#op-chat`:

```css
.op-tool{font:11px/1.4 var(--mono);color:var(--faint);padding:2px 0 2px 8px;border-left:2px solid var(--line2);margin:2px 0}
.op-tool.ok{border-color:var(--done)} .op-tool.bad{border-color:var(--waiting)} .op-tool.run{border-color:var(--working)}
```

- [ ] **Step 5: Verificar**

Run: `bash tests/test_js_parses.sh && uv run --with pytest python -m pytest -q tests/test_remote_ui.py tests/test_operator_agent_loop.py`

Manual: `systemctl --user restart cc-dash`; en el tablero escribir "abre el pomodoro y dime cuánto uso llevo hoy". El texto debe aparecer por partes, con dos líneas ⚙ (open_pomodoro, usage_state) y el panel abriéndose antes de terminar la respuesta. En DevTools → Network, `TTFB` de `/operator/chat/stream` < 2 s.

- [ ] **Step 6: Commit**

```bash
git add bin/cc-dash dash/index.html dash/sw.js tests/test_remote_ui.py tests/test_operator_agent_loop.py
git commit -m "feat(operator): streaming SSE del chat con líneas de tool en vivo"
```

---

### Task 7: Acciones `ui` genéricas en el tablero y globales por control

**Files:**
- Modify: `dash/index.html` (`opApplyActions` :8464; `notifPaint` :5402-5446 para `data-id`)
- Modify: `tests/test_operator_catalog.py` (quitar el `xfail` de `test_ui_targets_point_to_real_selectors_or_functions`), `tests/test_remote_ui.py`

**Interfaces:**
- Produces globales `window.*`: `opFavorite(session,on)`, `setPollSeconds(n)`, `setBrowserNotifications(on)`, `setLimitStyle(s)`, `nfDismiss(id)`, `nfPin(id)`, `nfUnpin(id)`, `nfSnooze(id)`, `setTimeline(on)`, `closeAllPanels()`, `swOpenWith(q)`, `nsOpenPrefilled(cwd,harness,motor,model,effort,danger)`, `selectSessionCard(session)`, `expandReply(session,on)`, `openAnalyticsTab(tab)`, `compareSetDays(n)`, `setSplitLeft(px)`, `setOpChatHeight(px)`, `reloadDashboard()`, `focusVisibleTerm()`. Ya existen: `showView`, `toast`, `copyText`, `askCloseTab`, `openMotorFor`.
- `opApplyActions` maneja `ui/click`, `ui/call`, `ui/term`, `pref`, `voice`, `notify_pos` además de lo que ya manejaba.

- [ ] **Step 1: Test** (en `tests/test_remote_ui.py`)

```python
def test_operator_generic_ui_actions_and_globals_exist():
    for fn in ("opFavorite", "setPollSeconds", "setBrowserNotifications", "setLimitStyle", "nfDismiss",
               "nfPin", "nfUnpin", "nfSnooze", "setTimeline", "closeAllPanels", "swOpenWith",
               "nsOpenPrefilled", "selectSessionCard", "expandReply", "openAnalyticsTab",
               "compareSetDays", "setSplitLeft", "setOpChatHeight", "reloadDashboard", "focusVisibleTerm"):
        assert f"window.{fn} = " in HTML, fn
    for op in ('a.op === "click"', 'a.op === "call"', 'a.op === "term"'):
        assert op in HTML, op
    assert 'a.type === "pref"' in HTML and 'a.type === "voice"' in HTML and 'a.type === "notify_pos"' in HTML
```

- [ ] **Step 2: Correr y ver fallar** → FAIL.

- [ ] **Step 3: Implementar** — al inicio del `for` de `opApplyActions`:

```javascript
    if(a.type === "ui" && a.op === "click"){
      const el = document.querySelector(a.selector);
      if(el) el.click(); else toast(tf("No encuentro el control ", "Control not found ") + a.selector, true);
      continue;
    }
    if(a.type === "ui" && a.op === "call"){
      const fn = window[a.fn];
      if(typeof fn === "function"){ try{ fn(...(a.args||[])); }catch(e){ toast(String(e.message||e), true); } }
      else toast(tf("Acción desconocida ", "Unknown action ") + a.fn, true);
      continue;
    }
    if(a.type === "ui" && a.op === "term"){
      const sess = a.term?.session || activeTerm || [...openTerms.keys()][0];
      const frame = sess && openTerms.get(sess)?.frame;
      if(frame?.contentWindow) frame.contentWindow.postMessage({source:"comandos", type:"toolbar", term:a.term}, location.origin);
      else toast(tf("No hay terminal abierta", "No terminal open"), true);
      continue;
    }
    if(a.type === "pref"){ api("/conf-set", {key:a.key, value:a.on?"1":"0"}).then(loadConf).catch(e=>toast(e.message,true)); continue; }
    if(a.type === "voice"){ Promise.all([api("/conf-set",{key:"SPEAK_DONE",value:a.on?"1":"0"}), api("/conf-set",{key:"SPEAK_ATTENTION",value:a.on?"1":"0"})]).then(loadConf).catch(e=>toast(e.message,true)); continue; }
    if(a.type === "notify_pos"){ document.querySelector(`[data-npos="${a.corner}"]`)?.click(); continue; }
```

`loadConf` = la función que hoy rellena Ajustes desde `GET /conf` (`grep -n 'api("/conf")' dash/index.html`); usa su nombre real. Ampliar el bloque existente `a.type === "ui"` (con `a.target`) para: `"analytics"` → `openAnalyticsTab(a.tab)`, `"wizard"` → `nsOpen()`, `"centro"` → `document.getElementById("centro")?.scrollIntoView({behavior:"smooth"})`, `"switcher"` → `swOpen()`, `"timeline"` → `setTimeline(true)`, `"sov"` → `$("#btn-sov")?.click()`.

Globales (después de `opApplyActions`). Verifica cada nombre interno con grep antes de usarlo (`PREFS`/`loadPrefs`/`render`, `activateMtab`, `restoreSplitLeft`, `applyAppLayout`, `initOpChatSplit`, clase de fila expandida, `.chip`/`data-v` del wizard) y ajusta al real:

```javascript
window.opFavorite = async (session, on) => {
  const favs = new Set(PREFS?.favorites || []);
  if(on) favs.add(session); else favs.delete(session);
  await api("/prefs-set", {favorites:[...favs]}); await loadPrefs(); render();
};
window.setPollSeconds = (n) => { const el = $("#poll"); if(el){ el.value = n; el.dispatchEvent(new Event("input", {bubbles:true})); } };
window.setBrowserNotifications = (on) => { const el = $("#sw-notif"); if(el && el.checked !== !!on) el.click(); };
window.setLimitStyle = (s) => setLimitStyle(s);
window.nfDismiss = (id) => document.querySelector(`.nf2-x[data-id="${CSS.escape(id)}"]`)?.click();
window.nfPin = (id) => document.querySelector(`[data-act="pin"][data-id="${CSS.escape(id)}"]`)?.click();
window.nfUnpin = (id) => document.querySelector(`[data-act="unpin"][data-id="${CSS.escape(id)}"]`)?.click();
window.nfSnooze = (id) => document.querySelector(`[data-act="snooze"][data-id="${CSS.escape(id)}"]`)?.click();
window.setTimeline = (on) => { const open = $("#timeline")?.classList.contains("on"); if(!!on !== !!open) $("#tl-toggle")?.click(); };
window.closeAllPanels = () => { document.querySelectorAll(".modal.open").forEach(m => m.classList.remove("open")); closeModelMenus(); if(typeof motorPopClose === "function") motorPopClose(); };
window.swOpenWith = (q) => { swOpen(); const i = $("#sw-in"); if(i){ i.value = q||""; i.dispatchEvent(new Event("input", {bubbles:true})); } };
window.nsOpenPrefilled = (cwd, harness, motor, model, effort, danger) => {
  nsOpen();
  if(cwd){ const c = $("#ns-cwd"); c.value = cwd; c.dispatchEvent(new Event("input", {bubbles:true})); }
  const pick = (box, val) => val && [...document.querySelectorAll(`${box} .chip`)].find(ch => ch.dataset.v === val || ch.textContent.trim() === val)?.click();
  pick("#ns-harness", harness); pick("#ns-motor", motor); pick("#ns-model", model); pick("#ns-effort", effort);
  if(typeof danger === "boolean"){ const d = $("#ns-danger"); if(d && d.checked !== danger) d.click(); }
};
window.selectSessionCard = (session) => document.querySelector(`.row[data-session="${CSS.escape(session)}"]`)?.click();
window.expandReply = (session, on) => { const row = document.querySelector(`.row[data-session="${CSS.escape(session)}"]`); const open = row?.querySelector(".rxp")?.classList.contains("on"); if(row && !!on !== !!open) row.querySelector(".xp")?.click(); };
window.openAnalyticsTab = (tab) => { $("#btn-usage")?.click(); activateMtab("usage", tab || "resumen"); };
window.compareSetDays = (n) => { const s = $("#compare-days"); if(s){ s.value = String(n); s.dispatchEvent(new Event("change", {bubbles:true})); } };
window.setSplitLeft = (px) => { localStorage.setItem("cc-split-left", String(px)); restoreSplitLeft(); applyAppLayout(); };
window.setOpChatHeight = (px) => { localStorage.setItem("cc-op-chat-h", String(px)); initOpChatSplit(); };
window.reloadDashboard = () => location.reload();
window.focusVisibleTerm = () => { const f = activeTerm && openTerms.get(activeTerm)?.frame; f?.contentWindow?.postMessage({source:"comandos", type:"toolbar", term:{type:"focus"}}, location.origin); };
```

En `notifPaint` añadir `data-id="${n.id}"` a `.nf2-x` y a los botones `[data-act]` si no lo llevan.

- [ ] **Step 4: Quitar el xfail y correr**

Run: `bash tests/test_js_parses.sh && uv run --with pytest python -m pytest -q tests/test_operator_catalog.py tests/test_remote_ui.py`
Expected: `test_ui_targets_point_to_real_selectors_or_functions` pasa sin xfail.

- [ ] **Step 5: Commit**

```bash
git add dash/index.html tests/test_remote_ui.py tests/test_operator_catalog.py
git commit -m "feat(operator): acciones ui genéricas (click/call/term) y globales para cada control del tablero"
```

---

### Task 8: Toolbar del terminal web controlable por postMessage (`term.html`)

**Files:**
- Modify: `dash/term.html` (`handleTerminalMessage`)
- Modify: `tests/test_remote_ui.py`

- [ ] **Step 1: Test**

```python
def test_term_page_accepts_toolbar_messages_from_parent():
    term = open("dash/term.html").read()
    assert "data.type === 'toolbar'" in term
    for k in ("sendToolbarKey(", "pasteTerminalText(", "requestInteractionMode(", "setCtrlArmed("):
        assert k in term
```

- [ ] **Step 2: Correr y ver fallar** → FAIL.

- [ ] **Step 3: Implementar** — en `handleTerminalMessage`, antes del `return false` final:

```javascript
    if (data.type === 'toolbar') {
      const t = data.term || {};
      if (t.type === 'toolbar' && TOOLBAR_KEYS[t.key]) { sendToolbarKey(t.key); return true; }
      if (t.type === 'paste' && typeof t.text === 'string') { pasteTerminalText(t.text); focusTerminal(); return true; }
      if (t.type === 'mode') {
        if (interactionState.known && interactionState.selecting !== !!t.selecting) requestInteractionMode();
        return true;
      }
      if (t.type === 'ctrl') { setCtrlArmed(true); focusTerminal(); return true; }
      if (t.type === 'focus') { focusTerminal(); return true; }
      return false;
    }
```

(`TOOLBAR_KEYS`, `sendToolbarKey`, etc. se definen más abajo en la misma IIFE; el handler solo corre tras la carga, así que la referencia es válida.)

- [ ] **Step 4: Correr** — `bash tests/test_js_parses.sh && uv run --with pytest python -m pytest -q tests/test_remote_ui.py -k toolbar` → pasa.

- [ ] **Step 5: Commit**

```bash
git add dash/term.html tests/test_remote_ui.py
git commit -m "feat(term): la toolbar del terminal web se controla por postMessage desde el operador"
```

---

### Task 9: Comandos a la app de escritorio (`app-command.json` → GTK) y `POST /app/command`

**Files:**
- Modify: `bin/cc-app` (junto a `on_tab_back_request` :5017 y monitores :4979-5031)
- Modify: `bin/cc-dash` (`do_POST`, antes de la zona que exige `session`)
- Modify: `tests/test_operator_catalog.py` (quitar el `xfail` de `test_app_targets_are_handled_by_cc_app`), `tests/test_desktop_tabs.py`, `tests/test_remote_ui.py`

**Interfaces:**
- Produces en cc-app: `APP_COMMANDS: dict[str, Callable[[dict], Any]]` y `on_app_command(path)`. Comandos: `split`, `split_ssh`, `kill_pane`, `toggle_window`, `select_pane`, `next_tab`, `prev_tab`, `mru_toggle`, `focus_page`, `tab_reorder`, `mosaic`, `mosaic_zoom`, `side_panel`, `terminals_visible`, `reload_dashboard`, `font_scale`, `open_switcher`, `tabs_overview`, `help`, `snippets`, `new_local_tab`, `open_xterm_tab`, `open_wizard`, `start_ai_here`, `copy_selection`, `paste_clipboard`, `copy_reply`, `window`, `paned_position`, `quit`.
- Produces en cc-dash: `POST /app/command {command, args}` → escribe el mismo archivo (para tablero remoto y scripts).

- [ ] **Step 1: Tests**

En `tests/test_desktop_tabs.py` (reutiliza la variable con el texto de `bin/cc-app` que ya use ese archivo; si no hay, `APP_SRC = (ROOT/"bin"/"cc-app").read_text()`):

```python
def test_app_command_table_covers_catalog():
    from lib import operator_catalog as cat
    table = APP_SRC.split("APP_COMMANDS = {", 1)[1].split("\n}\n", 1)[0]
    for t in cat.CATALOG:
        if t.target["kind"] == "app":
            assert f'"{t.target["command"]}":' in table, t.target["command"]
    assert "app-command.json" in APP_SRC and "def on_app_command(" in APP_SRC
```

En `tests/test_remote_ui.py`:

```python
def test_app_command_endpoint_exists():
    dash = open("bin/cc-dash").read()
    assert '"/app/command"' in dash and "APP_COMMAND_NAMES" in dash
```

- [ ] **Step 2: Correr y ver fallar** → FAIL.

- [ ] **Step 3: Implementar en cc-app** (antes de `on_tab_back_request`). Cada nombre usado ya existe en cc-app según el inventario; verifica la firma con `grep -n "^def <nombre>\|^    def <nombre>" bin/cc-app` y adapta:

```python
def _cur_page():
    return nb.get_nth_page(nb.get_current_page())


def _cur_term():
    page = _cur_page()
    return getattr(page, "_term", None) or (page if hasattr(page, "feed_child") else None)


def _page_for(sess):
    for i in range(nb.get_n_pages()):
        p = nb.get_nth_page(i)
        if getattr(p, "_key", None) == sess:
            return i, p
    return -1, None


def _cur_pane_id():
    key = getattr(_cur_page(), "_key", None) or "local"
    r = subprocess.run(["tmux", "display-message", "-p", "-t", f"={key}", "#{pane_id}"], capture_output=True, text=True)
    return r.stdout.strip()


def _split_cmd(side, ssh=False):
    flags = {"right": ["-h"], "left": ["-h", "-b"], "down": ["-v"], "up": ["-v", "-b"]}[side]
    term = _cur_term()
    if term is not None:
        split(term, flags, ssh=ssh)     # :1128 — si la firma real es split(flags) o split(term, flags), adapta


APP_COMMANDS = {
    "split": lambda a: _split_cmd(a.get("side", "right")),
    "split_ssh": lambda a: _split_cmd(a.get("side", "right"), ssh=True),
    "kill_pane": lambda a: subprocess.run(["tmux", "kill-pane", "-t", _cur_pane_id()]),
    "toggle_window": lambda a: feed(_cur_term(), b"\x02l"),
    "select_pane": lambda a: subprocess.run(["tmux", "select-pane", "-t", str(a.get("pane") or "")]),
    "next_tab": lambda a: _cycle_tab(1),
    "prev_tab": lambda a: _cycle_tab(-1),
    "mru_toggle": lambda a: _mru_toggle(),
    "focus_page": lambda a: (lambda i: nb.set_current_page(i) if i >= 0 else None)(_page_for(a.get("session"))[0]),
    "tab_reorder": lambda a: (lambda i, p: (nb.reorder_child(p, int(a.get("index", 0))), save_tabs()) if p else None)(*_page_for(a.get("session"))),
    "mosaic": lambda a: {"on": _mosaic_open, "off": _mosaic_close, "toggle": _mosaic_toggle}[a.get("state", "toggle")](),
    "mosaic_zoom": lambda a: _mosaic_zoom(a.get("session"), a.get("session")),
    "side_panel": lambda a: _side_toggle() if bool(a.get("on")) != (paned.get_position() > 0) else None,
    "terminals_visible": lambda a: nb.set_visible(bool(a.get("on", True))),
    "reload_dashboard": lambda a: wv.load_uri(URL),
    "font_scale": lambda a: set_font_scale(1.0) if not a.get("delta") else set_font_scale(_cur_term().get_font_scale() + float(a["delta"])),
    "open_switcher": lambda a: open_switcher(),
    "tabs_overview": lambda a: open_tabs_overview(),
    "help": lambda a: show_help(),
    "snippets": lambda a: open_snippets_dialog(),
    "new_local_tab": lambda a: new_local_tab(),
    "open_xterm_tab": lambda a: open_xterm_tab(a.get("session") or "local"),
    "open_wizard": lambda a: _open_wizard(),
    "start_ai_here": lambda a: open_ai_session_here(a.get("session"), a.get("pane")),
    "copy_selection": lambda a: copy_vte_selection(_cur_term()),
    "paste_clipboard": lambda a: (exit_copy_mode(_cur_term()), _cur_term().paste_clipboard()),
    "copy_reply": lambda a: copy_claude_reply(_cur_term()),
    "window": lambda a: {"minimize": win.iconify, "maximize": win.maximize, "restore": win.unmaximize, "raise": raise_main_window}[a.get("action", "raise")](),
    "paned_position": lambda a: paned.set_position(int(a.get("px", 700))),
    "quit": lambda a: Gtk.main_quit(),
}


def on_app_command(path):
    try:
        with open(path) as f:
            data = json.load(f)
        os.remove(path)
    except Exception:
        return
    fn = APP_COMMANDS.get(str(data.get("command") or ""))
    if not fn:
        return
    try:
        fn(data.get("args") or {})
    except Exception as e:
        notify_popup(f"Comando '{data.get('command')}' falló: {e}")
```

Registrar el monitor igual que el de `app-tab-back.json` (:5031):

```python
_cmd_file = Gio.File.new_for_path(os.path.join(HOOKS, "app-command.json"))
_cmd_mon = _cmd_file.monitor_file(Gio.FileMonitorFlags.NONE, None)
_cmd_mon.connect("changed", lambda m, f, o, ev: on_app_command(f.get_path())
                 if ev in (Gio.FileMonitorEvent.CREATED, Gio.FileMonitorEvent.CHANGES_DONE_HINT) else None)
```

En cc-dash `do_POST` (antes de la zona `sess = data.get("session")` :7465):

```python
        if self.path == "/app/command":
            cmd = str(data.get("command") or "")
            if cmd not in operator_catalog.APP_COMMAND_NAMES:
                return self._json(400, {"error": "comando desconocido"})
            spec = operator_catalog.by_name(operator_catalog.APP_TOOL_BY_COMMAND[cmd])
            args = dict(data.get("args") or {})
            if spec.destructive and args.get("confirm") is not True:
                return self._json(400, {"error": "requiere confirm=true"})
            operator_build_dispatcher()._run_app(spec, args)
            return self._json(200, {"ok": True})
```

- [ ] **Step 4: Quitar el xfail y correr**

Run: `python3 -m py_compile bin/cc-app && uv run --with pytest python -m pytest -q tests/test_desktop_tabs.py tests/test_operator_catalog.py tests/test_remote_ui.py -k "app_command or app_targets"`
Expected: pasan.

Manual con la app abierta: `echo '{"command":"mosaic","args":{"state":"toggle"},"ts":0}' > ~/.claude/hooks/app-command.json` → abre el mosaico; repetir lo cierra.

- [ ] **Step 5: Commit**

```bash
git add bin/cc-app bin/cc-dash tests/test_desktop_tabs.py tests/test_operator_catalog.py tests/test_remote_ui.py
git commit -m "feat(app): comandos del operador a la app GTK (splits, mosaico, pestañas, ventana, zoom) + POST /app/command"
```

---

### Task 10: Tools de solo lectura en paralelo y medición de latencia

**Files:**
- Modify: `bin/cc-dash` (`operator_agent_stream`)
- Modify: `tests/test_operator_agent_loop.py`

**Interfaces:**
- Produces: `_run_tool_batch(dispatcher, calls) -> list[dict]` (resultados en el orden de `calls`; `READONLY` en paralelo, el resto en serie).

- [ ] **Step 1: Test**

```python
def test_readonly_tools_run_in_parallel_and_keep_order():
    import time as _t
    ns = _load({"operator_agent_stream", "_llm_poison", "_run_tool_batch"})
    ns["operator_provider_stream"] = lambda *a, **k: ("anthropic", iter([
        {"t": "tool_call", "id": "1", "name": "usage_state", "input": {}},
        {"t": "tool_call", "id": "2", "name": "list_tabs", "input": {}},
        {"t": "done", "stop": "tool_use"}]))
    ns["operator_tools"] = types.SimpleNamespace(agent_system_prompt=lambda *a, **k: "SYS")
    ns["operator_chat"] = _fake_chat()
    def run(name, args):
        _t.sleep(0.2); return {"ok": True, "reply": name, "actions": [], "data": None}
    t0 = _t.monotonic()
    events = list(ns["operator_agent_stream"]("x", model="haiku", tabs=[], memory="", active=None, convo={"messages": []},
                                              dispatcher=types.SimpleNamespace(run=run), max_rounds=1))
    assert _t.monotonic() - t0 < 0.35
    assert [e["name"] for e in events if e["t"] == "tool_result"] == ["usage_state", "list_tabs"]
```

- [ ] **Step 2: Correr y ver fallar** → FAIL (`_run_tool_batch` no existe).

- [ ] **Step 3: Implementar** — encima de `operator_agent_stream`:

```python
def _run_tool_batch(dispatcher, calls):
    from concurrent.futures import ThreadPoolExecutor
    out = [None] * len(calls)
    ro = [(i, c) for i, c in enumerate(calls) if c["name"] in operator_catalog.READONLY]
    rw = [(i, c) for i, c in enumerate(calls) if c["name"] not in operator_catalog.READONLY]
    if len(ro) > 1:
        with ThreadPoolExecutor(max_workers=min(6, len(ro))) as ex:
            for (i, _c), res in zip(ro, ex.map(lambda ic: dispatcher.run(ic[1]["name"], ic[1]["input"]), ro)):
                out[i] = res
    else:
        for i, c in ro:
            out[i] = dispatcher.run(c["name"], c["input"])
    for i, c in rw:
        out[i] = dispatcher.run(c["name"], c["input"])
    return out
```

y en `operator_agent_stream` sustituir el bucle `for c in calls:` (el que llama a `dispatcher.run`) por:

```python
        for c in calls:
            yield {"t": "tool", "name": c["name"], "args": c["input"]}
        results = []
        for c, out in zip(calls, _run_tool_batch(dispatcher, calls)):
            actions.extend(out.get("actions") or [])
            for a in out.get("actions") or []:
                yield {"t": "action", **a}
            note = str(out.get("reply") or "ok")
            notes.append(note)
            yield {"t": "tool_result", "name": c["name"], "ok": bool(out.get("ok")), "reply": note}
            results.append((c, note))
```

- [ ] **Step 4: Correr** — `uv run --with pytest python -m pytest -q tests/test_operator_agent_loop.py` → pasa (incluido `test_agent_stream_runs_tool_then_final_text`, cuyo orden de eventos ya no intercala `tool` y `tool_result`; si ese test comprobaba `kinds[:2] == ["delta","tool"]` sigue válido).

- [ ] **Step 5: Medir en vivo**

```bash
systemctl --user restart cc-dash && sleep 2
T=$(cat ~/.claude/hooks/dash-token)
curl -s -N -o /dev/null -w 'ttfb=%{time_starttransfer}s total=%{time_total}s\n' -H "X-Comandos-Token: $T" -H 'Content-Type: application/json' \
  -d '{"text":"cuánto uso llevo hoy y qué pestañas tengo"}' http://127.0.0.1:4777/operator/chat/stream
```
Expected: `ttfb < 2.0`, `total < 8`. Si `ttfb` es mayor: imprime `len(operator_tools.agent_system_prompt(...))` y comprueba en el segundo mensaje que `message_start.message.usage.cache_read_input_tokens > 0` (añade un `dbg` temporal en `stream_anthropic`). Si el prompt de tools supera ~25k tokens, reduce descripciones del catálogo, no el número de tools.

- [ ] **Step 6: Commit**

```bash
git add bin/cc-dash tests/test_operator_agent_loop.py
git commit -m "perf(operator): tools de solo lectura en paralelo"
```

---

### Task 11: Documentación generada y aceptación end-to-end

**Files:**
- Create: `docs/operator-actions.md`
- Modify: `README.es.md` (sección del operador), `tests/test_operator_catalog.py`

- [ ] **Step 1: Test**

```python
def test_operator_actions_doc_lists_every_tool():
    doc = (ROOT / "docs" / "operator-actions.md").read_text()
    for t in cat.CATALOG:
        assert f"`{t.name}`" in doc, t.name
```

- [ ] **Step 2: Correr y ver fallar** → FAIL (no existe el archivo).

- [ ] **Step 3: Generar la doc** (script one-shot; se commitea la salida, no el script)

```bash
python3 - <<'PY' > docs/operator-actions.md
from lib import operator_catalog as cat
print("# Acciones del operador de ComandOS\n\nGenerado desde `lib/operator_catalog.py`. Un control de la UX sin fila aquí es un defecto.\n")
groups = {}
for t in cat.CATALOG: groups.setdefault(t.group, []).append(t)
for g, items in groups.items():
    print(f"\n## {g} ({len(items)})\n\n| Tool | Qué hace | Parámetros | Cómo se ejecuta |\n|---|---|---|---|")
    for t in items:
        tg = t.target
        how = {"api": f'{tg.get("method")} {tg.get("path")}', "ui": f'ui:{tg.get("op")} {tg.get("selector") or tg.get("fn") or tg.get("type")}', "app": f'app:{tg.get("command")}', "local": f'local:{tg.get("intent")}'}[tg["kind"]]
        flags = (" ⚠ confirm" if t.destructive else "") + (" 👁" if t.readonly else "")
        print(f"| `{t.name}`{flags} | {t.description} | {', '.join(t.params) or '—'} | {how} |")
PY
```

En `README.es.md`, en la sección del chat del operador, dos líneas: responde en streaming y puede ejecutar cualquier acción del tablero, terminal web y app; enlace a `docs/operator-actions.md`.

- [ ] **Step 4: Aceptación manual** (cc-dash reiniciado, app abierta; anotar resultado en el mensaje del commit)

1. "lista mis pestañas" → tabla real, sin acciones.
2. "pásate a Signara y divide el pane a la derecha" → foco y split visibles en la app; dos líneas ⚙.
3. "mata la sesión dup-probe" → el operador pide confirmación; "sí" → se ejecuta.
4. "abre analytics en comparar con 30 días" → panel en esa tab con 30 días.
5. "pon el tema neon y el volumen al 40" → tema cambiado y slider en 40.
6. Desde el celular: "en el terminal manda Escape" → la toolbar del iframe recibe la tecla.
7. "activa el mosaico" → mosaico en la app; "quítalo" → se cierra.
8. "prende el remoto" → `remote-on`; "apágalo" → pide confirmación.
9. Streaming: el texto aparece por partes; TTFB de `/operator/chat/stream` < 2 s.
10. Con Grok 4.5 como modelo del chat, repetir 1 y 2 (ruta OpenAI).

- [ ] **Step 5: Commit**

```bash
git add docs/operator-actions.md README.es.md tests/test_operator_catalog.py
git commit -m "docs(operator): tabla de todas las acciones + aceptación e2e"
```

---

## Self-review

- **Cobertura de la spec:** streaming (Tasks 3, 4, 6); latencia (3, 4, 10); catálogo completo (1) con targets `api` (2), `ui` (7), `term` (8), `app` (9), `local` (5); confirmación de destructivas (1, 2, 9); documentación (11). Los mínimos por grupo de `test_minimum_coverage_per_group` se cumplen con el catálogo de la Parte 2 (198 tools; verificado al escribir el plan: todos los paths `api` existen en cc-dash).
- **Placeholders:** ninguno. Los nombres que dependen del código real (`optimization_plans`, `model_switch_for_session`, `model_switch_cancel`, `read_snippets`, `tmux_paste`, `split`, `loadConf`, `PREFS`, `activateMtab`, `restoreSplitLeft`, `applyAppLayout`, `initOpChatSplit`) van con el `grep`/línea exacta para localizarlos y la instrucción de extraer la función si el handler la tiene inline.
- **Consistencia de tipos:** `Dispatcher.run` devuelve siempre `{ok, reply, actions, data}` (Task 2) y es lo que consumen `operator_agent_stream` (4), `_run_tool_batch` (10) y los handlers locales normalizados con `setdefault` (5). Eventos SSE (`delta`, `tool`, `tool_result`, `action`, `error`, `final`, `payload`) idénticos en productor (4), endpoint y frontend (6) y paralelismo (10). Acciones `{"type":"ui","op":"click|call|term"}` producidas en 2 y consumidas en 7; `postMessage {source:"comandos", type:"toolbar", term:{...}}` producido en 7 y consumido en 8; `app-command.json` `{command,args,ts}` producido en 2 y consumido en 9; `APP_COMMAND_NAMES`/`APP_TOOL_BY_COMMAND` definidos en el catálogo (Parte 2) y usados en 9.
