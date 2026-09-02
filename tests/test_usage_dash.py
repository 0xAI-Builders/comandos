#!/usr/bin/env python3
import importlib.machinery
import importlib.util
import json
import sys
from pathlib import Path
from types import SimpleNamespace


SRC = Path("bin/cc-dash").read_text()


def load_dash_module():
    bin_dir = str(Path("bin").resolve())
    if bin_dir not in sys.path:
        sys.path.insert(0, bin_dir)
    loader = importlib.machinery.SourceFileLoader("cc_dash_under_test", str(Path("bin/cc-dash").resolve()))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


def test_cc_dash_imports_usage_module():
    assert "import cc_usage" in SRC


def test_usage_state_endpoint_exists_and_is_authenticated():
    assert '"/usage/state"' in SRC
    api_get = SRC.split("API_GET = ", 1)[1].split("def do_GET", 1)[0]
    assert '"/usage/state"' in api_get


def test_combination_analytics_and_rating_endpoints_are_authenticated():
    assert '"/usage/analytics"' in SRC
    assert '"/usage/interactions"' in SRC
    api_get = SRC.split("API_GET = ", 1)[1].split("def do_GET", 1)[0]
    assert '"/usage/analytics"' in api_get
    assert '"/usage/interactions"' in api_get
    assert '"/usage/experiments"' in api_get
    assert 'self.path == "/usage/rating"' in SRC
    assert '"/usage/experiments"' in SRC
    assert 'self.path == "/usage/experiment"' in SRC
    assert "cc_usage.create_experiment" in SRC and "cc_usage.create_experiment_pair" in SRC
    assert "cc_usage.experiment_analytics" in SRC
    assert "cc_usage.set_interaction_feedback" in SRC
    assert "cc_usage.set_interaction_task" in SRC


def test_usage_live_panes_records_pane_pwd_and_git_root():
    assert "def usage_live_panes" in SRC
    assert "cc_usage.normalize_pane_identity" in SRC
    assert "cc_usage.git_root_for_path" in SRC
    assert "cc_usage.record_pane" in SRC


def test_usage_capture_and_refresh_endpoints_exist():
    assert 'self.path == "/usage/capture"' in SRC
    assert 'self.path == "/usage/refresh"' in SRC
    assert 'self.path == "/usage/settings"' in SRC
    assert "write_usage_settings" in SRC
    assert "usage_runtime_env" in SRC
    assert "record_local_codex_threads" in SRC
    assert "record_local_claude_jsonl" in SRC
    assert "usage_credential_health" in SRC
    assert 'state["credential_health"] = usage_credential_health(env)' in SRC


def test_cards_reconcile_harness_and_model_from_live_pane_and_confirmed_config():
    assert '"agent": live_agent or d.get("agent") or "claude"' in SRC
    assert "def reconcile_card_config" in SRC
    assert "claude_pane_model(pane)" in SRC
    assert "cc_usage.latest_session_config" in SRC
    assert 'item["modelSource"] = "pane"' in SRC
    html = Path("dash/index.html").read_text()
    assert "harnessLabel" in html and "→ ${mdEsc(engineLabel(motor))}" in html


def test_effort_only_switch_keeps_current_pane_model():
    # Cambiar SOLO el esfuerzo (model vacío + routeId) NO debe saltar al modelo
    # default del registro ni vaciar el modelo confirmado: usa el modelo que ya
    # corre el pane, no teclea /model, y lo conserva para el registro.
    assert 'if not selection_data.get("model") and scope == "session_model" and current_item.get("model"):' in SRC
    assert 'selection_data["model"] = current_item.get("model")' in SRC
    assert "effort_only = True" in SRC
    assert "if (model and not effort_only) or not effort:" in SRC
    # session-effort: al fijar clave pane-level, se limpia la de sesión huérfana
    assert 'd.pop(sess.split("|", 1)[0], None)' in SRC


