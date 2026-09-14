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
    if(c.models.length && !c.models.some(m => m.id === state.model)) return 'Selecciona un modelo disponible';
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
  const api = {draft, route, choices, update, validate, changed, switchError};
  if(typeof module !== 'undefined') module.exports = api;
  else root.SessionConfig = api;
})(typeof window === 'undefined' ? globalThis : window);
