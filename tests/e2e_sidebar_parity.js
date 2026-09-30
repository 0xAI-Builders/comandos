#!/usr/bin/env node
// A fully isolated renderer: no desktop bridge calls, terminal connections or live APIs.
// Contract (S2): the left panel is the per-CLI command sidebar. A click on a
// command types into the selected pane through POST /pane/type and never
// through /send; desktop (GTK bridge) and remote render the same ids.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {loadPlaywright} = require('./e2e_mobile_remote');
const root = path.resolve(__dirname, '..');
const output = path.join(root, 'design/sidebar-comparison');
const baseline = process.argv.includes('--before');
const catalog = JSON.parse(fs.readFileSync(path.join(root, 'tests/fixtures/command-catalog.json'), 'utf8'));
const registry = {harnesses:{claude:{label:'Claude Code',accounts:[{alias:'main',selectable:true}]}},motors:{claude:{label:'Claude',models:[{id:'opus',name:'Opus',efforts:['high']}]}},matrix:[{id:'claude:claude',harness:'claude',motor:'claude',selectable:true}]};
const items = [
  {session:'alpha',pane:'%1',project:'Alpha',status:'working',agent:'claude'},
  {session:'alpha',pane:'%2',project:'Alpha · revisión',status:'waiting',agent:'codex'},
  {session:'beta',pane:'%3',project:'Beta',status:'idle',agent:'shell'},
  {session:'term-qfixture1',pane:'%4',project:'Terminal 10:00',status:'idle',agent:'shell'},
].map(it=>({...it,motor:it.agent,model:'opus',account:'main',cwd:'/tmp/'+it.session,alive:true,detail:'Sesión de prueba',ts:Math.floor(Date.now()/1000)}));
const agentOf = pane => (items.find(it=>it.pane===pane)||{}).agent;
const CLIS = new Set(catalog.clis.map(c=>c.id));
async function main(){
 fs.mkdirSync(output,{recursive:true});
 const browser=await loadPlaywright().chromium.launch({headless:true,executablePath:'/usr/bin/google-chrome',args:['--no-sandbox']});
 try {
  const snapshots={};
  for(const mode of ['desktop','remote','phone390','phone320']){
   const desktop=mode==='desktop',phone=mode.startsWith('phone');
   const context=await browser.newContext({viewport:{width:desktop?380:phone?Number(mode.slice(5)):1400,height:phone?844:1000},hasTouch:phone,isMobile:phone,serviceWorkers:'block'});
   const page=await context.newPage(),errors=[],posts=[];
   let desktopTab={session:'alpha',pane:'%1'};
   page.on('pageerror',e=>errors.push(e.message));
   if(desktop)await page.addInitScript(()=>{window.webkit={messageHandlers:{centro:{postMessage(){}}}};});
   await page.route('**/*',async route=>{
    const req=route.request(),url=new URL(req.url()),p=url.pathname;
    const file=path.join(root,p.startsWith('/assets/')?'':'dash',p==='/'?'index.html':p);
    if(req.method()==='GET'&&fs.existsSync(file)&&fs.statSync(file).isFile())return route.fulfill({body:fs.readFileSync(file),contentType:p.endsWith('.js')?'text/javascript':p.endsWith('.css')?'text/css':p.endsWith('.svg')?'image/svg+xml':'text/html'});
    if(req.method()==='POST')posts.push({path:p,body:req.postDataJSON?.()||null});
    let data={};
    if(p==='/state')data=items;
    if(p==='/providers')data=registry;
    if(p==='/active-tab')data=desktopTab;
    if(p==='/tabs')data=[{session:'alpha',label:'Alpha'},{session:'beta',label:'Beta'},{session:'term-qfixture1',label:'Terminal 10:00'}];
    if(['/events','/ssh','/tab-history'].includes(p))data=[];
    if(p==='/commands/catalog'){
      const pane=url.searchParams.get('pane')||'',agent=agentOf(pane);
      data={cliInPane:CLIS.has(agent)?agent:'',target:{session:url.searchParams.get('session')||'',pane},catalog,versionsAt:0};
    }
    if(p==='/chains')data={chains:[{slug:'demo',name:'Demo',steps:[{kind:'pane',text:'/status'}]}]};
    if(p==='/pane/type')data={ok:true,typed:(posts.at(-1)?.body?.text||'').length};
    if(p==='/usage/state')data={providers:[{active_panes:3}]};
    if(p==='/remote')data={};
    if(p==='/term/health')data={ok:true};
    if(p==='/webterm-token')data={token:'isolated-fixture-token'};
    if(p.startsWith('/term'))return route.fulfill({body:'<!doctype html><title>Isolated terminal fixture</title>',contentType:'text/html'});
    await route.fulfill({json:data});
   });
   await page.goto(desktop?'http://sidebar.test/':'https://sidebar.test/');
   await page.waitForFunction(()=>S.list?.length===4);
   if(!desktop)await page.evaluate(()=>{activeTerm='alpha';remotePaneFocus.set('alpha',{pane:'%1',ts:Date.now()});showView('panel');render(S.list);});
   await page.waitForFunction(()=>document.querySelectorAll('#command-sidebar .cs-cli').length===5);
   await page.waitForTimeout(600);
   await page.locator('#view-panel').screenshot({path:path.join(output,`${baseline?'before':'after'}-${mode}.png`)});
   snapshots[mode]=await page.evaluate(()=>({width:document.querySelector('#view-panel').clientWidth,
     ids:['command-sidebar','side-top','workspace-tools','toggle-overview','open-session-profiles','open-extension-usage','session-overview','newsess'].filter(id=>document.getElementById(id)),
     clis:[...document.querySelectorAll('#command-sidebar .cs-cli')].map(e=>e.dataset.cli),
     here:[...document.querySelectorAll('#command-sidebar .cs-cli.here')].map(e=>e.dataset.cli),
     target:document.querySelector('#command-sidebar .cs-target')?.textContent||'',
     gone:['op-chat','op-split','toggle-chat','rows','centro','sidebar-insights','tl-wrap','op-target'].filter(id=>document.getElementById(id)),
     overflow:document.documentElement.scrollWidth>innerWidth}));
   if(!baseline){
    const s=snapshots[mode];
    assert.equal(s.clis.length,5,mode+' five CLIs');
    assert.deepEqual(s.here,['claude'],mode+' pane %1 runs claude');
    assert.match(s.target,/%1/,mode+' target is the selected pane');
    assert.deepEqual(s.gone,[],mode+' chat and old sections are gone');
    assert(!s.overflow,mode+' no horizontal overflow');
    assert(await page.locator('#command-sidebar .cs-terms').isVisible(),mode+' quick terminals listed');
    assert.equal(await page.locator('#command-sidebar .cs-terms [data-focus-term="term-qfixture1"]').count(),1,mode+' quick terminal entry');
    // Open claude and click its first slash command: exactly one /pane/type, no /send.
    const claude=page.locator('#command-sidebar .cs-cli[data-cli="claude"]');
    if(!await claude.evaluate(e=>e.classList.contains('open')))await claude.locator('.cli-h').click();
    const grp=claude.locator('.grp').first();
    if(!await grp.evaluate(e=>e.classList.contains('open')))await grp.locator('.lab3').click();
    posts.length=0;
    await grp.locator('.cmd').first().click();
    await page.waitForFunction(()=>!document.querySelector('#command-sidebar').classList.contains('typing'));
    assert.deepEqual(posts.map(x=>x.path),['/pane/type'],mode+' a click types into the pane');
    assert.equal(posts[0].body.session,'alpha');assert.equal(posts[0].body.pane,'%1');
    assert(!/[\r\n]/.test(posts[0].body.text),mode+' never sends Enter');
    await page.locator('#view-panel').screenshot({path:path.join(output,`after-${mode}-typed.png`)});
    // Changing the selected pane re-reads the catalog for that pane.
    if(desktop){desktopTab={session:'alpha',pane:'%2'};await page.waitForFunction(()=>ACTIVE_TAB.pane==='%2');}
    else await page.evaluate(()=>{remotePaneFocus.set('alpha',{pane:'%2',ts:Date.now()});render(S.list);});
    await page.waitForFunction(()=>[...document.querySelectorAll('#command-sidebar .cs-cli.here')].map(e=>e.dataset.cli).join()==='codex');
    await page.locator('#toggle-overview').click();
    assert.equal(await page.locator('.overview-card').count(),4,mode+' global inventory available');
    await page.evaluate(()=>{S.sel='';activeTerm=null;ACTIVE_TAB={session:''};render([]);});
    assert(await page.locator('#open-session-profiles').isVisible(),mode+' empty inventory retains profiles');
    assert.equal(await page.locator('#command-sidebar .cs-cli').count(),5,mode+' catalog stays without a target');
   }
   assert.deepEqual(errors,[],mode+' no JavaScript errors');
   await context.close();
  }
  fs.writeFileSync(path.join(output,`${baseline?'before':'after'}-dom.json`),JSON.stringify(snapshots,null,2)+'\n');
  if(!baseline){
   assert.deepEqual(snapshots.remote.ids,snapshots.desktop.ids,'same ids desktop/remote');
   assert.deepEqual(snapshots.remote.clis,snapshots.desktop.clis,'same CLIs desktop/remote');
  }
  console.log(baseline?'Captured baseline sidebar comparison.':'PASS: command sidebar on desktop, remote and phones: five CLIs, pane CLI marked, a click types via /pane/type without Enter, quick terminals listed, same ids.');
 }finally{await browser.close();}
}
main().catch(e=>{console.error(e);process.exitCode=1;});