def test_motor_switch_auto_picks_a_logged_in_account():
    # Cambiar de MOTOR no debe heredar la cuenta del harness para un motor que
    # no la tiene (causa de motor_account_login_required). Se resuelve contra el
    # motor DESTINO y cae a una cuenta con login.
    assert "def _pick_motor_account" in SRC
    assert 'if not data.get("motorAccount"):' in SRC
    assert "_pick_motor_account(requested_motor, pref)" in SRC
    # el error se traduce a un mensaje humano, no el código crudo
    assert "no tiene login en la cuenta" in SRC
    mod = load_dash_module()
    # _pick_motor_account devuelve un alias con login (o el preferido si falla)
    assert callable(mod._pick_motor_account)


def test_global_optimization_profiles_apply_through_real_switch_endpoint():
    html = Path("dash/index.html").read_text()
    assert 'data-mtab="optimizar"' in html
    assert 'api("/optimization/default"' in html
    assert 'api("/model/switch"' in html
    assert "Confirmar ${keys.length} cambios" in html
    assert '"/optimization/plans"' in SRC


def test_external_motor_switch_locks_all_subagent_slots_and_is_recoverable():
    assert "def motor_lock_env" in SRC
    for key in ("ANTHROPIC_DEFAULT_OPUS_MODEL", "ANTHROPIC_DEFAULT_SONNET_MODEL", "ANTHROPIC_DEFAULT_HAIKU_MODEL", "CLAUDE_CODE_SUBAGENT_MODEL"):
        assert key in SRC
    assert '"kind":"restart"' in SRC
    assert "resume_restart_payload(payload)" in SRC
    assert 'source="switch-restart-confirmed"' not in SRC  # source is positional, not a dead keyword
    assert "def _restore_shell_tty" in SRC
    assert "def _paste_shell_command" in SRC
    assert 'motor_result_set(sess, False, "Claude salió, pero el resume no arrancó; usa Reintentar")' in SRC
    assert 'candidate.get("agent") == "claude"' in SRC


def test_motor_switch_state_survives_dashboard_restart():
    assert "MOTOR_RESULT_FILE" in SRC
    assert "MOTOR_RESULT = _motor_result_load()" in SRC
    resume = SRC.split("def motor_queue_resume", 1)[1].split("def _set_motor_result", 1)[0]
    assert 'payload["key"] = opkey' in resume
    assert 'payload["sess"] = payload.get("sess") or opkey.partition("|")[0]' in resume
    assert 'motor_stage(opkey, "recuperando cambio después del reinicio"' in resume


def test_confirmed_switch_beats_stale_launch_model_in_cards():
    reconcile = SRC.split("def reconcile_card_config",1)[1].split("def read_states",1)[0]
    assert reconcile.index('config = cc_usage.latest_session_config') < reconcile.index('elif launch_model:')
    assert 'item["modelSource"] = "confirmed"' in reconcile


def test_model_status_uses_operation_specific_fast_polling():
    assert '"/model/status"' in SRC.split("API_GET =", 1)[1].split("def do_GET", 1)[0]
    assert 'self.path.startswith("/model/status")' in SRC
    html = Path("dash/index.html").read_text()
    assert 'api(`/model/status?operationKey=' in html
    assert "},450);" in html
    assert "time.sleep(3.0)" not in SRC.split("def motor_apply", 1)[1].split("def motor_queue_resume", 1)[0]
    assert '"queuedMs"' not in SRC  # camelCase is emitted as a keyword, not a quoted fake UI value
    assert "queuedMs=max(0" in SRC and "applyMs=max(0" in SRC


def test_model_switch_endpoint_targets_requested_pane():
    assert 'self.path == "/model/switch"' in SRC
    assert "cc_usage.model_switch_text" in SRC
    assert 'tmux("send-keys", "-t", pane, "-l", "--", c)' in SRC  # cada comando (/model, /effort) va al pane pedido
    assert "claude_pane_busy(pane)" in SRC  # y nunca a mitad de turno


def test_model_switch_accepts_direct_model():
    assert 'data.get("model")' in SRC
    assert "cc_usage.model_switch_text(provider, preset, model)" in SRC


