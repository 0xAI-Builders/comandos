// Test inputs and calls to the existing product controls. No HTML is generated
// here: index.html, pomodoro.js and workspace.js own rendering and handlers.
export async function prepareProduct() {
  const params = new URLSearchParams(location.search);
  const kind = params.get('__product');
  window.productApiCalls ||= [];
  const trace = (api, name, label = name) => {
    const original = api[name];
    api[name] = (...args) => { productApiCalls.push(label); return original(...args); };
  };
  const until = async check => {
    for (let i = 0; i < 100; i++) { if (check()) return; await new Promise(r => setTimeout(r, 20)); }
    throw new Error('Product control did not reach the requested state: ' + kind);
  };
  await document.fonts.ready;
  if (kind === 'drafts') {
    await until(() => document.querySelector('#mobile-draft')?.value==='git status\n😀 borrador local');
    if (document.querySelector('#mobile-compose').hidden) document.querySelector('[data-action=keyboard]').click();
    const draft=document.querySelector('#mobile-draft');
    window.productProof={consumer:'literal dash/term.html deviceDrafts.restore and mobile composer/keyboard handlers',source_sha256:document.querySelector('meta[name=product-source-sha256]').content,text:draft.value,selection:[draft.selectionStart,draft.selectionEnd],sent:productSends};
    if (productSends.some(s=>typeof s==='string' && s.startsWith('0'))) throw new Error('Restore wrote composer content to the PTY');
  } else if (kind === 'sounds') {
    await until(() => document.querySelector('[data-pm-sound]'));
    for (const name of ['isEnabled','getVolume','setEnabled','setVolume']) trace(uiSounds, name, 'uiSounds.' + name);
    uiSounds.setEnabled(false); uiSounds.setVolume(.6);
    document.querySelector('[data-pm-sound]').click();
    const volume = document.querySelector('[data-pm-volume]');
    volume.value = '35'; volume.dispatchEvent(new Event('input', {bubbles:true}));
    document.querySelector('.pm-sound-options').open = true;
    window.productProof = {consumer:'dash/pomodoro.js soundHtml/render and click/input handlers',enabled:uiSounds.isEnabled(),volume:uiSounds.getVolume()};
    if (!productProof.enabled || productProof.volume !== .35) throw new Error('Sound controls did not update the actual API');
  } else if (kind?.startsWith('push')) {
    document.querySelector('#btn-settings').click();
    activateMtab(document.querySelector('#settings .modal-panel'), 'notif');
    let subscription = null, prompts = 0;
    const requests = [];
    const fakeSubscription = {endpoint:'https://push.example.invalid/private-test',toJSON(){return {endpoint:this.endpoint,keys:{auth:'a',p256dh:'b'}};},async unsubscribe(){subscription=null;return true;}};
    Object.defineProperty(Notification, 'permission', {value:'default',writable:true,configurable:true});
    Notification.requestPermission = async () => { prompts++; Notification.permission='granted'; return 'granted'; };
    Object.defineProperty(navigator.serviceWorker, 'ready', {configurable:true,value:Promise.resolve({pushManager:{async getSubscription(){return subscription;},async subscribe(){subscription=fakeSubscription;return subscription;}}})});
    const originalFetch = window.fetch;
    window.fetch = async (url, opts={}) => {
      if (String(url).startsWith('/push/')) {
        requests.push([opts.method || 'GET', String(url)]);
        return new Response(JSON.stringify(String(url)==='/push/key'?{available:true,publicKey:'AQIDBA'}:{ok:true}),{headers:{'Content-Type':'application/json'}});
      }
      return originalFetch(url, opts);
    };
    document.querySelector('#push-enable').click();
    await until(() => document.querySelector('#push-settings').dataset.state==='enabled' && !document.querySelector('#push-test').disabled);
    document.querySelector('#push-test').click();
    await until(() => document.querySelector('#push-status').textContent.startsWith('Aviso de prueba enviado') && !document.querySelector('#push-disable').disabled);
    if (kind==='push-disabled') {
      document.querySelector('#push-disable').click();
      await until(() => document.querySelector('#push-status').textContent==='Avisos push desactivados en este dispositivo.' && !document.querySelector('#push-enable').disabled);
    }
    productApiCalls.push('actual push enable/test' + (kind==='push-disabled'?'/disable':''));
    window.productProof = {consumer:'dash/index.html #push-settings and installed production handlers',state:document.querySelector('#push-settings').dataset.state,prompts,requests};
    if (prompts !== 1 || requests.length !== (kind==='push-disabled'?4:3)) throw new Error('Product push actions did not run');
  } else if (kind === 'session') {
    const negative = params.has('__negative') && params.get('web')!=='off';
    for (const name of ['choices','update']) trace(SessionConfig, name, 'SessionConfig.' + name);
    if (negative) SessionConfig.choices = () => ({models:[],efforts:[],accounts:[],motorAccounts:[]});
    PROVIDERS = {harnesses:{codex:{label:'Codex',accounts:[{alias:'work',selectable:true}]},claude:{label:'Claude',accounts:[{alias:'main',selectable:true}]}},motors:{codex:{label:'Codex',models:[{id:'gpt-6-astra',name:'GPT-6 Astra',selectable:true,efforts:['high','ultra']}]},claude:{label:'Claude',models:[{id:'opus',name:'Opus',selectable:true,efforts:['high']}]}},matrix:[{harness:'codex',motor:'codex',selectable:true},{harness:'claude',motor:'claude',selectable:true}]};
    const originalApi = api;
    api = async (url,...args) => url.startsWith('/session-profiles') ? {profiles:[],inventory:{skills:[],mcps:[]},capabilities:{}} : originalApi(url,...args);
    await openSessionProfiles({agent:'codex',model:'gpt-6-astra',effort:'high',account:'work'});
    const effort = document.querySelector('#session-workspace-modal [name=effort]');
    effort.value='ultra'; effort.dispatchEvent(new Event('change',{bubbles:true}));
    await until(() => productApiCalls.includes('SessionConfig.update'));
    window.productProof = {consumer:'dash/workspace.js openSessionProfiles/render and effort onchange',selected:document.querySelector('#session-workspace-modal [name=effort]').value};
    if (!negative && productProof.selected !== 'ultra') throw new Error('Session control did not apply the actual update');
  } else if (kind === 'workspace') {
    for (const name of ['moveTab','tabIds']) trace(WorkspaceLayout,name,'WorkspaceLayout.'+name);
    openTerms.clear(); openTerms.set('a',{label:'Terminal A'});openTerms.set('b',{label:'Terminal B'});
    ensureFrame=()=>{}; // No PTY/iframe connection: only the existing workspace chrome.
    const initial={schema:1,tabs:{a:{session:'a',paneKeys:['a']},b:{session:'b',paneKeys:['b']}},groups:[{id:'ga',tree:{type:'tab',tabId:'a'}},{id:'gb',tree:{type:'tab',tabId:'b'}}]};
    const moved=WorkspaceLayout.moveTab(initial,'a','b','left');
    WorkspaceDock.adopt({...moved,revision:4242});
    activeTerm='a';document.body.classList.add('app','split');showView('term:a');renderTabbar();WorkspaceDock.render('a');
    window.productProof={consumer:'dash/workspace-dock.js tree/strip render after actual WorkspaceLayout.moveTab',tabs:WorkspaceDock.visibleTabs('a')};
  } else if (kind === 'quick') {
    trace(ComandosQuickTerminal,'createQuickTerminal','ComandosQuickTerminal.createQuickTerminal');
    const originalApi=api;
    api=async (url,...args)=>{if(url==='/terminal/quick') {productApiCalls.push('terminal request');throw new Error('No se pudo abrir la terminal de prueba');}return originalApi(url,...args);};
    window.quickTerminal=null;sessionStorage.setItem('comandos.quickTerminal.pending','private-dom-test');
    document.querySelector('#btn-terminal').click();
    await until(()=>document.querySelector('#toasts').textContent.includes('No se pudo abrir la terminal de prueba'));
    // Crop the actual terminal error; unrelated dashboard polling can also
    // append toasts and is outside this component's comparison.
    [...document.querySelector('#toasts').children].find(e=>e.textContent==='No se pudo abrir la terminal de prueba').setAttribute('data-product-quick','');
    window.productProof={consumer:'dash/index.html #btn-terminal -> quickTerminalInstance -> toast',pending:quickTerminal.pendingRequestId,busy:quickTerminal.busy};
    if(productProof.pending!=='private-dom-test'||productProof.busy)throw new Error('Actual terminal retry contract changed');
  } else throw new Error('Unknown production consumer ' + kind);
  // Freeze only animation/transition paint; preserve product DOM and layout.
  for (const animation of document.getAnimations()) { try { animation.finish(); } catch { animation.pause(); animation.currentTime=0; } }
  await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
  return productProof;
}
