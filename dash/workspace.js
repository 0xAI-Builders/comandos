/* Session controls share the dashboard's observed state and authenticated API. */
function scOption(value, label, selected, disabled=false) {
  return `<option value="${attrEsc(value)}" ${value===selected?'selected':''} ${disabled?'disabled':''}>${mdEsc(label)}</option>`;
}
function scSelect(name, label, options) {
  return `<label class="sc-field">${label}<select name="${name}">${options}</select></label>`;
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

function initSessionWorkspace() {
  document.querySelector('#open-session-profiles').onclick=()=>openSessionProfiles();
  document.querySelector('#open-extension-usage').onclick=()=>openExtensionUsage();
  document.querySelector('#toggle-overview').onclick=e=>{const b=document.querySelector('#session-overview');b.hidden=!b.hidden;e.currentTarget.setAttribute('aria-pressed',String(!b.hidden));renderSessionOverview(S.list||[]);};
  document.querySelector('#ns-profile-open').onclick=()=>openSessionProfiles();
  document.querySelector('#ns-profile-clear').onclick=()=>{delete NS.profileId;document.querySelector('#ns-profile-note').textContent='Sin perfil';};
}

initSessionWorkspace();
