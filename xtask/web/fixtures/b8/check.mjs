// Original assertions, async adaptation only; real WASM and real native route.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import vm from 'node:vm';
import assert from 'node:assert/strict';
import {spawn,spawnSync} from 'node:child_process';
import {createRequire} from 'node:module';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {JSDOM} from 'jsdom';
import {runDOM} from './dom-check.mjs';
const repo=fileURLToPath(new URL('../../../../',import.meta.url));
const [artifacts,executable]=process.argv.slice(2);
const focused=process.argv.includes('--focused');
if(!artifacts||!executable)throw new Error('Pass actual artifact directory and built web_markdown_fixture executable');
const require=createRequire(import.meta.url);
const original=require(repo+'/dash/news-reader.js');
const beforePolicy=require('./news-reader-before-link-policy.cjs');
const md=require(repo+'/dash/vendor/markdown-it-15.0.2.umd.min.js');
const purifyDOM=new JSDOM('');const actualPurify=require(repo+'/dash/vendor/purify-3.4.16.min.js')(purifyDOM.window);assert.equal(actualPurify.isSupported,true);assert.equal(typeof actualPurify.sanitize,'function');const xss='<img src=x onerror=alert(1)><script>alert(2)</script>';assert.notEqual(actualPurify.sanitize(xss),xss);assert.ok(!actualPurify.sanitize(xss).includes('onerror'));
const jsdomVersion=require('./node_modules/jsdom/package.json').version;assert.equal(jsdomVersion,'26.1.0');
const tick=()=>new Promise(r=>setImmediate(r));
const temp=fs.mkdtempSync(path.join(os.tmpdir(),'comandos-news-wasm-'));
for(const name of fs.readdirSync(artifacts))if(name.endsWith('.js')||name.endsWith('.wasm'))fs.copyFileSync(path.join(artifacts,name),path.join(temp,name));
fs.writeFileSync(path.join(temp,'package.json'),' {"type":"module"}');
const processFixture=spawn(executable,[],{stdio:['pipe','pipe','pipe']});
let fixtureStderr='';processFixture.stderr.on('data',b=>fixtureStderr+=b);
const url=await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(new Error('fixture startup timeout '+fixtureStderr)),10000);processFixture.stdout.once('data',b=>{clearTimeout(timer);resolve(String(b).trim());});processFixture.once('exit',c=>reject(new Error('fixture exited '+c+' '+fixtureStderr)));});
const fetchNative=globalThis.fetch;
let requests=[];
globalThis.Window=class{static [Symbol.hasInstance](v){return v===globalThis;}};
globalThis.window=globalThis;
globalThis.document={visibilityState:'visible',readyState:'loading',querySelector(){return{getAttribute(){return'news-reader vendor-markdown-it vendor-purify';}}},addEventListener(){},removeEventListener(){}};
globalThis.localStorage={getItem:k=>k==='cc_token'?'b8-disposable-fixture':null,setItem(){}};
globalThis.fetch=async(p,options={})=>{requests.push({path:p,body:options.body});return fetchNative(url+p,{...options,headers:{...options.headers,'X-Comandos-Token':'b8-disposable-fixture'}});};
try{
 const baseline=spawnSync(process.execPath,[repo+'/tests/news_reader_checks.cjs'],{encoding:'utf8'});process.stdout.write(baseline.stdout);if(baseline.status!==0)throw new Error(baseline.stderr);
 const wasm=await import(pathToFileURL(temp+'/comandos_web.js'));await wasm.default({module_or_path:fs.readFileSync(temp+'/comandos_web_bg.wasm')});wasm.boot('b8-private');await tick();
 assert.equal(typeof NewsReader.mount,'function');assert.equal(__comandosReady,true);
 const source=fs.readFileSync(repo+'/tests/news_reader_checks.cjs','utf8').replace("const NR = require(path.join(root, 'dash/news-reader.js'));",'const NR = globalThis.NewsReader;').replace(/await check\(([^\n]*), \(\) => \{/g,'await check($1, async () => {').replace(/const out = (render|plain)\(/g,'const out = await $1(').replace('(async () => {','return (async () => {').replace('.catch(err => { console.error(err); process.exit(1); });','.catch(err => { throw err; });');
 await vm.runInThisContext(`(async function(require,__dirname,globalThis){${source}\n})`,{filename:'original-news-assertions-async.cjs'})(require,repo+'/tests',globalThis);
 await tick();await tick();
 const result=[];function eq(name,actual,expected){assert.deepEqual(actual,expected,name);result.push({name,equal:true});}
 const texts=['normal 🍅','high\ud800end','low\udc00end','marker\ue000\udb80\udc00end','double\ue000\ue000\udb80\udc00end'];
 for(const text of texts){eq('inline '+JSON.stringify(text),NewsReader.inlineText(text),original.inlineText(text));eq('blocks '+JSON.stringify(text),NewsReader.blocksHtml([{type:'p',text},{type:'img',media:'0123456789abcdef0123456789abcdef.png',alt:text}]),original.blocksHtml([{type:'p',text},{type:'img',media:'0123456789abcdef0123456789abcdef.png',alt:text}]));eq('note '+JSON.stringify(text),NewsReader.noteCommand('/nota '+text),original.noteCommand('/nota '+text));const render=NewsReader.createRenderer(undefined,undefined);eq('server UTF16 '+JSON.stringify(text),(await render(text)).trim(),'<p>'+original.inlineText(text)+'</p>');render.dispose();}
 for(const cost of [0,1.005,2.675,1.115,1e21]){const edition={models:{'20':1,'3':1,'opencode:a/b':2},costUsd:cost,sourceCount:3,failedSourceCount:1,job:{startedAt:1,finishedAt:60001}};eq('provenance '+cost,NewsReader.provenance(edition),original.provenance(edition));}
 // Meaningful negative calibration: missing HTML is rejected, never a successful placeholder.
 const missing=NewsReader.createRenderer(undefined,undefined,{fetchJson:async()=>({})});await assert.rejects(missing('missing'),/Respuesta Markdown inválida/);missing.dispose();result.push({name:'negative missing HTML',equal:true});
 let calls=0,resolve;const render=NewsReader.createRenderer(undefined,undefined,{fetchJson:()=>{calls++;return new Promise(r=>resolve=r);}});const a=render('same'),b=render('same');assert.equal(a,b);await tick();resolve({html:'<p>same</p>'});eq('coalesced actual promise',await a,await b);assert.equal(calls,1);await render('same');assert.equal(calls,1);render.dispose();
 const pending=NewsReader.createRenderer(undefined,undefined,{fetchJson:()=>new Promise(()=>{})});const cancelled=pending('never');pending.dispose();await assert.rejects(Promise.race([cancelled,new Promise((_,reject)=>setTimeout(()=>reject(new Error('cancel did not settle within 1500ms')),1500))]),/Lectura cancelada/);result.push({name:'dispose settles noncooperative transport',equal:true});
 let cacheCalls=0;const cache=NewsReader.createRenderer(undefined,undefined,{fetchJson:async(_,b)=>{cacheCalls++;return{html:'<p>'+b.text+'</p>'}}});for(let i=0;i<65;i++)await cache('t'+i);await cache('t64');assert.equal(cacheCalls,65);await cache('t0');assert.equal(cacheCalls,66);cache.dispose();result.push({name:'cache bounded 64',equal:true});
 // Retain the independent originals; verify the oracle again instead of
 // replacing expectations with the candidate's output.
 const reviewOriginal=original.createRenderer(md,actualPurify);
 const reviewActual=NewsReader.createRenderer(undefined,undefined);
 const reviewPairs=[];
 for(const fixture of ['review-baselines.json','repair-grammar.json','review-v2-baselines.json','review-v3-baselines.json']){
  const rows=JSON.parse(fs.readFileSync(repo+'/xtask/web/fixtures/b8/'+fixture,'utf8')).cases;
  for(const row of rows){
   assert.equal(reviewOriginal(row.text),row.baseline,'immutable original '+row.text);
   const candidate=await reviewActual(row.text);
   const template=purifyDOM.window.document.createElement('template');template.innerHTML=candidate;
   assert.equal(template.content.querySelectorAll('img,script,svg,[onerror],[onload]').length,0);
   for(const link of template.content.querySelectorAll('a')){assert.match(link.getAttribute('href'),/^https?:\/\//i);assert.equal(link.target,'_blank');assert.equal(link.rel,'noopener noreferrer nofollow');}
   reviewPairs.push({id:fixture+':'+reviewPairs.length,text:row.text,baseline:row.baseline,candidate,knownPreexistingNul:row.knownPreexistingNul===true});
   if(row.knownPreexistingNul)assert.equal(candidate,row.knownCandidate,'preexisting NUL remains outside repair');
  }
 }
 const reviewNormalized=spawnSync(executable,['--normalize'],{input:JSON.stringify(reviewPairs),encoding:'utf8',timeout:10000});assert.equal(reviewNormalized.status,0,reviewNormalized.stderr);
 const allReviewDifferences=JSON.parse(reviewNormalized.stdout).filter(row=>row.difference);
 const knownReviewDifferences=allReviewDifferences.filter(row=>reviewPairs.find(pair=>pair.id===row.id)?.knownPreexistingNul);assert.equal(knownReviewDifferences.length,1,'preexisting NUL calibration');
 const reviewDifferences=allReviewDifferences.filter(row=>!reviewPairs.find(pair=>pair.id===row.id)?.knownPreexistingNul);
 let alphabet='';for(let u=0xd800;u<=0xdfff;u++)alphabet+='x'+String.fromCharCode(u)+'y';for(let u=0xf0000;u<0xf0800;u++)alphabet+='x\ue000'+String.fromCodePoint(u)+'y';alphabet+='\ue000\ue000\ue000';
 assert.equal(NewsReader.inlineText(alphabet),original.inlineText(alphabet));
 assert.equal((await reviewActual(alphabet)).trim(),'<p>'+original.inlineText(alphabet)+'</p>');reviewActual.dispose();
 const review={actualWasm:artifacts,pairs:reviewPairs,differences:reviewDifferences,knownPreexistingDifferences:knownReviewDifferences,utf16:{loneSurrogates:2048,literalSentinelScalars:2048,equal:true},sanitizerNegative:true};
 fs.writeFileSync(process.env.B8_REVIEW_OUTPUT||temp+'/review-results.json',JSON.stringify(review,null,2));assert.equal(reviewDifferences.length,0,JSON.stringify(reviewDifferences));result.push({name:'independent originals + repair grammar actual WASM, full UTF16 alphabets',equal:true});
 if(!focused){
 const domResult=await runDOM(repo,original,NewsReader,md,async(path,body)=>{const r=await fetchNative(url+path,{method:'POST',headers:{'X-Comandos-Token':'b8-disposable-fixture','Content-Type':'application/json'},body:JSON.stringify(body)});if(!r.ok)throw new Error('Markdown fixture '+r.status);return r.json();});
 const domPairs=domResult.cases.flatMap(row=>['edition','panel'].map(part=>({id:row.name+' '+part,baseline:row.baselineHTML[part],candidate:row.candidateHTML[part]})));const domNormalized=spawnSync(executable,['--normalize'],{input:JSON.stringify(domPairs),encoding:'utf8',timeout:10000});if(domNormalized.status!==0)throw new Error(domNormalized.stderr);domResult.domDifferences=JSON.parse(domNormalized.stdout).filter(row=>row.difference);
 fs.writeFileSync(process.env.B8_DOM_OUTPUT||temp+'/dom-results.json',JSON.stringify(domResult,null,2));assert.equal(domResult.domDifferences.length,0,JSON.stringify(domResult.domDifferences));result.push({name:'original product DOM actual handler comparisons',equal:true});
 const corpus=JSON.parse(fs.readFileSync(repo+'/xtask/web/fixtures/b8/corpus.json','utf8'));assert.equal(corpus.cases.length,200);
 const oldRender=original.createRenderer(md,actualPurify);const policyBeforeRender=beforePolicy.createRenderer(md,actualPurify);
 const beforeLink=policyBeforeRender('[docs](https://example.org)');const afterLink=oldRender('[docs](https://example.org)');assert.ok(!beforeLink.includes('target=')&&!beforeLink.includes('rel='));assert.ok(afterLink.includes('target="_blank"')&&afterLink.includes('rel="noopener noreferrer nofollow"'));assert.ok(!oldRender('[x](javascript:alert(1))').includes('<a '));
 const actualRender=NewsReader.createRenderer(undefined,undefined);const pairs=[];
 const realManifest=JSON.parse(fs.readFileSync(repo+'/xtask/web/fixtures/markdown/corpus/manifest.json','utf8'));assert.equal(realManifest.entries.length,200);
 const real=realManifest.entries.map(row=>{const file=fs.readFileSync(repo+'/xtask/web/fixtures/markdown/corpus/'+row.file);assert.equal(createHash('sha256').update(file).digest('hex'),row.sha256);return{id:row.file,kind:'real-derived-'+row.kind,text:file.toString('utf8')};});
 for(const row of [...corpus.cases,...real])pairs.push({id:row.id,kind:row.kind,text:row.text,baseline:oldRender(row.text),candidate:await actualRender(row.text)});
 actualRender.dispose();
 const normalization=spawnSync(executable,['--normalize'],{input:JSON.stringify(pairs),encoding:'utf8',timeout:10000});if(normalization.status!==0)throw new Error(normalization.stderr);
 const exactPreDifferences=pairs.filter(row=>{const pre=html=>{const template=purifyDOM.window.document.createElement('template');template.innerHTML=html;return Array.from(template.content.querySelectorAll('pre'),node=>node.textContent);};return JSON.stringify(pre(row.baseline))!==JSON.stringify(pre(row.candidate));}).map(row=>row.id);assert.deepEqual(exactPreDifferences,[],'preformatted whitespace must remain exact');
 const normalized=JSON.parse(normalization.stdout);const policyPairs=pairs.map(p=>({...p,baseline:policyBeforeRender(p.text)}));const policyNormalize=spawnSync(executable,['--normalize'],{input:JSON.stringify(policyPairs),encoding:'utf8',timeout:10000});if(policyNormalize.status!==0)throw new Error(policyNormalize.stderr);const beforeDifferences=JSON.parse(policyNormalize.stdout).filter(r=>r.difference);const differences=normalized.filter(r=>r.difference).map(r=>({...r,...pairs.find(p=>p.id===r.id)}));
 fs.writeFileSync(process.env.B8_CORPUS_OUTPUT||temp+'/corpus-results.json',JSON.stringify({provenance:corpus.provenance,actualWasm:artifacts,normalizedCases:normalized.length,exactPreWhitespace:true,synthetic:200,realDerived:200,realManifestSha256:createHash('sha256').update(fs.readFileSync(repo+'/xtask/web/fixtures/markdown/corpus/manifest.json')).digest('hex'),policyCorrection:{beforeSha256:createHash('sha256').update(fs.readFileSync(repo+'/xtask/web/fixtures/b8/news-reader-before-link-policy.cjs')).digest('hex'),afterSha256:createHash('sha256').update(fs.readFileSync(repo+'/dash/news-reader.js')).digest('hex'),beforeLink,afterLink,beforeDifferences},jsdomVersion,domPurifySupported:actualPurify.isSupported,domPurifySha256:createHash('sha256').update(fs.readFileSync(repo+'/dash/vendor/purify-3.4.16.min.js')).digest('hex'),differences},null,2));
 assert.equal(differences.length,0,'Synthetic corpus DOM differences: '+JSON.stringify(differences.slice(0,8).map(r=>({id:r.id,kind:r.kind,difference:r.difference}))));
 result.push({name:'200 synthetic + 200 real-derived original-Markdown/server actual WASM normalized DOM cases',equal:true});
 }
 console.log(JSON.stringify({focused,review:{pairs:reviewPairs.length,unexpectedDifferences:reviewDifferences.length,knownPreexistingDifferences:knownReviewDifferences.length},artifact:artifacts,wasmSha256:createHash('sha256').update(fs.readFileSync(artifacts+'/comandos_web_bg.wasm')).digest('hex'),originalSha256:createHash('sha256').update(fs.readFileSync(repo+'/dash/news-reader.js')).digest('hex'),results:result,routeCalls:requests.filter(r=>r.path==='/web/markdown').length},null,2));
}finally{processFixture.stdin.end();await new Promise(resolve=>{if(processFixture.exitCode!==null)resolve();else processFixture.once('exit',resolve);});purifyDOM.window.close();fs.rmSync(temp,{recursive:true,force:true});}
