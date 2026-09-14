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
    return capabilities.read_config(Path(path))


def _json(path):
    return capabilities.read_config(Path(path))


def _claude_mcp_sources(home, cwd, alias='main'):
    errors = []
    global_data = capabilities.read_config(capabilities.claude_user_json(home, alias), errors, 'claude-user')
    project_data = capabilities.read_config(Path(cwd) / '.mcp.json', errors, 'mcp-json')
    servers = dict(capabilities._dict(global_data.get('mcpServers')))
    local = capabilities._dict(capabilities._dict(global_data.get('projects')).get(str(Path(cwd).resolve())))
    if local.get('mcpServers'):
        raise ValueError('MCPs locales de Claude presentes: selección aislada todavía no soportada')
    for settings in (home / 'settings.json', Path(cwd) / '.claude/settings.json', Path(cwd) / '.claude/settings.local.json'):
        plugins = capabilities.read_config(settings, errors, 'claude-settings').get('enabledPlugins') or {}
        if not isinstance(plugins, dict) or any(plugins.values()):
            raise ValueError('plugins de Claude presentes: selección aislada de MCPs todavía no soportada')
    if errors:
        raise ValueError('configuración de Claude ilegible: no se puede preservar la selección de MCPs')
    servers.update(capabilities._dict(project_data.get('mcpServers')))
    return servers


def launch_capabilities(harness):
    return {
        'skills': {'status': 'next_launch' if harness == 'codex' else 'unsupported',
                   'supported': harness == 'codex', 'reason': 'Se aplica al iniciar una sesión nueva. Las sesiones abiertas conservan su configuración.' if harness == 'codex' else
                       'Este CLI no ofrece selección individual verificada de skills por lanzamiento.'},
        'mcps': {'status': 'next_launch' if harness == 'codex' else ('conditional' if harness == 'claude' else 'unsupported'),
                 'supported': harness in ('codex', 'claude'),
                 'reason': 'Se aplica al iniciar una sesión nueva. Las sesiones abiertas conservan su configuración.' if harness == 'codex' else
                     ('Selección de servidores JSON por lanzamiento; sin recarga del agente vivo.' if harness == 'claude' else
                      'Selección por lanzamiento no verificada para este CLI.')},
        'hotReload': False,
    }


def _codex_configs(home, cwd):
    # Installed Codex reads skills.config only from user and session flag layers.
    paths = [home / 'config.toml']
    errors=[]
    result=[capabilities.read_config(p,errors,'codex-user') for p in dict.fromkeys(p.resolve() for p in paths)]
    if errors:
        raise ValueError('configuración TOML ilegible; no se pueden conservar overrides')
    return result


def _skill_overrides(configs):
    result = []
    for cfg in configs:
        entries = capabilities._dict(cfg.get('skills')).get('config')
        if isinstance(entries, list):
            result = [dict(x) for x in entries if isinstance(x, dict)]
    return result


def _skill_path(value, base):
    if not isinstance(value, str):
        return None
    path = Path(os.path.expanduser(value))
    if not path.is_absolute():
        path = base / path
    return str(path.resolve())


