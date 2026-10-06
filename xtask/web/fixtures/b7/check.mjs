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
globalThis.document={visibilityState:'visible',readyState:'loading',querySelector(){return {getAttribute(){return 'ui-sounds analytics-render analytics notifications';}}},addEventListener(){},removeEventListener(){}};
globalThis.addEventListener=()=>{};globalThis.removeEventListener=()=>{};
Object.defineProperty(globalThis,'navigator',{value:{userActivation:{isActive:true}}});
globalThis.localStorage={getItem(){return null},setItem(){}};
globalThis.fetch=async()=>({ok:true});
const wasm=await import(pathToFileURL(path.join(temp,'comandos_web.js')));await wasm.default({module_or_path:fs.readFileSync(path.join(temp,'comandos_web_bg.wasm'))});wasm.boot('private-b7');await tick();
try {
const modules={render:globalThis.AnalyticsRender,analytics:globalThis.Analytics,notices:globalThis.ComandosNotices};
assert.equal(typeof modules.render?.create,'function','actual WASM must render the analytics model');
assert.equal(typeof modules.analytics?.create,'function','actual WASM must handle analytics controls');
assert.equal(typeof modules.notices?.createController,'function','actual WASM must own notices state/long-poll');
if(process.argv.includes('--negative')) modules.render.create=()=>({html:()=>'',phoneDays:()=>0});
const suiteResults=[];
for(const name of ['analytics_parity_checks.cjs','analytics_ui_checks.cjs','analytics_agy_checks.cjs','notification_ui_checks.cjs']) {
 const file=path.join(repo,'tests',name),baseRequire=createRequire(file);let source=fs.readFileSync(file,'utf8').replace(/^#!.*\n/,'').replace('(async () => {','return (async () => {');
 const customRequire=target=>{const full=baseRequire.resolve(target);if(full===path.join(repo,'dash/analytics-render.js'))return modules.render;if(full===path.join(repo,'dash/analytics.js'))return modules.analytics;if(full===path.join(repo,'dash/notifications.js'))return modules.notices;return baseRequire(target)};
 let failed=0;const proc={exit:code=>{failed=code;if(code)throw new Error(name+' failed with exit '+code)},set exitCode(code){failed=code}};
 const output={log:(...args)=>console.log(...args),error:(...args)=>console.error(...args)};
 await new Function('require','__dirname','console','process',source)(customRequire,path.dirname(file),output,proc);
 assert.equal(failed,0,name+' must complete successfully');suiteResults.push(name);
}
const originalRender=require(path.join(repo,'dash/analytics-render.js'));
for(const hours of [1.15,1.25]){
 const model=JSON.parse(fs.readFileSync(path.join(repo,'tests/fixtures/analytics/week-normal.json'),'utf8'));model.sessions=[{...model.sessions[0],st:0,en:hours,tok:hours*1000}];model.accounts[0].hoy.h=hours;
 const actual=modules.render.create(model,{}).html('cuentas'),expected=originalRender.create(model,{}).html('cuentas');
 if(actual!==expected){let i=0;while(actual[i]===expected[i])i++;throw new Error('binary rounding '+hours+' at '+i+' actual='+actual.slice(i-60,i+100)+' expected='+expected.slice(i-60,i+100));}
}
const legacy=require(path.join(repo,'dash/notifications.js'));
const rows=[{eventId:'pending',category:'attention',project:'A<&',title:'Permiso',excerpt:'Texto <&',sequence:3,occurredAtMs:2000000000000,read:true},{eventId:'error',category:'error',project:'A<&',title:'Error',excerpt:'Completo',sequence:2,occurredAtMs:2000000000000,read:false},{eventId:'news',category:'news',title:'Noticias',excerpt:'Edición',sequence:1,occurredAtMs:2000000000000,read:false}];
for(const state of [{},{loaded:false},{error:'sin red'},{collapsed:true},{filter:'pending'},{filter:'news'},{filter:'usage'},{unavailable:'error'}]){
 const view={notices:rows,pending:new Set(['pending']),loaded:true,now:2000000001000,...state};const actual=modules.notices.renderStrip(view),expected=legacy.renderStrip(view);if(actual!==expected){let i=0;while(actual[i]===expected[i])i++;throw new Error('complete notice markup '+JSON.stringify(state)+' at '+i+' actual='+actual.slice(i-80,i+160)+' expected='+expected.slice(i-80,i+160));}
}
for(const persistent of [false,true]){const view={notices:rows,float:{eventIds:['error','pending'],persistent},pending:new Set(['pending']),unavailable:persistent?'error':null};const actual=modules.notices.renderFloat(view),expected=legacy.renderFloat(view);if(actual!==expected){let i=0;while(actual[i]===expected[i])i++;throw new Error('complete float markup at '+i+' actual='+actual.slice(i-60,i+140)+' expected='+expected.slice(i-60,i+140));}}
for(const local of [false,true])assert.equal(modules.notices.renderSettings({prefs:{volume:.35,muted:true,modes:{info:'sound'}},localSound:local,localAvailable:local}),legacy.renderSettings({prefs:{volume:.35,muted:true,modes:{info:'sound'}},localSound:local,localAvailable:local}));
for(const N of [legacy,modules.notices])for(const text of ['x'.repeat(5000),'Aviso completo: '+ 'á<&🍅'.repeat(1300)]){
 const html=N.renderStrip({notices:[{eventId:'full',category:'info',title:'Texto',excerpt:text,sequence:1,read:false}],loaded:true});const {mkRoot}=require(path.join(repo,'tests/dom_stub.cjs'));const host=mkRoot();host.innerHTML=html;assert.equal(host.querySelector('.nt-full summary').textContent,'Ver TODO');assert.equal(host.querySelector('.nt-full p').textContent,text,'all5000+characters are in the disclosure DOM');
}
// A pending long poll has one owner and settles on dispose even if the
// injected transport does not observe AbortSignal. The installed browser
// transport also passes this owned signal to fetch.
let requests=0,signal;
const owner=modules.notices.createController({transport:(method,path,body,options)=>{requests++;signal=options?.signal;return new Promise(()=>{});}});
const first=owner.watchOnce(25);assert.equal(owner.watchOnce(25),first,'only one long poll is in flight');await tick();assert.equal(requests,1);owner.dispose();
await Promise.race([first,new Promise((_,reject)=>setTimeout(()=>reject(new Error('owned long poll did not settle on dispose')),100))]);assert.equal(signal?.aborted,true,'destroy aborts the fetch owner');
console.log(JSON.stringify({ownedLongPoll:true,suites:suiteResults,fullTextLegacyAndActualWasm:true,artifact:artifacts},null,2));
} finally {fs.rmSync(temp,{recursive:true,force:true});}
