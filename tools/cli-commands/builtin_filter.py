import json, re, sys
cmds = json.load(open(sys.argv[1])); data = open(sys.argv[2], 'rb').read()
out = []
for n, d in cmds:
    d = re.sub(r'\s{2,}built-in\s*$', '', d).strip()
    probe = d[:40].encode()
    if probe and probe in data:
        out.append([n, d])
print(json.dumps(out, ensure_ascii=False))
