import json
import os
from pathlib import Path
import subprocess


def test_opencode_metadata_is_pid_start_scoped_and_excludes_delegated_sessions(tmp_path):
    plugin=tmp_path/'plugin.mjs';plugin.write_text(Path('adapters/opencode-comandos.js').read_text())
    script=tmp_path/'probe.mjs'
    script.write_text('''import {Comandos} from './plugin.mjs';
import {readFile} from 'node:fs/promises';
globalThis.fetch=async()=>({ok:true});
const plugin=await Comandos({directory:process.env.HOME,client:{session:{get:async({path})=>({data:{id:path.id,parentID:path.id==='ses_child'?'ses_root':undefined}})}}});
await plugin.event({event:{type:'message.updated',properties:{info:{role:'user',sessionID:'ses_root'}}}});
await plugin.event({event:{type:'message.updated',properties:{info:{role:'assistant',sessionID:'ses_root',modelID:'model',providerID:'provider'}}}});
await plugin.event({event:{type:'message.updated',properties:{info:{role:'user',sessionID:'ses_child'}}}});
await plugin.event({event:{type:'session.idle',properties:{sessionID:'ses_root'}}});
const data=JSON.parse(await readFile(process.env.HOME+'/.claude/hooks/native-processes/'+process.pid+'.json','utf8'));
if(data.sessionId!=='ses_root'||data.pid!==process.pid||!data.start||data.model!=='provider/model'||data.busy!==false)throw Error('bad metadata');
console.log('metadata-ok');
''')
    result=subprocess.run(['node',str(script)],env={**os.environ,'HOME':str(tmp_path)},capture_output=True,text=True)
    assert result.returncode==0,result.stderr
    assert result.stdout.strip()=='metadata-ok'
    files=list((tmp_path/'.claude/hooks/native-processes').glob('*.json'))
    assert len(files)==1 and files[0].stat().st_mode&0o777==0o600
    data=json.loads(files[0].read_text())
    assert set(data)<={'pid','start','harness','sessionId','parentId','updatedAt','busy','model','effort'}


def test_agy_hook_records_own_process_session_even_without_workspace(tmp_path):
    hooks=tmp_path/'.claude/hooks';hooks.mkdir(parents=True)
    notifier=hooks/'cc-notify.sh';notifier.write_text('#!/bin/sh\nexit 0\n');notifier.chmod(0o700)
    probe=tmp_path/'probe.py'
    hook=str(Path('adapters/agy-hooks.sh').resolve())
    probe.write_text('import json,os,pathlib,subprocess\n'+
        'subprocess.run(["bash",'+repr(hook)+',"working"],input=json.dumps({"conversationId":"exact-agy","workspacePaths":[],"modelName":"gemini-model"}),text=True,check=True,capture_output=True)\n'+
        'p=pathlib.Path(os.environ["HOME"])/".claude/hooks/native-processes"/(str(os.getpid())+".json")\n'+
        'd=json.loads(p.read_text());assert d["sessionId"]=="exact-agy" and d["busy"] is True and d["pid"]==os.getpid()\n')
    result=subprocess.run(['bash','-c','exec -a agy python3 "$1"','fixture',str(probe)],env={**os.environ,'HOME':str(tmp_path)},capture_output=True,text=True)
    assert result.returncode==0,result.stderr
