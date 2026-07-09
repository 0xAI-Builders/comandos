# Usage Accounting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build pane-level Codex/Claude usage accounting with exact provider totals, local SQLite history, dashboard graphs, alerts, and pane-targeted model switching.

**Architecture:** Add a focused Python usage module next to the existing scripts, then wire it into `cc-dash` through small endpoints. The UI consumes `/usage/state` and renders a `Uso` drawer plus compact chips on cards/rows. Provider totals are exact only when imported from official APIs; pane/session rollups are exact only when local telemetry or provider identity supports that claim.

**Tech Stack:** Python 3 standard library (`sqlite3`, `urllib.request`, `json`, `subprocess`), tmux, existing `cc-dash` HTTP server, existing vanilla JS dashboard, existing shell hooks.

## Global Constraints

- The smallest local attribution unit is `tmux_session + tmux_pane + pane_pwd + git_root + agent`.
- `pane_pwd` must be resolved per tmux pane, not per session.
- Secrets must not be stored in SQLite or returned through dashboard APIs.
- No UI number may be marked exact unless it came from official provider API data or verified local per-pane telemetry.
- Provider fetches must use short timeouts and must not block `/state`.
- Remote UI may see metrics and credential health, never raw credential values.
- Tests must follow red-green-refactor: write failing test, run it, implement minimal code, run green test.

---

## File Structure

- Create `bin/cc_usage.py`: focused usage accounting module; owns SQLite schema, pane identity normalization, provider response parsing, usage summaries, alert calculation, and model preset helpers.
- Modify `bin/cc-dash`: imports `cc_usage`, exposes `/usage/state`, `/usage/events`, `/usage/projects`, `/usage/refresh`, `/usage/capture`, `/usage/settings`, and `/model/switch`; adds pane usage chips to `/state`.
- Modify `dash/index.html`: adds `Uso` button/drawer, graph rendering, usage state polling, project/pane rollups, alert list, credential health, usage chips on cards/rows, and model preset buttons.
- Modify `hooks/cc-notify.sh`: optionally forwards local turn start/finish metadata to the SQLite store through a safe helper call when available.
- Modify `adapters/codex-hooks.sh`: preserves Codex cwd/session state and prepares usage capture metadata.
- Create `tests/test_usage_core.py`: unit tests for SQLite schema, pane identity, provider parsers, summary, alerts, and exactness rules.
- Create `tests/test_usage_dash.py`: AST-level tests for `cc-dash` endpoint routing and model switch pane targeting.
- Create `tests/test_usage_ui.py`: HTML/JS structural tests for drawer, API calls, chips, and secret redaction.

---

### Task 1: Usage Core And SQLite Schema

**Files:**
- Create: `bin/cc_usage.py`
- Create: `tests/test_usage_core.py`

**Interfaces:**
- Produces: `usage_db_path(hooks_dir: str | None = None) -> str`
- Produces: `init_db(db_path: str) -> None`
- Produces: `git_root_for_path(path: str, run=None) -> str`
- Produces: `normalize_pane_identity(raw: dict, labels: dict | None = None, now: int | None = None) -> dict`
- Produces: `record_pane(db_path: str, pane: dict) -> None`
- Produces: `list_panes(db_path: str) -> list[dict]`

- [ ] **Step 1: Write failing tests**

Add `tests/test_usage_core.py` with:

