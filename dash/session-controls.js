/* Direct session controls. The durable operation/status APIs own the active state. */
const SC_LABELS = {toHarness:'Agente',motor:'Proveedor',model:'Modelo',effort:'Esfuerzo',harnessAccount:'Cuenta',motorAccount:'Cuenta del proveedor'};
function scPending(context) { return MOTOR_PENDING.get(motorTargetKey(context.ctx.item)); }
function scLocked(context) { return !!(context.sending || context.statusLoading || context.statusError || context.recovery || scPending(context)); }
function scRefresh(context) { if(MPOP===context && !context.previewing) renderSessionConfig(); }
function scIcon(provider) {
  const name={codex:'openai',claude:'anthropic',grok:'grok'}[provider];
  return name ? `<span class="sc-provider sc-provider-${name}" aria-hidden="true">${svg(name,16)}</span>` : '';
}
function scLabel(field, state) {
  return SessionConfig.fieldOptions(PROVIDERS||{},state,field).find(o=>o.value===state[field])?.label || state[field] || 'Sin confirmar';
}
function scSummary(state) {
  return `${scLabel('toHarness',state)} · ${scLabel('model',state)} · ${state.effort||'sin esfuerzo'} · ${state.toHarness==='acp'?state.motorAccount:state.harnessAccount}${state.motor!==state.toHarness&&state.toHarness!=='acp'?' → '+state.motorAccount:''}`;
}
async function scLoadHistory(context) {
  const item=context.ctx.item;
  context.historyLoading=true;
  try {
    context.history=await api('/session-config-history?'+new URLSearchParams({session:item.session,pane:item.pane||''}));
    context.historyError='';
  } catch(e) { context.historyError='No se pudo consultar el historial.'; }
  finally { context.historyLoading=false;scRefresh(context); }
}
async function scLoadStatus(context) {
  const resultEpoch=context.resultEpoch||0;
  context.statusRequested=true;context.statusLoading=true;context.statusError='';
  try {
    const status=await api('/model/status?'+new URLSearchParams({operationKey:motorTargetKey(context.ctx.item)}));
    if(MPOP!==context || (context.resultEpoch||0)!==resultEpoch)return;
    const seen=typeof SWITCH_SEEN==='undefined'?null:SWITCH_SEEN.get(motorTargetKey(context.ctx.item));
    if(seen && JSON.parse(seen)[0]===status.operationId && status.ok===undefined)return;
    context.recovery=status.recoveryRequired||status.recoveryAllowed?status:null;
    context.continuity=status.handoffRequired?status:null;
    if(status.operationId && status.ok===undefined && (status.stage||status.state)) {
      MOTOR_PENDING.set(motorTargetKey(context.ctx.item),{operationId:status.operationId,since:Date.now(),
        stageCode:status.stageCode||status.stage||status.state,stageTxt:sessionStageLabel(status)});
    }
  } catch(e) { context.statusError='No se pudo verificar si hay un cambio pendiente. Reintenta antes de cambiar la configuración.'; }
  finally { context.statusLoading=false;scRefresh(context); }
}
async function scChoose(context, field, value) {
  if(MPOP!==context || scLocked(context))return;
  const option=SessionConfig.fieldOptions(PROVIDERS||{},context.draft,field,context.ctx.item).find(o=>o.value===value);
  if(!option || option.disabled)return;
  context.draft=SessionConfig.update(PROVIDERS||{},context.draft,field,value);
  context.requestId=null;context.error='';context.openField=null;context.previewing=false;
  return scApply(context);
}
async function scApply(context, confirmed=false) {
  if(MPOP!==context || scLocked(context))return;
  const item=context.ctx.item, d=context.draft;
  const error=SessionConfig.validate(PROVIDERS||{},d,item);
  if(error){context.error=error;scRefresh(context);return;}
  if(!SessionConfig.changed(context.original,d)){context.confirmation=null;scRefresh(context);return;}
  if(SessionConfig.requiresConfirmation(context.original,d)&&!confirmed) {
    context.confirmation=d.interrupt?'interrupt':'route';scRefresh(context);return;
  }
  context.confirmation=null;context.sending=true;context.submittedHere=true;context.error='';
  context.requestId ||= crypto.randomUUID ? crypto.randomUUID() : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  const request={session:item.session,pane:item.pane,...d,requestId:context.requestId};
  scRefresh(context);
  try {
    const r=await api('/session/configure',request);
    MOTOR_PENDING.set(r.operationKey||motorTargetKey(item),{operationId:r.operationId,motor:request.motor,
      model:request.model,effort:request.effort,since:Date.now(),queued:!!r.queued,
      stageCode:r.state||'validating',stageTxt:'Cambio solicitado; comprobando sesión…'});
    renderCentro(S.list||[]);
  } catch(e) {context.error=e.message;}
  finally {context.sending=false;scRefresh(context);}
}
function scSessionResult(item, result) {
  const context=MPOP;
  if(!context || context.ctx.mode!=='session' || motorTargetKey(context.ctx.item)!==motorTargetKey(item))return;
  context.resultEpoch=(context.resultEpoch||0)+1;
  context.recovery=result.recoveryRequired||result.recoveryAllowed?result:null;
  context.continuity=result.handoffRequired?result:null;
  context.confirmation=null;context.previewing=false;context.openField=null;context.requestId=null;
  if((result.ok && result.state==='confirmed') || result.rolledBack) {
    const observed=result.observed||result;
    if(observed.harness)item.agent=observed.harness;
    for(const k of ['motor','model','effort','harnessAccount','motorAccount'])if(observed[k]!==undefined)item[k]=observed[k];
    if(observed.harnessAccount)item.account=observed.harnessAccount;
    item.observedConfig={...observed};
    context.ctx.item=item;
    context.original=SessionConfig.draft(item);context.draft={...context.original};
    context.error='';context.confirmed=true;
    scLoadHistory(context);
  } else {context.error=result.detail||result.error||'El cambio no se confirmó. Revisa la sesión.';}
}
function scDiscard(context) {
  context.confirmation=null;context.error='';
  if(!scPending(context)){context.draft={...context.original};context.requestId=null;}
  scRefresh(context);
}
function scCanCancel(pending) {
  return pending && ['validating','waiting','snapshot'].includes(pending.stageCode||'validating');
}
async function scCancelPending(context, interrupt=false) {
  const pending=scPending(context);
  if(context.sending||!scCanCancel(pending))return;
  context.sending=true;context.error='';scRefresh(context);
  try {
    await api('/model/switch-cancel',{session:context.ctx.item.session,pane:context.ctx.item.pane});
    MOTOR_PENDING.delete(motorTargetKey(context.ctx.item));
    context.requestId=null;
    if(interrupt) {
      context.draft={...context.draft,interrupt:true};context.sending=false;
      // Explicit confirmation precedes cancellation. A racing apply makes cancellation fail.
      if(MPOP===context)await scApply(context,true);
    } else {context.draft={...context.original};context.confirmation=null;}
  } catch(e){context.error=e.message;}
  finally{context.sending=false;scRefresh(context);renderCentro(S.list||[]);}
}
function scRecommendations(context) {
  return SessionConfig.recommendations(PROVIDERS||{},context.original,context.history?.items,context.ctx.item);
}
function scPrevious(context) {
  const previous=context.history?.previous;
  return previous && SessionConfig.recommendations(PROVIDERS||{},context.original,[{...previous,count:1}],context.ctx.item)[0];
}
function scWord(context, field, cycle=false) {
  const d=context.draft, locked=scLocked(context), rows=SessionConfig.fieldOptions(PROVIDERS||{},d,field,context.ctx.item);
  const chevron=svg('chevron-down',11),icon=field==='toHarness'?scIcon(d.toHarness):field==='model'||field==='motor'?scIcon(d.motor):'';
  const arrow=direction=>`<button class="sc-cycle" data-cycle="${field}" data-direction="${direction}" aria-label="${SC_LABELS[field]} ${direction<0?'anterior':'siguiente'}" ${locked||SessionConfig.cycle(PROVIDERS||{},d,field,direction,context.ctx.item)===null?'disabled':''}>${svg(direction<0?'chevron-left':'chevron-right',13)}</button>`;
  return `<span class="sc-word-wrap ${cycle?'sc-cycling':''} ${d[field]!==context.original[field]?'sc-changed':''}">
    ${cycle?arrow(-1):''}<button class="sc-word" data-word="${field}" aria-label="Cambiar ${SC_LABELS[field]}: ${attrEsc(scLabel(field,d))}" aria-expanded="${context.openField===field}" aria-controls="sc-options-${field}" ${locked||!rows.length?'disabled':''}>${icon}<span data-sc-label="${field}">${mdEsc(scLabel(field,d))}</span>${chevron}</button>${cycle?arrow(1):''}
    ${context.openField===field?`<span class="sc-word-options" id="sc-options-${field}" role="group" aria-label="${SC_LABELS[field]}">${rows.map(o=>`<button data-choice="${field}" data-value="${attrEsc(o.value)}" aria-pressed="${d[field]===o.value}" ${o.disabled?'disabled':''} title="${attrEsc(o.disabled?o.reason||'No disponible':o.detail||o.label)}">${field==='toHarness'?scIcon(o.value):field==='model'||field==='motor'?scIcon(field==='motor'?o.value:d.motor):''}<span>${mdEsc(o.label)}${o.detail?`<small>${mdEsc(o.detail)}</small>`:''}${o.disabled?`<small>${mdEsc(o.reason||'No disponible')}</small>`:''}</span>${d[field]===o.value?svg('check',13):''}</button>`).join('')}<button data-close-options class="sc-close-options">Cerrar opciones</button></span>`:''}
  </span>`;
}
function scFooter(context) {
  const pending=scPending(context), error=context.error||SessionConfig.validate(PROVIDERS||{},context.draft,context.ctx.item);
  const before=scSummary(context.original), after=scSummary(context.draft);
  if(context.statusLoading)return '<div class="sc-state" role="status">Comprobando la sesión…</div>';
  if(context.statusError)return `<div class="sc-state sc-state-warn" role="alert">${mdEsc(context.statusError)}<button data-retry-status class="sc-btn">Reintentar</button></div>`;
  if(context.recovery)return `<div class="sc-state sc-state-warn" role="alert"><b>${context.recovery.recoveryRequired?'El cambio necesita recuperación':'Confirmación pendiente'}</b><span>El historial original está guardado. Revisa la terminal antes de continuar.</span>${context.error?`<span class="sc-error">${mdEsc(context.error)}</span>`:''}<button data-recover class="sc-btn" ${context.sending?'disabled':''}>Recuperar sesión original</button></div>`;
  if(context.confirmation)return `<div class="sc-state sc-state-warn"><b>${context.confirmation==='interrupt'?'¿Interrumpir el turno?':'¿Cambiar de agente o proveedor?'}</b><span>${mdEsc(after)}</span><span>${context.confirmation==='interrupt'?'Se detendrá el trabajo actual para aplicar el cambio.':'Puede abrir otra conversación. Se conservan el historial original y una nota para continuar.'}</span>${context.error?`<span class="sc-error">${mdEsc(context.error)}</span>`:''}<div class="sc-actions"><button class="sc-btn sc-primary" data-confirm ${context.sending?'disabled':''}>Confirmar cambio</button><button class="sc-btn" data-discard>Cancelar</button></div></div>`;
  if(context.sending)return `<div class="sc-state" role="status"><b>Solicitando cambio…</b><span>Aún activa: ${mdEsc(before)}</span></div>`;
  if(pending)return `<div class="sc-state sc-state-warn" role="status"><b>${mdEsc(pending.stageTxt||'Comprobando configuración…')}</b><span>Aún activa: ${mdEsc(before)}</span>${SessionConfig.changed(context.original,context.draft)?`<span>Solicitada: ${mdEsc(after)}</span>`:''}${error?`<span class="sc-error">${mdEsc(error)}</span>`:''}${scCanCancel(pending)?`<div class="sc-actions"><button class="sc-btn" data-cancel-pending>Cancelar cambio</button>${pending.stageCode==='waiting'&&context.submittedHere?'<button class="sc-btn" data-interrupt>Interrumpir y cambiar ahora</button>':''}</div>`:''}</div>`;
  if(error||SessionConfig.changed(context.original,context.draft))return `<div class="sc-state sc-state-warn" role="status"><span>${mdEsc(error||'Cambio sin confirmar')}</span><div class="sc-actions"><button class="sc-btn" data-discard>Descartar</button>${!SessionConfig.validate(PROVIDERS||{},context.draft,context.ctx.item)?'<button class="sc-btn" data-retry>Reintentar cambio</button>':''}</div></div>`;
  return `<div class="sc-state" role="status"><b>● Activa · ${mdEsc(before)}</b><span>${context.confirmed?'Cambio confirmado por el proceso.':'Si hay un turno en curso, se espera a que termine.'}</span>${scPrevious(context)?'<button class="sc-btn sc-undo" data-previous>↶ Volver a la configuración anterior</button>':''}</div>`;
}
function renderSessionConfig() {
  const context=MPOP;if(!context)return;
  const item=context.ctx.item,registry=PROVIDERS||{};
  context.original ||= SessionConfig.draft(item);context.draft ||= {...context.original};
  if(!context.statusRequested)scLoadStatus(context);
  if(!context.historyRequested){context.historyRequested=true;scLoadHistory(context);}
  const d=context.draft,c=SessionConfig.choices(registry,d),locked=scLocked(context),recs=scRecommendations(context);
  const index=c.efforts.indexOf(d.effort),fill=c.efforts.length>1&&index>=0?index/(c.efforts.length-1)*100:0;
  const cross=d.motor!==d.toHarness&&d.toHarness!=='acp';
  const pop=document.querySelector('#motor-pop');
  const focused=pop.contains(document.activeElement)?document.activeElement:null;
  const focusWord=focused?.dataset.word, focusCycle=focused?.dataset.cycle, focusDirection=focused?.dataset.direction;
  const scroll=pop.querySelector('.sc-direct-body')?.scrollTop||0;
  pop.classList.add('sc-direct');
  pop.innerHTML=`<div class="mp-head"><div class="mp-htitle"><b>Configurar IA</b><span>${mdEsc(item.project||item.session)} · ${mdEsc(item.pane||'panel')}</span></div><button class="mp-close sc-btn" aria-label="Cerrar configuración">×</button></div>
    <div class="sc-direct-body"><div class="sc-eyebrow">${SessionConfig.changed(context.original,d)?'Configuración solicitada':'Este pane trabaja así'}</div>
      <div class="sc-sentence"><div>Uso ${scWord(context,'toHarness')}</div><div>con ${scWord(context,'model',true)}</div><div>esfuerzo ${scWord(context,'effort')}</div>
      ${c.efforts.length>1?`<div class="sc-effort"><input id="sc-effort-range" type="range" min="0" max="${c.efforts.length-1}" step="1" value="${Math.max(index,0)}" aria-label="Esfuerzo del modelo" aria-valuetext="${attrEsc(d.effort)}" style="--sc-fill:${fill}%" ${locked||index<0?'disabled':''}><div class="sc-effort-ticks">${c.efforts.map(e=>`<span class="${e===d.effort?'on':''}">${mdEsc(e)}</span>`).join('')}</div></div>`:''}
      <div>y la cuenta ${scWord(context,d.toHarness==='acp'?'motorAccount':'harnessAccount',true)}</div></div>
      <p class="sc-gesture-hint" aria-live="polite">Flechas para cambiar · desliza y suelta el esfuerzo.</p>
      <details class="sc-route" ${context.routeOpen||cross?'open':''}><summary>Proveedor ${mdEsc(registry.motors?.[d.motor]?.label||d.motor)}${cross?' · ruta avanzada':''}</summary><div>${scWord(context,'motor')}${cross?`<p>Cuenta del proveedor</p>${scWord(context,'motorAccount',true)}`:''}</div></details>
      <section class="sc-habits"><div class="sc-habits-title">Usas seguido <small>En este proyecto</small></div>${recs.map((r,i)=>`<button class="sc-recommendation" data-recommendation="${i}" ${locked?'disabled':''}>${scIcon(r.config.motor)}<span><b>${mdEsc(scSummary(r.config))}</b><small>${r.count} cambios confirmados</small></span>${svg('chevron-right',12)}</button>`).join('')}
      ${!recs.length?`<p class="sc-note">${context.historyLoading?'Consultando historial…':context.historyError||'Las combinaciones aparecerán después de confirmar cambios en este proyecto.'}</p>`:''}
      <details class="sc-habit-reason"><summary>Cómo se eligen</summary>Solo configuraciones confirmadas y compatibles, ordenadas por frecuencia y uso reciente. No se estiman ahorros sin tarifas y una carga comparable.</details></section>
      ${context.continuity?'<div class="sc-note">Este agente usa otra conversación. El historial original y la nota de continuidad están guardados.<button class="sc-btn" data-copy-handoff>Copiar indicación para continuar</button></div>':''}
      <div class="sc-actions sc-extension-link"><button class="sc-btn" data-sc="profile">MCPs · Skills</button></div>
    </div>${scFooter(context)}`;
  pop.querySelector('.sc-direct-body').scrollTop=scroll;
  pop.querySelector('.mp-close').onclick=motorPopClose;
  pop.querySelector('.sc-route').ontoggle=e=>context.routeOpen=e.target.open;
  pop.querySelectorAll('[data-word]').forEach(b=>b.onclick=()=>{context.openField=context.openField===b.dataset.word?null:b.dataset.word;context.previewing=false;renderSessionConfig();pop.querySelector('.sc-word-options button:not(:disabled)')?.focus({preventScroll:true});});
  pop.querySelectorAll('[data-choice]').forEach(b=>b.onclick=()=>scChoose(context,b.dataset.choice,b.dataset.value));
  pop.querySelectorAll('[data-cycle]').forEach(b=>b.onclick=()=>{const v=SessionConfig.cycle(registry,context.draft,b.dataset.cycle,Number(b.dataset.direction),item);if(v!==null)scChoose(context,b.dataset.cycle,v);});
  const closeOptions=()=>{const field=context.openField;context.openField=null;context.previewing=false;renderSessionConfig();pop.querySelector(`[data-word="${field}"]`)?.focus({preventScroll:true});};
  pop.querySelector('[data-close-options]')?.addEventListener('click',closeOptions);
  pop.onkeydown=e=>{if(e.key==='Escape'&&context.openField){e.stopPropagation();closeOptions();}};
  const slider=pop.querySelector('#sc-effort-range');
  if(slider){slider.oninput=()=>{context.previewing=true;const effort=c.efforts[Number(slider.value)];slider.style.setProperty('--sc-fill',Number(slider.value)/(c.efforts.length-1)*100+'%');slider.setAttribute('aria-valuetext',effort);pop.querySelector('[data-sc-label="effort"]').textContent=effort;pop.querySelectorAll('.sc-effort-ticks span').forEach(n=>n.classList.toggle('on',n.textContent===effort));pop.querySelector('.sc-gesture-hint').textContent='Vista previa · suelta para aplicar';};slider.onchange=()=>{context.previewing=false;scChoose(context,'effort',c.efforts[Number(slider.value)]);};slider.onblur=()=>{if(context.previewing){context.previewing=false;scRefresh(context);}};}
  // Preview is presentation only: focus and hover never change the draft or submit.
  const previewConfig=value=>{
    if(scLocked(context))return;
    for(const label of pop.querySelectorAll('[data-sc-label]'))label.textContent=scLabel(label.dataset.scLabel,value);
    pop.querySelector('.sc-gesture-hint').textContent='Vista previa · pulsa para aplicar';
  };
  const restorePreview=()=>{
    if(context.previewing)return;
    for(const label of pop.querySelectorAll('[data-sc-label]'))label.textContent=scLabel(label.dataset.scLabel,context.draft);
    pop.querySelector('.sc-gesture-hint').textContent='Flechas para cambiar · desliza y suelta el esfuerzo.';
  };
  pop.querySelectorAll('[data-recommendation], [data-choice]:not(:disabled)').forEach(b=>{
    const preview=()=>previewConfig(b.dataset.recommendation!==undefined?recs[Number(b.dataset.recommendation)].config:SessionConfig.update(registry,context.draft,b.dataset.choice,b.dataset.value));
    b.onmouseenter=preview;b.onfocus=preview;b.onmouseleave=restorePreview;b.onblur=restorePreview;
  });
  pop.querySelectorAll('[data-recommendation]').forEach(b=>b.onclick=()=>{if(scLocked(context))return;context.draft=SessionConfig.configuration(context.original,recs[Number(b.dataset.recommendation)].config);context.requestId=null;context.openField=null;scApply(context);});
  pop.querySelector('[data-previous]')?.addEventListener('click',()=>{const prior=scPrevious(context);if(!prior||scLocked(context))return;context.draft=prior.config;context.requestId=null;scApply(context);});
  pop.querySelector('[data-confirm]')?.addEventListener('click',()=>context.confirmation==='interrupt'&&scPending(context)?scCancelPending(context,true):scApply(context,true));
  pop.querySelector('[data-discard]')?.addEventListener('click',()=>scDiscard(context));
  pop.querySelector('[data-retry]')?.addEventListener('click',()=>scApply(context));
  pop.querySelector('[data-retry-status]')?.addEventListener('click',()=>{scLoadStatus(context);renderSessionConfig();});
  pop.querySelector('[data-cancel-pending]')?.addEventListener('click',()=>scCancelPending(context));
  pop.querySelector('[data-interrupt]')?.addEventListener('click',()=>{context.confirmation='interrupt';renderSessionConfig();});
  pop.querySelector('[data-recover]')?.addEventListener('click',async()=>{if(context.sending)return;context.sending=true;renderSessionConfig();try{const r=await api('/session/recover',{operationId:context.recovery.operationId});MOTOR_PENDING.set(r.operationKey||motorTargetKey(item),{operationId:r.operationId,recovering:true,since:Date.now(),stageCode:'recovering',stageTxt:'Recuperando conversación original…'});context.recovery=null;}catch(e){context.error=e.message;}finally{context.sending=false;scRefresh(context);}});
  pop.querySelector('[data-copy-handoff]')?.addEventListener('click',async()=>{try{await navigator.clipboard.writeText('Lee la nota de continuidad en '+context.continuity.handoffPath+' y retoma la tarea. El historial original se conserva por separado.');toast('Indicación copiada. Pégala en la terminal para continuar.');}catch(e){toast('No se pudo copiar: '+e.message,true);}});
  pop.querySelector('[data-sc="profile"]').onclick=()=>{motorPopClose();window.openPaneExtensions(item.session,item.pane||'',item.agent||'');};
  if(focusCycle)pop.querySelector(`[data-cycle="${focusCycle}"][data-direction="${focusDirection}"]:not(:disabled)`)?.focus({preventScroll:true});
  else if(focusWord)pop.querySelector(`[data-word="${focusWord}"]:not(:disabled)`)?.focus({preventScroll:true});
}
