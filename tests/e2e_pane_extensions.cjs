// Run only on the configured Mac runtime. APIs use fixture sessions, never real agents.
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const http=require('node:http');
const chrome='/home/john/.local/share/comandos-browser/runtime/bin/chrome';
assert(fs.existsSync(chrome),'Run browser tests on the Mac host.');
const {chromium}=require(process.env.PLAYWRIGHT_CORE||'playwright-core');
const root=path.resolve(__dirname,'..');
const source={ok:true,identity:'socket|pid|start|session|%4|pid',conversationId:'exact-sid',harness:'codex',account:'main',
 inventory:{mcps:[{id:'docs',name:'docs',enabled:true,toggleable:true},{id:'web',name:'web',enabled:false,toggleable:true},{id:'locked',name:'locked',enabled:null,toggleable:false,reason:'Sin soporte por sesión'}],skills:[{id:'shared:tdd',name:'tdd',enabled:true,toggleable:true}]},
 desired:{mcps:{docs:true,web:false},skills:{'shared:tdd':true}},loaded:{mcps:{docs:true,web:false},skills:{'shared:tdd':true}},revision:1,operation:null,
 usage:{counts:{mcps:{docs:2,web:null,locked:null},skills:{'shared:tdd':null}},complete:false},templates:[],busy:false,applySupported:true};
