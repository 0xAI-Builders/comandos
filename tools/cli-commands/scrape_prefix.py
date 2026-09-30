import re, subprocess, sys, time, json, string
sock, sess, rx, lead = sys.argv[1], sys.argv[2], re.compile(sys.argv[3]), sys.argv[4]
MAXV = int(sys.argv[5]) if len(sys.argv) > 5 else 8
def keys(*a): subprocess.run(['tmux','-L',sock,'send-keys','-t',sess,*a])
def cap(): return subprocess.run(['tmux','-L',sock,'capture-pane','-p','-J','-t',sess],capture_output=True,text=True).stdout
def clear():
    keys('C-u'); time.sleep(0.15)
def items(prefix):
    clear(); keys('-l', lead + prefix); time.sleep(0.6)
    out = {}
    for line in cap().splitlines():
        m = rx.match(line)
        if m and m.group(1).startswith(prefix): out[m.group(1)] = m.group(2).strip()
    return out
seen = {}
def walk(prefix, depth):
    got = items(prefix)
    seen.update(got)
    if len(got) >= MAXV and depth < 4:
        for ch in string.ascii_lowercase + '-':
            walk(prefix + ch, depth + 1)
for ch in string.ascii_lowercase:
    walk(ch, 1)
clear()
print(json.dumps(sorted(seen.items()), ensure_ascii=False))
