// Run on the Mac browser host: node tests/e2e_remote_tab_selection.mjs [baseline-root].
// Fixtures use real dashboard CSS/controllers and xterm; no live sessions.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawn} from 'node:child_process';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const chromePath = '/home/john/.local/share/comandos-browser/runtime/bin/chrome';
assert.ok(fs.existsSync(chromePath), 'Run this test on the Mac browser host, never start a local browser.');
const variants = {fixed:root};
if (process.argv[2]) variants.before = path.resolve(process.argv[2]);
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
function fn(source, name) {
  const start = source.indexOf(`function ${name}(`), brace = source.indexOf('{', start);
  assert.ok(start >= 0, name);
  let depth = 0;
  for (let i=brace; i<source.length; i++) {
    if (source[i] === '{') depth++;
    if (source[i] === '}' && --depth === 0) return source.slice(start, i+1);
  }
  throw new Error(`Unclosed ${name}`);
}
const server = http.createServer((req,res) => {
  const url = new URL(req.url, 'http://localhost');
  if (url.pathname === '/terminal-history') {
    let body=''; req.on('data', chunk => body += chunk);
    req.on('end', () => {
      const input=JSON.parse(body), pane=input.pane || '%2';
      res.setHeader('Content-Type','application/json');
      res.end(JSON.stringify({ok:true,pane,panes:[{id:'%1',title:'Codex'},{id:'%2',title:'Claude'}],
        text:Array.from({length:300},(_,i)=>`SELECT_ME_NATIVE ${pane} line ${i} café repeated repeated`).join('\n')}));
    }); return;
  }
  const [variant, ...parts] = url.pathname.slice(1).split('/');
  const sourceRoot=variants[variant];
  if (!sourceRoot) {res.writeHead(404);res.end();return;}
  if (parts.join('/') === 'tabs') {
    const source=fs.readFileSync(path.join(sourceRoot,'dash/index.html'),'utf8');
    const styles=[...source.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g)].map(x=>x[1]).join('\n');
    res.setHeader('Content-Type','text/html');
    res.end(`<!doctype html><meta name="viewport" content="width=device-width, initial-scale=1"><style>${styles}
      body.app #tabbar{position:relative;width:100%;box-sizing:border-box}</style>
      <body class="app"><div id="tabbar"></div><script>
      let activeView='term:s0',activeTerm='s0',tabsPollTs=0;
      const openTerms=new Map(Array.from({length:20},(_,i)=>['s'+i,{label:'Session '+i}]));
      const S={list:[...openTerms.keys()].map(session=>({session,alive:true,status:'waiting'}))};
      const tf=x=>x,mdEsc=x=>x;function swOpen(){}function askCloseTab(){}
      function showView(key){activeView=key;renderTabbar();revealActiveTab(document.getElementById('tabbar'));}
      ${fn(source,'renderTabbar')}\n${fn(source,'revealActiveTab')}\n${source.includes('function updateTabNavigation(')?fn(source,'updateTabNavigation'):''}
      renderTabbar();</script>`); return;
  }
  const relative=parts.join('/');
  // Both variants share the unchanged bundled assets.
  const file=path.resolve(relative.startsWith('assets/')?root:sourceRoot,relative);
  if (!file.startsWith(root+path.sep) && !file.startsWith(sourceRoot+path.sep)) {res.writeHead(403);res.end();return;}
  fs.readFile(file,(error,data)=>{
    if(error){res.writeHead(404);res.end();return;}
    res.setHeader('Content-Type',file.endsWith('.html')?'text/html':file.endsWith('.js')?'text/javascript':file.endsWith('.css')?'text/css':'application/octet-stream');
    res.end(data);
  });
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const origin=`http://127.0.0.1:${server.address().port}`;
const dir=fs.mkdtempSync('/tmp/comandos-tab-selection-');
const chrome=spawn(chromePath,['--remote-debugging-port=0','--remote-debugging-address=127.0.0.1',`--user-data-dir=${dir}/profile`,'--no-first-run','about:blank'],{stdio:'ignore'});
let ws,seq=0;const pending=new Map(),errors=[];
const send=(method,params={})=>new Promise((resolve,reject)=>{
  const id=++seq,timer=setTimeout(()=>{pending.delete(id);reject(new Error('Timeout '+method));},15000);
  pending.set(id,{resolve,reject,timer});ws.send(JSON.stringify({id,method,params}));
});
const evaluate=async expression=>{
  const r=await send('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});
  if(r.exceptionDetails)throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text);
  return r.result.value;
};
async function until(expression){
  for(let i=0;i<100;i++){if(await evaluate(expression))return;await delay(100);}
  throw new Error('Did not settle: '+expression);
}
async function tap(selector){
  const p=await evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});e.scrollIntoView({block:'nearest',inline:'nearest'});const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()`);
  await send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{...p,id:1}]});
  await delay(80);await send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
}
try{
  let port;
  for(let i=0;i<100;i++){try{port=fs.readFileSync(`${dir}/profile/DevToolsActivePort`,'utf8').split('\n')[0];break;}catch{}await delay(100);}
  assert.ok(port,'Remote Chrome started');
  const pages=await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  ws=new WebSocket(pages.find(p=>p.type==='page').webSocketDebuggerUrl);
  ws.onmessage=e=>{
    const m=JSON.parse(e.data),p=pending.get(m.id);
    if(p){pending.delete(m.id);clearTimeout(p.timer);m.error?p.reject(new Error(JSON.stringify(m.error))):p.resolve(m.result);}
    if(m.method==='Runtime.exceptionThrown')errors.push(m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text);
  };
  await new Promise((resolve,reject)=>{ws.onopen=resolve;ws.onerror=reject;});
  await send('Page.enable');await send('Runtime.enable');
  await send('Browser.grantPermissions',{origin,permissions:['clipboardReadWrite','clipboardSanitizedWrite']});
  await send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});
  await send('Emulation.setTouchEmulationEnabled',{enabled:true});
  await send('Page.addScriptToEvaluateOnNewDocument',{source:`
    window.__sent=[];window.__sockets=[];
    window.WebSocket=class extends EventTarget{
      constructor(){super();this.readyState=0;window.__sockets.push(this);setTimeout(()=>{this.readyState=1;this.dispatchEvent(new Event('open'));setTimeout(()=>{this.dispatchEvent(new MessageEvent('message',{data:'2{"fontSize":11}'}));this.dispatchEvent(new MessageEvent('message',{data:'0SELECT_ME_NATIVE\\r\\nSecond pane output\\r\\n'}));},100);},0);}
      send(data){if(data instanceof Uint8Array)window.__sent.push(Array.from(data));}
      close(){this.readyState=3;this.dispatchEvent(new Event('close'));}
    };`});
  const results={};
  for(const variant of Object.keys(variants).reverse()){
    await send('Page.navigate',{url:`${origin}/${variant}/tabs`});
    await until(`document.querySelectorAll('.apptab').length===${variant==='fixed'?20:23}`);await delay(200);
    const lines=await evaluate(`(()=>{const e=document.querySelector('.apptab.on'),s=getComputedStyle(e),p=getComputedStyle(e,'::after');return Number(parseFloat(s.borderBottomWidth)>0&&s.borderBottomColor!=='rgba(0, 0, 0, 0)')+Number(p.content!=='none'&&p.content!=='normal'&&parseFloat(p.height)>0);})()`);
    await send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x:340,y:22,id:1}]});
    for(let i=1;i<=10;i++){await send('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:340-i*30,y:22,id:1}]});await delay(25);}
    await send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await delay(700);
    const scrolled=await evaluate(`window.__tab=document.querySelector('.apptab.on');document.getElementById('tabbar').scrollLeft`);
    assert.ok(scrolled>100,'Real touch swipe moved tab bar');
    await evaluate(`for(let i=0;i<10;i++){S.list[0].status=i%2?'working':'waiting';renderTabbar();}`);await delay(150);
    const after=await evaluate(`({left:document.getElementById('tabbar').scrollLeft,same:window.__tab===document.querySelector('.apptab.on')})`);
    if(variant==='fixed'){assert.equal(lines,1);assert.equal(after.left,scrolled);assert.equal(after.same,true);
      await evaluate(`showView('term:s19')`);await delay(100);
      assert.ok(await evaluate(`(()=>{const a=document.querySelector('.apptab.on').getBoundingClientRect(),b=document.getElementById('tabbar').getBoundingClientRect();return a.left>=b.left-1&&a.right<=b.right+1;})()`),'Explicit selection reveals tab');
    }else{assert.equal(lines,2);assert.equal(after.same,false);assert.ok(after.left<scrolled);}
    await send('Page.navigate',{url:`${origin}/${variant}/dash/term.html?arg=fixture&auth=fixture-token`});
    await until(`!!document.querySelector('.xterm-screen') && !document.getElementById('mobile-compose').hidden`);await delay(900);
    const disabled=await evaluate(`document.querySelector('[data-action="mode"]').disabled`);
    assert.equal(disabled,variant!=='fixed');
    await tap(variant==='fixed'?'[data-action="mode"]':'[data-action="history"]');
    await until(`document.getElementById('history-text').textContent.includes('line 299')`);
    const typography=await evaluate(`(()=>{const s=getComputedStyle(document.getElementById('history-text'));return {font:s.fontSize,wrap:s.whiteSpace,scale:visualViewport.scale};})()`);
    if(variant==='fixed'){
      assert.deepEqual(typography,{font:'11px',wrap:'pre',scale:1});
      const point=await evaluate(`(()=>{const e=document.getElementById('history-text');e.scrollTop=0;
        window.__touch=[];for(const type of ['touchstart','touchmove','touchend'])document.addEventListener(type,event=>__touch.push({type,prevented:event.defaultPrevented}),{passive:true});
        const range=document.createRange();range.setStart(e.firstChild,2);range.setEnd(e.firstChild,3);const r=range.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()`);
      await send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{...point,id:2}]});await delay(900);
      await send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await delay(200);
      assert.ok((await evaluate(`window.__touch`)).every(e=>!e.prevented),'Native selection gestures are never canceled');
      // Linux headless Chrome does not provide Android/iOS selection handles
      // (even a plain pre cannot select by touch hold). Exercise its native
      // text selection with pointer input; never manufacture a Selection range.
      await send('Input.dispatchMouseEvent',{type:'mousePressed',...point,button:'left',clickCount:2});
      await send('Input.dispatchMouseEvent',{type:'mouseReleased',...point,button:'left',clickCount:2});
      const selected=await evaluate(`getSelection().toString()`);
      assert.equal(selected,'SELECT_ME_NATIVE','Browser native selection selects the exact word');
      await evaluate(`window.__historyNode=document.getElementById('history-text').firstChild;window.__sockets.at(-1).dispatchEvent(new MessageEvent('message',{data:'0new output\\r\\n'}));`);await delay(100);
      assert.equal(await evaluate(`getSelection().toString()`),selected);
      assert.equal(await evaluate(`window.__historyNode===document.getElementById('history-text').firstChild`),true);
      await tap('#history-copy');
      assert.equal(await evaluate(`navigator.clipboard.readText()`),selected);
      assert.deepEqual(await evaluate(`window.__sent`),[],'Selecting/copying never sends terminal input');
      const shot=await send('Page.captureScreenshot',{format:'png'});fs.writeFileSync(`${dir}/selection.png`,Buffer.from(shot.data,'base64'));
      await tap('[data-action="mode"]');
      assert.equal(await evaluate(`document.getElementById('terminal-history').hidden`),true);
      assert.equal(await evaluate(`visualViewport.scale`),1);
    }
    results[variant]={lines,scrolled,after,disabled,typography};
  }
  assert.deepEqual(errors,[]);
  console.log(JSON.stringify({ok:true,results,screenshot:`${dir}/selection.png`}));
}finally{
  if(ws?.readyState===WebSocket.OPEN){try{await send('Browser.close');}catch{}}
  ws?.close();
  for(let i=0;i<30&&chrome.exitCode===null&&chrome.signalCode===null;i++)await delay(100);
  if(chrome.exitCode===null&&chrome.signalCode===null)chrome.kill('SIGTERM');
  await delay(500);
  fs.rmSync(`${dir}/profile`,{recursive:true,force:true,maxRetries:5,retryDelay:100});
  await new Promise(resolve=>server.close(resolve));
}
