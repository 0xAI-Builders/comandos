// Evaluate this function in the remote browser against session_controls_fixture.cjs.
async function sessionControlsBrowserChecks() {
  const checks=[],pause=ms=>new Promise(r=>setTimeout(r,ms));
  const fixture=async action=>{const f=await (await fetch('/__fixture',{method:'POST',body:JSON.stringify({action}),headers:{'Content-Type':'application/json'}})).json();f.posts=f.posts.filter(p=>p.path!=='/ui-log');return f;};
  const check=(name,ok)=>{if(!ok)throw Error(name);checks.push(name);};
  const waitFor=async pred=>{for(let i=0;i<40;i++){if(pred())return;await pause(100);}throw Error('Timed out waiting for UI');};
  const button=q=>{const b=document.querySelector('#motor-pop '+q);if(!b)throw Error('Missing '+q);return b;};
  async function reset(){motorPopClose();MOTOR_PENDING.clear();SWITCH_SEEN.clear();const f=await fixture('reset');PROVIDERS=f.registry;S.list=f.items;inApp=()=>true;document.documentElement.classList.add('gtkapp');openMotorFor('fixture','%7');await waitFor(()=>MPOP&&!MPOP.statusLoading&&!MPOP.historyLoading);}
  await reset();
  check('native panel retains its shared sidebar host',document.querySelector('#motor-pop').classList.contains('inline'));
  check('provider and arrow icons render',document.querySelectorAll('.sc-provider svg').length>=2&&document.querySelectorAll('.sc-cycle svg').length===4);
  const rec=button('[data-recommendation="0"]');rec.dispatchEvent(new Event('mouseenter'));
  check('recommendation previews without changing draft',MPOP.draft.model==='gpt-6-astra'&&button('[data-sc-label=model]').textContent==='GPT-6 Sol'&&(await fixture()).posts.length===0);
  rec.dispatchEvent(new Event('mouseleave'));check('leaving preview restores active phrase',button('[data-sc-label=model]').textContent==='GPT-6 Astra');
  button('[data-cycle=model][data-direction="1"]').click();
  await waitFor(()=>MOTOR_PENDING.size===1);
  check('active configuration waits for observed confirmation',MPOP.original.model==='gpt-6-astra'&&MPOP.draft.model==='gpt-6-sol');
  check('pending controls cannot submit twice',button('[data-cycle=model][data-direction="1"]').disabled);
  await fixture('confirm');await waitFor(()=>!MOTOR_PENDING.size&&MPOP.original.model==='gpt-6-sol');
  check('confirmed result refreshes process identity',MPOP.draft.expectedIdentity.pid===84&&MPOP.draft.expectedConversationId==='fixture-destination');
  check('one request targets exact pane',(await fixture()).posts.filter(p=>p.path==='/session/configure').length===1);
  await reset();const slider=button('#sc-effort-range');slider.value='1';slider.dispatchEvent(new Event('input',{bubbles:true}));
  check('slider previews without submitting',(await fixture()).posts.length===0&&button('[data-sc-label=effort]').textContent==='medium');
  slider.dispatchEvent(new Event('change',{bubbles:true}));await waitFor(()=>MOTOR_PENDING.size===1);
  check('slider commits on release',(await fixture()).posts[0].data.effort==='medium');
  await waitFor(()=>!!document.querySelector('[data-interrupt]'));
  button('[data-interrupt]').click();button('[data-discard]').click();
  check('dismissing interruption preserves queued target',MPOP.draft.effort==='medium'&&MOTOR_PENDING.size===1);
  button('[data-cancel-pending]').click();await waitFor(()=>!MOTOR_PENDING.size);
  check('cancel restores active phrase',MPOP.draft.effort==='high');
  await reset();button('[data-word=toHarness]').click();button('[data-choice=toHarness][data-value=claude]').click();
  check('agent transition waits for explicit confirmation',MPOP.confirmation==='route'&&(await fixture()).posts.length===0);
  button('[data-confirm]').click();await waitFor(()=>MOTOR_PENDING.size===1);
  check('confirmed transition sends coherent route',(await fixture()).posts[0].data.model==='opus');
  await fixture('fail');await waitFor(()=>!MOTOR_PENDING.size);
  check('failed operation exposes error and preserves active model',MPOP.error==='Fallo de prueba'&&MPOP.original.model==='gpt-6-astra'&&document.querySelector('.sc-state').textContent.includes('Fallo de prueba'));
  await reset();button('[data-word=harnessAccount]').click();
  check('logged out account is visibly disabled',button('[data-value=expired]').disabled);
  button('[data-close-options]').click();button('[data-word=model]').click();
  check('future model cannot be chosen',button('[data-value=future]').disabled);
  button('[data-close-options]').click();button('[data-recommendation="0"]').click();await waitFor(()=>MOTOR_PENDING.size===1);
  check('recommendation is one coherent request',(await fixture()).posts[0].data.model==='gpt-6-sol'&&(await fixture()).posts[0].data.effort==='medium');
  await reset();check('no browser runtime errors',fixtureErrors.length===0);
  return {checks,errors:fixtureErrors};
}