```python
#!/usr/bin/env python3
import os
import sqlite3
import tempfile
from pathlib import Path

import importlib.util


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("cc_usage", ROOT / "bin" / "cc_usage.py")
cc_usage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cc_usage)


def test_usage_db_path_lives_under_hooks_dir():
    assert cc_usage.usage_db_path("/tmp/hooks") == "/tmp/hooks/comandos-usage.sqlite"


def test_init_db_creates_required_tables():
    with tempfile.TemporaryDirectory() as d:
        db = os.path.join(d, "usage.sqlite")
        cc_usage.init_db(db)
        con = sqlite3.connect(db)
        tables = {r[0] for r in con.execute("select name from sqlite_master where type='table'")}
        assert {
            "usage_panes",
            "usage_turns",
            "provider_usage_buckets",
            "provider_cost_buckets",
            "usage_reconciliation",
            "usage_alerts",
            "model_presets",
            "usage_settings",
        }.issubset(tables)


def test_git_root_for_path_uses_git_when_available_and_falls_back():
    calls = []

    def fake_run(args, cwd=None, timeout=3):
        calls.append((tuple(args), cwd))
        class R:
            returncode = 0
            stdout = "/repo\n"
            stderr = ""
        return R()

    assert cc_usage.git_root_for_path("/repo/app", run=fake_run) == "/repo"
    assert calls == [(("git", "rev-parse", "--show-toplevel"), "/repo/app")]

    def failing_run(args, cwd=None, timeout=3):
        class R:
            returncode = 1
            stdout = ""
            stderr = "not a repo"
        return R()

    assert cc_usage.git_root_for_path("/repo/app", run=failing_run) == "/repo/app"


def test_normalize_pane_identity_requires_pane_pwd_not_session_pwd():
    pane = cc_usage.normalize_pane_identity({
        "session": "term-123",
        "pane": "%18",
        "cwd": "/repo/frontend",
        "agent": "claude",
        "pid": 9001,
        "model": "sonnet",
    }, labels={"term-123": "Frontend"}, now=123)

    assert pane["tmux_session"] == "term-123"
    assert pane["tmux_pane"] == "%18"
    assert pane["pane_pwd"] == "/repo/frontend"
    assert pane["agent"] == "claude"
    assert pane["tab_label"] == "Frontend"
    assert pane["last_seen_at"] == 123


def test_record_and_list_panes_round_trip():
    with tempfile.TemporaryDirectory() as d:
        db = os.path.join(d, "usage.sqlite")
        cc_usage.init_db(db)
        pane = cc_usage.normalize_pane_identity({
            "session": "term-123",
            "pane": "%18",
            "cwd": "/repo/frontend",
            "agent": "codex",
            "pid": 111,
        }, now=123)
        cc_usage.record_pane(db, pane)
        rows = cc_usage.list_panes(db)

    assert len(rows) == 1
    assert rows[0]["tmux_session"] == "term-123"
    assert rows[0]["tmux_pane"] == "%18"
    assert rows[0]["pane_pwd"] == "/repo/frontend"
    assert rows[0]["agent"] == "codex"
```

- [ ] **Step 2: Run tests to verify RED**

Run: `python3 tests/test_usage_core.py`

Expected: FAIL with `FileNotFoundError` or missing `cc_usage.py`.

- [ ] **Step 3: Implement minimal module**

Create `bin/cc_usage.py` with schema creation, `git_root_for_path`, pane normalization, `record_pane`, and `list_panes`.

- [ ] **Step 4: Run tests to verify GREEN**

Run: `python3 tests/test_usage_core.py`

Expected: exit code 0.

- [ ] **Step 5: Commit**

```bash
git add bin/cc_usage.py tests/test_usage_core.py
git commit -m "Add usage accounting core"
```

---

### Task 2: Provider Parsers And Exactness Rules

**Files:**
- Modify: `bin/cc_usage.py`
- Modify: `tests/test_usage_core.py`

**Interfaces:**
- Produces: `parse_openai_usage_buckets(payload: dict) -> list[dict]`
- Produces: `parse_openai_cost_buckets(payload: dict) -> list[dict]`
- Produces: `parse_anthropic_usage_rows(payload: dict) -> list[dict]`
- Produces: `parse_anthropic_cost_rows(payload: dict) -> list[dict]`
- Produces: `record_provider_usage(db_path: str, provider: str, rows: list[dict]) -> None`
- Produces: `record_provider_costs(db_path: str, provider: str, rows: list[dict]) -> None`

- [ ] **Step 1: Write failing tests**

Append tests that verify:

