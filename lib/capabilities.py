#!/usr/bin/env python3
"""Read-only provider extension declarations, never a claim about a live process.

Only names, public descriptions, states and provenance leave this module. Discovery
never executes providers, plugins, MCP commands, JavaScript config or remote URLs.
"""
from __future__ import annotations

from collections import OrderedDict
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import threading
from typing import Any

from accounts import account_home, validate_alias
from mcp_descriptions import metadata as description_metadata

try:
    import tomllib
except ImportError:
    try:
        import tomli as tomllib
    except ImportError:
        tomllib = None

_SAFE_NAME_RE = re.compile(r'^[^\x00-\x1f]{1,160}$')
_MAX_CONFIG = 2 * 1024 * 1024
_TOML_CACHE = OrderedDict()
_TOML_LOCK = threading.Lock()


def _dict(value):
    return value if isinstance(value, dict) else {}


def _list(value):
    return value if isinstance(value, list) else []


def _jsonc(text):
    # Preserve strings, including URLs and escaped quotes, while stripping comments.
    token = re.compile(r'"(?:\\.|[^"\\])*"|//[^\n]*|/\*[\s\S]*?\*/')
    text = token.sub(lambda m: m[0] if m[0].startswith('"') else ' ', text)
    text = re.sub(r'("(?:\\.|[^"\\])*")|,\s*([}\]])',
                  lambda m: m[1] if m[1] is not None else m[2], text)
    return json.loads(text)


def _toml(text):
    if tomllib is not None:
        return tomllib.loads(text)
    # Python 3.10 installs can use a local 3.11 reader. The bounded digest cache
    # avoids a subprocess on every inventory refresh. No files or providers run.
    digest = hashlib.sha256(text.encode()).hexdigest()
    with _TOML_LOCK:
        if digest in _TOML_CACHE:
            _TOML_CACHE.move_to_end(digest)
            return _TOML_CACHE[digest]
        binary = shutil.which('python3.11')
        if not binary:
            raise ValueError('toml_reader_unavailable')
        result = subprocess.run([binary, '-I', '-c',
            'import json,sys,tomllib; print(json.dumps(tomllib.loads(sys.stdin.read()),default=str))'],
            input=text, text=True, capture_output=True, timeout=3)
        if result.returncode:
            raise ValueError('invalid_toml')
        value = _configuration_fields(json.loads(result.stdout))
        _TOML_CACHE[digest] = value
        while len(_TOML_CACHE) > 64:
            _TOML_CACHE.popitem(last=False)
        return value


def _configuration_fields(data):
    """Keep only public extension settings in the Python 3.10 fallback cache."""
    result = {}
    if 'skills' in data:
        result['skills'] = data['skills']
    if 'features' in data:
        result['features'] = {k:v for k,v in _dict(data['features']).items() if type(v) is bool}
    if 'compat' in data:
        result['compat'] = {k:{a:b for a,b in _dict(v).items() if type(b) is bool} for k,v in _dict(data['compat']).items()}
    # Provider skill schemas contain paths, selectors and booleans. Reject
    # unknown entry fields at launch instead of caching unrelated values.
    if isinstance(result.get('skills'),dict):
        cfg = result['skills']
        result['skills'] = {k:cfg[k] for k in ('config','paths','ignore','disabled','enabled','bundled','include_instructions') if k in cfg}
        if isinstance(cfg.get('config'),list):
            result['skills']['config'] = [
                {**{k:e[k] for k in ('path','name','enabled') if k in e},
                 **({'_unsupportedKeys':True} if set(e)-{'path','name','enabled'} else {})}
                for e in cfg['config'] if isinstance(e,dict)]
    if 'plugins' in data:
        result['plugins'] = {}
        for name, spec in _dict(data['plugins']).items():
            if name in ('paths','disabled','enabled') and isinstance(spec,list):
                result['plugins'][name]=spec
            elif isinstance(spec,dict):
                result['plugins'][name]={k:spec[k] for k in ('enabled',) if k in spec}
                if 'mcp_servers' in spec:
                    result['plugins'][name]['mcp_servers']={n:{'enabled':s['enabled']} for n,s in _dict(spec['mcp_servers']).items() if isinstance(s,dict) and 'enabled' in s}
    if 'marketplaces' in data:
        result['marketplaces']={n:{'source_type':'local','source':s.get('source')} for n,s in _dict(data['marketplaces']).items() if isinstance(s,dict) and s.get('source_type')=='local'}
    if 'projects' in data:
        result['projects']={n:{'trust_level':s.get('trust_level')} for n,s in _dict(data['projects']).items() if isinstance(s,dict)}
    if 'mcp_servers' in data:
        result['mcp_servers'] = {n: {k:s[k] for k in ('enabled','disabled','description') if k in s}
            for n,s in _dict(data['mcp_servers']).items() if isinstance(s,dict)}
    return result


