import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def run_js(script):
    result = subprocess.run(['node', '-e', "const S=require('./dash/session-config.js');\n" + script], cwd=ROOT, text=True, capture_output=True, check=True)
    return json.loads(result.stdout)


REGISTRY = """const registry={harnesses:{claude:{accounts:[{alias:'main',selectable:true}]},codex:{accounts:[{alias:'work',selectable:true}]}},motors:{claude:{models:[{id:'opus',efforts:['high'],defaultEffort:'high'}]},codex:{models:[{id:'gpt-6-astra',efforts:['high','ultra'],defaultEffort:'high'}]}},matrix:[{harness:'claude',motor:'claude',selectable:true},{harness:'codex',motor:'codex',selectable:true}]};"""


def test_combined_cli_account_effort_uses_one_consistent_draft():
    result = run_js(REGISTRY + """
      const before=S.draft({agent:'claude',model:'opus',effort:'high',account:'main'});
      let after=S.update(registry,before,'toHarness','codex');
      const invalid=S.validate(registry,after);
      after=S.update(registry,after,'harnessAccount','work');
      after=S.update(registry,after,'effort','ultra');
      console.log(JSON.stringify({before,after,invalid,valid:S.validate(registry,after),changed:S.changed(before,after)}));
    """)
    assert result['before']['toHarness'] == 'claude'
    assert result['invalid']
    assert result['valid'] == ''
    assert result['after']['model'] == 'gpt-6-astra'
    assert result['after']['harnessAccount'] == result['after']['motorAccount'] == 'work'
    assert result['after']['effort'] == 'ultra'
    assert result['changed']


def test_invalid_effort_cannot_submit_and_unchanged_draft_is_not_change():
    result = run_js(REGISTRY + """
      const before=S.draft({agent:'codex',model:'gpt-6-astra',effort:'high',account:'work'});
      console.log(JSON.stringify({same:S.changed(before,{...before,interrupt:true}),error:S.validate(registry,{...before,effort:'nonsense'})}));
    """)
    assert result['same'] is False
    assert result['error']


def test_acp_keeps_named_motor_account_separate_from_local_harness():
    result=run_js(REGISTRY+"""
      registry.harnesses.acp={};registry.matrix.push({harness:'acp',motor:'codex',selectable:true});
      let d=S.draft({agent:'codex',motor:'codex',model:'gpt-6-astra',effort:'high',account:'work'});
      d=S.update(registry,d,'toHarness','acp');
      d=S.update(registry,d,'motorAccount','work');
      console.log(JSON.stringify({d,error:S.validate(registry,d)}));
    """)
    assert result['d']['harnessAccount']=='main'
    assert result['d']['motorAccount']=='work'
    assert not result['error']


def test_direct_options_skip_unavailable_models_and_accounts():
    result = run_js(REGISTRY + """
      registry.motors.codex.models.push({id:'future',soon:true,efforts:['high']});
      registry.harnesses.codex.accounts.push({alias:'logged-out',selectable:false});
      const d=S.draft({agent:'codex',model:'gpt-6-astra',effort:'high',account:'work'});
      console.log(JSON.stringify({models:S.fieldOptions(registry,d,'model'),
        next:S.cycle(registry,d,'model',1),accounts:S.fieldOptions(registry,d,'harnessAccount'),
        invalid:S.validate(registry,{...d,model:'future'})}));
    """)
    assert result['models'][1]['disabled']
    assert result['next'] is None
    assert result['accounts'][1]['disabled']
    assert result['invalid']


def test_recommendations_keep_live_identity_and_filter_incompatible_configurations():
    result = run_js(REGISTRY + """
      const d=S.draft({agent:'codex',model:'gpt-6-astra',effort:'high',account:'work',
        observedConfig:{identity:{pid:99},conversationId:'live-conversation'}});
      const config={...d,effort:'ultra',expectedConversationId:'old',interrupt:true};
      const rows=[{config,count:3,lastUsed:3},{config,count:1,lastUsed:1},
        {config:{...config,harnessAccount:'logged-out'},count:10,lastUsed:5},
        {config:d,count:20,lastUsed:4}];
      console.log(JSON.stringify(S.recommendations(registry,d,rows,{agent:'codex'})));
    """)
    assert len(result) == 1
    assert result[0]['config']['expectedConversationId'] == 'live-conversation'
    assert result[0]['config']['expectedIdentity'] == {'pid': 99}
    assert result[0]['config']['interrupt'] is False


def test_direct_changes_require_confirmation_only_for_route_or_explicit_interruption():
    result = run_js(REGISTRY + """
      const d=S.draft({agent:'codex',model:'gpt-6-astra',effort:'high',account:'work'});
      console.log(JSON.stringify({effort:S.requiresConfirmation(d,{...d,effort:'ultra'}),
        account:S.requiresConfirmation(d,{...d,harnessAccount:'other'}),
        cli:S.requiresConfirmation(d,{...d,toHarness:'claude'}),
        motor:S.requiresConfirmation(d,{...d,motor:'claude'}),
        interrupt:S.requiresConfirmation(d,{...d,interrupt:true})}));
    """)
    assert result == dict(effort=False, account=False, cli=True, motor=True, interrupt=True)
