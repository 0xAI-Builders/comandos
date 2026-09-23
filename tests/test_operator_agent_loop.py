# tests/test_operator_agent_loop.py
import ast
import io
import json
import sys
import types
import urllib.error
from pathlib import Path
import pytest

ROOT = Path(__file__).resolve().parents[1]
SRC = (ROOT / "bin" / "cc-dash").read_text()
sys.path.insert(0, str(ROOT / "lib"))
import operator_stream  # noqa: E402
import operator_catalog  # noqa: E402


def _load(names):
    """Ejecuta solo las defs pedidas de cc-dash en un namespace controlado."""
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


@pytest.mark.parametrize('model', ['haiku', 'grok-4.5'])
@pytest.mark.parametrize('names', [['session_status', 'session_usage'], {'session_status', 'session_usage'}])
def test_payload_filters_tool_catalog_without_changing_schemas(model, names):
    ns = _load({'operator_build_payload'})
    ns['OPERATOR_CREDS'] = types.SimpleNamespace(claude=lambda:'tok', grok=lambda:'key')
    ns['load_proxy_cfg'] = lambda: {'port':18765}
    family, payload, _, _ = ns['operator_build_payload'](model, 'SYS', [], tools=names)
    tools = payload['tools']
    actual = {t['name'] if family == 'anthropic' else t['function']['name'] for t in tools}
    assert actual == set(names)
    if family == 'anthropic':
        assert tools[-1]['cache_control'] == {'type':'ephemeral'}
    _, no_tools, _, _ = ns['operator_build_payload'](model, 'SYS', [], tools=False)
    assert 'tools' not in no_tools


@pytest.mark.parametrize('text,readonly', [('estado y consumo de esta sesión', True), ('abre pomodoro', False)])
def test_analysis_offers_only_readonly_tools_and_actions_keep_full_catalog(text, readonly):
    import operator_tools
    ns = _load({'operator_agent_stream', '_llm_poison'})
    ns['operator_tools'] = types.SimpleNamespace(agent_system_prompt=lambda *a, **k:'SYS',
                                                analysis_request=operator_tools.analysis_request)
    ns['operator_chat'] = _fake_chat()
    offered = []
    def provider(*args, tools=True):
        offered.append(tools)
        return 'anthropic', iter([{'t':'delta','text':'respuesta'},{'t':'done','stop':'end_turn'}])
    ns['operator_provider_stream'] = provider
    dispatcher = types.SimpleNamespace()
    list(ns['operator_agent_stream'](text, model='haiku', tabs=[], memory='', active='local',
                                    convo={'messages':[]}, dispatcher=dispatcher))
    assert dispatcher.readonly is readonly
    assert offered == [operator_catalog.READONLY if readonly else True]
    if readonly:
        assert {'session_status','session_usage','session_brain'} <= offered[0]
        assert not {'kill_session','configure_session','send_text'} & offered[0]


def test_agent_stream_runs_tool_then_final_text(tmp_path):
    ns = _load({"operator_agent_stream", "_llm_poison", "_run_tool_batch"})
    rounds = [
        [{"t": "delta", "text": "Voy."}, {"t": "tool_call", "id": "1", "name": "focus_tab", "input": {"tab": "Signara"}}, {"t": "done", "stop": "tool_use"}],
        [{"t": "delta", "text": "Listo, "}, {"t": "delta", "text": "enfocada."}, {"t": "done", "stop": "end_turn"}],
    ]
    ns["operator_provider_stream"] = lambda model, sys_txt, conv, tools=True: ("anthropic", iter(rounds.pop(0)))
    ns["operator_tools"] = types.SimpleNamespace(agent_system_prompt=lambda *a, **k: "SYS")
    ns["operator_chat"] = _fake_chat()
    ns["operator_store_root"] = lambda: tmp_path
    calls = []
    dispatcher = types.SimpleNamespace(run=lambda name, args: (calls.append((name, args)) or {"ok": True, "reply": "Foco en Signara", "actions": [{"type": "ui", "op": "click", "selector": "#x"}], "data": None}))
    events = list(ns["operator_agent_stream"]("enfoca signara", model="haiku", tabs=[], memory="", active=None, convo={"messages": []}, dispatcher=dispatcher))
    kinds = [e["t"] for e in events]
    assert kinds[:2] == ["delta", "tool"]
    assert {"t": "tool_result", "name": "focus_tab", "ok": True, "pending": False, "reply": "Foco en Signara"} in events
    assert calls == [("focus_tab", {"tab": "Signara"})]
    assert events[-1]["reply"] == "Listo, enfocada."
    action = events[-1]["actions"][0]
    assert action["selector"] == "#x" and action["actionId"]
    assert next(e for e in events if e["t"] == "action")["actionId"] == action["actionId"]


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


