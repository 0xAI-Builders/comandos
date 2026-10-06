// node ABS_TEST ABS_BINDGEN_NODE_MODULE ABS_ORIGINAL_HTML
// Both implementations run the same behavioral fixtures. No browser or network.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const native = require(process.argv[2]);
const original = fs.readFileSync(process.argv[3], 'utf8').split('// ---------- App combinada:')[1].split('// ---------- toasts ----------')[0];
const { makeEl, doc } = require('./workspace_dock_dom.cjs');
const tick = () => new Promise(r=>setImmediate(r));
const plain = value=>JSON.parse(JSON.stringify(value));
const RealDate = Date;
class FixedDate extends RealDate { static now() { return 1234567890123; } }
function setup(wasm) {
  const nodes={}, timers=new Map(), events={}, requests=[], toasts=[], posted=[], bridge=[], overview=[], paint=[], raf=new Map();
  let clock=0;
  function el(tag='div') {
    const node=makeEl(tag,{},null);const dataset={};Object.defineProperty(node,'dataset',{configurable:true,value:dataset});
    node.style.setProperty=(k,v)=>{node.style[k]=v;};
    node.insertBefore=(child,next)=>{if(child===next)return child;if(child.parentNode){const index=child.parentNode.children.indexOf(child);if(index>=0)child.parentNode.children.splice(index,1);}let index=next?node.children.indexOf(next):node.children.length;if(index<0)throw Error('bad next');node.children.splice(index,0,child);child.parentNode=node;return child;};
    node.replaceChildren=(...children)=>{for(const n of node.children)n.parentNode=null;node.children=[];for(const child of children)node.appendChild(child);};
    node.getBoundingClientRect=()=>({left:10,right:410,width:400,bottom:40});
    node.scrollBy=o=>{node.scrollLeft=(node.scrollLeft||0)+o.left;};node.setPointerCapture=()=>{};node.releasePointerCapture=()=>{};
    node.scrollLeft=0;node.scrollTop=0;node.clientWidth=1100;node.clientHeight=800;node.offsetHeight=44;node.scrollWidth=1800;
    node.getContext=()=>({fillRect(...a){paint.push(['fillRect',...a]);},beginPath(){},arc(...a){paint.push(['arc',...a]);},fill(){}});
    node.toDataURL=()=> 'data:fake';
    if(tag==='iframe'){node.contentWindow={postMessage:(msg,origin)=>posted.push([msg,origin])};}
    return node;
  }
  const body=el('body'), root=el('html'), head=el('head');
  for(const id of ['panes','splitter','view-panel','term-area','tabbar','tab-prev','tab-next','tab-panel','tab-open','tab-sort','tabclose-name','tabclose','tabclose-yes','groupclose','groupclose-list','groupclose-status','groupclose-yes','groupclose-title','groupclose-desc','groupclose-cancel','n-waiting','n-done','n-working','servers-panel','btn-servers','ssh-bar','ssh-toggle','command-sidebar']){nodes[id]=el();nodes[id].attrs.id=id;body.appendChild(nodes[id]);}
  const document={body,head,documentElement:root,visibilityState:'visible',hidden:false,title:'',createElement:el,getElementById:id=>nodes[id]||null,
    querySelector(sel){return body.querySelector(sel)||head.querySelector(sel);},querySelectorAll(sel){return body.querySelectorAll(sel);},addEventListener:(k,f)=>{(events[k]||=[]).push(f);},removeEventListener:(k,f)=>{events[k]=(events[k]||[]).filter(x=>x!==f);}};
  const data=new Map([['comandos.deviceId','device-fixed']]);
  const storage={getItem:k=>data.get(k)??null,setItem:(k,v)=>data.set(k,String(v)),removeItem:k=>data.delete(k)};
  const env={Date:FixedDate,document,localStorage:storage,sessionStorage:storage,location:{origin:'https://dash.test',hostname:'dash.test',search:''},
    S:{list:[],sel:'',prev:new Map(),favs:new Set(),cfg:{notif:false}},WEBTERM:true,TERM_BASE:'https://dash.test/term',TERM_FALLBACK_BASE:'https://dash.test:8443',TERM_PRIMARY_ATTEMPTS:3,TERM_PRIMARY_RETRY_MS:400,TERM_PRIMARY_PROBE_TIMEOUT_MS:800,termFallbackNotified:false,curTheme:'dia',ONLY_PANEL:'',ACTIVE_TAB:{session:'native',pane:'%4',ts:4},
    innerWidth:1100,innerHeight:800,visualViewport:{height:600,width:1080,offsetTop:2.4,offsetLeft:3.6,addEventListener(){}},screen:{width:1100,height:800,orientation:{type:'landscape-primary'}},navigator:{maxTouchPoints:0},scrollX:0,scrollY:0,
    matchMedia:()=>({matches:false}),scrollTo:(...args)=>requests.push(['scrollTo',args]),
    $:s=>document.querySelector(s),__comandosTranslate:(es,en)=>en,tf:(es,en)=>en,inApp:()=>false,toast:(...a)=>toasts.push(a),authToken:()=> 'private-fixture-token',webtermAccessToken:()=>Promise.resolve('private-fixture-token'),
    api:(p,b)=>{requests.push([p,b]);return Promise.resolve(p.startsWith('/tmux-mouse')?{mouse:'on'}:{});},fetch:(p,o)=>{requests.push(['fetch',p,o]);return Promise.resolve({ok:true});},
    setTimeout:(f,ms)=>{const id=++clock;timers.set(id,{f,ms});return id;},clearTimeout:id=>timers.delete(id),setInterval:(f,ms)=>{const id=++clock;timers.set(id,{f,ms,interval:true});return id;},
    requestAnimationFrame:f=>{const id=++clock;raf.set(id,f);return id;},cancelAnimationFrame:id=>raf.delete(id),addEventListener:(k,f)=>{(events[k]||=[]).push(f);},removeEventListener:(k,f)=>{events[k]=(events[k]||[]).filter(x=>x!==f);},
    __comandosState(){return this.S;},renderSessionOverview:v=>overview.push(v),alertClear:s=>requests.push(['alertClear',s]),notifBadge:()=>{},
    hydrateIcons:()=>{},getComputedStyle:()=>({getPropertyValue:()=>''}),focus:()=>{},swOpen:()=>requests.push(['switcher']),
    setSessionFavorite:()=>Promise.resolve(),updateFavoriteButton:(b,s,label)=>{b.title=label;b.dataset.session=s;},
    pickSel:list=>list.find(it=>it.alive&&`${it.session}|${it.pane}`===env.S.sel)||list.find(it=>it.alive)||null,
    openInApp:(...a)=>requests.push(['openInApp',...a]),loadSsh:()=>requests.push(['loadSsh']),openAnalytics:(...a)=>requests.push(['openAnalytics',...a]),
    webkit:{messageHandlers:{centro:{postMessage:v=>bridge.push(v)}}},URLSearchParams,AbortController,Notification:class{},CSS:{escape:v=>v},console,
  };
  for(const k of ['commandSidebar','chainBuilder','ComandosCommandSidebar','ComandosChainBuilder','ComandosQuickTerminal','quickTerminal','sidebarQuickTerm','WorkspaceDock'])env[k]=null;
  env.__comandosState=()=>env.S;
  env.window=env;
  const constants=['openTerms','remotePaneFocus','termInteraction','SBT','sbTermFrames','CS','SB_LIMITS','WS_DEVICE'];
  const variables=['activeTerm','activeTermTs','activeView','termInteractionSeq','termPageHidden','appViewportFrame','appViewportSettle','tabRowsHold','tabsPollTs','wsFocusTimer','wsFocusSaved','wsFocusRestored','sbNativeMsg','favColor'];
  if(wasm){for(const [k,v]of Object.entries(env)){Object.defineProperty(globalThis,k,{value:v,writable:true,configurable:true});}globalThis.window=globalThis; native.proof_app_coordinator();for(const k of [...constants,...variables,...Object.keys(globalThis).filter(k=>typeof globalThis[k]==='function'&&!Object.hasOwn(env,k))]){Object.defineProperty(env,k,{configurable:true,get:()=>globalThis[k],set:v=>{globalThis[k]=v;}});} // consumers can replace dependencies after mounting
    for(const k of Object.keys(env)){if(!constants.includes(k)&&!variables.includes(k)&&k!=='window'){const v=env[k];Object.defineProperty(env,k,{configurable:true,get:()=>globalThis[k],set:v=>globalThis[k]=v});globalThis[k]=v;}}
  } else {
    vm.createContext(env);vm.runInContext('// App combinada:'+original+'\n'+constants.map(k=>`Object.defineProperty(globalThis,${JSON.stringify(k)},{configurable:true,get:()=>${k}});`).join('\n')+'\n'+variables.map(k=>`Object.defineProperty(globalThis,${JSON.stringify(k)},{configurable:true,get:()=>${k},set:v=>${k}=v});`).join('\n'),env);
  }
  return {env,nodes,timers,raf,events,requests,toasts,posted,bridge,overview,paint,data,el};
}
async function exercise(wasm){
  const f=setup(wasm),e=f.env,out={};
  // Viewport / orientation / split bounds / storage / pointer lifecycle.
  assert.equal(e.currentViewportHeight(),600);e.updateAppViewport();out.viewport=plain(e.document.documentElement.style);
  assert.equal(e.shouldSplitLayout(748,600,true),true);assert.equal(e.shouldSplitLayout(899,500,false),false);
  assert.ok(Number.isNaN(e.setSplitLeft(NaN,false)));assert.equal(e.setSplitLeft(1200,true),740);assert.equal(f.data.get('cc-split-left'),'740');
  e.document.body.classList.add('app');e.scrollY=5;f.nodes.panes.scrollTop=20;e.pinAppScroll();assert.equal(f.nodes.panes.scrollTop,0);
  e.scheduleAppViewport();e.scheduleAppViewport();assert.equal(f.raf.size,1);assert.equal([...f.timers.values()].filter(t=>t.ms===350).length,1);
  e.document.body.classList.add('split');e.initSplitDrag();const splitter=f.nodes.splitter;
  splitter.dispatch('pointerdown',splitter,{pointerId:7,button:0,clientX:510});assert.equal(e.document.body.classList.contains('dragging'),true);
  for(const cb of f.events.pointermove||[])cb({pointerId:8,clientX:900});for(const cb of f.events.pointerup||[])cb({pointerId:7,clientX:550,type:'pointerup'});
  assert.equal(f.data.get('cc-split-left'),'540');assert.equal(e.document.body.classList.contains('dragging'),false);assert.equal((f.events.pointermove||[]).length,0);
  // Live Maps, tab DOM identity / order / labels / height hold; no frame reparent.
  e.document.body.classList.remove('app','split');e.addTermTab('local','Local',true);e.addTermTab('demo','Demo',true);e.addTermTab('term-q1','Quick',false);
  e.S.list=[{session:'demo',pane:'%2',alive:true,agent:'codex',status:'working',model:'claude-sonnet-20250929[1m]',project:'Demo',paneActive:true},{session:'term-q1',pane:'%9',alive:true,agent:'shell',status:'waiting',project:'Quick'}];
  e.S.favs.add('demo');e.renderTabbar();const nodes=[...f.nodes.tabbar.children];assert.equal(nodes.length,3);const labels=nodes.map(n=>n.querySelector('.lbl').textContent);assert.deepEqual(labels,['Local','Demo','Quick']);
  f.nodes.tabbar.scrollLeft=19;e.S.list[0].model='claude-opus-5';e.addTermTab('demo','Demo changed',false);e.renderTabbar();assert.deepEqual([...f.nodes.tabbar.children],nodes);assert.equal(f.nodes.tabbar.scrollLeft,19);assert.equal(nodes[1].querySelector('.mdl').textContent,'opus');
  e.document.body.classList.add('tabs-rows');e.holdTabRowsHeight();f.nodes.tabbar.offsetHeight=20;e.holdTabRowsHeight();assert.equal(f.nodes.tabbar.style.minHeight,'44px');e.addTermTab('x','X',false);e.renderTabbar();assert.equal(f.nodes.tabbar.style.minHeight,'20px');e.document.body.classList.remove('tabs-rows');e.holdTabRowsHeight();assert.equal(f.nodes.tabbar.style.minHeight,'');
  out.labels=labels;out.identity=true;
  // Remote focus and invalid messages never retarget a pane.
  e.activeTerm='demo';e.activeTermTs=2;e.rememberRemotePaneFocus(e.S.list);assert.equal(e.sidebarActiveTab().pane,'%2');
  const frame={dataset:{},contentWindow:{postMessage:(m,o)=>f.posted.push([m,o])},classList:{toggle(){}}};e.openTerms.get('demo').frame=frame;
  assert.equal(e.handleTermFrameMessage({origin:'https://evil.test',source:frame.contentWindow,data:{source:'comandos-term',type:'pane-selected',pane:'%2'}}),false);
  assert.equal(e.handleTermFrameMessage({origin:e.location.origin,source:frame.contentWindow,data:{source:'comandos-term',type:'pane-selected',pane:'%99'}}),false);
  assert.equal(e.handleTermFrameMessage({origin:e.location.origin,source:frame.contentWindow,data:{source:'comandos-term',type:'pane-selected',pane:'%2'}}),true);assert.equal(e.S.sel,'demo|%2');
  // Pending-read cancellation and pagehide compensation for a pending write.
  let readResolve;e.api=(p,b)=>{f.requests.push([p,b]);if(!b&&p.includes('demo'))return new Promise(r=>readResolve=r);return Promise.resolve({mouse:'off'});};
  const pending=e.syncTermInteraction('demo');assert.equal(e.termInteraction.get('demo').busy,true);e.restoreInactiveTermInteractions('term-q1');readResolve({mouse:'on'});assert.equal(await pending,false);assert.equal(e.termInteraction.get('demo').mouse,null);
  let finishSelection;let mouse=true;const keep=[];e.fetch=(p,o)=>{keep.push(JSON.parse(o.body));mouse=true;return Promise.resolve({ok:true});};
  e.api=(p,b)=>{if(!b)return Promise.resolve({mouse:mouse?'on':'off'});if(!b.enabled)return new Promise(r=>finishSelection=()=>{mouse=false;r({mouse:'off'});});mouse=true;return Promise.resolve({mouse:'on'});};
  const st=e.termInteractionState('demo');st.mouse=true;const selecting=e.setTermSelectionMode('demo',true);e.restoreAllTermInteractions();assert.equal(keep.length,1);finishSelection();assert.equal(await selecting,true);assert.equal(keep.length,2);await e.handleTermInteractionsPageShow();assert.equal(st.mouse,true);assert.equal(st.temporary,false);assert.equal(st.busy,false);out.mouse=plain(st);out.keep=keep;
  // Switching tabs preserves temporary selection; close restores exact session.
  e.api=(p,b)=>Promise.resolve({mouse:b?.enabled===false?'off':'on'});await e.setTermSelectionMode('demo',true,true);e.restoreInactiveTermInteractions('term-q1');assert.equal(st.temporary,true);await e.restoreTermInteraction('demo');assert.equal(st.temporary,false);
  // Sidebar selected quick-term and active tab ownership; no fallback destination.
  e.activeTerm=null;e.S.sel='';assert.equal(e.activePaneTarget(),null);e.activeTerm='demo';e.S.sel='demo|%2';assert.equal(e.activePaneTarget().pane,'%2');
  e.selectSidebarTerm({session:'term-q1',pane:'%9'});assert.equal(e.activePaneTarget().session,'term-q1');assert.equal(e.sidebarLimitsCur(),'');e.activeTerm='local';assert.equal(e.sidebarTermTarget(),null);
  out.quick=plain(e.quickTermEntries());
  // Deduplicated sidebar refresh, follow-up on target change, errors surfaced.
  let releases=[],refreshes=0,renders=0;e.commandSidebar={state:{cliInPane:'codex'},refresh(){refreshes++;return new Promise(r=>releases.push(r));},render(){renders++;}};
  e.S.sel='demo|%2';e.activeTerm='demo';e.syncCommandSidebar();e.syncCommandSidebar();await tick();assert.equal(refreshes,1);e.S.sel='term-q1|%9';e.syncCommandSidebar();assert.equal(refreshes,1);releases.shift()();await tick();await tick();assert.equal(refreshes,2);releases.shift()();await tick();
  e.S.list.push({session:'term-q2',pane:'%20',agent:'shell',alive:true,project:'Two'});e.syncCommandSidebar();assert.equal(renders,1);e.syncCommandSidebar();assert.equal(renders,1);e.commandSidebar=null;
  // Native sidebar bridge equality and safe quick-session-only kill.
  e.inApp=()=>true;e.sidebarTermMount('term-q1',null,{session:'term-q1',hidden:false,tabs:[]});e.sidebarTermMount('term-q1',null,{session:'term-q1',hidden:false,tabs:[]});assert.equal(f.bridge.length,1);assert.equal(e.sidebarActiveTab(),e.ACTIVE_TAB);
  await assert.rejects(e.sidebarKillTerm('production'),/Only quick terminals/);e.inApp=()=>false;out.bridge=f.bridge;
  // Device restore marks admission synchronously, and never steals a user's choice after await.
  e.activeView='panel';e.wsFocusRestored=false;let deviceResolve;e.api=()=>new Promise(r=>deviceResolve=r);const focusJob=e.restoreDeviceFocus();assert.equal(e.wsFocusRestored,true);e.activeView='term:term-q1';deviceResolve({activeTabId:'demo'});await focusJob;assert.equal(e.activeView,'term:term-q1');
  e.activeView='panel';e.wsFocusRestored=false;e.api=async()=>({activeTabId:'demo'});await e.restoreDeviceFocus();assert.equal(e.activeView,'term:demo');e.saveDeviceFocus('term-q1');e.saveDeviceFocus('term-q1');const saveTimers=[...f.timers.entries()].filter(([,v])=>v.ms===800);assert.equal(saveTimers.length,1);e.api=async(p,b)=>{f.requests.push([p,b]);return {};};saveTimers[0][1].f();await tick();assert.deepEqual(plain(f.requests.at(-1)),['/workspace/client',{deviceId:'device-fixed',activeTabId:'term-q1'}]);
  // Desktop poll retains the map and frame references while adopting server order.
  const openMap=e.openTerms;e.addTermTab('stale','Stale',true);const demoFrame=e.openTerms.get('demo').frame;e.activeView='panel';e.document.body.classList.remove('split');e.api=async p=>p==='/tabs'?[{session:'demo',label:'Demo'},{session:'local',label:'Local'}]:{};await e.loadDesktopTabs();assert.equal(e.openTerms,openMap);assert.equal(e.openTerms.get('demo').frame,demoFrame);assert.equal(e.openTerms.has('stale'),false);assert.deepEqual([...e.openTerms.keys()].slice(0,2),['demo','local']);
  // Endpoint retry deadlines and degraded notice once; no terminal is created by probing.
  let probes=0;e.fetch=async(p,o)=>{if(p.endsWith('/token')){probes++;return {ok:false};}return {ok:true};};e.api=async p=>({fallbackTerminalOn:true});
  async function settleWithDelays(p){let settled=false,result;p.then(v=>{result=v;settled=true;});for(let n=0;n<12&&!settled;n++){await tick();for(const [id,t]of [...f.timers])if(t.ms===400){f.timers.delete(id);t.f();}}assert.ok(settled);return result;}
  assert.equal(await settleWithDelays(e.resolveTermBase()),e.TERM_FALLBACK_BASE);assert.equal(probes,3);assert.equal(f.toasts.at(-1)[0],'Terminal running in degraded mode');const notices=f.toasts.length;await settleWithDelays(e.resolveTermBase());assert.equal(f.toasts.length,notices);out.probes=probes;
  // Touch scroll emits bounded wheel ticks, leaves another controller untouched.
  const childDoc=f.el(),childHead=f.el('head'),target=f.el();childDoc.head=childHead;childDoc.body=target;childDoc.getElementById=id=>childHead.children.find(c=>c.id===id)||null;childDoc.createElement=f.el;childDoc.elementFromPoint=()=>target;
  const wheels=[];target.dispatchEvent=ev=>wheels.push(ev.deltaY);const childWin={document:childDoc,WheelEvent:class{constructor(type,options){Object.assign(this,options);}},addEventListener(){}};
  const touchFrame={dataset:{},contentWindow:childWin,contentDocument:childDoc};e.styleTermFrame(touchFrame);const css=childHead.children[0].textContent;e.styleTermFrame(touchFrame);assert.equal(childHead.children.length,1);out.css=css;
  e.wireTermFrameScroll(touchFrame);e.wireTermFrameScroll(touchFrame);assert.equal(childDoc.listenerCount('touchmove'),1);childDoc.dispatch('touchstart',target,{touches:[{clientY:400}]});childDoc.dispatch('touchmove',target,{touches:[{clientY:20,clientX:1}],target});assert.deepEqual(wheels,[168]);
  const own={dataset:{},contentWindow:{document:childDoc,__comandosOwnsTouchGestures:true}};e.wireTermFrameScroll(own);assert.equal(childDoc.listenerCount('touchmove'),1);
  // Sort updates shared order once, Undo retains the returned previous snapshot.
  e.api=async(p,b)=>{f.requests.push([p,b]);return {previous:['demo','local']};};e.loadDesktopTabs=()=>Promise.resolve();e.initTabNavigation();f.nodes['tab-sort'].onclick({stopPropagation(){}});let sortMenu=e.document.querySelector('.tab-sort-menu');sortMenu.dispatch('click',sortMenu.querySelector('[data-sort="fav"]'));await tick();assert.equal(f.toasts.at(-1)[0],'Tabs sorted · Undo in ⇅');assert.deepEqual(plain(f.requests.at(-1)),['/workspace/sort',{by:'fav'}]);
  f.nodes['tab-sort'].onclick({stopPropagation(){}});sortMenu=e.document.querySelector('.tab-sort-menu');assert.equal(sortMenu.querySelector('[data-sort="undo"]').textContent,'↶ Undo');sortMenu.dispatch('click',sortMenu.querySelector('[data-sort="undo"]'));await tick();assert.deepEqual(plain(f.requests.at(-1)),['/workspace/sort',{restore:['demo','local']}]);
  // Group close snapshots members/revision and reports a rejected request without closing.
  e.api=async()=>({revision:9,members:[{session:'demo',label:'Demo',kept:false},{session:'local',label:'Local',kept:true}]});await e.askCloseGroup('g1');let sent;e.fetch=async(p,o)=>{sent=JSON.parse(o.body);return {ok:false,statusText:'Conflict',json:async()=>({error:'Changed'})};};await f.nodes['groupclose-yes'].onclick();assert.equal(sent.expectedRevision,9);assert.deepEqual(plain(sent.members.map(m=>m.session)),['demo','local']);assert.equal(f.nodes['groupclose-status'].textContent,'Changed Nothing was closed.');assert.equal(e.openTerms.has('demo'),true);
  // Lazy frame: one append, retained object through repeated ensure/theme.
  e.resolveTermBase=()=>Promise.resolve(e.TERM_BASE);e.api=()=>Promise.resolve({mouse:'on'});e.ensureFrame('term-q1');const iframe=e.openTerms.get('term-q1').frame;e.ensureFrame('term-q1');await tick();assert.equal(e.openTerms.get('term-q1').frame,iframe);assert.equal(iframe.parentNode,f.nodes['term-area']);assert.equal(iframe.src,'https://dash.test/term/?auth=private-fixture-token&arg=term-q1&theme=dia');out.src=iframe.src;
  // Quota switch captures target and refuses drift before interrupt.
  const slot=f.el();e.SB_LIMITS.accounts=[];e.SB_LIMITS.at=123;e.S.sel='demo|%2';e.activeTerm='demo';let opts,updates=0;e.ComandosCommandSidebar={createLimitsView(slot,o){opts=o;return {update(){updates++;}};}};e.sidebarLimitsPaint(slot);e.sidebarLimitsPaint(slot);assert.equal(updates,1);e.S.sel='term-q1|%9';await assert.rejects(opts.onSwitch({alias:'work'},'codex:main'),/selected pane changed/);
  // Builder refresh before run; GTK bridge for native open; isolated chains page.
  let builderOpts;e.ComandosChainBuilder={createChainBuilder(o){builderOpts=o;return {open(){},state:{}};}};e.commandSidebar={state:{catalog:{c:1},chains:[],cliInPane:'codex'},refresh:()=>Promise.resolve(),startChain:s=>f.requests.push(['startChain',s])};e.chainBuilder=null;e.mountChainBuilder();await builderOpts.onSaved({slug:'foo'},{run:true});assert.deepEqual(f.requests.at(-1),['startChain','foo']);
  e.chainBuilder=null;e.ONLY_PANEL='chains';e.location.search='?session=demo&pane=%252';e.api=(p,b)=>{f.requests.push([p,b]);return Promise.resolve(p.startsWith('/commands/catalog')?{catalog:{x:1},cliInPane:'codex'}:{chains:[{slug:'foo'}]});};e.mountChainPage();await tick();assert.equal(builderOpts.target(),'demo · %2 · codex');assert.deepEqual(plain(builderOpts.chains()),[{slug:'foo'}]);
  // Render deduplicates pane keys, clears only changed nonwaiting/done states.
  e.commandSidebar=null;e.ONLY_PANEL='';e.S.prev.clear();e.document.body.classList.remove('app');e.render([{session:'demo',pane:'%2',alive:true,status:'working',project:'Demo'},{session:'demo',pane:'%2',alive:true,status:'working'},{session:'demo',pane:'%3',alive:true,status:'waiting'},{session:'ssh-prod',pane:'%0',alive:false,status:'done'}]);
  out.counts=[f.nodes['n-waiting'].textContent,f.nodes['n-done'].textContent,f.nodes['n-working'].textContent];assert.deepEqual(out.counts,['1','1','1']);out.title=e.document.title;assert.equal(e.document.title,'(1) ComandOS');out.live=[...e.S.liveSess];const painted=f.paint.length;e.favicon('#FFAE1A');assert.equal(f.paint.length,painted);
  const models=['claude-sonnet-20250929[1m]','x[abc\nlast','x[abc\nlast[1m]','x-5\n','claude-x-claude-5','x-2025bad1','A\ud800-5','a\u2028b[1m]'];out.models=models.map(m=>e.tabModelShort(m));
  // Preserve UTF-16 text values at object/DOM boundaries.
  e.addTermTab('edge','A\ud800B',false);e.renderTabbar();assert.equal(f.nodes.tabbar.children.find(n=>n.dataset.tabKey==='term:edge').querySelector('.lbl').textContent,'A\ud800B');
  return out;
}
(async()=>{const expected=await exercise(false);const actual=await exercise(true);assert.deepEqual(actual,expected);console.log('app coordinator compiled WASM/original behavioral oracle: 23 fixture groups passed');})().catch(e=>{console.error(e);process.exitCode=1;});
