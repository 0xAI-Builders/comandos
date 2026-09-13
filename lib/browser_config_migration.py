#!/usr/bin/env python3.11
"""Review and migrate browser MCP configs without logging configuration values."""

import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import stat
import sys
import tempfile
import uuid

if sys.version_info < (3, 11):
    raise SystemExit('browser_config_migration requires Python 3.11 or newer; run python3.11.')
import tomllib

TARGETS = frozenset({'chrome-bg', 'chrome-current', 'chrome-devtools', 'chrome-devtools-current'})
PLUGIN = 'chrome-devtools-mcp@chrome-devtools-plugins'
DEFAULT_WRAPPER = '/home/someguy/.local/bin/cc-browser-remote'
ROOT_VARIABLES = {'CLAUDE_CONFIG_DIR': 'claude', 'CODEX_HOME': 'codex', 'GROK_HOME': 'grok'}


class MigrationError(Exception):
    """A safe, value-free error suitable for command-line reports."""


def digest(data):
    return hashlib.sha256(data).hexdigest()


def transform_json(text, wrapper):
    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise MigrationError('Duplicate JSON key; refusing a potentially lossy rewrite')
            result[key] = value
        return result

    try:
        data = json.loads(text, object_pairs_hook=unique_object)
    except (ValueError, TypeError):
        raise MigrationError('Invalid JSON configuration') from None
    if not isinstance(data, dict):
        raise MigrationError('Configuration must be a JSON object')
    original = copy.deepcopy(data)

    def servers(container):
        entries = container.get('mcpServers')
        if not isinstance(entries, dict) or not TARGETS.intersection(entries):
            return
        for name in TARGETS:
            entries.pop(name, None)
        entries['chrome-bg'] = {'type': 'stdio', 'command': wrapper, 'args': []}

    servers(data)
    projects = data.get('projects', {})
    if isinstance(projects, dict):
        for project in projects.values():
            if isinstance(project, dict):
                servers(project)
    plugins = data.get('enabledPlugins')
    if isinstance(plugins, dict) and PLUGIN in plugins:
        plugins[PLUGIN] = False
    if data == original:
        return text
    return json.dumps(data, ensure_ascii=False, indent=2) + '\n'


def _header_path(line):
    stripped = line.lstrip()
    if not stripped.startswith('['):
        return None
    try:
        node = tomllib.loads(stripped + '\n__migration_marker__ = true\n')
    except tomllib.TOMLDecodeError:
        return None
    keys = []
    while isinstance(node, dict) and '__migration_marker__' not in node and len(node) == 1:
        key, node = next(iter(node.items()))
        keys.append(key)
        if isinstance(node, list) and len(node) == 1:
            node = node[0]
    return keys if isinstance(node, dict) and '__migration_marker__' in node else None


def _string_state(line, state):
    """Track TOML strings so header-shaped multiline contents remain untouched."""
    i = 0
    while i < len(line):
        if state:
            if state.startswith('"') and line[i] == '\\':
                i += 2
                continue
            if line.startswith(state, i):
                i += len(state)
                state = None
            else:
                i += 1
        elif line[i] == '#':
            break
        elif line[i] in ('"', "'"):
            state = line[i] * (3 if line.startswith(line[i] * 3, i) else 1)
            i += len(state)
        else:
            i += 1
    return state


def transform_toml(text, wrapper):
    try:
        data = tomllib.loads(text)
    except tomllib.TOMLDecodeError:
        raise MigrationError('Invalid TOML configuration') from None
    servers = data.get('mcp_servers', {})
    if not isinstance(servers, dict) or not TARGETS.intersection(servers):
        return text
    desired = copy.deepcopy(data)
    for name in TARGETS:
        desired['mcp_servers'].pop(name, None)
    desired['mcp_servers']['chrome-bg'] = {'command': wrapper, 'args': []}
    if desired == data:
        return text
    result = []
    dropping = False
    string_state = None
    for line in text.splitlines(keepends=True):
        header = _header_path(line) if string_state is None else None
        if header is not None:
            dropping = len(header) >= 2 and header[0] == 'mcp_servers' and header[1] in TARGETS
        if not dropping:
            result.append(line)
        string_state = _string_state(line, string_state)
    out = ''.join(result)
    if out and not out.endswith('\n'):
        out += '\n'
    out += '\n[mcp_servers.chrome-bg]\ncommand = ' + json.dumps(wrapper, ensure_ascii=False) + '\nargs = []\n'
    try:
        matches = tomllib.loads(out) == desired
    except tomllib.TOMLDecodeError:
        matches = False
    if not matches:
        raise MigrationError('Unsupported TOML layout; targeted servers must use separate table blocks')
    return out