def _skill_roots(ctx):
    h, home, cwd = ctx['harness'], ctx['home'], ctx['project']
    roots = []
    def add(path, scope, source, plugin=None):
        roots.append((Path(path), scope, source, plugin))
    if home is None or h == 'shell':
        return roots
    if h == 'codex':
        add(home / 'skills', 'user', 'skills-directory')
        add(Path.home() / '.agents/skills', 'user', 'shared-skills')
        add(Path('/etc/codex/skills'), 'admin', 'admin-skills')
        for parent in ctx['parents']:
            add(parent / '.agents/skills', 'project', 'shared-skills')
            add(parent / '.codex/skills', 'project', 'skills-directory')
    elif h == 'claude':
        add(home / 'skills', 'user', 'skills-directory')
        for parent in ctx['parents']:
            add(parent / '.claude/skills', 'project', 'skills-directory')
    elif h == 'grok':
        user = ctx['layers'][0]['data'] if ctx['layers'] else {}
        for vendor in ('claude', 'cursor'):
            if capabilities._dict(capabilities._dict(user.get('compat')).get(vendor)).get('skills') is not False:
                add(Path.home() / ('.' + vendor) / 'skills', 'compatible', vendor + '-skills')
                add(cwd / ('.' + vendor) / 'skills', 'project', vendor + '-skills')
        add(home / 'skills', 'user', 'skills-directory')
        for parent in ctx['parents']:
            add(parent / '.grok/skills', 'project', 'skills-directory')
        for path in capabilities._list(capabilities._dict(user.get('skills')).get('paths')):
            if isinstance(path, str):
                add(Path(path).expanduser() if os.path.isabs(os.path.expanduser(path)) else cwd / path, 'custom', 'grok-skill-path')
    elif h == 'opencode':
        for base in (Path.home() / '.claude', Path.home() / '.agents', home):
            add(base / 'skills', 'user', 'skills-directory')
        for parent in ctx['parents']:
            for folder in ('.claude', '.agents', '.opencode'):
                add(parent / folder / 'skills', 'project', 'skills-directory')
        if os.environ.get('OPENCODE_CONFIG_DIR'):
            add(Path(os.path.expanduser(os.environ['OPENCODE_CONFIG_DIR'])) / 'skills', 'custom', 'opencode-custom-directory')
        for path in capabilities._list(capabilities._dict(ctx['settings'].get('skills')).get('paths')):
            if isinstance(path,str):
                add(Path(path).expanduser() if os.path.isabs(os.path.expanduser(path)) else cwd / path, 'custom', 'opencode-skill-path')
    elif h == 'gemini':
        for base, scope in ((home, 'user'), (Path.home() / '.agents', 'user'), (cwd / '.gemini', 'project'), (cwd / '.agents', 'project')):
            add(base / 'skills', scope, 'skills-directory')
    elif h == 'agy':
        add(home / 'config/skills', 'user', 'agy-skills')
        add(cwd / '.agent/skills', 'project', 'agy-legacy-skills')
        add(cwd / '.agents/skills', 'project', 'agy-skills')
    for plugin in ctx['plugins']:
        declared = plugin['manifest'].get('skills', './skills')
        for relative in [declared] if isinstance(declared, str) else capabilities._list(declared):
            if not isinstance(relative,str):
                continue
            path = (plugin['root'] / relative).resolve()
            if not path.is_relative_to(plugin['root']):
                ctx['errors'].append({'source': plugin['source'], 'code':'plugin_path_outside_root'})
                continue
            add(path, plugin['scope'], plugin['source'], plugin)
    # Plugin skills have lower priority than standalone skills in these CLIs.
    return sorted(roots, key=lambda r: r[3] is None) if h in ('claude','grok','gemini') else roots


def _skill_files(root, errors, source):
    # Follow directory aliases once. Bound recursion and total visited directories
    # so a symlink cycle or a mistaken broad root cannot hang a dashboard request.
    pending, seen, found = [(root,0)], set(), []
    while pending and len(seen) < 2000 and len(found) < 500:
        path, depth = pending.pop(0)
        try:
            real = path.resolve()
            if real in seen or not path.is_dir():
                continue
            seen.add(real)
            if (path / 'SKILL.md').is_file():
                found.append(path / 'SKILL.md')
                continue
            if depth < 5:
                pending.extend((p,depth+1) for p in sorted(path.iterdir()) if p.is_dir() and (not p.name.startswith('.') or p.name == '.system'))
        except (OSError, RuntimeError):
            errors.append({'source':source, 'code':'skill_directory_unreadable'})
    if pending:
        errors.append({'source':source, 'code':'skill_scan_truncated'})
    return found


