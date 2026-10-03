import json,os,pathlib,socket,subprocess
root=pathlib.Path('/work');binary=(root/'.superpowers/sdd/schema-traversal-library-binary.txt').read_text().strip()
marker=pathlib.Path('/tmp/schema-traversal-private-marker.json');marker.write_text('{"type":123}')
os.utime(marker,ns=(0,0))
listener=socket.socket();listener.bind(('127.0.0.1',0));listener.listen();listener.settimeout(0.1)
rows=[]
for name,ref in [('file-marker','file://'+str(marker)),('network-marker',f'http://127.0.0.1:{listener.getsockname()[1]}/schema')]:
    rows.append({'name':name,'schema_json':json.dumps({'$ref':ref}),'content_json':'{}','mode':'ExhaustErrors'})
cases=root/'.superpowers/sdd/schema-traversal-marker-cases.json';cases.write_text(json.dumps(rows,indent=2)+'\n')
command=['/venv/bin/python','/work/crates/comandos-extensions/tests/output_schema_traversal_audit.py','--stage-binary',binary,'--binary','/work/.migration-build/target/release/comandos-extensions','--cases',str(cases),'--full-cases','/work/crates/comandos-extensions/tests/output_schema_preflight_full_cases.json','--out','/work/.superpowers/sdd/schema-traversal-marker-audit.json']
result=subprocess.run(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE);assert result.returncode==0,(result.returncode,result.stderr,result.stdout)
assert marker.stat().st_atime_ns==0,marker.stat()
try:listener.accept()
except TimeoutError:pass
else:raise AssertionError('audit attempted network contact')
artifact=json.loads((root/'.superpowers/sdd/schema-traversal-marker-audit.json').read_text());assert all(row['native_traversal']=='Abort:Unresolvable' for row in artifact['results'])
print('PASS: exact Python oracle, native preflight, traversal probe and stock worker left file/network markers untouched')
listener.close();marker.unlink()
