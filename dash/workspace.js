/* Session controls share the dashboard's observed state and authenticated API. */
function scOption(value, label, selected, disabled=false) {
  return `<option value="${attrEsc(value)}" ${value===selected?'selected':''} ${disabled?'disabled':''}>${mdEsc(label)}</option>`;
}
function scSelect(name, label, options) {
  return `<label class="sc-field">${label}<select name="${name}">${options}</select></label>`;
}
function renderSessionConfig() {
  const context = MPOP;
  if(!context) return;
  const pop = document.querySelector('#motor-pop'), item = context.ctx.item;
  const registry = PROVIDERS || {};
  context.original ||= SessionConfig.draft(item);
  context.draft ||= {...context.original};
  const d = context.draft, c = SessionConfig.choices(registry,d);
  const error = SessionConfig.validate(registry,d,item);
  const pending = MOTOR_PENDING.get(motorTargetKey(item));
  if(!context.statusRequested) {
    context.statusRequested=true;
    api('/model/status?'+new URLSearchParams({operationKey:motorTargetKey(item)})).then(status=>{
      context.recovery=status.recoveryRequired||status.recoveryAllowed?status:null;
      context.continuity=status.handoffRequired?status:null;
      if(status.operationId && status.ok === undefined && status.state && !MOTOR_PENDING.has(motorTargetKey(item)))
        MOTOR_PENDING.set(motorTargetKey(item),{operationId:status.operationId,since:Date.now(),stageTxt:sessionStageLabel(status)});
      if(MPOP===context)renderSessionConfig();
    }).catch(()=>{});
  }
  const names = registry.harnesses || {};
  const harnesses = [...new Set((registry.matrix || []).map(r=>r.harness))];
  if(!harnesses.includes(d.toHarness)) harnesses.unshift(d.toHarness);
  const motors = [...new Set((registry.matrix || []).filter(r=>r.harness===d.toHarness).map(r=>r.motor))];
  const accounts = (rows, selected) => {
    const list = rows.slice();
    if(!list.some(a=>a.alias===selected)) list.unshift({alias:selected,selectable:!rows.length,identity:rows.length?'no disponible':''});
    return list.map(a=>scOption(a.alias,`${a.alias}${a.identity?' · '+a.identity:''}`,selected,!a.selectable)).join('');
  };
  const models = c.models.slice();
  if(d.model && !models.some(m=>m.id===d.model)) models.unshift({id:d.model,name:d.model+' · sin confirmar'});
  pop.innerHTML = `<div class="mp-head"><div class="mp-htitle"><b>Configurar sesión</b><span>${mdEsc(item.project||item.session)} · ${mdEsc(item.pane||'panel')}</span></div><button class="mp-close sc-btn" aria-label="Cerrar">×</button></div>
    <div class="session-config">
      <div class="sc-note">Actual: ${mdEsc(names[item.agent]?.label||item.agent||'shell')} · ${mdEsc(item.model||'modelo sin confirmar')} · ${mdEsc(item.effort||'esfuerzo sin confirmar')}<br>Cuenta: ${mdEsc(item.harnessAccount||item.account||'sin confirmar')}</div>
      ${pending?`<div class="mp-prog">${motorProgressHtml(pending)}</div>`:''}
      ${context.recovery?`<div class="sc-summary" role="alert">${context.recovery.recoveryRequired?'El cambio necesita recuperación.':'La confirmación sigue pendiente. Puedes volver a la sesión original.'} El historial original está guardado.<br><button class="sc-btn" data-recover>Recuperar ${mdEsc(context.recovery.sourceHarness||'sesión original')}</button></div>`:''}
      ${context.continuity?`<div class="sc-summary">Este CLI usa una conversación nueva. El historial original y la nota para retomar la tarea están guardados.<br><button class="sc-btn" data-copy-handoff>Copiar indicación para continuar</button></div>`:''}
      <div class="sc-grid">
        ${scSelect('toHarness','Interfaz / CLI',harnesses.map(h=>scOption(h,names[h]?.label||h,d.toHarness)).join(''))}
        ${scSelect('motor','Motor de IA',motors.map(m=>scOption(m,registry.motors?.[m]?.label||m,d.motor,!(registry.matrix||[]).some(r=>r.harness===d.toHarness&&r.motor===m&&r.selectable) || !!SessionConfig.switchError(registry,{...d,motor:m},item))).join(''))}
        ${scSelect('model','Modelo',models.map(m=>scOption(m.id,m.name||m.id,d.model,!!m.soon)).join(''))}
        ${scSelect('effort','Esfuerzo',(c.efforts.length?c.efforts:['']).map(e=>scOption(e,e||'No configurable',d.effort)).join(''))}
        ${scSelect(d.toHarness==='acp'?'motorAccount':'harnessAccount',d.toHarness==='acp'?'Cuenta del motor':'Cuenta del CLI',accounts(c.accounts,d.toHarness==='acp'?d.motorAccount:d.harnessAccount))}
        ${d.motor!==d.toHarness&&d.toHarness!=='acp'?scSelect('motorAccount','Cuenta del motor',accounts(c.motorAccounts.map(a=>({...a,selectable:a.motorSelectable})),d.motorAccount)):''}
      </div>
      <label class="sc-check"><input type="checkbox" name="interrupt" ${d.interrupt?'checked':''}>Interrumpir el turno para cambiar ahora</label>
      <div class="sc-summary">Destino: ${mdEsc(names[d.toHarness]?.label||d.toHarness)} → ${mdEsc(d.model||d.motor)}${d.effort?' · '+mdEsc(d.effort):''}<br>Cuenta ${mdEsc(d.harnessAccount)}${d.motorAccount!==d.harnessAccount?' → '+mdEsc(d.motorAccount):''}<br>${d.interrupt?'Interrumpe el turno actual.':'Espera a que termine el turno actual.'}</div>
      <div class="sc-error" role="status">${mdEsc(context.error||error)}</div>
      <button class="sc-apply" ${context.sending||pending||context.recovery||error||!SessionConfig.changed(context.original,d)?'disabled':''}>${context.sending?'Guardando cambio…':'Aplicar cambios'}</button>
      <div class="sc-note">El cambio guarda un punto de recuperación. Entre CLIs se conserva el historial original; el contexto compartido no sustituye ese historial.</div>
      <div class="sc-actions"><button class="sc-btn" data-sc="profile">Skills, MCPs y perfiles</button><button class="sc-btn" data-sc="usage">Uso de herramientas</button></div>
    </div>`;
  pop.querySelector('.mp-close').onclick = motorPopClose;
  const handoff=pop.querySelector('[data-copy-handoff]');
  if(handoff)handoff.onclick=async()=>{try{
    await navigator.clipboard.writeText('Lee la nota de continuidad en '+context.continuity.handoffPath+' y retoma la tarea. El historial original se conserva por separado.');
    toast('Indicación copiada. Pégala en la terminal para continuar.');
  }catch(e){toast('No se pudo copiar: '+e.message,true);}};
  pop.querySelectorAll('select,input').forEach(el=>el.addEventListener('change',()=>{
    context.draft=SessionConfig.update(registry,context.draft,el.name,el.type==='checkbox'?el.checked:el.value);
    context.error=''; context.requestId=null; renderSessionConfig();
  }));
  const extensions=()=>{motorPopClose();window.openPaneExtensions(item.session,item.pane||'',item.agent||'');};
  pop.querySelector('[data-sc=profile]').onclick=extensions;
  pop.querySelector('[data-sc=usage]').onclick=extensions;
  const recover=pop.querySelector('[data-recover]');
  if(recover)recover.onclick=async()=>{recover.disabled=true;try{
    const r=await api('/session/recover',{operationId:context.recovery.operationId});
    MOTOR_PENDING.set(r.operationKey||motorTargetKey(item),{operationId:r.operationId,recovering:true,since:Date.now(),stageTxt:'Recuperando conversación original…'});
    motorPopClose();renderCentro(S.list||[]);
  }catch(e){context.error=e.message;renderSessionConfig();}};
  pop.querySelector('.sc-apply').onclick=async()=>{
    if(context.sending) return;
    context.sending=true; context.error='';
    context.requestId ||= crypto.randomUUID ? crypto.randomUUID() : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
    renderSessionConfig();
    try {
      const r=await api('/session/configure',{session:item.session,pane:item.pane,...context.draft,requestId:context.requestId});
      MOTOR_PENDING.set(r.operationKey||motorTargetKey(item),{operationId:r.operationId,motor:d.motor,model:d.model,effort:d.effort,since:Date.now(),queued:!!r.queued,stageTxt:'Cambio guardado; comprobando sesión…'});
      if(MPOP===context) motorPopClose();
      renderCentro(S.list||[]); toast('Cambio solicitado. El resultado se confirmará en la sesión.');
    } catch(e) {context.error=e.message;} finally {context.sending=false;if(MPOP===context)renderSessionConfig();}
  };
}