```python
def test_parse_openai_usage_buckets_preserves_grouping_dimensions():
    payload = {
        "data": [{
            "start_time": 100,
            "end_time": 160,
            "results": [{
                "input_tokens": 1000,
                "output_tokens": 500,
                "input_cached_tokens": 250,
                "num_model_requests": 3,
                "project_id": "proj_1",
                "user_id": "user_1",
                "api_key_id": "key_1",
                "model": "gpt-test",
                "service_tier": "default",
            }]
        }],
        "has_more": False,
    }

    rows = cc_usage.parse_openai_usage_buckets(payload)

    assert rows == [{
        "provider": "openai",
        "start_time": 100,
        "end_time": 160,
        "input_tokens": 1000,
        "output_tokens": 500,
        "cache_read_tokens": 250,
        "cache_write_tokens": 0,
        "total_tokens": 1500,
        "request_count": 3,
        "project_id": "proj_1",
        "workspace_id": "",
        "user_id": "user_1",
        "api_key_id": "key_1",
        "model": "gpt-test",
        "service_tier": "default",
        "confidence": "exact",
    }]


def test_parse_openai_cost_buckets_preserves_amount_currency():
    payload = {"data": [{"start_time": 100, "end_time": 200, "results": [{
        "amount": {"value": 1.25, "currency": "usd"},
        "line_item": "Completions",
        "project_id": "proj_1",
        "api_key_id": "key_1",
    }]}]}

    rows = cc_usage.parse_openai_cost_buckets(payload)

    assert rows[0]["cost_usd"] == 1.25
    assert rows[0]["currency"] == "usd"
    assert rows[0]["project_id"] == "proj_1"
    assert rows[0]["api_key_id"] == "key_1"
    assert rows[0]["confidence"] == "exact"


def test_parse_anthropic_rows_accepts_current_and_generic_shapes():
    payload = {"data": [{"starting_at": "2026-07-09T00:00:00Z", "ending_at": "2026-07-10T00:00:00Z",
                         "workspace_id": "wrk_1", "model": "claude-sonnet",
                         "input_tokens": 10, "output_tokens": 5,
                         "cache_read_input_tokens": 3, "cache_creation_input_tokens": 2}]}

    rows = cc_usage.parse_anthropic_usage_rows(payload)

    assert rows[0]["provider"] == "anthropic"
    assert rows[0]["workspace_id"] == "wrk_1"
    assert rows[0]["model"] == "claude-sonnet"
    assert rows[0]["input_tokens"] == 10
    assert rows[0]["output_tokens"] == 5
    assert rows[0]["cache_read_tokens"] == 3
    assert rows[0]["cache_write_tokens"] == 2
    assert rows[0]["confidence"] == "exact"
```

- [ ] **Step 2: Run tests to verify RED**

Run: `python3 tests/test_usage_core.py`

Expected: FAIL with missing parser functions.

- [ ] **Step 3: Implement parsers and database recording**

Add provider parsers that flatten official bucket/row data into normalized rows. Preserve unknown fields in a JSON `raw` column when recording.

- [ ] **Step 4: Run tests to verify GREEN**

Run: `python3 tests/test_usage_core.py`

Expected: exit code 0.

- [ ] **Step 5: Commit**

```bash
git add bin/cc_usage.py tests/test_usage_core.py
git commit -m "Parse provider usage and cost buckets"
```

---

### Task 3: Pane-Aware Usage State In cc-dash

**Files:**
- Modify: `bin/cc-dash`
- Create: `tests/test_usage_dash.py`

**Interfaces:**
- Consumes: `cc_usage.normalize_pane_identity`, `record_pane`, `usage_db_path`, `init_db`
- Produces: `usage_live_panes() -> list[dict]` in `bin/cc-dash`
- Produces: `GET /usage/state`

- [ ] **Step 1: Write failing tests**

Create `tests/test_usage_dash.py` with AST checks:

```python
#!/usr/bin/env python3
from pathlib import Path


SRC = Path("bin/cc-dash").read_text()


def test_cc_dash_imports_usage_module():
    assert "import cc_usage" in SRC


def test_usage_state_endpoint_exists_and_is_authenticated():
    assert '"/usage/state"' in SRC
    api_get = SRC.split("API_GET = ", 1)[1].split("def do_GET", 1)[0]
    assert '"/usage/state"' in api_get


def test_usage_live_panes_records_pane_pwd_and_git_root():
    assert "def usage_live_panes" in SRC
    assert "cc_usage.normalize_pane_identity" in SRC
    assert "cc_usage.git_root_for_path" in SRC
    assert "cc_usage.record_pane" in SRC


if __name__ == "__main__":
    test_cc_dash_imports_usage_module()
    test_usage_state_endpoint_exists_and_is_authenticated()
    test_usage_live_panes_records_pane_pwd_and_git_root()
```

- [ ] **Step 2: Run tests to verify RED**

Run: `python3 tests/test_usage_dash.py`

Expected: FAIL because `cc-dash` has no usage endpoint yet.

- [ ] **Step 3: Implement cc-dash integration**

Modify `bin/cc-dash`:

- Add `import cc_usage`.
- Add `usage_live_panes()` after `read_states()`.
- Add `/usage/state` to `API_GET`.
- Add GET handler that returns `cc_usage.build_usage_state(db_path, live_panes)`.

The live pane identity must be built from `agent_pane_maps(agent_procs())`, `tab_labels()`, per-pane cwd, and `git_root_for_path`.

- [ ] **Step 4: Run tests to verify GREEN**

Run: `python3 tests/test_usage_dash.py && python3 tests/test_remote_controls.py`

