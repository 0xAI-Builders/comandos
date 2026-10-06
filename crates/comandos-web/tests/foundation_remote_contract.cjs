// Execute original and native remote UI against the same private Node fixture.
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const {fixture}=require('./theme_preferences_contract.cjs');
let active;
function setup(){
  const f=fixture(),e=f.e;const sample=f.nodes.get('#btn-theme');
  // Fresh nodes come from a fresh fixture so event ownership is never shared.
  const fresh=()=>fixture().nodes.get('#btn-theme');
  for(const key of ['#remote','#servers','#usage','#btn-remote','#remote-status','#remote-dashboard-url','#remote-term-url','#remote-fallback-url','#remote-qr','#remote-no-qr','#remote-on','#remote-off','#remote-webterm-on','#remote-webterm-off','#remote-open-terminal','#remote-copy-dashboard','#remote-copy-term','#remote-copy-fallback'])f.nodes.set(key,fresh());
  f.status=fresh();f.nodes.get('#remote-status').querySelector=()=>f.status;
  e.URLSearchParams=URLSearchParams;e.Date=Date;e.authToken=()=> 'private&token';e.arm=()=>f.calls.push('arm');
  e.activeTerm=null;e.WEBTERM=true;e.appEnabled=()=>e.WEBTERM;
  e.openTerms=new Map();e.open=(...a)=>f.calls.push(['window-open',...a]);
  e.loadDesktopTabs=()=>{f.calls.push('load-tabs');return Promise.resolve();};
  e.addTermTab=(session,label)=>{f.calls.push(['add',session,label]);e.openTerms.set(session,{label});};e.openTerm=(...a)=>f.calls.push(['open-term',...a]);
  e.copyText=v=>{f.calls.push(['copy',v]);return Promise.resolve(true);};
  return f;
}
const tick=()=>new Promise(setImmediate);
async function exercise(ctx,f){
  const states=[];
  for(let mask=0;mask<32;mask++){const row={primaryHealthy:!!(mask&1),fallbackHealthy:!!(mask&2),webtermReachable:!!(mask&4),remoteOn:!!(mask&8)};states.push(ctx.remoteButtonState(row,!!(mask&16)));}
  assert.equal(ctx.remoteButtonState(null,false).remoteOffDisabled,true);
  ctx.renderRemote({terminalState:'degraded',remoteOn:true,primaryHealthy:true,urls:{dashboard:'https://private.invalid',terminal:'https://term.invalid'},qrAvailable:true});
  assert.equal(f.status.textContent,'Terminal: Degraded');assert.equal(f.nodes.get('#remote-off').disabled,false);assert.equal(f.nodes.get('#remote-off').classList.contains('warn'),true);
  const qr=new URL(ctx.remoteQrPath(),'https://private.invalid');assert.equal(qr.pathname,'/remote-qr.png');assert.equal(qr.searchParams.get('token'),'private&token');assert.ok(Number(qr.searchParams.get('ts'))>0);
  const request=ctx.remoteAction('/remote-off','off',()=>{f.calls.push('after');return Promise.resolve();});assert.equal(ctx.remoteBusy(),true);assert.equal(f.nodes.get('#remote-on').disabled,true);assert.equal(f.pending.length,1);f.pending.shift().resolve({terminalState:'off'});await request;assert.equal(ctx.remoteBusy(),false);assert.equal(f.nodes.get('#remote-on').disabled,false);
  const failure=ctx.remoteAction('/remote-on','on');f.pending.shift().reject(Error('failed'));await failure;assert.equal(ctx.remoteBusy(),false);assert.deepEqual(f.notices.at(-1),['failed',true]);
  f.nodes.get('#remote-term-url').value=' https://fallback.invalid ';
  ctx.WEBTERM=false;if(ctx===globalThis)ctx.appEnabled=()=>false;await ctx.ensureRemoteTerminalVisible();assert.deepEqual(f.calls.at(-1),['window-open','https://fallback.invalid','_blank','noopener']);
  ctx.WEBTERM=true;if(ctx===globalThis)ctx.appEnabled=()=>true;
  const ensure=ctx.ensureRemoteTerminalVisible();await tick();assert.deepEqual(f.calls.at(-1),['api','/state']);f.pending.shift().resolve([{alive:true,session:'local'},{alive:true,session:'real',project:'Project'}]);await ensure;assert.deepEqual(f.calls.at(-1),['open-term','real','Project']);
  ctx.openTerms.clear();const fallback=ctx.ensureRemoteTerminalVisible();await tick();f.pending.shift().reject(Error('no state'));await fallback;assert.equal(ctx.location.href,'https://fallback.invalid');
  f.nodes.get('#remote-dashboard-url').value='  https://copy.invalid  ';ctx.copyRemote('#remote-dashboard-url');await tick();assert.deepEqual(f.notices.at(-1),['URL copied',false]);f.nodes.get('#remote-dashboard-url').value=' ';ctx.copyRemote('#remote-dashboard-url');assert.deepEqual(f.notices.at(-1),['No URL yet',true]);
  f.nodes.get('#btn-remote').dispatch('click');assert.equal(f.nodes.get('#remote').classList.contains('open'),true);assert.deepEqual(f.calls.at(-1),['api','/remote-state']);f.pending.shift().resolve({terminalState:'active',primaryHealthy:true,fallbackHealthy:true,webtermReachable:true});await tick();assert.equal(f.status.textContent,'Terminal: Active');
  return {states,calls:f.calls,notices:f.notices,status:f.status.textContent,remote:f.nodes.get('#remote').classList.contains('open'),busy:ctx.remoteBusy(),url:ctx.location.href,buttons:['#remote-on','#remote-off','#remote-webterm-on','#remote-webterm-off','#remote-open-terminal'].map(k=>({disabled:f.nodes.get(k).disabled,on:f.nodes.get(k).classList.contains('on'),warn:f.nodes.get(k).classList.contains('warn')}))};
}
exports.reference=async root=>{const f=setup(),src=fs.readFileSync(root+'/dash/index.html','utf8');vm.runInNewContext(src.slice(src.indexOf('// ---------- remoto ----------'),src.indexOf('// ---------- servidores ----------'))+'\nthis.remoteBusy=()=>REMOTE_BUSY;',f.e);return exercise(f.e,f);};
exports.install=()=>{active=setup();Object.assign(globalThis,active.e);globalThis.window=globalThis;globalThis.addTermTab=(session,label)=>{active.calls.push(['add',session,label]);globalThis.openTerms.set(session,{label});};};
exports.native=async()=>{globalThis.remoteBusy=()=>globalThis.REMOTE_BUSY;return exercise(globalThis,active);};
