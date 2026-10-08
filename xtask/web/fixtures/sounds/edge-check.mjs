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
fs.copyFileSync(path.join(repo,'dash/device-drafts.js'),path.join(temp,'drafts.cjs'));
fs.copyFileSync(path.join(repo,'assets/uisfx/uisfx-0.4.0.js'),path.join(temp,'uisfx.js'));
const sources=[];
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url);
const tick=()=>new Promise(r=>setImmediate(r));
globalThis.Window=class {static [Symbol.hasInstance](v){return v===globalThis;}};
globalThis.window=globalThis;
globalThis.document={visibilityState:'visible',readyState:'loading',querySelector(){return {getAttribute(){return 'ui-sounds device-drafts';}}},addEventListener(){},removeEventListener(){}};
globalThis.addEventListener=()=>{};globalThis.removeEventListener=()=>{};
Object.defineProperty(globalThis,'navigator',{value:{userActivation:{isActive:true}}});
globalThis.localStorage={getItem(){return null},setItem(){}};
globalThis.fetch=async()=>({ok:true});
class Param{value=1;cancelScheduledValues(){};setValueAtTime(){};linearRampToValueAtTime(){}}
class Node{gain=new Param();connect(){};disconnect(){}}
class AudioContext{
 state='running';sampleRate=48000;currentTime=0;destination={};
 createGain(){return new Node();}
 createBuffer(channels,length){return {getChannelData(){return new Float32Array(length);}};}
 createBufferSource(){const ctx=this;const source={playbackRate:{value:1},connect(){},disconnect(){},addEventListener(k,cb){this.ended=cb;},removeEventListener(k,cb){if(this.ended===cb)this.ended=null;},start(){},stop(){const src=this;setTimeout(()=>{if(ctx.state!=='closed')src.ended?.();},20);}};sources.push(source);return source;}
 resume(){return Promise.resolve();}
 close(){this.state='closed';return Promise.resolve();}
}
globalThis.AudioContext=AudioContext;
const original=require(path.join(temp,'drafts.cjs'));
const wasm=await import(pathToFileURL(path.join(temp,'comandos_web.js')));await wasm.default({module_or_path:fs.readFileSync(path.join(temp,'comandos_web_bg.wasm'))});wasm.boot('private-review');
await tick();
const results={ready:globalThis.__comandosReady,exports:[typeof ComandosDeviceDrafts,typeof uiSounds]};
const source='a'.repeat(299)+'😀 tail';
async function anchor(api){let patch;const a=api.createAnchor({key:'k',load:async()=>({readingAnchors:{k:patch.anchorsPatch.k}}),save:v=>{patch=v;return Promise.resolve()},schedule:fn=>{queueMicrotask(fn);return 1},cancel(){}});a.remember(source,0,0);await tick();return {savedLength:patch.anchorsPatch.k.text.length,lastCodeUnit:patch.anchorsPatch.k.text.charCodeAt(299),find:await a.find(source)};}
results.anchor={original:await anchor(original),candidate:await anchor(ComandosDeviceDrafts)};
const uisfx=await import(pathToFileURL(path.join(temp,'uisfx.js')));
const p=uisfx.createUISFX({pack:'arcade'});await p.unlock();const oh=p.play('complete');let originalEnded=false;oh.ended.then(()=>originalEnded=true);await p.destroy();await tick();
const candidateSourceStart=sources.length;
uiSounds.setEnabled(true);uiSounds.unlock({isTrusted:true});await tick();const normal=uiSounds.play('error');let naturalEnded=false;normal.ended.then(()=>naturalEnded=true);sources.at(-1).ended();await tick();
const ch=uiSounds.play('complete');const active=uiSounds.play('warning');let activeEnded=false;active.ended.then(()=>activeEnded=true);ch.stop();let candidateEnded=false;ch.ended.then(()=>candidateEnded=true);await uiSounds.dispose();await new Promise(r=>setTimeout(r,40));
results.dispose={originalEnded,candidateEnded,activeEnded,naturalEnded};
results.ownedListenersAfterDispose=sources.slice(candidateSourceStart).filter(s=>!!s.ended).length;
console.log(JSON.stringify(results,null,2));
try {
assert.equal(results.ready,true);
assert.deepEqual(results.anchor.candidate,results.anchor.original);
assert.equal(results.dispose.candidateEnded,true);
assert.equal(results.dispose.activeEnded,true);
assert.equal(results.dispose.naturalEnded,true);
assert.equal(results.ownedListenersAfterDispose,0);
} finally { fs.rmSync(temp,{recursive:true,force:true}); }
