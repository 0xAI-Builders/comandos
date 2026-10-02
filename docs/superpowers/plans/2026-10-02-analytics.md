# Analytics (Cuentas · Comparar · Pomodoro) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reemplazar el modal Analytics del tablero por las tres pestañas aprobadas en el grill del 1–2 oct (Cuentas · Comparar · Pomodoro), idénticas pixel por pixel al mockup maestro, con datos reales por cuenta, y retirar Guardia, Alertas, la propuesta/Aplicar/arrastre de Reparto y la comparación de configuraciones.

**Architecture:** El mockup aprobado es la fuente de la verdad visual. `tools/analytics_extract.cjs` copia tal cual sus 57 piezas de dibujo a `dash/analytics-render.js` y `tools/analytics_scope_css.py` encierra su CSS bajo `.an` en `dash/analytics.css`. Una prueba de paridad compara el HTML del tablero contra el HTML exportado del mockup en 21 casos. El servidor arma esos mismos datos (`lib/analytics_week.py`, `GET /analytics/week`) a partir de importadores corregidos: Claude y Codex por cuenta, una fila por respuesta, tramos de trabajo medidos y fotos de cuota por ciclo. `dash/analytics.js` lo monta en `#usage`.

**Tech Stack:** Python 3.10 stdlib (sqlite3, zoneinfo), JS vanilla en el tablero, node solo para generar y revisar, pytest, MCP `chrome-bg` en la Mac para la verificación visual.

