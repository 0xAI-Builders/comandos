import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import {fileURLToPath,pathToFileURL} from 'node:url';
const repo=fileURLToPath(new URL('../../../../',import.meta.url));
const artifacts=process.argv[2];
if(!artifacts) throw new Error('Pass the actual built artifact directory');
const temp=fs.mkdtempSync(path.join(os.tmpdir(),'comandos-web-edge-'));
for(const name of fs.readdirSync(artifacts)) if(name.endsWith('.js')||name.endsWith('.wasm')) fs.copyFileSync(path.join(artifacts,name),path.join(temp,name));
fs.writeFileSync(path.join(temp,'package.json'),JSON.stringify({type:'module'}));
fs.copyFileSync(path.join(repo,'assets/uisfx/uisfx-0.4.0.js'),path.join(temp,'uisfx.js'));
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url);
const tick=()=>new Promise(r=>setImmediate(r));
globalThis.Window=class {static [Symbol.hasInstance](v){return v===globalThis;}};
globalThis.window=globalThis;
globalThis.document={visibilityState:'visible',readyState:'loading',querySelector(){return {getAttribute(){return 'ui-sounds work-marks pomodoro analytics-render analytics notifications';}}},addEventListener(){},removeEventListener(){}};
globalThis.addEventListener=()=>{};globalThis.removeEventListener=()=>{};
Object.defineProperty(globalThis,'navigator',{value:{userActivation:{isActive:true}}});
globalThis.localStorage={getItem(){return null},setItem(){}};
globalThis.fetch=async()=>({ok:true});
const wasm=await import(pathToFileURL(path.join(temp,'comandos_web.js')));await wasm.default({module_or_path:fs.readFileSync(path.join(temp,'comandos_web_bg.wasm'))});wasm.boot('private-b7');await tick();
try {
const W=require(repo+'/dash/work-marks.js'),A=require(repo+'/dash/analytics.js'),N=require(repo+'/dash/notifications.js');const results=[];
function save(name,actual,expected){results.push({name,equal:JSON.stringify(actual)===JSON.stringify(expected),actual,expected});}
const texts=['normal 🍅','high\ud800end','low\udc00end','collision\ue000\udb80\udc00end','markers\ue000\ue000\udb80\udc00end'];
const oracleRenderer=require(repo+'/dash/analytics-render.js');const boundary=JSON.parse(fs.readFileSync(repo+'/tests/fixtures/analytics/week-normal.json','utf8'));boundary.sessions=['4294967295','01','4294967294','0','-0','1.0','0003','3'].map((proj,i)=>({...boundary.sessions[0],proj,st:i*2,en:i*2+1,tok:100}));
for(const tab of ['cuentas','comparar'])save('numeric index boundary '+tab,AnalyticsRender.create(boundary,{}).html(tab),oracleRenderer.create(boundary,{}).html(tab));
for(const text of texts){
 const panes=[{session:text,paneId:'%1',paneKey:text+':pane',title:text}];
 save('workmarks row identity '+JSON.stringify(text),WorkMarks.targetForRow(text+'|%1',panes),W.targetForRow(text+'|%1',panes));
 save('workmarks label '+JSON.stringify(text),WorkMarks.label(text,0),W.label(text,0));
 const prefs={custom:text,[text]:text};save('notice keys '+JSON.stringify(text),ComandosNotices.normalizePrefs(prefs),N.normalizePrefs(prefs));
}
globalThis.innerWidth=900;globalThis.innerHeight=800;
function popFixture(){const listeners={};const pop={innerHTML:'',hidden:true,offsetWidth:50,offsetHeight:30,style:{setProperty(){}}};const el={querySelector:s=>s==='.pop'?pop:null,querySelectorAll:()=>[],addEventListener:(name,fn)=>listeners[name]=fn,classList:{toggle(){}}};return{el,pop,listeners};}
for(const text of texts){
 const btn={dataset:{pop:encodeURIComponent(JSON.stringify([[text,text,text,'red']])),title:text},getBoundingClientRect:()=>({left:0,bottom:10}),closest(sel){return sel==='[data-pop]'?this:null}};
 let actual,expected;
 for(const [label,api]of[['native',Analytics],['original',A]]){const f=popFixture();api.create(f.el,{fetchWeek:async()=>null,width:()=>900});f.listeners.click({target:btn,stopPropagation(){}});if(label==='native')actual=f.pop.innerHTML;else expected=f.pop.innerHTML;}
 save('analytics popover '+JSON.stringify(text),actual,expected);
}


for(const text of texts){
 const out=[];
 for(const api of [Analytics,A]){
  const listeners={},tip={innerHTML:'',hidden:true},el={querySelector:sel=>sel==='.tip'?tip:null,querySelectorAll:()=>[],addEventListener:(name,fn)=>listeners[name]=fn,classList:{toggle(){}}};
  api.create(el,{fetchWeek:async()=>null,width:()=>900});
  const src={dataset:{tip:[text,text,text].join('|')},closest:()=>src};
  listeners.pointerover({target:src});out.push({html:tip.innerHTML,hidden:tip.hidden});
 }
 save('analytics tooltip delegated handler '+JSON.stringify(text),out[0],out[1]);
}
// Actual adopted map, row decoration, suggestion listener and POST boundary.
function marksFixture(text){
 const listeners={};let extra=null;const name={};
 const button={dataset:{},classList:{toggle(){}},setAttribute(k,v){this[k]=v;}};
 const host={querySelector(sel){return sel===':scope > .wm-ind'?button:sel===':scope > .wm-extra'?extra:sel==='.name'?name:null},insertBefore(el){extra=el;el.parentNode=this;}};
 button.parentNode=host;button._wmBefore=name;name.parentNode=host;
 const row={dataset:{rk:text+'|%1'},querySelector:sel=>sel==='.ident'?host:null};
 return {button,listeners,get extra(){return extra},doc:{...document,querySelectorAll:sel=>sel==='.row[data-rk]'?[row]:[],createElement(){return{innerHTML:'',addEventListener:(name,fn)=>listeners[name]=fn}}}};
}
const originalDocument=document,originalFetch=fetch;
for(const text of texts){
 const key=text+':pane',mark={scope:'pane',key,mark:'none',favorite:false,revision:17};
 save('workmarks indexed JS key '+JSON.stringify(text),WorkMarks.indexMarks([mark]).get('pane\0'+key),W.indexMarks([mark]).get('pane\0'+key));
 save('workmarks icon class '+JSON.stringify(text),WorkMarks.iconSvg(text,16),W.iconSvg(text,16));
 const out=[];
 for(const api of [WorkMarks,W]){
  const f=marksFixture(text),requests=[];globalThis.document=f.doc;
  globalThis.fetch=async(path,opts)=>{const body=JSON.parse(opts.body);requests.push({path,body});return {status:200,json:async()=>({mark:{...mark,mark:body.value,revision:18}})};};
  api.adopt({marks:[mark],panes:[{session:text,paneId:'%1',paneKey:key,title:text}],activity:{['pane:'+key]:{state:'completed'}}});
  const target=structuredClone(f.button._wmTarget);
  assert.equal(typeof f.listeners.click,'function','production suggestion listener registered');
  assert.ok(f.extra.innerHTML.includes('data-wm-suggest'));
  f.listeners.click({target:{closest:()=>({})},stopPropagation(){},preventDefault(){}});
  await tick();await tick();
  out.push({target,requests,extra:f.extra.innerHTML});
 }
 save('workmarks decoration suggestion POST '+JSON.stringify(text),out[0],out[1]);
 globalThis.document=originalDocument;globalThis.fetch=originalFetch;
}
// Native client send error text and real notices controller group-read requests.
const P=require(repo+'/dash/pomodoro.js');
for(const text of texts){
 const errors=[];
 for(const api of [ComandosPomodoro,P]){
  const client=api.createClient({transport:async()=>({status:400,body:{code:text,error:text}}),now:()=>2000,requestId:()=> 'fixed-private'});
  errors.push((await client.send('start',{project:text})).error);
 }
 save('pomodoro send error identity '+JSON.stringify(text),errors[0],errors[1]);
 const reads=[];
 for(const api of [ComandosNotices,N]){
  const requests=[],notice={eventId:text,project:text,title:text,excerpt:text,sequence:1,occurredAtMs:1000,read:false,category:'error'};
  const c=api.createController({now:()=>2000,transport:async(method,path,body)=>{
   if(method==='POST')requests.push({method,path,body});
   if(path.startsWith('/notices?'))return {notices:[notice],pending:[],badge:1};
   if(path==='/notifs/count')return {count:0};return {read:[text]};
  }});
  await c.poll();await c.markGroupRead(text);await tick();reads.push(requests);c.dispose?.();
 }
 save('notice group read identity '+JSON.stringify(text),reads[0],reads[1]);
 {
  const watches=[];
  for(const api of [ComandosNotices,N]){
   const paths=[],c=api.createController({transport:async(method,path)=>{paths.push(path);return {};}});
   c.applyWatch({rev:text});let error=null;try{await c.watchOnce(25)}catch(e){error={name:e.name,message:e.message}}c.dispose?.();watches.push({paths,error});
  }
  save('notice watch revision identity '+JSON.stringify(text),watches[0],watches[1]);
 }
}
console.log(JSON.stringify(results.map(r=>({name:r.name,equal:r.equal})),null,2));assert.ok(results.every(r=>r.equal),'all production consumer closures must preserve original UTF16');

} finally {fs.rmSync(temp,{recursive:true,force:true});}
