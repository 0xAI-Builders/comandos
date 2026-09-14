// Run on the Mac mini. All requests are fixtures; no live session is modified.
const assert = require('node:assert/strict');
const fs = require('node:fs'), path = require('node:path');
const chrome = '/home/john/.local/share/comandos-browser/runtime/bin/chrome';
assert(fs.existsSync(chrome), 'Browser audit must run on the Mac host');
const {chromium} = require(process.env.PLAYWRIGHT_CORE || 'playwright-core');
const root = path.resolve(__dirname, '..'), baseline = process.argv[2];
const accounts = [{alias:'main',selectable:true,motorSelectable:true},{alias:'work',selectable:true,motorSelectable:true}];
const registry = {harnesses:{},motors:{},matrix:[],matrixHarnesses:['claude','codex','grok','acp']};
for(const h of ['claude','codex','grok']) {
  registry.harnesses[h] = {label:h,accounts};
  registry.motors[h] = {label:h,models:[{id:h==='codex'?'gpt-6-astra':h==='claude'?'opus':'grok-4',name:h,efforts:['high','low'],defaultEffort:'high'}]};
  registry.matrix.push({id:h+':'+h,harness:h,motor:h,selectable:true});
  registry.matrix.push({id:'acp:'+h,harness:'acp',motor:h,selectable:true});
}
registry.harnesses.acp={label:'ACP',accounts:[]};
const item = (session,pane,agent='codex') => ({session,pane,project:session==='local'?'⌂ local':'Signara',
  agent,motor:agent,model:agent==='codex'?'gpt-6-astra':agent==='claude'?'opus':'',account:'main',harnessAccount:'main',motorAccount:'main',
  effort:'high',alive:true,operable:true,cwd:'/tmp/fixture',status:'idle',ts:1,
  observedConfig:{identity:'identity-'+pane,conversationId:'thread-'+pane}});
