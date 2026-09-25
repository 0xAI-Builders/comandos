// Mac-only full-page regression: real HTML, CSS, xterm and polling, isolated APIs.
// PLAYWRIGHT_CORE=/path/to/playwright-core node tests/e2e_remote_workspace.cjs [baseline]
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const chrome = '/home/john/.local/share/comandos-browser/runtime/bin/chrome';
assert(fs.existsSync(chrome), 'Run browser tests on the Mac host.');
const {chromium} = require(process.env.PLAYWRIGHT_CORE || 'playwright-core');
const root = path.resolve(__dirname, '..');
const extensionsOnly = process.argv.includes('--extensions-only');
const baseline = extensionsOnly ? null : process.argv[2];
const output = process.env.QA_OUTPUT || '/tmp/comandos-parity-qa/shots';
fs.mkdirSync(output, {recursive:true});
const registry = {harnesses:{codex:{label:'Codex',accounts:[{alias:'main',selectable:true}]}},
  motors:{codex:{label:'Codex',models:[{id:'gpt-6-astra',name:'GPT',efforts:['high']}]}},
  matrix:[{id:'codex:codex',harness:'codex',motor:'codex',selectable:true}]};
const initialTabs = [{session:'local',label:'⌂ local',closable:false},
  ...Array.from({length:20},(_,i)=>({session:'s'+i,label:'Session '+i}))];
