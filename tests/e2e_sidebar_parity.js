#!/usr/bin/env node
// A fully isolated renderer: no desktop bridge calls, terminal connections or live APIs.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {loadPlaywright} = require('./e2e_mobile_remote');
const root = path.resolve(__dirname, '..');
const output = path.join(root, 'design/sidebar-comparison');
const baseline = process.argv.includes('--before');
const registry = {harnesses:{claude:{label:'Claude Code',accounts:[{alias:'main',selectable:true}]}},motors:{claude:{label:'Claude',models:[{id:'opus',name:'Opus',efforts:['high']}]}},matrix:[{id:'claude:claude',harness:'claude',motor:'claude',selectable:true}]};
const items = [
  {session:'alpha',pane:'%1',project:'Alpha',status:'working'},
  {session:'alpha',pane:'%2',project:'Alpha · revisión',status:'waiting'},
  {session:'beta',pane:'%3',project:'Beta',status:'idle'},
].map(it=>({...it,agent:'claude',motor:'claude',model:'opus',account:'main',cwd:'/tmp/'+it.session,alive:true,detail:'Sesión de prueba',ts:Math.floor(Date.now()/1000)}));
async function main(){
 fs.mkdirSync(output,{recursive:true});
 const browser=await loadPlaywright().chromium.launch({headless:true,executablePath:'/usr/bin/google-chrome',args:['--no-sandbox']});
 try {
  const snapshots={};
  for(const mode of ['desktop','remote','phone390','phone320']){
   const desktop=mode==='desktop',phone=mode.startsWith('phone');
   const context=await browser.newContext({viewport:{width:desktop?380:phone?Number(mode.slice(5)):1400,height:phone?844:1000},hasTouch:phone,isMobile:phone,serviceWorkers:'block'});
   const page=await context.newPage(),errors=[];
   let desktopTab={session:'alpha',pane:'%1'};
   page.on('pageerror',e=>errors.push(e.message));
   if(desktop)await page.addInitScript(()=>{window.webkit={messageHandlers:{centro:{postMessage(){}}}};});
   await page.route('**/*',async route=>{
    const url=new URL(route.request().url()),p=url.pathname;
    const file=path.join(root,p.startsWith('/assets/')?'':'dash',p==='/'?'index.html':p);
    if(fs.existsSync(file)&&fs.statSync(file).isFile())return route.fulfill({body:fs.readFileSync(file),contentType:p.endsWith('.js')?'text/javascript':p.endsWith('.css')?'text/css':p.endsWith('.svg')?'image/svg+xml':'text/html'});
    let data={};
    if(p==='/state')data=items;
    if(p==='/providers')data=registry;
    if(p==='/active-tab')data=desktopTab;
    if(p==='/tabs')data=[{session:'alpha',label:'Alpha'},{session:'beta',label:'Beta'}];
    if(['/events','/ssh','/tab-history'].includes(p))data=[];
    if(p==='/operator')data={id:'test',messages:[],conversations:[],models:[]};
    if(p==='/session-brain')data={skills:[],mcps:[],accounts:[]};
    if(p==='/dedication')data={days:[{projects:[{name:'Alpha',minutes:95,path:'/tmp/alpha'},{name:'Beta',minutes:25,path:'/tmp/beta'}]}]};
    if(p==='/usage/state')data={providers:[{active_panes:3}]};
    if(p==='/remote')data={};
    if(p==='/term/health')data={ok:true};
    if(p==='/webterm-token')data={token:'isolated-fixture-token'};
    if(p.startsWith('/term'))return route.fulfill({body:'<!doctype html><title>Isolated terminal fixture</title>',contentType:'text/html'});
    await route.fulfill({json:data});
   });
   await page.goto(desktop?'http://sidebar.test/':'https://sidebar.test/');
   await page.waitForFunction(()=>S.list?.length===3);
   if(!desktop)await page.evaluate(()=>{activeTerm='alpha';showView('panel');});
   await page.waitForTimeout(1300);
   await page.locator('#view-panel').screenshot({path:path.join(output,`${baseline?'before':'after'}-${mode}.png`)});
   snapshots[mode]=await page.evaluate(()=>({width:document.querySelector('#view-panel').clientWidth,geometry:Object.fromEntries(['content','side-top','op-chat'].map(id=>[id,{height:document.getElementById(id).clientHeight,basis:getComputedStyle(document.getElementById(id)).flexBasis}])),sections:[...document.querySelectorAll('#side-top > section')].filter(e=>e.getBoundingClientRect().height).map(e=>e.id),rows:[...document.querySelectorAll('#rows .row')].filter(e=>e.getBoundingClientRect().height).map(e=>e.dataset.rk),insights:document.querySelector('#sidebar-insights').innerText,tools:document.querySelector('#workspace-tools').getBoundingClientRect().height,overflow:document.documentElement.scrollWidth>innerWidth,errors:[]}));
   if(!baseline){
    assert.deepEqual(snapshots[mode].rows,['alpha|%1','alpha|%2'],mode+' split inventory');
    assert(snapshots[mode].sections.includes('sidebar-insights'),mode+' insights visible');
    assert(snapshots[mode].tools>0,mode+' workspace controls visible');
    assert(!snapshots[mode].overflow,mode+' no horizontal overflow');
    assert.match(snapshots[mode].insights,/3\s+agentes activos/i);
    await page.locator('#toggle-chat').click();
    assert(await page.locator('#op-chat').isHidden());
    await page.locator('#side-top').evaluate(e=>e.scrollTop=0);
    await page.locator('#view-panel').screenshot({path:path.join(output,`after-${mode}-chat-hidden.png`)});
    await page.locator('#sidebar-insights').scrollIntoViewIfNeeded();
    await page.locator('#view-panel').screenshot({path:path.join(output,`after-${mode}-insights.png`)});
    if(!desktop){
      await page.locator('#side-usage').click();
      assert(await page.locator('#usage').isVisible(),mode+' analytics opens');
      await page.locator('[data-close="usage"]').click();
    }
    await page.locator('.row[data-rk="alpha|%2"] .name').click();
    assert.match(await page.locator('#op-target').innerText(),/%2/,mode+' pane selection updates chat target');
    assert(await page.locator('.row[data-rk="alpha|%2"]').evaluate(e=>e.classList.contains('active-pane')),mode+' selected pane highlighted');
    if(!desktop)assert.equal(await page.evaluate(()=>activeTerm),'alpha',mode+' selecting split preserves terminal session');
    await page.locator('#toggle-chat').click();
    assert(await page.locator('#op-chat').isVisible());
    const splitter=await page.locator('#op-split').boundingBox();
    await page.mouse.move(splitter.x+splitter.width/2,splitter.y+5);
    await page.mouse.down();await page.mouse.move(splitter.x+splitter.width/2,splitter.y+65);await page.mouse.up();
    assert(Number(await page.evaluate(()=>localStorage.getItem('cc-op-chat-h')))>0,mode+' splitter saves explicit height');
    if(desktop){desktopTab={session:'beta',pane:'%3'};await page.waitForFunction(()=>ACTIVE_TAB.session==='beta');}
    else{await page.locator('.apptab').filter({hasText:'Beta'}).click();if(phone)await page.locator('.apptab').filter({hasText:'Panel'}).click();}
    await page.waitForFunction(()=>document.querySelector('#op-target').textContent.includes('Beta'));
    assert.equal(await page.locator('#rows .row:visible').count(),0,mode+' single pane hides inventory');
    assert(await page.locator('#toggle-chat').isVisible(),mode+' controls recoverable with single pane');
    await page.locator('#toggle-overview').click();
    assert.equal(await page.locator('.overview-card').count(),3,mode+' global inventory available');
    // Server focus changes must never hijack the remote device's active session.
    if(!desktop){desktopTab={session:'alpha',pane:'%2'};await page.waitForTimeout(1200);assert.match(await page.locator('#op-target').innerText(),/Beta/);}
    await page.locator('#toggle-chat').click();await page.reload();
    await page.waitForFunction(()=>S.list?.length===3);
    assert(await page.locator('#op-chat').isHidden(),mode+' chat preference persists');
    await page.locator('#toggle-chat').click();assert(await page.locator('#op-chat').isVisible());
    await page.evaluate(()=>{S.sel='';activeTerm=null;ACTIVE_TAB={session:''};render([]);});
    assert(await page.locator('#toggle-chat').isVisible(),mode+' empty inventory retains chat recovery');
    assert(await page.locator('#open-session-profiles').isVisible(),mode+' empty inventory retains profiles');
   }
   assert.deepEqual(errors,[],mode+' no JavaScript errors');
   await context.close();
  }
  fs.writeFileSync(path.join(output,`${baseline?'before':'after'}-dom.json`),JSON.stringify(snapshots,null,2)+'\n');
  if(!baseline){
   assert.deepEqual(snapshots.remote.sections,snapshots.desktop.sections);
   assert.equal(snapshots.remote.insights,snapshots.desktop.insights);
   assert(Math.abs(snapshots.remote.geometry['op-chat'].height-snapshots.desktop.geometry['op-chat'].height)<=1,'shared default chat geometry');
  }
  console.log(baseline?'Captured baseline sidebar comparison.':'PASS: shared sidebar sections, analytics, active session splits, local remote selection, recoverable chat and mobile layout.');
 }finally{await browser.close();}
}
main().catch(e=>{console.error(e);process.exitCode=1;});