Expected: exit code 0.

- [ ] **Step 5: Commit**

```bash
git add bin/cc-dash tests/test_usage_dash.py
git commit -m "Expose pane-aware usage state"
```

---

### Task 4: Usage Capture, Provider Refresh, And Alerts

**Files:**
- Modify: `bin/cc_usage.py`
- Modify: `bin/cc-dash`
- Modify: `tests/test_usage_core.py`
- Modify: `tests/test_usage_dash.py`

**Interfaces:**
- Produces: `record_turn(db_path: str, event: dict) -> dict`
- Produces: `build_usage_state(db_path: str, live_panes: list[dict], now: int | None = None) -> dict`
- Produces: `fetch_openai_usage(env: dict, now: int | None = None) -> tuple[list[dict], list[dict], dict]`
- Produces: `fetch_anthropic_usage(env: dict, now: int | None = None) -> tuple[list[dict], list[dict], dict]`
- Produces: `calculate_alerts(state: dict, settings: dict | None = None) -> list[dict]`
- Produces: `POST /usage/capture`
- Produces: `POST /usage/refresh`

- [ ] **Step 1: Write failing tests**

Add tests that verify:

```python
def test_record_turn_rolls_up_by_project_session_and_pane():
    with tempfile.TemporaryDirectory() as d:
        db = os.path.join(d, "usage.sqlite")
        cc_usage.init_db(db)
        event = {
            "provider": "claude",
            "agent": "claude",
            "tmux_session": "term-1",
            "tmux_pane": "%1",
            "pane_pwd": "/repo/frontend",
            "git_root": "/repo",
            "model": "sonnet",
            "turn_started_at": 100,
            "turn_finished_at": 120,
            "input_tokens": 10,
            "output_tokens": 5,
            "cost_usd": 0.01,
            "source": "cli_turn",
            "confidence": "exact",
        }
        cc_usage.record_turn(db, event)
        state = cc_usage.build_usage_state(db, [], now=130)

    assert state["totals"]["cost_usd"] == 0.01
    assert state["projects"][0]["git_root"] == "/repo"
    assert state["projects"][0]["panes"][0]["tmux_pane"] == "%1"
    assert state["projects"][0]["panes"][0]["confidence"] == "exact"


def test_aggregate_provider_bucket_is_unattributed_without_matching_pane():
    with tempfile.TemporaryDirectory() as d:
        db = os.path.join(d, "usage.sqlite")
        cc_usage.init_db(db)
        cc_usage.record_provider_costs(db, "openai", [{
            "provider": "openai",
            "start_time": 100,
            "end_time": 200,
            "cost_usd": 0.25,
            "currency": "usd",
            "project_id": "",
            "api_key_id": "",
            "line_item": "Completions",
            "confidence": "exact",
        }])
        state = cc_usage.build_usage_state(db, [], now=210)

    assert state["totals"]["cost_usd"] == 0.25
    assert state["unattributed"][0]["cost_usd"] == 0.25
    assert state["unattributed"][0]["confidence"] == "unattributed"


def test_alerts_fire_on_cost_threshold_and_spike():
    state = {
        "totals": {"cost_usd": 9.5},
        "windows": {"daily_budget_usd": 10.0},
        "series": [{"ts": 100, "cost_usd": 1.0}, {"ts": 200, "cost_usd": 9.5}],
    }

    alerts = cc_usage.calculate_alerts(state, {"cost_thresholds": [0.7, 0.85, 0.95], "spike_usd": 5.0})

    assert any(a["kind"] == "budget" and a["level"] == "danger" for a in alerts)
    assert any(a["kind"] == "spike" for a in alerts)
```

Update `tests/test_usage_dash.py`:

```python
def test_usage_capture_and_refresh_endpoints_exist():
    assert 'self.path == "/usage/capture"' in SRC
    assert 'self.path == "/usage/refresh"' in SRC
```

- [ ] **Step 2: Run tests to verify RED**

Run: `python3 tests/test_usage_core.py && python3 tests/test_usage_dash.py`

Expected: FAIL with missing record/build/endpoint functions.

- [ ] **Step 3: Implement minimal capture and refresh**

Implement local turn recording and provider refresh. Provider calls must:

- Use `OPENAI_ADMIN_KEY` for OpenAI.
- Use `ANTHROPIC_ADMIN_KEY` for Anthropic.
- Use `urllib.request` with timeout <= 8 seconds.
- Return credential health without returning secret values.
- Store complete successful provider responses only.

