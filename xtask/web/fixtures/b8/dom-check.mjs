// Node DOM parser only. Uses the product document and actual original/WASM handlers.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {JSDOM} from 'jsdom';
const require=createRequire(import.meta.url);
const tick=()=>new Promise(r=>setImmediate(r));
async function settled(check){for(let i=0;i<100;i++){await new Promise(r=>setTimeout(r,5));if(check())return;}throw new Error('Node DOM did not settle within 100 bounded turns');}
export async function runDOM(repo,original,candidate,markdownit,fetchMarkdown){
 const globals=new Map(['document','window','location','localStorage','navigator','requestAnimationFrame','cancelAnimationFrame','fetch','CSS'].map(k=>[k,Object.getOwnPropertyDescriptor(globalThis,k)]));
 const html=fs.readFileSync(repo+'/dash/index.html','utf8');
 const cases=[];const snapshots=[];const lifecycle=[];
 for(const [label,api]of [['original',original],['wasm',candidate]]){
  const dom=new JSDOM(html,{url:'http://private.example.invalid/'});const w=dom.window;w.matchMedia=()=>({matches:false});w.HTMLElement.prototype.scrollIntoView=function(){};w.HTMLElement.prototype.scrollTo=function(o){this.scrollLeft=o.left||0;};w.requestAnimationFrame=f=>setTimeout(f,0);w.cancelAnimationFrame=clearTimeout;
  const copy=[];Object.defineProperty(w.navigator,'clipboard',{value:{writeText:async text=>copy.push(text)}});
  for(const[k,v]of Object.entries({window:w,document:w.document,location:w.location,localStorage:w.localStorage,navigator:w.navigator,requestAnimationFrame:w.requestAnimationFrame,cancelAnimationFrame:w.cancelAnimationFrame,CSS:{escape:s=>String(s).replace(/[^\w-]/g,'\\$&')}}))Object.defineProperty(globalThis,k,{value:v,writable:true,configurable:true});
  const purifier=require(repo+'/dash/vendor/purify-3.4.16.min.js')(w);assert.equal(purifier.isSupported,true);
  const at=Date.UTC(2026,9,5,12);const ids=['2026-10-05@09:00','2026-10-05@15:00'];
  const edition=id=>({edition:{id,localDate:'2026-10-05',slot:id.split('@')[1],status:'published',storyCount:1,models:{'claude:model':1},costUsd:1.115,sourceCount:1,job:{startedAt:at,finishedAt:at+60000}},stories:[{id:id===ids[0]?1:2,title:'Título <seguro> 🍅',category:'oficial',meta:{lab:'Laboratorio'},summary:'**Resumen** de la noticia.',body:'## Detalle\n\n- Una fuente\n- Un cliente',model:'claude:model',counts:{notes:0,chat:0,saved:false},sources:[{id:7,captured:true,official:true,origin:'Publicación',url:'https://example.org/article',heat:'oficial'}]}]});
  const requests=[];let notes=[],saved=[],messages=[],translated=false,deferMarkdown=false,resolveMarkdown;
  const source=()=>({id:7,title:'Fuente capturada',url:'https://example.org/article',official:true,publishedAt:at,capture:{title:'Original <fuente>',capturedAt:at,byline:'Autor',blocks:[{type:'p',text:'Captura `local` <texto>'},{type:'img',media:'0123456789abcdef0123456789abcdef.png',alt:'Diagrama'}]},translation:translated?{state:'done',title:'Traducción guardada',model:'claude:model',blocks:[{type:'p',text:'Traducida'}]}:null});
  const fetchJson=async(path,body)=>{
   requests.push({path,body:body===undefined?null:structuredClone(body)});
   if(path==='/web/markdown'){if(deferMarkdown&&body.text==='deferred old body')return new Promise(resolve=>resolveMarkdown=resolve);return fetchMarkdown(path,body);}
   if(path==='/news/editions')return{configured:true,latest:ids[0],editions:ids.map(id=>edition(id).edition),next:{id:'2026-10-06@09:00',localDate:'2026-10-06',slot:'09:00',status:'scheduled'},chain:['claude:model']};
   if(path.startsWith('/news/edition?id='))return edition(decodeURIComponent(path.split('=')[1]));
   if(path==='/news/saved'&&body){saved=body.saved?[{...edition(ids[0]).stories[0],editionId:ids[0]}]:[];return{ok:true};}
   if(path==='/news/saved')return{stories:saved};
   if(path.startsWith('/news/source?id='))return source();
   if(path==='/news/translate'){translated=true;return{translation:source().translation};}
   if(path==='/news/chat'&&body){messages=[{id:10,role:'user',state:'done',text:body.message},{id:11,role:'assistant',state:'done',text:'**Respuesta** basada en la captura.',model:'claude:model',cite:'Fuente ¶1',noted:false}];return{messages};}
   if(path.startsWith('/news/chat?story='))return{messages};
   if(path==='/news/chat/note'){messages[1].noted=!messages[1].noted;return{noted:messages[1].noted};}
   if(path==='/news/notes'&&body){if(body.action==='add')notes.push({id:notes.length+20,storyId:body.storyId,storyTitle:'Título <seguro> 🍅',editionId:ids[0],createdAt:at,kind:'text',text:body.text});if(body.action==='update'){const n=notes.find(n=>n.id===body.noteId);n.text=body.text;}if(body.action==='delete')notes=notes.filter(n=>n.id!==body.noteId);return{ok:true};}
   if(path.startsWith('/news/notes?'))return{notes:notes.filter(n=>!path.includes('?q=')||n.text.includes(decodeURIComponent(path.split('=')[1]))),total:notes.length};
   throw new Error('Unexpected news API '+path);
  };
  const media=[];globalThis.fetch=async(path,opts)=>{assert.match(path,/^\/news\/media\/[0-9a-f]{32}\.png$/);assert.equal(opts.headers['X-Comandos-Token'],'fixture-media-token');media.push(path);return{ok:true,blob:async()=>new Blob(['local'])}};w.localStorage.setItem('cc_token','fixture-media-token');
  const reader=api.mount({document:w.document,markdownit,purify:purifier,fetchJson,mountTerminal:async host=>{host.innerHTML='<p>Fixture terminal</p>';}});
  const q=s=>reader.element.querySelector(s);const click=s=>{const el=q(s);assert.ok(el,label+' missing '+s);el.click();};
  const snapshot=name=>{cases.push(label+': '+name);snapshots.push({label,name,edition:q('.nr-edition').innerHTML,panel:q('.nr-panel').innerHTML.replace(/blob:nodedata:[0-9a-f-]+/g,'blob:nodedata:owned-local-fixture'),state:structuredClone({open:reader.state.open,tab:reader.state.tab,view:reader.state.view,terminal:reader.state.terminal,share:reader.state.share,size:reader.state.size,scope:reader.state.scope,notes:reader.state.notes,allNotes:reader.state.allNotes}),requests:structuredClone(requests.filter(r=>r.path!=='/web/markdown'))});};
  await reader.open();await settled(()=>q('.nr-edition h1'));assert.equal(requests.filter(r=>r.body&&r.path!=='/web/markdown').length,0);snapshot('read never generates');
  click('.nr-details-toggle');await settled(()=>q('.nr-details'));snapshot('provenance details');
  click('[data-open="1"]');await settled(()=>q('[data-tab="fuentes"]'));assert.equal(reader.state.open,1);snapshot('open article');
  click('[data-tab="fuentes"]');await settled(()=>q('.nr-captured'));await settled(()=>q('img[data-media]')?.src.startsWith('blob:'));assert.equal(media.length,1);assert.ok(!q('.nr-panel').innerHTML.includes('src="https://'));const external=q('.nr-src-head a');assert.equal(external.target,'_blank');assert.equal(external.rel,'noopener noreferrer nofollow');const linkClick=new w.MouseEvent('click',{bubbles:true,cancelable:true});external.dispatchEvent(linkClick);assert.equal(linkClick.defaultPrevented,false);snapshot('captured source local blob');
  click('[data-translate="7"]');await settled(()=>q('.nr-article h1')?.textContent==='Traducción guardada');snapshot('explicit translation only');
  click('[data-lang="orig"]');await settled(()=>q('.nr-article h1')?.textContent==='Original <fuente>');snapshot('original source toggle');
  click('[data-tab="chat"]');await settled(()=>q('[data-ask]'));click('[data-ask]');await settled(()=>q('.nr-msg:not(.me)'));snapshot('explicit chat only');
  const input=q('.nr-chat-input');input.value='/nota Dos líneas\nsegunda';q('.nr-dock').dispatchEvent(new w.Event('submit',{bubbles:true,cancelable:true}));await settled(()=>notes.length===1);assert.equal(requests.filter(r=>r.path==='/news/chat'&&r.body).length,1);await settled(()=>reader.state.notes?.notes.length===1);snapshot('slash nota never asks agent');
  click('[data-tab="notas"]');await settled(()=>q('[data-note-edit]'));click('[data-note-edit]');await settled(()=>q('.nr-note-edit'));q('.nr-note-edit').value='Actualizada <segura>';click('[data-note-save]');await settled(()=>notes[0].text==='Actualizada <segura>'&&!q('.nr-note-edit'));snapshot('edit note');
  click('[data-note-copy]');await settled(()=>copy.length===1);assert.equal(copy[0],'Actualizada <segura>');click('[data-note-ask-delete]');await settled(()=>q('[data-note-delete]'));assert.equal(notes.length,1);click('[data-note-cancel]');await settled(()=>!q('[data-note-delete]'));assert.equal(notes.length,1);snapshot('delete confirmation cancel');
  click('[data-note-ask-delete]');await settled(()=>q('[data-note-delete]'));click('[data-note-delete]');await settled(()=>notes.length===0);await settled(()=>q('.nr-empty'));snapshot('confirmed delete');
  click('.nr-panel-close');await settled(()=>q('.nr-panel').hidden);click('[data-save="1"]');await settled(()=>saved.length===1&&q('[data-save="1"]').textContent==='Guardada');click('.nr-saved-btn');await settled(()=>q('.nr-edition h1')?.textContent==='Noticias guardadas');snapshot('save and saved view');
  click('.nr-back-edition');await settled(()=>q('.nr-edition h1')?.textContent==='Resumen de las 09:00');click('[data-font="1"]');assert.equal(reader.state.size,17);q('.nr-edition-divider').dispatchEvent(new w.KeyboardEvent('keydown',{key:'ArrowLeft',bubbles:true,cancelable:true}));assert.equal(reader.state.share,63);await reader.setTerminal(true);assert.ok(q('.nr-terminal-host').textContent.includes('Fixture terminal'));assert.equal(q('.nr-terminal').hidden,false);snapshot('manual terminal font split');
  const before=requests.filter(r=>r.path.startsWith('/news/edition')).length;reader.close();assert.equal(reader.element.hidden,true);await reader.open();await settled(()=>Array.isArray(reader.state.allNotes?.notes));assert.equal(requests.filter(r=>r.path.startsWith('/news/edition')).length,before);snapshot('reopen preserves edition');
  if(label==='wasm'){
   reader.state.current.stories.push({...edition(ids[0]).stories[0],id:2,title:'Second selection',summary:'Second summary',body:'Second body'});
   reader.state.current.stories[0].body='deferred old body';deferMarkdown=true;
   reader.openStory(1);await settled(()=>typeof resolveMarkdown==='function');reader.openStory(2);resolveMarkdown({html:'<p>deferred old body</p>'});await settled(()=>q('.nr-pt b')?.textContent==='Second selection');assert.equal(reader.state.open,2);lifecycle.push({name:'late Markdown cannot paint previous article',equal:true});
   const controlled=api.mount({document:w.document,markdownit,purify:purifier,fetchJson,mountTerminal:()=>new Promise(()=>{})});await controlled.open();const pendingTerminal=controlled.setTerminal(true);await tick();controlled.close();await Promise.race([pendingTerminal,new Promise((_,reject)=>setTimeout(()=>reject(new Error('terminal cancellation did not settle')),1500))]);assert.equal(controlled.state.terminalMounted,false);assert.equal(controlled.element.hidden,true);controlled.dispose();lifecycle.push({name:'close settles owned noncooperative terminal and permits retry',equal:true});
   const closed=api.mount({document:w.document,markdownit,purify:purifier,fetchJson});const oldOpen=closed.open();closed.close();await assert.rejects(oldOpen,/Lectura cancelada/);assert.equal(closed.element.hidden,true);closed.dispose();lifecycle.push({name:'close before open future prevents queued activity',equal:true});
  }
  if(reader.dispose){reader.dispose();assert.equal(reader.element.isConnected,false);}else reader.close();dom.window.close();
 }
 for(const[k,descriptor]of globals){if(descriptor)Object.defineProperty(globalThis,k,descriptor);else delete globalThis[k];}
 const groups=new Map();for(const row of snapshots){const list=groups.get(row.name)||[];list.push(row);groups.set(row.name,list);}
 const compared=[];for(const[name,rows]of groups){assert.equal(rows.length,2);const [a,b]=rows;assert.deepEqual(b.state,a.state,name+' state');assert.deepEqual(b.requests,a.requests,name+' exact source API calls');compared.push({name,equal:true,baselineHTML:{edition:a.edition,panel:a.panel},candidateHTML:{edition:b.edition,panel:b.panel}});}
 return{document:'actual production dash/index.html parsed without resources/scripts',jsdomVersion:require('./node_modules/jsdom/package.json').version,visualAcceptance:false,blobNormalization:'only random owned local blob UUID replaced after authenticated GET and blob protocol checks',lifecycle,cases:compared};
}
