// Run against wasm-bindgen --target nodejs output; no browser process is used.
const assert=require('node:assert/strict');
const path=require('node:path');
const geometry=require('../../../dash/workspace-dock.js');
const {doc,makeEl,parse}=require('./workspace_dock_dom.cjs');
function enhance(el){if(!el||el.nodeType!==1||el._enhanced)return el;el._enhanced=true;el.style.setProperty=function(k,v){this[k.replace(/-([a-z])/g,(_,c)=>c.toUpperCase())]=String(v)};
 Object.defineProperty(el,'dataset',{configurable:true,get:()=>new Proxy({}, {get:(_,k)=>el.attrs['data-'+String(k).replace(/[A-Z]/g,c=>'-'+c.toLowerCase())],set:(_,k,v)=>{el.attrs['data-'+String(k).replace(/[A-Z]/g,c=>'-'+c.toLowerCase())]=String(v);return true;}})});
 Object.defineProperty(el,'firstElementChild',{get:()=>el.children.find(x=>x.nodeType===1)||null});
 Object.defineProperty(el,'innerHTML',{configurable:true,get:()=>el._html||'',set:v=>{el._html=v;el.renders=(el.renders||0)+1;parse(v,el);const walk=e=>{enhance(e);for(const c of e.children||[])if(c.nodeType===1)walk(c)};for(const c of el.children)if(c.nodeType===1)walk(c);}});
 el.append=(...children)=>children.forEach(c=>el.appendChild(c));el.prepend=c=>{el.appendChild(c);el.children.splice(el.children.indexOf(c),1);el.children.unshift(c)};el.toggleAttribute=(k,on)=>on?el.setAttribute(k,''):el.removeAttribute(k);el.setPointerCapture=()=>{};el.releasePointerCapture=()=>{};
 const qs=el.querySelector.bind(el);el.querySelector=s=>s===':scope > .ws-tree'?el.children.find(x=>x.nodeType===1&&x.classList.contains('ws-tree'))||null:qs(s);
 const closest=el.closest.bind(el);el.closest=s=>s.split(',').map(s=>closest(s.trim())).find(Boolean)||null;
 el.getBoundingClientRect=()=>{let r=el._rect||{left:0,top:80,width:1000,height:600};if(el.classList.contains('ws-tray'))r={left:50+Array.from(el.parentNode.children).indexOf(el)*150,top:140,width:130,height:80};return {...r,right:r.left+r.width,bottom:r.top+r.height}};return el;}