- [ ] **Step 4: Run tests to verify GREEN**

Run: `python3 tests/test_usage_core.py && python3 tests/test_usage_dash.py`

Expected: exit code 0.

- [ ] **Step 5: Commit**

```bash
git add bin/cc_usage.py bin/cc-dash tests/test_usage_core.py tests/test_usage_dash.py
git commit -m "Record usage turns and provider totals"
```

---

### Task 5: Dashboard Usage Drawer And Session Chips

**Files:**
- Modify: `dash/index.html`
- Create: `tests/test_usage_ui.py`

**Interfaces:**
- Consumes: `GET /usage/state`
- Produces: JS functions `tickUsage`, `renderUsage`, `usageChipText`, `openUsageDrawer`

- [ ] **Step 1: Write failing UI tests**

Create `tests/test_usage_ui.py`:

```python
#!/usr/bin/env python3
import re
from pathlib import Path


HTML = Path("dash/index.html").read_text()


def test_usage_drawer_markup_exists():
    assert 'id="btn-usage"' in HTML
    assert 'id="usage"' in HTML
    assert 'id="usage-providers"' in HTML
    assert 'id="usage-projects"' in HTML
    assert 'id="usage-alerts"' in HTML


def test_usage_state_is_fetched_without_secret_rendering():
    assert 'api("/usage/state")' in HTML
    assert "renderUsage" in HTML
    forbidden = ["OPENAI_ADMIN_KEY", "ANTHROPIC_ADMIN_KEY", "x-api-key"]
    for text in forbidden:
        assert text not in HTML


def test_session_cards_have_usage_chip_container():
    assert 'class="usage-chip hidden"' in HTML
    assert "usageChipText" in HTML


if __name__ == "__main__":
    test_usage_drawer_markup_exists()
    test_usage_state_is_fetched_without_secret_rendering()
    test_session_cards_have_usage_chip_container()
```

- [ ] **Step 2: Run tests to verify RED**

Run: `python3 tests/test_usage_ui.py`

Expected: FAIL because UI does not contain usage drawer yet.

- [ ] **Step 3: Implement UI**

Modify `dash/index.html`:

- Add header button `Uso`.
- Add drawer `<aside id="usage" class="drawer">`.
- Add compact CSS for provider cards, bars, sparkline, alerts, and chips.
- Fetch `/usage/state` every normal poll cycle, slower when hidden.
- Render provider health, totals, projects, panes, unattributed totals, and alerts.
- Add usage chip spans in `cardEl` and `rowEl`.
- Never render secret env var values.

- [ ] **Step 4: Run tests to verify GREEN**

Run: `python3 tests/test_usage_ui.py && python3 tests/test_dashboard_cards.py`

Expected: exit code 0.

- [ ] **Step 5: Commit**

```bash
git add dash/index.html tests/test_usage_ui.py
git commit -m "Add usage dashboard UI"
```

---

### Task 6: Pane-Targeted Model Switching

**Files:**
- Modify: `bin/cc_usage.py`
- Modify: `bin/cc-dash`
- Modify: `dash/index.html`
- Modify: `tests/test_usage_core.py`
- Modify: `tests/test_usage_dash.py`
- Modify: `tests/test_usage_ui.py`

**Interfaces:**
- Produces: `model_presets() -> list[dict]`
- Produces: `model_switch_text(provider: str, preset: str) -> str`
- Produces: `POST /model/switch`

- [ ] **Step 1: Write failing tests**

Add core tests:

```python
def test_model_presets_include_savings_daily_hard_maximum():
    names = {p["id"] for p in cc_usage.model_presets()}
    assert {"ahorro", "diario", "dificil", "maximo"}.issubset(names)


def test_model_switch_text_opens_provider_model_picker():
    assert cc_usage.model_switch_text("claude", "diario") == "/model sonnet"
    assert cc_usage.model_switch_text("codex", "diario") == "/model"
```

Add dash tests:

```python
def test_model_switch_endpoint_targets_requested_pane():
    assert 'self.path == "/model/switch"' in SRC
    assert 'cc_usage.model_switch_text' in SRC
    assert 'tmux("send-keys", "-t", pane, "-l", "--", switch_text)' in SRC
```

Add UI tests:

```python
def test_usage_ui_exposes_model_preset_buttons():
    for preset in ("Ahorro", "Diario", "Difícil", "Máximo"):
        assert preset in HTML
    assert 'api("/model/switch"' in HTML
```

- [ ] **Step 2: Run tests to verify RED**

