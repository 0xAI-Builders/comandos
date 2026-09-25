"""Process-local extension selections. Shared config and authentication stay untouched.

Public bundles contain only metadata. Config copies and resolved environments live
in private operation directories retained for running processes and rollback.
"""
from __future__ import annotations

import ctypes
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import uuid

import capabilities
import session_profiles as profiles

SUPPORTED = {'claude', 'codex', 'grok', 'opencode', 'agy'}
MARKER = 'COMANDOS_EXTENSION_OPERATION_ID'
MANIFEST_ENV = 'COMANDOS_EXTENSION_MANIFEST'
DIGEST_ENV = 'COMANDOS_EXTENSION_MANIFEST_SHA256'
RECEIPT_ENV = 'COMANDOS_EXTENSION_RECEIPT'
RECEIPT_HASH_ENV = 'COMANDOS_EXTENSION_RECEIPT_SHA256'
HELPER = Path(__file__).resolve().parents[1] / 'bin/cc-extension-session'


def _parse_toml(text):
    try:
        import tomllib
        return tomllib.loads(text)
    except ImportError:
        binary=shutil.which('python3.11')
        if not binary: raise ValueError('Lector TOML no disponible.')
        result=subprocess.run([binary,'-I','-c','import json,sys,tomllib; print(json.dumps(tomllib.loads(sys.stdin.read())))'],
                              input=text,text=True,capture_output=True,timeout=3)
        if result.returncode: raise ValueError('Configuración TOML no soportada.')
        return json.loads(result.stdout)


def _read(path):
    path=Path(path)
    try:
        if not path.exists():return {}
        if path.stat().st_size>4_000_000:raise ValueError('too large')
        text=path.read_text()
        data=_parse_toml(text) if path.suffix=='.toml' else capabilities._jsonc(text)
        if not isinstance(data,dict):raise ValueError('object required')
        return data
    except (OSError,ValueError,subprocess.SubprocessError):
        raise ValueError('Configuración ilegible; no se puede preparar el lanzamiento aislado.') from None


def _proxy_entry(kind,name):
    launcher=str(Path.home()/'.local/bin/cc-extensions')
    if kind=='opencode':return {'type':'local','command':[launcher,'serve',name],'enabled':True}
    result={'command':launcher,'args':['serve',name]}
    if kind=='claude':result['type']='stdio'
    if kind=='agy':result['disabled']=False
    if kind in ('codex','grok'):result['startup_timeout_sec']=45
    return result


def _private(path, data):
    raw = data if isinstance(data, bytes) else json.dumps(data).encode()
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'wb') as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())


