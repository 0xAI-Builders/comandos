"""Install the explicit-YOLO launcher when the user runs the batch command."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

MARKER = '# COMANDOS_CODEX_YOLO_LAUNCHER'
START = '# BEGIN COMANDOS CODEX YOLO'
END = '# END COMANDOS CODEX YOLO'


def _write(path, content, mode):
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix='.' + path.name + '.', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as f:
            f.write(content); f.flush(); os.fsync(f.fileno())
        os.chmod(temporary, mode)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def install(home=None, executable=None):
    home = Path(home or Path.home()).resolve()
    directory = home / '.local/share/comandos/codex-yolo'
    wrapper = home / '.local/bin/codex'
    manifest = directory / 'launcher.json'
    try:
        previous = json.loads(manifest.read_text())
    except FileNotFoundError:
        previous = {}
    if wrapper.exists() or wrapper.is_symlink():
        if MARKER not in wrapper.read_text()[:512]:
            raise ValueError(f'Ya existe un lanzador ajeno en {wrapper}; se conserva sin sobrescribir')
    entry = Path(executable or previous.get('launcher') or shutil.which('codex') or '')
    if not str(entry) or str(entry) == '.':
        raise ValueError('No se encontró el ejecutable de Codex')
    entry = entry.absolute()
    original = entry.resolve()
    if original == wrapper.resolve():
        original = Path(previous.get('original') or '')
    if not original.is_file() or original == wrapper.resolve() or not os.access(original, os.X_OK):
        raise ValueError('No se pudo identificar el lanzador original; se evita un bucle de ejecución')
    help_text = subprocess.run([str(original), '--help'], capture_output=True, text=True, timeout=15, check=True).stdout
    if '--no-daemon' not in help_text or '--dangerously-bypass-approvals-and-sandbox' not in help_text:
        raise ValueError('Esta versión no admite las opciones necesarias; no se instaló la regla')
    # Validate shell blocks before installing anything.
    shells = []
    for name in ('.zshrc', '.bashrc'):
        path = home / name
        target = path.resolve() if path.is_symlink() else path
        text = target.read_text() if target.exists() else ''
        if text.count(START) != text.count(END) or text.count(START) > 1:
            raise ValueError(f'Bloque de PATH inválido en {path}; se conserva sin cambiar')
        block = START + '\nexport PATH=' + json.dumps(str(wrapper.parent)) + ':"$PATH"\n' + END
        if START in text:
            a = text.index(START); b = text.index(END, a) + len(END)
            updated = text[:a] + block + text[b:]
        else:
            updated = text + ('' if not text or text.endswith('\n') else '\n') + '\n' + block + '\n'
        shells.append((path, target, text, updated))
    directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    # Copy the policy so moving the repo does not break future Codex launches.
    policy = directory / 'policy.py'
    _write(policy, Path(__file__).with_name('codex_yolo_policy.py').read_text(), 0o600)
    script = ('#!/usr/bin/env python3\n' + MARKER + '\n'
              '# COMANDOS_CODEX_ORIGINAL=' + json.dumps(str(original)) + '\n'
              'import importlib.util,os,sys\n'
              'spec=importlib.util.spec_from_file_location("comandos_yolo",' + repr(str(policy)) + ')\n'
              'policy=importlib.util.module_from_spec(spec);spec.loader.exec_module(policy)\n'
              'try: args=policy.normalize_args(sys.argv[1:])\n'
              'except ValueError as e: print(str(e),file=sys.stderr);raise SystemExit(2)\n'
              'original=' + repr(str(original)) + '\n'
              'os.execv(original,[original,*args])\n')
    metadata = {'version': 1, 'wrapper': str(wrapper), 'original': str(original),
                'launcher': previous.get('launcher') or str(entry),
                'originalLink': previous.get('originalLink') or (os.readlink(entry) if entry.is_symlink() and entry.resolve() != wrapper.resolve() else None)}
    # Store recovery information before replacing either entry point.
    _write(manifest, json.dumps(metadata, indent=2) + '\n', 0o600)
    _write(wrapper, script, 0o755)
    if entry != wrapper and entry.is_symlink():
        temporary = entry.with_name('.' + entry.name + '.comandos-' + str(os.getpid()))
        try:
            temporary.symlink_to(wrapper)
            os.replace(temporary, entry)
        finally:
            if temporary.is_symlink(): temporary.unlink()
    backup = directory / 'zshrc.before'
    for path, target, text, updated in shells:
        saved = directory / (path.name.lstrip('.') + '.before')
        if not saved.exists(): _write(saved, text, 0o600)
        if updated != text:
            mode = target.stat().st_mode & 0o777 if target.exists() else 0o644
            _write(target, updated, mode)
    return {**metadata, 'backup': str(backup)}