Run: `python3 tests/test_usage_core.py && python3 tests/test_usage_dash.py && python3 tests/test_usage_ui.py`

Expected: FAIL with missing model preset/switch code.

- [ ] **Step 3: Implement model switching**

Implement:

- Preset definitions in `cc_usage`.
- `/model/switch` in `cc-dash`.
- Pane validation using the same optional `pane` targeting logic used by `/send` and `/key`.
- UI buttons in usage pane rows.

For Claude, send `/model <alias>` where aliases are documented. For Codex, send `/model` to open the picker unless a direct installed model can be resolved safely later.

- [ ] **Step 4: Run tests to verify GREEN**

Run: `python3 tests/test_usage_core.py && python3 tests/test_usage_dash.py && python3 tests/test_usage_ui.py`

Expected: exit code 0.

- [ ] **Step 5: Commit**

```bash
git add bin/cc_usage.py bin/cc-dash dash/index.html tests/test_usage_core.py tests/test_usage_dash.py tests/test_usage_ui.py
git commit -m "Add pane-targeted model switching"
```

---

### Task 7: Hook Integration And Manual Capture Path

**Files:**
- Modify: `hooks/cc-notify.sh`
- Modify: `adapters/codex-hooks.sh`
- Modify: `tests/test_codex_adapters.sh`
- Modify: `tests/test_usage_core.py`

**Interfaces:**
- Consumes: `record_turn`
- Produces: best-effort local usage capture metadata for turn lifecycle.

- [ ] **Step 1: Write failing tests**

Add test expectations:

- `hooks/cc-notify.sh` must preserve `TMUX_PANE` and session hints.
- `adapters/codex-hooks.sh` must pass Codex cwd and event metadata without losing pane attribution.
- Hook scripts must not fail if `cc_usage.py` or Python usage capture is unavailable.

- [ ] **Step 2: Run tests to verify RED**

Run: `bash tests/test_codex_adapters.sh`

Expected: FAIL until metadata/capture path exists.

- [ ] **Step 3: Implement best-effort capture**

Add a helper path that posts to `/usage/capture` or invokes a local Python helper with bounded timeout. Hooks must remain non-blocking and must not break notifications if usage capture fails.

- [ ] **Step 4: Run tests to verify GREEN**

Run: `bash tests/test_codex_adapters.sh && python3 tests/test_usage_core.py`

Expected: exit code 0.

- [ ] **Step 5: Commit**

```bash
git add hooks/cc-notify.sh adapters/codex-hooks.sh tests/test_codex_adapters.sh tests/test_usage_core.py
git commit -m "Capture usage metadata from agent hooks"
```

---

### Task 8: Full Verification, Restart, And Desktop Smoke Test

**Files:**
- Modify only if verification finds a defect.

**Interfaces:**
- Consumes all earlier tasks.

- [ ] **Step 1: Run full automated verification**

Run:

```bash
python3 tests/test_usage_core.py
python3 tests/test_usage_dash.py
python3 tests/test_usage_ui.py
python3 tests/test_remote_controls.py
python3 tests/test_dashboard_cards.py
python3 tests/test_desktop_tabs.py
python3 tests/test_remote_ui.py
python3 tests/test_desktop_links.py
bash tests/test_codex_adapters.sh
```

Expected: every command exits 0.

- [ ] **Step 2: Validate backend imports**

Run:

```bash
python3 -m py_compile bin/cc_usage.py bin/cc-dash
```

Expected: exit code 0.

- [ ] **Step 3: Restart services**

Run:

```bash
systemctl --user restart cc-dash.service cc-notifyd.service
systemctl --user status cc-dash.service --no-pager
```

Expected: `cc-dash.service` is active.

- [ ] **Step 4: Browser smoke check**

Open `http://127.0.0.1:4777`, verify:

- `Uso` button is visible.
- Drawer opens.
- Provider cards render credential health.
- Active panes show session, pane, folder, git root, provider, and confidence.
- No secret values appear.

- [ ] **Step 5: Commit final fixes if needed**

```bash
git add .
git commit -m "Verify usage accounting feature"
```

Only commit if Step 1-4 required changes after Task 7.

---

## Self-Review

- Spec coverage: pane identity, SQLite storage, exactness rules, provider usage/cost imports, alerts, model recommendations, UI, and tests are covered by Tasks 1-8.
- No placeholder scan: this plan intentionally avoids unresolved markers and gives exact files, commands, and expected outcomes.
- Type consistency: produced function names are stable across tasks and consumed by later tasks.
