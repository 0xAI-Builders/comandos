"""One guarded draft mutation per batch; no accidental activation or restart."""
import subprocess
from pathlib import Path


def test_batches_respect_group_filter_unknown_locked_and_save_once():
    source=Path('dash/extensions.js').read_text().split('  // The dashboard shelf height')[0]+'\nglobalThis.TestShelf=Shelf;})();'
    script=r'''
const assert=require('node:assert/strict');
global.document={addEventListener(){},activeElement:null};global.setTimeout=()=>0;global.clearTimeout=()=>{};
global.fetch=()=>new Promise(()=>{});global.localStorage={getItem(){return ''}};
const root={addEventListener(){},querySelectorAll(){return []},setAttribute(){},innerHTML:''};
''' + source + r'''
const shelf=new TestShelf(root,{session:'test',pane:'%1',harness:'codex'});
shelf.state={inventory:{mcps:[{id:'m',name:'Mail',toggleable:true,origin:{id:'shared'}}],skills:[
{id:'a',name:'Alpha',toggleable:true,origin:{id:'matt'}},{id:'b',name:'Beta',toggleable:true,origin:{id:'obra'}},
{id:'blocked',name:'Blocked',toggleable:false,origin:{id:'matt'}},{id:'unknown',name:'Unknown',toggleable:true,origin:{id:'matt'}}]},
desired:{mcps:{m:false},skills:{a:false,b:false,blocked:false}},loaded:null};
const calls=[];shelf.mutate=(path,data)=>calls.push({path,data});
shelf.batch(true,'matt');assert.equal(calls.length,1);assert.equal(calls[0].path,'');
assert.deepEqual(calls.pop().data.desired,{mcps:{m:false},skills:{a:true,b:false,blocked:false}});
shelf.batch(true);assert.deepEqual(calls.pop().data.desired,{mcps:{m:true},skills:{a:true,b:true,blocked:false}});
shelf.filter='skills';shelf.query='Alpha';shelf.batch(true);
assert.deepEqual(calls.pop().data.desired,{mcps:{m:false},skills:{a:true,b:false,blocked:false}});
shelf.batch(false);assert.equal(calls.length,0);
shelf.sending=true;shelf.batch(true);assert.equal(calls.length,0);
shelf.sending=false;shelf.stale=true;shelf.batch(true);assert.equal(calls.length,0);
'''
    result=subprocess.run(['node','-e',script],capture_output=True,text=True)
    assert result.returncode==0,result.stderr