let state=structuredClone(source),sibling=structuredClone(source),posts=[],getCount=0,conflict=false;
const server=http.createServer(async(req,res)=>{
 const url=new URL(req.url,'http://localhost');
 if(url.pathname.startsWith('/pane-extensions')) {
  if(req.method==='GET'){getCount++;res.setHeader('Content-Type','application/json');return res.end(JSON.stringify(url.searchParams.get('pane')==='%9'?sibling:state));}
  let raw='';for await(const chunk of req)raw+=chunk;const body=JSON.parse(raw);posts.push({path:url.pathname,body});
  assert.equal(body.pane,'%4');assert.equal(body.expectedIdentity,state.identity);assert.equal(body.expectedConversationId,state.conversationId);
  res.setHeader('Content-Type','application/json');
  if(conflict){conflict=false;res.statusCode=409;return res.end(JSON.stringify({ok:false,error:'La selección cambió. Actualiza.'}));}
  assert.equal(body.revision,state.revision);
  let reply={ok:true};
  if(url.pathname==='/pane-extensions'){state.desired=body.desired;state.revision++;}
  if(url.pathname.endsWith('/apply')){state.operation={operationId:body.requestId,state:'waiting'};reply={...reply,...state.operation};}
  if(url.pathname.endsWith('/cancel'))state.operation={...state.operation,state:'failed'};
  if(url.pathname.endsWith('/recover'))state.operation={...state.operation,state:'rolled_back'};
  if(url.pathname.endsWith('/template')) {
   if(body.name)state.templates.push({id:'tpl-1',name:body.name});
   else {state.desired={mcps:{docs:false,web:true},skills:{'shared:tdd':false}};state.revision++;reply.missing={mcps:['missing-server'],skills:[]};}
  }
  return res.end(JSON.stringify(reply));
 }
 const name=path.basename(url.pathname),file=path.join(root,'dash',name);
 if(!['extensions.html','extensions.js','extensions.css'].includes(name)){res.statusCode=404;return res.end();}
 res.setHeader('Content-Type',name.endsWith('.js')?'text/javascript':name.endsWith('.css')?'text/css':'text/html');res.end(fs.readFileSync(file));
});
(async()=>{
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
 const origin=`http://127.0.0.1:${server.address().port}`;
 const browser=await chromium.launch({executablePath:chrome,headless:true});
 try {
  const page=await browser.newPage({viewport:{width:1280,height:510}}),errors=[];
  page.on('pageerror',e=>errors.push(e.message));
  await page.goto(origin+'/extensions.html?session=term-fixture&pane=%254');
  const doc=page.locator('[data-id="docs"]'),web=page.locator('[data-id="web"]');
  await doc.waitFor();assert.equal(await page.locator('.bubble').count(),4);
  assert(await page.locator('[data-id="locked"]').isDisabled());
  assert.equal(await web.locator('small').textContent(),'sin dato');
  await doc.focus();await page.keyboard.press('Space');await page.waitForFunction(()=>document.querySelector('[data-id="docs"]').dataset.on==='false');
  assert.equal(await page.evaluate(()=>document.activeElement.dataset.id),'docs');
  assert.equal(state.desired.mcps.docs,false);assert.equal(state.loaded.mcps.docs,true);assert.equal(sibling.desired.mcps.docs,true);
  assert.equal(await page.locator('.bubble .change').count(),1);
  // Drag an available bubble into selected. Real pointer events, no DOM mutation.
  const box=await web.boundingBox(),zone=await page.locator('[data-zone="on"]').boundingBox();
  await page.mouse.move(box.x+box.width/2,box.y+box.height/2);await page.mouse.down();await page.mouse.move(zone.x+zone.width-20,zone.y+30,{steps:12});await page.mouse.up();
  await page.waitForFunction(()=>document.querySelector('[data-id="web"]').dataset.on==='true');assert(state.desired.mcps.web);
  conflict=true;await web.click();await page.getByRole('status').filter({hasText:'La selección cambió'}).waitFor();assert(state.desired.mcps.web);
  await page.locator('#template-name').fill('Trabajo <A>');await page.getByRole('button',{name:'+ Guardar set',exact:true}).click();await page.getByRole('button',{name:'Trabajo <A>',exact:true}).waitFor();
  await page.getByRole('button',{name:'Trabajo <A>',exact:true}).click();await page.getByRole('status').filter({hasText:'missing-server'}).waitFor();
  state.busy=true;await page.reload();await page.getByRole('button',{name:'Aplicar al terminar',exact:true}).click();
  await page.getByRole('button',{name:'Cancelar espera',exact:true}).waitFor();assert(await doc.isDisabled());assert.equal(state.operation.state,'waiting');
  await page.getByRole('button',{name:'Cancelar espera',exact:true}).click();await page.waitForFunction(()=>!document.querySelector('[data-id="docs"]').disabled);
  await page.getByRole('button',{name:'Interrumpir y aplicar ahora',exact:true}).click();
  await page.getByRole('button',{name:'Cancelar espera',exact:true}).waitFor();assert.equal(posts.at(-1).body.interrupt,true);
  state.operation.state='recovery_required';state.operation.error='El destino no confirmó la conversación <exacta>.';await page.getByRole('button',{name:'Recuperar sesión anterior',exact:true}).waitFor();
  assert((await page.getByRole('status').textContent()).includes('El destino no confirmó la conversación <exacta>.'));
  await page.getByRole('button',{name:'Recuperar sesión anterior',exact:true}).click();await page.getByRole('status').filter({hasText:'Se recuperó'}).waitFor();
  state.operation=null;state.loaded=null;await page.reload();await page.getByText('Carga sin verificar',{exact:true}).waitFor();assert.equal(await page.locator('.bubble .change').count(),0);
  await page.locator('#ext-search').fill('web');assert.equal(await page.locator('.bubble').count(),1);await page.locator('#ext-search').fill('');
  await page.screenshot({path:'/tmp/comandos-pane-qa/shelf-desktop.png'});
  await page.setViewportSize({width:390,height:700});await page.screenshot({path:'/tmp/comandos-pane-qa/shelf-mobile.png'});
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
  // New conversations cannot silently inherit an edit from the previously inspected scope.
  state.conversationId='replacement-sid';await page.getByRole('button',{name:'Actualizar panel',exact:true}).waitFor({timeout:15000});assert(await doc.isDisabled());
  assert.deepEqual(errors,[]);assert(getCount>5);
  console.log(JSON.stringify({passed:true,posts:posts.length,queries:getCount,errors,screenshots:['shelf-desktop.png','shelf-mobile.png']}));
 } finally {await browser.close();server.close();}
})().catch(e=>{console.error(e);server.close();process.exitCode=1;});
