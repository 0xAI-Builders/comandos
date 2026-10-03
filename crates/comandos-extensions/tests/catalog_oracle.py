"""Differential catalog sequences, synthetic homes, original read-only Python oracle."""
import copy, hashlib, json, os, pathlib, shutil, subprocess, sys
sys.path.insert(0, '/work/lib')
import extension_catalog as m
from extension_auth import import_credentials, load_credentials
ROOT=pathlib.Path('/tmp/catalog-oracle')
BIN='/work/.migration-build/target/debug/comandos-extensions'
def put(home,path,text):
 p=home/path;p.parent.mkdir(parents=True,exist_ok=True);p.write_text(text)
def python_action(home,action):
 path=m.catalog_path(home);launcher=str(home/'.local/bin/cc-extensions')
 if action=='import':
  with m.locked(home):
   if path.exists():raise m.CatalogError('Catalog already exists; refusing to overwrite it')
   c=m.import_catalog(home);imported=import_credentials(home,c);m.save_json(path,c);m.save_snapshot(home,c,launcher)
  return {'servers':len(c['servers']),'credentials_imported':imported}
 c=m.read_config(path)
 if action=='sync':
  with m.locked(home):
   original=path.read_bytes();c=m.parse_config(path,original);inputs={};revised=m.reconcile(home,c,launcher,observed_inputs=inputs)
   if revised!=c:c=revised;m.replace_config(home,path,original,(json.dumps(c,ensure_ascii=False,indent=2)+'\n').encode())
   import_credentials(home,c);observed={};changed=m.sync_configs(home,c,launcher,observed=observed,expected_inputs=inputs);skills=m.sync_skills(home);m.save_snapshot(home,c,launcher,expected_targets=observed)
  return {'configurations_changed':len(changed),'skill_links_changed':len(skills)}
 if action=='status':
  credentials=load_credentials(home)
  return {'servers':[{'name':n,'enabled':s.get('enabled',True),'credential_saved':n in credentials,'transport':'stdio' if s.get('command') else s.get('transport','http')}for n,s in sorted(c['servers'].items())], 'targets':[str(t['path'])for t in m.targets(home)]}
def action(home,kind,name):
 if kind=='python':
  try:return {'ok':python_action(home,name)}
  except Exception as e:return {'error':str(e) if isinstance(e,m.CatalogError) else type(e).__name__}
 p=subprocess.run([BIN,'--home',str(home),name],capture_output=True,text=True,check=False)
 return {'ok':json.loads(p.stdout)} if p.returncode==0 else {'error':p.stderr.strip()}
def state(home):
 # Python fingerprint observes exact logical types and bool wrappers in both renderers.
 targets={str(t['path']):{n:m.fingerprint(s)for n,s in m.native_specs(m.read_config(t['path']),t['key']).items()}for t in m.targets(home)}
 skills={p.name:m.skill_fingerprint(p)for p in (home/'.agents/skills').glob('*')if (p/'SKILL.md').is_file()}
 return {'catalog':m.read_config(m.catalog_path(home)),'targets':targets,'snapshot':m.read_config(m.state_dir(home)/'snapshot.json'),'skills':skills,'policies':m.read_config(m.state_dir(home)/'client-policies.json')}
def record(home,kind,name,rows):
 result=action(home,kind,name);rows.append((name,result,state(home)))
 return result
def setup(home):
 put(home,'.claude.json',json.dumps({'mcpServers':{'linear-server':{'command':'first'},'linear':{'command':'second'},'demo':{'url':'https://example.test/mcp'},'gmail':{'url':'https://remote.example/mcp'}},'projects':{'/work':{'mcpServers':{'gmail':{'url':'http://127.0.0.1:7288/mcp'},'project':{'url':'https://project.example/mcp'}}}}}))
 put(home,'.codex/config.toml','''# root comment
model='unchanged'
big=9223372036854775808
negative=-9223372036854775809
when=1979-05-27T07:32:00.123456789Z
unknown=nan
[[unknown_tables]]
foo=true
[mcp_servers.demo]
url='https://example.test/mcp'
startup_timeout_sec=70
required=true
when=1979-05-27T07:32:00.123456789Z
truth=[true,false]
[mcp_servers.demo.tools.write]
approval_mode='approve'
[mcp_servers.gmail]
url='http://127.0.0.1:7000/mcp'
[mcp_servers.node_repl]
command='native'
when=1979-05-27
truth=[true,false]
[mcp_servers.chrome-current]
command='old-browser'
[mcp_servers.playwright]
command='unverified'
''')
 put(home,'.config/opencode/opencode.jsonc','''{/* comment */"mcp":{"url-server":{"type":"remote","url":"https://example.test/a//b?q=,}",}},"keep":{"$serde_json::private::Number":"value","$serde_json::private::RawValue":"value2","unicode":"é😀"}}''')
 for path,key in [('.gemini/settings.json','mcpServers'),('.config/Code/User/mcp.json','servers'),('.config/github-copilot/intellij/mcp.json','servers')]:put(home,path,json.dumps({key:{'other':{'url':'https://other.example/mcp'}}}))
 put(home,'.claude/skills/demo/SKILL.md','old');put(home,'.claude/skills/common/resource','resource');(home/'.claude/skills/demo/resources').symlink_to('../common')
 (home/'.agents/skills').mkdir(parents=True);(home/'.agents/skills/demo').symlink_to(home/'.claude/skills/demo')
 put(home,'repo/external/SKILL.md','external');(home/'.agents/skills/external').symlink_to(home/'repo/external')
 (home/'.codex/skills').symlink_to(home/'.agents/skills')