def test_agent_stream_stops_after_max_rounds():
    ns = _load({"operator_agent_stream", "_llm_poison", "_run_tool_batch"})
    ns["operator_provider_stream"] = lambda *a, **k: ("anthropic", iter([{"t": "tool_call", "id": "1", "name": "list_tabs", "input": {}}, {"t": "done", "stop": "tool_use"}]))
    ns["operator_tools"] = types.SimpleNamespace(agent_system_prompt=lambda *a, **k: "SYS")
    ns["operator_chat"] = _fake_chat()
    dispatcher = types.SimpleNamespace(run=lambda n, a: {"ok": True, "reply": "[]", "actions": [], "data": []})
    events = list(ns["operator_agent_stream"]("x", model="haiku", tabs=[], memory="", active=None, convo={"messages": []}, dispatcher=dispatcher, max_rounds=3))
    assert sum(1 for e in events if e["t"] == "tool") == 3 and events[-1]["t"] == "final"


@pytest.mark.parametrize('reply', ['', '  \n', 'HTTP Error 401: Unauthorized'])
@pytest.mark.parametrize('with_tool_result', [False, True])
def test_empty_or_contaminated_completion_never_invents_success(reply, with_tool_result):
    ns = _load({'operator_agent_stream', '_llm_poison', '_run_tool_batch'})
    ns['operator_tools'] = types.SimpleNamespace(agent_system_prompt=lambda *a, **k:'SYS')
    ns['operator_chat'] = _fake_chat()
    final_round = ([{'t':'delta','text':reply}] if reply else []) + [{'t':'done','stop':'end_turn'}]
    rounds = ([ [{'t':'tool_call','id':'status','name':'session_status','input':{}},
                 {'t':'done','stop':'tool_use'}] ] if with_tool_result else []) + [final_round]
    ns['operator_provider_stream'] = lambda *a, **k: ('anthropic', iter(rounds.pop(0)))
    calls = []
    def run(name, args):
        calls.append(name)
        return {'ok':True,'reply':'Estado observado: waiting','actions':[]}
    events = list(ns['operator_agent_stream']('estado',model='haiku',tabs=[],memory='',active='local',
        convo={'messages':[]},dispatcher=types.SimpleNamespace(run=run)))
    final = events[-1]
    assert calls == (['session_status'] if with_tool_result else [])
    assert 'Listo.' not in final['reply'] and not final['actions']
    if with_tool_result:
        assert 'Estado observado: waiting' in final['reply']
    if reply.strip() or not with_tool_result:
        assert final['ok'] is False
        assert any(event['t'] == 'error' and event['text'] == final['reply'] for event in events)
    else:
        assert final.get('ok', True) is True
        assert final['reply'] == 'Estado observado: waiting'


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