(async()=>{
 const browser=await chromium.launch({headless:true,executablePath:chrome,chromiumSandbox:true});
 try {
  for(const [version,dir] of [...(baseline?[['before',baseline]]:[]),['fixed',root]]) {
   for(const [width,height,native] of [[320,568,false],[390,844,false],[844,390,false],[1400,900,false],[420,900,true]]) {
    if(version==='before'&&(native||width!==390))continue;
    const context=await browser.newContext({viewport:{width,height},hasTouch:!native&&width<900,isMobile:!native&&width<900,serviceWorkers:'block',permissions:['clipboard-read','clipboard-write']});
    if(native)await context.addInitScript(()=>{window.webkit={messageHandlers:{centro:{postMessage(){}}}};});
    let items=[{...item('local',null,'claude'),alive:false,operable:false,status:'waiting',project:'Signara'},item('signara','%21'),item('local','%0'),item('local','%1','claude')];
    let brainAccounts=accounts, brainHarness='codex', operationStatus=null;
    const page=await context.newPage(), errors=[], failures=[], posts=[];
    page.on('pageerror',e=>errors.push(e.message));
    const check=(x,label)=>{if(!x)failures.push(label);};
    await page.route('**/*',async route=>{
      const u=new URL(route.request().url()), p=u.pathname;
      const file=path.join(p.startsWith('/assets/')?root:dir,p.startsWith('/assets/')?'':'dash',p==='/'?'index.html':p);
      if(fs.existsSync(file)&&fs.statSync(file).isFile())return route.fulfill({body:fs.readFileSync(file),contentType:p.endsWith('.js')?'text/javascript':p.endsWith('.css')?'text/css':p.endsWith('.svg')?'image/svg+xml':'text/html'});
      let data={};
      if(p==='/term/')return route.fulfill({body:'<!doctype html><p>Disposable terminal fixture</p>',contentType:'text/html'});
      if(route.request().method()==='POST')posts.push({path:p,body:route.request().postDataJSON()});
      if(p==='/state')data=items;
      if(p==='/providers')data=registry;
      if(p==='/tabs')data=[{session:'local',label:'⌂ local',closable:false},{session:'signara',label:'Signara'}];
      if(p==='/active-tab')data={session:'local',pane:'%0',ts:1};
      if(p==='/webterm-token')data={token:'fixture'};
      if(p==='/proxy')data={alive:false};
      if(p==='/operator')data={id:'fixture',messages:[],conversations:[],models:[]};
      if(['/events','/ssh','/tab-history'].includes(p))data=[];
      if(p==='/tmux-mouse')data={mouse:'on'};
      if(p==='/session-brain')data={status:'detected',harness:brainHarness,accounts:brainAccounts,skills:[{name:'audit-skill',enabled:true,description:'Fixture skill'}],mcps:[{name:'audit-mcp',enabled:true,description:'Fixture server description',source:'project'}]};
      if(p==='/session-profiles')data={profiles:[],inventory:{skills:[],mcps:[]},capabilities:{}};
      if(p==='/extension-usage')data={items:[],note:'No attributed calls'};
      if(p==='/session/configure'){
        data={operationId:'op-'+posts.length,operationKey:'local|%0',queued:true};
        operationStatus={...data,state:'awaiting_confirmation',stage:'awaiting_confirmation',pending:true,confirmed:false,recoveryAllowed:true,handoffRequired:true,handoffPath:'/tmp/fixture-continuation.md',ts:1};
      }
      if(p==='/model/status')data=operationStatus||{stage:'waiting',stageCode:'waiting',operationKey:'local|%0'};
      return route.fulfill({json:data});
    });
    await page.goto(native?'http://localhost/':'https://remote.test/');
    await page.waitForFunction(()=>S.list?.length===4);
    if(!native)await page.evaluate(()=>{showView('term:local');showView('panel');});
    else await page.waitForFunction(()=>ACTIVE_TAB.session==='local');
    await page.evaluate(()=>{S.sel='signara|%21';S.selTs=Date.now();render(S.list);});
    check(await page.locator('#centro .cx-name').textContent()==='⌂ local','Local control card stays on selected session');
    check((await page.locator('#op-target').textContent()).includes('local'),'chat destination matches Local');
    if(!native){
      check(await page.locator('#btn-pomo').isVisible(),'remote Pomodoro is visible');
      if(await page.locator('#btn-pomo').isVisible()){
        await page.locator('#btn-pomo').click();
        check(await page.locator('#pomo-panel').isVisible(),'remote Pomodoro opens');
        await page.evaluate(()=>document.querySelector('#pomo-panel').classList.add('hidden'));
      }
    }
    if(version==='fixed') {
      const button=page.locator('#centro .motor-pill');
      check(await button.isVisible(),'AI configuration is visible');
      check(await button.isEnabled(),'native AI change works without proxy');
      await button.click();
      check(await page.locator('#motor-pop [name=toHarness]').isVisible(),'CLI control visible');
      check(await page.locator('#motor-pop [name=motor]').isVisible(),'engine control visible');
      check(await page.locator('#motor-pop [name=harnessAccount]').isVisible(),'account control visible');
      for(const route of registry.matrix){
        await page.locator('#motor-pop [name=toHarness]').selectOption(route.harness);
        await page.locator('#motor-pop [name=motor]').selectOption(route.motor);
        await page.locator('#motor-pop [name=effort]').selectOption('low');
        await page.locator('#motor-pop [name='+ (route.harness==='acp'?'motorAccount':'harnessAccount') +']').selectOption('work');
        check(await page.locator('#motor-pop .sc-apply').count()===1,'combined changes use one Apply');
        check(await page.locator('#motor-pop .sc-apply').isEnabled(),'valid route '+route.id+' can apply');
      }
      check(!posts.some(x=>x.path==='/session/configure'),'editing all routes does not mutate');
      await page.locator('#motor-pop .sc-apply').click();
      await page.waitForFunction(()=>MOTOR_PENDING.has('local|%0'));
      const config=posts.filter(x=>x.path==='/session/configure');
      check(config.length===1,'one request for combined change');
      check(config[0]?.body.session==='local'&&config[0]?.body.pane==='%0','request targets exact selected pane');
      check(config[0]?.body.expectedIdentity==='identity-%0'&&config[0]?.body.expectedConversationId==='thread-%0','request pins process and conversation');
      await page.evaluate(()=>renderCentro(S.list));
      check(await page.evaluate(()=>MOTOR_PENDING.has('local|%0')),'same observed model cannot clear queued account or effort change');
      await page.locator('#centro .motor-pill').click();
      await page.waitForSelector('#motor-pop [data-recover]');
      check(await page.locator('#motor-pop [data-copy-handoff]').isVisible(),'pending startup offers continuation prompt');
      check(await page.locator('#motor-pop .sc-apply').isDisabled(),'pending startup cannot queue conflicting configuration');
      await page.locator('#motor-pop [data-copy-handoff]').click();
      check((await page.evaluate(()=>navigator.clipboard.readText())).includes('/tmp/fixture-continuation.md'),'continuation prompt is copied without sending terminal input');
      await page.locator('#motor-pop .mp-close').click();
      operationStatus=null;
      await page.evaluate(()=>{MOTOR_PENDING.clear();showView('panel');S.sel='local|%1';S.selTs=Date.now()+10;render(S.list);});
      check(await page.locator('#centro .motor-pill').isEnabled(),'Claude native switch works when gateway down');
      await page.locator('#centro .motor-pill').click();
      await page.locator('#motor-pop .mp-close').click();
      await page.evaluate(()=>{S.sel='local|%0';S.selTs=Date.now()+20;render(S.list);});
      await page.locator('#centro .cx-more').click();
      await page.locator('#centro .cx-tabs [data-pane=mcps]').click();
      check((await page.locator('#centro .cx-mcps').textContent()).includes('Fixture server description'),'MCP description visible');
      await page.waitForSelector('#centro .cx-accs',{state:'attached'});
      brainAccounts=[accounts[0]];brainHarness='grok';
      items=items.map(x=>x.session==='local'&&x.pane==='%0'?{...x,agent:'grok',motor:'grok',model:'grok-4'}:x);
      await page.evaluate(list=>{brainCache.expires=0;render(list);},items);
      await page.waitForFunction(()=>!document.querySelector('#centro .cx-accs'));
      check(await page.locator('#centro .cx-accs').count()===0,'old provider account chips disappear');
      if(!native){
        items=items.map(x=>({...x,paneActive:x.pane==='%1'}));
        await page.evaluate(list=>{S.sel='local|%0';S.selTs=0;render(list);},items);
        check(await page.evaluate(()=>CENTRO_VIEW.item.pane)==='%1','remote terminal focus chooses actual split');
        check((await page.locator('#op-target').textContent()).includes('%1'),'chat follows actual remote split');
      }
      const overflow=await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth+1);
      check(!overflow,'no page horizontal overflow');
    }
    await page.screenshot({path:path.join(process.env.QA_OUTPUT||'/tmp',`session-audit-${version}-${width}-${native?'native':'remote'}.png`)});
    check(errors.length===0,'no JavaScript exceptions: '+errors.join('; '));
    console.log(JSON.stringify({version,width,height,native,failures}));
    await context.close();
    if(version==='fixed')assert.deepEqual(failures,[]);
    else assert(failures.length>0,'baseline must reproduce reported failures');
   }
  }
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