let scModalReturnFocus=null;
function scDialog(title) {
  let modal=document.querySelector('#session-workspace-modal');
  if(!modal){modal=document.createElement('div');modal.id='session-workspace-modal';modal.className='sc-modal';document.body.appendChild(modal);}
  scModalReturnFocus=document.activeElement;
  modal.hidden=false;
  modal.innerHTML=`<section class="sc-panel" role="dialog" aria-modal="true" aria-label="${attrEsc(title)}"><header><h2>${mdEsc(title)}</h2><button class="sc-btn" data-close-sc aria-label="Cerrar">×</button></header><div class="sc-body" aria-live="polite">Cargando…</div></section>`;
  modal.querySelector('[data-close-sc]').onclick=()=>{modal.hidden=true;scModalReturnFocus?.focus?.({preventScroll:true});};
  modal.onclick=e=>{if(e.target===modal)modal.querySelector('[data-close-sc]').click();};
  modal.onkeydown=e=>{if(e.key==='Escape'){e.stopPropagation();modal.querySelector('[data-close-sc]').click();}};
  modal.querySelector('[data-close-sc]').focus({preventScroll:true});
  return modal.querySelector('.sc-body');
}

async function openExtensionUsage(item) {
  const body=scDialog('Uso de skills y MCPs');
  try {
    const params=new URLSearchParams({days:'7'});
    if(item?.session)params.set('session',item.session);
    if(item?.pane)params.set('pane',item.pane);
    const result=await api('/extension-usage?'+params);
    if(!body.isConnected)return;
    const rows=result.items||result.extensions||result.usage||[];
    body.innerHTML=`<p class="sc-note">${mdEsc(item?`${item.project||item.session} · ${item.pane||''}`:'Todas las sesiones')} · últimos 7 días. Uso observado; habilitar una herramienta no cuenta como usarla.</p>
      ${rows.length?`<table class="sc-table"><thead><tr><th>Skill / herramienta</th><th>Llamadas</th><th>Tiempo medido</th></tr></thead><tbody>${rows.map(r=>`<tr><td>${mdEsc(r.name||r.tool||r.id)}<br><small>${mdEsc(r.kind||r.type||'')}</small></td><td>${Number(r.calls??r.count??0)}</td><td>${r.durationMs!=null?(Number(r.durationMs)/1000).toFixed(1)+' s':'Sin atribución'}</td></tr>`).join('')}</tbody></table>`:'<p>Todavía no hay llamadas observadas para este filtro.</p>'}
      <p class="sc-note">${mdEsc(result.note||(result.provenance==='observed'?'Medido en eventos locales':typeof result.provenance==='string'?result.provenance:'')||'Los tokens de una conversación no se atribuyen automáticamente a cada skill.')}<br>${Number(result.unattributedSkillCalls||0)} llamadas a skills sin nombre registrado.<br>No se calculan ahorros de tokens sin una medición comparable.</p>`;
  }catch(e){body.textContent=e.message;}
}

