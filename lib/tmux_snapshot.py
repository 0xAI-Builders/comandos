"""Persist tmux geometry and per-pane metadata; restore only absent sessions."""
import json
import os
from pathlib import Path
import re
import shlex
import tempfile
import time

_LEAF = re.compile(r'(\d+x\d+,\d+,\d+),(\d+)(?=[,}\]]|$)')


def _checked(tmux, *args):
    r = tmux(*[str(a) for a in args])
    if r.returncode:
        raise RuntimeError(f'tmux {args[0]}: {r.stderr.strip()}')
    return r.stdout.strip()


def remap_layout(layout, panes):
    """Replace leaf IDs, preserving the native tree, then regenerate its checksum."""
    body = layout.split(',', 1)[1]
    def leaf(match):
        old = '%' + match.group(2)
        new = panes[old].lstrip('%')
        return match.group(1) + ',' + new
    body = _LEAF.sub(leaf, body)
    checksum = 0
    for c in body:
        checksum = ((checksum >> 1) | ((checksum & 1) << 15))
        checksum = (checksum + ord(c)) & 0xffff
    return f'{checksum:04x},{body}'


def capture_session(tmux, session, describe_pane):
    """Read a complete stable session; fail instead of publishing a partial layout."""
    fmt = '\t'.join('#{' + x + '}' for x in (
        'window_id', 'window_index', 'window_name', 'window_layout',
        'window_width', 'window_height', 'window_active', 'window_zoomed_flag'))
    raw = _checked(tmux, 'list-windows', '-t', '=' + session, '-F', fmt)
    windows = []
    for line in raw.splitlines():
        wid, index, name, layout, width, height, active, zoomed = line.split('\t')
        window = dict(id=wid, index=int(index), name=name, layout=layout,
                      width=int(width), height=int(height), active=active == '1',
                      zoomed=zoomed == '1', panes=[])
        window['border_status'] = _checked(tmux, 'show-options', '-wAv', '-t', wid, 'pane-border-status') or 'off'
        window['automatic_rename'] = _checked(tmux, 'show-options', '-wAv', '-t', wid, 'automatic-rename') or 'off'
        window['window_size'] = _checked(tmux, 'show-options', '-wAv', '-t', wid, 'window-size') or 'latest'
        pfmt = '\t'.join('#{' + x + '}' for x in (
            'pane_id', 'pane_index', 'pane_current_path', 'pane_pid', 'pane_current_command', 'pane_active'))
        for row in _checked(tmux, 'list-panes', '-t', wid, '-F', pfmt).splitlines():
            pid, idx, cwd, process, command, selected = row.split('\t')
            pane = dict(id=pid, index=int(idx), cwd=cwd, pid=int(process),
                        command=command, active=selected == '1')
            pane.update(describe_pane(pane))
            window['panes'].append(pane)
        leaf_ids = {'%' + m.group(2) for m in _LEAF.finditer(layout)}
        if leaf_ids != {p['id'] for p in window['panes']}:
            raise RuntimeError('Pane layout changed during capture')
        # A concurrent resize/swap must not pair new pane state with an old tree.
        current = _checked(tmux, 'display-message', '-p', '-t', wid, '#{window_layout}')
        if current != layout:
            raise RuntimeError('Window resized during capture')
        windows.append(window)
    if not windows:
        raise RuntimeError('No windows to capture')
    return {'windows': windows, 'captured_at': int(time.time())}


_AGENTS = {'claude', 'codex', 'grok'}
_PANE_PLACE = {'id', 'index', 'cwd', 'pid', 'command', 'active'}