def sequence(kind):
 home=ROOT/'home';shutil.rmtree(ROOT,ignore_errors=True);home.mkdir(parents=True);setup(home);rows=[]
 for name in ['import','status','sync']:assert 'ok' in record(home,kind,name,rows),rows[-1][1]
 raw={str(t['path']):t['path'].read_bytes()for t in m.targets(home)}
 record(home,kind,'sync',rows);assert all(pathlib.Path(p).read_bytes()==b for p,b in raw.items())
 p=home/'.claude.json';d=m.read_config(p);d['mcpServers']['demo']={'command':'native-edit','args':['é😀']};put(home,'.claude.json',json.dumps(d));record(home,kind,'sync',rows)
 c=m.read_config(m.catalog_path(home));c['servers']['demo']['enabled']=False;m.save_json(m.catalog_path(home),c);record(home,kind,'sync',rows)
 c['servers']['demo']['enabled']=True;m.save_json(m.catalog_path(home),c);record(home,kind,'sync',rows)
 put(home,'.claude-accounts/new/.claude.json',json.dumps({'mcpServers':{'extra':{'command':'new-account'}}}));record(home,kind,'sync',rows)
 p=home/'.grok/skills/demo';p.unlink();p.mkdir();(p/'SKILL.md').write_text('reinstall');record(home,kind,'sync',rows)
 p=home/'.claude.json';d=m.read_config(p);d['mcpServers']['demo']={'command':'conflict-native'};put(home,'.claude.json',json.dumps(d));c=m.read_config(m.catalog_path(home));c['servers']['demo']['command']='conflict-catalog';m.save_json(m.catalog_path(home),c)
 before={str(t['path']):t['path'].read_bytes()for t in m.targets(home)};result=record(home,kind,'sync',rows);assert 'error' in result;assert all(pathlib.Path(p).read_bytes()==b for p,b in before.items())
 return rows
expected=sequence('python');actual=sequence('rust')
for i,(a,b)in enumerate(zip(expected,actual)):
 if a!=b:
  # Keep values private: identify only stage and structural key.
  mismatches=[k for k in a[2]if a[2][k]!=b[2][k]]
  raise AssertionError(f'differential stage {i} {a[0]} differs: result={a[1]!=b[1]}, state keys={mismatches}')
assert len(expected)==len(actual)
print(f'Catalog differential oracle: {len(expected)} matching import/status/sync stages; exact fingerprints, policies, skills, conflicts and no-op bytes.')
scalars=[
 '9223372036854775807','9223372036854775808','-9223372036854775809','99999999999999999999999999999999999999999999999','0xFFFFFFFFFFFFFFFFFFFFFFFF','0o7777777777777777777777777','0b111111111111111111111111111111111111111111111111111111111111111111111',
 '1_000','1.0','-0.0','1e-7','1e+20','nan','+inf','-inf',
 '1979-05-27T07:32:00.123456789Z','1979-05-27T07:32:00.000000001-00:00','1979-05-27T07:32:00.100001999+03:15','07:32:00.1','1979-05-27',
 '[[true,false],{flag=true}]','"é😀\\u007F"','{"$serde_json::private::Number"="opaque", "$serde_json::private::RawValue"="opaque", sibling=1}'
]
for idx,scalar in enumerate(scalars):
 outcomes=[]
 for kind in ['python','rust']:
  shutil.rmtree(ROOT,ignore_errors=True);home=ROOT/'home';home.mkdir(parents=True)
  put(home,'.codex/config.toml',f'[mcp_servers.demo]\ncommand="echo"\nunknown={scalar}\n')
  result=action(home,kind,'import');assert 'ok' in result,(idx,kind,result)
  outcomes.append((result,state(home)))
 assert outcomes[0]==outcomes[1],f'TOML scalar fingerprint mismatch case {idx}'
print(f'TOML differential oracle: {len(scalars)} matching scalar/array/opaque-map snapshots.')
for comment in ['/* " */', '// "\n', '/* "balanced" */']:
 outcomes=[]
 for kind in ['python','rust']:
  shutil.rmtree(ROOT,ignore_errors=True);home=ROOT/'home';home.mkdir(parents=True)
  put(home,'.claude.json',comment+'{"unknown":[NaN,Infinity,-Infinity],"literal":"NaN \\"Infinity\\" -Infinity","marker":"__comandos_nonfinite_0__","mcpServers":{"demo":{"command":"echo"}}}')
  result=action(home,kind,'import');assert 'ok' in result,(repr(comment),kind,result)
  outcomes.append((result,state(home)))
 assert outcomes[0]==outcomes[1],f'JSONC comment differential mismatch {repr(comment)}'
print('JSONC differential oracle: 3 comment styles with nonfinite atoms and quoted literals match.')
# The sync lock is the same flock domain as the original Python manager.
import time
shutil.rmtree(ROOT,ignore_errors=True);home=ROOT/'home';home.mkdir(parents=True)
with m.locked(home):
 child=subprocess.Popen([BIN,'--home',str(home),'import'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
 time.sleep(.1);assert child.poll() is None;assert not m.catalog_path(home).exists()
stdout,stderr=child.communicate(timeout=10);assert child.returncode==0,stderr
assert json.loads(stdout)['servers']==0
print('Python flock and Rust sync transaction lock interoperate.')