let scProfilesCache=[];
async function openSessionProfiles(item) {
  const body=scDialog('Perfiles de sesión');
  const current=item||pickSel(S.list||[])||{};
  let draft={name:'Mi perfil',harness:current.agent==='shell'?'codex':current.agent||'codex',motor:current.motor||current.agent||'codex',
    model:current.model||'',effort:current.effort||'',harnessAccount:current.harnessAccount||current.account||'main',motorAccount:current.motorAccount||current.account||'main',skills:{},mcps:{}};
  let data;
  async function load() {
    const params=new URLSearchParams({harness:draft.harness,account:draft.harnessAccount});
    const cwd=current.cwd||document.querySelector('#ns-cwd')?.value;if(cwd)params.set('cwd',cwd);
    data=await api('/session-profiles?'+params);
    scProfilesCache=data.profiles||[];
  }
  function render() {
    const inventory=data.inventory||{}, caps=data.capabilities||{};
    const selection={...draft,toHarness:draft.harness};
    const choices=SessionConfig.choices(PROVIDERS||{},selection);
    const accOptions=(items,value)=>scOption('', 'Selecciona cuenta',value)+items.map(a=>scOption(a.alias,a.alias+(a.identity?' · '+a.identity:''),value,!a.selectable)).join('');
    const inventoryGroup=(kind,label)=>`<fieldset><legend>${label}</legend><p class="sc-note">${mdEsc(caps[kind]?.reason||'Se aplican al iniciar una sesión nueva.')}</p>${(inventory[kind]||[]).map(x=>{
      const key=x.id||x.name, enabled=Object.hasOwn(draft[kind],key)?draft[kind][key]:x.enabled!==false;
      const description=kind==='mcps'?`<small class="mcp-description">${mdEsc(x.description||'Este servidor no tiene una descripción disponible.')}</small>`:'';
      const scope={user:'Cuenta',project:'Proyecto',mixed:'Varias configuraciones',compatible:'Configuración compartida'}[x.scope]||'';
      return `<label><input type="checkbox" data-kind="${kind}" data-id="${attrEsc(key)}" ${enabled?'checked':''} ${x.toggleable===false?'disabled':''}><span>${mdEsc(x.name||key)}${description}<small>${mdEsc(scope)}${scope?' · ':''}${x.toggleable===false?'No configurable por perfil':'Al iniciar'}</small></span></label>`;
    }).join('')||'<p class="sc-note">No hay elementos disponibles.</p>'}</fieldset>`;
    body.innerHTML=`<p class="sc-note">Guarda CLI, modelo, cuentas, skills y MCPs para iniciar con el mismo equipo. Editar un perfil no modifica las sesiones abiertas.</p>
      <div class="sc-grid">${scSelect('profile','Perfil guardado',scOption('','Nuevo perfil',draft.id||'')+scProfilesCache.map(p=>scOption(p.id,p.name,draft.id)).join(''))}
      <label class="sc-field">Nombre<input name="name" maxlength="80" value="${attrEsc(draft.name)}"></label>
      ${scSelect('harness','CLI',Object.entries(PROVIDERS?.harnesses||{}).filter(([h])=>h!=='shell').map(([h,s])=>scOption(h,s.label||h,draft.harness)).join(''))}
      ${scSelect('motor','Motor',(PROVIDERS?.matrix||[]).filter(r=>r.harness===draft.harness).map(r=>scOption(r.motor,PROVIDERS?.motors?.[r.motor]?.label||r.motor,draft.motor,!r.selectable)).join(''))}
      ${scSelect('model','Modelo',scOption('','Predeterminado',draft.model)+choices.models.map(m=>scOption(m.id,m.name||m.id,draft.model)).join(''))}
      ${scSelect('effort','Esfuerzo',scOption('','Predeterminado',draft.effort)+choices.efforts.map(e=>scOption(e,e,draft.effort)).join(''))}
      ${scSelect(draft.harness==='acp'?'motorAccount':'harnessAccount',draft.harness==='acp'?'Cuenta del motor':'Cuenta del CLI',accOptions(choices.accounts,draft.harness==='acp'?draft.motorAccount:draft.harnessAccount))}
      ${draft.motor!==draft.harness&&draft.harness!=='acp'?scSelect('motorAccount','Cuenta del motor',accOptions(choices.motorAccounts.map(a=>({...a,selectable:a.motorSelectable})),draft.motorAccount)):''}</div>
      <div class="sc-actions" style="margin:12px 0"><button class="sc-btn" data-template>Producto completo</button><span class="sc-note">Superpowers · infraestructura · diseño · investigación · x402</span></div>
      ${draft.templateReport?`<p class="sc-note">${mdEsc(draft.templateReport)}</p>`:''}
      <div class="sc-extensions">${inventoryGroup('skills','Skills')}${inventoryGroup('mcps','MCPs')}</div>
      <p class="sc-error" data-error role="status"></p><div class="sc-actions"><button class="sc-apply" data-save>Guardar perfil</button><button class="sc-btn" data-launch ${draft.id?'':'disabled'}>Nueva sesión con este perfil</button></div>`;
    body.querySelector('[name=name]').oninput=e=>{draft.name=e.target.value;body.querySelector('[data-launch]').disabled=true;};
    body.querySelector('[name=profile]').onchange=async e=>{const p=scProfilesCache.find(p=>p.id===e.target.value);if(p)draft=JSON.parse(JSON.stringify(p));else{delete draft.id;draft.name='Mi perfil';}try{await load();render();}catch(err){body.querySelector('[data-error]').textContent=err.message;}};
    body.querySelector('[name=harness]').onchange=async e=>{draft.harness=e.target.value;draft.motor=draft.harness;delete draft.id;draft.model='';draft.effort='';draft.routeId=draft.harness+':'+draft.motor;draft.skills={};draft.mcps={};try{await load();render();}catch(err){body.querySelector('[data-error]').textContent=err.message;}};
    for(const name of ['motor','model','effort','harnessAccount','motorAccount']){
      const field=body.querySelector(`[name=${name}]`);if(!field)continue;
      field.onchange=async()=>{
        const next=SessionConfig.update(PROVIDERS||{},{...draft,toHarness:draft.harness},name,field.value);
        delete next.toHarness;draft=next;draft.routeId=draft.harness+':'+draft.motor;
        if(name==='harnessAccount')await load();
        render();body.querySelector('[data-launch]').disabled=true;
      };
    }
    body.querySelectorAll('[data-kind]').forEach(el=>el.onchange=()=>{draft[el.dataset.kind]||={};draft[el.dataset.kind][el.dataset.id]=el.checked;body.querySelector('[data-launch]').disabled=true;});
    body.querySelector('[data-template]').onclick=()=>{
      draft.name='Producto completo';
      for(const x of inventory.skills||[]){if(x.toggleable!==false)draft.skills[x.id||x.name]=/superpowers|brainstorm|systematic-debug|executing-plans|verification-before|digitalocean|design-research|deep.research|x402/i.test(x.name+' '+x.path);}
      const families={Superpowers:/superpowers|brainstorm|systematic-debug/i,DigitalOcean:/digitalocean/i,Diseño:/design-research/i,Investigación:/deep.research/i,x402:/x402/i};
      const missing=Object.entries(families).filter(([,pattern])=>!(inventory.skills||[]).some(x=>x.toggleable!==false&&pattern.test(x.name+' '+x.path))).map(([name])=>name);
      draft.templateReport=missing.length?'Sin selección verificable para: '+missing.join(', ')+'. Revisa las skills disponibles.':'Las cinco familias tienen skills seleccionadas. Revisa la lista antes de guardar.';
      render();body.querySelector('[data-launch]').disabled=true;
    };
    body.querySelector('[data-save]').onclick=async e=>{e.target.disabled=true;try{
      const {templateReport,...saved}=draft;
      const r=await api('/session-profiles',saved);draft=r.profile||r;
      if(!draft.id)throw new Error('El servidor no devolvió el perfil guardado');
      await load();render();toast('Perfil guardado. Se aplica a nuevas sesiones.');
    }catch(err){body.querySelector('[data-error]').textContent=err.message;e.target.disabled=false;}};
    body.querySelector('[data-launch]').onclick=async()=>{try{
      await useSessionProfile(draft.id,current.cwd);document.querySelector('#session-workspace-modal').hidden=true;
    }catch(err){body.querySelector('[data-error]').textContent=err.message;}};
  }
  try{if(!PROVIDERS)PROVIDERS=await api('/providers');await load();render();}catch(e){body.textContent=e.message;}
}
async function useSessionProfile(id,cwd) {
  const r=await api('/session-profile-apply',{profileId:id,cwd:cwd||document.querySelector('#ns-cwd')?.value||''});
  const draft=r.launchDraft||{};
  NS.intoPane=null;
  await nsOpen();
  Object.assign(NS,draft,{profileId:id});
  if(draft.agent)NS.harness=draft.agent;
  if(draft.toHarness)NS.harness=draft.toHarness;
  if(cwd)document.querySelector('#ns-cwd').value=cwd;
  nsSelectDefaults();nsRender();
  const note=document.querySelector('#ns-profile-note');if(note)note.textContent='Perfil seleccionado. Skills y MCPs se aplicarán al iniciar.';
}

