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
 const state={inventory:{mcps:[{id:'removed',name:'Removed MCP',enabled:false,toggleable:false,reason:'Desactivado en el catálogo compartido'},{id:'optional',name:'Optional MCP',enabled:false,toggleable:true,origin:{id:'repo:source',label:'source/repo'},size:{tokens:1000,tokenizer:'cl100k_base',basis:'tool-definitions'}},{id:'unknown',name:'Unknown MCP',enabled:null,toggleable:false}],skills:[]},desired:{mcps:{removed:false,optional:false},skills:{}},loaded:null,harness:'codex',conversationId:'c',templates:[],busy:false,configurationStatus:'external'};
 pending.shift()({ok:true,json:async()=>state});await retry;
 assert.match(root.innerHTML,/shelf-enter/);
 assert.doesNotMatch(root.innerHTML,/terminar el turno|Aún no hemos comprobado/);
 assert.match(root.innerHTML,/Estado del proceso/);
 assert.match(root.innerHTML,/<div class="notice " role="status"><\/div>/);
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
 assert.match(available,/source\/repo/);
 assert.match(available,/1000 tokens/);
 assert.match(available,/--bubble-size:88px/);
 assert.match(root.innerHTML,/cl100k_base/);
 const large=shelf.bubble({...state.inventory.mcps[1],size:{tokens:2250}},'mcps',false);
 assert.match(large,/--bubble-size:132px/);
 const unknownSize=shelf.bubble({...state.inventory.mcps[1],size:{tokens:null}},'mcps',false);
 assert.match(unknownSize,/Sin medir/);
 shelf.closedGroups.add('off:repo:source');shelf.render();
 assert.match(root.innerHTML,/data-group="off:repo:source" ><summary/);
 assert.doesNotMatch(available,/Removed MCP|Unknown MCP/);
 assert.match(available,/Sin seleccionar · 1/);
 assert.match(root.innerHTML.split('<footer>')[1],/Removed MCP/);
 shelf.state={...state,configurationStatus:'unverified'};shelf.render();
 assert.match(root.innerHTML,/No se pudo verificar la configuración aplicada/);
 shelf.state=state;shelf.render();
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


PRELUDE = r'''
const assert=require('node:assert/strict');
global.document={activeElement:null,addEventListener(){},getElementById(){return null}};
global.localStorage={getItem(){return ''}};
global.clearTimeout=()=>{};global.setTimeout=()=>0;
let pending=[],requests=[];
global.fetch=(path,opt)=>new Promise(resolve=>{requests.push({path,method:opt.method,body:opt.body&&JSON.parse(opt.body)});pending.push(resolve);});
const WAIT_TURN=/terminar el turno|Aplicar al terminar|Interrumpir turno/;
const reply=(data,ok=true)=>pending.shift()({ok,json:async()=>data});
const root={innerHTML:'',addEventListener(){},contains(){return false},querySelectorAll(){return []},setAttribute(){}};
'''


def run(body):
    source = Path('dash/extensions.js').read_text().split('  // The dashboard shelf height')[0]
    source += '\n globalThis.TestShelf=Shelf;})();'
    script = PRELUDE + source + "\n(async()=>{\n" + body + "\n})().catch(e=>{console.error(e);process.exitCode=1});"
    result = subprocess.run(['node', '-e', script], capture_output=True, text=True)
    assert result.returncode == 0, result.stderr


STATE = r'''
const row=(id,name,extra={})=>({id,name,enabled:false,toggleable:true,origin:{id:'o',label:'Origen'},size:{tokens:100},...extra});
const base={identity:'pane-1',conversationId:'c',harness:'codex',revision:4,templates:[],busy:false,configurationStatus:'verified',loaded:null,
 inventory:{status:'configured',mcps:[row('m','Mail')],skills:[row('a','Alpha'),row('b','Beta'),row('fixed','Fixed',{toggleable:false,reason:'Se controla mediante el grupo del plugin completo.'}),row('ghost','Ghost')]},
 desired:{mcps:{m:false},skills:{a:false,b:false,fixed:true}}};
'''


def test_excluded_and_unknown_rows_are_counted_and_explained():
    run(STATE + r'''
 const shelf=new TestShelf(root,{session:'s',pane:'%1',harness:'codex'});reply(base);await new Promise(setImmediate);
 const bar=root.innerHTML.split('class="selection-toolbar"')[1].split('</div>')[0];
 assert.match(bar,/0 de 3/);                         // Mail, Alpha, Beta: editable
 assert.match(bar,/2 fuera del lote/);                // Fixed (plugin) + Ghost (unknown state)
 const footer=root.innerHTML.split('<footer>')[1];
 assert.match(footer,/2 excluidas o gestionadas aparte/);
 assert.match(footer,/<b>Fixed<\/b> · Se controla mediante el grupo del plugin completo\./);
 assert.match(footer,/<b>Ghost<\/b> · Estado desconocido en este panel/);
''')


