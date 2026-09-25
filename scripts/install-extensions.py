#!/usr/bin/env python3.11
"""Install shared extensions for the current Unix user."""
import os
from pathlib import Path
import shlex
import subprocess
import tempfile


def write(path,data,mode=0o600):
    path.parent.mkdir(parents=True,exist_ok=True,mode=0o700)
    fd,tmp=tempfile.mkstemp(dir=path.parent,prefix='.'+path.name)
    try:
        with os.fdopen(fd,'wb') as f:
            os.fchmod(f.fileno(),mode);f.write(data);f.flush();os.fsync(f.fileno())
        os.replace(tmp,path)
    finally:
        if os.path.exists(tmp):os.unlink(tmp)


def main():
    home=Path.home();source=Path(__file__).resolve().parents[1]
    venv=home/'.local/share/comandos/extensions-venv';python=venv/'bin/python'
    if not python.exists():
        subprocess.run(['uv','venv','--python','python3.11',str(venv)],check=True)
    subprocess.run(['uv','pip','install','--python',str(python),'-r',str(source/'requirements-extensions.txt')],check=True)
    dest=home/'.local/share/comandos/extensions'
    for part,names in [('bin',['cc-extensions']),('lib',['extension_catalog.py','extension_auth.py','extension_proxy.py','extension_metadata.py'])]:
        for name in names:write(dest/part/name,(source/part/name).read_bytes())
    subprocess.run([str(python),str(dest/'lib/extension_metadata.py'),'--warm'],check=True)
    launcher=home/'.local/bin/cc-extensions'
    expected=('#!/bin/sh\nexec '+shlex.quote(str(python))+' '+shlex.quote(str(dest/'bin/cc-extensions'))+' "$@"\n').encode()
    if launcher.exists() and launcher.read_bytes()!=expected:
        raise SystemExit('Refusing to overwrite an unrelated cc-extensions launcher')
    write(launcher,expected,0o700)
    if not (home/'.config/comandos/extensions/catalog.json').exists():
        subprocess.run([str(launcher),'import'],check=True)
    subprocess.run([str(launcher),'sync'],check=True)
    units=home/'.config/systemd/user'
    service='''[Unit]
Description=Synchronize shared MCPs and portable skills

[Service]
Type=oneshot
ExecStart=%h/.local/bin/cc-extensions sync
TimeoutStartSec=90
UMask=0077
'''
    timer='''[Unit]
Description=Propagate extension changes to agent CLIs

[Timer]
OnBootSec=45s
OnUnitInactiveSec=60s
AccuracySec=5s
RandomizedDelaySec=5s
Unit=comandos-extensions-sync.service

[Install]
WantedBy=timers.target
'''
    for name,content in [('comandos-extensions-sync.service',service),('comandos-extensions-sync.timer',timer)]:
        p=units/name
        if p.exists() and p.read_text()!=content:
            raise SystemExit('Refusing to overwrite a customized extension unit: '+name)
        write(p,content.encode())
    subprocess.run(['systemctl','--user','daemon-reload'],check=True)
    subprocess.run(['systemctl','--user','enable','--now','comandos-extensions-sync.timer'],check=True)
    subprocess.run(['systemctl','--user','start','comandos-extensions-sync.service'],check=True)
    print('Shared extensions installed; synchronization timer enabled.')


if __name__=='__main__':main()