def read_config(path, errors=None, source='configuration'):
    path = Path(path)
    try:
        if not path.exists():
            return {}
        if path.stat().st_size > _MAX_CONFIG:
            raise ValueError('configuration_too_large')
        text = path.read_text(encoding='utf-8')
        data = _toml(text) if path.suffix == '.toml' else _jsonc(text) if path.suffix == '.jsonc' else json.loads(text)
        if not isinstance(data, dict):
            raise ValueError('invalid_configuration_shape')
        return data
    except (OSError, ValueError, UnicodeError, subprocess.SubprocessError):
        if errors is not None:
            error = {'source': source, 'code': 'configuration_unreadable'}
            if error not in errors:
                errors.append(error)
        return {}


def _json(path):
    return read_config(path)


def _entry(name, provider, scope, source, enabled, description=None):
    return {'name': name, 'provider': provider, 'scope': scope, 'source': source,
            'sources': [source], 'enabled': bool(enabled),
            'status': 'configured' if enabled else 'disabled', 'confidence': 'configured',
            'effectiveNow': None, 'runtimeEnabled': None, **description_metadata(name, description)}


def _invalid_servers(value):
    return (not isinstance(value, dict) or any(
        not isinstance(spec, dict) or any(
            key in spec and type(spec[key]) is not bool for key in ('enabled', 'disabled'))
        for spec in value.values()))


def _server_entries(servers, provider, scope, source, disabled=(), *, inherit=False):
    disabled = {x for x in (disabled if isinstance(disabled,(list,tuple,set)) else []) if isinstance(x,str)}
    output = []
    for name, spec in _dict(servers).items():
        if not isinstance(name, str) or not _SAFE_NAME_RE.fullmatch(name) or not isinstance(spec, dict):
            continue
        row = _entry(name, provider, scope, source,
            name not in disabled and spec.get('enabled') is not False and spec.get('disabled') is not True,
            spec.get('description'))
        if any(key in spec and type(spec[key]) is not bool for key in ('enabled','disabled')):
            row.update(enabled=None,status='invalid')
        if inherit:
            row['_inheritEnabled'] = not ('enabled' in spec or 'disabled' in spec or name in disabled)
        output.append(row)
    return output


def _toml_mcps(path, provider, scope, source):
    return _server_entries(read_config(path).get('mcp_servers'), provider, scope, source, inherit=True)


def _json_mcps(path, provider, scope, source, disabled=()):
    return _server_entries(read_config(path).get('mcpServers'), provider, scope, source, disabled)


def _parents_to_root(cwd):
    if not cwd or not os.path.isabs(cwd):
        return []
    current = Path(cwd).resolve()
    result = []
    while True:
        result.append(current)
        if (current / '.git').exists() or current.parent == current:
            break
        current = current.parent
    return list(reversed(result))


def _deep_merge(left, right):
    result = dict(left)
    for key, value in right.items():
        result[key] = _deep_merge(result[key], value) if isinstance(result.get(key), dict) and isinstance(value, dict) else value
    return result


def provider_home(registry, harness, alias):
    spec = _dict(registry.get('harnesses')).get(harness)
    if not isinstance(spec, dict):
        raise ValueError('CLI no registrado')
    alias = validate_alias(alias)
    if _dict(spec.get('capabilities')).get('accounts'):
        home = account_home(registry, harness, alias)
        if alias != 'main' and not home.is_dir():
            raise ValueError('cuenta no encontrada')
        return home
    if alias != 'main':
        raise ValueError('cuentas no soportadas para este CLI')
    defaults = {'opencode': Path(os.environ.get('XDG_CONFIG_HOME') or Path.home() / '.config') / 'opencode',
                'gemini': Path.home() / '.gemini', 'agy': Path.home() / '.gemini'}
    value = spec.get('defaultHome') or defaults.get(harness)
    return Path(os.path.expanduser(str(value))).resolve() if value else None


