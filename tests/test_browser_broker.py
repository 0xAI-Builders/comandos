import asyncio
import importlib.util
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]

def load():
    spec = importlib.util.spec_from_file_location('browser_broker', ROOT/'services/browser/broker.py')
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module

FAKE = '''import json,sys,time,os
for line in sys.stdin:
 r=json.loads(line)
 if 'id' not in r: continue
 method=r.get('method'); p=r.get('params',{})
 if method=='initialize': result={'protocolVersion':'2025-11-25','capabilities':{'tools':{}},'serverInfo':{'name':'fake','version':'1'}}
 elif method=='tools/call':
  if p.get('name')=='slow':time.sleep(10)
  result={'content':[{'type':'text','text':str(os.getpid())}], 'isError':False}
 else:result={}
 print(json.dumps({'jsonrpc':'2.0','id':r['id'],'result':result}),flush=True)
'''

async def setup(tmp_path, **extra):
    m=load(); fake=tmp_path/'fake.py';fake.write_text(FAKE)
    config={'command':[sys.executable,str(fake)],'state_dir':str(tmp_path/'state'),
            'catalog':{'protocolVersion':'2025-11-25','tools':[{'name':'who','inputSchema':{'type':'object'}},{'name':'slow','inputSchema':{'type':'object'}}]},
            'max_workers':2,'idle_seconds':300,'tool_timeout':2,'stop_grace':.1,**extra}
    b=m.Broker(config);server=await asyncio.start_server(b.handle_client,'127.0.0.1',0)
    return b,server,server.sockets[0].getsockname()[1]

async def connect(port):
    r,w=await asyncio.open_connection('127.0.0.1',port)
    await rpc(r,w,1,'initialize',{'protocolVersion':'2025-11-25','clientInfo':{'name':'test','version':'1'},'capabilities':{}})
    return r,w

async def rpc(r,w,ident,method,params=None):
    w.write((json.dumps({'jsonrpc':'2.0','id':ident,'method':method,'params':params or {}})+'\n').encode());await w.drain()
    while True:
        response=json.loads(await asyncio.wait_for(r.readline(),3))
        if response.get('id')==ident:return response

async def until(predicate):
    for _ in range(100):
        if predicate():return
        await asyncio.sleep(.02)
    assert predicate()

async def finish(b,server,clients):
    for _,w in clients:w.close()
    await b.close();server.close();await server.wait_closed()


def test_catalog_connections_do_not_launch_workers(tmp_path):
    async def run():
        b,s,p=await setup(tmp_path);clients=[await connect(p) for _ in range(8)]
        for r,w in clients:assert len((await rpc(r,w,2,'tools/list'))['result']['tools'])==2
        assert b.worker_count==0
        await finish(b,s,clients)
    asyncio.run(run())


def test_two_sticky_workers_third_busy_and_disconnect_releases(tmp_path):
    async def run():
        b,s,p=await setup(tmp_path);clients=[await connect(p) for _ in range(3)]
        first=await rpc(*clients[0],2,'tools/call',{'name':'who'})
        second=await rpc(*clients[1],2,'tools/call',{'name':'who'})
        assert first['result']['content']!=second['result']['content']
        assert (await rpc(*clients[0],3,'tools/call',{'name':'who'}))['result']==first['result']
        assert (await rpc(*clients[2],2,'tools/call',{'name':'who'}))['result']['isError']
        assert b.worker_count==2
        clients[0][1].close();await until(lambda:b.worker_count==1)
        assert not (await rpc(*clients[2],3,'tools/call',{'name':'who'}))['result']['isError']
        await finish(b,s,clients)
    asyncio.run(run())


def test_timeout_reaps_worker_before_capacity_reuse(tmp_path):
    async def run():
        b,s,p=await setup(tmp_path,max_workers=1,tool_timeout=.1);c=await connect(p)
        response=await rpc(*c,2,'tools/call',{'name':'slow'})
        assert response['result']['isError'];assert b.worker_count==0
        await finish(b,s,[c])
    asyncio.run(run())


