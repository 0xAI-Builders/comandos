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
fs.copyFileSync(path.join(repo,'dash/work-marks.js'),path.join(temp,'marks.cjs'));
fs.copyFileSync(path.join(repo,'dash/pomodoro.js'),path.join(temp,'pomo.cjs'));
fs.copyFileSync(path.join(repo,'assets/uisfx/uisfx-0.4.0.js'),path.join(temp,'uisfx.js'));
const sources=[];
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url);
const tick=()=>new Promise(r=>setImmediate(r));
globalThis.Window=class {static [Symbol.hasInstance](v){return v===globalThis;}};
globalThis.window=globalThis;
globalThis.document={visibilityState:'visible',readyState:'loading',querySelector(){return {getAttribute(){return 'ui-sounds work-marks pomodoro';}}},addEventListener(){},removeEventListener(){}};
globalThis.addEventListener=()=>{};globalThis.removeEventListener=()=>{};
Object.defineProperty(globalThis,'navigator',{value:{userActivation:{isActive:true}}});
globalThis.localStorage={getItem(){return null},setItem(){}};
globalThis.fetch=async()=>({ok:true});
const wasm=await import(pathToFileURL(path.join(temp,'comandos_web.js')));await wasm.default({module_or_path:fs.readFileSync(path.join(temp,'comandos_web_bg.wasm'))});wasm.boot('private-b6');await tick();
const originalMarks=require(path.join(temp,'marks.cjs'));const originalPomo=require(path.join(temp,'pomo.cjs'));
const results=[];
function check(name,fn){fn();results.push({name,ok:true});}
try {
 assert.equal(typeof globalThis.WorkMarks?.menuItems,'function','actual WASM must expose executable marks methods');
 assert.equal(typeof globalThis.ComandosPomodoro?.createClient,'function','actual WASM must expose executable Pomodoro state machine');
 const M=WorkMarks,P=ComandosPomodoro;
 if(process.argv.includes('--negative'))P.elapsedMs=()=>0;
 const panes=[{paneKey:'pk1',session:'s',paneId:'%1'},{paneKey:'pk2',session:'s',paneId:'%2'},{paneKey:'dupA',session:'t',paneId:'%9'},{paneKey:'dupB',session:'t',paneId:'%9'}];
 const activity={'pane:pk1':{state:'working',session:'s'},'tmux:s:%2':{state:'completed',session:'s'}};
 check('human mark wins; finished turns neutral and two channels remain independent',()=>{for(const mark of [...originalMarks.MARKS,'bogus'])for(const state of ['working','completed','cancelled','failed','awaiting_permission',null,undefined]){assert.deepEqual(M.display(mark,state),originalMarks.display(mark,state));assert.deepEqual(M.channels(mark,state),originalMarks.channels(mark,state));}});
 check('fixed SVG icons, AI colors, pulse and sizes',()=>{for(const name of [...originalMarks.MARKS,'working','favorite'])assert.equal(M.iconSvg(name),originalMarks.iconSvg(name));for(const name of [...originalMarks.AI_STATES,'bogus']){assert.equal(M.aiCycle(name),originalMarks.aiCycle(name));for(const size of [undefined,16])assert.equal(M.aiIconSvg(name,size),originalMarks.aiIconSvg(name,size));}assert.deepEqual(M.AI_COLORS,originalMarks.AI_COLORS);});
 check('row binding never guesses duplicate panes and activity urgency is retained',()=>{for(const rk of ['s|%2','s','t|%9',''])assert.deepEqual(M.targetForRow(rk,panes),originalMarks.targetForRow(rk,panes));for(const target of [{scope:'pane',key:'pk1',session:'s',paneId:'%1'},{scope:'pane',key:'pk2',session:'s',paneId:'%2'},{scope:'session',key:'s'},{scope:'session',key:'other'},null])assert.deepEqual(M.activityFor(target,activity),originalMarks.activityFor(target,activity));assert.equal(M.activityFor({scope:'session',key:'s'},{a:{session:'s',state:'completed'},b:{session:'s',state:'awaiting_input'}}),'awaiting_input');});
 check('four marks plus independent scope favorite and wrapping keyboard',()=>{for(const scope of ['pane','session'])assert.deepEqual(M.menuItems(scope,{mark:'awaiting_reply',favorite:true},false),originalMarks.menuItems(scope,{mark:'awaiting_reply',favorite:true},false));for(const key of ['ArrowDown','ArrowUp','Home','End','x'])for(const index of [-1,0,4])assert.equal(M.nextIndex(key,index,5),originalMarks.nextIndex(key,index,5));const rows=[{scope:'pane',key:'a',mark:'frozen'}];assert.equal(M.indexMarks(rows).get('pane\0a'),rows[0]);});
 check('six styles and vendored asset metadata/HTML retain all original frames',()=>{assert.deepEqual(P.STYLES,originalPomo.STYLES);assert.deepEqual(P.STYLE_ORDER,originalPomo.STYLE_ORDER);assert.deepEqual(P.ART_SOURCES,originalPomo.ART_SOURCES);assert.deepEqual(P.artFiles(),originalPomo.artFiles());for(const style of [...originalPomo.STYLE_ORDER,'nope']){assert.equal(P.styleOf(style),originalPomo.styleOf(style));for(const role of ['clock','crystal','first','hundred','streak','level'])assert.equal(P.assetHtml(role,style),originalPomo.assetHtml(role,style));}});
 check('server time math, capped ruler delta and hourglass cycle',()=>{for(const status of ['running','paused','completed'])for(const now of [0,600000,9999999]){const b={status,targetMs:1500000,activeMs:0,resumedAtMs:0};assert.equal(P.elapsedMs(b,now),originalPomo.elapsedMs(b,now));assert.equal(P.remainingMs(b,now),originalPomo.remainingMs(b,now));for(const m of [5,30,500])assert.equal(P.deltaForRemaining(b,now,m),originalPomo.deltaForRemaining(b,now,m));}for(const now of [0,3500,7350,7460,7900,8010,-1])for(const flip of [null,1000])assert.equal(P.hourglassFrame(null,now,flip),originalPomo.hourglassFrame(null,now,flip));for(const ms of [-1,0,1,61000,9999999])assert.equal(P.fmt(ms),originalPomo.fmt(ms));});
 check('sound route describes the selected device',()=>{for(const device of ['desktop','local-speaker','me','other',null])for(const on of [true,false])for(const enabled of [true,false]){const sound={enabled,device,desktopDevice:'desktop'};assert.deepEqual(P.soundWhere(sound,'me',on),originalPomo.soundWhere(sound,'me',on));}});
 let reply={status:200,body:{marks:[{scope:'pane',key:'pk1',mark:'frozen',favorite:false,revision:3}],panes,activity:{}}};const calls=[];globalThis.fetch=async(url,opt)=>{calls.push({url,body:opt?.body?JSON.parse(opt.body):null});return {status:reply.status,json:async()=>reply.body};};
 await M.load();reply={status:409,body:{current:{scope:'pane',key:'pk1',mark:'resolved',favorite:true,revision:4}}};await assert.rejects(M.setMark('pane','pk1','none'));assert.equal(calls.at(-1).body.expectedRevision,3);reply={status:200,body:{mark:{scope:'pane',key:'pk1',mark:'none',favorite:true,revision:5}}};assert.equal((await M.setMark('pane','pk1','none')).revision,5);assert.equal(calls.at(-1).body.expectedRevision,4);results.push({name:'stale mark adopts revision and next write uses confirmed state',ok:true});
 const vm=await import('node:vm');
 // Run the supplied sync oracle unchanged except its imported client. It talks
 // only to the original authority in a temporary database, never live state.
 const filename=path.join(repo,'tests/pomodoro_sync_checks.cjs');let source=fs.readFileSync(filename,'utf8').replace("const P = require(path.join(ROOT, 'dash', 'pomodoro.js'));","const P = candidate;");
 let exit;await new Promise((resolve,reject)=>{const context={candidate:P,require:createRequire(filename),__dirname:path.dirname(filename),console:{log:(...args)=>{const output=args.join(' ');console.log(output);if(output.includes('pomodoro sync checks passed'))resolve();},error:reject},process:{set exitCode(value){exit=value;},get exitCode(){return exit;}},setImmediate,structuredClone};vm.runInNewContext(source,context,{filename});});assert.equal(exit||0,0);results.push({name:'all9 supplied original server authority sync checks',ok:true});
 console.log(JSON.stringify({status:'pass',artifact:artifacts,results},null,2));
} finally { fs.rmSync(temp,{recursive:true,force:true}); }