def _internal_inventory(registry, harness, account, cwd):
    if harness not in SUPPORTED:
        raise ValueError('CLI sin adaptador de extensiones por proceso.')
    ctx = capabilities.configuration(registry, harness, account, cwd)
    base = capabilities.session_capabilities(registry, harness, account, cwd, _context=ctx)
    caps = {'skills': {'supported': True}}
    skills = profiles._skills(ctx, caps)
    mcps = [dict(r, id=r['name']) for r in base['mcps']]
    shared = _read(Path.home()/'.config/comandos/extensions/catalog.json').get('servers', {})
    existing = {r['name'] for r in mcps}
    for name, spec in shared.items():
        if name not in existing and re.fullmatch(r'[A-Za-z0-9_-]+',name):
            mcps.append({'id':name,'name':name,'enabled':spec.get('enabled') is not False,'scope':'shared','source':'shared-catalog',
                         'synthetic':True})
    settings = ctx['settings']
    if harness == 'grok':
        for row in mcps:
            if row['name'] in settings.get('disabled_mcp_servers', []): row['enabled'] = False
    if harness == 'agy':
        skill_settings = _read(ctx['home']/'config/skills.json')
        for row in skills:
            if row['name'] in skill_settings.get('exclude', []): row['enabled'] = False
    for row in skills:
        if harness == 'claude' and not row.get('plugin'):
            if settings.get('skillOverrides', {}).get(row['name']) == 'off': row['enabled'] = False
        if harness == 'opencode':
            permissions = settings.get('permission', {}).get('skill', {})
            if permissions == 'deny' or isinstance(permissions,dict) and permissions.get(row['name'],permissions.get('*')) == 'deny':
                row['enabled'] = False
        path = Path(row['path'])
        shared_root = (Path.home()/'.agents/skills').resolve()
        if path.is_relative_to(shared_root) and not row.get('plugin'):
            row['id'] = 'shared:' + row['name']
        elif row.get('plugin'):
            row['id'] = 'plugin-skill:' + row['plugin'] + ':' + row['name']
        else:
            row['id'] = 'skill:' + row['id']
        row['toggleable'] = row.get('status') != 'shadowed'
        row['reason'] = ''
        if row.get('plugin') and harness == 'claude':
            row['toggleable'] = False
            row['reason'] = 'Se controla mediante el grupo del plugin completo.'
        elif row.get('plugin') and row.get('enabled') is not True:
            row['toggleable'] = False
            row['reason'] = 'El plugin está desactivado o su carga no está confirmada.'
    # Claude exposes one group switch because skillOverrides ignores plugin skills.
    if harness == 'claude':
        for plugin in ctx['plugins']:
            skills.append({'id':'plugin:'+plugin['key'], 'name':plugin['name'], 'plugin':plugin['key'],
                           'group':True,'scope':plugin['scope'],'source':plugin['source'],
                           'enabled':plugin['enabled'],'toggleable':type(plugin['enabled']) is bool,
                           'reason':'Activa o desactiva todas las skills y hooks del plugin.'})
    for row in mcps:
        row['toggleable'] = not row.get('plugin') and not row.get('unavailable')
        row['reason'] = '' if row['toggleable'] else 'Declaración no controlable individualmente por este adaptador.'
        if harness == 'codex' and '.' in row['name']:
            row.update(toggleable=False, reason='Codex no admite componentes con puntos en sus overrides CLI.')
        if row['name'] in shared and shared[row['name']].get('enabled') is False:
            row.update(enabled=False,toggleable=False,enforceDisabled=True,reason='Desactivado en el catálogo compartido; no se activa por sesión.')
    for row in mcps + skills:
        if type(row.get('enabled')) is not bool:
            row.update(toggleable=False,reason='Estado configurado desconocido; se conserva sin cambios.')
        if ctx['errors']:
            row.update(toggleable=False,reason='Inventario incompleto; corrige la configuración antes de cambiar extensiones.')
    return ctx, {'harness':harness,'account':account,'mcps':mcps,'skills':skills,
                 'limitations':ctx['limitations'] + ['El catálogo previo puede permanecer en el historial al reanudar.',
                    'La verificación comprueba configuración del proceso, no conexiones MCP ni disponibilidad del proveedor.'],
                 'status':'incomplete' if ctx['errors'] else 'configured'}


def inventory(registry, harness, account, cwd):
    """Sanitized catalog; raw MCP IDs, namespaced skill IDs, plugin groups explicit."""
    _, inv = _internal_inventory(registry,harness,account,cwd)
    fields = ('id','name','enabled','toggleable','reason','scope','source','plugin','group')
    return {**inv, **{kind:[{k:row[k] for k in fields if k in row} for row in inv[kind]] for kind in ('mcps','skills')}}


def _normalize(inv, selection):
    if not isinstance(selection,dict) or set(selection)-{'mcps','skills'}:
        raise ValueError('Selección de extensiones inválida.')
    result = {}
    for kind in ('mcps','skills'):
        supplied = selection.get(kind,{})
        rows = {r['id']:r for r in inv[kind]}
        if not isinstance(supplied,dict) or set(supplied)-rows.keys() or any(type(v) is not bool for v in supplied.values()):
            raise ValueError('Selección desconocida o estado de extensión inválido.')
        for ident, value in supplied.items():
            if not rows[ident]['toggleable'] and value != rows[ident]['enabled']:
                raise ValueError('Esta extensión no admite cambios aislados: '+rows[ident]['name'])
        result[kind] = {ident:supplied.get(ident,row['enabled']) for ident,row in rows.items() if type(row['enabled']) is bool}
    return result