def claude_user_json(home, alias='main'):
    # CLAUDE_CONFIG_DIR keeps .claude.json inside that directory. Only the
    # conventional default home uses the historical ~/.claude.json location.
    inner = home / '.claude.json'
    if inner.exists() or alias != 'main':
        return inner
    if home == (Path.home() / '.claude').resolve():
        return Path.home() / '.claude.json'
    return inner


def configuration(registry, harness, alias, cwd):
    if not isinstance(cwd, str) or not os.path.isabs(cwd) or not Path(cwd).is_dir():
        raise ValueError('cwd inválido')
    home = provider_home(registry, harness, alias)
    project = Path(cwd).resolve()
    parents = _parents_to_root(cwd)
    ctx = {'harness':harness, 'account':alias, 'home':home, 'project':project,
           'parents':parents, 'layers':[], 'settings':{}, 'errors':[], 'limitations':[], 'plugins':[]}
    seen = set()
    def add(path, scope, source):
        path = Path(path).resolve()
        if path in seen:
            return {}
        seen.add(path)
        data = read_config(path, ctx['errors'], source)
        ctx['layers'].append({'path':path, 'scope':scope, 'source':source, 'data':data})
        return data
    if harness in ('codex','grok'):
        add(home/'config.toml','user',harness+'-user')
        chain = parents if harness == 'codex' else list(dict.fromkeys([parents[0],project]))
        for parent in chain:
            add(parent/('.'+harness)/'config.toml','project',harness+'-project')
        for layer in ctx['layers']:
            ctx['settings'] = _deep_merge(ctx['settings'],layer['data'])
        ctx['limitations'].append('La confianza del proyecto, políticas administradas y overrides del proceso no se verifican en este inventario.')
    elif harness == 'claude':
        ctx['userJson'] = claude_user_json(home,alias)
        ctx['userData'] = add(ctx['userJson'],'user','claude-user')
        add(project/'.mcp.json','project','mcp-json')
        for path,scope in [(home/'settings.json','user'),(project/'.claude/settings.json','project'),(project/'.claude/settings.local.json','local')]:
            data = read_config(path,ctx['errors'],'claude-'+scope+'-settings')
            ctx['settings'] = _deep_merge(ctx['settings'],data)
        ctx['limitations'].append('No incluye conectores de claude.ai ni políticas administradas remotas; la aprobación del proyecto y los flags del proceso pueden limitar la carga.')
    elif harness == 'opencode':
        for suffix in ('json','jsonc'):
            add(home/('opencode.'+suffix),'user','opencode-user')
        if os.environ.get('OPENCODE_CONFIG'):
            add(os.path.expanduser(os.environ['OPENCODE_CONFIG']),'custom','opencode-custom')
        for parent in parents:
            for suffix in ('json','jsonc'):
                add(parent/('opencode.'+suffix),'project','opencode-project')
        for parent in parents:
            for suffix in ('json','jsonc'):
                add(parent/'.opencode'/('opencode.'+suffix),'project','opencode-directory')
        if os.environ.get('OPENCODE_CONFIG_DIR'):
            for suffix in ('json','jsonc'):
                add(Path(os.path.expanduser(os.environ['OPENCODE_CONFIG_DIR']))/('opencode.'+suffix),'custom','opencode-custom-directory')
        for layer in ctx['layers']:
            ctx['settings'] = _deep_merge(ctx['settings'],layer['data'])
        ctx['limitations'].append('No ejecuta plugins JS ni consulta configuración remota; overrides del proceso y permisos por agente no se verifican.')
    elif harness == 'gemini':
        add(home/'settings.json','user','gemini-user')
        add(project/'.gemini/settings.json','project','gemini-project')
        for layer in ctx['layers']:
            ctx['settings'] = _deep_merge(ctx['settings'],layer['data'])
        ctx['limitations'].append('No verifica configuración administrada, flags del proceso ni skills integradas en el binario.')
    elif harness == 'agy':
        add(home/'config/mcp_config.json','user','agy-user')
        add(project/'.agents/mcp_config.json','project','agy-project')
        ctx['limitations'].append('Rutas de Antigravity CLI documentadas; no verifica migración desde versiones antiguas ni plugins del IDE.')
    elif harness == 'acp':
        ctx['limitations'].append('ACP depende del agente y cuenta de la ruta seleccionada; no tiene un inventario de extensiones propio.')
    elif harness != 'shell':
        ctx['limitations'].append('Detección de extensiones no implementada para este CLI registrado.')
    if harness == 'codex' and any(_dict(l['data'].get('skills')).get('config') for l in ctx['layers'] if l['scope']=='project'):
        ctx['limitations'].append('Codex ignora skills.config del proyecto; las reglas de skills se leen de la cuenta y de flags de sesión.')
    for layer in ctx['layers']:
        key = 'mcp_servers' if harness in ('codex','grok') else 'mcp' if harness=='opencode' else 'mcpServers'
        data = layer['data']
        if key in data and _invalid_servers(data[key]):
            ctx['errors'].append({'source':layer['source'],'code':'mcp_definition_invalid'})
    ctx['plugins'] = _plugins(ctx)
    return ctx


