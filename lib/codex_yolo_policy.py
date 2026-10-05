"""Explicit YOLO launch options, shared by the batch and installed launcher.

Only explicit YOLO invocations change. The caller's OS/managed permissions are
never changed, and remote servers must configure their own launch policy.
"""
YOLO = '--dangerously-bypass-approvals-and-sandbox'
VALUE_FLAGS = {'-c', '--config', '-m', '--model', '-p', '--profile', '-s', '--sandbox',
               '-a', '--ask-for-approval', '-C', '--cd', '--add-dir', '-i', '--image',
               '--remote', '--remote-auth-token-env', '--local-provider', '--enable', '--disable',
               '-o', '--output-last-message', '--output-schema'}
PERMISSION_FLAGS = {'-s', '--sandbox', '-a', '--ask-for-approval'}
PERMISSION_KEYS = {'sandbox_mode', 'approval_policy', 'approvals_reviewer', 'permissions'}


def _arguments(args):
    i = 0
    while i < len(args):
        arg = args[i]
        if arg == '--':
            yield args[i:]
            return
        key = arg.split('=', 1)[0]
        length = 2 if key in VALUE_FLAGS and '=' not in arg and i + 1 < len(args) else 1
        yield args[i:i + length]
        i += length


def normalize_args(args):
    args = list(args)
    chunks = list(_arguments(args))
    if not any(chunk[0] in ('--yolo', YOLO) for chunk in chunks):
        return args
    result = []
    for chunk in chunks:
        arg = chunk[0]
        key = arg.split('=', 1)[0]
        if arg == '--':
            result.extend(chunk)
            continue
        if key in ('--remote', '--remote-auth-token-env'):
            raise ValueError('YOLO remoto debe configurarse en el equipo del servidor; no se cambiará a local')
        if arg in ('--yolo', YOLO, '--no-daemon', '--approve-for-me', '--full-auto') or key in PERMISSION_FLAGS:
            continue
        config = chunk[1] if arg in ('-c', '--config') and len(chunk) == 2 else arg.split('=', 1)[1] if arg.startswith('--config=') else ''
        config_key = config.split('=', 1)[0].strip()
        if config_key in PERMISSION_KEYS or config_key.startswith('permissions.'):
            continue
        result.extend(chunk)
    # resume/fork parse their own interactive options. Set the policy on that
    # command so it does not depend on root-option forwarding in a CLI version.
    offset = 0
    for chunk in _arguments(result):
        if chunk[0] == '--':
            break
        if not chunk[0].startswith('-'):
            if chunk[0] in ('resume', 'fork'):
                result[offset + 1:offset + 1] = ['--no-daemon', YOLO]
                return result
            break
        offset += len(chunk)
    result[:0] = ['--no-daemon', YOLO]
    return result