def _read_regular(path):
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
        with os.fdopen(fd, 'rb') as stream:
            metadata = os.fstat(stream.fileno())
            if not stat.S_ISREG(metadata.st_mode):
                raise MigrationError('Configuration is not a regular file')
            return stream.read(), metadata
    except OSError:
        raise MigrationError('Cannot read regular configuration file: ' + str(path)) from None


def _transform(path, raw, wrapper):
    try:
        text = raw.decode('utf-8')
    except UnicodeError:
        raise MigrationError('Configuration is not UTF-8: ' + str(path)) from None
    if path.suffix == '.toml':
        return transform_toml(text, wrapper).encode('utf-8')
    if path.suffix == '.json':
        return transform_json(text, wrapper).encode('utf-8')
    raise MigrationError('Unsupported configuration extension: ' + str(path))


def build_plan(paths, wrapper=DEFAULT_WRAPPER):
    if not Path(wrapper).is_absolute() or '\n' in wrapper or '\r' in wrapper:
        raise MigrationError('Wrapper must be an absolute single-line path')
    entries = []
    for path in sorted({Path(os.path.abspath(p)) for p in paths}):
        raw, metadata = _read_regular(path)
        changed = _transform(path, raw, wrapper)
        entries.append({'path': str(path), 'before_sha256': digest(raw),
                        'after_sha256': digest(changed), 'changed': changed != raw,
                        'device': metadata.st_dev, 'inode': metadata.st_ino})
    return {'version': 1, 'wrapper': wrapper, 'files': entries}


