"""Controller tests execute production JS without a browser or live terminal."""
import subprocess
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]


def run_controller(body):
    script=r"""
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict');
const SessionConfig=require('./dash/session-config.js');
const PROVIDERS={harnesses:{codex:{accounts:[{alias:'main',selectable:true}]},claude:{accounts:[{alias:'main',selectable:true}]}},motors:{codex:{models:[{id:'astra',name:'Astra',efforts:['low','high']},{id:'sol',name:'Sol',efforts:['low','high']}]},claude:{models:[{id:'opus',efforts:['high']}]}},matrix:[{harness:'codex',motor:'codex',selectable:true},{harness:'claude',motor:'claude',selectable:true}]};
const item={session:'test',pane:'%7',agent:'codex',motor:'codex',model:'astra',effort:'high',account:'main',observedConfig:{identity:{pid:42},conversationId:'source'}};
const MOTOR_PENDING=new Map(), motorTargetKey=i=>i.session+'|'+i.pane;
let MPOP={ctx:{mode:'session',item},original:SessionConfig.draft(item),draft:SessionConfig.draft(item)},requests=[];
const S={list:[item]},crypto=require('node:crypto');
const renderCentro=()=>{},toast=()=>{},sessionStageLabel=r=>r.stage||r.state;
let api=async(path,payload)=>{requests.push({path,payload});return {operationId:'operation-test',operationKey:'test|%7',queued:true};};
eval(fs.readFileSync('./dash/session-controls.js','utf8'));
renderSessionConfig=()=>{};
scLoadHistory=()=>{};
(async()=>{
"""+body+r"""
})().catch(e=>{console.error(e);process.exitCode=1});
"""
    result=subprocess.run(['node','-e',script],cwd=ROOT,capture_output=True,text=True)
    assert result.returncode == 0, result.stderr


def test_direct_choice_submits_exact_pane_once_and_keeps_observed_original():
    run_controller("""
      const before=JSON.stringify(MPOP.original);
      await scChoose(MPOP,'model','sol');
      await scChoose(MPOP,'model','astra');
      assert.equal(requests.length,1);
      assert.equal(requests[0].path,'/session/configure');
      assert.equal(requests[0].payload.pane,'%7');
      assert.equal(requests[0].payload.expectedConversationId,'source');
      assert.equal(requests[0].payload.interrupt,false);
      assert.equal(JSON.stringify(MPOP.original),before);
      assert.equal(MPOP.draft.model,'sol');
    """)


def test_route_requires_confirmation_and_invalid_account_never_submits():
    run_controller("""
      await scChoose(MPOP,'toHarness','claude');
      assert.equal(requests.length,0);
      assert.equal(MPOP.confirmation,'route');
      await scApply(MPOP);
      assert.equal(requests.length,0);
      await scApply(MPOP,true);
      assert.equal(requests.length,1);
      MOTOR_PENDING.clear();MPOP.confirmation=null;MPOP.draft={...MPOP.original,harnessAccount:'missing'};
      await scApply(MPOP,true);
      assert.equal(requests.length,1);
    """)


def test_delayed_response_cannot_replace_another_panes_context():
    run_controller("""
      let resolve;api=(path,payload)=>{requests.push({path,payload});return new Promise(r=>resolve=r)};
      const context=MPOP;
      const work=scChoose(context,'model','sol');
      await scChoose(context,'effort','low');
      assert.equal(requests.length,1);
      MPOP={ctx:{mode:'session',item:{...item,pane:'%8'}}};
      resolve({operationId:'operation-test',operationKey:'test|%7'});await work;
      assert.equal(MPOP.ctx.item.pane,'%8');assert(MOTOR_PENDING.has('test|%7'));
      assert.equal(requests[0].payload.effort,'high');
    """)


def test_confirmed_status_rebases_identity_and_draft_but_failed_status_does_not_claim_success():
    run_controller("""
      MPOP.draft={...MPOP.draft,model:'sol'};
      scSessionResult(item,{ok:true,state:'confirmed',harness:'codex',motor:'codex',model:'sol',effort:'low',harnessAccount:'main',motorAccount:'main',identity:{pid:84},conversationId:'dest'});
      assert.equal(MPOP.original.model,'sol');assert.equal(MPOP.draft.model,'sol');
      assert.equal(MPOP.draft.expectedIdentity.pid,84);assert.equal(MPOP.draft.expectedConversationId,'dest');
      MPOP.draft={...MPOP.draft,model:'astra'};
      scSessionResult(item,{ok:false,state:'failed',detail:'Could not verify'});
      assert.equal(MPOP.original.model,'sol');assert.equal(MPOP.draft.model,'astra');
      assert.equal(MPOP.error,'Could not verify');
    """)


def test_cancel_interrupt_confirmation_preserves_queued_request():
    run_controller("""
      await scChoose(MPOP,'model','sol');
      MPOP.confirmation='interrupt';
      const id=MPOP.requestId;
      scDiscard(MPOP);
      assert.equal(MPOP.draft.model,'sol');assert.equal(MPOP.requestId,id);
      assert.equal(MPOP.confirmation,null);assert.equal(MOTOR_PENDING.size,1);
    """)


def test_late_initial_waiting_status_cannot_resurrect_confirmed_operation():
    run_controller("""
      let resolve;api=()=>new Promise(r=>resolve=r);
      const loading=scLoadStatus(MPOP);
      scSessionResult(item,{ok:true,state:'confirmed',harness:'codex',motor:'codex',model:'sol',effort:'high',harnessAccount:'main',motorAccount:'main'});
      resolve({operationId:'operation-test',stage:'waiting',state:'waiting'});await loading;
      assert.equal(MOTOR_PENDING.size,0);assert.equal(MPOP.statusLoading,false);
    """)
