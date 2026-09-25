/* One shelf, one explicit pane. Every mutation uses the last observed scope and revision. */
(() => {
  'use strict';
  const esc = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  const terminal = new Set(['confirmed','failed','rolled_back']);
  const stages = {validating:'Comprobando selección…',waiting:'En espera: se aplicará al terminar el turno.',snapshot:'Guardando punto de recuperación…',applying:'Reanudando con la selección…',verifying:'Verificando el proceso…',recovering:'Recuperando la sesión anterior…',recovery_required:'Hace falta recuperar la sesión anterior.',awaiting_confirmation:'Revisa la terminal: hay una confirmación pendiente.',confirmed:'Selección verificada en el proceso.',rolled_back:'Se recuperó la sesión anterior.',failed:'No se aplicó el cambio.'};
  const harnesses = [['claude','Claude'],['codex','Codex'],['grok','Grok'],['opencode','OpenCode'],['agy','Antigravity']];
  const active = state => !!state?.operation && !terminal.has(state.operation.state);
  const diff = state => {
    if(!state?.loaded) return null;
    const out={add:0,remove:0};
    for(const kind of ['mcps','skills']) for(const [id,on] of Object.entries(state.desired[kind]||{})) {
      if(on !== state.loaded[kind]?.[id]) out[on?'add':'remove']++;
    }
    return out;
  };
  async function request(path, body) {
    const headers={};
    try { const token=localStorage.getItem('cc_token'); if(token)headers['X-Comandos-Token']=token; } catch(_){}
    if(body) headers['Content-Type']='application/json';
    const response=await fetch(path,{method:body?'POST':'GET',headers,body:body?JSON.stringify(body):undefined});
    const data=await response.json();
    if(!response.ok || data.ok===false) throw new Error(data.error||data.message||'No se pudo completar la acción.');
    return data;
  }
  function closeShelf() {
    if(window.webkit?.messageHandlers?.extensions) window.webkit.messageHandlers.extensions.postMessage('close');
    else if(parent!==window) parent.postMessage({type:'comandos-extensions-close'},location.origin);
  }
  class Shelf {
    constructor(root, target) {
      this.root=root;this.target=target;this.state=null;this.filter='all';this.query='';this.sending=false;this.error='';this.message='';this.generation=0;this.timer=null;this.stale=false;this.templateName='';
      root.addEventListener('click', e=>this.click(e));
      root.addEventListener('input', e=>{if(e.target.id==='ext-search'){this.query=e.target.value;this.render();}else if(e.target.id==='template-name')this.templateName=e.target.value;});
      root.addEventListener('change', e=>{if(e.target.id==='ext-harness')this.choose(e.target.value);});
      root.addEventListener('submit', e=>{e.preventDefault();const name=root.querySelector('#template-name')?.value.trim();if(name)this.mutate('/template',{name});});
      root.addEventListener('pointerdown', e=>this.drag(e));
      document.addEventListener('keydown',e=>{if(e.key==='Escape')closeShelf();});
      this.refresh();
    }
    guards() {const s=this.state;return {...this.target,expectedIdentity:s.identity,expectedConversationId:s.conversationId||'',revision:s.revision};}
    async choose(harness) {this.generation++;this.state=null;this.stale=false;this.target.harness=harness;this.error='';this.message='';await this.refresh();}
    async refresh() {
      clearTimeout(this.timer);
      if(this.sending){this.schedule();return;}
      if(!this.state){this.error='';this.render();}
      const generation=this.generation;
      try {
        const next=await request('/pane-extensions?'+new URLSearchParams(this.target));
        if(generation!==this.generation)return;
        if(this.state && (this.state.identity!==next.identity || this.state.conversationId!==next.conversationId) && !active(this.state)) {
          this.stale=true;this.error='El panel cambió de conversación. Actualiza antes de editar.';
        } else {this.state=next;this.stale=false;this.error='';}
      } catch(e) {if(generation===this.generation)this.error=e.message;}
      if(generation===this.generation){this.render();this.schedule();}
    }
    schedule() {clearTimeout(this.timer);this.timer=setTimeout(()=>this.refresh(),active(this.state)?2000:12000);}
    locked() {return this.sending||this.stale||active(this.state);}
    async mutate(suffix, extra={}) {
      if(this.sending||this.stale||!this.state)return;
      this.sending=true;clearTimeout(this.timer);this.error='';this.message='';this.render();
      const generation=++this.generation;
      try {
        const data=await request('/pane-extensions'+suffix,{...this.guards(),...extra});
        if(generation!==this.generation)return;
        const missing=[data.missing,data.unavailable].flatMap(value=>Array.isArray(value)?value:Object.values(value||{}).flat());
        if(missing.length) this.message='Plantilla cargada. No disponibles en este CLI: '+missing.map(x=>typeof x==='string'?x:(x.name||x.id)).join(', ');
        else if(suffix==='/template'&&extra.name)this.message='Plantilla guardada.';
        if(suffix==='/apply')this.state.operation={...data,state:data.state||'validating'};
      } catch(e) {if(generation===this.generation)this.error=e.message;}
      finally {
        if(generation===this.generation){const failure=this.error;this.sending=false;await this.refresh();if(failure){this.error=failure;this.render();}}
      }
    }
    toggle(kind,id,on) {
      if(this.locked())return;
      const row=this.state.inventory[kind]?.find(x=>x.id===id);
      if(!row||row.toggleable!==true||typeof this.state.desired[kind]?.[id]!=='boolean')return;
      const desired=JSON.parse(JSON.stringify(this.state.desired));desired[kind][id]=on;
      this.mutate('',{desired});
    }
    click(e) {
      if(this.dragged){this.dragged=false;return;}
      const button=e.target.closest('button');if(!button||button.disabled)return;
      if(button.dataset.kind)return this.toggle(button.dataset.kind,button.dataset.id,button.dataset.on!=='true');
      if(button.dataset.filter){this.filter=button.dataset.filter;this.render();return;}
      if(button.dataset.template)return this.mutate('/template',{templateId:button.dataset.template});
      switch(button.dataset.action){
        case 'close':closeShelf();break;
        case 'refresh':this.generation++;this.state=null;this.stale=false;this.refresh();break;
        case 'apply':this.apply(false);break;
        case 'interrupt':this.apply(true);break;
        case 'discard':if(this.state.loaded)this.mutate('',{desired:this.state.loaded});break;
        case 'unused':{
          const desired=JSON.parse(JSON.stringify(this.state.desired));
          for(const kind of ['mcps','skills']) for(const row of this.state.inventory[kind]||[]) if(row.toggleable&&this.state.usage?.counts?.[kind]?.[row.id]===0)desired[kind][row.id]=false;
          this.mutate('',{desired});break;
        }
        case 'cancel':this.mutate('/cancel',{operationId:this.state.operation.operationId||this.state.operation.id});break;
        case 'recover':this.mutate('/recover',{operationId:this.state.operation.operationId||this.state.operation.id});break;
      }
    }
    apply(interrupt) {
      if(this.locked()||this.state.applySupported===false)return;
      this.mutate('/apply',{requestId:crypto.randomUUID?crypto.randomUUID():`${Date.now()}-${Math.random().toString(16).slice(2)}`,interrupt});
    }
    drag(event) {
      const button=event.target.closest('.bubble');
      if(!button||button.disabled||event.button!==0)return;
      const x=event.clientX,y=event.clientY;let ghost=null;
      const move=e=>{
        if(!ghost&&Math.hypot(e.clientX-x,e.clientY-y)>7){ghost=button.cloneNode(true);ghost.classList.add('dragghost');document.body.append(ghost);this.dragged=true;}
        if(!ghost)return;
        e.preventDefault();ghost.style.left=e.clientX+'px';ghost.style.top=e.clientY+'px';
        const hit=document.elementFromPoint(e.clientX,e.clientY)?.closest('[data-zone]');
        this.root.querySelectorAll('[data-zone]').forEach(z=>z.classList.toggle('hot',z===hit));
      };
      const end=e=>{
        document.removeEventListener('pointermove',move);document.removeEventListener('pointerup',end);document.removeEventListener('pointercancel',end);
        this.root.querySelectorAll('[data-zone]').forEach(z=>z.classList.remove('hot'));
        if(ghost){ghost.remove();const zone=e.type!=='pointercancel'&&document.elementFromPoint(e.clientX,e.clientY)?.closest('[data-zone]');if(zone)this.toggle(button.dataset.kind,button.dataset.id,zone.dataset.zone==='on');setTimeout(()=>{this.dragged=false;},0);}
      };
      document.addEventListener('pointermove',move,{passive:false});document.addEventListener('pointerup',end);document.addEventListener('pointercancel',end);
    }
    bubble(row,kind,on) {
      const n=this.state.usage?.counts?.[kind]?.[row.id],use=n==null?'sin dato':n===0?'sin uso':`${n} usos`;
      const unknown=typeof this.state.desired[kind]?.[row.id]!=='boolean';
      const changed=this.state.loaded && !unknown && on!==this.state.loaded[kind]?.[row.id];
      const disabled=this.locked()||row.toggleable!==true||unknown;
      const label=`${row.name} · ${kind==='mcps'?'MCP':'Skill'} · ${use}${row.reason?' · '+row.reason:''}`;
      return `<button class="bubble ${kind} ${on?'':'off'}" data-kind="${kind}" data-id="${esc(row.id)}" data-on="${on}" ${disabled?'disabled':''} title="${esc(label)}" aria-label="${esc((on?'Quitar ':'Añadir ')+label)}" aria-pressed="${on}"><span class="name">${esc(row.name)}</span><small>${unknown?'estado sin dato':use}</small>${changed?`<span class="change">${on?'+':'−'}</span>`:''}</button>`;
    }
    render() {
      const focus=document.activeElement,id=focus?.id,position=focus?.selectionStart,value=focus?.value;
      const button=focus?.closest?.('button');
      const focusKey=button&&this.root.contains(button)?Object.fromEntries(['kind','id','filter','template','action'].filter(k=>button.dataset[k]!=null).map(k=>[k,button.dataset[k]])):null;
      const restoreKey=focusKey||(!id?this.pendingFocus:null);
      this.pendingFocus=this.sending?restoreKey:null;
      const scrolls=[...this.root.querySelectorAll('.field,.templates')].map(x=>x.scrollTop);
      const s=this.state;
      this.root.setAttribute('aria-busy',String(!s&&!this.error));
      if(!s){this.enterOnReady=true;this.root.innerHTML=`<section class="shelf"><header><strong>Extensiones · ${esc(this.target.session)} · ${esc(this.target.pane)}</strong><span class="spacer"></span><button data-action="close" aria-label="Cerrar estante">×</button></header><div class="loading" role="status">${this.error?'':'<span class="loading-spinner" aria-hidden="true"></span>'}<span>${esc(this.error||'Cargando MCPs y skills…')}</span>${this.error?'<button data-action="refresh">Reintentar</button>':''}</div>${!this.target.harness?this.harnessPicker():''}</section>`;return;}
      const enter=this.enterOnReady;this.enterOnReady=false;
      const d=diff(s),pending=d&&(d.add+d.remove),locked=this.locked(),op=s.operation,opState=op?.state;
      const items=['mcps','skills'].flatMap(kind=>(s.inventory[kind]||[]).map(row=>({row,kind,on:s.desired[kind]?.[row.id]===true})));
      const visible=items.filter(x=>(x.on||(x.row.toggleable===true&&s.desired[x.kind]?.[x.row.id]===false))&&(this.filter==='all'||this.filter===x.kind)&&x.row.name.toLowerCase().includes(this.query.toLowerCase()));
      const unused=items.filter(x=>x.on&&x.row.toggleable&&s.usage?.counts?.[x.kind]?.[x.row.id]===0).length;
      const excluded=items.filter(x=>x.row.toggleable!==true);
      const operationNotice=opState?[stages[opState]||opState,op.error].filter(Boolean).join(' '):'';
      const notice=this.error||this.message||operationNotice||s.reason||(!s.loaded?'Aún no hemos comprobado qué MCPs y skills cargó este proceso. Puedes aplicar la selección al terminar el turno.':'');
      this.root.innerHTML=`<section class="shelf${enter?' shelf-enter':''}"><header><span class="label">Extensiones</span><strong>${esc(this.target.session)} · ${esc(this.target.pane)} · ${esc(harnesses.find(x=>x[0]===s.harness)?.[1]||s.harness)}</strong><span class="muted">Solo este panel</span><span class="spacer"></span><span class="muted">${d?pending?`+${d.add} / −${d.remove} pendientes`:'Sin cambios':'Carga sin verificar'}</span>${pending?`<button data-action="discard" ${locked?'disabled':''}>Deshacer</button>`:''}<button class="go" data-action="apply" ${locked||s.applySupported===false||(!pending&&s.loaded)?'disabled':''}>${active(s)?'Aplicando…':!s.conversationId?'Iniciar con este set':s.busy?'Aplicar al terminar':'Aplicar y reanudar'}</button><button class="close" data-action="close" aria-label="Cerrar estante">×</button></header>
      <div class="notice ${this.error||['failed','recovery_required'].includes(opState)?'error':''}" role="status">${esc(notice)}${this.stale?'<button data-action="refresh">Actualizar panel</button>':''}${['validating','waiting','snapshot'].includes(opState)?'<button data-action="cancel">Cancelar espera</button>':''}${['recovery_required','awaiting_confirmation'].includes(opState)?'<button data-action="recover">Recuperar sesión anterior</button>':''}${s.busy&&!locked&&s.applySupported!==false?'<button data-action="interrupt">Interrumpir turno y aplicar</button>':''}</div>
      ${!s.conversationId?this.harnessPicker():''}
      <div class="toolbar"><input id="ext-search" type="search" placeholder="Buscar extensión…" aria-label="Buscar extensión" value="${esc(this.query)}">${[['all','Todo'],['mcps','MCPs'],['skills','Skills']].map(([k,l])=>`<button data-filter="${k}" class="${this.filter===k?'active':''}" aria-pressed="${this.filter===k}">${l}</button>`).join('')}<button data-action="unused" ${locked||!unused?'disabled':''}>Apagar ${unused} sin uso</button><span class="spacer"></span><span class="muted gesture">Toca o arrastra · azul MCP · ámbar skill</span></div>
      <div class="body"><aside class="templates"><div class="label">Plantillas</div>${(s.templates||[]).map(t=>`<button data-template="${esc(t.id)}" ${locked?'disabled':''}>${esc(t.name)}</button>`).join('')}<form><input id="template-name" value="${esc(this.templateName)}" placeholder="Nombre del set" aria-label="Nombre de la plantilla" maxlength="80" required ${locked?'disabled':''}><button type="submit" data-action="save-template" ${locked?'disabled':''}>+ Guardar set</button></form><p class="muted">Disponibles entre agentes. Se cargan solo cuando las eliges.</p></aside>${[true,false].map(on=>`<div class="zone ${on?'on':'off'}" data-zone="${on?'on':'off'}"><div class="label">${on?'Seleccionadas':'Disponibles'} · ${visible.filter(x=>x.on===on).length}</div><span class="muted">${on?'Selección guardada para este panel':'Puedes añadirlas a este panel'}</span><div class="field">${visible.filter(x=>x.on===on).map(x=>this.bubble(x.row,x.kind,on)).join('')||'<div class="empty">Ninguna con este filtro.</div>'}</div></div>`).join('')}</div>
      <footer><span>${s.inventory.mcps.length} MCPs + ${s.inventory.skills.length} skills</span><span>Tokens: sin medición</span><span>${s.usage?.complete?'Uso registrado en esta conversación':'Uso parcial: ausencia de datos ≠ sin uso'}</span>${excluded.length?`<details><summary>${excluded.length} excluidas o gestionadas aparte</summary>${excluded.map(x=>`<p><b>${esc(x.row.name)}</b> · ${esc(x.row.reason||'Gestionada fuera de este panel')}</p>`).join('')}</details>`:''}</footer></section>`;
      this.root.querySelectorAll('.field,.templates').forEach((x,i)=>x.scrollTop=scrolls[i]||0);
      if(id){const next=document.getElementById(id);if(next){if(id==='template-name')next.value=value;next.focus({preventScroll:true});if(typeof position==='number'&&next.setSelectionRange)try{next.setSelectionRange(position,position);}catch(_){}}}
      else if(restoreKey&&Object.keys(restoreKey).length){const next=[...this.root.querySelectorAll('button')].find(el=>Object.entries(restoreKey).every(([k,v])=>el.dataset[k]===v));if(next&&!next.disabled)next.focus({preventScroll:true});}
    }
    harnessPicker() {return `<div class="toolbar"><label for="ext-harness">CLI para iniciar</label><select id="ext-harness" ${this.sending?'disabled':''}><option value="">Elige un CLI…</option>${harnesses.map(([h,n])=>`<option value="${h}" ${this.target.harness===h?'selected':''}>${n}</option>`).join('')}</select></div>`;}
  }
  // The dashboard shelf height is the user's: drag the top edge, persisted per browser.
  const SHELF_KEY='cc_pane_shelf_height',SHELF_MIN=180,TERMINAL_MIN=120,SHELF_STEP=24;
  const rootStyle=()=>document.documentElement.style;
  const shelfFrame=()=>document.getElementById('pane-extensions-frame');
  const savedShelfHeight=()=>{try{return parseInt(localStorage.getItem(SHELF_KEY),10)||0;}catch(_){return 0;}};
  const shelfLimit=()=>{
    const heights=[window.visualViewport?.height,window.innerHeight,document.documentElement.clientHeight].filter(h=>h>0);
    return Math.max(SHELF_MIN,(heights.length?Math.min(...heights):SHELF_MIN+TERMINAL_MIN)-TERMINAL_MIN);
  };
  function setShelfHeight(px,save) {
    const height=Math.round(Math.max(SHELF_MIN,Math.min(px,shelfLimit())));
    rootStyle().setProperty('--pane-shelf-height',height+'px');
    document.getElementById('pane-extensions-grip')?.setAttribute('aria-valuenow',height);
    if(save)try{localStorage.setItem(SHELF_KEY,String(height));}catch(_){}
  }
  function restoreShelfHeight() {
    const saved=savedShelfHeight();
    if(saved)setShelfHeight(saved,false);else rootStyle().removeProperty('--pane-shelf-height');
    const grip=document.getElementById('pane-extensions-grip'),frame=shelfFrame();
    if(grip&&frame){grip.setAttribute('aria-valuemax',Math.round(shelfLimit()));grip.setAttribute('aria-valuenow',Math.round(frame.getBoundingClientRect().height));}
  }
  function shelfGrip() {
    let grip=document.getElementById('pane-extensions-grip');
    if(grip)return grip;
    grip=document.createElement('div');grip.id='pane-extensions-grip';grip.tabIndex=0;grip.title='Arrastra para cambiar la altura · doble clic restablece';
    for(const [k,v] of Object.entries({role:'separator','aria-orientation':'horizontal','aria-label':'Altura del estante de extensiones','aria-valuemin':SHELF_MIN}))grip.setAttribute(k,v);
    let start=null,last=0;
    const current=()=>shelfFrame()?.getBoundingClientRect().height||SHELF_MIN;
    const refit=force=>{const now=performance.now();if(force||now-last>100){last=now;window.dispatchEvent(new Event('resize'));}};
    grip.addEventListener('pointerdown',e=>{if(e.button!==0)return;e.preventDefault();grip.setPointerCapture(e.pointerId);start={y:e.clientY,height:current()};document.body.classList.add('pane-extensions-resizing');});
    grip.addEventListener('pointermove',e=>{if(!start)return;setShelfHeight(start.height+start.y-e.clientY,false);refit(false);});
    const end=()=>{if(!start)return;start=null;document.body.classList.remove('pane-extensions-resizing');setShelfHeight(current(),true);refit(true);};
    for(const type of ['pointerup','pointercancel','lostpointercapture'])grip.addEventListener(type,end);
    grip.addEventListener('keydown',e=>{
      const next={ArrowUp:current()+SHELF_STEP,ArrowDown:current()-SHELF_STEP,Home:shelfLimit(),End:SHELF_MIN}[e.key];
      if(next===undefined)return;e.preventDefault();setShelfHeight(next,true);refit(true);
    });
    grip.addEventListener('dblclick',()=>{try{localStorage.removeItem(SHELF_KEY);}catch(_){}restoreShelfHeight();refit(true);});
    document.body.append(grip);
    return grip;
  }
  function closeFrame() {
    shelfFrame()?.remove();document.getElementById('pane-extensions-grip')?.remove();
    document.body.classList.remove('pane-extensions-open','pane-extensions-resizing');window.dispatchEvent(new Event('resize'));
  }
  // A smaller window must never push the shelf over the terminals; a larger one restores the saved height.
  const resizeShelf=()=>{if(shelfFrame()&&savedShelfHeight()&&!document.body.classList.contains('pane-extensions-resizing'))setShelfHeight(savedShelfHeight(),false);};
  window.addEventListener('resize',resizeShelf);
  window.visualViewport?.addEventListener('resize',resizeShelf);
  // Dashboard and desktop load the exact same page; bubbles never enter terminal panes.
  window.openPaneExtensions=(session,pane,harness='')=>{
    if(!session||!/^%\d+$/.test(pane))return;
    if(window.webkit?.messageHandlers?.centro){window.webkit.messageHandlers.centro.postMessage(JSON.stringify({type:'extensions',session,pane,harness}));return;}
    let frame=document.getElementById('pane-extensions-frame');
    if(!frame){frame=document.createElement('iframe');frame.id='pane-extensions-frame';frame.title='Extensiones del panel seleccionado';Object.assign(frame.style,{position:'fixed',left:'var(--app-left,0px)',top:'calc(var(--app-top,0px) + var(--app-height,100dvh) - var(--pane-shelf-height,55vh))',width:'var(--app-width,100%)',height:'var(--pane-shelf-height,55vh)',border:'0',zIndex:1000,boxShadow:'0 -12px 40px #0007'});document.body.append(frame);shelfGrip();restoreShelfHeight();document.body.classList.add('pane-extensions-open');window.dispatchEvent(new Event('resize'));}
    const target={session,pane};if(harness&&harness!=='shell')target.harness=harness;
    frame.src='/extensions.html?'+new URLSearchParams(target);
  };
  window.addEventListener('message',event=>{const frame=document.getElementById('pane-extensions-frame');if(event.origin===location.origin&&event.source===frame?.contentWindow&&event.data?.type==='comandos-extensions-close')closeFrame();});
  const root=document.getElementById('extensions');
  if(root){const query=new URLSearchParams(location.search),target={session:query.get('session')||'',pane:query.get('pane')||''};if(query.get('harness'))target.harness=query.get('harness');new Shelf(root,target);}
})();