global.window=global;global.self=global;global.Window=class{static [Symbol.hasInstance](o){return o===global;}};global.document=doc;doc.readyState='complete';enhance(doc.body);
const create=doc.createElement;doc.createElement=t=>enhance(create(t));
const area=doc.createElement('div');area.id='term-area';area.attrs.id='term-area';area.clientWidth=1000;area.clientHeight=600;area.scrollLeft=0;area.scrollTop=0;doc.body.append(area);
const strip=doc.createElement('div');strip.attrs.id='tabbar';strip._rect={left:0,top:0,width:1000,height:44};strip.scrollLeft=0;doc.body.append(strip);
const meta=doc.createElement('meta');meta.attrs.name='comandos-web';meta.attrs.content='workspace-layout workspace-dock';doc.body.append(meta);
doc.getElementById=id=>doc.body.attrs.id===id?doc.body:doc.body.querySelector('#'+id);doc.querySelector=s=>doc.body.querySelector(s);doc.querySelectorAll=s=>doc.body.querySelectorAll(s);
doc.elementFromPoint=()=>doc.body.querySelector('.ws-leaf[data-tab="b"]');global.CSS={escape:x=>x};global.tf=(es,en)=>es;global.authToken=()=> 'fixture-token';global.activeTerm='local';global.activeView='term:local';global.openTerms=new Map();global.toast=(...args)=>toasts.push(args);const toasts=[];
const frames=[];for(const s of ['local','b']){const frame=doc.createElement('iframe');area.append(frame);frames.push(frame);global.openTerms.set(s,{label:s==='local'?'Local':'Beta',frame});}
const timers=new Map();let timerId=1;global.setTimeout=(f,ms)=>{const id=timerId++;timers.set(id,{f,ms});return id};global.clearTimeout=id=>timers.delete(id);
const rafs=new Map();let rafId=1;global.requestAnimationFrame=f=>{const id=rafId++;rafs.set(id,f);return id};global.cancelAnimationFrame=id=>rafs.delete(id);const windowListeners={};global.addEventListener=(t,f)=>(windowListeners[t]||=[]).push(f);
const requests=[];const pending=[];global.fetch=(p,opt)=>{if(p==='/web/ready')return Promise.resolve({ok:true,status:200,json:async()=>({})});requests.push({p,opt,body:JSON.parse(opt.body)});return new Promise(resolve=>pending.push(resolve));};
// Model the owner's actual let/const bindings, not merely window properties.
require('node:vm').runInThisContext(`const openTerms=globalThis.openTerms; let activeTerm=globalThis.activeTerm; let activeView=globalThis.activeView; globalThis.__dockRead=()=>({activeTerm,activeView}); globalThis.__dockWrite=(key,v)=>{if(key==='activeTerm')activeTerm=v;else activeView=v;};`);
for(const key of ['activeTerm','activeView'])Object.defineProperty(global,key,{configurable:true,get:()=>global.__dockRead()[key],set:v=>global.__dockWrite(key,v)});
const wasm=require(path.resolve(process.argv[2]||'/tmp/native-workspace-dock-wasm/comandos_web.js'));wasm.boot('fixture');assert.equal(global.__comandosAttached,true);const dock=global.WorkspaceDock;assert.ok(dock);
const r={left:0,top:0,right:200,bottom:100,width:200,height:100};for(const [x,y]of [[10,50],[195,50],[100,48],[300,50]])assert.equal(dock.edgeFor(r,x,y),geometry.edgeFor(r,x,y));assert.deepEqual(dock.TRAYS,geometry.TRAYS);
const state={schema:1,revision:4,groups:[{id:'g',tree:{type:'split',axis:'x',ratio:0.5,first:{type:'tab',tabId:'local'},second:{type:'tab',tabId:'b'}}}],tabs:{local:{session:'local'},b:{session:'b'}}};
assert.equal(dock.adopt(state),true);assert.equal(dock.adopt(state),false);assert.deepEqual(dock.visibleTabs('local'),['local','b']);assert.equal(dock.stripEntries()[0].target,'term:local');
global.showView=key=>{global.activeView=key;if(key.startsWith('term:'))global.activeTerm=key.slice(5);dock.render(global.activeTerm)};
dock.render('local');const tree=area.querySelector('.ws-tree');assert.ok(tree);const initialTree=tree.firstElementChild;const initialFrames=frames.map(f=>f.parentNode);assert.deepEqual(frames.map(f=>f.parentNode),initialFrames);
assert.equal(tree.style.height,'600px');assert.equal(tree.firstElementChild.style.gridTemplateColumns,'minmax(0,0.5fr) 6px minmax(0,0.5fr)');
area.clientWidth=390;dock.layout();assert.equal(tree.style.height,'880px');let divider=doc.querySelector('.ws-divider');assert.equal(divider.getAttribute('aria-orientation'),'horizontal');assert.equal(divider.hasAttribute('data-stacked'),true);area.clientWidth=1000;global.activeTerm='absent';dock.layout(area,tree,state.groups[0]);assert.equal(divider.hasAttribute('data-stacked'),false);global.activeTerm='local';
function dispatch(t,target,extra={}){doc.dispatch(t,{target,pointerId:1,button:0,pointerType:'mouse',clientX:500,clientY:100,stopImmediatePropagation(){},...extra});}
function ratio(){return dock.doc.groups[0].tree.ratio;}
function flushTimer(ms){for(const [id,t]of [...timers])if(t.ms===ms){timers.delete(id);t.f();}}
const tick=()=>new Promise(setImmediate);
(async()=>{
 // Drag resize and cancellation retain original doc and frame parent.
 dispatch('pointerdown',divider);assert.equal(dock.adopt({...state,revision:99}),false);dispatch('pointermove',divider,{clientX:700});assert.equal(ratio(),0.7);dispatch('pointercancel',divider);assert.equal(ratio(),0.5);assert.equal(requests.length,0);
 // Keyboard debounce batches two changes and refuses late external adoption.
 dispatch('keydown',divider,{key:'ArrowRight'});dispatch('keydown',divider,{key:'ArrowRight'});assert.ok(Math.abs(ratio()-0.6)<1e-9);assert.equal([...timers.values()].filter(t=>t.ms===400).length,1);flushTimer(400);await tick();assert.equal(requests.length,1);assert.equal(requests[0].body.expectedRevision,4);assert.equal(requests[0].opt.headers['X-Comandos-Token'],'fixture-token');assert.equal(dock.adopt({...state,revision:20}),false);
 pending.shift()({status:409,json:async()=>({current:{...state,revision:5}})});await tick();assert.equal(dock.revision,5);assert.equal(ratio(),0.5);assert.match(toasts.at(-1)[0],/Otro dispositivo/);
 // Touch/pen movement before the hold is scrolling; held gestures cancel cleanly.
 let handle=doc.querySelector('.ws-leaf[data-tab="b"] header');dispatch('pointerdown',handle,{pointerType:'touch',clientX:30,clientY:20});dispatch('pointermove',handle,{pointerType:'touch',clientX:50,clientY:20});flushTimer(180);assert.equal(doc.body.classList.contains('ws-dragging'),false);
 dispatch('pointerdown',handle,{pointerType:'pen',clientX:30,clientY:20});flushTimer(180);assert.equal(doc.body.classList.contains('ws-dragging'),true);dispatch('keydown',handle,{key:'Escape'});assert.equal(doc.body.classList.contains('ws-dragging'),false);assert.equal(doc.querySelector('.ws-ghost'),null);assert.equal(rafs.size,0);
 // Trays keep marks on native session and mark decoration after successful save.
 const marks=[];let decorated=0;global.WorkMarks={setMark:(...a)=>{marks.push(a);return Promise.resolve()},decorate:()=>decorated++};dispatch('pointerdown',handle,{clientX:30,clientY:20});dispatch('pointermove',handle,{clientX:60,clientY:170});assert.ok(doc.querySelector('.ws-trays'));dispatch('pointerup',handle,{clientX:60,clientY:170});await tick();assert.deepEqual(marks,[['session','b','frozen']]);assert.equal(decorated,1);assert.equal(requests.length,1);
 // Ratio updates reuse tree DOM, and frame positioning never reparents terminal.
 assert.equal(tree.firstElementChild,initialTree);assert.deepEqual(frames.map(f=>f.parentNode),initialFrames);assert.equal(doc.querySelector('.ws-preview'),null);let suppressed=false;dispatch('click',handle,{preventDefault(){suppressed=true;}});assert.equal(suppressed,true);

 // Edge RAF scroll is frame driven and stops on cancellation.
 dispatch('pointerdown',handle,{clientX:30,clientY:20});dispatch('pointermove',handle,{clientX:995,clientY:20});const [frameId,frameFn]=[...rafs][0];rafs.delete(frameId);frameFn();assert.equal(strip.scrollLeft,geometry.edgeScroll(995,strip.getBoundingClientRect()));assert.equal(doc.querySelector('.ws-preview').classList.contains('bar'),true);dispatch('pointercancel',handle);assert.equal(rafs.size,0);
 // Detaching into the strip sends the whole layout and restores it on failure.
 dispatch('pointerdown',handle,{clientX:30,clientY:20});dispatch('pointermove',handle,{clientX:300,clientY:20});dispatch('pointerup',handle);await tick();assert.equal(requests.length,2);assert.equal(requests[1].body.document.groups.length,2);assert.equal(requests[1].body.expectedRevision,5);pending.shift()({status:500,json:async()=>({error:'fixture refused'})});await tick();assert.equal(dock.doc.groups.length,1);assert.equal(dock.revision,5);assert.equal(toasts.at(-1)[0],'fixture refused');
 // Outer docking commits beside the whole group, then adopts server revision.
 handle=doc.querySelector('.ws-leaf[data-tab="b"] header');dispatch('pointerdown',handle,{clientX:30,clientY:20});dispatch('pointermove',handle,{clientX:2,clientY:300});assert.match(doc.querySelector('.ws-preview').textContent,/Dividir todo/);dispatch('pointerup',handle);await tick();assert.equal(requests.length,3);const next=requests[2].body.document;assert.equal(next.groups[0].tree.first.tabId,'b');pending.shift()({status:200,json:async()=>({...next,revision:6})});await tick();assert.equal(dock.revision,6);assert.deepEqual(frames.map(f=>f.parentNode),initialFrames);
 console.log('workspace dock actual WASM DOM contracts passed');
})().catch(e=>{console.error(e);process.exitCode=1;});
