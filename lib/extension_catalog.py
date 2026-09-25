"""Private shared extension catalog and loss-checked native configuration writes."""
from __future__ import annotations

import contextlib
import copy
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import tempfile
import time
import uuid
from urllib.parse import urlsplit

import tomlkit

ALIASES = {'x_suite': 'x-suite', 'linear-server': 'linear', 'chrome-devtools': 'chrome-bg',
           'chrome-devtools-current': 'chrome-bg', 'chrome-current': 'chrome-bg'}
# These runtimes have not been migrated to the remote Mac. Keep their definitions
# recoverable, but do not start them implicitly through the shared catalog.
REMOTE_UNVERIFIED = {'playwright', 'x-playwright', 'lightpanda', 'obscura', 'screenwright'}
NATIVE_ONLY = {'node_repl'}
SAFE_NAME = re.compile(r'^[A-Za-z0-9_-]+$')
CLIENT_FIELDS = ('tools','default_tools_approval_mode','enabled_tools','disabled_tools',
                 'required','startup_timeout_sec','tool_timeout_sec')


class CatalogError(Exception):
    """Messages contain names and paths, never configuration values."""


def state_dir(home):
    return Path(home) / '.local/state/comandos/extensions'


def catalog_path(home):
    return Path(home) / '.config/comandos/extensions/catalog.json'