def _toml(value):
    if isinstance(value,dict): return '{'+','.join(json.dumps(k)+'='+_toml(v) for k,v in value.items())+'}'
    if isinstance(value,list): return '['+','.join(_toml(v) for v in value)+']'
    if type(value) is bool: return str(value).lower()
    if isinstance(value,(str,int,float)): return json.dumps(value)
    raise ValueError('Override TOML no soportado.')


def prepare_launch(registry, harness, account, cwd, selection, runtime_dir, operation_id):
    if not isinstance(operation_id,str) or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]{0,100}',operation_id):
        raise ValueError('Identificador de operación inválido.')
    ctx, inv = _internal_inventory(registry,harness,account,cwd)
    chosen = _normalize(inv,selection)
    if ctx['errors']: raise ValueError('Inventario incompleto; no se puede garantizar el lanzamiento aislado.')
    root = Path(runtime_dir).resolve()
    root.mkdir(mode=0o700,parents=True,exist_ok=True)
    if root.is_symlink() or root.stat().st_uid != os.getuid(): raise ValueError('Directorio privado inválido.')
    os.chmod(root,0o700)
    private = Path(tempfile.mkdtemp(prefix='extensions-'+operation_id+'-',dir=root))
    args, env, mounts, artifacts = [], {}, [], {}
    home = ctx['home']
    selected_mcps = {r['name']:chosen['mcps'][r['id']] for r in inv['mcps'] if r['toggleable'] or (r.get('enforceDisabled') and not r.get('synthetic'))}
    selected_skills = [(r,chosen['skills'][r['id']]) for r in inv['skills'] if r['toggleable']]
    def file(name, data):
        path = private/name; _private(path,data)
        artifacts[str(path)] = hashlib.sha256(path.read_bytes()).hexdigest()
        return str(path)
    def mount(target, data):
        target = Path(target)
        if not target.is_file() or target.is_symlink():
            raise ValueError('Falta un archivo de configuración montable; no se modifica el archivo global.')
        source = file('mount-'+str(len(mounts))+target.suffix,data)
        mounts.append({'source':source,'target':str(target.resolve()),'sha256':hashlib.sha256(Path(source).read_bytes()).hexdigest()})
    synthetic = {r['name']:_proxy_entry(harness,r['name'])
                 for r in inv['mcps'] if r.get('synthetic') and chosen['mcps'][r['id']] is True}
    if harness == 'codex':
        for name,spec in synthetic.items(): args += ['-c','mcp_servers.'+name+'='+_toml(spec)]
        entries = profiles._skill_overrides(profiles._codex_configs(home,cwd))
        entries += [{'path':row['path'],'enabled':enabled} for row,enabled in selected_skills]
        args += ['-c','skills.config='+_toml(entries)]
        for name, enabled in selected_mcps.items(): args += ['-c','mcp_servers.'+name+'.enabled='+_toml(enabled)]
    elif harness == 'claude':
        servers = {}
        for layer in ctx['layers']: servers.update(layer['data'].get('mcpServers',{}))
        servers.update(synthetic)
        local = ctx['userData'].get('projects',{}).get(str(ctx['project']),{})
        servers.update(local.get('mcpServers',{}))
        # Strict mode excludes plugin MCPs: refuse to silently drop an active one.
        if any(r.get('plugin') and r['enabled'] is not False and chosen['skills'].get('plugin:'+r['plugin']) is not False for r in inv['mcps']):
            raise ValueError('Claude strict no puede conservar este MCP de plugin; requiere un adaptador del plugin.')
        servers = {n:s for n,s in servers.items() if chosen['mcps'].get(n) is not False}
        if '${' in json.dumps(servers) or any(isinstance(s,dict) and s.get('cwd') and not os.path.isabs(s['cwd']) for s in servers.values()):
            raise ValueError('MCP con expansión o cwd relativo no admite traslado aislado.')
        settings = {'skillOverrides':{},'enabledPlugins':{}}
        for row,enabled in selected_skills:
            if row.get('group'): settings['enabledPlugins'][row['plugin']] = enabled
            else: settings['skillOverrides'][row['name']] = 'on' if enabled else 'off'
        args += ['--settings',file('settings.json',settings),'--strict-mcp-config','--mcp-config',file('mcp.json',{'mcpServers':servers})]
    elif harness == 'opencode':
        # Only selected extensions belong in this overlay. The caller's complete
        # environment is merged at execution, after env -u/assignments resolve.
        overlay = {}
        overlay = capabilities._deep_merge(overlay,{'mcp':{n:{'enabled':enabled} for n,enabled in selected_mcps.items()}})
        overlay['mcp'].update(synthetic)
        if isinstance(overlay.get('permission'),str): overlay['permission']={'*':overlay['permission']}
        permission = overlay.setdefault('permission',{})
        existing = permission.get('skill',{})
        permission['skill'] = dict(existing) if isinstance(existing,dict) else {'*':existing}
        permission['skill'].update({row['name']:'allow' if enabled else 'deny' for row,enabled in selected_skills})
        env['OPENCODE_CONFIG_CONTENT'] = json.dumps(overlay,separators=(',',':'))
    elif harness == 'grok':
        # Project config has precedence; cover each existing contributing config.
        for layer in ctx['layers']:
            target = layer['path']
            if not target.exists(): continue
            data = _read(target)
            data.setdefault('mcp_servers',{}).update(synthetic)
            previous = set(data.get('disabled_mcp_servers',[]))
            data['disabled_mcp_servers'] = sorted((previous-set(selected_mcps))|{n for n,v in selected_mcps.items() if not v})
            for name, enabled in selected_mcps.items():
                if name in data.get('mcp_servers',{}): data['mcp_servers'][name]['enabled'] = enabled
            cfg = data.setdefault('skills',{})
            names = {r['name']:v for r,v in selected_skills}
            cfg['disabled'] = sorted((set(cfg.get('disabled',[]))-set(names))|{n for n,v in names.items() if not v})
            mount(target,('\n'.join(json.dumps(k)+'='+_toml(v) for k,v in data.items())+'\n').encode())
        if not mounts: raise ValueError('Falta config.toml para el montaje aislado de Grok.')
        args += ['--leader-socket',str(private/'leader.sock')]
    elif harness == 'agy':
        data = _read(home/'config/mcp_config.json')
        data.setdefault('mcpServers',{}).update(synthetic)
        data['mcpServers'] = {n:s for n,s in data.get('mcpServers',{}).items() if chosen['mcps'].get(n) is not False}
        for name,enabled in selected_mcps.items():
            if name in data['mcpServers']: data['mcpServers'][name]['disabled'] = not enabled
        mount(home/'config/mcp_config.json',data)
        for layer in ctx['layers']:
            if layer['scope']=='project' and layer['path'].exists():
                project_data=_read(layer['path'])
                project_data['mcpServers']={n:spec for n,spec in project_data.get('mcpServers',{}).items() if chosen['mcps'].get(n) is not False}
                for name,enabled in selected_mcps.items():
                    if name in project_data['mcpServers']: project_data['mcpServers'][name]['disabled']=not enabled
                mount(layer['path'],project_data)
        data = _read(home/'config/skills.json')
        names = {r['name']:v for r,v in selected_skills}
        data['exclude'] = sorted((set(data.get('exclude',[]))-set(names))|{n for n,v in names.items() if not v})
        if (home/'config/skills.json').exists():
            mount(home/'config/skills.json',data)
        else:
            # Bind each discovered skill root, never a home/auth/session directory.
            # Copies preserve resources when the original skill root is an alias
            # of ~/.agents/skills; symlinks back into that root would recurse.
            roots = {root.resolve() for root,_,_,_ in profiles._skill_roots(ctx) if root.is_dir()}
            for target in roots:
                source = private / ('skills-'+str(len(mounts)))
                source.mkdir(mode=0o700)
                for row,enabled in selected_skills:
                    path = Path(row['path'])
                    if enabled and path.is_relative_to(target):
                        destination = source/path.parent.relative_to(target)
                        shutil.copytree(path.parent,destination,symlinks=False,dirs_exist_ok=True)
                for child in source.rglob('*'):
                    os.chmod(child,0o700 if child.is_dir() else 0o600)
                mounts.append({'source':str(source),'target':str(target),'kind':'directory'})
    if mounts: namespace_preflight(str(private))
    manifest = private/'manifest.json'
    bundle = {'manifest':str(manifest),'selection':chosen,'operationId':operation_id,'harness':harness,
              'method':'namespace' if mounts else 'native','verification':{'marker':MARKER,'evidence':'manifest+process-configuration'}}
    _private(manifest,{'version':1,'bundle':bundle,'args':args,'env':env,'mounts':mounts,
                       'codexSelectedSkillCount':len(selected_skills),'artifacts':artifacts,'cwd':str(ctx['project']),'home':str(Path.home()),'account':account})
    return bundle