function renderSessionOverview(list) {
  const box=document.querySelector('#session-overview');
  if(!box||box.hidden)return;
  const rows=(list||[]).filter(it=>it.alive);
  const signature=JSON.stringify(rows.map(it=>[rowKey(it),it.project,it.agent,it.motor,it.model,it.effort,it.account,it.status,usageForItem(it)?.total_tokens]));
  if(box.dataset.signature===signature)return;
  box.dataset.signature=signature;
  box.innerHTML=rows.map(it=>{
    const usage=usageForItem(it)||{};
    const tokens=usage.tokens??usage.totalTokens??usage.total_tokens;
    return `<article class="overview-card" data-key="${attrEsc(rowKey(it))}"><h3>${mdEsc(it.project||it.session)} · ${mdEsc(it.pane||'')}</h3><p>${mdEsc(LABEL[it.status]||it.status)}</p><p>${mdEsc(it.agent||'shell')} → ${mdEsc(it.model||'modelo sin confirmar')}${it.effort?' · '+mdEsc(it.effort):''}</p><p>Cuenta ${mdEsc(it.harnessAccount||it.account||'sin confirmar')}${tokens!=null?' · '+mdEsc(fmtTokens(tokens))+' tokens':''}</p><div class="sc-actions"><button class="sc-btn" data-open>Terminal</button><button class="sc-btn" data-config>Configurar</button><button class="sc-btn" data-tools>Herramientas</button></div></article>`;
  }).join('')||'<p class="sc-note">No hay sesiones activas.</p>';
  box.querySelectorAll('[data-key]').forEach(card=>{
    const it=rows.find(i=>rowKey(i)===card.dataset.key);
    card.querySelector('[data-open]').onclick=()=>openSession(it);
    card.querySelector('[data-config]').onclick=e=>motorPopOpen(e.currentTarget,{mode:'session',item:it,model:it.model});
    card.querySelector('[data-tools]').onclick=()=>openSessionProfiles(it);
  });
}