const items = initialTabs.flatMap((t,i)=>[0,1].map(p=>({
  session:t.session,pane:'%'+(i*2+p),project:t.label,agent:'codex',motor:'codex',
  model:'gpt-6-astra',effort:'high',account:'main',cwd:'/tmp/fixture',alive:true,
  status:p?'waiting':'working',detail:'Fixture',ts:Math.floor(Date.now()/1000),
})));
async function checkExtensionShelf(page, check, cdp) {
  await page.evaluate(()=>showView('term:local',true));
  const termBefore=await page.locator('#term-area').boundingBox();
  const pillBefore=await page.locator('.cx-motor').first().innerHTML();
  await page.evaluate(()=>openPaneExtensions('local','%0','codex'));
  const shelf=page.frameLocator('#pane-extensions-frame');
  await shelf.getByRole('button',{name:'Cerrar estante',exact:true}).waitFor();
  const term=await page.locator('#term-area').boundingBox(),frame=await page.locator('#pane-extensions-frame').boundingBox();
  check(term.y+term.height<=frame.y+1,'shelf resizes terminal without covering input');
  check(await page.locator('.cx-motor').first().innerHTML()===pillBefore,'shelf preserves provider/model/effort pill');
  await cdp.send('Emulation.setPageScaleFactor',{pageScaleFactor:1.5});
  await page.waitForTimeout(500);
  const visible=await page.evaluate(()=>({top:visualViewport.offsetTop,height:visualViewport.height}));
  const zoomFrame=await page.locator('#pane-extensions-frame').boundingBox();
  check(zoomFrame.y+zoomFrame.height<=visible.top+visible.height+2,'shelf fits current visible viewport after zoom');
  await cdp.send('Emulation.setPageScaleFactor',{pageScaleFactor:1});await page.waitForTimeout(300);
  await shelf.getByRole('button',{name:'Cerrar estante',exact:true}).click();
  await page.waitForFunction(()=>!document.getElementById('pane-extensions-frame'));
  const restored=await page.locator('#term-area').boundingBox();
  check(Math.abs(restored.height-termBefore.height)<2,'closing shelf restores terminal height');
}
(async()=>{
  const browser = await chromium.launch({headless:true,executablePath:chrome,chromiumSandbox:true});
  try {
    for(const [variant,dir] of [...(baseline?[['before',baseline]]:[]),['fixed',root]]) {
      const matrix=variant==='before'?[[390,844,true],[1400,900,false]]:
        [[320,568,true],[390,844,true],[844,390,true],[1400,900,false]];
      for(const [width,height,touch] of matrix){
        const name=`${variant}-${width}x${height}`, errors=[], failures=[];
        const check=(value,message)=>{if(!value)failures.push(message);};
        let tabs=initialTabs.slice(), favorites=[], rejectFavorite=false;
        const context=await browser.newContext({viewport:{width,height},hasTouch:touch,isMobile:touch,
          serviceWorkers:'block',permissions:['clipboard-read','clipboard-write']});
        await context.addInitScript(()=>{
          window.__sent=[];window.__sockets=[];
          window.WebSocket=class extends EventTarget{
            constructor(){super();this.readyState=0;__sockets.push(this);setTimeout(()=>{
              this.readyState=1;this.dispatchEvent(new Event('open'));
              setTimeout(()=>this.dispatchEvent(new MessageEvent('message',{data:'0Fixture terminal output\r\n'})),100);
            },0);}
            send(data){if(data instanceof Uint8Array)__sent.push([...data]);}
            close(){this.readyState=3;this.dispatchEvent(new Event('close'));}
          };
        });
        const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));
        await page.route('**/*',async route=>{
          const p=new URL(route.request().url()).pathname;
          let file=p==='/term/'?path.join(dir,'dash/term.html'):
            path.join(p.startsWith('/assets/')?root:dir,p.startsWith('/assets/')?'':'dash',p==='/'?'index.html':p);
          if(fs.existsSync(file)&&fs.statSync(file).isFile())return route.fulfill({body:fs.readFileSync(file),
            contentType:file.endsWith('.js')?'text/javascript':file.endsWith('.css')?'text/css':
              file.endsWith('.svg')?'image/svg+xml':file.endsWith('.html')?'text/html':'application/octet-stream'});
          let data={};
          if(p==='/state')data=items;
          if(p==='/providers')data=registry;
          if(p==='/pane-extensions')data={ok:true,identity:'fixture-pane',conversationId:'exact',harness:'codex',inventory:{mcps:[],skills:[]},desired:{mcps:{},skills:{}},loaded:null,revision:1,operation:null,usage:{counts:{},complete:false},templates:[],busy:false,applySupported:true};
          if(p==='/tabs')data=tabs;
          if(p==='/prefs')data={favorites};
          if(p==='/prefs-set'){
            if(rejectFavorite)return route.fulfill({status:503,json:{error:'Favorite save failed'}});
            const patch=route.request().postDataJSON().favorite;
            if(patch){
              favorites=favorites.filter(s=>s!==patch.session);
              if(patch.enabled)favorites.push(patch.session);
            }
            data={ok:true,favorites};
          }
          if(p==='/active-tab')data={session:'local',pane:'%0'};
          if(['/events','/ssh','/tab-history'].includes(p))data=[];
          if(p==='/operator')data={id:'fixture',messages:[],conversations:[],models:[]};
          if(p==='/session-brain')data={skills:[],mcps:[],accounts:[]};
          if(p==='/webterm-token')data={token:'fixture-token'};
          if(p==='/tmux-mouse')data={mouse:'on'};
          if(p==='/terminal-history')data={text:'SELECT_THIS_WORD\nSecond pane',pane:'%0',panes:[{id:'%0',title:'Codex'},{id:'%1',title:'Claude'}]};
          return route.fulfill({json:data});
        });
        await page.goto('https://remote.test/');
        await page.waitForFunction(()=>openTerms.size===21);
        await page.evaluate(()=>showView('term:local',true));
        await page.waitForFunction(()=>!!openTerms.get('local').frame?.contentDocument.querySelector('.xterm-screen'));
        await page.waitForTimeout(700);
        if(extensionsOnly){
          const cdp=await context.newCDPSession(page);
          await checkExtensionShelf(page,check,cdp);
          check(errors.length===0,'no page errors: '+errors.join('; '));
          console.log(JSON.stringify({name,scope:'pane-extensions',failures,errors}));
          await context.close();assert.deepEqual(failures,[],name);continue;
        }
        if(variant==='fixed'&&touch){
          const terminal=page.frames().find(f=>f.url().includes('/term/'));
          const sent=()=>terminal.evaluate(()=>window.__sent.map(bytes=>new TextDecoder().decode(new Uint8Array(bytes).slice(1))));
          const erase=terminal.locator('[data-key="backspace"]');
          check(await erase.isVisible(),'remote backspace is visible');
          const bounds=await erase.boundingBox();
          check(bounds.width>=44&&bounds.height>=44&&bounds.x+bounds.width<=width,'backspace fits the first toolbar controls');
          await terminal.locator('#mobile-draft').fill('Borrador sin enviar');
          const before=await sent();
          await erase.tap();
          check(JSON.stringify((await sent()).slice(before.length))===JSON.stringify(['\x7f']),'one tap sends one terminal Backspace');
          check(await terminal.locator('#mobile-draft').inputValue()==='Borrador sin enviar','terminal erase preserves local draft');
          await page.evaluate(()=>showView('term:s0',true));
          await page.waitForFunction(()=>!!openTerms.get('s0').frame?.contentDocument.querySelector('.xterm-screen'));
          const other=page.frames().find(f=>f!==terminal&&f.url().includes('/term/'));
          const otherBefore=await other.evaluate(()=>window.__sent.length);
          await page.evaluate(()=>showView('term:local',true));
          const afterSwitch=await sent();
          await erase.tap();
          check(JSON.stringify((await sent()).slice(afterSwitch.length))===JSON.stringify(['\x7f']),'backspace still reaches original terminal after switching tabs');
          check(await other.evaluate(()=>window.__sent.length)===otherBefore,'backspace never edits the background session');
          await terminal.evaluate(()=>window.__sockets.at(-1).readyState=3);
          const offline=await sent();
          await erase.tap();
          check((await sent()).length===offline.length,'offline backspace is not queued for replay');
          await terminal.evaluate(()=>window.__sockets.at(-1).readyState=1);
          await terminal.locator('#mobile-draft').fill('');
        }
        const home=page.locator('.apptab').filter({hasText:'⌂ local'});
        check(await home.count()===1,'one LOCAL tab');
        check(await home.locator('.x').count()===0,'LOCAL cannot close');
        const geometry=await page.evaluate(()=>{
          const bar=document.getElementById('tabbar'),term=document.getElementById('term-area');
          return {width:document.documentElement.scrollWidth,height:document.documentElement.scrollHeight,
            header:term.getBoundingClientRect().y,barHeight:bar.getBoundingClientRect().height,
            scrollbar:getComputedStyle(bar).scrollbarWidth,activeAfter:getComputedStyle(document.querySelector('.apptab.on'),'::after').content};
        });
        check(geometry.width===width && geometry.height===height,'layout stays in viewport');
        check(geometry.barHeight===44 && geometry.header===44,'fixed tab row');
        check(geometry.scrollbar==='none' && geometry.activeAfter==='none','one active line, no second scrollbar strip');
        await page.evaluate(()=>{window.__labelNode=document.querySelector('.apptab.on .lbl').firstChild;window.__frame=openTerms.get('local').frame;});
        await page.evaluate(()=>toast('Modelos nuevos detectados — aviso de prueba que debe dejar libres las pestañas.'));
        const bar=await page.locator('#tabbar').boundingBox(),cdp=await context.newCDPSession(page);
        check(await page.locator('.toast').last().evaluate(e=>e.getBoundingClientRect().top>=44),'notices stay below session tabs');
        check(await page.evaluate(({x,y})=>!!document.elementFromPoint(x,y)?.closest('#tabbar'),{x:bar.x+bar.width/2,y:bar.y+22}),'notices never intercept tab gestures');
        if(touch){
          const start=bar.x+bar.width-12, distance=Math.min(220,bar.width-24),y=bar.y+22;
          await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x:start,y,id:1}]});
          for(let i=1;i<=10;i++){
            await cdp.send('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:start-distance*i/10,y,id:1}]});
            await page.evaluate(()=>renderTabbar());await page.waitForTimeout(25);
          }
          await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
        }else{await page.mouse.move(bar.x+bar.width/2,bar.y+22);await page.mouse.wheel(400,0);}
        await page.waitForTimeout(800);
        const scroll=await page.locator('#tabbar').evaluate(e=>e.scrollLeft);
        check(scroll>80,'gesture scrolls sessions');
        await page.evaluate(async()=>{for(let i=0;i<4;i++){applyAppLayout();await loadDesktopTabs();}});
        await page.waitForTimeout(5500);
        check(Math.abs(await page.locator('#tabbar').evaluate(e=>e.scrollLeft)-scroll)<2,'polls/layout updates preserve horizontal scroll');
        check(await page.evaluate(()=>window.__labelNode===document.querySelector('.apptab.on .lbl').firstChild),'polls preserve touch target text node');
        check(await page.evaluate(()=>window.__frame===openTerms.get('local').frame),'polls keep terminal iframe');
        tabs=[tabs[0],...tabs.slice(1).reverse()];
        await page.evaluate(()=>loadDesktopTabs());
        check(await page.evaluate(()=>[...openTerms.keys()][1])==='s19','desktop tab reorder is mirrored');
        check(await page.evaluate(()=>window.__frame===openTerms.get('local').frame),'reorder keeps terminal iframe');
        if(variant==='fixed'){
          const tabOrder=()=>page.locator('#tabbar > .apptab').evaluateAll(els=>els.map(e=>e.dataset.tabKey));
          const star=session=>page.locator(`[data-tab-key="term:${session}"] .tab-fav`);
          check(await home.locator('.tab-fav').count()===0,'LOCAL has no favorite control');
          // Real pointer and keyboard actions must not select/close the tab.
          await star('s17').click();
          await page.waitForFunction(()=>!favoritePending.size);
          check((await tabOrder()).slice(0,3).join(',')==='term:local,term:s17,term:s19','favorite moves directly after home');
          check(await star('s17').getAttribute('aria-pressed')==='true','star reflects saved favorite');
          await star('s19').focus();await page.keyboard.press('Space');
          await page.waitForFunction(()=>!favoritePending.size);
          check((await tabOrder()).slice(0,3).join(',')==='term:local,term:s19,term:s17','multiple favorites retain desktop order');
          check(await page.evaluate(()=>activeTerm)==='local','favorite keyboard/click actions preserve active session');
          check(await page.evaluate(()=>window.__frame===openTerms.get('local').frame),'favorites preserve terminal iframe');
          await star('s19').click();await page.waitForFunction(()=>!favoritePending.size);
          check((await tabOrder())[1]==='term:s17','unfavorite returns tab to remaining group');
          rejectFavorite=true;
          await star('s19').click();await page.waitForFunction(()=>!favoritePending.size);
          check(await star('s19').getAttribute('aria-pressed')==='false','failed save rolls back favorite');
          check((await tabOrder())[1]==='term:s17','failed save rolls back order');
          check(await page.locator('.toast.err').count()>0,'failed save is visible');
          rejectFavorite=false;
          // Another client changes favorites; the existing state loop must sync.
          favorites=['s18'];
          await page.waitForFunction(()=>S.favs.has('s18')&&!S.favs.has('s17'),null,{timeout:10000});
          check((await tabOrder())[1]==='term:s18','external favorite syncs');
          await page.reload();await page.waitForFunction(()=>openTerms.size===21&&S.favs.has('s18'));
          check((await tabOrder()).slice(0,2).join(',')==='term:local,term:s18','favorites survive reload with home first');
        }
        if(touch){
          const beforeZoom=await page.locator('#panes').evaluate(e=>e.clientHeight);
          await cdp.send('Emulation.setPageScaleFactor',{pageScaleFactor:1.5});await page.waitForTimeout(500);
          const visibleHeight=await page.evaluate(()=>visualViewport.height);
          check(await page.locator('#panes').evaluate(e=>e.clientHeight)<=visibleHeight+1,'pinch fits layout into visible viewport');
          await cdp.send('Emulation.setPageScaleFactor',{pageScaleFactor:1});await page.waitForTimeout(300);
        }
        await page.evaluate(()=>showView('term:local',true));
        if(variant==='fixed'){
          await checkExtensionShelf(page,check,cdp);
        }
        await page.screenshot({path:path.join(output,name+'.png')});
        check(errors.length===0,'no page errors: '+errors.join('; '));
        console.log(JSON.stringify({name,geometry,scroll,failures,errors}));
        await context.close();
        if(variant==='fixed')assert.deepEqual(failures,[],name);
        else assert(failures.length>0,'baseline reproduces the layout/parity defects');
      }
    }
  }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
