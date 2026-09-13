import json
import subprocess
from pathlib import Path


def test_service_worker_never_caches_new_or_unknown_api_routes():
    root=Path(__file__).resolve().parents[1]
    script="""
      const fs=require('fs'),vm=require('vm');const listeners={};
      const context={URL,Promise,self:{location:{origin:'https://comandos.test'},addEventListener:(k,fn)=>listeners[k]=fn},
        fetch:async()=>({clone:()=>({})}),caches:{open:async()=>({put:()=>{}})}};
      vm.runInNewContext(fs.readFileSync('dash/sw.js','utf8'),context);
      const results={};
      for(const path of ['/session-profiles','/model/status?operationKey=x','/extension-usage','/operator/chat/stream','/future-api','/workspace.js']){
        let intercepted=false;
        listeners.fetch({request:{method:'GET',url:'https://comandos.test'+path},respondWith:()=>{intercepted=true;}});
        results[path]=intercepted;
      }
      console.log(JSON.stringify(results));
    """
    result=subprocess.run(['node','-e',script],cwd=root,capture_output=True,text=True,check=True)
    data=json.loads(result.stdout)
    assert data.pop('/workspace.js') is True
    assert not any(data.values())
