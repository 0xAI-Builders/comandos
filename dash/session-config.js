/* Shared draft rules: selecting options never executes a session operation. */
(function(root) {
  function draft(item) {
    const harness = item.agent || 'shell';
    return {toHarness:harness, motor:item.motor || harness, model:item.model || '',
      effort:item.effort || '', harnessAccount:item.harnessAccount || item.account || 'main',
      motorAccount:item.motorAccount || item.account || 'main', interrupt:false,
      expectedIdentity:item.observedConfig?.identity || undefined,
      expectedConversationId:item.observedConfig?.conversationId || undefined};
  }
  function route(registry, state) {
    return (registry.matrix || []).find(r => r.harness === state.toHarness && r.motor === state.motor);
  }
  function choices(registry, state) {
    const r = route(registry, state);
    const models = ((registry.motors || {})[state.motor] || {}).models || [];
    const model = models.find(m => m.id === state.model);
    const provider = state.toHarness === 'acp' ? state.motor : state.toHarness;
    const accounts = ((registry.harnesses || {})[provider] || {}).accounts || [];
    const motorAccounts = ((registry.harnesses || {})[state.motor] || {}).accounts || [];
    return {route:r, models, efforts:model?.efforts || [], accounts, motorAccounts};
  }
  function update(registry, state, field, value) {
    const next = {...state, [field]:value};
    if(field === 'toHarness' && !route(registry, next)?.selectable) {
      const r = (registry.matrix || []).find(r => r.harness === value && r.motor === value && r.selectable)
        || (registry.matrix || []).find(r => r.harness === value && r.selectable);
      next.motor = r?.motor || value;
    }
    const c = choices(registry, next);
    if(['toHarness','motor'].includes(field)) {
      if(!c.models.some(m => m.id === next.model)) next.model = c.models[0]?.id || '';
      // Accounts are deliberately retained: an invalid explicit choice needs user correction.
    }
    if(['toHarness','motor','model'].includes(field)) {
      const m = c.models.find(m => m.id === next.model);
      if(!(m?.efforts || []).includes(next.effort)) next.effort = m?.defaultEffort || m?.efforts?.[0] || '';
    }
    if(next.toHarness === 'acp') next.harnessAccount = 'main';
    else if(next.motor === next.toHarness) next.motorAccount = next.harnessAccount;
    return next;
  }
  function switchError(registry, state, source) {
    if(!source) return '';
    if(source.agent && !['shell','claude','codex','grok','acp'].includes(source.agent))
      return 'Este CLI no ofrece recuperación exacta. Puedes iniciar una sesión nueva con otro CLI.';
    const rule = registry.midSessionRoutes?.[route(registry,state)?.id || state.toHarness+':'+state.motor];
    if(!rule || rule.selectable) return '';
    const observed = source.observedConfig || {};
    if(rule.reason?.code === 'acp_effort_unobserved' && observed.harness === 'acp' &&
       observed.motor === state.motor && observed.effortSource === 'acp-config-options') return '';
    return rule.reason?.message || 'Este cambio no está disponible en una sesión abierta';
  }
  function validate(registry, state, source) {
    const c = choices(registry, state);
    if(!c.route?.selectable) return c.route?.reason?.message || 'Esta combinación no está disponible';
    const unsupported = switchError(registry,state,source);
    if(unsupported) return unsupported;
    if(c.models.length && !c.models.some(m => m.id === state.model && !m.soon)) return 'Selecciona un modelo disponible';
    if(state.effort && !c.efforts.includes(state.effort)) return 'Selecciona un esfuerzo compatible';
    const account = state.toHarness === 'acp' ? state.motorAccount : state.harnessAccount;
    if(c.accounts.length && !c.accounts.some(a => a.alias === account && a.selectable)) return 'Selecciona una cuenta con sesión iniciada';
    if(state.motor !== state.toHarness && state.toHarness !== 'acp' && c.motorAccounts.length &&
       !c.motorAccounts.some(a => a.alias === state.motorAccount && a.motorSelectable)) return 'Selecciona una cuenta compatible para el motor';
    return '';
  }
  function changed(before, after) {
    return ['toHarness','motor','model','effort','harnessAccount','motorAccount'].some(k => (before[k] || '') !== (after[k] || ''));
  }
  const configFields = ['toHarness','motor','model','effort','harnessAccount','motorAccount'];
  function configuration(base, values) {
    const next = {...base, interrupt:false};
    for(const key of configFields) if(typeof values?.[key] === 'string') next[key] = values[key];
    return next;
  }
  function requiresConfirmation(before, after) {
    return before.toHarness !== after.toHarness || before.motor !== after.motor || !!after.interrupt;
  }
  function fieldOptions(registry, state, field, source) {
    const c = choices(registry,state);
    if(field === 'model') return c.models.map(m => ({value:m.id,label:m.name||m.id,disabled:!!m.soon,reason:m.soon?'Próximamente':''}));
    if(field === 'effort') return c.efforts.map(value => ({value,label:value,disabled:false}));
    if(field === 'toHarness' || field === 'motor') {
      const values = [...new Set((registry.matrix||[]).filter(r => field==='toHarness'||r.harness===state.toHarness).map(r => field==='toHarness'?r.harness:r.motor))];
      return values.map(value => {
        const next=update(registry,state,field,value), r=route(registry,next);
        const reason=!r?.selectable ? (r?.reason?.message||'Ruta no disponible') : switchError(registry,next,source);
        return {value,label:(field==='toHarness'?registry.harnesses:registry.motors)?.[value]?.label||value,disabled:!!reason,reason};
      });
    }
    const motorAccount = field==='motorAccount' && state.toHarness!=='acp' && state.motor!==state.toHarness;
    return (motorAccount?c.motorAccounts:c.accounts).map(a=>({value:a.alias,label:a.alias,
      detail:a.identity||'',disabled:!(motorAccount?a.motorSelectable:a.selectable),reason:'Cuenta sin sesión compatible'}));
  }
  function cycle(registry, state, field, direction, source) {
    const rows=fieldOptions(registry,state,field,source).filter(o=>!o.disabled);
    if(!rows.length) return null;
    const i=rows.findIndex(o=>o.value===state[field]);
    const next=rows[i<0?0:(i+(direction<0?-1:1)+rows.length)%rows.length].value;
    return next===state[field]?null:next;
  }
  function recommendations(registry, state, rows, source) {
    const seen=new Set();
    return (Array.isArray(rows)?rows:[]).filter(r=>r?.config && Number.isFinite(r.count) && r.count>0)
      .slice().sort((a,b)=>b.count-a.count || (Number(b.lastUsed)||0)-(Number(a.lastUsed)||0) ||
        JSON.stringify(configFields.map(k=>a.config[k]||'')).localeCompare(JSON.stringify(configFields.map(k=>b.config[k]||''))))
      .flatMap(r=>{
        if(!configFields.every(k=>typeof r.config[k]==='string'))return [];
        const config=configuration(state,r.config), key=JSON.stringify(configFields.map(k=>config[k]));
        if(seen.has(key)||!changed(state,config)||validate(registry,config,source))return [];
        // History cannot legitimize unknown or no-longer-installed accounts/models.
        if(!fieldOptions(registry,config,'model',source).some(o=>o.value===config.model&&!o.disabled))return [];
        const accountFields=config.toHarness==='acp'?['motorAccount']:config.motor===config.toHarness?['harnessAccount']:['harnessAccount','motorAccount'];
        if(accountFields.some(k=>!fieldOptions(registry,config,k,source).some(o=>o.value===config[k]&&!o.disabled)))return [];
        seen.add(key);return [{...r,config}];
      }).slice(0,3);
  }
  const api = {draft, route, choices, update, validate, changed, switchError,
    configuration, requiresConfirmation, fieldOptions, cycle, recommendations};
  if(typeof module !== 'undefined') module.exports = api;
  else root.SessionConfig = api;
})(typeof window === 'undefined' ? globalThis : window);