def wrap_command(command, launch):
    """Wrap a simple shell command, preserving its argv and environment prefix."""
    if not isinstance(command,str): raise ValueError('Comando inválido.')
    words = shlex.split(command)
    if not words or any(w in (';','&&','||','|','>','<','&') for w in words):
        raise ValueError('Se requiere un comando simple para aplicar extensiones.')
    data = _load_manifest(launch['manifest'],launch)
    _resolve_command(data,words)  # Fail before a caller stops the running agent.
    return shlex.join([str(HELPER),'--manifest',launch['manifest'],'--',*words])


OPENCODE_ENV_KEYS = ('OPENCODE_CONFIG_CONTENT','OPENCODE_PERMISSION','OPENCODE_CONFIG','OPENCODE_CONFIG_DIR')


def capture_opencode_environment(source, runtime_dir, inventory, managed=False):
    """Capture only supported CLI overrides; returned reference contains no values."""
    values={key:source[key.encode()].decode() for key in OPENCODE_ENV_KEYS if key.encode() in source}
    if values.get('OPENCODE_CONFIG') or values.get('OPENCODE_CONFIG_DIR'):
        raise ValueError('OpenCode usa un catálogo alternativo no compatible; el agente sigue abierto')
    try:
        content=json.loads(values.get('OPENCODE_CONFIG_CONTENT','{}'))
        if not isinstance(content,dict): raise ValueError()
        known={row['id'] for row in inventory['mcps']}
        if set(content.get('mcp',{}))-known: raise ValueError()
        if content.get('plugin') or ((content.get('skills') or content.get('mcp')) and not managed): raise ValueError()
        permission=json.loads(values.get('OPENCODE_PERMISSION','{}'))
        if not isinstance(permission,(dict,str)): raise ValueError()
    except (ValueError,TypeError):
        raise ValueError('OpenCode contiene overrides no compatibles; el agente sigue abierto') from None
    directory=Path(runtime_dir);directory.mkdir(mode=0o700,parents=True,exist_ok=True)
    if directory.stat().st_uid!=os.getuid() or directory.stat().st_mode & 0o077:
        raise ValueError('Directorio de entorno no privado')
    path=directory/('environment-'+uuid.uuid4().hex+'.json')
    _private(path,{'version':1,'values':values})
    return {'path':str(path),'sha256':hashlib.sha256(path.read_bytes()).hexdigest()}