def test_usage_state_wires_exact_provider_limits():
    assert "def usage_provider_limits" in SRC
    assert "cc_usage.fetch_claude_oauth_limits" in SRC
    assert "cc_usage.read_codex_rate_limits" in SRC
    # El fetch OAuth corre en hilo aparte: /usage/state nunca se bloquea por red
    assert "threading.Thread" in SRC
    assert "limits=" in SRC


def test_codex_dropdown_drives_numbered_picker():
    # El picker /model de codex es de dos pasos numerados: modelo → reasoning
    assert "CODEX_MODEL_DIGITS" in SRC
    assert "CODEX_EFFORT_DIGITS" in SRC
    assert '"gpt-5.5": "1"' in SRC
    assert '"gpt-5.6-sol": "2"' in SRC   # picker 0.144.x reordenado
    assert '"xhigh": "4"' in SRC
    assert 'data.get("effort")' in SRC


def test_opencode_models_endpoint_and_picker_automation():
    assert '"/opencode/models"' in SRC
    assert "def opencode_models" in SRC
    assert "cc_usage.parse_opencode_models" in SRC
    assert 'provider == "opencode"' in SRC
    assert "cc_usage.opencode_picker_query" in SRC


def test_pane_models_file_for_tmux_borders():
    assert "def write_pane_models" in SRC
    assert "pane-models.txt" in SRC


def test_pane_border_uses_tmux_option_not_per_pane_subprocess():
    # Con 50 panes, un #(cc-pane-model) por pane cada status-interval causaba
    # lag al teclear. Ahora cc-dash setea la opcion @ccmodel (en un hilo) y el
    # borde la lee con #{E:@ccmodel} — cero spawns por pane.
    assert '"@ccmodel"' in SRC
    assert '"set-option", "-p"' in SRC
    assert "def _pane_model_values" in SRC
    assert "_pane_model_state" in SRC
    assert "threading.Thread" in SRC
    conf = Path("config/tmux.conf").read_text()
    # Los titulos de TEXTO en el borde se eliminaron a proposito: el renglon
    # queda reservado VACIO como riel de las pildoras por pane de cc-app.
    # @ccmodel se sigue escribiendo (tooling externo / estados deterministas).
    assert "pane-border-status top" in conf
    assert "pane-border-format ' '" in conf
    assert "#{E:@ccmodel}" not in conf
    assert "cc-pane-model" not in conf


def test_pane_model_values_normalize_labels_and_preserve_clears():
    dash = load_dash_module()

    values, plain = dash._pane_model_values([
        {"tmux_pane": "%1", "agent": "claude", "model": "claude-sonnet"},
        {"tmux_pane": "%2", "provider": "codex", "model": "openai/gpt-5.6"},
        {"tmux_pane": "%3", "agent": "", "model": ""},
        {"tmux_pane": "not-a-pane", "agent": "codex", "model": "ignored"},
    ])

    assert values == {
        "%1": "#[fg=colour141,bold]▸ claude#[default]#[fg=colour179,bold] · sonnet $$#[default]",
        "%2": "#[fg=colour43,bold]▸ codex#[default]#[fg=colour203,bold] · gpt-5.6 $$$#[default]",
        "%3": None,
    }
    assert plain == "%1 claude · sonnet $$\n%2 codex · gpt-5.6 $$$\n"


def test_context_shrink_warning_fires_only_when_window_gets_smaller():
    dash = load_dash_module()
    dash.read_states_cached = lambda ttl=1.2: [
        {"session": "s", "pane": "%1", "model": "claude-fable-5[1m]", "contextPct": 45.0}]
    warn = dash.context_shrink_warning("s", "%1", "claude-opus-5")
    assert "compactará en ráfaga" in warn and "1000k" in warn   # 45% de 1M → 225% de 200k
    assert dash.context_shrink_warning("s", "%1", "claude-fable-5[1m]") == ""   # misma ventana
    dash.read_states_cached = lambda ttl=1.2: [
        {"session": "s", "pane": "%1", "model": "claude-opus-5", "contextPct": 45.0}]
    assert dash.context_shrink_warning("s", "%1", "claude-fable-5[1m]") == ""   # crecer nunca advierte


