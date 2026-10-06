// Inputs and calls to original product consumers; no HTML replacement.
export async function prepareB6(){
 const q=new URLSearchParams(location.search),kind=q.get('__b6');
 const T0=2000000000000;Date.now=()=>T0;window.productApiCalls=[];
 if(kind==='marks'){
  openTerms.clear();openTerms.set('a',{label:'Terminal A'});openTerms.set('b',{label:'Terminal B'});ensureFrame=()=>{};
  WorkspaceDock.adopt({schema:1,revision:4242,tabs:{a:{session:'a',paneKeys:['a']},b:{session:'b',paneKeys:['b']}},groups:[{id:'ga',tree:{type:'tab',tabId:'a'}},{id:'gb',tree:{type:'tab',tabId:'b'}}]});activeTerm='a';document.body.classList.add('app','split');showView('term:a');renderTabbar();
  WorkMarks.adopt({marks:[{scope:'session',key:'a',mark:'frozen',favorite:false,revision:7}],panes:[],activity:{a:{session:'a',state:'working'},b:{session:'b',state:'completed'}}});WorkMarks.decorate();productApiCalls.push('WorkMarks.adopt/decorate -> actual workspace tabs');
  const button=document.querySelector('#tabbar [data-wm-ind] .wm-ind')||document.querySelector('#tabbar .wm-ind');if(!button)throw new Error('Product workspace mark not rendered');
  button.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowDown',bubbles:true}));
  const menu=document.querySelector('.wm-menu');if(!menu)throw new Error('Actual mark menu did not open');
  menu.dispatchEvent(new KeyboardEvent('keydown',{key:'End',bubbles:true}));
  if(document.activeElement.getAttribute('role')!=='menuitemcheckbox')throw new Error('Actual keyboard navigation did not focus favorite');
  productApiCalls.push('actual mark keyboard menu/open/End');window.productProof={consumer:'actual workspace tab indicators and WorkMarks menu',radios:menu.querySelectorAll('[role=menuitemradio]').length,activeRole:document.activeElement.getAttribute('role')};
  if(q.has('__negative')&&q.get('web')!=='off')menu.querySelectorAll('.wm-item')[2].remove();
 } else {
  const running=kind==='running',paused=kind==='paused',done=kind==='completed';
  const block=kind==='idle'?null:{blockId:'private-b6',mode:'focus',status:done?'completed':paused?'paused':'running',targetMs:1500000,activeMs:paused?300000:done?1500000:0,resumedAtMs:T0-300000,deadlineMs:T0+1200000,endedAtMs:done?T0-1000:null,project:'ComandOS'};
  const progress={policyVersion:'v1',xpPerMinute:10,xp:5000,level:6,levelPct:50,xpToNextLevel:500,todayMinutes:30,dailyGoalMinutes:100,streakDays:3,achievements:[],lastLevelUp:null};
  const snap={revision:4242,serverNowMs:T0,settings:{focusMinutes:25,shortBreakMinutes:5,style:'crystals'},block,progress,sound:{enabled:false,device:null,desktopDevice:'private-desktop'}};
  const requests=[],original=window.fetch;window.fetch=async(url,options={})=>{if(String(url).startsWith('/pomodoro')||url==='/notices/sound'){const body=options.body?JSON.parse(options.body):null;requests.push({url:String(url),body});return new Response(JSON.stringify(url==='/notices/sound'?{play:false}:body?.settings?{settings:{...snap.settings,...body.settings}}:snap),{headers:{'Content-Type':'application/json'}});}return original(url,options);};
  ComandosPomodoro.ui.client.accept(snap,T0);ComandosPomodoro.ui.state.seenCompletion=null;ComandosPomodoro.ui.state.banner='';ComandosPomodoro.ui.state.mode='focus';ComandosPomodoro.ui.state.rulerPreview=null;ComandosPomodoro.ui.state.flipAt=null;
  const panel=document.querySelector('#pomo-panel');if(!panel.classList.contains('hidden'))panel.classList.add('hidden');document.querySelector('#btn-pomo').click();ComandosPomodoro.ui.render();
  if(kind==='idle'){ComandosPomodoro.ui.setMinutes(45,false);if(requests.some(r=>r.body?.action==='start'))throw new Error('Pomodoro automatically started');}
  if(q.has('__negative')&&q.get('web')!=='off')panel.querySelector('#pp-go').remove();
  productApiCalls.push('actual server snapshot/client.accept + product panel button + ui.render');
  window.productProof={consumer:'original Pomodoro panel and actual client/render/handlers',status:ComandosPomodoro.ui.client.view().status,remaining:ComandosPomodoro.ui.client.view().remainingMs,mode:ComandosPomodoro.ui.state.mode,actions:requests.filter(r=>r.body?.action)};
  if(done&&productProof.mode!=='break')throw new Error('Completion did not suggest manual break');if(productProof.actions.length)throw new Error('Rendering initiated an automatic block');
 }
 for(const animation of document.getAnimations()){try{animation.finish()}catch{animation.pause();animation.currentTime=0}}
 // Deterministic rendering of the original progress sprite, not its wall clock.
 document.querySelectorAll('.pm-motion-progress>.pm-pixel').forEach(e=>{e.style.backgroundPositionX='-0px'});
 await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
 return productProof;
}