function setChatVisible(visible) {
  document.documentElement.classList.toggle('chat-hidden',!visible);
  try{localStorage.setItem('cc-chat-visible',visible?'1':'0');}catch(e){}
  const button=document.querySelector('#toggle-chat');
  button.textContent=visible?'Ocultar chat':'Abrir chat';button.setAttribute('aria-expanded',String(visible));
  if(visible)document.querySelector('#op-in')?.focus({preventScroll:true});
  window.dispatchEvent(new Event('resize'));
}
function initSessionWorkspace() {
  let visible=true;try{visible=localStorage.getItem('cc-chat-visible')!=='0';}catch(e){}
  document.documentElement.classList.toggle('chat-hidden',!visible);
  const button=document.querySelector('#toggle-chat');button.textContent=visible?'Ocultar chat':'Abrir chat';button.setAttribute('aria-expanded',String(visible));
  button.onclick=()=>setChatVisible(document.documentElement.classList.contains('chat-hidden'));
  document.querySelector('#op-hide').onclick=()=>setChatVisible(false);
  document.querySelector('#op-action-history').onclick=openOperatorActions;
  document.querySelector('#open-session-profiles').onclick=()=>openSessionProfiles();
  document.querySelector('#open-extension-usage').onclick=()=>openExtensionUsage();
  document.querySelector('#toggle-overview').onclick=e=>{const b=document.querySelector('#session-overview');b.hidden=!b.hidden;e.currentTarget.setAttribute('aria-pressed',String(!b.hidden));renderSessionOverview(S.list||[]);};
  const input=document.querySelector('#op-in');
  try{input.value=localStorage.getItem('cc-chat-draft')||'';}catch(e){}
  input.addEventListener('input',()=>{try{localStorage.setItem('cc-chat-draft',input.value);}catch(e){}});
  input.addEventListener('keydown',e=>{if(e.key==='Enter'&&!e.shiftKey&&!e.isComposing){e.preventDefault();document.querySelector('#op-form').requestSubmit();}});
  document.querySelector('#ns-profile-open').onclick=()=>openSessionProfiles();
  document.querySelector('#ns-profile-clear').onclick=()=>{delete NS.profileId;document.querySelector('#ns-profile-note').textContent='Sin perfil';};
}