def test_pane_suggestions_are_deterministic_with_one_executable_action():
    dash = load_dash_module()
    guard = {"forecasts": [{"scope": "Fable", "level": "critical", "downtimeHours": 28.0}],
             "projects": [{"project": "mrp", "level": "warning", "calls10m": 80, "tokensHour": 45_000_000}]}
    routes = {"claude:codex", "codex:codex", "grok:grok"}

    hot = {"alive": True, "agent": "claude", "session": "s", "pane": "%1", "contextPct": 12,
           "model": "claude-fable-5[1m]", "project": "otro", "cwd": "/x/otro"}
    dash._annotate_suggestion(hot, guard, routes)
    assert hot["suggestion"]["kind"] == "switch"
    assert hot["suggestion"]["model"] == "gpt-5.6-luna"
    assert "28h sin Fable" in hot["suggestion"]["text"].replace("~", "")

    ctx = {"alive": True, "agent": "codex", "session": "s", "pane": "%2", "contextPct": 86, "model": "gpt-5.6-sol"}
    dash._annotate_suggestion(ctx, guard, routes)
    assert ctx["suggestion"] == {"kind": "send", "icon": "zap", "command": "/compact",
                                 "text": ctx["suggestion"]["text"], "button": "Compactar ahora"}

    cheap = {"alive": True, "agent": "grok", "session": "s", "pane": "%3", "contextPct": 10, "model": "grok-4.5"}
    dash._annotate_suggestion(cheap, guard, routes)
    assert "suggestion" not in cheap   # ya es barato: no molestar

    import time as _t
    stuck = {"alive": True, "agent": "claude", "session": "s", "pane": "%4", "status": "working",
             "ts": _t.time() - 20 * 60, "project": "sin-llamadas", "model": "claude-opus-5"}
    dash._annotate_suggestion(stuck, guard, routes)
    assert stuck["suggestion"]["kind"] == "key" and stuck["suggestion"]["key"] == "Escape"
    assert "parece colgado" in stuck["suggestion"]["text"]

    looping = {"alive": True, "agent": "claude", "session": "s", "pane": "%5", "status": "working",
               "ts": _t.time() - 25 * 60, "project": "mrp", "model": "claude-opus-5"}
    guard_loop = dict(guard, projects=[{"project": "mrp", "level": "critical", "calls10m": 120, "tokensHour": 9}])
    dash._annotate_suggestion(looping, guard_loop, routes)
    assert "Posible loop: 120 llamadas" in looping["suggestion"]["text"]

    html = Path("dash/index.html").read_text()
    assert "cx-suggest" in html and 'class="sg-go"' in html


def test_defensive_wizard_never_creates_shell_from_unavailable_route():
    assert '"routeId" in data and not data.get("routeId")' in SRC
    assert '"code": "route_unavailable"' in SRC
    html = Path("dash/index.html").read_text()
    assert 'go.disabled=!shell&&!NS.routeId' in html
    assert 'class="sw" id="ns-danger" role="switch" aria-checked="false"' in html


def test_change_ledger_records_switches_and_offers_undo():
    assert '"/usage/changes"' in SRC.split("API_GET =", 1)[1].split("def do_GET", 1)[0]
    assert "cc_usage.record_change" in SRC
    assert "cc_usage.change_ledger" in SRC
    html = Path("dash/index.html").read_text()
    assert 'id="guard-ledger"' in html
    assert "data-undo=" in html
    assert '${target&&target.selectable?"":"disabled"}' in html  # Optimizar: nada preseleccionado