def carry_resume_ids(captured, previous):
    """An agent without an id yet inherits the saved one only for the same pane process."""
    old = {p.get('id'): p for w in (previous or {}).get('windows', []) for p in w.get('panes', [])}
    for window in captured['windows']:
        for pane in window['panes']:
            if pane.get('agent') not in _AGENTS or pane.get('resume_id'):
                continue
            prev = old.get(pane.get('id'))
            if not prev or not prev.get('resume_id'):
                continue
            if all(prev.get(k) == pane.get(k) for k in ('pid', 'command', 'agent')):
                for key, value in prev.items():
                    if key not in _PANE_PLACE:
                        pane.setdefault(key, value)
    return captured


def _cwd(pane):
    path = pane.get('cwd') or str(Path.home())
    return path if os.path.isdir(path) else str(Path.home())


def restore_session(tmux, session, snapshot, resume_command):
    """Restore geometry before starting agents; never alter a surviving session."""
    if tmux('has-session', '-t', '=' + session).returncode == 0:
        return {}
    windows = snapshot.get('windows', [])
    if not windows or any(not w.get('panes') for w in windows):
        raise ValueError('Snapshot has no complete windows')
    # Validate leaf mappings before creating anything.
    for w in windows:
        remap_layout(w['layout'], {p['id']: p['id'] for p in w['panes']})
    mapping = {}
    starts = []
    active_window = None
    created = False
    started = 0
    try:
        for i, w in enumerate(windows):
            panes = w['panes']
            # Temporary space allows split creation even for a tall/narrow layout.
            width = max(w['width'], 4 * len(panes))
            if i == 0:
                first = _checked(tmux, 'new-session', '-d', '-P', '-F', '#{pane_id}',
                                 '-s', session, '-n', w['name'], '-x', width, '-y', w['height'],
                                 '-c', _cwd(panes[0]), 'sleep', '2147483647')
                created = True
                wid = _checked(tmux, 'display-message', '-p', '-t', first, '#{window_id}')
                index = _checked(tmux, 'display-message', '-p', '-t', first, '#{window_index}')
                if int(index) != w['index']:
                    _checked(tmux, 'move-window', '-s', wid, '-t', '=' + session + ':' + str(w['index']))
            else:
                first = _checked(tmux, 'new-window', '-d', '-P', '-F', '#{pane_id}',
                                 '-t', '=' + session + ':' + str(w['index']), '-n', w['name'],
                                 '-c', _cwd(panes[0]), 'sleep', '2147483647')
                wid = _checked(tmux, 'display-message', '-p', '-t', first, '#{window_id}')
            _checked(tmux, 'set-option', '-w', '-t', wid, 'pane-border-status', w.get('border_status', 'off'))
            _checked(tmux, 'resize-window', '-t', wid, '-x', width, '-y', w['height'])
            mapping[panes[0]['id']] = first
            starts.append((first, panes[0]))
            last = first
            for p in panes[1:]:
                new = _checked(tmux, 'split-window', '-d', '-h', '-l', '1', '-P', '-F', '#{pane_id}',
                               '-t', last, '-c', _cwd(p), 'sleep', '2147483647')
                mapping[p['id']] = new
                starts.append((new, p))
                last = new
                _checked(tmux, 'select-layout', '-t', wid, 'even-horizontal')
            _checked(tmux, 'select-layout', '-t', wid, remap_layout(w['layout'], mapping))
            _checked(tmux, 'resize-window', '-t', wid, '-x', w['width'], '-y', w['height'])
            # resize-window makes window-size manual; allow adaptation on attach.
            _checked(tmux, 'set-option', '-w', '-t', wid, 'window-size', w.get('window_size', 'latest'))
            _checked(tmux, 'set-option', '-w', '-t', wid, 'automatic-rename', w.get('automatic_rename', 'off'))
            selected = next((p for p in panes if p.get('active')), panes[0])
            _checked(tmux, 'select-pane', '-t', mapping[selected['id']])
            if w.get('zoomed'):
                _checked(tmux, 'resize-pane', '-Z', '-t', mapping[selected['id']])
            if w.get('active'):
                active_window = wid
        if active_window:
            _checked(tmux, 'select-window', '-t', active_window)
        # Spawn directly: terminal device replies cannot corrupt a typed command.
        shell = os.environ.get('SHELL') or '/bin/sh'
        for pane_id, saved in starts:
            cmd = resume_command(saved)
            # Recovery can be invoked by an automation process with NO_COLOR=1.
            # That setting must not disable highlighting in the user's terminal.
            argv = ['env', '-u', 'NO_COLOR', shell, '-il']
            if cmd:
                argv = ['env', '-u', 'NO_COLOR', shell, '-ilc', cmd + '; exec ' + shlex.quote(shell) + ' -il']
            _checked(tmux, 'respawn-pane', '-k', '-t', pane_id, '-c', _cwd(saved), *argv)
            started += 1
        return mapping
    except Exception:
        # Only our newly-created placeholder session may be removed on failure.
        # Once agents start, preserve them and let the caller report the error.
        if created and started == 0:
            tmux('kill-session', '-t', '=' + session)
        raise


