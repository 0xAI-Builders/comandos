// Execute generated ESM transport itself; no application logic is copied here.
const vm=require('node:vm'),fs=require('node:fs'),assert=require('node:assert/strict');
const source=fs.readFileSync(process.argv[2],'utf8');
(async()=>{
 for(const scenario of ['unused','success','import-fail','init-fail','register-fail']){
  const calls=[],context=vm.createContext({URL,document:{currentScript:{dataset:{k:'private-proof'}},querySelector(){throw Error('unexpected fallback')}}});
  const selected=scenario!=='unused';
  const core=new vm.SyntheticModule(['default','boot','needs_content','dependency_failed'],function(){
   this.setExport('default',async()=>calls.push('core-init'));
   this.setExport('needs_content',()=>selected);
   this.setExport('boot',k=>{assert.equal(k,'private-proof');calls.push('boot')});
   this.setExport('dependency_failed',(k,error)=>{assert.equal(k,'private-proof');assert.match(String(error),/fixture/);calls.push('failed')});
  },{context});
  const content=new vm.SyntheticModule(['default','register_content'],function(){
   this.setExport('default',async()=>{calls.push('content-init');if(scenario==='init-fail')throw Error('fixture init')});
   this.setExport('register_content',()=>{calls.push('register');if(scenario==='register-fail')throw Error('fixture register')});
  },{context});
  const loader=new vm.SourceTextModule(source,{context,initializeImportMeta(meta){meta.url='https://private.invalid/web/hash/boot.js'},async importModuleDynamically(spec){
   assert.equal(spec,'./comandos_web_content.js');calls.push('import');if(scenario==='import-fail')throw Error('fixture import');await content.link(()=>{});await content.evaluate();return content;
  }});
  await loader.link(spec=>{assert.equal(spec,'./comandos_web.js');return core});
  if(scenario.endsWith('fail')){await assert.rejects(loader.evaluate(),/fixture/);assert.equal(calls.at(-1),'failed');assert(!calls.includes('boot'));}
  else{await loader.evaluate();assert.deepEqual(calls,scenario==='unused'?['core-init','boot']:['core-init','import','content-init','register','boot']);}
 }
 console.log('PASS generated content ESM: lazy selection, ordered registration and three transport failures');
})().catch(e=>{console.error(e);process.exitCode=1});
