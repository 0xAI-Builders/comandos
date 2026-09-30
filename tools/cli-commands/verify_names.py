"""Teclea `/<nombre>` en el CLI (tmux privado) y lee la descripción exacta que
muestra su menú. Uso: verify_names.py SOCKET SESION nombres.json > out.json
(nombres.json = lista de nombres sin barra). Nunca envía Enter."""
import json, re, subprocess, sys, time
S, T, names = sys.argv[1], sys.argv[2], json.load(open(sys.argv[3]))
rx = re.compile(r'^\s*[❯›]?\s*/([a-z][\w:.-]*)\s{2,}(\S.*?)\s*$')
def keys(*a): subprocess.run(['tmux', '-L', S, 'send-keys', '-t', T, *a])
def cap(): return subprocess.run(['tmux', '-L', S, 'capture-pane', '-p', '-J', '-t', T], capture_output=True, text=True).stdout
def clear(): keys(*['BSpace'] * 40); time.sleep(0.2)
out = {}
for n in sorted(set(names)):
    clear(); keys('-l', '/' + n); got = None
    for _ in range(12):
        time.sleep(0.25); c = cap()
        if not re.search(r'[❯›]\s/' + re.escape(n) + r'\s*$', c, re.M):   # el prompt usa espacio duro
            continue
        got = next((m.group(2) for m in map(rx.match, c.splitlines()) if m and m.group(1) == n), None)
        if got: break
    out[n] = got
clear()
print(json.dumps(out, ensure_ascii=False, indent=0))