def _private_write(path, data, mode=0o600):
    fd, temporary = tempfile.mkstemp(prefix='.' + path.name + '.', dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as stream:
            os.fchmod(stream.fileno(), mode)
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def _verify_entry(entry):
    path = Path(entry['path'])
    raw, metadata = _read_regular(path)
    if digest(raw) != entry['before_sha256'] or (metadata.st_dev, metadata.st_ino) != (entry['device'], entry['inode']):
        raise MigrationError('Configuration changed since review: ' + str(path))
    return raw, metadata


def apply_plan(plan, backup_dir):
    if plan.get('version') != 1 or not isinstance(plan.get('files'), list):
        raise MigrationError('Unsupported migration plan')
    staged = []
    seen = set()
    for entry in plan['files']:
        path = Path(entry['path'])
        if not path.is_absolute() or path in seen:
            raise MigrationError('Plan contains relative or duplicate paths')
        seen.add(path)
        raw, metadata = _verify_entry(entry)
        changed = _transform(path, raw, plan['wrapper'])
        if digest(changed) != entry['after_sha256'] or bool(entry['changed']) != (raw != changed):
            raise MigrationError('Plan transformation no longer matches review: ' + str(path))
        if raw != changed:
            staged.append((entry, raw, changed, stat.S_IMODE(metadata.st_mode)))
    if not staged:
        return []
    backup_dir = Path(backup_dir)
    if backup_dir.is_symlink():
        raise MigrationError('Backup directory must not be a symlink')
    backup_dir.mkdir(parents=True, mode=0o700, exist_ok=True)
    backup_dir.chmod(0o700)
    run = backup_dir / uuid.uuid4().hex
    run.mkdir(mode=0o700)
    backups = []
    for number, (entry, raw, changed, mode) in enumerate(staged):
        backup = run / (str(number) + '-' + Path(entry['path']).name)
        _private_write(backup, raw)
        backups.append(str(backup))
    manifest = {'files': [{'path': item[0]['path'], 'backup': backup, 'before_sha256': item[0]['before_sha256']} for item, backup in zip(staged, backups)]}
    _private_write(run / 'manifest.json', (json.dumps(manifest, indent=2) + '\n').encode())
    # Recheck all inputs after backup creation, then again immediately before each replace.
    for entry in plan['files']:
        _verify_entry(entry)
    for entry, raw, changed, mode in staged:
        _verify_entry(entry)
        _private_write(Path(entry['path']), changed, mode)
    return backups


def _process_roots(proc_root):
    if proc_root is None:
        return []
    result = []
    root = Path(proc_root)
    try:
        entries = root.iterdir()
        for process in entries:
            if not process.name.isdigit():
                continue
            try:
                if process.stat().st_uid != os.getuid():
                    continue
                with (process / 'environ').open('rb') as stream:
                    # Only allowlisted path variables leave this function.
                    for item in stream.read(1024 * 1024).split(b'\0'):
                        name, separator, value = item.partition(b'=')
                        name = name.decode('ascii', errors='ignore')
                        if separator and name in ROOT_VARIABLES:
                            result.append((ROOT_VARIABLES[name], os.fsdecode(value)))
            except (OSError, ValueError):
                continue
    except OSError:
        pass
    return result


def discover_configs(home=None, environ=None, proc_root='/proc'):
    home = Path(home or Path.home())
    environ = os.environ if environ is None else environ
    roots = {(kind, str(home / ('.' + kind))) for kind in ROOT_VARIABLES.values()}
    roots.update((ROOT_VARIABLES[key], value) for key, value in environ.items() if key in ROOT_VARIABLES)
    roots.update(_process_roots(proc_root))
    # Account discovery stays within named provider roots, never a whole-home recursive scan.
    for kind in ROOT_VARIABLES.values():
        for pattern in ('.' + kind + '-*', '.' + kind + '_*'):
            roots.update((kind, str(path)) for path in home.glob(pattern) if path.is_dir())
    paths = {home / '.claude.json'}
    for kind, value in roots:
        root = Path(value)
        if not root.is_absolute():
            continue
        candidates = [root]
        for branch in ('accounts', 'profiles'):
            directory = root / branch
            if directory.is_dir():
                candidates.extend(path for path in directory.iterdir() if path.is_dir())
        for candidate in candidates:
            if kind == 'claude':
                paths.update((candidate / '.claude.json', candidate / 'claude.json', candidate / 'settings.json'))
            else:
                paths.add(candidate / 'config.toml')
    return sorted(path for path in paths if path.is_file() and not path.is_symlink())


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='action', required=True)
    discovery = commands.add_parser('discover', help='List scoped candidate paths only')
    discovery.add_argument('--home', type=Path)
    discovery.add_argument('--no-process-env', action='store_true')
    dry = commands.add_parser('dry-run', help='Save a value-free plan with exact content hashes')
    dry.add_argument('--config', type=Path, action='append', required=True)
    dry.add_argument('--wrapper', default=DEFAULT_WRAPPER)
    dry.add_argument('--plan', type=Path, required=True)
    apply = commands.add_parser('apply', help='Apply a reviewed plan after verifying every hash')
    apply.add_argument('--plan', type=Path, required=True)
    apply.add_argument('--backup-dir', type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        if args.action == 'discover':
            print(json.dumps({'configs': [str(path) for path in discover_configs(args.home, proc_root=None if args.no_process_env else '/proc')]}, indent=2))
        elif args.action == 'dry-run':
            plan = build_plan(args.config, args.wrapper)
            if args.plan.is_symlink() or args.plan.absolute() in {path.absolute() for path in args.config}:
                raise MigrationError('Plan path must be separate from configurations and not a symlink')
            _private_write(args.plan, (json.dumps(plan, indent=2) + '\n').encode())
            print(json.dumps({'plan': str(args.plan), 'files': [{'path': item['path'], 'changed': item['changed']} for item in plan['files']]}, indent=2))
        else:
            raw, _ = _read_regular(args.plan)
            try:
                plan = json.loads(raw)
            except ValueError:
                raise MigrationError('Invalid migration plan') from None
            backups = apply_plan(plan, args.backup_dir)
            print(json.dumps({'changed_files': len(backups), 'backups': backups}, indent=2))
    except MigrationError as error:
        print(str(error), file=sys.stderr)
        return 1
    except (OSError, KeyError, TypeError, ValueError):
        # Parser and OS exception strings can contain configuration values.
        print('Migration failed; inspect file permissions and plan structure. Existing backups are retained.', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
