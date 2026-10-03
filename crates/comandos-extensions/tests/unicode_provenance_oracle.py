"""Generate/verify Python 3.11 Unicode-14 word ranges for test-only parity."""
import argparse,json,pathlib,re,unicodedata
p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args()
assert unicodedata.unidata_version=='14.0.0',unicodedata.unidata_version
word=re.compile(r'\w');ranges=[];start=None;last=None
for scalar in range(0x110000):
    matches=bool(word.fullmatch(chr(scalar)))
    if matches:
        if start is None:start=scalar
        last=scalar
    elif start is not None:
        ranges.append([start,last]);start=None
if start is not None:ranges.append([start,last])
value={'python':'3.11','unicode':unicodedata.unidata_version,'ranges':ranges}
path=pathlib.Path('/work/crates/comandos-extensions/tests/fixtures/python_word_ranges.json')
if args.write:path.write_text(json.dumps(value,separators=(',',':'))+'\n')
else:assert json.loads(path.read_text())==value
print(json.dumps({'unicode':unicodedata.unidata_version,'ranges':len(ranges),'U+0345':bool(word.fullmatch('\u0345')),'U+11F02':bool(word.fullmatch('\U00011f02')),'write':args.write}))