def test_global_buttons_ignore_search_and_filter_and_send_one_mutation():
    run(STATE + r'''
 const shelf=new TestShelf(root,{session:'s',pane:'%1',harness:'codex'});reply(base);await new Promise(setImmediate);
 shelf.filter='skills';shelf.query='alp';shelf.render();
 assert.match(root.innerHTML,/Añadir resultados \(1\)/);
 const click=(attrs)=>shelf.click({target:{closest:()=>({disabled:false,dataset:attrs})},preventDefault(){}});
 click({batch:'on'});                                 // "Seleccionar todos"
 assert.equal(requests.length,2);
 assert.deepEqual(requests[1].body.desired,{mcps:{m:true},skills:{a:true,b:true,fixed:true}});
 assert.equal(requests[1].body.revision,4);
 reply({...base,revision:5,desired:requests[1].body.desired});await new Promise(setImmediate);
 assert.equal(requests.length,2);                     // the save answer is reused: no second read
 assert.equal(shelf.state.revision,5);
 click({batch:'off'});                                // "Quitar todos" with the search still active
 assert.deepEqual(requests[2].body.desired,{mcps:{m:false},skills:{a:false,b:false,fixed:true}});
 reply({...base,revision:6});await new Promise(setImmediate);
 click({batch:'on',batchScope:'visible'});            // only the visible result
 assert.deepEqual(requests[3].body.desired,{mcps:{m:false},skills:{a:true,b:false,fixed:true}});
''')


def test_saving_status_and_turn_wording_only_with_activity_evidence():
    run(STATE + r'''
 const shelf=new TestShelf(root,{session:'s',pane:'%1',harness:'codex'});reply(base);await new Promise(setImmediate);
 shelf.batch(true);
 assert.match(root.innerHTML,/Guardando selección/);
 assert.doesNotMatch(root.innerHTML,WAIT_TURN);
 reply({error:'la selección cambió en otro cliente; vuelve a cargarla'},false);await new Promise(setImmediate);
 reply(base);await new Promise(setImmediate);
 assert.match(root.innerHTML,/cambió en otro cliente/);
 shelf.error='';shelf.state={...base,inventory:{...base.inventory,status:'incomplete'}};shelf.render();
 assert.match(root.innerHTML,/Inventario incompleto/);
 assert.doesNotMatch(root.innerHTML,WAIT_TURN);
 shelf.state={...base,configurationStatus:'unverified'};shelf.render();
 assert.match(root.innerHTML,/No se pudo verificar la configuración aplicada/);
 assert.doesNotMatch(root.innerHTML,WAIT_TURN);assert.doesNotMatch(root.innerHTML,/Inventario incompleto/);
 shelf.state={...base,busy:true};shelf.render();
 assert.match(root.innerHTML,/Aplicar al terminar/);
 assert.match(root.innerHTML,/Interrumpir turno y aplicar/);
''')


def test_stale_responses_are_discarded():
    run(STATE + r'''
 const shelf=new TestShelf(root,{session:'s',pane:'%1',harness:'codex'});reply(base);await new Promise(setImmediate);
 const poll=shelf.refresh();                          // a periodic read is in flight
 shelf.batch(true);                                   // then the user saves a batch
 const saved={...base,revision:5,desired:{mcps:{m:true},skills:{a:true,b:true,fixed:true}}};
 pending[1]({ok:true,json:async()=>saved});pending.splice(1,1);await new Promise(setImmediate);
 reply({...base,revision:4});await poll;              // the older read answers last
 assert.equal(shelf.state.revision,5);
 const change=shelf.choose('claude');                 // harness change supersedes any read
 const late=pending.length;
 reply({...base,harness:'claude',revision:9});await change;
 assert.equal(shelf.state.harness,'claude');
''')


def test_unchanged_poll_does_not_rebuild_the_shelf():
    run(STATE + r'''
 const shelf=new TestShelf(root,{session:'s',pane:'%1',harness:'codex'});reply(base);await new Promise(setImmediate);
 let renders=0;const render=shelf.render.bind(shelf);shelf.render=()=>{renders++;render();};
 const poll=shelf.refresh();reply(structuredClone(base));await poll;
 assert.equal(renders,0);
 const next=shelf.refresh();reply({...base,revision:5});await next;
 assert.equal(renders,1);
''')