def test_pane_border_shows_deterministic_switching_and_detecting_states():
    dash = load_dash_module()
    dash.MOTOR_RESULT["term-x|%9"] = {"stage": "tecleando /model", "model": "gpt-5.6-luna", "ts": __import__("time").time()}
    values, plain = dash._pane_model_values([
        {"tmux_pane": "%9", "tmux_session": "term-x", "agent": "claude", "model": "claude-fable-5"},
        {"tmux_pane": "%10", "tmux_session": "term-y", "agent": "grok", "model": ""},
    ])
    assert "cambiando → gpt-5.6-luna" in values["%9"]
    assert "detectando…" in values["%10"]
    assert "%9 claude · cambiando → gpt-5.6-luna" in plain
    assert "%10 grok · detectando…" in plain


def test_alert_rules_endpoint_and_evaluation():
    assert '"/usage/alert-rule"' in SRC
    assert "cc_usage.set_alert_rule" in SRC
    assert "cc_usage.delete_alert_rule" in SRC
    assert "cc_usage.rule_current_values" in SRC
    assert "cc_usage.rule_alerts" in SRC
    assert 'state["alert_rules"]' in SRC


def test_limit_alerts_notify_by_desktop_and_telegram():
    assert "def usage_alert_send" in SRC
    # Popups PROPIOS de ComandOS (cc-notifyd), jamas notify-send
    assert "127.0.0.1:4778/notify" in SRC
    assert "notify-send" not in SRC
    assert "TELEGRAM_ENABLED" in SRC
    assert "cc_usage.limit_threshold_alerts" in SRC
    assert "cc_usage.record_alert_once" in SRC
    assert 'state["alerts"] = cc_usage.list_alerts(USAGE_DB)' in SRC


def test_new_sessions_load_provider_keys_env():
    # Las sesiones nuevas cargan ~/.claude/hooks/providers.env (keys de
    # groq/cerebras/sambanova/cloudflare para opencode y amigos)
    assert "providers.env" in SRC
    assert 'data.get("agent")' in SRC


def test_tab_close_endpoint_delegates_with_explicit_ephemeral_flag():
    assert '"/tab-close"' in SRC
    assert "app-tab-close.json" in SRC
    body = SRC.split('"/tab-close"', 1)[1].split('self.path ==', 1)[0]
    assert 'data.get("ephemeral") is True' in body
    assert "close_app_tab(sess, ephemeral=ephemeral)" in body


def test_ephemeral_tab_close_skips_history_and_rejects_non_e2e_names(tmp_path, monkeypatch):
    dash = load_dash_module()
    tabs_file = tmp_path / "app-tabs.json"
    history_file = tmp_path / "app-tabs-history.json"
    tabs_file.write_text(json.dumps({
        "comandos-e2e-4242": "e2e",
        "term-user": "user",
    }))
    history_file.write_text(json.dumps([{"session": "existing", "ts": 1}]))
    monkeypatch.setattr(dash, "HOOKS", str(tmp_path))
    monkeypatch.setattr(dash, "TABS_FILE", str(tabs_file))
    monkeypatch.setattr(dash, "TAB_HISTORY_FILE", str(history_file))
    monkeypatch.setattr(dash, "tmux", lambda *_args, **_kwargs: (_ for _ in ()).throw(
        AssertionError("ephemeral close must not inspect tmux")
    ))

    assert dash.close_app_tab("comandos-e2e-4242", ephemeral=True) is None
    assert json.loads(tabs_file.read_text()) == {"term-user": "user"}
    assert json.loads(history_file.read_text()) == [{"session": "existing", "ts": 1}]
    assert json.loads((tmp_path / "app-tab-close.json").read_text())["session"] == "comandos-e2e-4242"

    before_tabs = tabs_file.read_text()
    before_history = history_file.read_text()
    assert dash.close_app_tab("term-user", ephemeral=True) == "ephemeral requiere comandos-e2e-"
    assert tabs_file.read_text() == before_tabs
    assert history_file.read_text() == before_history