**Spec:** `design/DESIGN.md` § «Analytics (grill del 2026-10-01 y 2026-10-02)» y el mockup maestro en la rama `prototype/analytics-grill`, archivo `dash/prototypes/prototype-analytics.html` (commit `717b32d` o posterior; publicado en https://claude.ai/artifact/TUhpGkx65o7ytP3hRMkggz).

## Global Constraints

- Todo es **por cuenta**: id `proveedor:alias` (`claude:main`, `claude:relotto`, `codex:main`, `grok:main`). Nunca se suman cuentas ni porcentajes de cuota entre cuentas.
- **Pixel perfect:** `dash/analytics-render.js` y `dash/analytics.css` son archivos GENERADOS desde el mockup. No se editan a mano. Un cambio visual se hace en el mockup (rama `prototype/analytics-grill`), se regenera y se actualizan las referencias.
- La app no actúa sola: nada propone ni aplica cambios (fuera Analizar/Aplicar/arrastrar de Reparto) y no hay avisos de cuota.
- La copia en pantalla es la del mockup, en español, sin cambios.
- Sin dependencias nuevas: Python 3.10 stdlib, JS vanilla; node solo para generar y revisar.
- Días locales en `America/Mexico_City`. La ventana es de 8 días (hoy y los 7 anteriores); la semana anterior son los 8 días previos. Solo existen offset `0` y `-1`.
- Retención del uso local: 21 días (antes 14).
- Navegador: solo MCP `chrome-bg` en la Mac, con `cc-browser-expose start <puerto>`. Nunca Chrome, Chromium, Playwright, Puppeteer ni Xvfb en Linux.
- Puertos de prueba con `devhost add <nombre>`. Nunca 32768–60999 ni 3000/5173/8000/8080.
- No reiniciar la app GTK de Jesús. `cc-dash.service` se reinicia solo después de su revisión.
- `docs/superpowers/` está en `.gitignore`: este plan se añade con `git add -f`.
- Tests: `pytest -q tests/<archivo>` desde la raíz del worktree; los checks node corren con `node tests/<archivo>.cjs` y también desde `tests/test_analytics_checks.py`.
- Otra sesión edita `dash/index.html` y `dash/workspace.css` en `main`. Trabajar en un worktree (`superpowers:using-git-worktrees`, rama `implementation/analytics`) y rebasar sobre `main` antes de mergear.

## Review Focus

1. **Rollout de Codex bifurcado:** repite el historial de su padre con los mismos ids; esas respuestas se cuentan una sola vez. Test en Task 2.
2. **Reset que el proveedor mueve unos segundos entre lecturas:** sigue siendo el mismo ciclo, no dos semanas. Test en Task 3.
3. **Semana anterior sin fotos de cuota:** la repisa de Comparar dice «—», no «100%», y nada truena. Test en Task 5.
4. **Falla la red al cambiar de semana:** se queda la vista que ya estaba y no se borra nada. Test en Task 8.
5. **Sesión que cruza medianoche:** son dos bloques, con horas y tokens en el día correcto. Test en Task 5.

## Decisión D1 (Jesús, 2-oct): dónde se abre Analytics

«En el mismo botón y que se abra en un modal en medio.» El botón Analytics de la cabecera abre un modal centrado. En la app de escritorio es una ventana sin bordes centrada sobre toda la app, como Cadenas (`HEADER_ACTIONS`, `bin/cc-app` ~6940), porque el tablero web solo ocupa la columna izquierda. En el remoto partido, el modal cubre toda la ventana en vez de quedarse en la columna. Lo implementa la Task 11.

---

## File Structure

| Archivo | Responsabilidad |
|---|---|
| `bin/cc_usage.py` (modificar) | Esquema v11 (`usage_spans`, `usage_quota_snapshots`), `account_homes`, `record_spans`, `_changed_files`, Claude por cuenta y por respuesta, `record_local_codex_rollouts` (reemplaza `record_local_codex_threads`), `record_quota_snapshots`, `quota_snapshots`; fuera los avisos de cuota. |
| `bin/cc-dash` (modificar) | Import por cuenta con retención de 21 días; fotos de cuota en cada lectura de límites más un loop de 5 min; `GET /analytics/week` (con `demo=`); fuera `/allocation/*`, `/usage/alert-rule` y los avisos de cuota. |
| `lib/analytics_week.py` (nuevo) | Puro: filas → modelo de la vista (semana, días, cuentas, sesiones, semana pasada, cuota sobrante, pomodoros). |
| `tools/analytics_extract.cjs` (nuevo) | Genera `dash/analytics-render.js` desde el mockup. |
| `tools/analytics_fixture.cjs` (nuevo) | Exporta del mockup los datos de ejemplo y el HTML de referencia. |
| `tools/analytics_scope_css.py` (nuevo) | Genera `dash/analytics.css` desde el `<style>` del mockup. |
| `tools/css_orphans.py` (nuevo) | Lista clases e ids del `<style>` de `index.html` que ya nadie usa. |
| `dash/analytics-render.js` (generado) | `AnalyticsRender.create(modelo, {phone, phoneDay, minOffset}).html(pestaña)`. |
| `dash/analytics.css` (generado) | Estilos del mockup bajo `.an` y las dos fuentes del diseño. |
| `dash/analytics.js` (nuevo) | `Analytics.create(el, {fetchWeek, tab, onTab, width})`: datos, pestañas, semana, celular, tooltips y +N. |
| `assets/fonts/Inter/Inter-latin.woff2`, `assets/fonts/UbuntuSansMono/UbuntuSansMono-latin.woff2` (+ licencias) | Las mismas fuentes que carga el mockup, servidas por el tablero. |
| `dash/index.html` (modificar) | `#usage` = `<div class="an" id="an-root">`; montaje y apertura; fuera la vista vieja (Resumen, Guardia, Alertas, Comparar viejo, Proveedores, Reparto, Pomodoro viejo). |
| `dash/pomodoro.js`, `dash/workspace.css` (modificar) | Fuera las stats viejas de Pomodoro (`loadAnalytics`, `renderAnalytics`). |
| `dash/reparto.js`, `dash/reparto.css`, `lib/allocation_batch.py` (borrar) | Reparto con propuesta/aplicar/arrastre. |
| `lib/allocation.py` (modificar) | Se queda solo `enrich_limits` y sus helpers. |
| `lib/operator_catalog.py` (modificar) | `open_analytics_tab` acepta `cuentas`, `comparar`, `pomodoro`. |
| `install.sh` (modificar) | Enlaces de los archivos nuevos; fuera los de Reparto. |
| `tests/fixtures/analytics/` (generado) | `week-{normal,normal-prev,pesada,vacio,limite}.json` y `reference.json`. |
| `tests/test_usage_accounts.py`, `tests/test_analytics_week.py`, `tests/test_analytics_endpoint.py`, `tests/test_analytics_checks.py`, `tests/analytics_parity_checks.cjs`, `tests/analytics_ui_checks.cjs`, `tests/test_analytics_dash.py` (nuevos) | Pruebas. |

---

### Task 1: Uso de Claude por cuenta, una vez por respuesta, con tramos medidos

Hoy `record_local_claude_jsonl` solo lee `~/.claude/projects`, así que no importa relotto. Además deja la cuenta en `unknown` y cuenta cada respuesta una vez por bloque de contenido, unas 2.2 veces de más. Esta tarea lee todas las cuentas, cuenta por `(message.id, requestId)` y guarda la duración real de cada turno (`system/turn_duration`) como tramo de trabajo.

**Files:**
- Modify: `bin/cc_usage.py` (`USAGE_SCHEMA_VERSION`, `_migrate_db`, `_migrate_schema`, `record_local_claude_jsonl`, `prune_old_turns`; nuevas `account_homes`, `record_spans`, `_changed_files`)
- Modify: `bin/cc-dash` (`_do_refresh_local_usage`, nueva `_IMPORT_SEEN`)
- Create: `tests/test_usage_accounts.py`

**Interfaces:**
- Produces: `cc_usage.account_homes(default_home: str, accounts_root: str) -> list[tuple[str, str]]` (`[("main", ~), (alias, ruta), …]`).
- Produces: `cc_usage.record_spans(db_path, spans: list[dict]) -> int`. Cada dict: `id, provider, account, session_id, git_root, started_at, finished_at, source`.
- Produces: `cc_usage._changed_files(files: list[tuple[float, str]], seen: dict|None) -> list`.
- Produces: `record_local_claude_jsonl(db_path, projects_root=None, now=None, max_age_days=14, max_files=400, account="main", seen=None) -> int`.
- Produces: tabla `usage_spans(id, provider, account, session_id, git_root, started_at real, finished_at real, source)` y tabla `usage_quota_snapshots(limit_id, provider, account, win, scope, resets_at, percent, captured_at)`, ambas en el esquema v11.

- [ ] **Step 1: Write the failing tests**

Crear `tests/test_usage_accounts.py`:

```python
"""Uso por cuenta: Claude y Codex se importan con su alias, una vez por respuesta, con tramos medidos."""
import importlib.util
import json
import sqlite3
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("cc_usage_accounts", ROOT / "bin" / "cc_usage.py")
cc_usage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cc_usage)

NOW = 1790841600  # 2026-10-01T06:00:00Z


def _write(path, rows):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("".join(json.dumps(r) + "\n" for r in rows))


def _assistant(uuid, msg_id, req, ts="2026-10-01T05:00:00Z", out=5):
    return {"type": "assistant", "uuid": uuid, "requestId": req, "timestamp": ts, "cwd": "/repo", "sessionId": "s1",
            "message": {"id": msg_id, "model": "claude-test",
                        "usage": {"input_tokens": 10, "output_tokens": out, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}}}


def test_account_homes_lists_main_then_each_account(tmp_path):
    (tmp_path / "accts" / "relotto").mkdir(parents=True)
    (tmp_path / "accts" / "relotto.lock").mkdir()
    (tmp_path / "accts" / ".hidden").mkdir()
    homes = cc_usage.account_homes(str(tmp_path / "main"), str(tmp_path / "accts"))
    assert homes == [("main", str(tmp_path / "main")), ("relotto", str(tmp_path / "accts" / "relotto"))]


def test_claude_counts_each_response_once_and_keeps_the_account(tmp_path, monkeypatch):
    monkeypatch.setattr(cc_usage, "git_root_for_path", lambda p: p)
    db = tmp_path / "u.sqlite"
    # Una respuesta con dos bloques de contenido = dos líneas con el mismo usage.
    _write(tmp_path / "relotto" / "projects" / "p" / "a.jsonl", [
        _assistant("u1", "msg_1", "req_1"), _assistant("u2", "msg_1", "req_1"),
        _assistant("u3", "msg_2", "req_2", ts="2026-10-01T05:10:00Z", out=7),
        {"type": "system", "subtype": "turn_duration", "durationMs": 600000, "uuid": "t1",
         "timestamp": "2026-10-01T05:10:00Z", "cwd": "/repo", "sessionId": "s1"},
    ])
    cc_usage.record_local_claude_jsonl(db, tmp_path / "relotto" / "projects", now=NOW, max_age_days=30, account="relotto")
    con = sqlite3.connect(db)
    turns = con.execute("select harness_account, motor_account, total_tokens from usage_turns order by id").fetchall()
    spans = con.execute("select provider, account, git_root, finished_at - started_at from usage_spans").fetchall()
    assert turns == [("relotto", "relotto", 15), ("relotto", "relotto", 17)]
    assert spans == [("claude", "relotto", "/repo", 600.0)]


def test_unchanged_transcripts_are_not_read_again(tmp_path, monkeypatch):
    monkeypatch.setattr(cc_usage, "git_root_for_path", lambda p: p)
    db = tmp_path / "u.sqlite"
    _write(tmp_path / "projects" / "a.jsonl", [_assistant("u1", "msg_1", "req_1")])
    seen = {}
    assert cc_usage.record_local_claude_jsonl(db, tmp_path / "projects", now=NOW, max_age_days=30, seen=seen) == 1
    assert cc_usage.record_local_claude_jsonl(db, tmp_path / "projects", now=NOW, max_age_days=30, seen=seen) == 0


def test_v10_database_drops_rows_that_double_counted(tmp_path):
    db = tmp_path / "u.sqlite"
    cc_usage.record_turns(db, [
        {"id": "claude-jsonl-u1", "provider": "claude", "agent": "claude", "source": "claude_jsonl", "confidence": "local"},
        {"id": "codex-state-t", "provider": "codex", "agent": "codex", "source": "codex_state_db", "confidence": "shared"},
        {"id": "grok-1", "provider": "grok", "agent": "grok", "source": "grok_updates", "confidence": "exact"},
    ])
    con = sqlite3.connect(db)
    con.execute("pragma user_version=10")
    con.commit()
    con.close()
    cc_usage.init_db(db)
    con = sqlite3.connect(db)
    assert [r[0] for r in con.execute("select id from usage_turns")] == ["grok-1"]
    assert con.execute("pragma user_version").fetchone()[0] == cc_usage.USAGE_SCHEMA_VERSION
    tables = {r[0] for r in con.execute("select name from sqlite_master where type='table'")}
    assert {"usage_spans", "usage_quota_snapshots"} <= tables


def test_refresh_imports_every_claude_account_for_21_days(tmp_path, monkeypatch):
    import sys
    sys.path.insert(0, str(ROOT / "tests"))
    from test_usage_dash import load_dash_module
    dash = load_dash_module()
    (tmp_path / ".claude-accounts" / "relotto").mkdir(parents=True)
    monkeypatch.setenv("HOME", str(tmp_path))
    calls = []
    monkeypatch.setattr(dash, "usage_runtime_env", lambda: {})
    monkeypatch.setattr(dash, "ensure_observed_configs", lambda: None)
    monkeypatch.setattr(dash.cc_usage, "reconcile_orphan_interactions", lambda *a, **k: 0)
    monkeypatch.setattr(dash.cc_usage, "prune_old_turns", lambda *a, **k: calls.append(("prune", k["max_age_days"])) or 0)
    for name in ("record_local_grok_updates", "record_local_opencode_db", "record_local_codex_threads", "record_local_codex_rollouts"):
        if hasattr(dash.cc_usage, name):
            monkeypatch.setattr(dash.cc_usage, name, lambda *a, **k: 0)
    monkeypatch.setattr(dash.cc_usage, "record_local_claude_jsonl",
                        lambda db, root, **k: calls.append((k["account"], str(root), k["max_age_days"])) or 0)
    monkeypatch.setattr(dash.grok_state, "account_homes", lambda: [])
    dash._do_refresh_local_usage()
    assert calls == [("prune", 21),
                     ("main", str(tmp_path / ".claude" / "projects"), 21),
                     ("relotto", str(tmp_path / ".claude-accounts" / "relotto" / "projects"), 21)]
```

- [ ] **Step 2: Run them to verify they fail**

Run: `pytest -q tests/test_usage_accounts.py`
Expected: FAIL with `AttributeError: module 'cc_usage_accounts' has no attribute 'account_homes'`, along with the failures of the other four tests.

- [ ] **Step 3: Esquema v11 en `bin/cc_usage.py`**

1. Cambiar `USAGE_SCHEMA_VERSION = 10` por `USAGE_SCHEMA_VERSION = 11`.
2. En `_migrate_db`, pasar la versión vieja a la migración: `_migrate_schema(con)` → `_migrate_schema(con, current)`.
3. `def _migrate_schema(con):` → `def _migrate_schema(con, current=0):`.
4. En `_migrate_schema`, justo antes de `con.execute(f"pragma user_version={USAGE_SCHEMA_VERSION}")`, agregar:

```python
    _execute_statements(con, """
    create table if not exists usage_spans (
      id text primary key,
      provider text not null,
      account text not null default 'main',
      session_id text not null default '',
      git_root text not null default '',
      started_at real not null,
      finished_at real not null,
      source text not null default ''
    );
    create index if not exists idx_usage_spans_finished on usage_spans(finished_at);
    create table if not exists usage_quota_snapshots (
      limit_id text not null,
      provider text not null,
      account text not null,
      win text not null default '',
      scope text not null default '',
      resets_at integer not null,
      percent real not null,
      captured_at integer not null,
      primary key (limit_id, resets_at)
    );
    """)
    if current and current < 11:
        # v11 cuenta cada respuesta de Claude una vez (antes, una por bloque: ~2x) y lee
        # Codex por respuesta desde sus rollouts: se borran las filas viejas y el próximo
        # import las rehace desde los transcripts.
        con.execute("delete from usage_turns where source in ('claude_jsonl', 'codex_state_db')")
```

(La columna se llama `win` y no `window` porque `WINDOW` es palabra reservada en SQLite.)

- [ ] **Step 4: Cuentas, tramos y archivos sin cambios**

Justo antes de `def record_local_claude_jsonl(`, agregar:

```python
def account_homes(default_home, accounts_root):
    """[(alias, carpeta)]: 'main' es la carpeta por defecto; cada subcarpeta de
    accounts_root es otra cuenta (mismo criterio que lib/accounts.list_accounts)."""
    out = [("main", os.path.expanduser(default_home))]
    root = os.path.expanduser(accounts_root)
    try:
        names = sorted(os.listdir(root))
    except OSError:
        names = []
    for name in names:
        path = os.path.join(root, name)
        if name.startswith((".", "-")) or name.endswith(".lock") or not os.path.isdir(path):
            continue
        out.append((name, path))
    return out


def record_spans(db_path, spans):
    """Tramos de trabajo medidos (inicio y fin de cada turno) por cuenta y carpeta."""
    rows = [(s["id"], s["provider"], s.get("account") or "main", s.get("session_id") or "",
             s.get("git_root") or "", float(s["started_at"]), float(s["finished_at"]), s.get("source") or "")
            for s in spans]
    if not rows:
        return 0
    init_db(db_path)
    with connect(db_path) as con:
        con.executemany(
            "insert or replace into usage_spans (id, provider, account, session_id, git_root, started_at, finished_at, source) "
            "values (?,?,?,?,?,?,?,?)", rows)
    return len(rows)


def _changed_files(files, seen):
    """Solo los archivos que cambiaron desde el último import (seen: {ruta: mtime})."""
    if seen is None:
        return files
    out = [(m, p) for m, p in files if seen.get(p) != m]
    for m, p in out:
        seen[p] = m
    return out
```

- [ ] **Step 5: `record_local_claude_jsonl` por cuenta y por respuesta**

1. Firma: `def record_local_claude_jsonl(db_path, projects_root=None, now=None, max_age_days=14, max_files=400, account="main", seen=None):`
2. Tras `files.sort(reverse=True)`, reemplazar:

```python
    events = []
    roots = {}
    for _mtime, path in files[:max_files]:
```
por:
```python
    events = []
    spans = []
    roots = {}
    for _mtime, path in _changed_files(files[:max_files], seen):
```
3. Reemplazar el comienzo del cuerpo del `for line_no, line in enumerate(fh, 1):`, desde `msg = data.get("message") if isinstance(data, dict) else None` hasta el `continue` que descarta lo que no es `assistant`, por:

```python
                    if not isinstance(data, dict):
                        continue
                    if data.get("type") == "system" and data.get("subtype") == "turn_duration":
                        end = _as_epoch(data.get("timestamp"))
                        ms = _as_int(data.get("durationMs"))
                        if end >= cutoff and ms > 0:
                            cwd = _text(data.get("cwd"))
                            if cwd not in roots:
                                roots[cwd] = git_root_for_path(cwd)
                            spans.append({"id": "claude-turn-" + _text(data.get("uuid") or f"{path}:{line_no}"),
                                          "provider": "claude", "account": account,
                                          "session_id": _text(data.get("sessionId")), "git_root": roots[cwd] or cwd,
                                          "started_at": end - ms / 1000, "finished_at": end, "source": "claude_jsonl"})
                        continue
                    msg = data.get("message")
                    usage = msg.get("usage") if isinstance(msg, dict) else None
                    if data.get("type") != "assistant" or not isinstance(usage, dict):
                        continue
```
4. Reemplazar `stable = _text(data.get("uuid") or data.get("requestId") or f"{path}:{line_no}")` por:

```python
                    # Claude Code escribe una línea por bloque de contenido y todas repiten el
                    # mismo usage: la respuesta es (message.id, requestId), no la línea.
                    msg_id = _text(msg.get("id"))
                    stable = (f"{msg_id}:{_text(data.get('requestId'))}" if msg_id
                              else _text(data.get("uuid") or data.get("requestId") or f"{path}:{line_no}"))
```
5. En el dict `event`, antes de `"source": "claude_jsonl",`, agregar `"harness_account": account,` y `"motor_account": account,`.
6. Al final de la función: `return record_turns(db_path, events)` → `record_spans(db_path, spans)` seguido de `return record_turns(db_path, events)`.
7. En `prune_old_turns`, después del `delete from usage_turns …`, agregar `con.execute("delete from usage_spans where finished_at < ?", (cutoff,))`.

- [ ] **Step 6: `bin/cc-dash` importa cada cuenta y conserva 21 días**

Junto a `LOCAL_USAGE_REFRESH_AT` (línea ~174) agregar:

```python
# {fuente: {ruta: mtime}}: el import local solo vuelve a leer los archivos que cambiaron.
_IMPORT_SEEN = {}
```

En `_do_refresh_local_usage`:
- `max_age = int(env.get("COMANDOS_USAGE_LOCAL_DAYS") or 14)` → `or 21`. Actualizar el comentario de arriba: «(21 d: la semana anterior de Analytics necesita 16)».
- `max_claude_files = int(env.get("COMANDOS_USAGE_CLAUDE_MAX_FILES") or 120)` → `or 600`.
- Reemplazar la entrada `"claude_local": cc_usage.record_local_claude_jsonl(…)` por:

```python
        "claude_local": sum(
            cc_usage.record_local_claude_jsonl(
                USAGE_DB,
                (env.get("COMANDOS_CLAUDE_PROJECTS_DIR") if alias == "main" else None) or os.path.join(home, "projects"),
                now=now,
                max_age_days=max_age,
                max_files=max_claude_files,
                account=alias,
                seen=_IMPORT_SEEN.setdefault(f"claude:{alias}", {}),
            )
            for alias, home in cc_usage.account_homes("~/.claude", "~/.claude-accounts")
        ),
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `pytest -q tests/test_usage_accounts.py tests/test_usage_core.py tests/test_usage_telemetry.py tests/test_usage_performance.py`
Expected: PASS (los 5 nuevos y los existentes).

- [ ] **Step 8: Commit**

```bash
git add bin/cc_usage.py bin/cc-dash tests/test_usage_accounts.py
git commit -m "fix(usage): Claude por cuenta y una vez por respuesta, con tramos medidos (esquema v11)"
```

---

### Task 2: Codex por respuesta y por cuenta, desde sus rollouts

Hoy Codex se lee de `~/.codex/state_5.sqlite` con una sola fila acumulada por hilo, fechada en su última actualización, y sin cuenta. Los rollouts traen cada respuesta (`token_usage_record`, con `response_id`) y cada turno (`task_complete`, con inicio y fin).

**Files:**
- Modify: `bin/cc_usage.py` (borrar `record_local_codex_threads`; nueva `record_local_codex_rollouts`)
- Modify: `bin/cc-dash` (`_do_refresh_local_usage`)
- Modify: `tests/test_usage_accounts.py`, `tests/test_usage_core.py` (borrar `test_record_local_codex_threads_imports_sqlite_tokens` y su llamada en la lista final), `tests/test_usage_dash.py:77`, `tests/test_usage_performance.py:114`

**Interfaces:**
- Consumes: `account_homes`, `record_spans`, `_changed_files` (Task 1).
- Produces: `cc_usage.record_local_codex_rollouts(db_path, homes=None, now=None, max_age_days=14, max_files=300, seen=None) -> int`. `homes` es `[(alias, CODEX_HOME)]` y por defecto `account_homes("~/.codex", "~/.codex-accounts")`. Las filas usan `source="codex_rollout"`, id `codex-resp-<response_id>` y tramos `codex-turn-<turn_id>`.

- [ ] **Step 1: Write the failing test**

Agregar a `tests/test_usage_accounts.py`:

```python
def _rollout(path, rows):
    _write(path, [{"timestamp": ts, "type": kind, "payload": payload} for ts, kind, payload in rows])


def test_codex_reads_each_response_once_per_account_even_across_forks(tmp_path, monkeypatch):
    monkeypatch.setattr(cc_usage, "git_root_for_path", lambda p: p)
    db = tmp_path / "u.sqlite"
    usage = {"input_tokens": 1000, "cached_input_tokens": 800, "cache_write_input_tokens": 0,
             "output_tokens": 50, "reasoning_output_tokens": 10, "total_tokens": 1050}
    parent = [
        ("2026-10-01T05:00:00Z", "session_meta", {"id": "th1", "cwd": "/repo"}),
        ("2026-10-01T05:00:00Z", "turn_context", {"turn_id": "tu1", "cwd": "/repo/app", "model": "gpt-5.5", "effort": "high"}),
        ("2026-10-01T05:01:00Z", "token_usage_record", {"thread_id": "th1", "turn_id": "tu1", "response_id": "resp_1", "usage": usage}),
        ("2026-10-01T05:02:00Z", "event_msg", {"type": "task_complete", "turn_id": "tu1", "started_at": 1790830800, "completed_at": 1790830920}),
    ]
    home = tmp_path / "codex-main"
    _rollout(home / "sessions" / "2026" / "10" / "01" / "rollout-a.jsonl", parent)
    # Un rollout bifurcado repite el historial del padre con los mismos ids.
    _rollout(home / "sessions" / "2026" / "10" / "01" / "rollout-b.jsonl", [("2026-10-01T05:10:00Z", "session_meta", {"id": "th2", "cwd": "/repo"})] + parent[1:])
    _rollout(tmp_path / "codex-work" / "sessions" / "rollout-c.jsonl", [
        ("2026-10-01T05:20:00Z", "token_usage_record", {"thread_id": "th3", "turn_id": "tu3", "response_id": "resp_3", "usage": usage}),
    ])
    cc_usage.record_local_codex_rollouts(db, [("main", str(home)), ("work", str(tmp_path / "codex-work"))], now=NOW, max_age_days=30)
    con = sqlite3.connect(db)
    turns = con.execute("select id, harness_account, git_root, model, input_tokens, cache_read_tokens, total_tokens, reasoning_tokens "
                        "from usage_turns order by id").fetchall()
    assert turns == [("codex-resp-resp_1", "main", "/repo/app", "gpt-5.5", 200, 800, 1050, 10),
                     ("codex-resp-resp_3", "work", "", "", 200, 800, 1050, 10)]
    assert con.execute("select id, account, git_root, finished_at - started_at from usage_spans").fetchall() == [
        ("codex-turn-tu1", "main", "/repo/app", 120.0)]
```

- [ ] **Step 2: Run it to verify it fails**

Run: `pytest -q tests/test_usage_accounts.py -k codex`
Expected: FAIL with `AttributeError: ... has no attribute 'record_local_codex_rollouts'`.

- [ ] **Step 3: Reemplazar `record_local_codex_threads` por `record_local_codex_rollouts`**

Borrar la función `record_local_codex_threads` completa y poner en su lugar:

```python
def record_local_codex_rollouts(db_path, homes=None, now=None, max_age_days=14, max_files=300, seen=None):
    """Codex por respuesta y por cuenta, desde los rollouts de cada CODEX_HOME.

    token_usage_record trae el uso exacto de cada respuesta (response_id); task_complete
    trae inicio y fin de cada turno. Un rollout bifurcado repite los eventos de su padre
    con los mismos ids, así que el upsert no los cuenta dos veces."""
    homes = homes if homes is not None else account_homes("~/.codex", "~/.codex-accounts")
    ts = int(now if now is not None else time.time())
    cutoff = ts - int(max_age_days) * 24 * 3600
    events, spans, roots = [], [], {}

    def root_of(cwd):
        if cwd not in roots:
            roots[cwd] = git_root_for_path(cwd) if cwd else ""
        return roots[cwd] or cwd

    for account, home in homes:
        files = []
        for root, _dirs, names in os.walk(os.path.join(home, "sessions")):
            for name in names:
                if not (name.startswith("rollout-") and name.endswith(".jsonl")):
                    continue
                path = os.path.join(root, name)
                try:
                    mtime = os.path.getmtime(path)
                except OSError:
                    continue
                if mtime >= cutoff:
                    files.append((mtime, path))
        files.sort(reverse=True)
        for _mtime, path in _changed_files(files[:max_files], seen):
            cwd, thread, turns = "", "", {}
            try:
                with open(path, errors="replace") as fh:
                    for line in fh:
                        try:
                            data = json.loads(line)
                        except Exception:
                            continue
                        if not isinstance(data, dict):
                            continue
                        kind = data.get("type")
                        payload = data.get("payload") if isinstance(data.get("payload"), dict) else {}
                        if kind == "session_meta":
                            cwd = _text(payload.get("cwd")) or cwd
                            thread = _text(payload.get("id")) or thread
                        elif kind == "turn_context":
                            turns[_text(payload.get("turn_id"))] = payload
                        elif kind == "token_usage_record":
                            rid = _text(payload.get("response_id"))
                            usage = payload.get("usage") if isinstance(payload.get("usage"), dict) else {}
                            finished = _as_epoch(data.get("timestamp"))
                            if not rid or finished < cutoff:
                                continue
                            ctx = turns.get(_text(payload.get("turn_id")), {})
                            here = _text(ctx.get("cwd")) or cwd
                            inp = _as_int(usage.get("input_tokens"))
                            cached = _as_int(usage.get("cached_input_tokens"))
                            out = _as_int(usage.get("output_tokens"))
                            total = _as_int(usage.get("total_tokens")) or inp + out
                            if total <= 0:
                                continue
                            events.append({
                                "id": "codex-resp-" + rid,
                                "provider": "codex",
                                "agent": "codex",
                                "tmux_session": _text(payload.get("thread_id")) or thread,
                                "tmux_pane": "",
                                "pane_pwd": here,
                                "git_root": root_of(here),
                                "model": _real_model(ctx.get("model")),
                                "reasoning_effort": _text(ctx.get("effort")),
                                "turn_started_at": finished,
                                "turn_finished_at": finished,
                                "input_tokens": max(0, inp - cached),
                                "output_tokens": out,
                                "cache_read_tokens": cached,
                                "cache_write_tokens": _as_int(usage.get("cache_write_input_tokens")),
                                "total_tokens": total,
                                "reasoning_tokens": _as_int(usage.get("reasoning_output_tokens")),
                                "harness_account": account,
                                "motor_account": account,
                                "source": "codex_rollout",
                                "confidence": "local",
                                "raw": json.dumps({"path": path, "turn_id": _text(payload.get("turn_id")),
                                                   "response_id": rid}, sort_keys=True),
                            })
                        elif kind == "event_msg" and payload.get("type") == "task_complete":
                            turn_id = _text(payload.get("turn_id"))
                            started = _as_int(payload.get("started_at"))
                            finished = _as_int(payload.get("completed_at"))
                            if not turn_id or not started or finished < max(started, cutoff):
                                continue
                            here = _text(turns.get(turn_id, {}).get("cwd")) or cwd
                            spans.append({"id": "codex-turn-" + turn_id, "provider": "codex", "account": account,
                                          "session_id": thread, "git_root": root_of(here),
                                          "started_at": started, "finished_at": finished, "source": "codex_rollout"})
            except OSError:
                continue
    record_spans(db_path, spans)
    return record_turns(db_path, events)
```

(OpenAI incluye los tokens en caché dentro de `input_tokens`; por eso `input_tokens` guarda `input - cached` y `cache_read_tokens` guarda `cached`, igual que en Claude.)

- [ ] **Step 4: `bin/cc-dash` usa los rollouts**

En `_do_refresh_local_usage`, reemplazar la entrada `"codex_local": cc_usage.record_local_codex_threads(…)` por:

```python
        "codex_local": cc_usage.record_local_codex_rollouts(
            USAGE_DB,
            now=now,
            max_age_days=max_age,
            seen=_IMPORT_SEEN.setdefault("codex", {}),
        ),
```

`COMANDOS_CODEX_STATE_DB` deja de usarse: `grep -n COMANDOS_CODEX_STATE_DB bin lib docs` no debe dar resultados fuera del changelog.

- [ ] **Step 5: Actualizar las pruebas viejas**

- `tests/test_usage_core.py`: borrar `test_record_local_codex_threads_imports_sqlite_tokens` y su llamada en la lista de ejecución del final del archivo.
- `tests/test_usage_dash.py:77`: `"record_local_codex_threads" in SRC` → `"record_local_codex_rollouts" in SRC`.
- `tests/test_usage_performance.py:114`: `'record_local_codex_threads'` → `'record_local_codex_rollouts'`.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `pytest -q tests/test_usage_accounts.py tests/test_usage_core.py tests/test_usage_dash.py tests/test_usage_performance.py tests/test_usage_telemetry.py`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add bin/cc_usage.py bin/cc-dash tests/test_usage_accounts.py tests/test_usage_core.py tests/test_usage_dash.py tests/test_usage_performance.py
git commit -m "fix(usage): Codex por respuesta y por cuenta desde los rollouts"
```

---

### Task 3: Fotos de cuota por ciclo y fin de los avisos de cuota

«Cuota que sobra» y la semana anterior de Comparar necesitan saber cuánto quedaba de cada límite al reset. Hoy eso no se guarda. Además los avisos al cruzar 70/85/95 % se van, porque Jesús nunca los usó.

**Files:**
- Modify: `bin/cc_usage.py` (nuevas `record_quota_snapshots`, `quota_snapshots`; borrar los avisos de cuota)
- Modify: `bin/cc-dash` (`_refresh_provider_limits`, nuevo `_limits_snapshot_loop`, `main`, `/usage/state`, POST `/usage/alert-rule`)
- Modify: `tests/test_usage_accounts.py`, `tests/test_usage_core.py`, `tests/test_usage_dash.py`
- Create: `tests/test_analytics_dash.py`

**Interfaces:**
- Consumes: tabla `usage_quota_snapshots` (Task 1).
- Produces: `cc_usage.record_quota_snapshots(db_path, rows: list[dict], now=None) -> int` (filas de límites con `id, provider, account, window, scope, percent, resets_at, captured_at`).
- Produces: `cc_usage.quota_snapshots(db_path, since=0) -> list[dict]` con `{provider, account, window, scope, resets_at, percent}`, ordenadas por `resets_at`.

- [ ] **Step 1: Write the failing tests**

Agregar a `tests/test_usage_accounts.py`:

```python
def test_quota_snapshots_keep_the_last_reading_of_each_cycle(tmp_path):
    db = tmp_path / "u.sqlite"
    row = {"id": "relotto:claude_weekly", "provider": "claude", "account": "relotto", "window": "7d", "scope": ""}
    cc_usage.record_quota_snapshots(db, [{**row, "percent": 40.0, "resets_at": 1791014400, "captured_at": 100}])
    # Mismo ciclo: el proveedor movió el reset 12 s; la lectura más nueva gana.
    cc_usage.record_quota_snapshots(db, [{**row, "percent": 91.0, "resets_at": 1791014412, "captured_at": 200}])
    cc_usage.record_quota_snapshots(db, [{**row, "percent": 50.0, "resets_at": 1791014400, "captured_at": 150}])
    cc_usage.record_quota_snapshots(db, [{**row, "id": "x", "window": "", "percent": 5.0, "resets_at": 1}, {**row, "percent": None, "resets_at": 9}])
    assert cc_usage.quota_snapshots(db) == [
        {"provider": "claude", "account": "relotto", "window": "7d", "scope": "", "resets_at": 1791014400, "percent": 91.0}]
```

Crear `tests/test_analytics_dash.py`:

```python
"""Cableado de Analytics en cc-dash: fotos de cuota, sin avisos, sin Reparto con Aplicar."""
from pathlib import Path

SRC = Path("bin/cc-dash").read_text()


def test_every_limits_read_saves_a_quota_snapshot():
    assert "cc_usage.record_quota_snapshots(USAGE_DB, claude_rows + codex_rows + grok_rows)" in SRC
    assert "threading.Thread(target=_limits_snapshot_loop, daemon=True).start()" in SRC


def test_quota_alerts_are_gone():
    for gone in ("_check_limit_alerts", "rule_alerts(", '"/usage/alert-rule"', "COMANDOS_ALERT_THRESHOLDS"):
        assert gone not in SRC, gone
```

- [ ] **Step 2: Run them to verify they fail**

Run: `pytest -q tests/test_usage_accounts.py -k quota tests/test_analytics_dash.py`
Expected: FAIL (`record_quota_snapshots` no existe; `_check_limit_alerts` sigue en cc-dash).

- [ ] **Step 3: Fotos en `bin/cc_usage.py`**

Agregar antes de `def _changed_files(`:

```python
def record_quota_snapshots(db_path, rows, now=None):
    """Última foto de cada límite (5 h / 7 d) por ciclo. El ciclo se identifica por su reset
    redondeado a la hora: el proveedor lo mueve unos segundos entre lecturas. Al cerrar el
    ciclo, la última foto dice cuánto se gastó; 100 - % es la cuota que sobró."""
    ts = int(now if now is not None else time.time())
    keep = []
    for r in rows or []:
        if r.get("percent") is None or not r.get("resets_at") or r.get("window") not in ("5h", "7d"):
            continue
        keep.append((_text(r.get("id")), _text(r.get("provider")), _text(r.get("account") or "main"),
                     _text(r.get("window")), _text(r.get("scope")), int(round(_as_int(r["resets_at"]) / 3600) * 3600),
                     float(r["percent"]), _as_int(r.get("captured_at")) or ts))
    if not keep:
        return 0
    init_db(db_path)
    with connect(db_path) as con:
        con.executemany(
            "insert into usage_quota_snapshots (limit_id, provider, account, win, scope, resets_at, percent, captured_at) "
            "values (?,?,?,?,?,?,?,?) on conflict(limit_id, resets_at) do update set "
            "percent=excluded.percent, captured_at=excluded.captured_at "
            "where excluded.captured_at >= usage_quota_snapshots.captured_at", keep)
    return len(keep)


def quota_snapshots(db_path, since=0):
    init_db(db_path)
    with connect(db_path) as con:
        rows = con.execute("select provider, account, win, scope, resets_at, percent from usage_quota_snapshots "
                           "where resets_at >= ? order by resets_at", (int(since),)).fetchall()
    return [{"provider": r[0], "account": r[1], "window": r[2], "scope": r[3], "resets_at": r[4], "percent": r[5]}
            for r in rows]
```

- [ ] **Step 4: cc-dash guarda fotos en cada lectura y cada 5 min**

En `_refresh_provider_limits` reemplazar:

```python
        _check_limit_alerts(claude_rows + codex_rows + [r for r in groq_rows if r.get("percent") is not None])
```
por:
```python
        try:
            cc_usage.record_quota_snapshots(USAGE_DB, claude_rows + codex_rows + grok_rows)
        except Exception:
            pass
```

Después de `def usage_provider_limits(...)`, agregar:

```python
def _limits_snapshot_loop():
    """Lee los límites cada 5 min aunque nadie abra el tablero: así la última foto de cada
    ciclo semanal queda a ≤5 min de su reset (Comparar → «cuota que sobra»)."""
    while True:
        try:
            usage_provider_limits()
        except Exception:
            pass
        time.sleep(300)
```

En `main()`, junto a los otros loops: `threading.Thread(target=_limits_snapshot_loop, daemon=True).start()`.

- [ ] **Step 5: Quitar los avisos de cuota**

En `bin/cc-dash`:
- Borrar `_check_limit_alerts` (~483) y `alert_thresholds` (~478).
- En el handler de `GET /usage/state` (~8511-8520), borrar la evaluación de reglas: `rule_current_values`, `rule_alerts`, `record_alert_once` y `usage_alert_send` de reglas. La respuesta deja de incluir `alert_rules` y `alert_thresholds`; `alerts` se queda con `list_alerts` para no romper a otros lectores.
- Borrar el handler POST `/usage/alert-rule` (~9194) y quitar `COMANDOS_ALERT_THRESHOLDS` de las claves que acepta `/usage/settings`.
- Borrar `rule_current_values` y `rule_alerts`.

En `bin/cc_usage.py`, borrar `limit_threshold_alerts`, `limit_alert_text`, `set_alert_rule`, `delete_alert_rule` y `parse_alert_thresholds`. Antes de borrar cada una, correr `grep -n "<nombre>" bin lib tests dash`: si queda un lector fuera de los avisos de cuota, esa función se conserva. Borrar también sus pruebas en `tests/test_usage_core.py`: parseo de umbrales, reglas, reglas `limit`, una vez por reset y texto del aviso. `usage_alert_send` y `record_alert_once` se quedan si `_maybe_tier_alert` u otro aviso los usa; comprobarlo con grep.

En `tests/test_usage_dash.py`, borrar las aserciones del cableado de avisos de cuota (~322-339).

- [ ] **Step 6: Run the tests to verify they pass**

Run: `pytest -q tests/test_usage_accounts.py tests/test_analytics_dash.py tests/test_usage_core.py tests/test_usage_dash.py tests/test_usage_telemetry.py tests/test_tier_alert.py`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add bin/cc_usage.py bin/cc-dash tests/test_usage_accounts.py tests/test_analytics_dash.py tests/test_usage_core.py tests/test_usage_dash.py
git commit -m "feat(usage): fotos de cuota por ciclo; fuera los avisos de 70/85/95 %"
```

---

### Task 4: Módulo de dibujo generado desde el mockup, con referencias pixel perfect

El tablero no reescribe el mockup: lo copia. El extractor toma del `<script>` del mockup 57 declaraciones de primer nivel, en su orden y sin tocarlas. Arriba les pone una capa que las alimenta con el modelo de datos y abajo la cabecera de `draw()`. El exportador saca del mockup sus datos de ejemplo, con la forma de `GET /analytics/week`, y el HTML exacto de 21 casos. La prueba de paridad exige que el tablero produzca ese mismo HTML byte por byte.

**Files:**
- Create: `tools/analytics_extract.cjs`, `tools/analytics_fixture.cjs`, `tests/analytics_parity_checks.cjs`, `tests/test_analytics_checks.py`
- Create (generados): `dash/analytics-render.js`, `tests/fixtures/analytics/week-normal.json`, `week-normal-prev.json`, `week-pesada.json`, `week-vacio.json`, `week-limite.json`, `reference.json`

**Interfaces:**
- Produces: `AnalyticsRender.create(model, {phone?: boolean, phoneDay?: number, minOffset?: number}) -> {html(tab: 'cuentas'|'comparar'|'pomodoro'): string, phoneDays(): number}`. Es `window.AnalyticsRender` en el navegador y `module.exports` en node.
- Produces el **contrato del modelo** (`GET /analytics/week`), igual a `tests/fixtures/analytics/week-normal.json`:
  - `week {offset, label, start, end, today|null, now|null, measuredAt}`
  - `days [[iso, wd, label]]`, del más nuevo al más viejo
  - `accounts [{id, provider, cli, alias, color, week|null, weekUsed|null, model|null {n, v, reset?, left?}, h5|null, reset, left, h5Reset, h5Left, hoy {h, tok, ses}, sem {h, tok, ses}}]`
  - `sessions [{d, acc, proj, st, en, tok}]`: horas locales; `tok` en millones
  - `lastWeek {proj: horas}`
  - `waste [{id, cyc: [% que sobró, del ciclo más nuevo al más viejo]}]`
  - `pomodoros [{d, st, en, plan, act, pause, status: completed|cancelled, proj}]`

- [ ] **Step 1: Write the failing parity test**

Crear `tests/analytics_parity_checks.cjs`:

```js
// Paridad pixel perfect: dash/analytics-render.js debe pintar EXACTAMENTE el HTML del mockup
// aprobado para los mismos datos. Las referencias salen de tools/analytics_fixture.cjs.
const assert = require('assert');
const fs = require('fs');
const path = require('path');
const { create } = require('../dash/analytics-render.js');
const dir = path.join(__dirname, 'fixtures', 'analytics');
const ref = JSON.parse(fs.readFileSync(path.join(dir, 'reference.json'), 'utf8'));
let n = 0;
for (const [key, want] of Object.entries(ref)) {
  const [week, tab, mode] = key.split('|');
  const model = JSON.parse(fs.readFileSync(path.join(dir, week + '.json'), 'utf8'));
  const got = create(model, { phone: mode === 'phone' }).html(tab);
  if (got !== want) {
    const i = [...got].findIndex((c, k) => c !== want[k]);
    assert.fail(`${key}: difiere en el carácter ${i}\n  mockup: ${want.slice(Math.max(0, i - 80), i + 80)}\n  tablero: ${got.slice(Math.max(0, i - 80), i + 80)}`);
  }
  n++;
}
assert.ok(n >= 21, `solo ${n} casos`);
console.log(`analytics parity: ${n} casos idénticos al mockup`);
```

Crear `tests/test_analytics_checks.py`:

```python
"""Corre desde pytest los checks node de Analytics (paridad con el mockup y la capa del tablero)."""
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
CHECKS = ["analytics_parity_checks.cjs"]


@pytest.mark.parametrize("name", CHECKS)
def test_node_check(name):
    if not shutil.which("node"):
        pytest.skip("sin node")
    subprocess.run(["node", str(ROOT / "tests" / name)], check=True, cwd=ROOT)
```

- [ ] **Step 2: Run it to verify it fails**

Run: `node tests/analytics_parity_checks.cjs`
Expected: FAIL with `Cannot find module '../dash/analytics-render.js'`.

- [ ] **Step 3: Crear el extractor**

Crear `tools/analytics_extract.cjs`:

```js
#!/usr/bin/env node
// Genera dash/analytics-render.js copiando TAL CUAL las piezas de dibujo del mockup aprobado
// (rama prototype/analytics-grill). Así el tablero pinta el mismo HTML que el mockup.
// Uso: git show prototype/analytics-grill:dash/prototypes/prototype-analytics.html > /tmp/proto.html
//      node tools/analytics_extract.cjs /tmp/proto.html dash/analytics-render.js
const fs = require('fs');
const [src, out] = process.argv.slice(2);
const html = fs.readFileSync(src, 'utf8');
const js = html.slice(html.indexOf('<script>') + 8, html.lastIndexOf('</script>'));
// Orden del mockup. Cada nombre es una declaración de primer nivel (const/let/function).
const NAMES = ['A_', 'fmtH', 'dur', 'defs', 'liquid', 'glass', 'floor', 'LOGO', 'limits', 'logoOf', 'liqC', 'colorLogo',
  'logoTile', 'brandBottle', 'capt', 'barShelf', 'dlab', 'accName', 'projTot', 'dayTot', 'weekHead', 'merged', 'lanes', 'tip2',
  'compAxis', 'calHead', 'axisHtml', 'gapBands', 'nowY', 'K1', 'usedFor', 'costData', 'fmtTok', 'franjas', 'vsWeek', 'sgn',
  'pHour', 'layeredBottle', 'wasteRest', 'insightCards', 'bottlesShelf', 'head2', 'C1', 'fmin', 'fstats', 'fByProj', 'fByHour',
  'PCOL', 'projColor', 'tomato', 'hh', 'pnums', 'pPerHour', 'pPerProj', 'S5', 'calendarCuentas', 'pomodoroPhone'];
const starts = [...js.matchAll(/^(?:const|let|function)\s+([A-Za-z_$][\w$]*)/gm)];
const decl = {};
starts.forEach((m, i) => {
  const end = i + 1 < starts.length ? starts[i + 1].index : js.length;
  // Quita el comentario de sección que precede a la siguiente declaración.
  const body = js.slice(m.index, end).replace(/(\n\/\*[^\n]*\*\/\s*)+$/, '\n').trimEnd();
  if (decl[m[1]]) throw new Error(`declaración repetida en el mockup: ${m[1]}`);
  decl[m[1]] = body;
});
const missing = NAMES.filter(n => !decl[n]);
if (missing.length) throw new Error('faltan en el mockup: ' + missing.join(', '));
const ADAPTER = `
  let uid = 0, phoneDay = 0;
  const ACC = AN.accounts.map(a => ({ ...a, c: a.color }));
  const accs = () => ACC.map(a => ({ ...a, hoy: { ...a.hoy }, sem: { ...a.sem }, model: a.model && { ...a.model } }));
  const DAYS = AN.days;
  const CHRONO = DAYS.slice().reverse();
  const sessions = () => AN.sessions;
  const LAST = AN.lastWeek;
  const WASTE = AN.waste;
  const H5 = Object.fromEntries(ACC.filter(a => a.h5Reset).map(a => [a.id, { left: a.h5Left, reset: a.h5Reset }]));
  const FOCUS = AN.pomodoros.map(f => ({ ...f, status: f.status === 'completed' ? 'completado' : 'cancelado', ag: {} }));
  const focusAll = () => FOCUS;
  const TODAYD = AN.week.today, NOWH = AN.week.now;
  const isPast = () => AN.week.offset < 0;
  const weekLabel = () => AN.week.label;
  const isPhone = () => !!view.phone;
`;
const SHELL = `
  const TABS = [['cuentas', 'Cuentas'], ['comparar', 'Comparar'], ['pomodoro', 'Pomodoro']];
  function body(tab) {
    const as = accs();
    if (tab === 'cuentas') return (isPast() ? '<div class="wk-note" style="margin:0 0 8px">Las botellas muestran tu cuota de ahora; el calendario es de la semana que elegiste.</div>' : '') + barShelf(as.filter(a => limits(a).length)) + \`<div class="sect"><h3>\${isPast() ? 'Semana' : 'Esta semana'}</h3><span>\${weekLabel()}</span></div>\` + calendarCuentas();
    if (tab === 'comparar') return C1();
    return isPhone() ? pomodoroPhone() : S5();
  }
  // Mismo marcado que draw() del mockup: cabecera con pestañas y flechas de semana, luego el cuerpo.
  function html(tab) {
    uid = 0;
    phoneDay = Math.max(0, Math.min(CHRONO.length - 3, view.phoneDay ?? CHRONO.length - 3));
    const off = AN.week.offset;
    const out = \`<div class="mhead"><h2>Analytics</h2><div class="tabs" role="tablist">\${TABS.map(([k, l]) => \`<button class="tab \${k === tab ? 'on' : ''}" role="tab" aria-selected="\${k === tab}" data-tab="\${k}">\${l}</button>\`).join('')}</div>
  <div class="wnav"><button data-w="-1" \${off <= (view.minOffset ?? -1) ? 'disabled' : ''} aria-label="Semana anterior">←</button><span>\${off ? 'Semana' : 'Esta semana'} · <b>\${weekLabel()}</b></span><button data-w="1" \${off >= 0 ? 'disabled' : ''} aria-label="Semana siguiente">→</button></div></div>\${body(tab)}\`;
    return out.replaceAll('🍅', \`<span class="tin">\${tomato(true, 16)}</span>\`);
  }
  return { html, phoneDays: () => CHRONO.length };
`;
const header = '/* GENERADO por tools/analytics_extract.cjs desde el mockup aprobado (rama prototype/analytics-grill).\n   No editar a mano: cambia el mockup, regenera y actualiza tests/fixtures/analytics. */\n';
const code = `${header}(function (root) {
  function create(AN, view = {}) {
${ADAPTER}
${NAMES.map(n => decl[n]).join('\n')}
${SHELL}
  }
  if (typeof module !== 'undefined' && module.exports) module.exports = { create };
  else root.AnalyticsRender = { create };
})(typeof window !== 'undefined' ? window : globalThis);
`;
fs.writeFileSync(out, code);
console.log(`${out}: ${NAMES.length} piezas, ${code.length} bytes`);
```

- [ ] **Step 4: Crear el exportador de referencias**

Crear `tools/analytics_fixture.cjs`:

```js
#!/usr/bin/env node
// Exporta del mockup aprobado (rama prototype/analytics-grill) los datos de ejemplo con la
// forma de GET /analytics/week y el HTML exacto que pinta cada caso. Los tests de paridad
// comparan dash/analytics.js contra estos archivos. Uso:
//   git show prototype/analytics-grill:dash/prototypes/prototype-analytics.html > /tmp/proto.html
//   node tools/analytics_fixture.cjs /tmp/proto.html tests/fixtures/analytics
// Casos: normal en semana actual y anterior, escritorio y celular; pesada, vacío y límite en escritorio.
const fs = require('fs'), vm = require('vm'), path = require('path');
const [src, outDir] = process.argv.slice(2);
const html = fs.readFileSync(src, 'utf8');
const js = html.slice(html.indexOf('<script>') + 8, html.lastIndexOf('</script>'));
const store = {};
const el = id => (store[id] ||= { id, html: '', style: {}, classList: { toggle() {} }, set innerHTML(v) { this.html = v; }, get innerHTML() { return this.html; },
  querySelectorAll: () => [], querySelector: () => el('x'), set onclick(_) {}, set textContent(_) {}, addEventListener() {}, getBoundingClientRect: () => ({}) });
const doc = { getElementById: el, querySelector: s => el(s), querySelectorAll: () => [], addEventListener() {} };
const ctx = { document: doc, localStorage: { getItem: () => null, setItem() {} }, addEventListener() {}, innerWidth: 1300, console, structuredClone };
vm.createContext(ctx);
vm.runInContext(js + `
;globalThis.__api = {
  model(st, off) {
    state = st; setWeek(off); const id = x => x;
    return { week: { offset: off, label: weekLabel(), start: DAYS.at(-1)[0], end: DAYS[0][0], today: off ? null : TODAYD, now: off ? null : NOWH, measuredAt: '08:16' },
      days: DAYS.map(x => [...x]),
      accounts: accs().map(a => ({ id: a.id, provider: a.provider, cli: a.cli, alias: a.alias, color: a.c, week: a.week, weekUsed: a.weekUsed === undefined ? a.week : a.weekUsed,
        model: a.model ? { n: a.model.n, v: a.model.v } : null, h5: a.h5, reset: a.reset, left: a.left,
        h5Reset: H5[a.id] ? H5[a.id].reset : null, h5Left: H5[a.id] ? H5[a.id].left : null, hoy: a.hoy, sem: a.sem })),
      sessions: sessions().map(s => ({ d: s.d, acc: s.acc, proj: s.proj, st: s.st, en: s.en, tok: (s.en - s.st) * TOKH[s.acc] })),
      lastWeek: LAST, waste: WASTE,
      pomodoros: focusAll().map(f => ({ d: f.d, st: f.st, en: f.en, plan: f.plan, act: f.act, pause: f.pause, status: f.status === 'completado' ? 'completed' : 'cancelled', proj: f.proj })) };
  },
  html(st, off, t, ph) { state = st; setWeek(off); tab = t; phone = ph; phoneDay = 5; draw(); return document.getElementById('modal').innerHTML; },
};`, ctx);
fs.mkdirSync(outDir, { recursive: true });
const ref = {};
const CASES = [['normal', 0, [false, true]], ['normal', -1, [false, true]], ['pesada', 0, [false]], ['vacio', 0, [false]], ['limite', 0, [false]]];
for (const [st, off, phones] of CASES) {
  const name = `week-${st}${off ? '-prev' : ''}`;
  fs.writeFileSync(path.join(outDir, name + '.json'), JSON.stringify(ctx.__api.model(st, off), null, 1) + '\n');
  for (const t of ['cuentas', 'comparar', 'pomodoro']) for (const ph of phones) ref[`${name}|${t}|${ph ? 'phone' : 'desk'}`] = ctx.__api.html(st, off, t, ph);
}
fs.writeFileSync(path.join(outDir, 'reference.json'), JSON.stringify(ref, null, 1) + '\n');
console.log('casos:', Object.keys(ref).length);
```

- [ ] **Step 5: Generar el módulo y las referencias**

```bash
PROTO=$(mktemp --suffix=.html)
git show prototype/analytics-grill:dash/prototypes/prototype-analytics.html > "$PROTO"
node tools/analytics_extract.cjs "$PROTO" dash/analytics-render.js
node tools/analytics_fixture.cjs "$PROTO" tests/fixtures/analytics
rm "$PROTO"
```
Expected: `dash/analytics-render.js: 57 piezas, …` y `casos: 21`.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `node tests/analytics_parity_checks.cjs && pytest -q tests/test_analytics_checks.py`
Expected: `analytics parity: 21 casos idénticos al mockup` y PASS.

- [ ] **Step 7: Commit**

```bash
git add tools/analytics_extract.cjs tools/analytics_fixture.cjs dash/analytics-render.js tests/fixtures/analytics tests/analytics_parity_checks.cjs tests/test_analytics_checks.py
git commit -m "feat(analytics): módulo de dibujo generado desde el mockup aprobado, con paridad de 21 casos"
```

---

### Task 5: Modelo de la semana a partir de los datos reales

**Files:**
- Create: `lib/analytics_week.py`, `tests/test_analytics_week.py`

**Interfaces:**
- Consumes: el contrato del modelo y `dash/analytics-render.js` (Task 4).
- Produces: `analytics_week.build_week(*, now: float, offset: int, limits: list[dict], turns: list[dict], spans: list[dict], snapshots: list[dict], records: list[dict], tz_name="America/Mexico_City") -> dict`.
  - `turns`: `{provider, account, git_root, pane_pwd, started, finished, tokens}`
  - `spans`: `{provider, account, git_root, started, finished}`
  - `snapshots`: salida de `cc_usage.quota_snapshots`
  - `records`: salida de `pomodoro.records`
- Produces: `analytics_week.account_id(provider, account) -> "proveedor:alias"`; `''` y `unknown` cuentan como `main`.

- [ ] **Step 1: Write the failing tests**

Crear `tests/test_analytics_week.py`:

```python
import json
import subprocess
import sys
from datetime import datetime
from pathlib import Path
from zoneinfo import ZoneInfo

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import analytics_week as aw  # noqa: E402

TZ = ZoneInfo("America/Mexico_City")


def ts(text):
    return datetime.fromisoformat(text).replace(tzinfo=TZ).timestamp()


NOW = ts("2026-10-01T08:16:00")
LIMITS = [
    {"provider": "claude", "account": "main", "window": "7d", "scope": "", "percent": 36.0, "resets_at": ts("2026-10-03T08:00:00")},
    {"provider": "claude", "account": "main", "window": "7d", "scope": "Fable", "percent": 31.0, "resets_at": ts("2026-10-03T08:00:00")},
    {"provider": "claude", "account": "main", "window": "5h", "scope": "", "percent": 6.0, "resets_at": ts("2026-10-01T10:40:00")},
    {"provider": "claude", "account": "relotto", "window": "7d", "scope": "", "percent": 59.0, "resets_at": ts("2026-10-02T23:00:00")},
    {"provider": "codex", "account": "main", "window": "7d", "scope": "", "percent": 53.0, "resets_at": ts("2026-10-03T21:58:00")},
    {"provider": "grok", "account": "main", "window": "7d", "scope": "", "percent": None, "resets_at": 0},
]


def turn(provider, account, root, at, tokens=1_000_000, started=None):
    return {"provider": provider, "account": account, "git_root": root, "pane_pwd": root,
            "started": started if started is not None else ts(at), "finished": ts(at), "tokens": tokens}


def test_days_and_label_follow_the_mockup():
    week = aw.build_week(now=NOW, offset=0, limits=[], turns=[], spans=[], snapshots=[], records=[])
    assert week["days"][0] == ["2026-10-01", "jue", "1 oct"]
    assert week["days"][1] == ["2026-09-30", "mié", "30"]
    assert week["days"][-1] == ["2026-09-24", "jue", "24"]
    assert week["week"]["label"] == "24 sep – 1 oct"
    assert week["week"]["today"] == "2026-10-01" and round(week["week"]["now"], 2) == 8.27
    prev = aw.build_week(now=NOW, offset=-1, limits=[], turns=[], spans=[], snapshots=[], records=[])
    assert prev["days"][0] == ["2026-09-23", "mié", "23 sep"] and prev["week"]["label"] == "16 – 23 sep"
    assert prev["week"]["today"] is None and prev["week"]["now"] is None


def test_accounts_have_one_bottle_per_limit_and_never_merge():
    week = aw.build_week(now=NOW, offset=0, limits=LIMITS, turns=[], spans=[], snapshots=[], records=[])
    by = {a["id"]: a for a in week["accounts"]}
    assert list(by) == ["claude:main", "claude:relotto", "codex:main"]
    main = by["claude:main"]
    assert (main["week"], main["h5"], main["model"]["n"], main["model"]["v"]) == (36, 6, "Fable", 31)
    assert main["reset"] == "sáb 3 oct, 08:00" and main["left"] == "1d 23h"
    assert main["h5Left"] == "2h 24m" and main["color"] == "#8B7CFF"
    assert by["claude:relotto"]["week"] == 59 and by["claude:relotto"]["h5"] is None
    assert by["codex:main"]["model"] is None


def test_sessions_merge_close_work_and_split_at_midnight():
    turns = [
        turn("claude", "main", "/x/ComandOS", "2026-09-30T23:40:00"),
        turn("claude", "main", "/x/ComandOS", "2026-10-01T00:20:00", tokens=3_000_000, started=ts("2026-09-30T23:50:00")),
        turn("claude", "relotto", "/x/ComandOS", "2026-10-01T00:10:00"),
        turn("claude", "main", "/x/ComandOS", "2026-10-01T02:00:00"),
    ]
    spans = [{"provider": "claude", "account": "main", "git_root": "/x/ComandOS",
              "started": ts("2026-09-30T23:30:00"), "finished": ts("2026-09-30T23:40:00")}]
    week = aw.build_week(now=NOW, offset=0, limits=LIMITS, turns=turns, spans=spans, snapshots=[], records=[])
    main = [s for s in week["sessions"] if s["acc"] == "claude:main"]
    assert [(s["d"], round(s["st"], 2), round(s["en"], 2)) for s in main] == [
        ("2026-09-30", 23.5, 24.0), ("2026-10-01", 0.0, 0.33), ("2026-10-01", 2.0, 2.0)]
    assert [round(s["tok"], 1) for s in main] == [1.0, 3.0, 1.0]
    assert [s["acc"] for s in week["sessions"] if s["acc"] != "claude:main"] == ["claude:relotto"]
    acc = {a["id"]: a for a in week["accounts"]}
    assert acc["claude:main"]["hoy"]["ses"] == 2 and acc["claude:relotto"]["sem"]["ses"] == 1


def test_unknown_account_counts_as_main_and_unlimited_accounts_still_exist():
    week = aw.build_week(now=NOW, offset=0, limits=[], turns=[turn("grok", "unknown", "/x/SAVA", "2026-10-01T01:00:00")],
                         spans=[], snapshots=[], records=[])
    assert [a["id"] for a in week["accounts"]] == ["grok:main"]
    assert week["accounts"][0]["week"] is None and week["sessions"][0]["acc"] == "grok:main"


def test_waste_and_past_week_use_closed_cycles_only():
    snaps = [
        {"provider": "grok", "account": "main", "window": "7d", "scope": "", "resets_at": ts("2026-09-26T20:00:00"), "percent": 12.0},
        {"provider": "grok", "account": "main", "window": "7d", "scope": "", "resets_at": ts("2026-09-19T20:00:00"), "percent": 8.0},
        {"provider": "grok", "account": "main", "window": "7d", "scope": "", "resets_at": ts("2026-10-03T20:00:00"), "percent": 6.0},
    ]
    limits = [{"provider": "grok", "account": "main", "window": "7d", "scope": "", "percent": 6.0, "resets_at": ts("2026-10-03T20:00:00")}]
    week = aw.build_week(now=NOW, offset=0, limits=limits, turns=[], spans=[], snapshots=snaps, records=[])
    assert week["waste"] == [{"id": "grok:main", "cyc": [88, 92]}]
    assert week["accounts"][0]["weekUsed"] == 6
    prev = aw.build_week(now=NOW, offset=-1, limits=limits, turns=[], spans=[], snapshots=snaps, records=[])
    assert prev["accounts"][0]["weekUsed"] == 12
    assert aw.build_week(now=NOW, offset=-1, limits=limits, turns=[], spans=[], snapshots=[], records=[])["accounts"][0]["weekUsed"] is None


def test_last_week_hours_come_from_the_previous_window():
    turns = [turn("codex", "main", "/x/SAVA", "2026-09-20T10:00:00", started=ts("2026-09-20T09:00:00")),
             turn("codex", "main", "/x/SAVA", "2026-09-30T10:00:00", started=ts("2026-09-30T09:30:00"))]
    week = aw.build_week(now=NOW, offset=0, limits=[], turns=turns, spans=[], snapshots=[], records=[])
    assert week["lastWeek"] == {"SAVA": 1.0}
    assert [s["d"] for s in week["sessions"]] == ["2026-09-30"]


def test_pomodoros_are_focus_blocks_with_clean_project_names():
    rec = lambda status, at, project, mode="focus": {  # noqa: E731
        "mode": mode, "status": status, "startedAtMs": ts(at) * 1000, "endedAtMs": ts(at) * 1000 + 25 * 60000,
        "targetMs": 25 * 60000, "plannedMs": 25 * 60000, "activeMs": 25 * 60000 if status == "completed" else 90000,
        "project": project}
    records = [rec("completed", "2026-10-01T01:29:00", "ComandOS ⎇ ⫽30"), rec("cancelled", "2026-09-30T14:15:00", "Signara ⫽42"),
               rec("completed", "2026-09-30T15:00:00", "x", mode="break"), rec("completed", "2026-09-01T10:00:00", "old")]
    week = aw.build_week(now=NOW, offset=0, limits=[], turns=[], spans=[], snapshots=[], records=records)
    assert [(p["d"], p["proj"], p["status"], p["plan"], p["act"]) for p in week["pomodoros"]] == [
        ("2026-09-30", "Signara", "cancelled", 25, 2), ("2026-10-01", "ComandOS", "completed", 25, 25)]


def test_model_has_the_fixture_shape_and_renders_every_tab():
    fixture = json.loads((ROOT / "tests/fixtures/analytics/week-normal.json").read_text())
    week = aw.build_week(now=NOW, offset=0, limits=LIMITS, turns=[turn("claude", "main", "/x/A", "2026-10-01T01:00:00")],
                         spans=[], snapshots=[], records=[])
    assert set(week) == set(fixture) and set(week["week"]) == set(fixture["week"])
    assert set(week["accounts"][0]) >= set(fixture["accounts"][0])
    assert set(week["sessions"][0]) == set(fixture["sessions"][0])
    script = ("const {create}=require('./dash/analytics-render.js');const m=JSON.parse(require('fs').readFileSync(0,'utf8'));"
              "for(const t of ['cuentas','comparar','pomodoro'])for(const phone of [false,true])create(m,{phone}).html(t);")
    for offset in (0, -1):
        model = aw.build_week(now=NOW, offset=offset, limits=LIMITS, turns=[], spans=[], snapshots=[], records=[])
        subprocess.run(["node", "-e", script], input=json.dumps(model), text=True, check=True, cwd=ROOT)
```

- [ ] **Step 2: Run them to verify they fail**

Run: `pytest -q tests/test_analytics_week.py`
Expected: FAIL with `ModuleNotFoundError: No module named 'analytics_week'`.

- [ ] **Step 3: Write the implementation**

Crear `lib/analytics_week.py`:

```python
"""Datos de la vista Analytics (Cuentas · Comparar · Pomodoro) para una ventana de 8 días.

Puro: recibe filas ya leídas y devuelve el modelo que pinta dash/analytics-render.js,
con la misma forma que tests/fixtures/analytics/week-*.json. Todo es POR CUENTA
(proveedor + alias); nunca se suman cuentas ni porcentajes de cuota entre cuentas.
"""
import os
import re
from datetime import datetime, timedelta
from zoneinfo import ZoneInfo

TZ = "America/Mexico_City"
WINDOW_DAYS = 8
GAP_S = 15 * 60
WASTE_CYCLES = 4
PROVIDERS = ("claude", "codex", "grok")
CLI = {"claude": "Claude", "codex": "Codex", "grok": "Grok"}
COLORS = {("claude", "main"): "#8B7CFF", ("claude", "relotto"): "#FF9A5C",
          ("codex", "main"): "#4CC2FF", ("grok", "main"): "#C5E35A"}
EXTRA_COLORS = ("#2fd3c0", "#FF6B5B", "#FFAE1A", "#FF9AD5", "#9AA6BF")
WD = ("lun", "mar", "mié", "jue", "vie", "sáb", "dom")
MON = ("ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic")


def account_of(account):
    """Alias limpio; las filas viejas sin cuenta cuentan como la principal."""
    alias = (account or "").strip()
    return "main" if alias in ("", "unknown") else alias


def account_id(provider, account):
    return f"{provider}:{account_of(account)}"


def project_of(path):
    path = (path or "").rstrip("/")
    return os.path.basename(path) if path else "Sin carpeta"


def pomodoro_project(name):
    """'ComandOS ⎇ ⫽30' -> 'ComandOS': quita rama y pane que añade el Pomodoro."""
    return re.split(r"\s+[⎇⫽]", name or "")[0].strip() or "Sin proyecto"


def fmt_left(seconds):
    s = max(0, int(seconds))
    d, h, m = s // 86400, s % 86400 // 3600, s % 3600 // 60
    if d:
        return f"{d}d {h}h"
    if h:
        return f"{h}h {m}m"
    return f"{m} min"


def fmt_reset(ts, tz):
    t = datetime.fromtimestamp(ts, tz)
    return f"{WD[t.weekday()]} {t.day} {MON[t.month - 1]}, {t:%H:%M}"


def window_dates(now, offset, tz):
    """Los 8 días de la ventana, del más nuevo al más viejo. offset -1 = los 8 anteriores."""
    end = datetime.fromtimestamp(now, tz).date() + timedelta(days=WINDOW_DAYS * offset)
    return [end - timedelta(days=i) for i in range(WINDOW_DAYS)]


def day_rows(dates):
    out = []
    for i, d in enumerate(dates):
        label = f"{d.day} {MON[d.month - 1]}" if i == 0 or d.day == 1 else str(d.day)
        out.append([d.isoformat(), WD[d.weekday()], label])
    return out


def week_label(dates):
    a, b = dates[-1], dates[0]
    if a.month == b.month:
        return f"{a.day} – {b.day} {MON[b.month - 1]}"
    return f"{a.day} {MON[a.month - 1]} – {b.day} {MON[b.month - 1]}"


def _intervals(turns, spans):
    """(cuenta, proyecto) -> [(inicio, fin, tokens)] de turnos y tramos medidos."""
    out = {}
    for t in turns:
        if t["provider"] not in PROVIDERS:
            continue
        key = (account_id(t["provider"], t.get("account")), project_of(t.get("git_root") or t.get("pane_pwd")))
        end = float(t["finished"])
        start = min(end, float(t.get("started") or end))
        out.setdefault(key, []).append((start, end, int(t.get("tokens") or 0)))
    for s in spans:
        if s["provider"] not in PROVIDERS:
            continue
        key = (account_id(s["provider"], s.get("account")), project_of(s.get("git_root")))
        end = float(s["finished"])
        out.setdefault(key, []).append((min(end, float(s["started"])), end, 0))
    return out


def sessions(turns, spans, tz):
    """Sesiones del calendario: tramos de la misma cuenta y proyecto separados por ≤15 min,
    partidos a medianoche local. Horas locales en st/en; tokens en millones."""
    out = []
    for (acc, proj), items in _intervals(turns, spans).items():
        items.sort()
        merged = []
        for start, end, tok in items:
            if merged and start - merged[-1][1] <= GAP_S:
                merged[-1][1] = max(merged[-1][1], end)
                merged[-1][2].append((end, tok))
            else:
                merged.append([start, end, [(end, tok)]])
        for start, end, toks in merged:
            cur = datetime.fromtimestamp(start, tz)
            stop = datetime.fromtimestamp(end, tz)
            while True:
                midnight = datetime.combine(cur.date() + timedelta(days=1), datetime.min.time(), tz)
                piece_end = min(stop, midnight)
                a, b = cur.timestamp(), piece_end.timestamp()
                tok = sum(t for at, t in toks if a <= at <= b and (at < b or piece_end == stop))
                st = cur.hour + cur.minute / 60 + cur.second / 3600
                en = st + (b - a) / 3600
                out.append({"d": cur.date().isoformat(), "acc": acc, "proj": proj,
                            "st": st, "en": en, "tok": tok / 1e6})
                if piece_end >= stop:
                    break
                cur = piece_end
    out.sort(key=lambda s: (s["d"], s["st"], s["acc"], s["proj"]))
    return out


def _limit_slots(limits):
    """Por cuenta: la fila semanal, la de modelo con más uso y la de 5 h."""
    slots = {}
    for row in limits:
        if row.get("provider") not in PROVIDERS or row.get("percent") is None:
            continue
        acc = account_id(row["provider"], row.get("account"))
        slot = slots.setdefault(acc, {"provider": row["provider"], "alias": account_of(row.get("account"))})
        if row.get("window") == "5h":
            slot["h5"] = row
        elif row.get("window") == "7d" and row.get("scope"):
            if "model" not in slot or row["percent"] > slot["model"]["percent"]:
                slot["model"] = row
        elif row.get("window") == "7d":
            slot["week"] = row
    return slots


def _cycles(snapshots, acc):
    rows = [s for s in snapshots if account_id(s["provider"], s["account"]) == acc
            and s["window"] == "7d" and not s.get("scope")]
    return sorted(rows, key=lambda s: -s["resets_at"])


def waste(snapshots, accounts, now):
    """Cuota que quedó sin usar en las últimas semanas cerradas, de la más nueva a la más vieja."""
    out = []
    for a in accounts:
        closed = [s for s in _cycles(snapshots, a["id"]) if s["resets_at"] <= now][:WASTE_CYCLES]
        out.append({"id": a["id"], "cyc": [round(100 - s["percent"]) for s in closed]})
    return out


def _used_at(snapshots, acc, window_end, now):
    """Uso final del ciclo semanal vigente al cerrar la ventana; None si aún no se sabe."""
    for s in reversed(_cycles(snapshots, acc)):
        if s["resets_at"] > window_end:
            return round(s["percent"]) if s["resets_at"] <= now else None
    return None


def build_accounts(limits, sess, today, window_end, snapshots, now, tz, past):
    slots = _limit_slots(limits)
    order = sorted(set(slots) | {s["acc"] for s in sess},
                   key=lambda a: (PROVIDERS.index(a.split(":")[0]), a.split(":")[1] != "main", a))
    out, extra = [], 0
    for acc in order:
        provider, alias = acc.split(":", 1)
        slot = slots.get(acc, {})
        color = COLORS.get((provider, alias))
        if color is None:
            color, extra = EXTRA_COLORS[extra % len(EXTRA_COLORS)], extra + 1
        week, model, h5 = slot.get("week"), slot.get("model"), slot.get("h5")
        mine = [s for s in sess if s["acc"] == acc]
        today_s = [s for s in mine if s["d"] == today]
        item = {
            "id": acc, "provider": provider, "cli": CLI[provider], "alias": alias, "color": color,
            "week": round(week["percent"]) if week else None,
            "model": ({"n": model["scope"], "v": round(model["percent"]),
                       "reset": fmt_reset(model["resets_at"], tz), "left": fmt_left(model["resets_at"] - now)}
                      if model else None),
            "h5": round(h5["percent"]) if h5 else None,
            "reset": fmt_reset(week["resets_at"], tz) if week else None,
            "left": fmt_left(week["resets_at"] - now) if week else None,
            "h5Reset": fmt_reset(h5["resets_at"], tz) if h5 else None,
            "h5Left": fmt_left(h5["resets_at"] - now) if h5 else None,
            "hoy": {"h": sum(s["en"] - s["st"] for s in today_s), "tok": round(sum(s["tok"] for s in today_s)),
                    "ses": len(today_s)},
            "sem": {"h": sum(s["en"] - s["st"] for s in mine), "tok": round(sum(s["tok"] for s in mine) / 1000, 1),
                    "ses": len(mine)},
        }
        item["weekUsed"] = _used_at(snapshots, acc, window_end, now) if past else item["week"]
        out.append(item)
    return out


def pomodoros(records, days, tz):
    keep = set(days)
    out = []
    for r in records:
        if r.get("mode") != "focus" or r.get("status") not in ("completed", "cancelled", "skipped"):
            continue
        start = datetime.fromtimestamp(r["startedAtMs"] / 1000, tz)
        if start.date().isoformat() not in keep:
            continue
        st = start.hour + start.minute / 60 + start.second / 3600
        ended = r.get("endedAtMs") or r["startedAtMs"] + (r.get("activeMs") or 0)
        out.append({"d": start.date().isoformat(), "st": st, "en": st + (ended - r["startedAtMs"]) / 3_600_000,
                    "plan": round((r.get("targetMs") or r.get("plannedMs") or 0) / 60000),
                    "act": round((r.get("activeMs") or 0) / 60000), "pause": 0,
                    "status": "completed" if r["status"] == "completed" else "cancelled",
                    "proj": pomodoro_project(r.get("project"))})
    out.sort(key=lambda f: (f["d"], f["st"]))
    return out


def build_week(*, now, offset, limits, turns, spans, snapshots, records, tz_name=TZ):
    tz = ZoneInfo(tz_name)
    dates = window_dates(now, offset, tz)
    prev = window_dates(now, offset - 1, tz)
    days = day_rows(dates)
    iso = {d[0] for d in days}
    all_sess = sessions(turns, spans, tz)
    sess = [s for s in all_sess if s["d"] in iso]
    prev_iso = {d.isoformat() for d in prev}
    last = {}
    for s in all_sess:
        if s["d"] in prev_iso:
            last[s["proj"]] = last.get(s["proj"], 0) + s["en"] - s["st"]
    window_end = datetime.combine(dates[0] + timedelta(days=1), datetime.min.time(), tz).timestamp()
    today = dates[0].isoformat() if offset == 0 else None
    accounts = build_accounts(limits, sess, today, window_end, snapshots, now, tz, offset < 0)
    local_now = datetime.fromtimestamp(now, tz)
    return {
        "week": {"offset": offset, "label": week_label(dates), "start": dates[-1].isoformat(), "end": dates[0].isoformat(),
                 "today": today, "now": (local_now.hour + local_now.minute / 60) if offset == 0 else None,
                 "measuredAt": f"{local_now:%H:%M}"},
        "days": days,
        "accounts": accounts,
        "sessions": sess,
        "lastWeek": {p: h for p, h in sorted(last.items())},
        "waste": waste(snapshots, accounts, now),
        "pomodoros": pomodoros(records, iso, tz),
    }
```

Reglas que el código implementa y las pruebas fijan:
- **Sesión:** tramos (turnos de `usage_turns` con `[inicio, fin]` y tramos medidos de `usage_spans`) de la misma cuenta y carpeta, separados por ≤15 min. Se parte en la medianoche local.
- **Tokens:** cada turno suma al tramo del día en que terminó.
- **Ventanas:** días de 8 en 8 que no se enciman. La semana anterior de hoy (1 oct) es 16–23 sep.
- **Botellas:** una por límite. La semanal es `window 7d` sin `scope`. La de modelo es `7d` con `scope` (la de más uso, si hay varias). La de sesión es `5h`. Una cuenta con sesiones pero sin límites existe para los colores y el calendario; la capa del tablero no le dibuja botellas.
- **Cuota que sobra:** solo ciclos semanales cerrados (`resets_at ≤ now`), máximo 4.
- **Semana anterior:** `weekUsed` es el uso final del ciclo vigente al cerrar esa semana; si ese ciclo no está cerrado o no hay foto, es `null`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `pytest -q tests/test_analytics_week.py`
Expected: PASS (8 tests). La última prueba dibuja con `dash/analytics-render.js` las tres pestañas, en escritorio y en celular, para modelos reales de esta semana y de la anterior, incluida la semana sin fotos.

- [ ] **Step 5: Commit**

```bash
git add lib/analytics_week.py tests/test_analytics_week.py
git commit -m "feat(analytics): modelo de la semana por cuenta desde turnos, tramos, fotos de cuota y pomodoros"
```

---

### Task 6: `GET /analytics/week` (con datos de ejemplo para la verificación visual)

**Files:**
- Modify: `bin/cc-dash` (import, `ANALYTICS_DEMO_DIR`, `analytics_week_payload`, `analytics_week_query`, ruta GET, `API_GET`)
- Create: `tests/test_analytics_endpoint.py`

**Interfaces:**
- Consumes: `analytics_week.build_week` (Task 5), `cc_usage.quota_snapshots` (Task 3), tablas `usage_turns`/`usage_spans` (Tasks 1–2), `pomodoro.records`, `usage_provider_limits()`.
- Produces: `GET /analytics/week?offset=0|-1[&demo=<nombre>]` → `200` con el modelo; `400` si el offset no es 0 ni -1; `404` si no existe la fixture `demo`. `demo` lee `tests/fixtures/analytics/week-<nombre>[-prev].json` del repo instalado.

- [ ] **Step 1: Write the failing tests**

Crear `tests/test_analytics_endpoint.py`:

```python
"""GET /analytics/week: el modelo de Analytics desde la base real, y los datos de ejemplo."""
import json
import time
from pathlib import Path

from test_usage_dash import load_dash_module

ROOT = Path(__file__).resolve().parents[1]


def test_week_reads_turns_spans_and_limits_per_account(tmp_path, monkeypatch):
    dash = load_dash_module()
    db = str(tmp_path / "usage.sqlite")
    monkeypatch.setattr(dash, "USAGE_DB", db)
    now = time.time()
    dash.cc_usage.record_turns(db, [{
        "id": "claude-jsonl-a", "provider": "claude", "agent": "claude", "tmux_session": "s", "tmux_pane": "",
        "pane_pwd": "/x/Relotto", "git_root": "/x/Relotto", "turn_started_at": int(now - 600), "turn_finished_at": int(now - 60),
        "total_tokens": 2_000_000, "harness_account": "relotto", "motor_account": "relotto", "source": "claude_jsonl", "confidence": "local"}])
    limits = [{"provider": "claude", "account": "relotto", "window": "7d", "scope": "", "percent": 59.0, "resets_at": int(now + 86400)}]
    monkeypatch.setattr(dash, "usage_provider_limits", lambda force=False: {"limits": limits, "health": {}})
    code, week = dash.analytics_week_query("/analytics/week?offset=0")
    assert code == 200
    assert [a["id"] for a in week["accounts"]] == ["claude:relotto"]
    assert week["accounts"][0]["week"] == 59
    # Un set: si la prueba corre justo después de medianoche, la sesión se parte en dos días.
    assert {(s["acc"], s["proj"]) for s in week["sessions"]} == {("claude:relotto", "Relotto")}
    assert dash.analytics_week_query("/analytics/week?offset=-1")[0] == 200


def test_week_rejects_other_offsets():
    dash = load_dash_module()
    assert dash.analytics_week_query("/analytics/week?offset=-2")[0] == 400
    assert dash.analytics_week_query("/analytics/week?offset=x")[0] == 400


def test_demo_serves_the_mockup_data_only_by_plain_name():
    dash = load_dash_module()
    code, week = dash.analytics_week_query("/analytics/week?demo=normal")
    assert code == 200
    assert week == json.loads((ROOT / "tests/fixtures/analytics/week-normal.json").read_text())
    assert dash.analytics_week_query("/analytics/week?demo=normal&offset=-1")[1]["week"]["offset"] == -1
    assert dash.analytics_week_query("/analytics/week?demo=../../etc")[0] == 404


def test_week_route_needs_the_token():
    src = Path("bin/cc-dash").read_text()
    assert '"/analytics/week"' in src[src.index("API_GET = ("):src.index("API_GET = (") + 4000]
```

- [ ] **Step 2: Run them to verify they fail**

Run: `pytest -q tests/test_analytics_endpoint.py`
Expected: FAIL with `AttributeError: module 'cc_dash_under_test' has no attribute 'analytics_week_query'`.

- [ ] **Step 3: Implementar en `bin/cc-dash`**

1. En el bloque de imports de `lib/` (~1351-1367), junto a `import pomodoro`, agregar `import analytics_week`.
2. Antes de `def pomodoro_report_query(path):` agregar:

```python
ANALYTICS_DEMO_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.realpath(__file__))),
                                  "tests", "fixtures", "analytics")
