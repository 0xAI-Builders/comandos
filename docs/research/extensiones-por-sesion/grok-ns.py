#!/usr/bin/env python3
# usage: grok-ns.py <session-config.toml> <cmd...>  -- bind <session-config> over $GROK_HOME/config.toml in a private userns+mountns, then exec cmd without caps
import ctypes, os, sys
uid, gid = os.getuid(), os.getgid()
libc0 = ctypes.CDLL(None, use_errno=True)
if libc0.unshare(0x10000000 | 0x00020000) != 0: sys.exit("unshare failed: " + os.strerror(ctypes.get_errno()))
open('/proc/self/setgroups','w').write('deny')
open('/proc/self/uid_map','w').write(f'{uid} {uid} 1')
open('/proc/self/gid_map','w').write(f'{gid} {gid} 1')
libc = ctypes.CDLL(None, use_errno=True)
if libc.mount(b"none", b"/", None, (1<<14)|(1<<18), None) != 0: sys.exit("make-private failed")
home = os.environ.get('GROK_HOME', os.path.expanduser('~/.grok'))
if libc.mount(sys.argv[1].encode(), f"{home}/config.toml".encode(), None, 4096, None) != 0:
    sys.exit("bind failed: " + os.strerror(ctypes.get_errno()))
os.execvp(sys.argv[2], sys.argv[2:])   # execve as non-root uid drops all caps