def _plugin_manifest(root, ctx):
    for rel in ('.codex-plugin/plugin.json','.claude-plugin/plugin.json','.grok-plugin/plugin.json','plugin.json','gemini-extension.json'):
        if (root/rel).is_file():
            return read_config(root/rel,ctx['errors'],ctx['harness']+'-plugin')
    return {}


def _version_key(path):
    # Codex's "default" cache directory takes precedence; otherwise numeric
    # version segments sort numerically, with a stable lexical fallback.
    return (path.name == 'default', tuple((1,int(x)) if x.isdigit() else (0,x) for x in re.split(r'(\d+)',path.name)))


def _plugins(ctx):
    h,home,project = ctx['harness'],ctx['home'],ctx['project']
    output=[]
    def add(root,key,scope,enabled,source):
        root=Path(root).expanduser().resolve()
        if not root.is_dir():
            ctx['errors'].append({'source':source,'code':'plugin_files_missing'})
            return
        manifest=_plugin_manifest(root,ctx)
        name=manifest.get('name') or key.split('@',1)[0]
        if not isinstance(name,str) or not _SAFE_NAME_RE.fullmatch(name):
            return
        output.append({'root':root,'key':key,'name':name,'scope':scope,'enabled':enabled,
                       'source':source,'manifest':manifest})
    if h=='codex':
        configs=_dict(ctx['settings'].get('plugins'))
        cache=home/'plugins/cache'
        keys=set(configs)
        if cache.is_dir():
            keys.update(p.name+'@'+p.parent.name for p in cache.glob('*/*') if p.is_dir())
        for key in sorted(keys)[:500]:
            if '@' not in key or '/' in key or '\\' in key:
                continue
            name,market=key.rsplit('@',1)
            if not all(re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]{0,159}', part) for part in (name,market)):
                ctx['errors'].append({'source':'codex-plugin','code':'plugin_identifier_invalid'})
                continue
            versions=sorted((p for p in (cache/market/name).glob('*') if p.is_dir()),key=_version_key)
            cfg=_dict(configs.get(key))
            if versions:
                # A downloaded cache entry alone is not installation/enablement.
                enabled=cfg.get('enabled') if type(cfg.get('enabled')) is bool else None
                add(versions[-1],key,'user',enabled,'codex-plugin')
            elif key in configs:
                ctx['errors'].append({'source':'codex-plugin','code':'plugin_files_missing'})
    elif h=='claude':
        installed=read_config(home/'plugins/installed_plugins.json',ctx['errors'],'claude-plugin-index')
        enabled=_dict(ctx['settings'].get('enabledPlugins'))
        for key,records in _dict(installed.get('plugins')).items():
            candidates=[]
            for record in _list(records):
                if not isinstance(record,dict) or not isinstance(record.get('installPath'),str):
                    continue
                scope=record.get('scope','user')
                if scope in ('project','local') and record.get('projectPath') != str(project):
                    continue
                candidates.append(record)
            if candidates:
                record=max(candidates,key=lambda r:{'user':0,'project':1,'local':2,'managed':3}.get(r.get('scope','user'),-1))
                state=enabled.get(key) if type(enabled.get(key)) is bool else None
                add(record['installPath'],key,record.get('scope','user'),state,'claude-plugin')
    elif h=='grok':
        cfg=_dict(ctx['settings'].get('plugins'))
        def grok_add(root,scope,source,default=False):
            root=Path(root).expanduser().resolve()
            manifest=_plugin_manifest(root,ctx)
            name=manifest.get('name') or root.name
            ident=scope+'/'+hashlib.sha256(str(root).encode()).hexdigest()[:8]+'/'+str(name)
            disabled=_list(cfg.get('disabled')); enabled=_list(cfg.get('enabled'))
            state=False if name in disabled or ident in disabled else True if name in enabled or ident in enabled else default
            add(root,ident,scope,state,source)
        # Include marketplace installs by their registry's exact root, without
        # guessing which cache version or sibling repository is installed.
        installed=read_config(home/'installed-plugins/registry.json',ctx['errors'],'grok-plugin-index')
        for record in _dict(installed.get('repos')).values():
            if not isinstance(record,dict) or not isinstance(record.get('path'),str):continue
            for spec in _dict(record.get('plugins')).values():
                root=Path(record['path'])/(_dict(spec).get('subdir') or '')
                grok_add(root,'user','grok-installed-plugin')
        for root in _list(cfg.get('paths')):
            if isinstance(root,str):
                grok_add(root,'config','grok-plugin-path',True)
        roots=[(home/'plugins','user')]+[(p/'.grok/plugins','project') for p in ctx['parents']]
        for parent,scope in roots:
            for root in sorted(parent.glob('*'))[:500]:
                if root.is_dir() and (_plugin_manifest(root,ctx) or (root/'skills').is_dir() or (root/'.mcp.json').is_file()):
                    grok_add(root,scope,'grok-plugin')
        if output:
            ctx['limitations'].append('Los plugins de Grok también requieren confianza; el estado configurado no confirma la carga del servidor.')
    elif h=='gemini':
        activation=read_config(home/'extensions/extension-enablement.json',ctx['errors'],'gemini-extension-enablement')
        for path in sorted((home/'extensions').glob('*/gemini-extension.json'))[:500]:
            manifest=read_config(path,ctx['errors'],'gemini-extension')
            name=manifest.get('name') or path.parent.name
            state=True
            for rule in _list(_dict(activation.get(name)).get('overrides')):
                if not isinstance(rule,str):continue
                pattern=rule[1:] if rule.startswith('!') else rule
                expression=re.escape(pattern).replace(r'\*','.*')
                if re.fullmatch(expression,str(project).rstrip('/')+'/'):
                    state=not rule.startswith('!')
            add(path.parent,name,'user',state,'gemini-extension')
    unique={}
    for plugin in output:
        unique[plugin['root']]=plugin
    return list(unique.values())