def _frontmatter(path):
    try:
        with path.open(encoding='utf-8') as f:
            raw = f.read(16000)
    except (OSError, UnicodeError):
        return None
    match = re.match(r'^---\r?\n(.*?)\r?\n---', raw, re.S)
    if not match:
        return {}
    try:
        import yaml
        value = yaml.safe_load(match[1])
        return value if isinstance(value,dict) else {}
    except ImportError:
        # Plain scalar frontmatter stays readable without an optional YAML lib.
        return {m[1]: m[2].strip().strip('"\'') for m in re.finditer(r'^([\w-]+):\s*(.+)$',match[1],re.M)}
    except Exception:
        return {}


def _skills(ctx, caps):
    overrides = _skill_overrides([layer['data'] for layer in ctx['layers'] if layer['scope']=='user']) if ctx['harness']=='codex' else []
    output = {}
    for root,scope,source,plugin in _skill_roots(ctx):
        for path in _skill_files(root,ctx['errors'],source):
            real = str(path.resolve())
            if real in output:
                output[real]['sources'] = list(dict.fromkeys(output[real]['sources']+[source]))
                continue
            front = _frontmatter(path)
            if front is None:
                ctx['errors'].append({'source':source,'code':'skill_unreadable'}); continue
            name = front.get('name') or path.parent.name
            if not isinstance(name,str) or not _NAME.fullmatch(name):
                name = path.parent.name
            if not _NAME.fullmatch(name):
                continue
            enabled = True
            for entry in overrides:
                # Codex ignores selectors with both path and name, or neither.
                if ('path' in entry) == ('name' in entry):
                    continue
                selector_name=entry.get('name')
                if isinstance(selector_name,str):
                    selector_name=selector_name.strip()
                if _skill_path(entry.get('path'),ctx['home']) == real or selector_name == name:
                    enabled = entry.get('enabled',True) is not False
            if ctx['harness'] in ('gemini','grok'):
                settings = ctx['layers'][0]['data'] if ctx['harness']=='grok' else ctx['settings']
                cfg = capabilities._dict(settings.get('skills'))
                if cfg.get('enabled') is False or name in capabilities._list(cfg.get('disabled')):
                    enabled=False
                for ignore in capabilities._list(cfg.get('ignore')):
                    ignored=_skill_path(ignore,ctx['project'])
                    if ignored and Path(real).is_relative_to(Path(ignored)):
                        enabled=False
            if plugin:
                enabled = False if plugin['enabled'] is False else enabled if plugin['enabled'] is True else None
                name=plugin['name']+':'+name if ctx['harness']!='gemini' else name
            if ctx['harness']=='codex' and '.system' in path.parts and capabilities._dict(capabilities._dict(ctx['settings'].get('skills')).get('bundled')).get('enabled') is False:
                enabled=False
            automatic = front.get('disable-model-invocation') not in (True,'true')
            if ctx['harness']=='codex' and capabilities._dict(ctx['settings'].get('skills')).get('include_instructions') is False:
                automatic=False
            # Codex invocation policy is stored in agents/openai.yaml, separate
            # from installation/enablement and from Claude frontmatter.
            if ctx['harness']=='codex':
                policy_path=path.parent/'agents/openai.yaml'
                if policy_path.is_file():
                    try:
                        text=policy_path.read_text(encoding='utf-8')[:16000]
                        if re.search(r'^\s+allow_implicit_invocation:\s*false\s*(?:#.*)?$',text,re.M):automatic=False
                    except (OSError,UnicodeError):
                        ctx['errors'].append({'source':source,'code':'skill_policy_unreadable'})
            row={'id':hashlib.sha256(real.encode()).hexdigest()[:24], 'name':name,
                 'path':real,'scope':scope,'source':source,'sources':[source],
                 'enabled':enabled,'configuredEnabled':enabled,'effectiveNow':None,'runtimeEnabled':None,
                 'status':'disabled' if enabled is False else 'configured' if enabled is True else 'installed',
                 'automaticInvocation':automatic,'toggleable':caps['skills']['supported'] and not plugin,
                 **capabilities.description_metadata('',front.get('description'))}
            if plugin:row['plugin']=plugin['key']
            output[real]=row
    rows=list(output.values())
    # Codex keeps equal names at distinct paths. Other providers choose one;
    # preserve the shadowed file in inventory without claiming it is selected.
    if ctx['harness'] in ('claude','grok','opencode','gemini'):
        winners={}
        for row in rows:
            old=winners.get(row['name'])
            if old:
                old.update(status='shadowed',enabled=False,configuredEnabled=False,toggleable=False)
            winners[row['name']]=row
    return sorted(rows,key=lambda row:(row['name'].casefold(),row['path']))


