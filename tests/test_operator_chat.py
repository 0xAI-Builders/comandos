#!/usr/bin/env python3
import importlib.util
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "operator_chat", ROOT / "lib" / "operator_chat.py")
assert SPEC and SPEC.loader
OP = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(OP)

TABS = [
    {"session": "term-1", "label": "Relotto"},
    {"session": "term-2", "label": "PaginasWeb"},
    {"session": "term-3", "label": "website"},
    {"session": "term-4", "label": "SAVA"},
]


def test_focus_latino_and_voseo_input():
    for phrase in ("pásate a Relotto", "pasate a relotto", "ve a Relotto",
                   "cambia a Relotto", "Relotto"):
        intent = OP.parse_intent(phrase, TABS)
        assert intent["action"] == "focus", phrase
        assert intent["session"] == "term-1"


def test_list_and_new_session_and_remember():
    assert OP.parse_intent("lista", TABS)["action"] == "list"
    assert OP.parse_intent("qué tabs hay", TABS)["action"] == "list"
    assert OP.parse_intent("sesión nueva", TABS)["action"] == "new_session"
    rem = OP.parse_intent("acuérdate: máximo 4 tabs", TABS)
    assert rem["action"] == "remember"
    assert "máximo 4 tabs" in rem["fact"] or "maximo 4 tabs" in OP.fold(rem["fact"])


def test_close_and_send_back():
    assert OP.parse_intent("cierra website", TABS)["session"] == "term-3"
    back = OP.parse_intent("manda website atrás", TABS)
    assert back["action"] == "send_back"
    assert back["session"] == "term-3"


def test_theme_and_split_tolerate_typos():
    theme = OP.parse_intent("cambiame el tema a brunio", TABS)
    assert theme["action"] == "ui"
    assert theme["target"] == "theme"
    assert theme["value"] == "bruno"
    assert OP.parse_intent("cambia el tema a bruno", TABS)["value"] == "bruno"
    assert OP.parse_intent("tema ubuntu", TABS)["value"] == "ubuntu"
    split = OP.parse_intent("abre un split a la derecha", TABS, active="term-2")
    assert split["action"] == "split" and split["side"] == "derecha"
    messy = OP.parse_intent("abr eun split a la derecha", TABS, active="term-1")
    assert messy["action"] == "split" and messy["side"] == "derecha"


def test_unknown_tab_is_unknown_action():
    intent = OP.parse_intent("pásate a Marte", TABS)
    assert intent["action"] == "unknown"
    assert "Marte" in str(intent.get("query") or "")


def test_replies_are_latino_not_voseo():
    blob = " ".join([OP.help_text(), OP.llm_system_prompt()])
    for banned in ("decile", "acordate", "mandá", "pasate a", "vos ", "tenés", "creá"):
        assert banned not in blob.lower()
    assert "sesión nueva" in blob.lower() or "sesion nueva" in OP.fold(blob)


def test_catalog_maps_chrome_and_session_buttons():
    cases = {
        "ajustes": ("ui", "settings"),
        "abre analytics": ("ui", "analytics"),
        "notificaciones": ("ui", "notif"),
        "pomodoro": ("ui", "pomo"),
        "snippets": ("ui", "snippets"),
        "remoto": ("ui", "remote"),
        "silencio": ("ui", "mute"),
        "sonido": ("ui", "unmute"),
        "buscador": ("ui", "switcher"),
        "soberanía": ("ui", "sov"),
        "servidores": ("ui", "servers"),
        "actividad": ("ui", "timeline"),
        "sesión nueva": ("new_session", None),
        "nueva conversación": ("new_chat", None),
        "siguiente": ("step", None),
        "anterior": ("step", None),
        "shell": ("toggle_shell", None),
        "cierra split": ("close_split", None),
        "copia respuesta": ("copy_reply", None),
        "ayuda": ("help", None),
    }
    for phrase, (action, target) in cases.items():
        intent = OP.parse_intent(phrase, TABS)
        assert intent["action"] == action, phrase
        if target:
            assert intent.get("target") == target, phrase
    theme = OP.parse_intent("tema ubuntu", TABS)
    assert theme["action"] == "ui" and theme["value"] == "ubuntu"
    remote = OP.parse_intent("remoto on", TABS)
    assert remote == {"action": "remote", "on": True}
    send = OP.parse_intent("envía a Relotto: hola", TABS)
    assert send["action"] == "send" and send["session"] == "term-1" and send["text"] == "hola"
    split = OP.parse_intent("split derecha", TABS, active="term-2")
    assert split["action"] == "split" and split["side"] == "derecha" and split["session"] == "term-2"
    kill = OP.parse_intent("mata website", TABS)
    assert kill["action"] == "kill" and kill["session"] == "term-3"
    pause = OP.parse_intent("pausa Relotto", TABS)
    assert pause["action"] == "pause" and pause["on"] is True
    resume = OP.parse_intent("sigue Relotto", TABS)
    assert resume["action"] == "pause" and resume["on"] is False
    key = OP.parse_intent("dale escape a SAVA", TABS)
    assert key["action"] == "key" and key["key"] == "Escape" and key["session"] == "term-4"
    export = OP.parse_intent("exporta respuesta de Relotto como pdf", TABS)
    assert export["action"] == "export" and export["fmt"] == "pdf" and export["session"] == "term-1"
    copy = OP.parse_intent("copia respuesta de website", TABS)
    assert copy["action"] == "copy_session" and copy["session"] == "term-3"
    rec = OP.parse_intent("recupera PaginasWeb", TABS)
    assert rec["action"] == "recover" and rec["query"] == "PaginasWeb"


def test_operator_models_are_cheap_fast_without_thinking():
    ids = [m["id"] for m in OP.MODELS]
    assert ids == ["haiku", "gpt-5.3-codex-spark", "grok-4.5"]
    assert "local" not in ids
    assert "grok-4.6" not in ids
    for item in OP.MODELS:
        assert item.get("effort") in ("", None)
        assert "high" not in str(item.get("effort") or "")
        assert "xhigh" not in str(item.get("effort") or "")
    assert OP.model_entry("local")["model"] == "haiku"
    assert OP.model_entry("grok-4.6")["model"] == "haiku"
    assert OP.model_entry("grok-4.5")["name"] == "Grok 4.5"


def test_store_persists_conversation_and_memory(tmp_path):
    root = tmp_path / "operator"
    store = OP.load_store(root)
    convo = OP.new_conversation(store)
    OP.append_message(convo, "user", "pásate a Relotto")
    OP.append_message(convo, "assistant", "Listo. Relotto.")
    OP.save_store(root, store)
    OP.append_memory(root, "máximo 4 tabs")
    again = OP.load_store(root)
    got = OP.get_conversation(again, convo["id"])
    assert got["messages"][0]["role"] == "user"
    assert "máximo 4 tabs" in OP.load_memory(root)
    pub = OP.public_store(again)
    assert pub["current"] == convo["id"]
    assert pub["conversations"][0]["n"] == 2
