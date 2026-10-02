"""Original SQLite behavior, synthetic memory database, no application home."""
import json
import sqlite3
import sys
from pathlib import Path
assert Path('/work/scripts/rust-sandbox').exists(), 'Run inside rust-sandbox'
sys.path.insert(0,'/work/lib')
import event_store
import notification_delivery as nd
base='/work/crates/comandos-store/tests/'
c=sqlite3.connect(':memory:',isolation_level=None)
c.executescript(open(base+'fixtures/events.sql').read())
c.executescript(open(base+'notification_schema.sql').read())
now=1_800_000_000_000
ops=[]
def apply(op):
    name=op['op']
    try:
        if name=='event':
            e=event_store.append_event(c,op['event']); out=None
        elif name=='presence':out=nd.record_presence(c,op['device'],op['visible'],op['audio'],op['interaction'],op['now'],op['kind'])
        elif name=='clients':out=nd.clients(c,now)
        elif name=='read':out=nd.mark_read(c,op['ids'],op['now'])
        elif name=='save':out=nd.save_prefs(c,op['update'])
        elif name=='load':out=nd.load_prefs(c)
        elif name=='badge':out=nd.badge_count(c, (lambda s,p:p=='%live') if op.get('live') else None)
        elif name=='unread':out=nd.unread_notice_ids(c,op.get('project'))
        elif name=='revision':out=nd.revision(c)
        elif name=='list':out=nd.list_notices(c,op['after'],op['limit'],now,focus_active=op.get('focus',False),is_live=(lambda s,p:p=='%live') if op.get('live') else None)
        elif name=='claim':out=nd.claim_sound(c,op['id'],op['device'],now,op.get('focus'))
        elif name=='focus':out=nd.focus_block_active(c)
        elif name=='sql':c.executescript(op['sql']);out=None
        elif name=='begin':c.execute('BEGIN IMMEDIATE');out=None
        elif name=='rollback':c.execute('ROLLBACK');out=None
        else:raise AssertionError(name)
        result={'ok':out}
    except Exception as err:result={'error':str(err)}
    op['expected']=result
    op['inTransaction']=c.in_transaction
    ops.append(op)
def ev(id,kind,pane='%live',project='ComandOS'):
    apply({'op':'event','event':{'eventId':id,'source':'test','kind':kind,'evidence':'confirmed','correlation':'local','occurredAtMs':now,'receivedAtMs':now,'projectKey':project,'sessionKey':'s','paneId':pane}})
apply({'op':'load'})
for visible,interaction,at in [(False,False,0),(False,False,5),(True,True,10),(False,False,15),(False,False,20)]:
    apply({'op':'presence','device':'phone','visible':visible,'audio':True,'interaction':interaction,'now':now+at,'kind':'web-client-name-too-long'})
    apply({'op':'clients'})
apply({'op':'presence','device':'','visible':True,'audio':True,'interaction':True,'now':now,'kind':None})
apply({'op':'presence','device':'é'*200,'visible':True,'audio':False,'interaction':False,'now':now,'kind':None})
apply({'op':'presence','device':'é'*201,'visible':True,'audio':True,'interaction':True,'now':now,'kind':None})
apply({'op':'clients'})
ev('perm','permission_requested')
ev('dead','input_requested','%dead')
ev('done','turn_completed')
ev('news','news_edition',None,'ComandOS')
ev('perm2','permission_requested')
apply({'op':'read','ids':['perm2','perm2','missing',None,1,''],'now':now})
apply({'op':'list','after':0,'limit':500})
apply({'op':'list','after':0,'limit':2,'live':True,'focus':True})
apply({'op':'badge'})
apply({'op':'badge','live':True})
apply({'op':'unread','project':'ComandOS'})
apply({'op':'unread'})
apply({'op':'revision'})
apply({'op':'save','update':{'volume':0.2,'modes':{'done':'sound'},'muted':False}})
apply({'op':'save','update':{'modes':{'done':'loud'}}})
apply({'op':'sql','sql':"UPDATE notice_prefs SET value='invalid';"})
apply({'op':'load'})
apply({'op':'sql','sql':"UPDATE notice_prefs SET value='[]';"})
apply({'op':'load'})
apply({'op':'claim','id':'unknown','device':'local-speaker'})
apply({'op':'claim','id':'done','device':'local-speaker'})
apply({'op':'claim','id':'perm2','device':'local-speaker'})
apply({'op':'presence','device':'phone','visible':True,'audio':True,'interaction':True,'now':now,'kind':'web'})
apply({'op':'claim','id':'perm2','device':'local-speaker'})
apply({'op':'claim','id':'perm2','device':'phone'})
apply({'op':'claim','id':'perm2','device':'phone'})
ev('nested','permission_requested')
apply({'op':'begin'})
apply({'op':'claim','id':'nested','device':'phone'})
apply({'op':'rollback'})
apply({'op':'claim','id':'nested','device':'phone'})
apply({'op':'sql','sql':"INSERT INTO pomodoro_blocks VALUES('b','focus','running');UPDATE pomodoro_state SET block_id='b';"})
apply({'op':'focus'})
ev('err','turn_failed')
ev('focus-perm','permission_requested')
apply({'op':'claim','id':'err','device':'phone'})
apply({'op':'claim','id':'focus-perm','device':'phone'})
apply({'op':'claim','id':'err','device':'phone','focus':False})
for i in range(510):ev('old-'+str(i),'prompt_accepted' if i%7==0 else 'turn_completed',None)
apply({'op':'badge'})
apply({'op':'unread'})
apply({'op':'list','after':500,'limit':500})
apply({'op':'read','ids':nd.unread_notice_ids(c),'now':now+1})
apply({'op':'badge'})
apply({'op':'revision'})
apply({'op':'sql','sql':'DROP TABLE pomodoro_state;'})
apply({'op':'focus'})
target = Path(base+'notification_fixture.json')
if '--write' in sys.argv:
    target.write_text(json.dumps(ops, ensure_ascii=False))
else:
    assert json.loads(target.read_text()) == ops, 'Durable oracle fixture differs from Python'
    print(f'{len(ops)} notification storage operations match Python')