def inventory(registry, harness, alias, cwd):
    ctx = capabilities.configuration(registry,harness,alias,cwd)
    caps = launch_capabilities(harness)
    if ctx['home'] and harness == 'claude':
        try:
            _claude_mcp_sources(ctx['home'],cwd,alias)
        except ValueError as e:
            caps['mcps'] = {'status':'unsupported','supported':False,'reason':str(e)}
    base = capabilities.session_capabilities(registry,harness,alias,cwd,_context=ctx)
    skills = _skills(ctx,caps)
    if ctx['errors']:
        for kind in ('skills','mcps'):
            caps[kind]={'status':'unsupported','supported':False,'reason':'Inventario incompleto: hay configuración o extensiones ilegibles.'}
        for row in skills:row['toggleable']=False
    mcps=[]
    for item in base['mcps']:
        row=dict(item)
        row.update(id=item['name'],configuredEnabled=item['enabled'],effectiveNow=None,
                   toggleable=caps['mcps']['supported'] and not item.get('plugin') and bool(_NAME.fullmatch(item['name'])))
        mcps.append(row)
    status='incomplete' if ctx['errors'] else base['status']
    if skills and status=='empty':status='configured'
    return {'harness':harness,'account':alias,'skills':skills,'mcps':mcps,'status':status,
            'confidence':base['confidence'],'limitations':ctx['limitations'],'errors':ctx['errors'],
            'capabilities':caps,'provenance':'configuration_files','effectiveNow':None,'runtimeEnabled':None,
            'note':'Inventario de archivos; no confirma las extensiones cargadas por un proceso vivo.'}


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
    if any(not skill_rows[x].get('toggleable') for x in skills) or any(not mcp_rows[x].get('toggleable') for x in mcps):
        raise ValueError('selección individual de esta extensión no soportada')
    result = []
    home = accounts.account_home(registry, harness, profile.get('harnessAccount') or 'main')
    if harness == 'codex':
        if skills:
            entries = _skill_overrides(_codex_configs(home, cwd))
            changed = {skill_rows[ident]['path']: enabled for ident, enabled in skills.items()}
            if any(set(x) - {'path', 'name', 'enabled'} or type(x.get('enabled')) is not bool for x in entries):
                raise ValueError('override de skill existente no soportado; no se puede conservar')
            entries = [x for x in entries if _skill_path(x.get('path'), home) not in changed]
            entries += [{'path': path, 'enabled': enabled} for path, enabled in changed.items()]
            encoded = ['{' + ','.join(f'{key}={_toml_value(e[key])}' for key in ('path', 'name', 'enabled') if key in e) + '}' for e in entries]
            result += ['-c', 'skills.config=[' + ','.join(encoded) + ']']
        for name, enabled in sorted(mcps.items()):
            result += ['-c', 'mcp_servers.' + json.dumps(name) + '.enabled=' + _toml_value(enabled)]
    elif harness == 'claude' and mcps:
        servers = _claude_mcp_sources(home, cwd, profile.get('harnessAccount') or 'main')
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
