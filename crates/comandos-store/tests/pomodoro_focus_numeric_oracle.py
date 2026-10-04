"""Numeric/catch-scope fixture generator; not part of the native product."""
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT/'lib'))
import app_state
import pomodoro as p
T0=2000000000000
MIN=60000
p.uuid.uuid4=lambda:SimpleNamespace(hex='a')
values=[('inf',float('inf')),('minus-inf',-float('inf')),('nan',float('nan')),('huge-int',10**100),('huge-float',1e30),('malformed','bad'),('unicode','٢_٥')]
live=[]
base={'blockId':'legacy','until':(T0+MIN)/1000,'startedAt':T0/1000,'mins':1}
for name,value in values: live.append((f'mins-{name}',dict(base,mins=value)))
for first in ['bad',float('nan'),None]: live.append(('until-invalid-before-start-overflow',dict(base,until=first,startedAt=float('inf'))))
for name,value in values[:5]: live.append((f'until-{name}',dict(base,until=value)))
for name,value in values[:5]: live.append((f'started-{name}',dict(base,startedAt=value)))
live.append(('invalid-duration-before-sqlite-start-overflow',dict(base,startedAt=1e30,mins=181)))
history=[]
row={'id':'after','started_at_ms':T0,'planned_minutes':25,'status':'completed'}
for field in ['planned_minutes','started_at_ms','ended_at_ms']:
    for name,value in values: history.append((f'{field}-{name}',dict(row,**{field:value})))
history += [('invalid-plan-before-start-overflow',dict(row,planned_minutes='bad',started_at_ms=float('inf'))),('nan-plan-before-start-overflow',dict(row,planned_minutes=float('nan'),started_at_ms=float('inf'))),('huge-plan-before-invalid-start',dict(row,planned_minutes=10**100,started_at_ms='bad')),('negative-huge-plan',dict(row,planned_minutes=-10**100)),('negative-huge-float-plan',dict(row,planned_minutes=-1e30))]
commands=[]
for field in ['targetMs','expectedRevision','cycleIndex','cycleTotal','deltaMs']:
    for name,value in values:
        action='extend' if field=='deltaMs' else 'start'
        req={'requestId':'numeric','action':action,'targetMs':MIN,field:value}
        commands.append((f'{field}-{name}',req,field=='deltaMs'))
fixture=[]
for kind,cases in [('live',live),('history',history),('command',commands)]:
    for case in cases:
        name,data=case[:2]
        with tempfile.TemporaryDirectory() as tmp:
            conn=app_state.connect(Path(tmp)/'state.sqlite3');app_state.migrate(conn)
            s=p.PomodoroStore(conn,clock=lambda:T0)
            if kind=='history': data=[dict(row,id='before'),data]
            if kind=='command' and case[2]: s.command({'requestId':'start','action':'start','targetMs':25*MIN})
            try:
                result={'ok':True,'result':getattr(s,{'live':'import_legacy_focus','history':'import_legacy_history','command':'command'}[kind])(data)}
            except p.PomodoroError as e:
                result={'ok':False,'domain':{'status':e.status,'payload':e.payload()}}
            except Exception as e:
                result={'ok':False,'exception':type(e).__name__}
            fixture.append({'kind':kind,'name':name,'raw':json.dumps(data,ensure_ascii=False),'prestart':kind=='command' and case[2],'expected':result,'snapshot':s.snapshot(),'records':p.records(conn)})
print(json.dumps(fixture,ensure_ascii=False,sort_keys=True,indent=2))
