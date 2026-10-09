// Runs the installed WASM controller in Node; no browser and no real service.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import {pathToFileURL, fileURLToPath} from 'node:url';
import {createRequire} from 'node:module';
const artifacts=process.argv[2];
if(!artifacts)throw Error('Pass the built comandos-web artifact directory');
const temp=fs.mkdtempSync(path.join(os.tmpdir(),'comandos-notice-switches-'));
try {
for(const name of fs.readdirSync(artifacts))if(name.endsWith('.js')||name.endsWith('.wasm'))fs.copyFileSync(path.join(artifacts,name),path.join(temp,name));
fs.writeFileSync(path.join(temp,'package.json'),'{"type":"module"}');
globalThis.Window=class {static [Symbol.hasInstance](v){return v===globalThis;}};
globalThis.window=globalThis;
globalThis.document={visibilityState:'visible',readyState:'loading',querySelector(){return {getAttribute(){return 'notifications';}}},addEventListener(){},removeEventListener(){}};
globalThis.addEventListener=()=>{};globalThis.removeEventListener=()=>{};
Object.defineProperty(globalThis,'navigator',{value:{userActivation:{isActive:true}}});
globalThis.localStorage={getItem(){return null},setItem(){}};
const wasm=await import(pathToFileURL(path.join(temp,'comandos_web.js')));
await wasm.default({module_or_path:fs.readFileSync(path.join(temp,'comandos_web_bg.wasm'))});wasm.boot('notice-switches-test');
const N=globalThis.ComandosNotices;
assert.equal(typeof N?.createController,'function');
let prefs={muted:false,modes:{done:'visual',attention:'sound'}},events=[],polls=0;
const controller=N.createController({setTimeout:()=>1,clearTimeout:()=>{},transport:async(method,url,body)=>{
 if(url==='/notices/prefs'){prefs={...prefs,...body};return prefs;}
 if(url.startsWith('/notices?')){polls++;const after=Number(new URL('http://fixture'+url).searchParams.get('after'));return {notices:events.filter(n=>n.sequence>after),prefs,pending:[],badge:events.length};}
 return {play:false};
}});
function arrival(category='done'){const seq=events.length+1;events.push({eventId:'n'+seq,sequence:seq,category,kind:category==='done'?'turn_completed':'permission_requested',read:false,float:{show:true,ms:6000}});}
await controller.poll();arrival();await controller.poll();assert.ok(controller.state.float,'normal arrival floats');
await controller.setPrefs({muted:true});assert.equal(controller.state.float,null,'muting removes an already visible card');
arrival();await controller.poll();assert.equal(controller.state.float,null,'muted arrivals cannot float');
assert.equal(controller.state.notices.length,2,'muted history retained');
await controller.setPrefs({muted:false,disabledCategories:['done']});
arrival();await controller.poll();assert.equal(controller.state.float,null,'done toggle suppresses that category');
arrival('attention');await controller.poll();assert.ok(controller.state.float,'other category still works');
prefs={...prefs,disabledCategories:['done','attention']};const before=polls;
controller.applyWatch({rev:'switches-changed',latest:events.length});
await new Promise(r=>setImmediate(r));await new Promise(r=>setImmediate(r));
assert.ok(polls>before,'preference revision fetches even with no new event');
assert.equal(controller.state.float,null,'remote switch change removes visible card');
assert.equal(controller.state.notices.length,4,'all history retained');
controller.dispose();console.log('PASS: mute, category switches, remote changes, and history preservation');
// Existing notification UI contracts, injected with the actual Rust/WASM owner.
const repo=fileURLToPath(new URL('../../../',import.meta.url));
const suite=path.join(repo,'tests/notification_ui_checks.cjs'),require=createRequire(suite);
const source=fs.readFileSync(suite,'utf8').replace(/^#!.*\n/,'').replace('(async () => {','return (async () => {');
const injected=target=>require.resolve(target)===path.join(repo,'dash/notifications.js')?N:require(target);
const processCheck={exit(code){if(code)throw Error('Notification UI suite failed: '+code);},set exitCode(code){if(code)throw Error('Notification UI suite failed: '+code);}};
await new Function('require','__dirname','console','process',source)(injected,path.dirname(suite),console,processCheck);
} finally {fs.rmSync(temp,{recursive:true,force:true});}
