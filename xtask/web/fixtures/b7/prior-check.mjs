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
globalThis.document={visibilityState:'visible',readyState:'loading',querySelector(){return {getAttribute(){return 'ui-sounds quick-terminal device-drafts session-config workspace-layout push-settings';}}},addEventListener(){},removeEventListener(){}};
globalThis.addEventListener=()=>{};globalThis.removeEventListener=()=>{};
Object.defineProperty(globalThis,'navigator',{value:{userActivation:{isActive:true}}});
globalThis.localStorage={getItem(){return null},setItem(){}};
globalThis.fetch=async url=>({ok:true,json:async()=>JSON.parse(fs.readFileSync(path.join(repo,String(url)),'utf8'))});
const wasm=await import(pathToFileURL(path.join(temp,'comandos_web.js')));await wasm.default({module_or_path:fs.readFileSync(path.join(temp,'comandos_web_bg.wasm'))});wasm.boot('private-b7');await tick();
try {
 const {runBehavior}=await import('../sounds/behavior.mjs');
 const actual=await runBehavior();const expected=JSON.parse(fs.readFileSync(path.join(repo,'xtask/web/fixtures/sounds/oracle.json'),'utf8'));
 assert.deepEqual(actual,expected);console.log('Actual WASM regression: all 44 prior behavioral groups match original oracle');
} finally {fs.rmSync(temp,{recursive:true,force:true});}
