// Run against sidebar-fixture.html using the Mac mini chrome-bg browser.
// The fixture loads the built Rust/WASM component, not a JavaScript replacement.
async function sidebarAChecks() {
  let passed=0;
  const check=(ok,message)=>{if(!ok)throw Error(message);passed++;};
  const flush=()=>new Promise(resolve=>setTimeout(resolve,50));
  const q=s=>document.querySelector(s);
  check(window.ready,'WASM sidebar ready');
  check(q('[data-sidebar-view="files"]').classList.contains('on'),'Files default');
  check(q('.et-lim').hidden && !q('.cs-explorer').hidden,'usage hidden');
  check(limitCalls===0,'no usage request while files visible');
  check(q('.cs-footer #clock'),'clock at footer');
  check(q('.sf-root').textContent==='/project/one','pane root');
  check(q('.sf-tree').textContent.includes('<unsafe>.txt')&&!q('.sf-tree unsafe'),'escaped filename');
  q('[data-sf-dir="src"]').click();await flush();
  check(q('[data-sf-file="nested.rs"]'),'lazy folder contents');
  q('[data-sf-file="main.rs"]').click();
  check(q('.sf-selection code').textContent==='/project/one/main.rs','full selected path');
  q('[data-sf-open]').click();await flush();
  check(calls.some(c=>c.path==='/open-path'&&c.body.path==='/project/one/main.rs'),'open exact file');
  q('[data-sf-dir="loop"]').click();await flush();
  check(q('.sf-tree').textContent.includes('carpeta ya visible'),'symlink ancestor guarded');
  check(q('.sf-tree').childElementCount<40,'cycle rendering bounded');
  q('[data-sidebar-view="usage"]').click();await flush();
  check(q('.cs-explorer').hidden&&!q('.et-lim').hidden,'usage separate view');
  check(q('.cs-credits').textContent.includes('$37.50'),'real response balance rendered');
  q('[data-sidebar-view="files"]').click();await flush();
  const beforeLimits=limitCalls;sb.render();await flush();check(limitCalls===beforeLimits,'render files skips quotas');
  hold=true;q('[data-sf-refresh]').click();await flush();
  target={session:'two',pane:'%2'};
  // No render/sync yet: the response itself must compare the current target.
  deferred.shift()();await flush();
  check(!q('.sf-root')||!q('.sf-root').textContent.includes('/one'),'late previous-pane response rejected');
  hold=false;for(const resolve of deferred.splice(0))resolve();await flush();
  check(q('.sf-root').textContent==='/project/two','new pane shown');
  hold=true;
  const requestsBefore=calls.filter(c=>c.path==='/fs/explorer').length;
  q('[data-sf-dir="src"]').click();q('[data-sf-dir="src"]').click();q('[data-sf-dir="src"]').click();await flush();
  check(calls.filter(c=>c.path==='/fs/explorer').length===requestsBefore+1,'collapse/reopen retains in-flight reservation');
  hold=false;for(const resolve of deferred.splice(0))resolve();await flush();
  check(q('[data-sf-file="nested.rs"]'),'reopened pending folder completes');
  const beforeCalls=calls.filter(c=>c.path==='/fs/explorer').length;
  hold=true;const dirs=[...document.querySelectorAll('[data-sf-dir]')].map(el=>el.dataset.sfDir);for(const path of dirs){const el=document.querySelector('[data-sf-dir="'+path+'"]');if(el)el.click();}await flush();
  const requests=calls.filter(c=>c.path==='/fs/explorer').length-beforeCalls;
  check(requests===15,'pending requests reserve bounded folder cache: '+requests);
  hold=false;for(const resolve of deferred.splice(0))resolve();await flush();
  check(errors.length===0,'no browser errors: '+errors.join(';'));
  return {passed,requests,root:q('.sf-root').textContent,errors};
}