ANALYTICS_LOOKBACK_S = 17 * 86400   # 8 días de la semana anterior + 8 de esta + margen


def analytics_week_payload(offset, now=None):
    """El modelo de Analytics (lib/analytics_week.py) con los datos locales."""
    now = time.time() if now is None else now
    since = now - ANALYTICS_LOOKBACK_S
    cc_usage.init_db(USAGE_DB)
    with cc_usage.connect(USAGE_DB) as con:
        turns = [{"provider": r[0], "account": r[1], "git_root": r[2], "pane_pwd": r[3],
                  "started": r[4], "finished": r[5], "tokens": r[6]}
                 for r in con.execute(
                     "select provider, harness_account, git_root, pane_pwd, turn_started_at, turn_finished_at, total_tokens "
                     "from usage_turns where turn_finished_at >= ?", (since,))]
        spans = [{"provider": r[0], "account": r[1], "git_root": r[2], "started": r[3], "finished": r[4]}
                 for r in con.execute(
                     "select provider, account, git_root, started_at, finished_at from usage_spans where finished_at >= ?",
                     (since,))]
    records = pomodoro.records(pomodoro_store().conn, int(since * 1000), None)
    return analytics_week.build_week(
        now=now, offset=offset, limits=usage_provider_limits()["limits"], turns=turns, spans=spans,
        snapshots=cc_usage.quota_snapshots(USAGE_DB, since=now - 40 * 86400), records=records)


