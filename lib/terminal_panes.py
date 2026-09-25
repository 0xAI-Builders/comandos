"""On-demand pane controls, with exact identities and last-pane protection."""
import hashlib
import json
import re
import threading

from terminal_history import friendly_path

_LOCK = threading.RLock()


def _version(identity):
    # Exclude changing command/cwd metadata; a new pane process invalidates it.
    fields = ('socket_path', 'pid', 'server_start', 'session_id', 'pane_id', 'pane_pid')
    stable = {key: identity.get(key, '') for key in fields}
    return hashlib.sha256(json.dumps(stable, sort_keys=True).encode()).hexdigest()


def _inventory(tmux, identify, session):
    result = tmux('list-panes', '-t', '=' + session + ':', '-F',
                  '#{pane_id}\t#{pane_active}\t#{pane_current_command}\t#{pane_index}\t#{pane_current_path}')
    if result.returncode:
        raise ValueError('No se encuentra la sesión')
    panes = []
    for line in result.stdout.splitlines():
        fields = line.split('\t', 4)
        if len(fields) != 5 or not re.fullmatch(r'%\d+', fields[0]):
            raise ValueError('No se pudo leer la lista de paneles')
        pane, active, command, index, path = fields
        identity = identify(session, pane)
        panes.append({'id': pane, 'active': active == '1', 'title': command[:100],
                      'path': friendly_path(path)[:300],
                      'index': int(index), 'identity': _version(identity), '_identity': identity})
    if not panes:
        raise ValueError('No hay paneles disponibles')
    return panes


def _public(panes):
    return [{key: value for key, value in pane.items() if not key.startswith('_')}
            for pane in panes]


def execute(tmux, identify, save_snapshot, data):
    if not isinstance(data, dict):
        raise ValueError('Solicitud inválida')
    session = data.get('session')
    if not isinstance(session, str) or not re.fullmatch(r'[A-Za-z0-9_-]{1,120}', session):
        raise ValueError('Sesión inválida')
    action = data.get('action', 'list')
    if action not in ('list', 'select', 'close', 'split'):
        raise ValueError('Acción de panel inválida')
    split_flag = None
    if action == 'split':
        split_flag = {'right': '-h', 'down': '-v'}.get(data.get('direction'))
        if not split_flag:
            raise ValueError('Dirección de split inválida')
    with _LOCK:
        panes = _inventory(tmux, identify, session)
        if action == 'list':
            return {'ok': True, 'panes': _public(panes)}
        pane = next((p for p in panes if p['id'] == data.get('pane')), None)
        if not pane or pane['identity'] != data.get('identity'):
            raise ValueError('El panel cambió. Abre Paneles y vuelve a elegirlo')
        if action == 'close' and len(panes) <= 1:
            raise ValueError('El último panel permanece abierto')
        snapshot = None
        if action == 'close':
            snapshot = save_snapshot(session, pane['id'])
            fresh = _inventory(tmux, identify, session)
            if len(fresh) <= 1 or not any(p['id'] == pane['id'] and p['identity'] == pane['identity'] for p in fresh):
                raise ValueError('El panel cambió mientras se guardaba. No se ha cerrado')
        identity = pane['_identity']
        checks = []
        for key in ('pid', 'session_id', 'pane_id', 'pane_pid'):
            value = str(identity.get(key, ''))
            if not re.fullmatch(r'[$%]?\d+', value):
                raise ValueError('No se pudo verificar la identidad del panel')
            checks.append('#{==:#{' + key + '},' + value + '}')
        if action == 'close':
            checks.append('#{>:#{window_panes},1}')
        condition = checks.pop()
        for check in checks:
            condition = '#{&&:' + check + ',' + condition + '}'
        # tmux evaluates the identity and final-pane guard in its command queue.
        # Pane IDs are validated digits, never shell input or a current-focus alias.
        if action == 'split':
            # El panel nuevo hereda la carpeta del panel partido y queda activo.
            command = 'split-window ' + split_flag + ' -t ' + pane['id'] + ' -c "#{pane_current_path}"'
        else:
            command = ('kill-pane' if action == 'close' else 'select-pane') + ' -t ' + pane['id']
        result = tmux('if-shell', '-F', '-t', pane['id'], condition,
                      command, 'display-message -p COMANDOS_PANE_CHANGED')
        if result.returncode or 'COMANDOS_PANE_CHANGED' in result.stdout:
            raise ValueError('El panel cambió. La acción no se ha aplicado')
        after = _inventory(tmux, identify, session)
        response = {'ok': True, 'panes': _public(after)}
        if action == 'split':
            known = {p['id'] for p in panes}
            response['opened'] = next((p['id'] for p in after if p['id'] not in known), None)
        if action == 'close':
            response.update(closed=pane['id'], snapshot=snapshot)
        return response
