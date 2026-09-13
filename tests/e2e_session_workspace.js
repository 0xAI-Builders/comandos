#!/usr/bin/env node
// Isolated headless dashboard; every API is a fixture and no live session is touched.
const assert=require('node:assert/strict'),http=require('node:http'),fs=require('node:fs'),path=require('node:path');
const {loadPlaywright}=require('./e2e_mobile_remote');
const root=path.resolve(__dirname,'..');
const registry={harnesses:{claude:{label:'Claude Code',accounts:[{alias:'main',selectable:true}]},codex:{label:'Codex',accounts:[{alias:'work',selectable:true,motorSelectable:true}]}},motors:{claude:{label:'Claude',models:[{id:'opus',name:'Opus',efforts:['high'],defaultEffort:'high'}]},codex:{label:'Codex',models:[{id:'gpt-6-astra',name:'Astra',efforts:['high','ultra'],defaultEffort:'high'}]}},matrix:[{id:'claude:claude',harness:'claude',motor:'claude',selectable:true},{id:'codex:codex',harness:'codex',motor:'codex',selectable:true}]};
const items=[{session:'test',pane:'%1',agent:'claude',motor:'claude',model:'opus',effort:'high',account:'main',cwd:'/tmp',project:'Proyecto',alive:true,status:'idle'},{session:'test',pane:'%2',agent:'codex',motor:'codex',model:'gpt-6-astra',effort:'ultra',account:'work',cwd:'/tmp',project:'Proyecto',alive:true,status:'working'}];
const mcpDescription='Busca documentación técnica. <img src=x onerror="window.mcpDescriptionExecuted=true">';
const mcps=[{id:'docs',name:'docs',description:mcpDescription,descriptionSource:'configuration',enabled:true,toggleable:true,scope:'user'},
  {id:'unknown',name:'unknown',description:'',descriptionSource:'unavailable',enabled:true,toggleable:true,scope:'user'}];
