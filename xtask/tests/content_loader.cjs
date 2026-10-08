// Executes generated ESM platform transport itself; Rust owns feature loading and errors.
const vm=require('node:vm'),fs=require('node:fs'),assert=require('node:assert/strict');
const source=fs.readFileSync(process.argv[2],'utf8');
(async()=>{
 for(const scenario of ['unused','success','content-import-fail','content-init-fail','content-register-fail','sound-init-fail']){
  const calls=[],context=vm.createContext({URL,document:{currentScript:{dataset:{k:'private-proof'}},querySelector(){throw Error('unexpected fallback')}}});let deferred;
  const core=new vm.SyntheticModule(['default','boot','needs_sound','dependency_failed','install_content_loader'],function(){
   this.setExport('default',async()=>calls.push('core-init'));
   this.setExport('needs_sound',()=>scenario!=='unused');
   this.setExport('boot',k=>{assert.equal(k,'private-proof');calls.push('boot')});
   this.setExport('install_content_loader',f=>{deferred=f;calls.push('install')});
   this.setExport('dependency_failed',(k,error)=>{assert.equal(k,'private-proof');assert.match(String(error),/fixture/);calls.push('failed')});
  },{context});
  const content=new vm.SyntheticModule(['default','register_content'],function(){
   this.setExport('default',async()=>{calls.push('content-init');if(scenario==='content-init-fail')throw Error('fixture content init')});
   this.setExport('register_content',()=>{calls.push('content-register');if(scenario==='content-register-fail')throw Error('fixture content register')});
  },{context});
  const sound=new vm.SyntheticModule(['default','register_sound'],function(){
   this.setExport('default',async()=>{calls.push('sound-init');if(scenario==='sound-init-fail')throw Error('fixture sound init')});
   this.setExport('register_sound',()=>calls.push('sound-register'));
  },{context});
  const loader=new vm.SourceTextModule(source,{context,initializeImportMeta(meta){meta.url='https://private.invalid/web/hash/boot.js'},async importModuleDynamically(spec){
   const module=spec==='./comandos_web_content.js'?content:sound;assert(['./comandos_web_content.js','./comandos_web_sound.js'].includes(spec));calls.push(spec.includes('_content.')?'content-import':'sound-import');if(scenario==='content-import-fail'&&module===content)throw Error('fixture content import');await module.link(()=>{});await module.evaluate();return module;
  }});
  await loader.link(spec=>{assert.equal(spec,'./comandos_web.js');return core});
  if(scenario==='sound-init-fail'){await assert.rejects(loader.evaluate(),/fixture/);assert.equal(calls.at(-1),'failed');assert(!calls.includes('boot'));continue;}
  await loader.evaluate();assert.equal(calls.at(-1),'boot');assert(!calls.includes('content-import'),'unused content must not fetch during native startup');
  if(scenario==='unused'){assert.deepEqual(calls,['core-init','install','boot']);continue;}
  if(scenario.startsWith('content-'))await assert.rejects(deferred(),/fixture/);else{await deferred();assert.deepEqual(calls.slice(-3),['content-import','content-init','content-register']);}
 }
 console.log('PASS generated ESM: no content startup fetch, first-use load and precise audio/content failures');
})().catch(e=>{console.error(e);process.exitCode=1});