def wrap_environment(command, reference):
    return shlex.join([str(HELPER),'--environment-file',reference['path'],'--environment-sha256',reference['sha256'],'--',*shlex.split(command)])


def run_environment(path, digest, command):
    path=Path(path);stat=path.stat();parent=path.parent.stat()
    if path.is_symlink() or stat.st_uid!=os.getuid() or stat.st_mode & 0o077 or parent.st_uid!=os.getuid() or parent.st_mode & 0o077:
        raise ValueError('Entorno privado inválido')
    raw=path.read_bytes()
    if hashlib.sha256(raw).hexdigest()!=digest: raise ValueError('Entorno privado modificado')
    data=json.loads(raw);values=data['values']
    if data['version']!=1 or set(values)-set(OPENCODE_ENV_KEYS): raise ValueError('Entorno incompatible')
    env=dict(os.environ)
    for key in OPENCODE_ENV_KEYS: env.pop(key,None)
    env.update(values)
    os.execvpe(command[0],command,env)


def _load_manifest(path, bundle=None):
    path = Path(path)
    stat = path.stat()
    if path.is_symlink() or stat.st_uid != os.getuid() or stat.st_mode & 0o077:
        raise ValueError('Manifest privado inválido.')
    data = json.loads(path.read_text())
    if data.get('version') != 1 or (bundle is not None and data.get('bundle') != bundle):
        raise ValueError('Manifest incompatible con el lanzamiento.')
    parent = path.parent.stat()
    if parent.st_uid != os.getuid() or parent.st_mode & 0o077:
        raise ValueError('Directorio del manifest no es privado.')
    for name,digest in data.get('artifacts',{}).items():
        artifact=Path(name)
        if artifact.parent != path.parent or artifact.is_symlink() or artifact.stat().st_mode & 0o077:
            raise ValueError('Artefacto privado inválido.')
        if hashlib.sha256(artifact.read_bytes()).hexdigest()!=digest:
            raise ValueError('El artefacto de lanzamiento cambió después de prepararse.')
    return data


