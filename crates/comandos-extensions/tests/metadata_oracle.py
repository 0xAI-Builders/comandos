"""Explicit fixture writer, run only through the isolated Python oracle runner."""
import argparse, json, hashlib, shutil, pathlib, sys
sys.path.insert(0,'/work/lib')
import extension_metadata as m
p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');p.add_argument('--prepare-cache',action='store_true');args=p.parse_args()
texts=['','hello world',"<|endoftext|><|fim_prefix|><|fim_middle|><|fim_suffix|><|endofprompt|>","I'm WE'LL hadn't wouldn’t 1234567890",' \t\n\r\n   \n', 'áéí 中文 日本語 العربية हिन्दी 한글', '😀👨‍👩‍👧‍👦 e\u0301 \x7f \u2028 \u200b', ''.join(chr(i) for i in range(1,256)), 'a'*12000, '😀'*500, '1 1.0 -0.0 1e-07 1e+20']
texts.append('A\U00011f02! 123\U0001e4d0x.\U000105c0 987é\U00011f02\U0001e4d0\U000105c0\n')
textcases=[{'text':t,'tokens':n} for t,n in zip(texts,m._offline_counts(texts))]
tools=[{'name':'é😀','description':'secret schema 文\x7f','inputSchema':{'α':0.0,'负':-0.0,'small':1e-7,'tiny':1e-6,'big':1e20,'integer':2**100,'special':'<|endoftext|>'},'ignored':'not included'},{'name':'a','outputSchema':{'default':1e16}},{'name':'a','description':'duplicate stable'}]
spec={'url':'https://example.test/é','auth':{'sensitive':'never persist'},'timeout':1.0}
defs=sorted(({k:t[k] for k in ('name','description','inputSchema','outputSchema') if k in t} for t in tools),key=lambda t:t['name'])
text=json.dumps(defs,sort_keys=True,separators=(',',':'),ensure_ascii=False)
fixture={'texts':textcases,'tools':tools,'spec':spec,'name':'é😀','definitions_text':text,'content':m._digest(defs),'configuration':m._digest(spec),'name_digest':m._digest('é😀'),'default_len':len(json.dumps(tools)),'tokens':m._offline_counts([text])[0]}
if args.write:
 pathlib.Path('/work/crates/comandos-extensions/tests/fixtures/metadata.json').write_text(json.dumps(fixture,ensure_ascii=True,indent=2)+'\n')
 print('Wrote immutable metadata/count fixture.')
else:
 actual=json.loads(pathlib.Path('/work/crates/comandos-extensions/tests/fixtures/metadata.json').read_text());assert actual==fixture;print('Python metadata oracle matches immutable fixture.')

if args.prepare_cache or args.write:
 cache=pathlib.Path('/work/.migration-build/public-token-cache');cache.mkdir(exist_ok=True)
 source=pathlib.Path('/tokens')/hashlib.sha1(m.ENCODING_URL.encode()).hexdigest()
 assert hashlib.sha256(source.read_bytes()).hexdigest()==m.ENCODING_HASH
 shutil.copyfile(source,cache/source.name)
 print('Copied SHA256-verified public encoding only.')