def valid_snapshot(data):
    if not isinstance(data, dict) or data.get('version') != 2 or not isinstance(data.get('sessions'), dict):
        return False
    try:
        for session in data['sessions'].values():
            windows = session['windows']
            if not windows or sum(bool(w['active']) for w in windows) != 1:
                return False
            if len({w['index'] for w in windows}) != len(windows):
                return False
            for window in windows:
                panes = window['panes']
                if not panes or sum(bool(p['active']) for p in panes) != 1:
                    return False
                if any(not isinstance(window[k], int) or window[k] < 1 for k in ('width', 'height')):
                    return False
                if not isinstance(window['index'], int) or not isinstance(window['name'], str):
                    return False
                ids = {p['id'] for p in panes}
                leaves = ['%' + m.group(2) for m in _LEAF.finditer(window['layout'])]
                if len(ids) != len(panes) or len(leaves) != len(panes) or set(leaves) != ids:
                    return False
                if remap_layout(window['layout'], {p: p for p in ids}) != window['layout']:
                    return False
        return True
    except (KeyError, TypeError, ValueError, AttributeError, IndexError):
        return False


def read_snapshot(path):
    """Read the current snapshot, or its previous valid generation."""
    path = Path(path)
    for p in (path, path.with_name(path.name + '.bak')):
        try:
            d = json.loads(p.read_text())
            if valid_snapshot(d):
                return d
        except (OSError, ValueError, AttributeError):
            pass
    return {'version': 2, 'sessions': {}}


def write_snapshot(path, sessions):
    """Keep a previous generation plus seven days of independent minute history."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    data = {'version': 2, 'saved_at': int(time.time()), 'sessions': sessions}
    encoded = json.dumps(data)
    if not valid_snapshot(data):
        raise ValueError('Refusing incomplete session layout snapshot')
    fd, tmp = tempfile.mkstemp(prefix=path.name + '.', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as f:
            f.write(encoded)
            f.flush()
            os.fsync(f.fileno())
        if path.exists():
            try:
                old = json.loads(path.read_text())
                valid = valid_snapshot(old)
            except (ValueError, AttributeError):
                valid = False
            if valid:
                history = path.with_name(path.name + '.history')
                history.mkdir(mode=0o700, exist_ok=True)
                stamp = int(old.get('saved_at', time.time())) // 60 * 60
                archived = history / f'{stamp:012d}.json'
                try:
                    # Exclusive creation: later captures cannot overwrite this minute.
                    os.link(path, archived)
                except FileExistsError:
                    pass
                else:
                    cutoff = int(time.time()) - 7 * 86400
                    for entry in history.glob('*.json'):
                        if entry.stem.isdigit() and int(entry.stem) < cutoff:
                            entry.unlink(missing_ok=True)
                # Link the old inode; the main path remains present until replace.
                bak = path.with_name(path.name + '.bak')
                stage = Path(tmp + '.bak')
                os.link(path, stage)
                os.replace(stage, bak)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)