def _plugin_mcps_from_context(ctx):
    output=[]
    for plugin in ctx['plugins']:
        root=plugin['root']; manifest=plugin['manifest']
        data=read_config(root/'.mcp.json',ctx['errors'],plugin['source'])
        servers=_dict(data.get('mcpServers',data))
        declared=manifest.get('mcpServers')
        if isinstance(declared,dict):
            servers.update(_dict(declared.get('mcpServers',declared)))
        else:
            for relative in [declared] if isinstance(declared,str) else _list(declared):
                if not isinstance(relative,str):continue
                path=(root/relative).resolve()
                if not path.is_relative_to(root):
                    ctx['errors'].append({'source':plugin['source'],'code':'plugin_path_outside_root'});continue
                data=read_config(path,ctx['errors'],plugin['source'])
                servers.update(_dict(data.get('mcpServers',data)))
        if _invalid_servers(servers):
            ctx['errors'].append({'source':plugin['source'],'code':'mcp_definition_invalid'})
        for row in _server_entries(servers,ctx['harness'],plugin['scope'],plugin['source']):
            name=row['name']
            row.update(name='plugin:'+plugin['key']+':'+name,serverName=name,plugin=plugin['key'],pluginEnabled=plugin['enabled'],sourcePath=str(root),declarations=[{'source':plugin['source'],'scope':plugin['scope'],'path':str(root)}])
            override=_dict(_dict(_dict(ctx['settings'].get('plugins')).get(plugin['key'])).get('mcp_servers')).get(name)
            state=plugin['enabled']
            if _dict(override).get('enabled') is False:state=False
            row['enabled']=False if state is False else row['enabled'] if state is True else None
            row['status']='disabled' if row['enabled'] is False else 'configured' if row['enabled'] is True else 'installed'
            output.append(row)
    return output


