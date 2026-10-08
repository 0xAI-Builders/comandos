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
const N=require(repo+'/dash/notifications.js'),A=require(repo+'/dash/analytics-render.js'),P=require(repo+'/dash/pomodoro.js');let results=[];
function save(name,actual,expected){let i=0;if(typeof actual==='string'&&typeof expected==='string')while(i<Math.max(actual.length,expected.length)&&actual[i]===expected[i])i++;results.push({name,equal:actual===expected,firstDifference:i,actualLength:actual?.length,expectedLength:expected?.length,actualContext:typeof actual==='string'?actual.slice(Math.max(0,i-100),i+180):actual,expectedContext:typeof expected==='string'?expected.slice(Math.max(0,i-100),i+180):expected});}
const base=JSON.parse(fs.readFileSync(repo+'/tests/fixtures/analytics/week-normal.json','utf8'));
for(const variant of ['tied-accounts','numeric-projects']){const m=structuredClone(base);m.sessions=[{...m.sessions[0],proj:variant==='numeric-projects'?'20':'Tie',acc:m.accounts[0].id,st:1,en:2,tok:100},{...m.sessions[0],proj:variant==='numeric-projects'?'3':'Tie',acc:m.accounts[1].id,st:3,en:4,tok:100}];for(const tab of ['cuentas','comparar'])save(variant+'-'+tab,AnalyticsRender.create(m,{}).html(tab),A.create(m,{}).html(tab));}
for(const label of ['normal','high-surrogate','low-surrogate']){const text=label==='normal'?'Normal á<&🍅':label==='high-surrogate'?'prefix\ud800suffix':'prefix\udc00suffix';const view={notices:[{eventId:'one',category:'error',project:'Demo',title:'Title',excerpt:text,sequence:1,occurredAtMs:1000,read:false}],loaded:true,pending:new Set(),now:2000};save('notice-'+label,ComandosNotices.renderStrip(view),N.renderStrip(view));}
const block={status:'running',targetMs:60000,activeMs:1000,resumedAtMs:1000,project:'x\ud800y'};save('pomo-UTF16-elapsed',ComandosPomodoro.elapsedMs(block,2000),P.elapsedMs(block,2000));
// Exercise every B6/B7 consumer of the explicitly selected lossless boundary.
const W=require(repo+'/dash/work-marks.js');
for(const text of ['prefix\ud800suffix','prefix\udc00suffix','paired 🍅','private \ue000\udb80\udc00']){
 const notice={eventId:'one',category:'error',project:text,title:text,excerpt:text,sequence:1,occurredAtMs:1000,read:false};
 for(const name of ['renderStrip','renderFloat']){
  const v={notices:[notice],pending:new Set(),loaded:true,now:2000,float:{eventIds:['one'],persistent:true},describe:()=>text};save(name+' UTF16 '+JSON.stringify(text),ComandosNotices[name](v),N[name](v));
 }
 assert.deepEqual(ComandosNotices.groupNotices([notice],'all',new Set()),N.groupNotices([notice],'all',new Set()));
 assert.deepEqual(ComandosNotices.normalizePrefs({[text]:text}),N.normalizePrefs({[text]:text}));
 const panes=[{session:'demo',paneId:'%1',paneKey:text,title:text}];assert.deepEqual(WorkMarks.targetForRow('demo|%1',panes),W.targetForRow('demo|%1',panes));
 const target={scope:'pane',key:text,session:'demo',paneId:'%1'},activity={['pane:'+text]:{state:'working',unrelated:text}};assert.equal(WorkMarks.activityFor(target,activity),W.activityFor(target,activity));
 const b={status:'running',targetMs:60000,activeMs:1000,resumedAtMs:1000,project:text};for(const method of ['elapsedMs','remainingMs','deltaForRemaining'])assert.equal(ComandosPomodoro[method](b,2000,2),P[method](b,2000,2));
 const m=structuredClone(base);m.sessions=[{...m.sessions[0],proj:text,st:1,en:2,tok:100}];m.accounts[0].alias=text;for(const tab of ['cuentas','comparar'])save('analytics UTF16 '+tab+' '+JSON.stringify(text),AnalyticsRender.create(m,{}).html(tab),A.create(m,{}).html(tab));
}
const boundary=structuredClone(base);boundary.sessions=['4294967295','01','4294967294','0'].map((proj,i)=>({...boundary.sessions[0],proj,st:i*2+1,en:i*2+2,tok:100}));for(const tab of ['cuentas','comparar'])save('numeric-boundaries-'+tab,AnalyticsRender.create(boundary,{}).html(tab),A.create(boundary,{}).html(tab));
let signal,calls=0;const owner=ComandosNotices.createController({transport:(method,p,b,opts)=>{calls++;signal=opts?.signal;return new Promise(()=>{})}});const first=owner.watchOnce(25);assert.equal(first,owner.watchOnce(25));owner.dispose();await Promise.race([first,new Promise((_,reject)=>setTimeout(()=>reject(Error('dispose pending')),100))]);results.push({name:'watch-owner-dispose',equal:calls===1&&signal.aborted,calls,aborted:signal.aborted});

console.log(JSON.stringify(results,null,2));assert.ok(results.every(row=>row.equal),'all independent review cases must match original');
} finally {fs.rmSync(temp,{recursive:true,force:true});}