def test_normal_tab_close_still_records_recents(tmp_path, monkeypatch):
    dash = load_dash_module()
    tabs_file = tmp_path / "app-tabs.json"
    history_file = tmp_path / "app-tabs-history.json"
    tabs_file.write_text(json.dumps({"term-user": "User tab"}))
    history_file.write_text("[]")
    monkeypatch.setattr(dash, "HOOKS", str(tmp_path))
    monkeypatch.setattr(dash, "TABS_FILE", str(tabs_file))
    monkeypatch.setattr(dash, "TAB_HISTORY_FILE", str(history_file))
    monkeypatch.setattr(dash, "session_labels", lambda: {"term-user": "User tab"})
    monkeypatch.setattr(dash, "state_agent", lambda _session: "codex")
    monkeypatch.setattr(dash, "tmux", lambda *_args, **_kwargs: SimpleNamespace(
        returncode=0, stdout="/tmp/project\n", stderr=""
    ))

    assert dash.close_app_tab("term-user", ephemeral=False) is None
    history = json.loads(history_file.read_text())
    assert isinstance(history[0]["ts"], int)
    assert history[0] == {
        "session": "term-user",
        "label": "User tab",
        "cwd": "/tmp/project",
        "agent": "codex",
        "reason": "closed",
        "ts": history[0]["ts"],
    }
    assert json.loads(tabs_file.read_text()) == {}


def test_usage_state_is_cached_and_refresh_is_backgrounded():
    assert "def cached_usage_state" in SRC
    assert "def read_states_cached" in SRC
    assert "_usage_state_cache" in SRC
    assert "threading.Thread(target=_bg" in SRC   # refresh local no bloquea
    assert "prune_old_turns" in SRC


def test_agent_pane_maps_keeps_one_agent_per_tmux_pane():
    dash = load_dash_module()

    class R:
        returncode = 0
        stdout = "term-1|%1|100\n"

    dash.tmux = lambda *args, **kwargs: R()
    parents = {300: 200, 200: 100, 400: 100}
    dash.parent_pid = lambda pid: parents.get(pid, 0)

    _by_session, by_cwd = dash.agent_pane_maps([
        (300, "/repo", "claude"),
        (400, "/repo", "codex"),
    ])

    assert len(by_cwd["/repo"]) == 1
    assert by_cwd["/repo"][0]["agent"] == "codex"
    assert by_cwd["/repo"][0]["pane"] == "%1"


if __name__ == "__main__":
    test_pane_border_uses_tmux_option_not_per_pane_subprocess()
    test_usage_state_is_cached_and_refresh_is_backgrounded()
    test_tab_close_endpoint_delegates_with_explicit_ephemeral_flag()
    test_alert_rules_endpoint_and_evaluation()
    test_codex_dropdown_drives_numbered_picker()
    test_limit_alerts_notify_by_desktop_and_telegram()
    test_cc_dash_imports_usage_module()
    test_usage_state_endpoint_exists_and_is_authenticated()
    test_usage_live_panes_records_pane_pwd_and_git_root()
    test_usage_capture_and_refresh_endpoints_exist()
    test_model_switch_endpoint_targets_requested_pane()
    test_model_switch_accepts_direct_model()
    test_usage_state_wires_exact_provider_limits()
    test_opencode_models_endpoint_and_picker_automation()
    test_pane_models_file_for_tmux_borders()
    test_new_sessions_load_provider_keys_env()
    test_agent_pane_maps_keeps_one_agent_per_tmux_pane()


def test_active_tab_endpoint_enriches_session_with_active_tmux_pane():
    assert 'if self.path.startswith("/active-tab")' in SRC
    assert '"#{pane_id}"' in SRC
    assert 'active["pane"] = pane_a' in SRC


def test_state_emits_one_control_card_per_live_claude_split():
    assert "def _claude_all_panes" in SRC
    assert '"pane": pane_l' in SRC
    assert '"split": True' in SRC
    assert "by_sess_count" in SRC


def test_providers_endpoint_is_security_gated_and_uses_sanitized_registry():
    assert '"/providers"' in SRC.split("API_GET =", 1)[1].split("def do_GET", 1)[0]
    assert "provider_registry.public_state(registry)" in SRC
    assert "account_registry.public_accounts(registry)" in SRC