def _namespace(mounts):
    """Create a user/mount namespace mapping only this uid; no shadow HOME."""
    libc = ctypes.CDLL(None,use_errno=True)
    uid,gid = os.getuid(),os.getgid()
    def call(result):
        if result != 0: raise OSError(ctypes.get_errno(),'namespace setup failed')
    call(libc.unshare(0x10000000 | 0x00020000))
    Path('/proc/self/setgroups').write_text('deny')
    Path('/proc/self/uid_map').write_text(f'{uid} {uid} 1')
    Path('/proc/self/gid_map').write_text(f'{gid} {gid} 1')
    call(libc.mount(None,b'/',None,ctypes.c_ulong((1<<18)|(1<<14)),None))
    for entry in mounts:
        call(libc.mount(os.fsencode(entry['source']),os.fsencode(entry['target']),None,ctypes.c_ulong(4096),None))
    # Exec as non-root clears namespace capabilities; prevent setuid escalation.
    call(libc.prctl(38,1,0,0,0))


def namespace_preflight(runtime_dir):
    """Real fixture child proof: mount isolation, original HOME and zero exec caps."""
    with tempfile.TemporaryDirectory(prefix='namespace-proof-',dir=runtime_dir) as directory:
        root=Path(directory); target=root/'target'; source=root/'source'
        target.write_text('parent'); source.write_text('child')
        os.chmod(target,0o600); os.chmod(source,0o600)
        proof = root/'proof.json'
        _private(proof,{'mounts':[{'source':str(source),'target':str(target)}], 'target':str(target),'home':os.environ.get('HOME','')})
        run = subprocess.run([sys.executable,str(HELPER),'--namespace-proof',str(proof)],capture_output=True,timeout=10)
        if run.returncode or run.stdout.strip()!=b'namespace-proof-ok' or target.read_text()!='parent':
            raise ValueError('El aislamiento user/mount namespace no está disponible; no se detuvo el agente.')
    return True


def _process_env(pid):
    values=Path(f'/proc/{int(pid)}/environ').read_bytes().split(b'\0')
    return dict(v.decode(errors='surrogateescape').split('=',1) for v in values if b'=' in v)