def test_idle_expiry_explicitly_reports_lost_pages(tmp_path):
    async def run():
        b,s,p=await setup(tmp_path,idle_seconds=.03);c=await connect(p)
        await rpc(*c,2,'tools/call',{'name':'who'});await asyncio.sleep(.06);await b.reap_idle()
        assert b.worker_count==0
        response=await rpc(*c,3,'tools/call',{'name':'who'})
        assert response['result']['isError'];assert 'expired' in response['result']['content'][0]['text']
        assert not (await rpc(*c,4,'tools/call',{'name':'who'}))['result']['isError']
        await finish(b,s,[c])
    asyncio.run(run())


def test_inflight_not_expired_and_disconnect_cleans_before_reuse(tmp_path):
    async def run():
        b,s,p=await setup(tmp_path,max_workers=1,idle_seconds=.02,tool_timeout=1);c=await connect(p)
        c[1].write(b'{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"slow"}}\n');await c[1].drain();await asyncio.sleep(.1)
        await b.reap_idle();assert b.worker_count==1
        c[1].close();await until(lambda:b.worker_count==0)
        await finish(b,s,[])
    asyncio.run(run())


def test_invalid_method_and_unknown_tool_do_not_launch_workers(tmp_path):
    async def run():
        b,s,p=await setup(tmp_path);c=await connect(p)
        assert (await rpc(*c,2,'bogus'))['error']['code']==-32601
        assert (await rpc(*c,3,'tools/call',{'name':'unknown'}))['result']['isError']
        assert b.worker_count==0
        await finish(b,s,[c])
    asyncio.run(run())


def test_stalled_client_send_is_bounded_and_connection_closed():
    async def run():
        m=load()
        class StalledWriter:
            def __init__(self):self.closed=False
            def write(self,data):pass
            async def drain(self):await asyncio.Event().wait()
            def close(self):self.closed=True
        w=StalledWriter();session=m.Session(w);session.send_timeout=.02
        await asyncio.wait_for(session.send({'jsonrpc':'2.0','id':1,'result':{}}),.2)
        assert session.closed and w.closed
    asyncio.run(run())


def test_capacity_queue_waits_for_cleanup_and_gets_released_slot(tmp_path):
    async def run():
        b,s,p=await setup(tmp_path,max_workers=1,queue_timeout=2);c1=await connect(p);c2=await connect(p)
        await rpc(*c1,2,'tools/call',{'name':'who'})
        waiting=asyncio.create_task(rpc(*c2,2,'tools/call',{'name':'who'}))
        await asyncio.sleep(.05);assert b.waiters==1 and not waiting.done()
        c1[1].close();assert not (await waiting)['result']['isError'];assert b.worker_count==1
        await finish(b,s,[c1,c2])
    asyncio.run(run())


def test_detached_child_is_gone_before_worker_capacity_released(tmp_path):
    async def run():
        b,s,p=await setup(tmp_path,max_workers=1)
        fake=tmp_path/'fake.py'
        fake.write_text(FAKE.replace('import json,sys,time,os','import json,sys,time,os,subprocess').replace("if p.get('name')=='slow':time.sleep(10)","if p.get('name')=='slow':time.sleep(10)\n  child=subprocess.Popen([sys.executable,'-c','import signal,time;signal.signal(signal.SIGTERM,signal.SIG_IGN);time.sleep(60)'],start_new_session=True)" ).replace("str(os.getpid())","str(child.pid)"))
        c=await connect(p);response=await rpc(*c,2,'tools/call',{'name':'who'});pid=int(response['result']['content'][0]['text'])
        assert Path(f'/proc/{pid}').exists();c[1].close();await until(lambda:b.worker_count==0)
        stat=Path(f'/proc/{pid}/stat')
        if stat.exists():
            raw=stat.read_text();assert raw[raw.rfind(')')+2:].split()[0]=='Z'
        await finish(b,s,[])
    asyncio.run(run())


def test_notification_cancellation_does_not_release_running_tool(tmp_path):
    async def run():
        b,s,p=await setup(tmp_path,max_workers=1,tool_timeout=.5);c=await connect(p)
        c[1].write(b'{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"slow"}}\n');await c[1].drain();await until(lambda:b.worker_count==1)
        c[1].write(b'{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2}}\n');await c[1].drain();await asyncio.sleep(.1)
        assert b.worker_count==1
        response=json.loads(await asyncio.wait_for(c[0].readline(),3));assert response['result']['isError'];assert b.worker_count==0
        await finish(b,s,[c])
    asyncio.run(run())