def private_write(path, data):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    fd, tmp = tempfile.mkstemp(prefix='.' + path.name, dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as stream:
            os.fchmod(stream.fileno(), 0o600)
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


def save_json(path, value):
    private_write(path, (json.dumps(value, ensure_ascii=False, indent=2) + '\n').encode())


@contextlib.contextmanager
def locked(home, name='sync'):
    root = state_dir(home)
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    fd = os.open(root / (name + '.lock'), os.O_CREAT | os.O_RDWR, 0o600)
    with os.fdopen(fd, 'w') as stream:
        fcntl.flock(stream, fcntl.LOCK_EX)
        yield


def parse_config(path, raw):
    try:
        text = raw.decode()
        if Path(path).suffix == '.toml':
            return tomlkit.parse(text)
        # Strings must survive both comment and trailing-comma removal.
        token = re.compile(r'"(?:\\.|[^"\\])*"|//[^\n]*|/\*[\s\S]*?\*/')
        text = token.sub(lambda m: m[0] if m[0].startswith('"') else ' ', text)
        text = re.sub(r'("(?:\\.|[^"\\])*")|,\s*([}\]])',
                      lambda m: m[1] if m[1] is not None else m[2], text)
        def unique(pairs):
            out = {}
            for k, v in pairs:
                if k in out:
                    raise ValueError('duplicate key')
                out[k] = v
            return out
        data = json.loads(text, object_pairs_hook=unique)
        if not isinstance(data, dict):
            raise ValueError('object required')
        return data
    except (ValueError, UnicodeError, tomlkit.exceptions.ParseError):
        raise CatalogError('Invalid configuration: ' + str(path)) from None


def read_config(path):
    path = Path(path)
    return parse_config(path, path.read_bytes()) if path.exists() else {}


def targets(home):
    h = Path(home)
    specs = [('claude', h / '.claude.json', 'mcpServers'),
             ('codex', h / '.codex/config.toml', 'mcp_servers'),
             ('grok', h / '.grok/config.toml', 'mcp_servers'),
             ('opencode', h / '.config/opencode/opencode.json', 'mcp'),
             ('agy', h / '.gemini/config/mcp_config.json', 'mcpServers')]
    p = h / '.config/opencode/opencode.jsonc'
    if p.exists():
        specs.append(('opencode', p, 'mcp'))
    for kind, file, key in [('claude', '.claude.json', 'mcpServers'),
                             ('codex', 'config.toml', 'mcp_servers'),
                             ('grok', 'config.toml', 'mcp_servers')]:
        for root in sorted(h.glob('.' + kind + '-accounts/*')):
            if root.is_dir() and not root.is_symlink():
                specs.append((kind, root / file, key))
    return [{'kind': k, 'path': p, 'key': key} for k, p, key in specs]


def normalize(spec, disabled=()):
    spec = copy.deepcopy(dict(spec))
    if not spec.get('command') and not any(spec.get(x) for x in ('url','serverUrl','httpUrl')):
        return None
    out = {'enabled': spec.get('enabled') is not False and not spec.get('disabled', False)}
    if spec.get('command'):
        command = spec['command']
        out['command'] = command[0] if isinstance(command, list) else command
        out['args'] = command[1:] if isinstance(command, list) else spec.get('args', [])
        for key in ('cwd', 'env', 'env_vars'):
            if key in spec:
                out[key] = spec[key]
        if 'environment' in spec:
            out['env'] = spec['environment']
    else:
        out['url'] = spec.get('url') or spec.get('serverUrl') or spec.get('httpUrl')
        out['transport'] = 'sse' if spec.get('type') == 'sse' else 'http'
        out['headers'] = spec.get('headers') or spec.get('http_headers') or {}
        for key in ('bearer_token_env_var', 'env_http_headers'):
            if key in spec:
                out[key] = spec[key]
    if spec.get('disabled_tools'):
        out['disabled_tools'] = spec['disabled_tools']
    if spec.get('enabled_tools') is not None:
        out['enabled_tools'] = spec['enabled_tools']
    return out


def is_managed(spec, launcher):
    command = spec.get('command')
    return command == launcher or (isinstance(command, list) and command[:1] == [launcher])


def import_catalog(home):
    servers, candidates = {}, []
    for target in targets(home):
        data = read_config(target['path'])
        for project in data.get('projects', {}).values():
            candidates += [(n, c, ()) for n, c in project.get('mcpServers', {}).items()]
        candidates += [(n, c, data.get('disabled_mcp_servers', [])) for n, c in data.get(target['key'], {}).items()]
    # First import establishes precedence. Later explicit edits are reconciled
    # against snapshots, not this discovery ordering.
    for name, spec, disabled in candidates:
        name = ALIASES.get(name, name)
        if name in NATIVE_ONLY or not SAFE_NAME.fullmatch(name):
            continue
        normalized = normalize(spec)
        if normalized is None:
            continue
        if name in disabled:
            normalized['enabled'] = False
        prefer_local=(urlsplit(normalized.get('url','')).hostname in ('127.0.0.1','localhost')
                      and urlsplit(servers.get(name,{}).get('url','')).hostname not in ('127.0.0.1','localhost'))
        if name not in servers or (not servers[name]['enabled'] and normalized['enabled']) or prefer_local:
            servers[name] = normalized
    if 'chrome-bg' in servers:
        servers['chrome-bg'] = {'command': str(Path(home)/'.local/bin/cc-browser-remote'), 'args': [], 'enabled': True}
    for name in REMOTE_UNVERIFIED & servers.keys():
        servers[name]['enabled'] = False
        servers[name]['unavailable_reason'] = 'Remote browser runtime has not been verified'
    return {'version': 1, 'servers': servers}


def fingerprint(value):
    return hashlib.sha256(json.dumps(value,sort_keys=True,default=str).encode()).hexdigest()


def native_specs(data,key):
    disabled=set(data.get('disabled_mcp_servers',[]))
    return {n:({**s,'enabled':False} if n in disabled else s) for n,s in data.get(key,{}).items()}


def save_snapshot(home,catalog,launcher,expected_targets=None):
    snapshot={'catalog':{n:fingerprint(s) for n,s in catalog['servers'].items()},'targets':{}}
    for target in targets(home):
        data=read_config(target['path'])
        snapshot['targets'][str(target['path'])]={n:fingerprint(s) for n,s in native_specs(data,target['key']).items()}
    if expected_targets is not None and snapshot['targets']!=expected_targets:
        raise CatalogError('MCP configuration changed during synchronization; retry')
    path=state_dir(home)/'snapshot.json'
    if read_config(path)!=snapshot:
        save_json(path,snapshot)


def reconcile(home,catalog,launcher,observed_inputs=None):
    """Adopt explicit native edits since the last successful export."""
    snapshot=read_config(state_dir(home)/'snapshot.json')
    proposals={}
    for target in targets(home):
        previous=snapshot.get('targets',{}).get(str(target['path']))
        data=read_config(target['path']);current=native_specs(data,target['key'])
        if observed_inputs is not None:
            observed_inputs[str(target['path'])]={n:fingerprint(s) for n,s in current.items()}
        if previous is None and not snapshot:
            continue
        previous=previous or {}
        for raw_name in set(previous)|set(current):
            name=ALIASES.get(raw_name,raw_name)
            if name in NATIVE_ONLY or not SAFE_NAME.fullmatch(name):
                continue
            item=current.get(raw_name)
            if item is not None and previous.get(raw_name)==fingerprint(item):
                continue
            if item is None:
                if name not in catalog['servers']:
                    continue
                desired={**catalog['servers'][name],'enabled':False}
            elif is_managed(item,launcher):
                if name not in catalog['servers']:
                    continue
                desired={**catalog['servers'][name],'enabled':item.get('enabled') is not False and not item.get('disabled',False)}
                if desired['enabled']==catalog['servers'][name].get('enabled',True):
                    continue  # Native permission/timeout edits stay client-owned.
            else:
                desired=normalize(item)
                if desired is None:
                    raise CatalogError('Incomplete MCP definition: '+name)
            if name=='chrome-bg':
                desired={'command':str(Path(home)/'.local/bin/cc-browser-remote'),'args':[],'enabled':True}
            if name in REMOTE_UNVERIFIED:
                desired['enabled']=False
                desired['unavailable_reason']='Remote browser runtime has not been verified'
            old=catalog['servers'].get(name)
            expected=snapshot.get('catalog',{}).get(name)
            if expected and fingerprint(old)!=expected and old!=desired:
                raise CatalogError('Simultaneous catalog and native edit: '+name)
            if name in proposals and proposals[name]!=desired:
                raise CatalogError('Conflicting native edits: '+name)
            proposals[name]=desired
    result=copy.deepcopy(catalog)
    result['servers'].update(proposals)
    return result


def replace_config(home, path, before, after):
    path = Path(path)
    if path.is_symlink():
        raise CatalogError('Configuration symlink requires an explicit target: ' + str(path))
    if (path.read_bytes() if path.exists() else None) != before:
        raise CatalogError('Configuration changed concurrently: ' + str(path))
    if before == after:
        return False
    if before is not None:
        root = state_dir(home) / 'backups'
        name = str(path.relative_to(home)).replace('/', '__')
        backup = root / (name + '.' + hashlib.sha256(before).hexdigest()[:16])
        if not backup.exists():
            private_write(backup, before)
    if (path.read_bytes() if path.exists() else None) != before:
        raise CatalogError('Configuration changed during backup: ' + str(path))
    private_write(path, after)
    return True


def native_entry(kind, name, launcher, old):
    if kind == 'opencode':
        return {'type': 'local', 'command': [launcher, 'serve', name], 'enabled': True}
    entry = {'command': launcher, 'args': ['serve', name]}
    if kind == 'claude':
        entry['type'] = 'stdio'
    if kind == 'agy':
        entry['disabled'] = False
    if kind in ('codex', 'grok'):
        # Permission policies are client-owned; migrating transport cannot relax them.
        for key in CLIENT_FIELDS:
            if key in old:
                entry[key] = old[key]
        entry.setdefault('startup_timeout_sec', 45)
    return entry


def sync_configs(home, catalog, launcher,observed=None,expected_inputs=None):
    changed = []
    policy_path=state_dir(home)/'client-policies.json'
    policies=read_config(policy_path)
    enabled = {n:c for n,c in catalog['servers'].items() if c.get('enabled', True)}
    for target in targets(home):
        path, key, kind = target['path'], target['key'], target['kind']
        before = path.read_bytes() if path.exists() else None
        data = parse_config(path, before) if before is not None else (tomlkit.document() if path.suffix=='.toml' else {})
        old = data.get(key, {})
        if expected_inputs is not None and expected_inputs.get(str(path))!={n:fingerprint(s) for n,s in native_specs(data,key).items()}:
            raise CatalogError('MCP configuration changed before export; retry: '+str(path))
        saved=policies.setdefault(str(path),{})
        for n,s in old.items():
            saved[ALIASES.get(n,n)]={k:s[k] for k in CLIENT_FIELDS if k in s}
        # Persist policies before removing disabled native entries.
        if read_config(policy_path)!=policies:
            save_json(policy_path,policies)
        desired = {n: native_entry(kind,n,launcher,saved.get(n,{})) for n in sorted(enabled)}
        for n in NATIVE_ONLY & old.keys():
            desired[n] = old[n]
        if dict(old) != desired:
            data[key] = desired
        # Matching project declarations would otherwise bypass the shared auth.
        # A different project endpoint remains local to that project.
        for project in data.get('projects', {}).values():
            for name, spec in list(project.get('mcpServers', {}).items()):
                canonical = ALIASES.get(name,name)
                if canonical in enabled and normalize(spec) == enabled[canonical]:
                    project['mcpServers'][name] = native_entry(kind,canonical,launcher,{})
        if kind == 'grok':
            data['disabled_mcp_servers'] = [n for n in data.get('disabled_mcp_servers',[]) if n not in enabled]
        if observed is not None:
            observed[str(path)]={n:fingerprint(s) for n,s in native_specs(data,key).items()}
        after = (tomlkit.dumps(data) if path.suffix=='.toml' else json.dumps(data,ensure_ascii=False,indent=2)+'\n').encode()
        if before is not None and parse_config(path,before) == data:
            continue
        if replace_config(home,path,before,after):
            changed.append(str(path))
    return changed


def skill_roots(home):
    h=Path(home)
    roots=[h/p for p in ('.agents/skills','.claude/skills','.codex/skills','.grok/skills',
                         '.config/opencode/skills','.opencode/skills','.gemini/config/skills')]
    for kind in ('claude','codex','grok'):
        roots += [p/'skills' for p in sorted(h.glob('.'+kind+'-accounts/*')) if p.is_dir() and not p.is_symlink()]
    return roots


def skill_fingerprint(path):
    digest=hashlib.sha256()
    for root,dirs,files in os.walk(path,followlinks=False):
        dirs.sort();files.sort()
        for name in dirs+files:
            p=Path(root)/name
            digest.update(str(p.relative_to(path)).encode())
            if p.is_symlink():
                digest.update(b'link:'+str(p.readlink()).encode())
            elif p.is_file():
                with p.open('rb') as stream:
                    for chunk in iter(lambda:stream.read(1024*1024),b''):
                        digest.update(chunk)
    return digest.hexdigest()


def backup_skill(home,path):
    backup=state_dir(home)/'backups'/('skill-'+uuid.uuid4().hex)/path.name
    backup.parent.mkdir(parents=True,mode=0o700)
    shutil.move(str(path),str(backup))


def adopt_skill_reinstalls(home,roots,canonical,previous):
    proposals={}
    if not previous:
        return []
    for root in roots[1:]:
        if not root.exists():continue
        for candidate in root.iterdir():
            if candidate.name.startswith('.') or not (candidate/'SKILL.md').is_file():continue
            target=canonical/candidate.name
            if candidate.resolve()==target.resolve() or not target.exists():continue
            current=skill_fingerprint(target);incoming=skill_fingerprint(candidate)
            if incoming==current:continue
            if current!=previous.get(candidate.name):
                raise CatalogError('Conflicting canonical and native skill edits: '+candidate.name)
            if candidate.name in proposals and proposals[candidate.name][1]!=incoming:
                raise CatalogError('Conflicting native skill edits: '+candidate.name)
            proposals[candidate.name]=(candidate,incoming,current)
    changed=[]
    for name,(source,incoming,current) in proposals.items():
        target=canonical/name;temporary=canonical/('.adopt-'+uuid.uuid4().hex)
        try:
            shutil.copytree(source,temporary,symlinks=False)
            if skill_fingerprint(source)!=incoming or skill_fingerprint(target)!=current:
                raise CatalogError('Skill changed during synchronization: '+name)
            backup_skill(home,target);temporary.rename(target);changed.append(str(target))
        finally:
            if temporary.exists():shutil.rmtree(temporary)
    return changed


def sync_skills(home):
    candidates=skill_roots(home);canonical=candidates[0];canonical.mkdir(parents=True,exist_ok=True)
    roots=[];seen=set()
    for root in candidates:
        resolved=root.resolve()
        if resolved not in seen:
            seen.add(resolved);roots.append(root)
    snapshot_path=state_dir(home)/'skills.json'
    changed=adopt_skill_reinstalls(home,roots,canonical,read_config(snapshot_path))
    # A canonical link into a provider directory would form a cycle when that
    # provider is linked back here. Materialize only these links; repository
    # links outside provider roots remain live.
    for skill in list(canonical.iterdir()):
        if skill.is_symlink() and skill.exists() and any(skill.resolve().is_relative_to(r.resolve()) for r in roots[1:] if r.exists()):
            tmp=canonical/('.materialized-'+uuid.uuid4().hex)
            shutil.copytree(skill,tmp,symlinks=False)
            skill.unlink();tmp.rename(skill)
            changed.append(str(skill))
    for root in roots[1:]:
        if not root.exists():
            continue
        for skill in sorted(root.iterdir()):
            if skill.name.startswith('.') or not (skill/'SKILL.md').is_file():
                continue
            dest=canonical/skill.name
            if not dest.exists():
                # Copy the complete resource tree, never just SKILL.md.
                temporary=canonical/('.import-'+uuid.uuid4().hex)
                try:
                    shutil.copytree(skill,temporary,symlinks=False)
                    temporary.rename(dest)
                finally:
                    if temporary.exists():shutil.rmtree(temporary)
                changed.append(str(dest))
    skills=[p for p in canonical.iterdir() if not p.name.startswith('.') and (p/'SKILL.md').is_file()]
    for root in roots[1:]:
        root.mkdir(parents=True,exist_ok=True)
        for stale in root.iterdir():
            if stale.is_symlink() and stale.readlink().parent==canonical and not stale.exists():
                stale.unlink();changed.append(str(stale))
        for skill in skills:
            dest=root/skill.name
            if dest.is_symlink() and dest.resolve()==skill.resolve():
                continue
            if dest.exists() or dest.is_symlink():
                backup_skill(home,dest)
            dest.symlink_to(skill,target_is_directory=True)
            changed.append(str(dest))
    snapshot={p.name:skill_fingerprint(p) for p in skills}
    if read_config(snapshot_path)!=snapshot:save_json(snapshot_path,snapshot)
    return changed