def verify_launch(pid, launch):
    """Verify configuration evidence, not MCP connections or a model's tool access."""
    try:
        data = _load_manifest(launch['manifest'],launch)
        env = _process_env(pid)
        if env.get(MARKER)!=launch['operationId'] or env.get(MANIFEST_ENV)!=launch['manifest']: return False
        digest=hashlib.sha256(Path(launch['manifest']).read_bytes()).hexdigest()
        if any(hashlib.sha256(Path(p).read_bytes()).hexdigest()!=h for p,h in data.get('artifacts',{}).items()): return False
        if env.get(DIGEST_ENV)!=digest or env.get('HOME')!=data['home']: return False
        receipt_path = Path(env.get(RECEIPT_ENV,''))
        if receipt_path.parent != Path(launch['manifest']).parent: return False
        receipt_stat = receipt_path.stat()
        if receipt_path.is_symlink() or receipt_stat.st_uid != os.getuid() or receipt_stat.st_mode & 0o077: return False
        receipt_raw = receipt_path.read_bytes()
        if hashlib.sha256(receipt_raw).hexdigest() != env.get(RECEIPT_HASH_ENV): return False
        receipt = json.loads(receipt_raw)
        if receipt.get('manifestSha256') != digest: return False
        if any(env.get(key)!=value for key,value in receipt['env'].items()): return False
        if any(hashlib.sha256(Path(p).read_bytes()).hexdigest()!=h for p,h in receipt['artifacts'].items()): return False
        argv=Path(f'/proc/{int(pid)}/cmdline').read_bytes().decode().rstrip('\0').split('\0')
        expected = receipt['argv'][1:]
        if expected and argv[-len(expected):] != expected: return False
        if data['mounts']:
            if os.readlink(f'/proc/{pid}/ns/mnt')==os.readlink('/proc/self/ns/mnt'): return False
            info=Path(f'/proc/{pid}/mountinfo').read_text()
            for mount in data['mounts']:
                escaped=mount['target'].replace('\\','\\134').replace(' ','\\040').replace('\t','\\011').replace('\n','\\012')
                if not any(line.split()[4]==escaped for line in info.splitlines()): return False
                child=Path(f'/proc/{pid}/root')/mount['target'].lstrip('/')
                if mount.get('kind') == 'directory':
                    original=Path(mount['source']).stat(); mounted=child.stat()
                    if (original.st_dev,original.st_ino)!=(mounted.st_dev,mounted.st_ino): return False
                elif hashlib.sha256(child.read_bytes()).hexdigest()!=mount['sha256']: return False
        return True
    except (OSError,ValueError,KeyError,TypeError):
        return False


def launch_from_pid(pid):
    """Reconstruct a sanitized bundle only after live evidence verifies it."""
    try:
        path=_process_env(pid).get(MANIFEST_ENV)
        if not path: return None
        bundle=_load_manifest(path)['bundle']
        return bundle if verify_launch(pid,bundle) else None
    except (OSError,ValueError,KeyError,TypeError): return None


def _resolve_command(data, command):
    """Resolve existing environment/extension overrides without executing the CLI."""
    command = list(command)
    env = dict(os.environ)
    if command and command[0] == 'env':
        command.pop(0)
        while command and command[0].startswith('-'):
            flag = command.pop(0)
            if flag in ('-u','--unset') and command:
                env.pop(command.pop(0),None)
            elif flag.startswith('--unset='):
                env.pop(flag.split('=',1)[1],None)
            elif flag == '--': break
            else: raise ValueError('Opción env no soportada por el lanzamiento aislado.')
    while command and re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*=.*',command[0],re.S):
        key,value=command.pop(0).split('=',1);env[key]=value
    if not command or env.get('HOME')!=data['home']:
        raise ValueError('El comando intenta cambiar HOME o está vacío.')
    extra = list(data['args'])
    generated_settings = None
    harness = data['bundle']['harness']
    if harness == 'opencode':
        try:
            prior = json.loads(env.get('OPENCODE_CONFIG_CONTENT','{}'))
            if isinstance(prior.get('permission'),str): prior['permission']={'*':prior['permission']}
            selected = json.loads(data['env']['OPENCODE_CONFIG_CONTENT'])
            env['OPENCODE_CONFIG_CONTENT'] = json.dumps(capabilities._deep_merge(prior,selected),separators=(',',':'))
        except (ValueError,TypeError): raise ValueError('Overlay OpenCode inválido.') from None
    else:
        env.update(data['env'])
    if harness == 'codex':
        old = []
        for index,word in enumerate(command):
            value = command[index+1] if word in ('-c','--config') and index+1<len(command) else word[len('--config='):] if word.startswith('--config=') else ''
            if value.startswith('skills.config='):
                try: old = _parse_toml(value)['skills']['config']
                except (ValueError,KeyError): raise ValueError('Override previo de skills inválido.') from None
        if old:
            for index,word in enumerate(extra):
                if word.startswith('skills.config='):
                    rules = _parse_toml(word)['skills']['config']
                    count = data['codexSelectedSkillCount']
                    before, selected = (rules[:-count],rules[-count:]) if count else (rules,[])
                    extra[index] = 'skills.config='+_toml(before+old+selected)
    if harness == 'claude':
        generated_settings = {}
        cleaned = []; index = 0
        while index < len(command):
            word = command[index];index += 1
            if word == '--settings' or word.startswith('--settings='):
                if word == '--settings':
                    if index >= len(command): raise ValueError('Settings de Claude incompletos.')
                    value=command[index];index+=1
                else: value=word.split('=',1)[1]
                try:
                    old = json.loads(value) if value.lstrip().startswith('{') else json.loads(Path(value).read_text())
                    generated_settings = capabilities._deep_merge(generated_settings,old)
                except (ValueError,OSError,TypeError): raise ValueError('Settings de Claude ilegibles.') from None
            elif word == '--mcp-config':
                while index<len(command) and not command[index].startswith('-'): index+=1
            elif word.startswith('--mcp-config=') or word in ('--strict-mcp-config','--disable-slash-commands'):
                continue
            else: cleaned.append(word)
        command=cleaned
        selected=json.loads(Path(extra[extra.index('--settings')+1]).read_text())
        generated_settings=capabilities._deep_merge(generated_settings,selected)
    return command, extra, env, generated_settings


