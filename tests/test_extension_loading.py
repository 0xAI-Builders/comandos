"""Loading and failure states without a browser or a live pane."""
import subprocess
from pathlib import Path


def test_loading_precedes_response_and_reappears_on_retry_and_harness_change():
    source = Path('dash/extensions.js').read_text().split('  // The dashboard shelf height')[0]
    source += '\n globalThis.TestShelf=Shelf;})();'
    script = r'''
const assert=require('node:assert/strict');
global.document={activeElement:null,addEventListener(){},getElementById(){return null}};
global.localStorage={getItem(){return ''}};
global.clearTimeout=()=>{};global.setTimeout=()=>0;
let pending=[];
global.fetch=()=>new Promise(resolve=>pending.push(resolve));
const root={innerHTML:'',addEventListener(){},contains(){return false},querySelectorAll(){return []},setAttribute(){}};
''' + source + r'''
(async()=>{
 const shelf=new TestShelf(root,{session:'test',pane:'%1',harness:'codex'});
 assert.match(root.innerHTML,/Cargando MCPs y skills/);
 assert.match(root.innerHTML,/loading-spinner/);
 pending.shift()({ok:false,json:async()=>({error:'No disponible'})});
 await new Promise(setImmediate);
 assert.match(root.innerHTML,/No disponible/);
 assert.match(root.innerHTML,/Reintentar/);
 assert.doesNotMatch(root.innerHTML,/loading-spinner/);
 const retry=shelf.refresh();
 assert.match(root.innerHTML,/Cargando MCPs y skills/);
 const state={inventory:{mcps:[{id:'removed',name:'Removed MCP',enabled:false,toggleable:false,reason:'Desactivado en el catálogo compartido'},{id:'optional',name:'Optional MCP',enabled:false,toggleable:true},{id:'unknown',name:'Unknown MCP',enabled:null,toggleable:false}],skills:[]},desired:{mcps:{removed:false,optional:false},skills:{}},loaded:null,harness:'codex',conversationId:'c',templates:[],busy:false};
 pending.shift()({ok:true,json:async()=>state});await retry;
 assert.match(root.innerHTML,/shelf-enter/);
 assert.doesNotMatch(root.innerHTML,/terminar el turno/);
 assert.match(root.innerHTML,/Aplicar y reanudar/);
 assert.doesNotMatch(root.innerHTML,/data-action="interrupt"/);
 shelf.state={...state,busy:true};shelf.render();
 assert.match(root.innerHTML,/Aplicar al terminar/);
 assert.match(root.innerHTML,/data-action="interrupt"/);
 shelf.state=state;shelf.render();
 assert.match(root.innerHTML,/Aplicar y reanudar/);
 assert.doesNotMatch(root.innerHTML,/data-action="interrupt"|terminar el turno/);
 const available=root.innerHTML.split('data-zone="off"')[1].split('<footer>')[0];
 assert.match(available,/Optional MCP/);
 assert.doesNotMatch(available,/Removed MCP|Unknown MCP/);
 assert.match(available,/Disponibles.*?1/);
 assert.match(root.innerHTML.split('<footer>')[1],/Removed MCP/);
 const poll=shelf.refresh();
 assert.doesNotMatch(root.innerHTML,/Cargando MCPs y skills/);
 pending.shift()({ok:true,json:async()=>state});await poll;
 assert.doesNotMatch(root.innerHTML,/shelf-enter/);
 const change=shelf.choose('claude');
 assert.match(root.innerHTML,/Cargando MCPs y skills/);
 pending.shift()({ok:true,json:async()=>({...state,harness:'claude'})});await change;
 assert.match(root.innerHTML,/shelf-enter/);
})().catch(e=>{console.error(e);process.exitCode=1});
'''
    result = subprocess.run(['node', '-e', script], capture_output=True, text=True)
    assert result.returncode == 0, result.stderr