def test_provider_stream_retries_second_url_on_http_error():
    """open_stream is a generator; HTTPError on URL 1 must fire inside the retry try."""
    ns = _load({"operator_provider_stream", "_operator_http_error"})
    ns["urllib"] = __import__("urllib")
    ns["OPERATOR_TIMEOUT"] = 30
    invalidated = []
    ns["OPERATOR_CREDS"] = types.SimpleNamespace(invalidate=lambda: invalidated.append(True))
    urls = ("https://api.anthropic.com/v1/messages", "http://127.0.0.1:18765/v1/messages")
    ns["operator_build_payload"] = lambda *a, **k: ("anthropic", {}, {}, urls)
    tried = []

    def fake_open(url, payload, headers, timeout=30):
        tried.append(url)
        if url == urls[0]:
            raise urllib.error.HTTPError(
                url, 500, "err", hdrs=None, fp=io.BytesIO(b'{"error":{"message":"boom"}}'))
        yield b'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}\n'

    ns["operator_stream"] = types.SimpleNamespace(
        open_stream=fake_open,
        parse_sse=operator_stream.parse_sse,
        stream_anthropic=operator_stream.stream_anthropic,
        stream_openai=operator_stream.stream_openai,
        RETRYABLE=operator_stream.RETRYABLE,
    )
    family, events = ns["operator_provider_stream"]("haiku", "SYS", [{"role": "user", "content": "x"}])
    assert family == "anthropic" and tried == list(urls) and invalidated == []
    assert {"t": "delta", "text": "ok"} in list(events)


def test_quota_failure_remains_visible_after_all_fallbacks_fail():
    ns = _load({"operator_agent_stream", "_llm_poison"})
    ns["operator_chat"] = _fake_chat()
    ns["operator_tools"] = types.SimpleNamespace(agent_system_prompt=lambda *a, **k: "SYS")
    errors = {"haiku": "You're out of extra usage. Add more at claude.ai/settings/usage and keep going.",
              "gpt-5.3-codex-spark": "Gateway unavailable", "grok-4.5": "No hay login de Grok."}
    def fail(model, *args, **kwargs):
        raise RuntimeError(errors[model])
    ns["operator_provider_stream"] = fail
    events = list(ns["operator_agent_stream"]("estado", model="haiku", tabs=[], memory="", active="local",
                                              convo={"messages": []}, dispatcher=None))
    final = events[-1]
    assert final["ok"] is False
    assert "cuota agotada" in final["reply"].lower()
    assert "Gateway unavailable" in final["reply"] and "No hay login de Grok" in final["reply"]
    assert "Prueba otra vez" not in final["reply"]
    assert events[0]["text"] == final["reply"]


def test_fallback_names_actual_model_and_reuses_it_after_readonly_tool():
    import operator_chat
    ns = _load({'operator_agent_stream','_llm_poison','_run_tool_batch'})
    ns['operator_chat'] = operator_chat
    ns['operator_tools'] = types.SimpleNamespace(agent_system_prompt=lambda *a, **k:'SYS')
    calls = []
    def provider(model, *args, **kwargs):
        calls.append(model)
        if model == 'haiku':
            raise RuntimeError("You're out of extra usage.")
        events = ([{'t':'tool_call','id':'status','name':'session_status','input':{}}, {'t':'done','stop':'tool_use'}]
                  if len(calls) == 2 else [{'t':'delta','text':'Local está libre.'},{'t':'done','stop':'end_turn'}])
        return 'anthropic', iter(events)
    ns['operator_provider_stream'] = provider
    dispatcher = types.SimpleNamespace(run=lambda *args:{'ok':True,'reply':'idle','actions':[]})
    events = list(ns['operator_agent_stream']('estado',model='haiku',tabs=[],memory='',active='local',
                                             convo={'messages':[]},dispatcher=dispatcher))
    assert calls == ['haiku','gpt-5.3-codex-spark','gpt-5.3-codex-spark']
    notice = 'Respuesta de Codex Spark; Haiku no estuvo disponible.\n\n'
    assert sum(event.get('text') == notice for event in events) == 1
    assert events[-1]['reply'] == notice + 'Local está libre.'
    assert not events[-1]['actions']