def _merge(entries):
    merged={}
    for item in entries:
        row=dict(item); key=row['name']
        inherit=row.pop('_inheritEnabled',False)
        old=merged.get(key)
        if old:
            row['sources']=list(dict.fromkeys(old['sources']+row['sources']))
            row['declarations']=old.get('declarations',[])+row.get('declarations',[])
            if inherit:row['enabled']=old['enabled']
            if row['provider'] in ('codex','opencode','gemini') and row.get('descriptionSource')!='configuration' and old.get('descriptionSource')=='configuration':
                row.update(description=old['description'],descriptionSource='configuration')
            row['status']='invalid' if row.get('status')=='invalid' else 'configured' if row['enabled'] is True else 'disabled' if row['enabled'] is False else 'installed'
        merged[key]=row
    return sorted(merged.values(),key=lambda r:(r['name'].casefold(),r['name']))


def session_capabilities(registry, harness, alias, cwd, *, _context=None):
    ctx=_context or configuration(registry,harness,alias,cwd)
    entries=[]
    if harness=='claude':
        disabled=_list(ctx['settings'].get('disabledMcpjsonServers'))
        for layer in ctx['layers']:
            rows = _server_entries(layer['data'].get('mcpServers'),harness,layer['scope'],layer['source'],disabled if layer['scope']=='project' else ())
            for row in rows:
                row['sourcePath']=str(layer['path'])
                row['declarations']=[{'source':layer['source'],'scope':layer['scope'],'path':str(layer['path'])}]
            entries += rows
        local=_dict(_dict(ctx['userData'].get('projects')).get(str(ctx['project'])))
        entries += _server_entries(local.get('mcpServers'),harness,'local','claude-local')
        # /mcp disable persists separately from project-MCP approval settings.
        disabled_all=set(_list(local.get('disabledMcpServers'))+_list(ctx['userData'].get('disabledMcpServers')))
        for row in entries:
            if row['name'] in disabled_all:row.update(enabled=False,status='disabled')
    else:
        if harness=='grok':
            user=ctx['layers'][0]['data'] if ctx['layers'] else {}
            compat=_dict(user.get('compat'))
            if _dict(compat.get('claude')).get('mcps') is not False:
                entries += _json_mcps(Path.home()/'.claude.json',harness,'compatible','claude-compatible')
                entries += _json_mcps(ctx['project']/'.mcp.json',harness,'project','mcp-json')
            if _dict(compat.get('cursor')).get('mcps') is not False:
                entries += _json_mcps(Path.home()/'.cursor/mcp.json',harness,'compatible','cursor-compatible')
                entries += _json_mcps(ctx['project']/'.cursor/mcp.json',harness,'project','cursor-project')
        for layer in ctx['layers']:
            key='mcp_servers' if harness in ('codex','grok') else 'mcp' if harness=='opencode' else 'mcpServers'
            rows = _server_entries(layer['data'].get(key),harness,layer['scope'],layer['source'],inherit=harness in ('codex','opencode','gemini'))
            for row in rows:
                row['sourcePath']=str(layer['path'])
                row['declarations']=[{'source':layer['source'],'scope':layer['scope'],'path':str(layer['path'])}]
            entries += rows
        if harness=='gemini':
            mcp=_dict(ctx['settings'].get('mcp')); excluded=_list(mcp.get('excluded')); allowed=mcp.get('allowed')
            for row in entries:
                if row['name'] in excluded or isinstance(allowed,list) and row['name'] not in allowed:
                    row.update(enabled=False,status='disabled',_inheritEnabled=False)
    entries += _plugin_mcps_from_context(ctx)
    mcps=_merge(entries)
    status='unsupported' if harness=='shell' else 'unknown' if ctx['home'] is None else 'incomplete' if ctx['errors'] else 'configured' if mcps else 'empty'
    return {'harness':harness,'account':alias,'mcps':mcps,'status':status,
            'confidence':'unknown' if status in ('unknown','unsupported') else 'configuration_files',
            'effectiveNow':None,'runtimeEnabled':None,'limitations':ctx['limitations'],'errors':ctx['errors']}