def analytics_week_query(path):
    """GET /analytics/week?offset=0|-1[&demo=nombre]: lo que pinta Analytics.
    demo= sirve los datos de ejemplo del mockup (tests/fixtures/analytics) para comparar
    el tablero con el mockup pixel por pixel."""
    query = urllib.parse.parse_qs(urllib.parse.urlsplit(path).query)
    try:
        offset = int((query.get("offset") or ["0"])[0])
    except ValueError:
        return 400, {"error": "offset inválido"}
    if offset not in (0, -1):
        return 400, {"error": "solo esta semana (0) o la anterior (-1)"}
    demo = re.sub(r"[^a-z]", "", (query.get("demo") or [""])[0])
    if demo:
        name = f"week-{demo}{'-prev' if offset else ''}.json"
        try:
            with open(os.path.join(ANALYTICS_DEMO_DIR, name), encoding="utf-8") as fh:
                return 200, json.load(fh)
        except OSError:
            return 404, {"error": "no hay datos de ejemplo con ese nombre"}
    return 200, analytics_week_payload(offset)
```

3. En `_do_GET`, junto a `/pomodoro/report`:

```python
        if self.path.startswith("/analytics/week"):
            return self._json(*analytics_week_query(self.path))
```

4. Agregar `"/analytics/week"` a la tupla `API_GET` (~8289).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `pytest -q tests/test_analytics_endpoint.py tests/test_usage_dash.py`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add bin/cc-dash tests/test_analytics_endpoint.py
git commit -m "feat(analytics): GET /analytics/week por cuenta, con los datos de ejemplo del mockup"
```

