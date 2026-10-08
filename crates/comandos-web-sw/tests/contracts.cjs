const assert=require('node:assert/strict'), fs=require('node:fs'), vm=require('node:vm');
const wasm=require(process.argv[2]), original=fs.readFileSync(process.argv[3],'utf8');
function scope(native=false, windows=[]){
 const handlers={}, shown=[], opened=[], calls=[];
 const store=new Map([['/',{offline:'shell'}]]);
 const caches={open:async name=>{calls.push(['open',name]);return {addAll:async urls=>calls.push(['addAll',Array.from(urls)]),put:async(req,res)=>{store.set(req.url||req,res);calls.push(['put',req.url||req])}}},keys:async()=>['old-cache','comandos-shell-v13','comandos-native-shell-v1-old'],delete:async k=>calls.push(['delete',k]),match:async req=>store.get(req.url||req)};
 const self={location:{origin:'https://app.test'},addEventListener:(k,fn)=>handlers[k]=fn,skipWaiting:()=>calls.push(['skipWaiting']),registration:{showNotification:async(title,options)=>shown.push({title,options})},clients:{claim:()=>calls.push(['claim']),matchAll:async()=>windows,openWindow:async u=>opened.push(u)}};
 const fetch=async(req,opts)=>{calls.push(['fetch',req.url||req,opts]);if((req.url||req).includes('offline'))throw Error('offline');return {network:true,clone(){return {copy:true}}}};
 const ctx={self,caches,URL,fetch,console};
 return {ctx,handlers,shown,opened,calls,store,native};
}
function load(which,native=false,windows=[]){const s=scope(native,windows);if(which==='original')vm.runInNewContext(original,s.ctx);else{Object.assign(global,s.ctx,s.ctx.self);global.self=global;wasm.boot(native,['/?web=native','/web/123456789abc/module.wasm']);}return s;}
function event(extra={}){const waits=[], responses=[];return {...extra,waits,responses,waitUntil:p=>waits.push(p),respondWith:p=>responses.push(p)}}
async function flush(e){await Promise.all([...e.waits,...e.responses]);await new Promise(r=>setImmediate(r));}
async function run(which){
 const records=[];
 for(const payload of [{eventId:'ev-1',title:'Project',body:'Done',url:'https://evil.test/?token=x'},null,{eventId:'x'.repeat(129),tag:'comandos-test',title:0},{eventId:'😀'.repeat(64),title:'😀'.repeat(40),body:'😀'.repeat(80)},{eventId:'a\n',tag:'evil'}]){
  const s=load(which),e=event({data:{json:()=>payload}});s.handlers.push(e);await flush(e);records.push(s.shown);
 }
 const bad=load(which),e=event({data:{json(){throw Error('bad')}}});bad.handlers.push(e);await flush(e);records.push(bad.shown);
 for(const id of ['a b&c','\0evil','😀'.repeat(65)]){const s=load(which),e=event({notification:{close(){s.calls.push(['closed'])},data:{eventId:id}}});s.handlers.notificationclick(e);await flush(e);records.push([s.opened,s.calls]);}
 const messages=[],win={url:'https://app.test/term/?draft=x',focus:async()=>messages.push('focus'),postMessage:m=>messages.push(m),navigate(){throw Error('reload')}};
 const click=load(which,false,[{url:'bad url'},{url:'https://foreign.test'},win]);const ce=event({notification:{close(){},data:{eventId:'ok'}}});click.handlers.notificationclick(ce);await flush(ce);records.push(messages);
 const s=load(which);assert.deepEqual(Object.keys(s.handlers).sort(),['activate','fetch','install','notificationclick','push']);
 const install=event();s.handlers.install(install);await flush(install);const activate=event();s.handlers.activate(activate);await flush(activate);records.push(s.calls);
 for(const [url,method] of [['https://app.test/','GET'],['https://app.test/?token=secret','GET'],['https://app.test/offline.css','GET'],['https://app.test/operator/x.js','GET'],['https://app.test/new-api','GET'],['https://else.test/x.js','GET'],['https://app.test/x.css','POST']]){const s=load(which),e=event({request:{url,method}});s.handlers.fetch(e);await flush(e);records.push([e.responses.length,s.calls,Array.from(s.store.keys())]);}
 return JSON.parse(JSON.stringify(records));
}
(async()=>{const expected=await run('original');const actual=await run('rust');assert.deepEqual(actual,expected);console.log('PASS original worker parity: 5 handlers, 6 push payloads, 4 click cases, install/activate and 7 fetch cases');const n=load('rust',true);const e=event();n.handlers.install(e);await flush(e);assert(n.calls.some(x=>x[0]==='addAll'&&x[1].includes('/?web=native')&&x[1].some(y=>y.endsWith('.wasm'))));const a=event();n.handlers.activate(a);await flush(a);assert(!n.calls.some(x=>x[0]==='delete'&&x[1]==='comandos-shell-v13'));console.log('PASS native precache includes compiled shell/WASM and preserves legacy cache');
const nativeClick=load('rust',true),click=event({notification:{close(){},data:{eventId:'a b'}}});nativeClick.handlers.notificationclick(click);await flush(click);assert.deepEqual(nativeClick.opened,['https://app.test/?web=native&event=a%20b']);
for(const [url,wanted] of [['https://app.test/web/123456789abc/module.wasm',1],['https://app.test/web/123456789abc/new-api.wasm',0]]) { const sw=load('rust',true),fe=event({request:{url,method:'GET'}});sw.handlers.fetch(fe);await flush(fe);assert.equal(fe.responses.length,wanted);await Promise.all(fe.waits); }
console.log('PASS native click retains mode and only precached WASM is intercepted');})().catch(e=>{console.error(e);process.exitCode=1});