def test_model_confirmation_handles_cross_provider_dialog_and_requires_new_marker(monkeypatch):
    dash = load_dash_module()
    captures = iter([
        "old\n❯ /model grok-4.6\nSwitch model?\n1. Yes, switch to grok-4.6\n2. No",
        "old\n❯ /model grok-4.6\nSet model to grok-4.6 and saved as your default",
    ])
    sent = []

    def fake_tmux(*args, **_kwargs):
        if args[0] == "capture-pane":
            return SimpleNamespace(returncode=0, stdout=next(captures), stderr="")
        sent.append(args)
        return SimpleNamespace(returncode=0, stdout="", stderr="")

    monkeypatch.setattr(dash, "tmux", fake_tmux)
    monkeypatch.setattr(dash.time, "sleep", lambda _n: None)
    ok, error = dash.claude_command_confirm("%1", "/model grok-4.6", "old", 2)
    assert (ok, error) == (True, "")
    assert ("send-keys", "-t", "%1", "1") in sent
    assert ("send-keys", "-t", "%1", "Enter") in sent


def test_model_confirmation_does_not_accept_stale_marker(monkeypatch):
    dash = load_dash_module()
    stale = "Set model to gpt-5.6-sol\n❯ /model grok-4.6\nSwitching..."
    monkeypatch.setattr(
        dash, "tmux",
        lambda *_args, **_kwargs: SimpleNamespace(returncode=0, stdout=stale, stderr=""),
    )
    monkeypatch.setattr(dash.time, "sleep", lambda _n: None)
    assert dash.claude_command_confirm("%1", "/model grok-4.6", stale, 1) == (False, "")


def test_native_codex_switch_reports_pane_keyed_confirmed_result():
    assert "def confirm_codex_native" in SRC
    assert "def codex_pane_model" in SRC
    assert 'operationKey": opkey' in SRC
    assert 'confirmando modelo y esfuerzo en Codex CLI' in SRC


def test_session_new_and_model_switch_accept_canonical_route_ids():
    assert 'route, selected = resolve_route_selection(data, "new_session")' in SRC
    assert 'routeId": route.get("id") if route else "shell"' in SRC
    assert 'if data.get("routeId"):' in SRC
    assert 'scope = "session_model" if requested.get("motor") == current_motor else "session_motor"' in SRC
    assert 'current_harness=current_harness' in SRC


def test_user_quota_is_persisted_validated_and_feeds_grok_percent(tmp_path, monkeypatch):
    # La cuota Grok la declara el USUARIO (xAI no expone limites por API):
    # se guarda local, solo providers permitidos, y 0 la borra.
    dash = load_dash_module()
    monkeypatch.setattr(dash, "QUOTAS_FILE", str(tmp_path / "provider-quotas.json"))
    saved = dash.set_user_quota("grok", 50_000_000)
    assert saved == {"tokens_7d": 50_000_000, "set_at": saved["set_at"]}
    assert dash.user_quotas()["grok"]["tokens_7d"] == 50_000_000
    dash.set_user_quota("grok", 0)
    assert "grok" not in dash.user_quotas()
    try:
        dash.set_user_quota("claude", 1)   # claude tiene limites REALES: no aplica
        raise AssertionError("provider no permitido acepto cuota")
    except ValueError:
        pass
    # El endpoint existe y refresca limits al guardar
    assert '"/usage/quota"' in SRC
    assert "usage_provider_limits(force=True)" in SRC.split('"/usage/quota"', 1)[1][:600]