---

### Task 7: CSS del mockup bajo `.an` y las fuentes del diseño

El tablero no carga Inter ni Ubuntu Sans Mono: depende de que estén instaladas, y en la Mac y en el celular no lo están. El mockup carga de Google Fonts el subconjunto latino de las dos. El tablero servirá esos mismos dos archivos.

**Files:**
- Create: `tools/analytics_scope_css.py`, `dash/analytics.css` (generado), `assets/fonts/Inter/Inter-latin.woff2`, `assets/fonts/Inter/OFL.txt`, `assets/fonts/UbuntuSansMono/UbuntuSansMono-latin.woff2`, `assets/fonts/UbuntuSansMono/LICENCE.txt`
- Modify: `tests/test_analytics_dash.py`

**Interfaces:**
- Produces: `dash/analytics.css`: todas las reglas bajo `.an` (`.an.phone` en celular) y las familias `ComandOS Inter` y `ComandOS Ubuntu Sans Mono`.

- [ ] **Step 1: Write the failing test**

Agregar a `tests/test_analytics_dash.py`:

```python
import re

CSS = Path("dash/analytics.css")


def test_analytics_css_is_scoped_and_ships_its_fonts():
    css = CSS.read_text()
    body = re.sub(r"@font-face\{[^}]*\}", "", css)
    body = re.sub(r"@keyframes[^{]*\{(?:[^{}]*\{[^}]*\})*[^}]*\}", "", body)
    selectors = []
    for head in re.findall(r"([^{}]+)\{", body):
        head = head.strip()
        if head.startswith(("@media", "@supports", "/*")) or not head:
            continue
        selectors += [s.strip() for s in head.split(",")]
    assert selectors and all(s == ".an" or s.startswith((".an ", ".an.", ".an,")) for s in selectors), \
        [s for s in selectors if not (s == ".an" or s.startswith((".an ", ".an.", ".an,")))][:5]
    assert "--sans:'ComandOS Inter','Inter'" in css
    for font in ("assets/fonts/Inter/Inter-latin.woff2", "assets/fonts/UbuntuSansMono/UbuntuSansMono-latin.woff2"):
        assert font in css and Path(font).stat().st_size > 10_000
```