async function openOperatorActions() {
  const body=scDialog('Resultados de acciones');
  try {
    const data=await api('/operator/action-results');
    const labels={pending:'Sin confirmar',dispatched:'Enviada; consulta el resultado',confirmed:'Confirmada',failed:'Fallida'};
    body.innerHTML=`<p class="sc-note">Las acciones enviadas pueden seguir en curso. Revisa su resultado antes de repetirlas.</p><table class="sc-table"><thead><tr><th>Acción</th><th>Estado</th><th>Hora</th></tr></thead><tbody>${(data.actions||[]).map(a=>`<tr><td>${mdEsc(a.tool)}<br><small>${mdEsc(a.detail||'')}</small></td><td>${mdEsc(labels[a.status]||a.status)}</td><td>${new Date(a.updated*1000).toLocaleTimeString()}</td></tr>`).join('')}</tbody></table>`;
  }catch(e){body.textContent=e.message;}
}

const opExecutedActions=new Map();
async function opApplyActions(actions) {
  for(const a of actions||[]) {
    if(a.actionId&&opExecutedActions.has(a.actionId))continue;
    if(a.actionId)opExecutedActions.set(a.actionId,true);
    if(opExecutedActions.size>2000)opExecutedActions.delete(opExecutedActions.keys().next().value);
    let status='dispatched',detail='Acción enviada; efecto final sin confirmar.';
    try {
      if(a.type==='ui'&&a.op==='click') {
        const el=document.querySelector(a.selector);
        if(!el||el.disabled)throw new Error('El control no está disponible en esta vista');
        el.click();
      }else if(a.type==='ui'&&a.op==='call') {
        const fn=window[a.fn];
        if(typeof fn!=='function')throw new Error('Esta función no está disponible: '+a.fn);
        const result=fn(...(a.args||[]));
        if(result&&typeof result.then==='function'){await result;status='confirmed';detail='Función completada.';}
      }else if(a.type==='ui'&&a.target==='theme') {
        const theme=a.theme||a.value;
        if(!theme)throw new Error('Falta el tema');
        selectTheme(theme);status='confirmed';detail='Tema aplicado.';
      }else if((a.type==='ui'&&a.target==='lang')||a.type==='lang') {
        const lang=a.lang||a.value,button=document.querySelector(`.lang-btn[data-lang="${CSS.escape(lang||'')}"]`);
        if(!button)throw new Error('Idioma no disponible');button.click();
      }else if(a.type==='ui'&&a.op==='term') {
        const session=a.term?.session||activeTerm,frame=openTerms.get(session)?.frame;
        if(!frame?.contentWindow)throw new Error('Abre la terminal de destino primero');
        frame.contentWindow.postMessage({source:'comandos',type:'toolbar',term:a.term},location.origin);
      }else if(a.type==='pref'||a.type==='volume') {
        await api('/conf-set',{key:a.key||'VOLUME',value:a.type==='pref'?(a.on?'1':'0'):String(a.value)});
        await loadConf();status='confirmed';detail='Preferencia guardada.';
      }else {
        opApplyActionsLegacy([a]);
      }
    }catch(e){status='failed';detail=e.message||String(e);toast(detail,true);}
    if(a.actionId) {
      try{await api('/operator/action-result',{actionId:a.actionId,status,detail});}
      catch(e){toast('No se pudo guardar el resultado de la acción. Revisa antes de repetirla.',true);}
    }
    for(const message of OP.messages||[]) {
      const tool=(message.tools||[]).find(t=>t.name===a.toolName&&t.ok===null);
      if(tool){tool.ok=status==='confirmed'?true:status==='failed'?false:null;tool.reply=detail;}
    }
    opRenderLive();
  }
}
initSessionWorkspace();