def test_initial_sse_error_tries_fallback_before_exposing_output():
    ns = _load({"operator_provider_stream", "_operator_http_error"})
    ns.update(urllib=__import__("urllib"), OPERATOR_TIMEOUT=30)
    urls = ("https://provider.invalid", "http://fallback.invalid")
    ns["operator_build_payload"] = lambda *a, **k: ("anthropic", {}, {}, urls)
    tried = []
    def open_stream(url, *args, **kwargs):
        tried.append(url)
        if url == urls[0]:
            yield b'data: {"type":"error","error":{"message":"quota exhausted"}}\n'
        else:
            yield b'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}\n'
            yield b'data: {"type":"message_delta","delta":{"stop_reason":"end_turn"}}\n'
    ns["operator_stream"] = types.SimpleNamespace(**{name:getattr(operator_stream,name) for name in
        ("parse_sse", "stream_anthropic", "stream_openai", "RETRYABLE")}, open_stream=open_stream)
    _, events = ns["operator_provider_stream"]("haiku", "SYS", [])
    events = list(events)
    assert tried == list(urls)
    assert events == [{"t":"delta", "text":"ok"}, {"t":"done", "stop":"end_turn"}]


def test_cc_dash_registers_every_local_intent():
    import operator_dispatch as od
    body = SRC.split("def operator_build_dispatcher(", 1)[1].split("\ndef ", 1)[0]
    for intent in od.LOCAL_INTENTS:
        assert f'"{intent}":' in body, intent


def test_handle_chat_uses_dispatcher_not_legacy_callbacks():
    body = SRC.split("def operator_handle_chat(", 1)[1].split("\ndef ", 1)[0]
    assert "dispatcher=operator_build_dispatcher(active, selected_pane)" in body
    assert "focus=focus_session" not in body


def test_sse_frame_helper_formats_events():
    ns = _load({"operator_sse_frame"})
    assert ns["operator_sse_frame"]({"t": "delta", "text": "ho\nla"}) == b'data: {"t": "delta", "text": "ho\\nla"}\n\n'


def _optimizacion(errores):
    ns = _load({"operator_optimization_apply"})
    ns["optimization_plans"] = lambda: {"plans": [{"id": "ahorro"}]}
    ns["model_switch_for_session"] = lambda sess, plan: errores.get(sess)
    return ns["operator_optimization_apply"]


def test_optimizacion_con_sesiones_fallidas_no_es_exito():
    todo_mal = _optimizacion({"s1": "No hay sesión tmux 's1'"})("ahorro", ["s1"])
    assert todo_mal["ok"] is False and "No hay sesión" in todo_mal["reply"]
    parcial = _optimizacion({"s2": "HTTP 409"})("ahorro", ["s1", "s2"])
    assert parcial["ok"] is False and "1 de 2" in parcial["reply"] and "HTTP 409" in parcial["reply"]
    bien = _optimizacion({})("ahorro", ["s1", "s2"])
    assert bien.get("ok", True) is True
    assert _optimizacion({})("ahorro", [])["ok"] is False


def test_rondas_agotadas_con_una_herramienta_fallida_no_es_exito():
    ns = _load({"operator_agent_stream", "_llm_poison", "_run_tool_batch"})
    ronda = [0]
    def stream(*a, **k):
        ronda[0] += 1
        name = "model_switch" if ronda[0] == 1 else "list_tabs"
        return ("anthropic", iter([{"t": "tool_call", "id": str(ronda[0]), "name": name, "input": {}},
                                   {"t": "done", "stop": "tool_use"}]))
    ns["operator_provider_stream"] = stream
    ns["operator_tools"] = types.SimpleNamespace(agent_system_prompt=lambda *a, **k: "SYS")
    ns["operator_chat"] = _fake_chat()
    def run(name, args):
        if name == "model_switch":
            return {"ok": False, "reply": "No pude (model_switch): pane ocupado", "actions": []}
        return {"ok": True, "reply": "[]", "actions": []}
    events = list(ns["operator_agent_stream"]("x", model="haiku", tabs=[], memory="", active=None, convo={"messages": []},
                                              dispatcher=types.SimpleNamespace(run=run), max_rounds=2))
    final = events[-1]
    assert final["t"] == "final" and final["ok"] is False
    assert "pane ocupado" in final["reply"]
