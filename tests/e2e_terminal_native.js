#!/usr/bin/env node
'use strict';
// Headless only. Uses in-memory websocket/capture fixtures, never user's tmux.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const http = require('node:http');
const {chromium} = require(process.env.PLAYWRIGHT_CORE || 'playwright-core');
const chrome='/home/john/.local/share/comandos-browser/runtime/bin/chrome';
assert(fs.existsSync(chrome),'Run browser checks on the Mac host.');

async function main() {
  const root = path.resolve(__dirname, '..');
  const historyRequests = [];
  const server = http.createServer((req, res) => {
    const url = new URL(req.url, 'http://localhost');
    if (url.pathname === '/terminal-history') {
      let body = '';
      req.on('data', data => { body += data; });
      req.on('end', () => {
        const data = JSON.parse(body);
        historyRequests.push(data);
        const pane = data.pane || '%2';
        res.setHeader('Content-Type', 'application/json');
        res.end(JSON.stringify({ok:true,pane,panes:[{id:'%1',title:'first'},{id:'%2',title:'second'}],
          text:Array.from({length:300}, (_, i) => `${pane} line ${i} café 😃 repeated repeated`).join('\n')}));
      });
      return;
    }
    const file = path.resolve(root, '.' + url.pathname);
    if (!file.startsWith(root + path.sep)) {res.writeHead(403);res.end();return;}
    fs.readFile(file, (error, data) => {
      if (error) {res.writeHead(404);res.end();return;}
      res.setHeader('Content-Type', file.endsWith('.html') ? 'text/html' : file.endsWith('.js') ? 'application/javascript' : file.endsWith('.css') ? 'text/css' : 'application/octet-stream');
      res.end(data);
    });
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  let browser;
  const failures = [];
  try {
    browser = await chromium.launch({headless:true,executablePath:chrome,chromiumSandbox:true});
    const context = await browser.newContext({viewport:{width:390,height:844},isMobile:true,hasTouch:true,permissions:['clipboard-read','clipboard-write']});
    await context.addInitScript(() => {
      window.__sent = [];
      window.__sockets = [];
      window.WebSocket = class extends EventTarget {
        constructor() {
          super(); this.readyState = 0; this.protocol = 'tty'; window.__sockets.push(this);
          setTimeout(() => {this.readyState = 1; this.dispatchEvent(new Event('open'));}, 0);
        }
        send(data) { if (data instanceof Uint8Array) window.__sent.push(Array.from(data)); }
        close() {this.readyState = 3;this.dispatchEvent(new Event('close'));}
      };
    });
    const page = await context.newPage();
    page.on('pageerror', error => failures.push(error.message));
    await page.goto(origin + '/dash/term.html?arg=fixture&auth=fixture-token');
    await page.waitForSelector('#mobile-compose:not([hidden])');
    const sent = () => page.evaluate(() => window.__sent.map(bytes => new TextDecoder().decode(new Uint8Array(bytes).slice(1))));
    await page.evaluate(() => {
      const draft = document.getElementById('mobile-draft');
      draft.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true}));
      for (const value of ['hol','hola','hola hola 😃\n終']) {
        draft.value = value;
        draft.dispatchEvent(new InputEvent('input',{bubbles:true,isComposing:true,inputType:'insertCompositionText',data:value}));
      }
      draft.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:draft.value}));
      draft.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:draft.value}));
      // A second input route must remain inert while native draft owns entry.
      const hidden = document.querySelector('.xterm-helper-textarea');
      hidden.value = 'DUPLICATE';
      hidden.dispatchEvent(new InputEvent('beforeinput',{bubbles:true,cancelable:true,inputType:'insertText',data:'DUPLICATE'}));
      hidden.dispatchEvent(new InputEvent('input',{bubbles:true,data:'DUPLICATE'}));
    });
    assert.deepEqual(await sent(), []);
    await page.locator('#draft-send').click();
    assert.deepEqual(await sent(), ['hola hola 😃\r終\r']);
    await page.locator('#mobile-draft').fill('repeat repeat');
    await page.locator('#mobile-draft').press('Backspace');
    await page.locator('#draft-insert').click();
    assert.equal((await sent()).at(-1), 'repeat repea');
    await page.locator('#mobile-draft').fill('offline draft');
    await page.evaluate(() => {window.__sockets.at(-1).readyState = 3;});
    const prior = await sent();
    await page.locator('#draft-send').click();
    assert.deepEqual(await sent(), prior);
    assert.equal(await page.locator('#mobile-draft').inputValue(), 'offline draft');
    await page.evaluate(() => {const socket=window.__sockets.at(-1);socket.readyState=1;socket.dispatchEvent(new Event('open'));});
    assert.deepEqual(await sent(), prior); // no automatic replay
    await page.locator('#draft-send').click();
    assert.equal((await sent()).at(-1), 'offline draft\r');
    await page.locator('[data-action="history"]').click();
    await page.waitForFunction(() => document.getElementById('history-text').value.includes('line 299'));
    const stable = await page.evaluate(() => {
      const text = document.getElementById('history-text');
      text.scrollTop = 350;
      text.focus({preventScroll:true});text.setSelectionRange(0,30);
      window.__historyValue = text.value;
      return {top:text.scrollTop,selection:text.value.slice(text.selectionStart,text.selectionEnd)};
    });
    await page.evaluate(() => window.__sockets.at(-1).dispatchEvent(new MessageEvent('message',{data:'0new terminal output\r\n'})));
    await page.waitForFunction(() => document.getElementById('history-status').textContent.startsWith('Hay salida'));
    const afterOutput = await page.evaluate(() => ({top:document.getElementById('history-text').scrollTop,
      selection:document.getElementById('history-text').value.slice(document.getElementById('history-text').selectionStart,document.getElementById('history-text').selectionEnd),same:window.__historyValue === document.getElementById('history-text').value}));
    assert.deepEqual(afterOutput, {...stable,same:true});
    const beforeCopy = await sent();
    await page.locator('#selection-copy').click();
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), stable.selection);
    assert.deepEqual(await sent(), beforeCopy);
    await page.locator('#history-pane').selectOption('%1');
    await page.waitForFunction(() => document.getElementById('history-text').value.startsWith('%1'));
    assert.equal(historyRequests.at(-1).pane,'%1');
    assert.equal(historyRequests.at(-1).session,'fixture');
    await page.locator('#selection-close').click();
    // Long press on output opens pane history and must suppress a synthetic
    // terminal click when the finger lifts, including with tmux mouse enabled.
    await page.evaluate(() => window.__sockets.at(-1).dispatchEvent(new MessageEvent('message',{data:'0\x1b[?1000h\x1b[?1006h'})));
    const cdp = await context.newCDPSession(page);
    const screen = await page.locator('.xterm-screen').boundingBox();
    const beforeHold = await sent();
    await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x:screen.x+100,y:screen.y+100,id:7}]});
    await page.waitForSelector('#terminal-history:not([hidden])');
    await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
    assert.deepEqual(await sent(),beforeHold);
    await page.waitForFunction(() => document.getElementById('history-text').value.includes('line 299'));
    assert.ok(Number.isInteger(historyRequests.at(-1).col));
    await page.screenshot({path:'/tmp/comandos-terminal-native-history.png'});
    await page.locator('#selection-close').click();
    await page.locator('[data-action="keyboard"]').click();
    await page.locator('.xterm-helper-textarea').focus();
    const beforeDirect = (await sent()).length;
    await page.keyboard.press('Control+c');
    await page.keyboard.press('Enter');
    assert.deepEqual((await sent()).slice(beforeDirect), ['\x03','\r']);
    await page.locator('[data-action="keyboard"]').click();
    await page.locator('#mobile-draft').fill('survives reload');
    await page.reload();
    await page.waitForSelector('#mobile-compose:not([hidden])');
    assert.equal(await page.locator('#mobile-draft').inputValue(),'survives reload');
    await page.screenshot({path:'/tmp/comandos-terminal-native-mobile.png'});
    // A narrow viewport still exposes both send actions and keeps all content inside the page.
    await page.setViewportSize({width:320,height:568});
    const layout = await page.evaluate(() => ({width:innerWidth,scroll:document.documentElement.scrollWidth,
      draft:document.getElementById('mobile-draft').getBoundingClientRect().width,
      sendRight:document.getElementById('draft-send').getBoundingClientRect().right}));
    assert.equal(layout.scroll,layout.width);assert.ok(layout.draft > 70);assert.ok(layout.sendRight <= layout.width);
    await context.close();
    // Desktop keeps raw physical input and does not reserve hidden toolbar/draft space.
    const desktop = await browser.newContext({viewport:{width:1000,height:720}});
    await desktop.addInitScript(() => {
      window.WebSocket = class extends EventTarget {constructor(){super();this.readyState=1;}send(){}close(){}};
    });
    const desk = await desktop.newPage();
    desk.on('pageerror', error => failures.push(error.message));
    await desk.goto(origin + '/dash/term.html');
    await desk.waitForSelector('.xterm-screen');
    assert.equal(await desk.locator('#mobile-compose').isVisible(),false);
    assert.equal(await desk.locator('#term-toolbar').isVisible(),false);
    assert.equal(await desk.locator('.xterm-helper-textarea').evaluate(el=>el.readOnly),false);
    await desktop.close();
    assert.deepEqual(failures,[]);
    console.log(JSON.stringify({ok:true,checks:['native IME bytes','repetition','delete','offline draft','no replay','stable history selection','copy without input','pane targeting','physical controls','reload draft','320px layout','desktop raw input']}));
  } finally {
    await browser?.close();
    await new Promise(resolve => server.close(resolve));
  }
}
main().catch(error => {console.error(error);process.exitCode=1;});