async function main(){
  const posts=[],errors=[];let profile;
  const server=http.createServer((req,res)=>{
    const url=new URL(req.url,'http://localhost');
    let file=path.join(root,'dash',url.pathname==='/'?'index.html':url.pathname);
    if(url.pathname.startsWith('/assets/'))file=path.join(root,url.pathname);
    if(fs.existsSync(file)&&fs.statSync(file).isFile()){
      res.setHeader('Content-Type',file.endsWith('.js')?'text/javascript':file.endsWith('.css')?'text/css':file.endsWith('.html')?'text/html':'application/octet-stream');res.end(fs.readFileSync(file));return;
    }
    let raw='';req.on('data',b=>raw+=b);req.on('end',()=>{
      const data=raw?JSON.parse(raw):{};if(req.method==='POST')posts.push({path:url.pathname,data});
      let out={};
      if(url.pathname==='/state')out=items;
      if(url.pathname==='/providers')out=registry;
      if(['/tabs','/tab-history','/events','/ssh'].includes(url.pathname))out=[];
      if(url.pathname==='/operator')out={id:'chat',messages:[],conversations:[],models:[]};
      if(url.pathname==='/session-brain')out={skills:[],mcps,accounts:[]};
      if(url.pathname==='/session/configure')out={ok:true,queued:true,operationKey:'test|%1'};
      if(url.pathname==='/session-profiles'){
        if(req.method==='POST'){profile={...data,id:'profile-test'};out={ok:true,profile};}
        else out={profiles:profile?[profile]:[],inventory:{skills:[{id:'design',name:'design-research',enabled:true,toggleable:true}],mcps},capabilities:{skills:{supported:true,reason:'Al iniciar'},mcps:{supported:true}}};
      }
      if(url.pathname==='/session-profile-apply')out={ok:true,launchDraft:{harness:'codex',motor:'codex',model:'gpt-6-astra',effort:'high',harnessAccount:'work',motorAccount:'work'}};
      if(url.pathname==='/extension-usage')out={extensions:[{kind:'skill',name:'design-research',count:3,durationMs:80}],provenance:'observed'};
      if(url.pathname==='/fs/dirs')out={dirs:[],path:'/tmp'};
      res.setHeader('Content-Type','application/json');res.end(JSON.stringify(out));
    });
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const browser=await loadPlaywright().chromium.launch({headless:true,executablePath:'/usr/bin/google-chrome',args:['--no-sandbox']});
  try{
    const page=await browser.newPage({viewport:{width:390,height:844},isMobile:true,hasTouch:true});
    page.setDefaultTimeout(10000);
    page.on('pageerror',e=>{errors.push(e.message);console.error('PAGE ERROR',e.message);});
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    await page.waitForFunction(()=>typeof SessionConfig==='object'&&typeof renderSessionConfig==='function');
    await page.locator('#op-in').fill('borrador á🙂');
    await page.locator('#toggle-chat').click();
    assert(await page.locator('#op-chat').isHidden());
    await page.reload();
    assert(await page.locator('#op-chat').isHidden());
    await page.locator('#toggle-chat').click();
    assert.equal(await page.locator('#op-in').inputValue(),'borrador á🙂');
    await page.evaluate(({registry,items})=>{PROVIDERS=registry;S.list=items;}, {registry,items});
    await page.locator('#toggle-overview').click();
    assert.equal(await page.locator('.overview-card').count(),2);
    await page.screenshot({path:'/tmp/comandos-overview-mobile.png'});
    await page.locator('.overview-card [data-config]').first().click();
    await page.locator('#motor-pop [name=toHarness]').selectOption('codex');
    assert.equal(posts.filter(p=>p.path==='/session/configure').length,0);
    assert(await page.locator('#motor-pop .sc-apply').isDisabled());
    await page.locator('#motor-pop [name=harnessAccount]').selectOption('work');
    await page.locator('#motor-pop [name=effort]').selectOption('ultra');
    assert.equal(await page.locator('#motor-pop .sc-apply').count(),1);
    assert.equal(await page.locator('#motor-pop .mp-cli-go').count(),0);
    await page.screenshot({path:'/tmp/comandos-selector-mobile.png'});
    await page.locator('#motor-pop .sc-apply').click();
    await page.waitForFunction(()=>!document.querySelector('#motor-pop').classList.contains('open'));
    const changes=posts.filter(p=>p.path==='/session/configure');assert.equal(changes.length,1);
    assert.equal(changes[0].data.harnessAccount,'work');assert.equal(changes[0].data.toHarness,'codex');assert.equal(changes[0].data.effort,'ultra');
    await page.locator('#open-session-profiles').click();
    await page.locator('.sc-extensions .mcp-description').first().waitFor();
    assert.equal(await page.locator('.sc-extensions .mcp-description').first().innerText(),mcpDescription);
    assert.match(await page.locator('.sc-extensions .mcp-description').last().innerText(),/no tiene una descripción/);
    assert.equal(await page.locator('.sc-extensions .mcp-description img').count(),0);
    await page.locator('[data-template]').click();
    await page.locator('[data-save]').click();
    await page.waitForFunction(()=>!document.querySelector('[data-launch]').disabled);
    await page.screenshot({path:'/tmp/comandos-profiles-mobile.png'});
    await page.locator('[data-launch]').click();
    await page.waitForFunction(()=>NS.profileId==='profile-test');
    assert.equal(posts.find(p=>p.path==='/session-profile-apply').data.cwd,'/tmp');
    await page.locator('#ns-cancel').click();
    await page.locator('#open-extension-usage').click();
    await page.waitForSelector('.sc-table');
    assert.match(await page.locator('.sc-table').innerText(),/design-research/);
    await page.screenshot({path:'/tmp/comandos-workspace-mobile.png'});
    await page.locator('[data-close-sc]').click();
    await page.setViewportSize({width:1440,height:960});
    await page.locator('.overview-card [data-config]').last().click();
    assert(await page.locator('#motor-pop').evaluate(el=>el.getBoundingClientRect().right<=innerWidth));
    await page.screenshot({path:'/tmp/comandos-workspace-desktop.png'});
    await page.evaluate(items=>render([...items,{session:'old',project:'Historial',status:'waiting',detail:'Pregunta conservada',alive:false,operable:false,agent:'claude'}]),items);
    const historical=page.locator('.row[data-rk="old"]');
    assert(await historical.locator('.up').isDisabled());
    assert(await historical.locator('.kill').isDisabled());
    assert.match(await historical.innerText(),/Pregunta conservada/);
    await page.locator('#motor-pop .mp-close').click();
    await page.evaluate(items=>{CENTRO_VIEW.item=items[0];renderCentro(items);},items);
    await page.locator('#centro .cx-more').click();
    await page.locator('#centro .cx-tabs [data-pane=mcps]').click();
    await page.locator('.cx-row .mcp-description').first().waitFor();
    assert.equal(await page.locator('.cx-row .mcp-description').first().innerText(),mcpDescription);
    assert.equal(await page.locator('.cx-row .mcp-description img').count(),0);
    assert.equal(await page.evaluate(()=>!!window.mcpDescriptionExecuted),false);
    items[0]={...items[0],model:'gpt-6-astra',motor:'codex',effort:'ultra',
      observedConfig:{model:'gpt-6-astra',effort:'ultra',confirmed:true},configConfirmed:true};
    await page.evaluate(()=>tick());
    await page.waitForFunction(()=>document.querySelector('#centro .cx-model')?.textContent.includes('ultra'));
    assert.match(await page.locator('.overview-card[data-key="test|%1"]').innerText(),/gpt-6-astra.*ultra/);
    items[0]={...items[0],model:'opus',motor:'claude',effort:'',observedConfig:{model:'opus',effort:'',confirmed:true}};
    await page.evaluate(()=>{PROXY={...(PROXY||{}),alive:true,sessionEffort:{'test|%1':'ultra'}};});
    await page.evaluate(()=>tick());
    await page.waitForFunction(()=>!document.querySelector('#centro .cx-model')?.textContent.includes('ultra'));
    assert.doesNotMatch(await page.locator('#centro .motor-pill').innerText(),/ultra/);
    assert.deepEqual(errors,[]);
    console.log('PASS: mobile chat persistence, overview, single combined switch, profiles launch, observed extension analytics; no page errors.');
  }finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
}
main().catch(e=>{console.error(e);process.exitCode=1;});