- [ ] **Step 2: Run it to verify it fails**

Run: `pytest -q tests/test_analytics_dash.py -k css`
Expected: FAIL with `FileNotFoundError: dash/analytics.css`.

- [ ] **Step 3: Crear el generador de CSS**

Crear `tools/analytics_scope_css.py`:

```python
#!/usr/bin/env python3
"""Genera dash/analytics.css desde el <style> del mockup aprobado de Analytics.

Cada regla queda bajo la raíz `.an`, así no toca el resto del tablero:
`:root`, `html,body` y `.modal` pasan a ser la propia raíz; `.phone` es una clase
de la raíz; lo que solo existe en el mockup (cabecera falsa, selector de diseños)
se descarta. Las fuentes se renombran a las que sirve el tablero (assets/fonts).

Uso: python3 tools/analytics_scope_css.py /tmp/proto.html > dash/analytics.css
"""
import re
import sys

ROOT = ".an"
MOCK_ONLY = re.compile(r"^(\*|\.app\b|\.hdr\b|\.picker\b|\.grip\b|\.states\b|\.desc\b)")
FONT_FACES = """@font-face{font-family:'ComandOS Inter';font-style:normal;font-weight:400 800;font-display:swap;src:url('assets/fonts/Inter/Inter-latin.woff2') format('woff2')}
@font-face{font-family:'ComandOS Ubuntu Sans Mono';font-style:normal;font-weight:400 700;font-display:swap;src:url('assets/fonts/UbuntuSansMono/UbuntuSansMono-latin.woff2') format('woff2')}
"""


def scope_selector(sel):
    sel = sel.strip()
    if sel in (":root", "html", "body"):
        return ROOT
    if sel == "*":
        return f"{ROOT},{ROOT} *"
    if MOCK_ONLY.match(sel):
        return None
    if sel.startswith(".modal"):
        return ROOT + sel[len(".modal"):]
    if sel.startswith(".phone"):
        return ROOT + sel
    return f"{ROOT} {sel}"


def block_end(css, open_at):
    depth, k = 1, open_at + 1
    while depth:
        depth += {"{": 1, "}": -1}.get(css[k], 0)
        k += 1
    return k


def scope(css):
    out, i = [], 0
    while i < len(css):
        j = css.find("{", i)
        if j < 0:
            break
        head = css[i:j].strip()
        if head.startswith("@media") or head.startswith("@supports"):
            k = block_end(css, j)
            out.append(f"{head}{{{scope(css[j + 1:k - 1])}}}\n")
            i = k
            continue
        if head.startswith("@keyframes"):
            k = block_end(css, j)
            out.append(css[i:k].strip() + "\n")
            i = k
            continue
        k = css.find("}", j)
        sels = [s for s in (scope_selector(x) for x in head.split(",")) if s]
        if sels:
            out.append(",".join(sels) + "{" + css[j + 1:k].strip() + "}\n")
        i = k + 1
    return "".join(out)


def main(path):
    html = open(path, encoding="utf-8").read()
    css = html[html.index("<style>") + 7:html.index("</style>")]
    css = re.sub(r"/\*.*?\*/", "", css, flags=re.S)
    css = css.replace("--sans:'Inter',", "--sans:'ComandOS Inter','Inter',")
    css = css.replace("--mono:'Ubuntu Sans Mono',", "--mono:'ComandOS Ubuntu Sans Mono','Ubuntu Sans Mono',")
    sys.stdout.write("/* GENERADO por tools/analytics_scope_css.py desde el mockup aprobado (rama prototype/analytics-grill). No editar a mano. */\n")
    sys.stdout.write(FONT_FACES)
    sys.stdout.write(scope(css))


if __name__ == "__main__":
    main(sys.argv[1])
```

- [ ] **Step 4: Generar el CSS y bajar las fuentes**

```bash
PROTO=$(mktemp --suffix=.html)
git show prototype/analytics-grill:dash/prototypes/prototype-analytics.html > "$PROTO"
python3 tools/analytics_scope_css.py "$PROTO" > dash/analytics.css
rm "$PROTO"
mkdir -p assets/fonts/Inter assets/fonts/UbuntuSansMono
# Los mismos archivos (subconjunto latino, fuente variable) que el mockup pide a Google Fonts.
curl -fsSL -o assets/fonts/Inter/Inter-latin.woff2 https://fonts.gstatic.com/s/inter/v20/UcC73FwrK3iLTeHuS_nVMrMxCp50SjIa1ZL7W0Q5nw.woff2
curl -fsSL -o assets/fonts/UbuntuSansMono/UbuntuSansMono-latin.woff2 https://fonts.gstatic.com/s/ubuntusansmono/v3/jVyR7mzgBHrR5yE7ZyRg0QRJMKI45grIfDfySZU.woff2
curl -fsSL -o assets/fonts/Inter/OFL.txt https://raw.githubusercontent.com/google/fonts/main/ofl/inter/OFL.txt
curl -fsSL -o assets/fonts/UbuntuSansMono/LICENCE.txt https://raw.githubusercontent.com/google/fonts/main/ufl/ubuntusansmono/LICENCE.txt
ls -l assets/fonts/Inter assets/fonts/UbuntuSansMono
```
Expected: Inter ≈ 48 KB y Ubuntu Sans Mono ≈ 21 KB. Si Google cambió las URLs, sacar las nuevas del bloque `/* latin */` de `curl -s -A 'Mozilla/5.0 (Macintosh) Chrome/126' 'https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&family=Ubuntu+Sans+Mono:wght@400;500;600;700&display=swap'`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `pytest -q tests/test_analytics_dash.py`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add tools/analytics_scope_css.py dash/analytics.css assets/fonts/Inter assets/fonts/UbuntuSansMono tests/test_analytics_dash.py
git commit -m "feat(analytics): CSS del mockup bajo .an y las fuentes del diseño servidas por el tablero"
```

---

### Task 8: La capa del tablero (`dash/analytics.js`)

**Files:**
- Create: `dash/analytics.js`, `tests/analytics_ui_checks.cjs`
- Modify: `tests/test_analytics_checks.py` (`CHECKS = ["analytics_parity_checks.cjs", "analytics_ui_checks.cjs"]`)

**Interfaces:**
- Consumes: `AnalyticsRender.create(...).html(tab)` (Task 4).
- Produces: `Analytics.create(el, {fetchWeek(offset) -> Promise<model>, tab?, onTab?(tab), width?() -> px, render?}) -> {open(tab?) -> Promise, load() -> Promise, paint(), state}`.
- Produces: `Analytics.tabName(nombreViejo) -> 'cuentas'|'comparar'|'pomodoro'`. Así los nombres viejos (`resumen`, `reparto`, `proveedores`…) que aún usan avisos, catálogo y `?tab=` abren la pestaña correcta.
- Comportamiento: celular si el ancho de la raíz es menor a 600 px. La semana anterior es el offset -1, que es el mínimo. Un error de red no borra la vista que ya está.

- [ ] **Step 1: Write the failing UI check**

Crear `tests/analytics_ui_checks.cjs`:

```js
// La capa del tablero sobre el mockup: datos por semana, pestañas, flechas, celular y +N.
const assert = require('assert');
const fs = require('fs');
const path = require('path');
const { mkRoot } = require('./dom_stub.cjs');
const render = require('../dash/analytics-render.js');
const { create, tabName } = require('../dash/analytics.js');

const fx = name => JSON.parse(fs.readFileSync(path.join(__dirname, 'fixtures', 'analytics', name + '.json'), 'utf8'));
const ref = JSON.parse(fs.readFileSync(path.join(__dirname, 'fixtures', 'analytics', 'reference.json'), 'utf8'));
const tail = '<div class="tip" hidden></div><div class="pop" hidden></div>';
globalThis.innerWidth = 1300; globalThis.innerHeight = 900;

