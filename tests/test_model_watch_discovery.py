import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "lib"))
import model_watch  # noqa: E402

SRC = (ROOT / "lib" / "model_watch.py").read_text()


def test_codex_regex_accepts_named_generations_like_astra():
    # OpenAI bautiza cada generación (sol, luna, terra, astra…): una lista
    # cerrada de sufijos dejaba fuera gpt-6-astra y el watcher no avisaba.
    for ident in ("gpt-6-astra", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.4-mini", "gpt-5.3-codex", "gpt-5.2"):
        assert model_watch._CODEX_RE.fullmatch(ident), ident
    assert not model_watch._CODEX_RE.fullmatch("gpt-5.6-lunacodex-auto-review")


def test_watcher_finds_binaries_outside_systemd_path():
    # cc-dash corre bajo systemd con PATH pelón: codex vive en ~/.bun/bin y
    # claude/grok en ~/.local/bin. shutil.which a secas devolvía nada y el
    # watcher reportaba codex: [] sin versión.
    assert "shutil.which(" not in SRC.split("_which = shutil.which", 1)[1]
    assert "from providers import which as _which" in SRC


def test_watcher_runs_resolved_paths_not_bare_names():
    # Con PATH pelón, `codex --version` a pelo devolvía nada y la versión era "?".
    assert "_run([exe] + cmd[1:]" in SRC
    assert '_run([grok_exe, "models"]' in SRC


def test_codex_discovery_uses_visible_account_catalog(tmp_path, monkeypatch):
    import json
    home = tmp_path / 'codex'
    home.mkdir()
    (home / 'models_cache.json').write_text(json.dumps({'models': [
        {'slug': 'gpt-6-astra', 'visibility': 'list'},
        {'slug': 'gpt-6-sol', 'visibility': 'list'},
        {'slug': 'codex-auto-review', 'visibility': 'hide'},
    ]}))
    monkeypatch.setenv('CODEX_HOME', str(home))
    monkeypatch.setattr(model_watch, '_claude_binary', lambda: '')
    monkeypatch.setattr(model_watch, '_codex_binary', lambda: '')
    monkeypatch.setattr(model_watch, '_which', lambda name: None)
    assert model_watch.discover_models()['codex'] == ['gpt-6-astra', 'gpt-6-sol']


def test_watcher_reports_sibling_models_previously_suppressed(tmp_path, monkeypatch):
    import json
    registry = tmp_path / 'registry.json'
    original = json.dumps({'motors': {'codex': {'models': [{'id': 'gpt-6-astra'}]}}})
    registry.write_text(original)
    discovered = {'codex': ['gpt-5.5', 'gpt-6-astra', 'gpt-6-sol', 'gpt-6-luna']}
    # The old watcher had already scanned the IDs but never reported them.
    (tmp_path / 'model-watch.json').write_text(json.dumps({'discovered': discovered, 'newSince': {}}))
    monkeypatch.setattr(model_watch, 'installed_versions', lambda: {'codex': '0.156.1'})
    monkeypatch.setattr(model_watch, 'discover_models', lambda gh=None: discovered)
    monkeypatch.setattr(model_watch, 'discover_addons', lambda: {})
    first = model_watch.watch_models(tmp_path, registry, now=100)
    assert first['news'] == {'codex': ['gpt-6-luna', 'gpt-6-sol']}
    assert first['snapshot']['newSince']['codex']['models'] == ['gpt-6-luna', 'gpt-6-sol']
    assert model_watch.watch_models(tmp_path, registry, now=200)['news'] == {}
    assert registry.read_text() == original


def test_grok_discovery_keeps_named_variants(monkeypatch, tmp_path):
    monkeypatch.setenv('CODEX_HOME', str(tmp_path))
    monkeypatch.setattr(model_watch, '_claude_binary', lambda: '')
    monkeypatch.setattr(model_watch, '_codex_binary', lambda: '')
    monkeypatch.setattr(model_watch, '_which', lambda name: '/grok' if name == 'grok' else None)
    monkeypatch.setattr(model_watch, '_run', lambda *a, **kw: '- grok-4.7\n- grok-4.7-build-fast\n')
    assert model_watch.discover_models()['grok'] == ['grok-4.7', 'grok-4.7-build-fast']


def test_runtime_registry_hydrates_only_visible_catalog_models(tmp_path, monkeypatch):
    import json
    import model_catalog
    monkeypatch.setenv('CODEX_HOME', str(tmp_path / 'codex'))
    monkeypatch.setenv('GROK_HOME', str(tmp_path / 'grok'))
    (tmp_path / 'codex').mkdir()
    (tmp_path / 'grok').mkdir()
    (tmp_path / 'codex' / 'models_cache.json').write_text(json.dumps({'fetched_at': '2026-09-25', 'identity': 'secret', 'models': [
        {'slug': 'gpt-6-astra', 'display_name': 'From CLI', 'visibility': 'list',
         'supported_reasoning_levels': [{'effort': 'medium'}, {'effort': 'ultra'}],
         'default_reasoning_level': 'medium', 'context_window': 272000},
        {'slug': 'gpt-6-sol', 'display_name': 'GPT-6-Sol', 'visibility': 'list',
         'supported_reasoning_levels': [{'effort': 'high'}], 'default_reasoning_level': 'high'},
        {'slug': 'gpt-6-reserve', 'visibility': 'hide'},
    ]}))
    (tmp_path / 'grok' / 'models_cache.json').write_text(json.dumps({'models': {
        'grok-4.7-build-fast': {'api_key': 'secret', 'info': {'id': 'grok-4.7-build-fast',
          'name': 'Grok 4.7 Fast', 'hidden': False, 'reasoning_effort': 'high',
          'reasoning_efforts': [{'id': 'high', 'default': True}], 'context_window': 500000}},
        'grok-hidden': {'info': {'id': 'grok-hidden', 'hidden': True}},
    }}))
    base = {'motors': {'codex': {'models': [{'id': 'gpt-6-astra', 'name': 'Curated', 'soon': True,
            'efforts': ['high'], 'defaultEffort': 'high', 'tag': 'aún no en tu cuenta'}]},
            'grok': {'models': []}}, 'harnesses': {'grok': {'models': []}}, 'routes': [{'id': 'keep'}]}
    before = json.dumps(base)
    result = model_catalog.hydrate_registry(base)
    assert json.dumps(base) == before
    codex = result['motors']['codex']['models']
    assert [m['id'] for m in codex] == ['gpt-6-astra', 'gpt-6-sol']
    assert codex[0]['name'] == 'Curated'
    assert not codex[0].get('soon')
    assert codex[0]['efforts'] == ['medium', 'ultra']
    assert codex[0]['defaultEffort'] == 'medium'
    assert codex[0]['contextWindow'] == 272000
    assert 'aún no' not in codex[0].get('tag', '')
    assert result['harnesses']['grok']['models'][0]['id'] == 'grok-4.7-build-fast'
    assert result['routes'] == base['routes']
    assert 'secret' not in json.dumps(result)
    signature = model_catalog.catalog_signature()
    (tmp_path / 'codex' / 'models_cache.json').write_text('{bad')
    assert signature != model_catalog.catalog_signature()
    assert model_catalog.hydrate_registry(base)['motors']['codex'] == base['motors']['codex']


def test_dash_registry_reloads_when_catalog_changes(tmp_path):
    import ast
    import os
    from types import SimpleNamespace
    path = tmp_path / 'providers.json'
    path.write_text('{}')
    source = (ROOT / 'bin' / 'cc-dash').read_text()
    fn = next(n for n in ast.parse(source).body if isinstance(n, ast.FunctionDef) and n.name == 'load_provider_registry')
    stamp = [1]
    ns = {'os': os, 'PROVIDERS_FILE': str(path),
          '_provider_registry_cache': {'mtime': None, 'data': None},
          'provider_registry': SimpleNamespace(load_registry=lambda path: {'base': True}, validate_registry=lambda data: data),
          'model_catalog_lib': SimpleNamespace(catalog_signature=lambda: stamp[0],
                                               hydrate_registry=lambda base: dict(base, generation=stamp[0]))}
    exec(compile(ast.Module(body=[fn], type_ignores=[]), '<registry-loader>', 'exec'), ns)
    assert ns['load_provider_registry']()['generation'] == 1
    stamp[0] = 2
    assert ns['load_provider_registry']()['generation'] == 2


def test_watcher_rescans_when_catalog_changes_without_cli_upgrade(tmp_path):
    import ast
    import os
    from types import SimpleNamespace
    source = (ROOT / 'bin' / 'cc-dash').read_text()
    fn = next(n for n in ast.parse(source).body if isinstance(n, ast.FunctionDef) and n.name == '_model_watch_cycle')
    calls = []
    clock = [1000]
    stamp = [1]
    ns = {'os': os, 'time': SimpleNamespace(time=lambda: clock[0]), 'HOOKS': str(tmp_path),
          'PROVIDERS_FILE': str(tmp_path / 'registry'), 'MODEL_WATCH_FILE': str(tmp_path / 'snapshot'),
          '_model_watch_state': {'versions': {'codex': '1'}, 'last_full': 1000, 'catalog': 0},
          'model_catalog_lib': SimpleNamespace(catalog_signature=lambda: stamp[0]),
          'model_watch_lib': SimpleNamespace(installed_versions=lambda: {'codex': '1'},
              watch_models=lambda *a, **kw: calls.append(True) or {'news': {}, 'snapshot': {}}),
          'news_watch_lib': SimpleNamespace(watch_news=lambda *a: {'news': []}),
          '_read_json_quiet': lambda path: {}, 'write_json_file': lambda *a, **kw: None}
    exec(compile(ast.Module(body=[fn], type_ignores=[]), '<watch-cycle>', 'exec'), ns)
    ns['_model_watch_cycle']()
    assert len(calls) == 1
    ns['_model_watch_cycle']()
    assert len(calls) == 1
    stamp[0] = 2
    ns['_model_watch_cycle']()
    assert len(calls) == 2


def test_catalog_read_rejects_oversize_and_malformed_data(tmp_path, monkeypatch):
    import json
    import model_catalog
    monkeypatch.setenv('CODEX_HOME', str(tmp_path))
    monkeypatch.setenv('GROK_HOME', str(tmp_path / 'missing'))
    path = tmp_path / 'models_cache.json'
    row = {'slug': 'gpt-6-sol', 'visibility': 'list', 'supported_reasoning_levels': [{'effort': 'high'}]}
    path.write_text(json.dumps({'models': [row], 'padding': 'x' * (4 * 1024 * 1024)}))
    assert model_catalog.catalog_models()['codex'] == []
    for bad in (b'\xff', b'[]', b'{"models": [null, 42, {"slug": []}]}', b'['*20000+b']'*20000):
        path.write_bytes(bad)
        assert model_catalog.catalog_models()['codex'] == []
