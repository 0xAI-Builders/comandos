"""Offline fixture generator only; never used by the product."""
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / 'lib'))
import app_state
import pomodoro as p
import focus_progress as f
MIN = 60000
T0 = 2000000000000
now = T0
counter = 0
def fresh():
    global counter
    counter += 1
    return SimpleNamespace(hex=f'b{counter}')
p.uuid.uuid4 = fresh
steps = []
def req(rid, action, **kw):
    return dict(requestId=rid, action=action, **kw)
with tempfile.TemporaryDirectory() as tmp:
    conn = app_state.connect(Path(tmp) / 'state.sqlite3')
    app_state.migrate(conn)
    f.ensure_policy(conn, f.POLICY_V1, T0)
    store = p.PomodoroStore(conn, clock=lambda: now, rewards=lambda c,r,t:f.award(c,r,f.POLICY_V1,t))
    def command(request, at):
        global now
        now=at
        try: result=store.command(request)
        except p.PomodoroError as e: result={'status':e.status,'payload':e.payload()}
        steps.append({'now':at,'request':request,'result':result,'snapshot':store.snapshot()})
    command(None,T0)
    command(req(' ', 'start', targetMs=MIN),T0)
    command(req('bad-action','finish'),T0)
    command(req('bad-expected','start',expectedRevision=True,targetMs=MIN),T0)
    command(req('bad-target','start',targetMs=30_000),T0)
    command(req('bad-mode','start',targetMs=MIN,mode=None),T0)
    command(req('bad-dest','start',targetMs=MIN,project='a\x01b'),T0)
    first=req('r','start',expectedRevision=0,targetMs=25*MIN,project='  México 😊  ',sessionKey=['a',True,1.0],paneKey={'x':None},cycleIndex=-100,cycleTotal=999)
    command(first,T0)
    command(req('active','start',targetMs=MIN),T0)
    command(req('stale','pause',expectedRevision=0),T0)
    command(req('pause','pause'),T0+2*MIN)
    command(req('pause-again','pause'),T0+3*MIN)
    command(req('zero','extend',deltaMs=0),T0+3*MIN)
    command(req('small','extend',deltaMs=-24*MIN),T0+3*MIN)
    command(req('large','extend',deltaMs=10**100),T0+3*MIN)
    command(req('extend','extend',deltaMs=5*MIN,project='Other'),T0+3*MIN)
    command(req('resume','resume'),T0+62*MIN)
    command(first,T0+63*MIN)
    command(dict(first,targetMs=MIN),T0+63*MIN)
    command(req('cancel','cancel'),T0+64*MIN)
    command(req('not-live','resume'),T0+64*MIN)
    command(req('break','start',expectedRevision=5,targetMs=5*MIN,mode='break'),T0+65*MIN)
    command(req('due-invalid','start',targetMs=0),T0+71*MIN)
    command(req('after-completion','start',expectedRevision=6,targetMs=25*MIN),T0+71*MIN)
    command(req('due-pause','pause'),T0+97*MIN)
    history=[{'id':'old','started_at_ms':T0,'planned_minutes':'25','status':'completed','project':'MRP'},{'id':'unknown','started_at_ms':T0,'planned_minutes':50,'status':'running','project':'MRP'},{'id':'skip','started_at_ms':T0,'planned_minutes':5,'status':'skipped','mode':'break'},{'id':'bad','started_at_ms':'invalid'},{'id':'zero','started_at_ms':0,'planned_minutes':-1,'status':'completed'}]
    imported=store.import_legacy_history(history,now_ms=now)
    fixture={'steps':steps,'history':history,'imported':imported,'records':p.records(conn),'reports':[{'from':T0-1,'to':T0+100*MIN,'project':project,'timezone':tz,'report':p.focus_report(conn,T0-1,T0+100*MIN,project,tz)} for project,tz in [(None,p.REPORT_TZ),('MRP',p.REPORT_TZ),(None,'Asia/Tokyo')]],'progress':f.ledger_progress(conn,f.POLICY_V1,now),'digests':[{'request':r,'digest':store._digest(r)} for r in [first,req('n','start',targetMs=10**100,extra=[-0.0,1e-5,True,None])]]}
    print(json.dumps(fixture,ensure_ascii=False,sort_keys=True,indent=2))