(async () => {
  assert.strictEqual(tabName('resumen'), 'cuentas');
  assert.strictEqual(tabName('reparto'), 'cuentas');
  assert.strictEqual(tabName('proveedores'), 'comparar');
  assert.strictEqual(tabName('pomodoro'), 'pomodoro');
  assert.strictEqual(tabName('nada'), 'cuentas');

  const asked = [];
  let width = 1100;
  const el = mkRoot();
  const an = create(el, { render, width: () => width, fetchWeek: async off => { asked.push(off); return fx(off ? 'week-normal-prev' : 'week-normal'); } });

  await an.open('comparar');
  assert.deepStrictEqual(asked, [0]);
  assert.strictEqual(el.innerHTML, ref['week-normal|comparar|desk'] + tail, 'Comparar = mockup');

  el.click('[data-tab="pomodoro"]');
  assert.strictEqual(el.innerHTML, ref['week-normal|pomodoro|desk'] + tail, 'Pomodoro = mockup');
  assert.deepStrictEqual(asked, [0], 'cambiar de pestaña no vuelve a pedir datos');

  el.click('[data-tab="cuentas"]');
  el.click('[data-w="-1"]');
  await an.state.loading;
  assert.deepStrictEqual(asked, [0, -1]);
  assert.strictEqual(el.innerHTML, ref['week-normal-prev|cuentas|desk'] + tail, 'semana anterior = mockup');
  el.click('[data-w="-1"]');
  assert.deepStrictEqual(asked, [0, -1], 'no hay semana más vieja que -1');

  el.click('[data-w="1"]');
  await an.state.loading;
  width = 390;
  an.paint();
  assert.ok(el.classList.contains('phone'), 'angosto = celular');
  assert.strictEqual(el.innerHTML, ref['week-normal|cuentas|phone'] + tail, 'celular = mockup');
  el.click('[data-pd="-1"]');
  assert.ok(el.innerHTML.includes('lun 28 · mar 29 · mié 30'), 'la flecha del calendario muestra días anteriores');

  // Un error de red no borra lo que ya se ve.
  const before = el.innerHTML;
  const bad = create(mkRoot(), { render, width: () => 1100, fetchWeek: async () => { throw new Error('sin red'); } });
  await bad.open();
  assert.ok(bad.state.error.includes('sin red'));
  assert.strictEqual(el.innerHTML, before);

  console.log('analytics ui: ok');
})().catch(e => { console.error(e); process.exit(1); });
```

Agregar `"analytics_ui_checks.cjs"` a `CHECKS` en `tests/test_analytics_checks.py`.

- [ ] **Step 2: Run it to verify it fails**

Run: `node tests/analytics_ui_checks.cjs`
Expected: FAIL with `Cannot find module '../dash/analytics.js'`.

- [ ] **Step 3: Write the implementation**

Crear `dash/analytics.js`:

```js
// Analytics del tablero (grill 1–2 oct): Cuentas · Comparar · Pomodoro.
// El marcado sale de dash/analytics-render.js (copia exacta del mockup aprobado);
// aquí solo van los datos (GET /analytics/week), las pestañas, la semana, el celular y los tooltips.
(function (root) {
  const TABS = ['cuentas', 'comparar', 'pomodoro'];
  // Nombres viejos que aún llegan por openAnalyticsTab (avisos, catálogo del operador, ?tab=).
  const ALIASES = { resumen: 'cuentas', guardia: 'cuentas', alertas: 'cuentas', reparto: 'cuentas',
    proyectos: 'comparar', proveedores: 'comparar', comparar: 'comparar', pomodoro: 'pomodoro', cuentas: 'cuentas' };
  const MIN_OFFSET = -1;
  const PHONE_PX = 600;

  function tabName(name) { return ALIASES[String(name || '').toLowerCase()] || 'cuentas'; }

  function create(el, opts) {
    const render = opts.render || root.AnalyticsRender;
    const width = opts.width || (() => el.getBoundingClientRect().width);
    const S = { tab: tabName(opts.tab), offset: 0, phoneDay: null, model: null, error: '', loading: null };
    const phone = () => width() < PHONE_PX;

    function paint() {
      el.classList.toggle('phone', phone());
      if (!S.model) {
        el.innerHTML = `<div class="mhead"><h2>Analytics</h2></div><p class="dim">${S.error || 'Leyendo el uso…'}</p>`;
        return;
      }
      const view = render.create(S.model, { phone: phone(), phoneDay: S.phoneDay, minOffset: MIN_OFFSET });
      el.innerHTML = view.html(S.tab) + '<div class="tip" hidden></div><div class="pop" hidden></div>';
      if (phone()) addScrollDots();
    }

    // Celular: la repisa se desliza de lado; los puntos dicen qué cuenta estás viendo.
    function addScrollDots() {
      el.querySelectorAll('.bar-row').forEach(row => {
        const bar = row.closest('.bar');
        if (!bar || typeof bar.insertAdjacentHTML !== 'function') return;
        const n = row.children.length;
        bar.insertAdjacentHTML('afterend', `<div class="sdots">${Array.from({ length: n }, (_, i) => `<i class="${i ? '' : 'on'}"></i>`).join('')}</div>`);
        const dots = bar.nextElementSibling;
        row.addEventListener('scroll', () => {
          const i = Math.round(row.scrollLeft / row.clientWidth);
          dots.querySelectorAll('i').forEach((d, j) => d.classList.toggle('on', j === i));
        }, { passive: true });
      });
    }

    async function load() {
      const offset = S.offset;
      const job = opts.fetchWeek(offset).then(model => {
        if (offset !== S.offset) return;
        S.model = model; S.error = '';
      }, err => {
        if (offset !== S.offset) return;
        if (!S.model) S.error = `No pude leer el uso: ${err && err.message ? err.message : err}`;
      });
      S.loading = job;
      await job;
      if (offset === S.offset) paint();
    }

    function open(name) {
      if (name) S.tab = tabName(name);
      paint();
      return load();
    }

    function showPop(btn) {
      const pop = el.querySelector('.pop');
      const rows = JSON.parse(decodeURIComponent(btn.dataset.pop));
      pop.innerHTML = `<h6>${btn.dataset.title}</h6>` + rows.map(r => `<div><span class="dot" style="background:${r[3]}"></span><b>${r[1]}</b><span>${r[0]}</span><em>${r[2]}</em></div>`).join('');
      pop.hidden = false;
      const rc = btn.getBoundingClientRect();
      pop.style.left = Math.max(8, Math.min(innerWidth - pop.offsetWidth - 10, rc.right + 8)) + 'px';
      pop.style.top = Math.min(innerHeight - pop.offsetHeight - 10, rc.top) + 'px';
    }

    el.addEventListener('click', e => {
      const t = e.target;
      const tab = t.closest('[data-tab]');
      if (tab) { S.tab = tabName(tab.dataset.tab); opts.onTab && opts.onTab(S.tab); paint(); return; }
      const week = t.closest('[data-w]');
      if (week && !week.disabled && !week.hasAttribute('disabled')) {
        S.offset = Math.max(MIN_OFFSET, Math.min(0, S.offset + Number(week.dataset.w)));
        S.phoneDay = null;
        load();
        return;
      }
      const day = t.closest('[data-pd]');
      if (day && S.model) {
        const last = S.model.days.length - 3;
        const cur = S.phoneDay == null ? last : S.phoneDay;
        S.phoneDay = Math.max(0, Math.min(last, cur + Number(day.dataset.pd)));
        paint();
        return;
      }
      const more = t.closest('[data-pop]');
      if (more) { e.stopPropagation(); showPop(more); return; }
      const pop = el.querySelector('.pop');
      if (pop && !t.closest('.pop')) pop.hidden = true;
    });

    el.addEventListener('pointerover', e => {
      const tip = el.querySelector('.tip');
      if (!tip) return;
      const src = e.target.closest('[data-tip]');
      if (!src) { tip.hidden = true; return; }
      const [a, b, c] = src.dataset.tip.split('|');
      tip.innerHTML = `<b>${b}</b><span>${a}</span><em>${c}</em>`;
      tip.hidden = false;
    });
    el.addEventListener('pointermove', e => {
      const tip = el.querySelector('.tip');
      if (!tip || tip.hidden) return;
      const x = Math.min(innerWidth - tip.offsetWidth - 8, e.clientX + 14);
      const y = e.clientY + 18 + tip.offsetHeight > innerHeight ? e.clientY - tip.offsetHeight - 10 : e.clientY + 18;
      tip.style.left = x + 'px';
      tip.style.top = y + 'px';
    });

    return { open, load, paint, state: S };
  }

  const api = { create, tabName, TABS };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.Analytics = api;
})(typeof window !== 'undefined' ? window : globalThis);
```

- [ ] **Step 4: Run the checks to verify they pass**

Run: `node tests/analytics_ui_checks.cjs && pytest -q tests/test_analytics_checks.py`
Expected: `analytics ui: ok` y PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add dash/analytics.js tests/analytics_ui_checks.cjs tests/test_analytics_checks.py
git commit -m "feat(analytics): capa del tablero — pestañas, semana anterior, celular, tooltips y +N"
```

---

### Task 9: Montar Analytics en el tablero y retirar la vista vieja

`dash/index.html` cambia mientras otra sesión trabaja en `main`. Por eso esta tarea nombra funciones y no líneas: cada borrado se confirma con `grep`. `tickUsage` y `GET /usage/state` se quedan, porque alimentan las fichas de las sesiones (`indexUsage`, `S.usageByPane`, `workspace.js`).

**Files:**
- Modify: `dash/index.html`, `dash/pomodoro.js`, `dash/workspace.css`, `install.sh`
- Delete: `dash/reparto.js`, `dash/reparto.css`, `tests/test_dashboard_reparto.py`
- Create: `tools/css_orphans.py`
- Modify (pruebas): `tests/test_analytics_dash.py`, `tests/test_usage_ui.py`, `tests/test_usage_dash.py`, `tests/test_header_layout.py`, `tests/test_remote_ui.py`, `tests/pomodoro_ui_checks.cjs`

**Interfaces:**
- Consumes: `Analytics.create` (Task 8), `GET /analytics/week` (Task 6), `dash/analytics.css` (Task 7).
- Produces: `window.openAnalyticsTab(tab)` (nombres nuevos y viejos), `openAnalytics(tab?)`, raíz `#an-root.an` dentro de `#usage.modal`.

- [ ] **Step 1: Write the failing tests**

Agregar a `tests/test_analytics_dash.py`:

```python
HTML = Path("dash/index.html").read_text()


def test_usage_modal_is_the_approved_analytics():
    start = HTML.index('<div id="usage"')
    block = HTML[start:start + 300]
    assert '<div class="an" id="an-root"></div>' in block
    assert 'href="analytics.css' in HTML
    assert HTML.index('src="analytics-render.js') < HTML.index('src="analytics.js')
    assert "window.openAnalyticsTab = tab => openAnalytics(tab);" in HTML
    assert "/analytics/week?offset=" in HTML


def test_old_analytics_is_gone():
    for gone in ("data-mpane=\"resumen\"", "data-mpane=\"guardia\"", "data-mpane=\"alertas\"", "data-mpane=\"reparto\"",
                 "renderQuotaHero", "renderUsageLimits", "limitCardKit", "wireQuotaEditors", "loadGuard", "guardPoll",
                 "renderLedger", "loadCompare", "renderCompare", "loadProvCompare", "renderProvCompare", "pcWire",
                 "renderAlertConfig", "openRuleMenu", "renderAlertRules", "renderDedication", "compareSetDays",
                 "setLimitStyle", "usage-refresh", "cc-guard-alert-key", "reparto.js", "reparto.css",
                 "/usage/guard", "/usage/provider-compare", "/usage/analytics", "/usage/alert-rule", "/allocation/"):
        assert gone not in HTML, gone
    pomo = Path("dash/pomodoro.js").read_text()
    assert "loadAnalytics" not in pomo and "/pomodoro/report" not in pomo
    assert not Path("dash/reparto.js").exists() and not Path("dash/reparto.css").exists()


def test_install_links_the_new_files():
    install = Path("install.sh").read_text()
    for name in ("analytics.js", "analytics-render.js", "analytics.css"):
        assert f" {name}" in install
    assert "reparto.js" not in install and "reparto.css" not in install
```

- [ ] **Step 2: Run them to verify they fail**

Run: `pytest -q tests/test_analytics_dash.py`
Expected: FAIL in `test_usage_modal_is_the_approved_analytics`, `test_old_analytics_is_gone` and `test_install_links_the_new_files`.

- [ ] **Step 3: Marcado, estilos y scripts**

En `dash/index.html`:
1. Reemplazar todo el bloque `<div id="usage" class="modal" …> … </div>` (hoy desde `<div id="usage"` hasta el cierre del modal, después del pane `alertas`) por:

```html
<div id="usage" class="modal" aria-label="Analytics" role="dialog" aria-modal="true">
  <div class="an" id="an-root"></div>
</div>
```
2. En `<head>`: `<link rel="stylesheet" href="reparto.css?v=fuentesD">` → `<link rel="stylesheet" href="analytics.css?v=an1">`.
3. Después del script inline principal: `<script src="reparto.js"></script>` → `<script src="analytics-render.js?v=an1"></script>` seguido de `<script src="analytics.js?v=an1"></script>`.
4. Reglas de `?panel=usage` (~350-352): borrar la de `#usage .modal-panel{…}` y cambiar la de `#usage` por `html[data-only-panel="usage"] body.only-panel #usage{position:static!important;display:flex!important;background:var(--bg)!important;padding:14px 16px!important}`, con el mismo margen que el mockup.
5. Junto a las reglas de `.modal` (~968), agregar `#usage > .an{width:100%}` y `#usage{-webkit-user-select:text;user-select:text}`. El overlay es flex, así que sin el ancho la raíz se encogería a su contenido; en el mockup llena hasta sus 1120 px.

- [ ] **Step 4: Abrir Analytics**

En el script inline principal, junto a `window.openAnalyticsTab` (~8094), reemplazar `window.openAnalyticsTab = …` y `window.compareSetDays = …` por:

```js
	// ---------- Analytics (grill 1–2 oct): Cuentas · Comparar · Pomodoro ----------
	// El marcado es el del mockup aprobado (dash/analytics-render.js, generado); aquí solo se abre.
	const ANALYTICS_DEMO = (new URLSearchParams(location.search).get("demo") || "").replace(/[^a-z]/g, "");
	let analyticsView = null, analyticsPhone = null;
	function analytics(){
	  if(!analyticsView && window.Analytics){
	    let saved = "cuentas";
	    try{ saved = localStorage.getItem("cc-analytics-tab") || saved; }catch(e){}
	    analyticsView = window.Analytics.create($("#an-root"), {
	      tab: saved,
	      fetchWeek: off => api(`/analytics/week?offset=${off}${ANALYTICS_DEMO ? `&demo=${ANALYTICS_DEMO}` : ""}`),
	      onTab: t => { try{ localStorage.setItem("cc-analytics-tab", t); }catch(e){} },
	    });
	  }
	  return analyticsView;
	}
	function openAnalytics(tab){
	  const m = $("#usage");
	  document.querySelectorAll(".modal.open").forEach(x => { if(x !== m) x.classList.remove("open"); });
	  m.classList.add("open");
	  analytics()?.open(tab);
	}
	window.openAnalyticsTab = tab => openAnalytics(tab);
	// Mientras está abierto, los datos se releen cada minuto; el ancho decide escritorio o celular.
	setInterval(()=>{ if($("#usage")?.classList.contains("open")) analytics()?.load(); }, 60000);
	new ResizeObserver(()=>{
	  if(!analyticsView || !$("#usage").classList.contains("open")) return;
	  const phone = $("#an-root").getBoundingClientRect().width < 600;
	  if(phone !== analyticsPhone){ analyticsPhone = phone; analyticsView.paint(); }
	}).observe($("#usage"));
```

Reemplazar el handler de `#btn-usage` (~7698) por:

```js
	$("#btn-usage").addEventListener("click", ()=>{
	  if($("#usage").classList.contains("open")){ $("#usage").classList.remove("open"); return; }
	  openAnalytics();
	});
```

En el bloque `ONLY_PANEL` (~2518), reemplazar la rama `usage` por:

```js
  if(ONLY_PANEL==="usage"){ openAnalytics(new URLSearchParams(location.search).get("tab") || ""); return; }
```

Se cierra como cualquier modal: Esc (`.modal.open`) o clic fuera de la raíz (`e.target === m`). El mockup no tiene botón ✕.

- [ ] **Step 5: Retirar la vista vieja de `dash/index.html`**

1. Reducir `renderUsage(state)` a:

```js
	function renderUsage(state){
	  indexUsage(state);
	  renderLimitsStrip(state);
	  render(S.list || []);
	}
```
2. En `tickUsage`, quitar la llamada a `renderDedication()`.
3. Borrar estas funciones y constantes completas: `renderQuotaHero`, `usageSummaryText`, `renderUsageLimits`, `limitCardKit`, `wireQuotaEditors`, `setLimitStyle`, `limitStyle`, `limitRingSvg`, `limitMetricHtml`, `limitInsight`, `LIMIT_WINDOW_SEC`, `limitSubText`, `loadGuard` (y su envoltura que repinta el héroe), `renderGuard`, `burnChartSvg`, `renderLedger`, `tierRank`, `guardPoll` (y su `setInterval`), `GUARD_CACHE`, `loadCompare`, `renderCompare`, `loadProvCompare`, `renderProvCompare`, `pcDailyInsight`, `pcDailyChart`, `pcEstDailyChart`, `pcHeatmap`, `pcCompositionChart`, `pcScatterChart`, `pcModelTable`, `pcRoiBlock`, `pcEstimateChart`, `pcModelsCsv`, `pcWire`, `SUBS_CURRENCIES`, `renderAlertConfig` (y el click de `#alert-config`), `RULE_BUDGETS`, `RULE_PERCENTS`, `openRuleMenu`, `renderAlertRules`, `renderDedication`, `dedTs`. Borrar también el hook `DOMContentLoaded` de los controles de comparar, los handlers de `#pc-days` y `#usage-refresh`, y `window.compareSetDays`.
   Antes de borrar cada helper de límites (`fmtPercent`, `limitSeverity`, `fmtReset`, `limitAge`, `fmtResetRel`), correr `grep -n "<nombre>(" dash/index.html dash/*.js`. Si `renderLimitsStrip` u otro código vivo lo usa, se queda.
4. Campana de avisos: en `nfPriorityItems` borrar las tarjetas de «desborde» que salen de la Guardia, y en el handler de acciones del panel borrar las ramas `guardia` y `ahorro`.
5. Pestañas: en `MTAB_GROUPS` borrar las entradas de Analytics (`resumen`, `comparar`, `reparto`) y en `activateMtab` las ramas `resumen` (loadGuard), `comparar`, `reparto` y `pomodoro`. `activateMtab` y `wireMtabs` siguen sirviendo a Ajustes.
6. CSS: correr `python3 tools/css_orphans.py .usage- .compare- .sec-title .guard .lat- .insight .opt- .cmp- .rating .uw- .uv- .pc- .qh- .rule- .alert #usage- #guard #quota #alert #pc- #ded` (crear antes `tools/css_orphans.py` con el contenido de abajo). Borrar del `<style>` cada regla cuyo selector aparezca en la lista y volver a correr el comando hasta que salga vacío.

`tools/css_orphans.py`:

