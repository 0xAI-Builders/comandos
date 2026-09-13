"""Reusable launch choices and observed extension usage, without global edits.

Config inventories describe files, not a running agent's effective tool set.
Overrides are attached to one new CLI command. Existing panes are never changed.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import tempfile
import time
import uuid

import accounts
import capabilities
import cc_usage

try:
    import tomllib
except ImportError:
    tomllib = None

_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,79}$")
_NAME = re.compile(r"^[A-Za-z0-9_][A-Za-z0-9_.:@/-]{0,159}$")
_FIELDS = {'id', 'name', 'harness', 'motor', 'routeId', 'model', 'effort',
           'harnessAccount', 'motorAccount', 'skills', 'mcps', 'updatedAt', 'createdAt'}


def _connect(db):
    con = cc_usage.connect(db)
    con.execute('''create table if not exists session_profiles (
        id text primary key, name text not null, payload text not null,
        created_at integer not null, updated_at integer not null)''')
    return con


def _id(value):
    if not isinstance(value, str) or not _ID.fullmatch(value):
        raise ValueError('id de perfil inválido')
    return value


def list_profiles(db):
    with _connect(db) as con:
        return [json.loads(r['payload']) for r in con.execute(
            'select payload from session_profiles order by name collate nocase, id')]


def get_profile(db, ident):
    with _connect(db) as con:
        row = con.execute('select payload from session_profiles where id=?', (_id(ident),)).fetchone()
    if not row:
        raise ValueError('perfil no encontrado')
    return json.loads(row['payload'])


def save_profile(db, data):
    if not isinstance(data, dict) or set(data) - _FIELDS:
        raise ValueError('campos de perfil inválidos')
    ident = _id(data.get('id') or uuid.uuid4().hex)
    name = data.get('name')
    if not isinstance(name, str) or not name.strip() or len(name) > 100 or any(ord(c) < 32 for c in name):
        raise ValueError('nombre de perfil inválido')
    out = {'id': ident, 'name': name.strip()}
    for key in ('harness', 'motor', 'routeId', 'model', 'effort', 'harnessAccount', 'motorAccount'):
        value = data.get(key, 'main' if key.endswith('Account') else '')
        if not isinstance(value, str) or len(value) > 160 or any(ord(c) < 32 for c in value) or (key != 'model' and value and not _NAME.fullmatch(value)):
            raise ValueError(f'{key} inválido')
        if key.endswith('Account'):
            value = accounts.validate_alias(value)
        out[key] = value
    if not out['harness']:
        out['harness'] = 'codex'
    if not out['motor']:
        out['motor'] = out['harness']
    for key in ('skills', 'mcps'):
        values = data.get(key, {})
        if not isinstance(values, dict) or len(values) > 1000:
            raise ValueError(f'{key} inválidos')
        if any(not isinstance(k, str) or not _NAME.fullmatch(k) or type(v) is not bool for k, v in values.items()):
            raise ValueError(f'{key} inválidos')
        out[key] = dict(values)
    now = int(time.time())
    with _connect(db) as con:
        previous = con.execute('select created_at from session_profiles where id=?', (ident,)).fetchone()
        out.update(createdAt=previous[0] if previous else now, updatedAt=now)
        con.execute('''insert into session_profiles values(?,?,?,?,?) on conflict(id) do update set
            name=excluded.name,payload=excluded.payload,updated_at=excluded.updated_at''',
            (ident, out['name'], json.dumps(out, ensure_ascii=False), out['createdAt'], now))
    return out


def delete_profile(db, ident):
    with _connect(db) as con:
        con.execute('delete from session_profiles where id=?', (_id(ident),))


def _toml(path):
    if tomllib is None:
        try:
            text = Path(path).read_text()
        except OSError:
            return {}
        # Python 3.10: no need to parse unrelated TOML. Refuse array replacement
        # whenever a skills key might exist; MCP leaf overrides remain safe.
        return {'_skillsUnparsed': bool(re.search(r'\bskills\b', text))}
    try:
        with open(path, 'rb') as f:
            return tomllib.load(f)
    except (OSError, ValueError):
        return {}


def _json(path):
    try:
        data = json.loads(Path(path).read_text())
        return data if isinstance(data, dict) else {}
    except (OSError, ValueError):
        return {}


def _claude_mcp_sources(home, cwd):
    global_json = home / '.claude.json'
    if not global_json.exists():
        global_json = home.parent / '.claude.json'
    global_data = _json(global_json)
    servers = dict(global_data.get('mcpServers') or {})
    local = ((global_data.get('projects') or {}).get(str(Path(cwd).resolve())) or {}).get('mcpServers') or {}
    if local:
        raise ValueError('MCPs locales de Claude presentes: selección aislada todavía no soportada')
    for settings in (home / 'settings.json', Path(cwd) / '.claude/settings.json', Path(cwd) / '.claude/settings.local.json'):
        plugins = _json(settings).get('enabledPlugins') or {}
        if not isinstance(plugins, dict) or any(plugins.values()):
            raise ValueError('plugins de Claude presentes: selección aislada de MCPs todavía no soportada')
    servers.update(_json(Path(cwd) / '.mcp.json').get('mcpServers') or {})
    return servers


def launch_capabilities(harness):
    skills = harness == 'codex'
    mcps = harness == 'codex'
    return {
        'skills': {'status': 'next_launch' if skills else 'unsupported',
                   'supported': skills, 'reason': 'Se aplica al iniciar una sesión nueva. Las sesiones abiertas conservan su configuración.' if skills else
                       'Este CLI no ofrece selección individual verificada de skills por lanzamiento.'},
        # Claude's strict MCP override excludes plugin MCPs and can alter variable
        # expansion. Do not advertise exact selection until those sources can be preserved.
        'mcps': {'status': 'next_launch' if mcps else ('conditional' if harness == 'claude' else 'unsupported'),
                 'supported': mcps or harness == 'claude',
                 'reason': 'Se aplica al iniciar una sesión nueva. Las sesiones abiertas conservan su configuración.' if mcps else
                     ('Selección de servidores JSON por lanzamiento; sin recarga del agente vivo.' if harness == 'claude' else
                      'Selección por lanzamiento no verificada para este CLI.')},
        'hotReload': False,
    }


def _codex_configs(home, cwd):
    paths = [home / 'config.toml']
    paths += [p / '.codex' / 'config.toml' for p in capabilities._parents_to_root(cwd)]
    return [_toml(p) for p in paths]


def _skill_overrides(configs):
    # Arrays in later config layers replace earlier arrays, matching Codex config.
    result = []
    for cfg in configs:
        entries = (cfg.get('skills') or {}).get('config')
        if isinstance(entries, list):
            result = [dict(x) for x in entries if isinstance(x, dict)]
    return result


def inventory(registry, harness, alias, cwd):
    if not os.path.isabs(cwd) or not os.path.isdir(cwd):
        raise ValueError('cwd inválido')
    caps = launch_capabilities(harness)
    base = capabilities.session_capabilities(registry, harness, alias, cwd)
    try:
        home = accounts.account_home(registry, harness, alias)
    except accounts.AccountError:
        home = None
    if home and harness == 'claude':
        try:
            _claude_mcp_sources(home, cwd)
        except ValueError as e:
            caps['mcps'] = {'status': 'unsupported', 'supported': False, 'reason': str(e)}
    roots = []
    configs = _codex_configs(home, cwd) if home and harness == 'codex' else []
    overrides = _skill_overrides(configs)
    if any(c.get('_skillsUnparsed') for c in configs):
        caps['skills'] = {'status': 'unsupported', 'supported': False,
            'reason': 'Este perfil requiere un lector TOML para conservar overrides existentes de skills.'}
    if home:
        roots.append((home / 'skills', 'user', 'skills-directory'))
        if harness == 'codex':
            roots.append((Path.home() / '.agents' / 'skills', 'user', 'shared-skills'))
            for parent in capabilities._parents_to_root(cwd):
                roots.append((parent / '.agents' / 'skills', 'project', 'shared-skills'))
                roots.append((parent / '.codex' / 'skills', 'project', 'skills-directory'))
        else:
            roots.append((Path(cwd) / ('.' + harness) / 'skills', 'project', 'skills-directory'))
    skills, seen = [], set()
    for root, scope, source in roots:
        try:
            paths = sorted(root.glob('*/SKILL.md'))[:500]
            if root.name == 'skills':
                paths += sorted((root / '.system').glob('*/SKILL.md'))[:100]
        except OSError:
            continue
        for path in paths:
            real = str(path.resolve())
            if real in seen:
                continue
            seen.add(real)
            try:
                with path.open() as f:
                    raw = f.read(12000)
            except OSError:
                continue
            match = re.match(r'^---\r?\n(.*?)\r?\n---', raw, re.S)
            front = match.group(1) if match else ''
            name_match = re.search(r'^name:\s*(.+)$', front, re.M)
            name = name_match.group(1).strip().strip('"\'') if name_match else path.parent.name
            if not _NAME.fullmatch(name):
                name = path.parent.name
            enabled = True
            for entry in overrides:
                if entry.get('path') == real or entry.get('name') == name:
                    enabled = entry.get('enabled', True) is not False
            skills.append({'id': hashlib.sha256(real.encode()).hexdigest()[:24], 'name': name,
                'path': real, 'scope': scope, 'source': source, 'enabled': enabled,
                'configuredEnabled': enabled, 'effectiveNow': None, 'status': 'configured',
                'automaticInvocation': not bool(re.search(r'^disable-model-invocation:\s*true\s*$', front, re.M)),
                'toggleable': caps['skills']['supported']})
    mcps = []
    for item in base.get('mcps', []):
        row = dict(item)
        row.update(id=item['name'], configuredEnabled=item['enabled'], effectiveNow=None,
                   toggleable=caps['mcps']['supported'] and bool(_NAME.fullmatch(item['name'])))
        mcps.append(row)
    return {'skills': sorted(skills, key=lambda x: x['name'].casefold()), 'mcps': mcps,
            'capabilities': caps, 'provenance': 'configuration_files', 'effectiveNow': None,
            'note': 'Inventario de archivos; no confirma las extensiones cargadas por un proceso vivo.'}


def launch_draft(profile):
    draft = {**{k: profile.get(k, '') for k in ('harness', 'motor', 'routeId', 'model', 'effort',
              'harnessAccount', 'motorAccount')}, 'agent': profile.get('harness', 'codex'), 'profileId': profile['id']}
    if not draft.get('routeId'):
        draft.pop('routeId', None)
    return draft


def _toml_value(value):
    if type(value) is bool:
        return 'true' if value else 'false'
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    raise ValueError('override de skill inválido')


def launch_args(profile, registry, cwd, runtime_dir, *, dry_run=False):
    """Validate inventory IDs before returning flags; never modify shared files."""
    harness = profile.get('harness') or 'codex'
    skills, mcps = profile.get('skills') or {}, profile.get('mcps') or {}
    caps = launch_capabilities(harness)
    if skills and not caps['skills']['supported']:
        raise ValueError('selección individual de skills no soportada para este CLI')
    if mcps and not caps['mcps']['supported']:
        raise ValueError('selección de MCPs no soportada para este CLI')
    if not skills and not mcps:
        return []
    inv = inventory(registry, harness, profile.get('harnessAccount') or 'main', cwd)
    if skills and not inv['capabilities']['skills']['supported']:
        raise ValueError(inv['capabilities']['skills']['reason'])
    if mcps and not inv['capabilities']['mcps']['supported']:
        raise ValueError(inv['capabilities']['mcps']['reason'])
    skill_rows = {s['id']: s for s in inv['skills']}
    mcp_rows = {m['name']: m for m in inv['mcps']}
    if set(skills) - skill_rows.keys():
        raise ValueError('skill no disponible en el inventario de esta cuenta y carpeta')
    if set(mcps) - mcp_rows.keys():
        raise ValueError('MCP no disponible en el inventario de esta cuenta y carpeta')
    if any(type(x) is not bool for x in list(skills.values()) + list(mcps.values())):
        raise ValueError('estado de extensión inválido')
    result = []
    home = accounts.account_home(registry, harness, profile.get('harnessAccount') or 'main')
    if harness == 'codex':
        if skills:
            entries = _skill_overrides(_codex_configs(home, cwd))
            changed = {skill_rows[ident]['path']: enabled for ident, enabled in skills.items()}
            entries = [x for x in entries if x.get('path') not in changed]
            entries += [{'path': path, 'enabled': enabled} for path, enabled in changed.items()]
            encoded = ['{' + ','.join(f'{key}={_toml_value(e[key])}' for key in ('path', 'name', 'enabled') if key in e) + '}' for e in entries]
            result += ['-c', 'skills.config=[' + ','.join(encoded) + ']']
        for name, enabled in sorted(mcps.items()):
            result += ['-c', 'mcp_servers.' + json.dumps(name) + '.enabled=' + _toml_value(enabled)]
    elif harness == 'claude' and mcps:
        servers = _claude_mcp_sources(home, cwd)
        selected = {name: cfg for name, cfg in servers.items()
                    if mcps.get(name, mcp_rows.get(name, {}).get('enabled', True))}
        # Moving a project declaration changes ${...} expansion and relative paths.
        # Reject those cases until we can preserve the harness's original resolution.
        serialized = json.dumps(selected)
        if '${' in serialized or any(isinstance(v, dict) and v.get('cwd') and not os.path.isabs(v['cwd']) for v in selected.values()):
            raise ValueError('MCP con expansión o cwd relativo: selección aislada no soportada')
        if dry_run:
            return ['--strict-mcp-config', '--mcp-config', '<private-launch-config>']
        root = Path(runtime_dir)
        root.mkdir(parents=True, exist_ok=True, mode=0o700)
        fd, path = tempfile.mkstemp(prefix='profile-mcp-', suffix='.json', dir=root)
        with os.fdopen(fd, 'w') as f:
            json.dump({'mcpServers': selected}, f)
        result += ['--strict-mcp-config', '--mcp-config', path]
    return result


def extension_usage(db, session='', pane='', days=7, now=None):
    if not isinstance(session, str) or (session and not re.fullmatch(r'[A-Za-z0-9._-]{1,80}', session)):
        raise ValueError('sesión inválida')
    if pane and (not session or not re.fullmatch(r'%[0-9]+', pane)):
        raise ValueError('panel inválido')
    days = max(1, min(90, int(days)))
    now = time.time() if now is None else now
    result = {'scope': {'session': session, 'pane': pane}, 'days': days,
              'provenance': 'usage_tool_calls + usage_interactions; observed hook events',
              'extensions': [], 'unattributedSkillCalls': 0, 'tokens': None, 'costUsd': None,
              'coverage': 'Solo eventos capturados; ausencia de eventos no demuestra ausencia de uso.',
              'status': 'empty'}
    if not Path(db).is_file():
        return result
    # A read endpoint must not create/migrate a live DB or trigger transcript scans.
    with sqlite3.connect('file:' + str(Path(db).resolve()) + '?mode=ro', uri=True, timeout=2) as con:
        con.row_factory = sqlite3.Row
        tables = {r[0] for r in con.execute("select name from sqlite_master where type='table'")}
        if not {'usage_tool_calls', 'usage_interactions'} <= tables:
            return result
        columns = {r[1] for r in con.execute('pragma table_info(usage_tool_calls)')}
        skill_col = 't.skill_name' if 'skill_name' in columns else "''"
        params = [int((now - days * 86400) * 1000)]
        where = 'coalesce(t.finished_at_ms,t.started_at_ms)>=?'
        if session:
            where += ' and i.tmux_session=?'
            params.append(session)
        if pane:
            where += ' and i.tmux_pane=?'
            params.append(pane)
        rows = con.execute(f'''select t.tool_name,{skill_col} as skill_name,count(*) as n,
            max(coalesce(t.finished_at_ms,t.started_at_ms)) as last_seen,
            sum(t.duration_ms) as duration, count(t.duration_ms) as measured,
            sum(case when t.status='failed' then 1 else 0 end) as failures,
            group_concat(distinct i.source) as sources, group_concat(distinct t.confidence) as confidence
            from usage_tool_calls t join usage_interactions i on i.id=t.interaction_id
            where {where} group by t.tool_name,{skill_col} limit 2000''', params).fetchall()
    grouped = {}
    for row in rows:
        tool = row['tool_name']
        if tool.lower() == 'skill':
            if not row['skill_name']:
                result['unattributedSkillCalls'] += row['n']
                continue
            kind, name = 'skill', row['skill_name']
        elif tool.startswith('mcp__') and len(tool.split('__')) >= 3:
            kind, name = 'mcp', tool.split('__', 2)[1]
        else:
            continue
        if not _NAME.fullmatch(name):
            continue
        key = (kind, name)
        entry = grouped.setdefault(key, {'kind': kind, 'name': name, 'count': 0, 'lastSeen': None,
            'durationMs': None, 'durationObservedCalls': 0, 'failures': 0,
            'tokens': None, 'costUsd': None, 'provenance': 'observed_tool_events', 'sources': [], 'confidence': []})
        entry['count'] += row['n']
        entry['lastSeen'] = max(entry['lastSeen'] or 0, (row['last_seen'] or 0) / 1000)
        entry['failures'] += row['failures'] or 0
        if row['measured']:
            entry['durationMs'] = (entry['durationMs'] or 0) + row['duration']
            entry['durationObservedCalls'] += row['measured']
        for field in ('sources', 'confidence'):
            entry[field] = sorted(set(entry[field] + (row[field] or '').split(',')) - {''})
    result['extensions'] = sorted(grouped.values(), key=lambda x: (-x['count'], x['name']))
    result['status'] = 'observed' if grouped or result['unattributedSkillCalls'] else 'empty'
    result['truncated'] = len(rows) == 2000
    return result
