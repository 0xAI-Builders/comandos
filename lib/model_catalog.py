"""Project visible native CLI catalogs onto the in-memory provider registry.

Only copy model IDs, display names and declared reasoning/context capabilities.
Catalog presence is cached account evidence, never a live inference test. The
curated registry, routing rules and credentials are never written or widened.
"""
from __future__ import annotations

import copy
import json
import os
import re

_MAX_CACHE_BYTES = 4 * 1024 * 1024


def catalog_paths():
    return {p: os.path.join(os.environ.get(p.upper() + '_HOME') or
                           os.path.expanduser('~/' + '.' + p), 'models_cache.json')
            for p in ('codex', 'grok')}


def catalog_signature():
    result = []
    for provider, path in catalog_paths().items():
        try:
            stat = os.stat(path)
            stamp = (stat.st_mtime_ns, stat.st_size, stat.st_ino)
        except OSError:
            stamp = None
        result.append((provider, path, stamp))
    return tuple(result)


def _read(path):
    try:
        with open(path, 'rb') as fh:
            raw = fh.read(_MAX_CACHE_BYTES + 1)
        if len(raw) > _MAX_CACHE_BYTES:
            return {}
        data = json.loads(raw)
        return data if isinstance(data, dict) else {}
    except (OSError, ValueError, RecursionError):
        return {}


def catalog_models():
    result = {}
    for provider, path in catalog_paths().items():
        data = _read(path)
        rows = data.get('models')
        if provider == 'codex':
            rows = rows if isinstance(rows, list) else []
        else:
            rows = [r.get('info') for r in rows.values() if isinstance(r, dict)] if isinstance(rows, dict) else []
        models = []
        for row in rows:
            if not isinstance(row, dict):
                continue
            if provider == 'codex' and row.get('visibility') != 'list':
                continue
            if provider == 'grok' and row.get('hidden') is not False:
                continue
            mid = row.get('slug' if provider == 'codex' else 'id')
            pattern = r'gpt-\d+(?:\.\d+)?(?:-[a-z0-9]+)*' if provider == 'codex' else r'grok-\d+(?:\.\d+)?(?:-[a-z0-9]+)*'
            if not isinstance(mid, str) or not re.fullmatch(pattern, mid):
                continue
            levels = row.get('supported_reasoning_levels' if provider == 'codex' else 'reasoning_efforts')
            # No capability inference from IDs or another model's settings.
            if not isinstance(levels, list):
                continue
            field = 'effort' if provider == 'codex' else 'id'
            efforts = list(dict.fromkeys(r[field] for r in levels
                if isinstance(r, dict) and isinstance(r.get(field), str)
                and re.fullmatch(r'[a-z][a-z0-9_-]{0,31}', r[field])))
            default = row.get('default_reasoning_level' if provider == 'codex' else 'reasoning_effort')
            if default not in efforts:
                default = ''
            name = row.get('display_name' if provider == 'codex' else 'name')
            spec = {'id': mid, 'name': name[:160] if isinstance(name, str) else mid,
                    'efforts': efforts, 'defaultEffort': default,
                    'catalogSource': {'provider': provider, 'kind': 'cli-cache',
                                      'fetchedAt': data['fetched_at'][:64] if isinstance(data.get('fetched_at'), str) else None}}
            context = row.get('context_window')
            if isinstance(context, int) and not isinstance(context, bool) and context > 0:
                spec['contextWindow'] = context
            models.append(spec)
        result[provider] = models
    return result


def hydrate_registry(registry):
    result = copy.deepcopy(registry)
    for provider, models in catalog_models().items():
        for section in ('motors', 'harnesses'):
            owner = (result.get(section) or {}).get(provider)
            if not owner or (section == 'harnesses' and 'models' not in owner):
                continue
            target = owner.setdefault('models', [])
            for observed in models:
                key = re.sub(r'\[.*$', '', observed['id'])
                existing = next((m for m in target if re.sub(r'\[.*$', '', m.get('id', '')) == key), None)
                if existing is None:
                    target.append(copy.deepcopy(observed))
                    continue
                # Retain curated IDs, names and tags, except obsolete rollout text.
                if existing.pop('soon', False):
                    existing.pop('tag', None)
                for field in ('efforts', 'defaultEffort', 'contextWindow', 'catalogSource'):
                    if field in observed:
                        existing[field] = copy.deepcopy(observed[field])
    return result