```python
#!/usr/bin/env python3
"""Lista las clases e ids del <style> de dash/index.html que ya nadie usa en dash/.

Uso: python3 tools/css_orphans.py [prefijo ...]
Sin prefijos lista todo; con prefijos (p. ej. .uw- .qh- #usage-) solo esos.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
html = (ROOT / "dash" / "index.html").read_text(encoding="utf-8")
style = "\n".join(re.findall(r"<style>(.*?)</style>", html, flags=re.S))
rest = re.sub(r"<style>.*?</style>", "", html, flags=re.S)
for path in sorted((ROOT / "dash").glob("*.js")):
    rest += path.read_text(encoding="utf-8")
names = sorted(set(re.findall(r"([.#][A-Za-z][\w-]*)", re.sub(r"\{[^{}]*\}", "{}", style))))
wanted = sys.argv[1:]
for name in names:
    if wanted and not any(name.startswith(p) for p in wanted):
        continue
    bare = name[1:]
    if not re.search(r"(?<![\w-])" + re.escape(bare) + r"(?![\w-])", rest):
        print(name)
```

- [ ] **Step 6: Pomodoro, Reparto e instalación**

- `dash/pomodoro.js`: borrar el bloque de stats viejas (`an`, `periodDates`, `loadAnalytics`, `renderAnalytics`) y sus exports (`.loadAnalytics`, `.analytics`). El botón `data-pm-analytics` sigue llamando `openAnalyticsTab('pomodoro')`.
- `dash/workspace.css`: borrar los estilos de `#pomodoro-analytics` (~271-293).
- `git rm dash/reparto.js dash/reparto.css tests/test_dashboard_reparto.py`.
- `install.sh:71`: en la lista `for f in …`, quitar `reparto.js reparto.css` y agregar `analytics.js analytics-render.js analytics.css`.

- [ ] **Step 7: Actualizar las pruebas viejas**

- `tests/test_usage_ui.py`: borrar las pruebas de la vista vieja: ids de `#usage` y de comparar (~10-31); guardia/ahorro en `nfPriorityItems` (~41-66); campanas, `RULE_*` y `#alert-config` (~249-270); tarjetas de límites y `/usage/quota` (~309-340); estilos de tarjeta, `pc*` y `optimizar` (~401-464). Lo demás se queda.
- `tests/test_usage_dash.py`: borrar las aserciones de `/allocation/*` en `reparto.js` (~111-123) y de `#guard-ledger`/`data-undo=` (~296-306).
- `tests/test_header_layout.py` (~230-243): borrar las aserciones de `.compare-stats`/`.qh-row` bajo `@container modal` y de `#pomodoro-analytics` en `workspace.css`.
- `tests/test_remote_ui.py` (~3377-3384): conservar `window.openAnalyticsTab = ` y quitar `setLimitStyle` y `compareSetDays`.
- `tests/pomodoro_ui_checks.cjs` (~325-341): borrar los checks de `loadAnalytics`/`/pomodoro/report`.

- [ ] **Step 8: Run the tests to verify they pass**

Run:
```bash
bash tests/test_js_parses.sh
pytest -q tests/test_analytics_dash.py tests/test_usage_ui.py tests/test_usage_dash.py tests/test_header_layout.py tests/test_remote_ui.py tests/test_dashboard_markup.py tests/test_dashboard_assets.py tests/test_state_polling.py tests/test_button_styles.py tests/test_analytics_checks.py
node tests/pomodoro_ui_checks.cjs && node tests/notification_ui_checks.cjs
```
Expected: `OK` and PASS.

- [ ] **Step 9: Commit**

```bash
git add -A dash/index.html dash/pomodoro.js dash/workspace.css install.sh tools/css_orphans.py tests
git commit -m "feat(analytics): el modal es Cuentas · Comparar · Pomodoro; fuera Guardia, Alertas, Reparto y el Comparar viejo"
```

---

### Task 10: Retirar Reparto (propuesta, aplicar, arrastrar) del servidor y del catálogo

**Files:**
- Modify: `bin/cc-dash` (rutas `/allocation/*` y su lógica), `lib/allocation.py`, `lib/operator_catalog.py`
- Delete: `lib/allocation_batch.py`, `tests/test_allocation_batch.py`
- Modify (pruebas): `tests/test_allocation.py`, `tests/test_allocation_endpoints.py`, `tests/test_analytics_dash.py`, `tests/test_operator_catalog.py`

**Interfaces:**
- Consumes: nada nuevo.
- Produces: `allocation.enrich_limits(limits, now, lang)` se queda tal cual, porque `/usage/state` lo usa. No queda ninguna ruta que aplique cambios de cuenta o modelo por su cuenta.

- [ ] **Step 1: Write the failing test**

Agregar a `tests/test_analytics_dash.py`:

```python
def test_nothing_proposes_or_applies_account_changes():
    for gone in ('"/allocation/propose"', '"/allocation/preview"', '"/allocation/apply"', '"/allocation/status"',
                 '"/allocation/retry"', '"/allocation/revert"', "import allocation_batch", "ALLOCATION_PLANS"):
        assert gone not in SRC, gone
    assert not Path("lib/allocation_batch.py").exists()
    catalog = Path("lib/operator_catalog.py").read_text()
    assert '("cuentas", "comparar", "pomodoro")' in catalog
    assert "setLimitStyle" not in catalog and "compareSetDays" not in catalog
```

- [ ] **Step 2: Run it to verify it fails**

Run: `pytest -q tests/test_analytics_dash.py -k applies`
Expected: FAIL (`"/allocation/propose"` is still in `bin/cc-dash`).

- [ ] **Step 3: Borrar Reparto con Aplicar**

- `bin/cc-dash`:
  - Borrar las rutas POST `/allocation/propose|preview|apply|retry|revert` (~8870-8879) y GET `/allocation/status` (~8537).
  - Borrar las funciones `ALLOCATION_PLANS`, `_batches`, `selectable_accounts_by_motor`, `_motor_options`, `allocation_inputs`, `_git_root_cached`, `_build_plan`, `_valid_overrides`, `allocation_propose`, `allocation_preview`, `_status_of`, `_run_batch_bg`, `_ALLOCATION_APPLY_LOCK`, `_batch_for_plan`, `_reuse_or_refuse`, `allocation_apply`, `allocation_status`, `allocation_retry`, `_still_applied` y `allocation_revert` (~2522-2797), además de `import allocation_batch`. Antes de borrar cada una, `grep -n "<nombre>" bin/cc-dash`: `enriched_limits`, `inherit_trust_for_switch` y lo que use otra ruta se quedan.
- `lib/allocation.py`: dejar `WINDOW_SECONDS`, `_t`, `fmt_duration` y `enrich_limits` (con lo que estas usen). Borrar `profile_for`, `signal_for`, `layer_of`, `stale_factor`, `_tier_of`, `model_for_layer`, `pool_key`, `governing_limit`, `session_key`, `plan_hash`, `_from`, `_pool_of`, `_load_map`, `_reference_load`, `_burn_after`, `_candidates`, `_within_layer`, `_risk`, `_same`, `impact`, `propose` e `invert`. Primero `grep -rn "allocation\.\(<nombre>\)" bin lib`: si algo vivo usa una, esa se queda.
- `git rm lib/allocation_batch.py tests/test_allocation_batch.py`.
- `lib/operator_catalog.py`:
  - Borrar las herramientas `set_limit_style` (~199) y `compare_set_window` (~250).
  - En `open_analytics_tab` (~249), cambiar la lista de tabs a `("cuentas", "comparar", "pomodoro")`.
- Pruebas:
  - `tests/test_allocation.py`: dejar solo las de `enrich_limits` y la de una fila sin porcentaje.
  - `tests/test_allocation_endpoints.py`: dejar solo la de la salida de `enriched_limits`.
  - `tests/test_operator_catalog.py`: actualizar la lista de tabs si la nombra.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `pytest -q tests/test_analytics_dash.py tests/test_allocation.py tests/test_allocation_endpoints.py tests/test_allocation_dash.py tests/test_operator_catalog.py tests/test_operator_analytics.py tests/test_usage_dash.py`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add -A bin/cc-dash lib/allocation.py lib/operator_catalog.py tests
git commit -m "refactor: fuera Reparto con propuesta y Aplicar; la app no cambia cuentas por su cuenta"
```

---

### Task 11: Analytics en un modal en medio (ventana propia en el escritorio, pantalla completa en el remoto partido)

**Files:**
- Modify: `bin/cc-app` (nueva `open_analytics_modal`, `HEADER_ACTIONS["analytics"]`), `dash/index.html` (`#btn-usage` en la app), `dash/workspace.css:1146`
- Modify: `tests/test_analytics_dash.py`

**Interfaces:**
- Consumes: `?panel=usage&tab=` (Task 9).
- Produces: en la app, `#btn-usage` envía `{headerAction: "analytics"}` y `cc-app` abre una ventana sin bordes centrada sobre la app (mín(1180, ancho-120) × mín(940, alto-90)) con `?panel=usage`. En el remoto partido, `#usage` usa toda la ventana.

- [ ] **Step 1: Write the failing test**

```python
def test_analytics_opens_wide_on_desktop_and_split_remote():
    app = Path("bin/cc-app").read_text()
    assert 'HEADER_ACTIONS["analytics"] = open_analytics_modal' in app
    assert '"panel": "usage"' in app
    assert 'postMessage(JSON.stringify({headerAction: "analytics"}))' in HTML
    css = Path("dash/workspace.css").read_text()
    assert ".modal:not(#tabclose):not(#groupclose):not(.chain-only):not(#usage)" in css
```

- [ ] **Step 2: Run it to verify it fails**

Run: `pytest -q tests/test_analytics_dash.py -k wide`
Expected: FAIL

- [ ] **Step 3: Implementar**

En `bin/cc-app`, después de `HEADER_ACTIONS["chains"] = open_chain_modal`:

```python
# Analytics (grill 2-oct): el mockup aprobado mide 1120 px y el tablero es solo la columna
# izquierda, así que se abre como Cadenas: una ventana centrada sobre toda la app.
_ANMODAL = {"win": None, "wv": None}


def open_analytics_modal(*_):
    if _ANMODAL["win"] is None:
        dlg = Gtk.Window(type=Gtk.WindowType.TOPLEVEL)
        dlg.set_transient_for(win)
        dlg.set_modal(True)
        dlg.set_decorated(False)
        dlg.get_style_context().add_class("mosaic-zoom")
        wv2 = WebKit2.WebView.new_with_user_content_manager(ucm)
        dlg.add(wv2)
        dlg.connect("key-press-event",
                    lambda _w, ev: (dlg.hide(), True)[1] if ev.keyval == Gdk.KEY_Escape else False)
        dlg.connect("delete-event", lambda *_: (dlg.hide(), True)[1])
        _ANMODAL.update(win=dlg, wv=wv2)
    w, h = win.get_size()
    _ANMODAL["win"].set_default_size(min(1180, max(640, w - 120)), min(940, max(480, h - 90)))
    _ANMODAL["win"].set_position(Gtk.WindowPosition.CENTER_ON_PARENT)
    _ANMODAL["wv"].load_uri(f"{BASE_URL}/?{urlencode({'panel': 'usage', 'v': _DASH_V})}")
    _ANMODAL["win"].show_all()
    return False


HEADER_ACTIONS["analytics"] = open_analytics_modal
```

En `dash/index.html`, primera línea del handler de `#btn-usage`:

```js
	  if(inApp()){ window.webkit.messageHandlers.centro.postMessage(JSON.stringify({headerAction: "analytics"})); return; }
```

En `dash/workspace.css:1146`, agregar `:not(#usage)` a la regla del remoto partido, para que Analytics use toda la ventana.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `pytest -q tests/test_analytics_dash.py tests/test_header_layout.py`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add bin/cc-app dash/index.html dash/workspace.css tests/test_analytics_dash.py
git commit -m "feat(analytics): ventana propia en el escritorio y pantalla completa en el remoto partido"
```

---

### Task 12: Verificación visual pixel perfect y entrega

La paridad de HTML ya está probada. Esta tarea prueba lo que el HTML no garantiza: fuentes, CSS del tablero alrededor y tamaño real. Se compara captura contra captura en el mismo Chrome de la Mac, con los mismos datos.

**Files:**
- Create: `tools/png_diff.py`

- [ ] **Step 1: Comparador de capturas**

Crear `tools/png_diff.py`:

```python
#!/usr/bin/env python3
"""Compara dos capturas: % de píxeles distintos y una imagen con las diferencias en rojo.
Uso: python3 tools/png_diff.py mockup.png tablero.png diff.png"""
import sys

from PIL import Image, ImageChops

a, b = (Image.open(p).convert("RGB") for p in sys.argv[1:3])
if a.size != b.size:
    sys.exit(f"tamaños distintos: {a.size} vs {b.size}")
diff = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > 24 else 0)
bad = sum(1 for v in diff.getdata() if v)
out = a.copy()
out.paste((255, 0, 0), mask=diff)
out.save(sys.argv[3])
pct = 100 * bad / (a.size[0] * a.size[1])
print(f"{pct:.3f}% de píxeles distintos")
sys.exit(0 if pct <= 0.1 else 1)
```

- [ ] **Step 2: Levantar un tablero de prueba aislado y el mockup**

```bash
WT=$(pwd)
TMPH=$(mktemp -d); mkdir -p "$TMPH/dash"
for f in "$WT"/dash/*; do ln -s "$f" "$TMPH/dash/"; done; ln -s "$WT/assets" "$TMPH/dash/assets"
devhost add analytics-check            # imprime el puerto, p. ej. 72xx
PORT=$(devhost port analytics-check)
HOME="$TMPH" COMANDOS_DASH_DIR="$TMPH/dash" python3 bin/cc-dash "$PORT" --no-open &
PROTO=$(mktemp -d); git show prototype/analytics-grill:dash/prototypes/prototype-analytics.html > "$PROTO/p.html"
devhost add analytics-mock; MPORT=$(devhost port analytics-mock)
python3 -m http.server "$MPORT" --bind 127.0.0.1 --directory "$PROTO" &
cc-browser-expose start "$PORT"; cc-browser-expose start "$MPORT"
```

- [ ] **Step 3: Capturar los 6 casos de escritorio y los 3 de celular**

Con `mcp__chrome-bg__new_page` y `resize_page` a 1300×1000 (y luego a 390×844):
- **Mockup:** abrir `http://127.0.0.1:$MPORT/p.html`. Ocultar el selector con `evaluate_script(() => { document.getElementById('picker').style.display = 'none' })`. Elegir pestaña con `[data-tab]` y semana con `[data-w="-1"]`; en celular, botón «Vista celular».
- **Tablero:** abrir `http://127.0.0.1:$PORT/?panel=usage&demo=normal&tab=<pestaña>` (semana anterior: clic en `[data-w="-1"]`).
- En ambos, tomar el rectángulo con `evaluate_script(() => JSON.stringify(document.querySelector('.an, #modal').getBoundingClientRect()))` y guardar la captura con `take_screenshot({fullPage: true, filePath: '/tmp/an-<caso>-<lado>.png'})` en la Mac. Traerla con `scp macmini:/tmp/an-<caso>-<lado>.png "$TMPH/"`, recortar ambas al rectángulo con PIL y correr `python3 tools/png_diff.py`.

Expected: ≤ 0.1 % de píxeles distintos en cada caso. Si un caso pasa del umbral, abrir `diff.png`: la causa está en CSS del tablero que se cuela en `.an` o en una fuente que no cargó. Se corrige en `dash/analytics.css`/`index.html`, nunca en el módulo generado.

- [ ] **Step 4: Datos reales**

Abrir `http://127.0.0.1:$PORT/?panel=usage` con `HOME` real en otro tablero de prueba (`HOME=$HOME COMANDOS_DASH_DIR="$TMPH/dash" python3 bin/cc-dash <otro puerto devhost> --no-open`), sin reiniciar el `cc-dash.service` de Jesús. Comprobar a ojo:
- botellas de `claude:main`, `claude:relotto`, Codex y Grok, cada una con su reset;
- calendario con sesiones de relotto;
- Comparar con proyectos;
- Pomodoro con los bloques reales.

Al terminar, apagar los servidores de prueba y `devhost rm analytics-check analytics-mock`.

- [ ] **Step 5: Entrega**

```bash
git add tools/png_diff.py && git commit -m "test(analytics): comparador de capturas para la verificación pixel perfect"
git fetch && git rebase origin/main   # otra sesión edita index.html: resolver conflictos conservando lo de ambos
pytest -q && bash tests/test_js_parses.sh
```
Pedir a Jesús la revisión con las capturas (rutas absolutas en esta máquina). Solo con su visto bueno: mergear a `main`, `bash install.sh` (crea los enlaces nuevos en `~/.claude/hooks/dash` y borra los de Reparto) y `systemctl --user restart cc-dash.service`. Jesús reinicia `cc-app` a mano para que el botón abra el modal nuevo (Task 11). La migración v11 borra y reimporta el uso de Claude y Codex en el primer minuto; durante ese minuto los totales bajan.
