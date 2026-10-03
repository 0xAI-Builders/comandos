import json,pathlib,subprocess
root=pathlib.Path('/work')
artifacts=[json.loads(line) for line in (root/'.superpowers/sdd/schema-traversal-build-artifacts.jsonl').read_text().splitlines()]
binaries=[row['executable'] for row in artifacts if row.get('reason')=='compiler-artifact' and row['target']['kind']==['lib'] and row.get('profile',{}).get('test') and row.get('executable')]
assert len(binaries)==1,binaries
binary=binaries[0]; (root/'.superpowers/sdd/schema-traversal-library-binary.txt').write_text(binary+'\n')
base=['/venv/bin/python','/work/crates/comandos-extensions/tests/output_schema_traversal_audit.py','--stage-binary',binary,'--binary','/work/.migration-build/target/release/comandos-extensions','--cases','/work/crates/comandos-extensions/tests/output_schema_traversal_cases.json','--full-cases','/work/crates/comandos-extensions/tests/output_schema_preflight_full_cases.json']
for name,extra in [('directed',[]),('full',['--full'])]:
    command=base+extra+['--out',f'/work/.superpowers/sdd/schema-traversal-{name}.json'];print(command,flush=True)
    with (root/f'.superpowers/sdd/schema-traversal-{name}.log').open('w') as stream:
        result=subprocess.run(command,stdout=stream,stderr=subprocess.STDOUT)
    print(name,result.returncode,flush=True)
    if result.returncode not in (0,1):raise SystemExit(result.returncode)
    artifact=json.loads((root/f'.superpowers/sdd/schema-traversal-{name}.json').read_text());print(artifact['summary'],flush=True)