def test_user_subscription_costs_are_validated_and_currency_aware(tmp_path, monkeypatch):
    dash = load_dash_module()
    monkeypatch.setattr(dash, "SUBS_FILE", str(tmp_path / "provider-subs.json"))
    data = dash.set_user_sub("claude", 200, "USD", None)
    assert data["subs"]["claude"] == {"monthly": 200, "currency": "USD"}
    data = dash.set_user_sub("", None, None, {"currency": "MXN", "usdRate": 18.5})
    assert data["display"] == {"currency": "MXN", "usdRate": 18.5}
    data = dash.set_user_sub("claude", 0, "USD", None)   # 0 borra
    assert "claude" not in data["subs"]
    for bad in (("openai", 10, "USD"), ("claude", 10, "BTC")):
        try:
            dash.set_user_sub(*bad, None)
            raise AssertionError(f"acepto {bad}")
        except ValueError:
            pass
    assert '"/usage/subscription"' in SRC


def test_fs_browser_is_home_confined_and_mkdir_is_explicit(tmp_path, monkeypatch):
    # El navegador de carpetas del wizard: solo NOMBRES de directorios bajo
    # $HOME; mkdir jamas sale de HOME ni pisa archivos.
    dash = load_dash_module()
    monkeypatch.setattr(dash, "FS_HOME", str(tmp_path))
    (tmp_path / "codebase" / "proyecto").mkdir(parents=True)
    (tmp_path / "codebase" / ".oculta").mkdir()
    (tmp_path / "codebase" / "archivo.txt").write_text("x")

    d = dash.fs_dirs(str(tmp_path / "codebase"))
    assert d["exists"] is True
    names = [x["name"] for x in d["dirs"]]
    assert "proyecto" in names and ".oculta" not in names and "archivo.txt" not in names

    # path inexistente: reporta lo que falta y lista el ancestro con filtro
    d2 = dash.fs_dirs(str(tmp_path / "codebase" / "pro"))
    assert d2["exists"] is False and d2["missing"] == "pro"
    assert [x["name"] for x in d2["dirs"]] == ["proyecto"]

    # fuera de HOME: rechazado
    assert "error" in dash.fs_dirs("/etc")

    created = dash.fs_mkdir(str(tmp_path / "nueva" / "sub"))
    assert created == str(tmp_path / "nueva" / "sub") and (tmp_path / "nueva" / "sub").is_dir()
    for bad in ("/etc/x", str(tmp_path / "codebase" / "archivo.txt")):
        try:
            dash.fs_mkdir(bad)
            raise AssertionError(f"mkdir acepto {bad}")
        except ValueError:
            pass
    # session-new distingue carpeta faltante (409 accionable, no 400 generico)
    assert '"code": "cwd_missing"' in SRC


def test_harness_handoff_switch_exists_and_is_guarded():
    # Cambio de HARNESS (el CLI) en vivo con handoff: endpoint + orquestación +
    # captura de contexto SIN secretos + comando de lanzamiento por harness.
    assert 'if self.path == "/harness/switch":' in SRC
    assert "def capture_handoff" in SRC
    assert "def harness_switch_apply" in SRC
    assert "def _harness_launch_cmd" in SRC
    # espera idle (no cambiar a mitad de un tool call) y captura ANTES de cerrar
    assert '"wait_idle"' in SRC and '"capture"' in SRC
    # el handoff es un archivo efímero en el cwd
    assert 'HANDOFF_FILE = ".comandos-handoff.md"' in SRC
    # rechaza harness inválido y el mismo harness
    assert "harness desconocido" in SRC
    assert "el pane ya corre" in SRC
    mod = load_dash_module()
    # comandos de lanzamiento por harness (misma forma que /session-new)
    assert "claude" in mod._harness_launch_cmd("claude", "claude-fable-5[1m]", "high", "main")
    assert "codex" in mod._harness_launch_cmd("codex", "gpt-5.6-luna", "low", "main")
    assert "grok" in mod._harness_launch_cmd("grok", "grok-4.6", "high", "main")


def test_harness_switch_ui_section_present():
    html = Path("dash/index.html").read_text()
    assert "Cambiar de CLI (harness)" in html
    assert 'data-harness=' in html
    assert 'api("/harness/switch"' in html
    assert "MPOP.harnessArm" in html   # confirmación de dos toques
