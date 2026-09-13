// Run only on the Mac browser host with PLAYWRIGHT_CORE set if necessary.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {chromium} = require(process.env.PLAYWRIGHT_CORE || 'playwright-core');
const root = path.resolve(__dirname, '..');
const chrome = '/home/john/.local/share/comandos-browser/runtime/bin/chrome';
assert(fs.existsSync(chrome), 'Browser verification must run on the Mac.');
(async () => {
  const browser = await chromium.launch({executablePath:chrome, headless:true, chromiumSandbox:true});
  try {
    for (const [width,height,touch] of [[320,568,true],[390,844,true],[844,390,true],[1400,900,false]]) {
      const context = await browser.newContext({viewport:{width,height},hasTouch:touch,isMobile:touch,
        permissions:['clipboard-read','clipboard-write'],serviceWorkers:'block'});
      const page = await context.newPage(), errors=[], requests=[];
      page.on('pageerror', error => errors.push(error.message));
      let panes=[{id:'%0',index:0,title:'Codex',active:true,identity:'original-0'},
        {id:'%1',index:1,title:'Claude',active:false,identity:'original-1'}];
      let failClose=false;
      await context.addInitScript(() => {
        window.__sent=[];
        window.WebSocket=class extends EventTarget {
          constructor(){super();window.__socket=this;this.readyState=0;setTimeout(()=>{
            this.readyState=1;this.dispatchEvent(new Event('open'));
            this.dispatchEvent(new MessageEvent('message',{data:'0Terminal fixture\r\n'}));
          },0);}
          send(data){if(data instanceof Uint8Array)__sent.push([...data]);}
          close(){this.readyState=3;this.dispatchEvent(new Event('close'));}
        };
      });
      await page.route('**/*', async route => {
        const url=new URL(route.request().url());
        if(url.pathname==='/terminal-history') {
          const data=route.request().postDataJSON();
          const pane=data.pane || panes.find(p=>p.active)?.id;
          return route.fulfill({json:{ok:true,pane,panes,text:`${pane} title\nfirst line\nA🙂B\nlast line`}});
        }
        if(url.pathname==='/terminal-panes') {
          const data=route.request().postDataJSON();requests.push(data);
          if(data.action==='close' && failClose) return route.fulfill({status:409,json:{error:'El panel cambió'}});
          if(data.action==='close') panes=panes.filter(p=>p.id!==data.pane);
          if(data.action==='select') panes=panes.map(p=>({...p,active:p.id===data.pane}));
          return route.fulfill({json:{ok:true,panes,closed:data.action==='close'?data.pane:undefined}});
        }
        const file=url.pathname==='/term/'?path.join(root,'dash/term.html'):path.join(root,url.pathname);
        if(fs.existsSync(file) && fs.statSync(file).isFile())return route.fulfill({body:fs.readFileSync(file),
          contentType:file.endsWith('.html')?'text/html':file.endsWith('.js')?'text/javascript':file.endsWith('.css')?'text/css':'application/octet-stream'});
        return route.fulfill({status:404,body:'missing fixture'});
      });
      const tap = async locator => touch ? locator.tap() : locator.click();
      try {
        await page.goto('https://remote.test/term/?arg=fixture&auth=fixture-token');
        await page.waitForSelector('.xterm-screen');
        const draft=page.locator('#mobile-draft');
        if(touch)await draft.fill('conservar mi borrador');
        await tap(page.locator('[data-action="mode"]'));
        await page.waitForFunction(()=>document.getElementById('history-text').value.includes('last line'));
        const selected=()=>page.locator('#history-text').evaluate(el=>el.value.slice(el.selectionStart,el.selectionEnd));
        assert.equal(await selected(),'last line');
        const before=await page.evaluate(()=>__sent.length);
        await tap(page.locator('[data-selection-key="left"]'));
        assert.equal(await selected(),'last lin');
        await page.locator('#selection-edge').selectOption('start');
        await tap(page.locator('[data-selection-key="up"]'));
        assert.equal(await selected(),'A🙂B\nlast lin');
        await tap(page.locator('#selection-copy'));
        assert.equal(await page.evaluate(()=>navigator.clipboard.readText()),'A🙂B\nlast lin');
        assert.equal(await page.evaluate(()=>__sent.length),before,'selection controls send no terminal input');
        const stable=await selected();
        await page.evaluate(()=>__socket.dispatchEvent(new MessageEvent('message',{data:'0fresh output\r\n'})));
        await page.waitForTimeout(100);
        assert.equal(await selected(),stable,'new terminal output preserves selected text');
        await tap(page.locator('#selection-all'));
        assert((await selected()).startsWith('%0 title'));
        await page.locator('#history-pane').selectOption('%1');
        await page.waitForFunction(()=>document.getElementById('history-text').value.startsWith('%1'));
        const geometry=await page.locator('#selection-toolbar').boundingBox();
        assert(geometry.y+geometry.height<=height+1 && geometry.width<=width,'selection controls fit bottom of viewport');
        assert.equal(await page.evaluate(()=>visualViewport.scale),1,'selection does not zoom the viewport');
        await page.screenshot({path:`/tmp/comandos-pane-controls-qa/selection-${width}.png`});
        await tap(page.locator('#selection-close'));
        if(touch)assert.equal(await draft.inputValue(),'conservar mi borrador');
        await tap(page.locator('[data-action="panes"]'));
        await page.waitForSelector('.pane-option');
        await tap(page.getByRole('button',{name:'Panel 2 · Claude',exact:true}));
        assert.deepEqual(requests.at(-1),{session:'fixture',action:'select',pane:'%1',identity:'original-1'});
        await page.waitForFunction(()=>!document.getElementById('pane-dialog').open);
        await tap(page.locator('[data-action="panes"]'));
        await tap(page.getByRole('button',{name:'Cerrar Panel 1 · Codex',exact:true}));
        assert.equal(requests.filter(r=>r.action==='close').length,0,'opening confirmation does not close anything');
        await tap(page.locator('#pane-cancel'));
        assert.equal(requests.filter(r=>r.action==='close').length,0,'cancel preserves panes');
        await tap(page.getByRole('button',{name:'Cerrar Panel 1 · Codex',exact:true}));
        // Another client changes focus while this exact target is confirmed.
        panes=panes.map(p=>({...p,active:p.id==='%1'}));
        await page.screenshot({path:`/tmp/comandos-pane-controls-qa/close-${width}.png`});
        await tap(page.locator('#pane-close-confirm'));
        await page.waitForFunction(()=>document.querySelectorAll('.pane-option').length===1);
        assert.deepEqual(requests.at(-1),{session:'fixture',action:'close',pane:'%0',identity:'original-0'});
        assert(await page.locator('.pane-option .danger').isDisabled(),'last split cannot close');
        await tap(page.locator('#pane-dismiss'));
        panes.push({id:'%2',index:2,title:'New pane',active:false,identity:'new-2'});failClose=true;
        await tap(page.locator('[data-action="panes"]'));
        await tap(page.getByRole('button',{name:'Cerrar Panel 2 · New pane',exact:true}));
        await tap(page.locator('#pane-close-confirm'));
        await page.waitForFunction(()=>document.getElementById('pane-status').textContent.includes('El panel cambió'));
        assert.equal(panes.length,2,'failed close preserves both panes');
        assert.deepEqual(errors,[]);
        console.log(JSON.stringify({width,height,touch,passed:true,requests:requests.length}));
      } catch(error) {
        await page.screenshot({path:`/tmp/comandos-pane-controls-qa/failure-${width}.png`});
        throw error;
      } finally {await context.close();}
    }
  } finally {await browser.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