def run_manifest(path, command):
    data=_load_manifest(path)
    command, extra, env, settings = _resolve_command(data,command)
    artifacts = {}
    if settings is not None:
        settings_path = Path(path).parent / ('settings-run-'+uuid.uuid4().hex+'.json')
        _private(settings_path,settings)
        artifacts[str(settings_path)] = hashlib.sha256(settings_path.read_bytes()).hexdigest()
        extra[extra.index('--settings')+1] = str(settings_path)
    argv = [*command,*extra]
    digest = hashlib.sha256(Path(path).read_bytes()).hexdigest()
    receipt = Path(path).parent / ('receipt-'+uuid.uuid4().hex+'.json')
    receipt_env = {key:env.get(key) for key in set(data['env']) | (set(OPENCODE_ENV_KEYS) if data['bundle']['harness']=='opencode' else set())}
    _private(receipt,{'manifestSha256':digest,'argv':argv,'env':receipt_env,'artifacts':artifacts})
    env.update({MARKER:data['bundle']['operationId'],MANIFEST_ENV:str(path),DIGEST_ENV:digest,
                RECEIPT_ENV:str(receipt),RECEIPT_HASH_ENV:hashlib.sha256(receipt.read_bytes()).hexdigest()})
    if data['mounts']: _namespace(data['mounts'])
    os.execvpe(argv[0],argv,env)


def main(argv=None):
    import argparse
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest');parser.add_argument('--namespace-proof')
    parser.add_argument('--environment-file');parser.add_argument('--environment-sha256')
    parser.add_argument('command',nargs=argparse.REMAINDER)
    args=parser.parse_args(argv)
    try:
        if args.environment_file:
            run_environment(args.environment_file,args.environment_sha256,args.command[1:] if args.command[:1]==['--'] else args.command)
        if args.namespace_proof:
            proof=json.loads(Path(args.namespace_proof).read_text())
            _namespace(proof['mounts'])
            code='import os,pathlib,sys; assert pathlib.Path(sys.argv[1]).read_text()=="child"; assert os.environ.get("HOME","")==sys.argv[2]; assert int(next(x.split()[1] for x in pathlib.Path("/proc/self/status").read_text().splitlines() if x.startswith("CapEff:")),16)==0; print("namespace-proof-ok")'
            os.execv(sys.executable,[sys.executable,'-c',code,proof['target'],proof['home']])
        run_manifest(args.manifest,args.command[1:] if args.command[:1]==['--'] else args.command)
    except Exception:
        print('No se pudo aplicar la configuración privada de extensiones.',file=sys.stderr)
        return 1
    return 0
