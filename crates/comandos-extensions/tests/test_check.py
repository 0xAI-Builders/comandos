"""Synthetic check fixtures. Run only in the isolated Rust sandbox."""
import json, os, pathlib, subprocess, sys, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import pytest

ROOT=pathlib.Path('/work')
BIN=ROOT/'.migration-build/target/debug/comandos-extensions'
MAIL={'gmail':'jesusbatallar@gmail.com','gmail-signara':'jesus@signara.ai','qcdr-mail':'jesus@qcdr.io','proton-mail':'pdlgmcn@protonmail.com'}

def spec(mode='normal', **extra):
    return {'command':sys.executable,'args':[__file__,'--fixture',mode], **extra}

def run(home, servers, names=(), version=1):
    path=home/'.config/comandos/extensions/catalog.json';path.parent.mkdir(parents=True,exist_ok=True)
    path.write_text(json.dumps({'version':version,'servers':servers}))
    p=subprocess.run([str(BIN),'--home',str(home),'check',*names],capture_output=True,text=True,timeout=10)
    lines=[json.loads(x) for x in p.stdout.splitlines()]
    saved=home/'.local/state/comandos/extensions/last-check.json'
    return p, lines, json.loads(saved.read_text()) if saved.exists() else None

def test_disabled_browser_and_validation_never_spawn(tmp_path):
    marker=tmp_path/'spawned'
    unsafe=spec('normal',env={'MARKER':str(marker)})
    servers={'z':{**unsafe,'enabled':False},**{n:unsafe for n in ['chrome-bg','claude-in-chrome','playwright','x-playwright','lightpanda','obscura','screenwright','teams']}}
    p,rows,saved=run(tmp_path,servers)
    assert p.returncode==0, p.stderr
    assert [r['name'] for r in saved]==sorted(servers)
    assert all(r==({'name':n,'status':'disabled'} if n=='z' else {'name':n,'status':'not_probed','reason':'interactive or browser runtime'}) for n,r in zip(sorted(servers),saved))
    assert not marker.exists() and rows==saved
    p,rows,_=run(tmp_path,{'spawn':unsafe},['spawn','missing'])
    assert p.returncode==1 and rows==[] and not marker.exists()

@pytest.mark.parametrize('mode,count',[('normal',4),('no-tools',0),('cap',60)])
def test_tool_counts_ignore_filters_and_cap_pagination(tmp_path,mode,count):
    p,rows,saved=run(tmp_path,{'demo':spec(mode,enabled_tools=[],disabled_tools=['tool'])})
    assert p.returncode==0, p.stderr
    assert rows==saved==[{'name':'demo','status':'connected','tools':count}]
    assert (tmp_path/'.local/state/comandos/extensions/last-check.json').stat().st_mode&0o777==0o600
    assert list((tmp_path/'.local/state/comandos/extensions').glob('.comandos-*'))==[]

@pytest.mark.parametrize('name',list(MAIL)+['google-drive','google-calendar'])
@pytest.mark.parametrize('denied',[False,True])
def test_minimal_google_reads_and_mailbox_identity(tmp_path,name,denied):
    log=tmp_path/'calls'
    p,rows,_=run(tmp_path,{name:spec('denied' if denied else 'normal',env={'CALLS':str(log),'EMAIL':MAIL.get(name,'')})})
    assert p.returncode==int(denied),p.stderr
    assert rows==[{'name':name,'status':'failed','phase':'read_access','tools':4} if denied else {'name':name,'status':'connected','tools':4}]
    call=json.loads(log.read_text())
    assert call=={'name':'get_profile' if name in MAIL else 'list-calendars' if name=='google-calendar' else 'list_recent_files','arguments':{'pageSize':1} if name=='google-drive' else {}}

@pytest.mark.parametrize('field',['emailAddress','email'])
def test_mailbox_wrong_and_correct_fields(tmp_path,field):
    for email,expected in [('other@example.test','failed'),(MAIL['gmail'],'connected')]:
        p,rows,_=run(tmp_path,{'gmail':spec('normal',env={'EMAIL':email,'EMAIL_FIELD':field})})
        assert rows[0]['status']==expected and rows[0]['tools']==4
        assert p.returncode==int(expected=='failed')
        if expected=='failed':assert rows[0]['phase']=='mailbox_identity'

@pytest.mark.parametrize('mode,error',[('rpc','McpError'),('invalid-init','ValidationError'),('bad-json','JSONDecodeError'),('no-text','StopIteration')])
def test_failures_are_safe_and_meaningful(tmp_path,mode,error):
    p,rows,_=run(tmp_path,{'gmail':spec(mode,env={'EMAIL':MAIL['gmail']})})
    assert p.returncode==1 and rows==[{'name':'gmail','status':'failed','errors':[error]}]
    assert 'SENTINEL' not in p.stdout+p.stderr

@pytest.mark.parametrize('status',[401,403,307,500])
def test_http_status_and_secret_body_suppression(tmp_path,status):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self,*args):pass
        def do_POST(self):
            self.rfile.read(int(self.headers['Content-Length']))
            body=b'SENTINEL_TOKEN private upstream exception 999'
            self.send_response(status);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
    server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    try:
        p,rows,_=run(tmp_path,{'demo':{'url':f'http://127.0.0.1:{server.server_port}/mcp','headers':{'Authorization':'Bearer SENTINEL_TOKEN'}}})
        assert p.returncode==1 and rows==[{'name':'demo','status':'failed','errors':['HTTPStatusError'],'http_status':[status]}]
        assert 'SENTINEL' not in p.stdout+p.stderr
    finally:server.shutdown();server.server_close()

def test_completion_stream_request_order_duplicates_and_four_concurrent(tmp_path):
    events=tmp_path/'events'
    servers={str(i):spec('normal',env={'DELAY':'.35' if i<4 else '.01','EVENTS':str(events),'LABEL':str(i)}) for i in range(6)}
    names=['0','1','2','3','4','5','0']
    p,rows,saved=run(tmp_path,servers,names)
    assert p.returncode==0,p.stderr
    assert [r['name'] for r in saved]==names and len(rows)==7
    events=[json.loads(x) for x in events.read_text().splitlines()]
    active=peak=0
    for event in events:
        active+=1 if event[0]=='start' else -1
        peak=max(peak,active)
    assert peak==4 and active==0
    p,rows,saved=run(tmp_path,{'slow':spec('normal',env={'DELAY':'.3'}),'fast':spec()},['slow','fast','fast'])
    assert [r['name'] for r in rows]==['fast','fast','slow']
    assert [r['name'] for r in saved]==['slow','fast','fast']

def test_catalog_version_rejected_without_spawning(tmp_path):
    marker=tmp_path/'spawned'
    p,rows,saved=run(tmp_path,{'demo':spec(env={'MARKER':str(marker)})},version=2)
    assert p.returncode==1 and rows==[] and saved is None and not marker.exists()

def fixture(mode):
    if os.getenv('MARKER'):pathlib.Path(os.environ['MARKER']).write_text(str(os.getpid()))
    def event(kind):
        if os.getenv('EVENTS'):
            with open(os.environ['EVENTS'],'a') as f:f.write(json.dumps([kind,os.getenv('LABEL')])+'\n')
    event('start')
    for line in sys.stdin:
        d=json.loads(line);method=d.get('method')
        if 'id' not in d:continue
        if mode=='rpc':print(json.dumps({'jsonrpc':'2.0','id':d['id'],'error':{'code':-32000,'message':'SENTINEL'}}),flush=True);continue
        if method=='initialize':
            time.sleep(float(os.getenv('DELAY','0')))
            result={} if mode=='invalid-init' else {'protocolVersion':'2025-03-26','capabilities':{} if mode=='no-tools' else {'tools':{}},'serverInfo':{'name':'fixture','version':'1'}}
            if mode=='unsupported-version':result['protocolVersion']='2099-01-01'
            if mode=='server-info':result['serverInfo']={}
            if mode=='tools-capability':result['capabilities']['tools']=False
        elif method=='tools/list':
            result={'tools':[{'name':'tool','inputSchema':{'type':'object'}}]*2}
            if mode=='cap' or not d.get('params',{}).get('cursor'):result['nextCursor']='next'
            else:event('end')
        elif method=='tools/call':
            if os.getenv('CALLS'):pathlib.Path(os.environ['CALLS']).write_text(json.dumps(d['params']))
            text='SENTINEL invalid' if mode=='bad-json' else json.dumps({os.getenv('EMAIL_FIELD','emailAddress'):os.getenv('EMAIL','')})
            text=os.getenv('PROFILE_JSON',text)
            result={'content':[] if mode=='no-text' else [{'type':'text','text':text}],'isError':mode=='denied'}
            if mode=='invalid-content':result['content']=[{'type':'text','text':[]} ]
        else:result={}
        print(json.dumps({'jsonrpc':'2.0','id':d['id'],'result':result}),flush=True)

if __name__=='__main__':fixture(sys.argv[2])

def test_many_immediate_results_use_backpressure(tmp_path):
    p,rows,saved=run(tmp_path,{str(i):{'enabled':False} for i in range(200)})
    assert p.returncode==0,p.stderr
    assert len(rows)==len(saved)==200

@pytest.mark.parametrize('mode',['server-info','tools-capability','invalid-content'])
def test_invalid_protocol_shapes_fail(tmp_path,mode):
    p,rows,_=run(tmp_path,{'google-drive':spec(mode)})
    assert p.returncode==1 and rows==[{'name':'google-drive','status':'failed','errors':['ValidationError']}]

@pytest.mark.parametrize('profile,status',[
    ('{"emailAddress":"other", "emailAddress":"jesusbatallar@gmail.com"}','connected'),
    ('{"email":"jesusbatallar@gmail.com", "extra":NaN}','connected'),
    ('{"emailAddress":null,"email":"jesusbatallar@gmail.com", "extra":Infinity}','connected'),
    ('{"emailAddress":NaN,"email":"jesusbatallar@gmail.com"}','failed'),
])
def test_profile_json_matches_python_duplicates_and_nonfinite(tmp_path,profile,status):
    p,rows,_=run(tmp_path,{'gmail':spec(env={'PROFILE_JSON':profile})})
    assert rows[0]['status']==status and rows[0]['tools']==4
    assert p.returncode==int(status=='failed')
    if status=='failed':assert rows[0]['phase']=='mailbox_identity'

@pytest.mark.parametrize('transport',['http','stream','sse'])
def test_native_http_session_and_sse_completion_cleanup(tmp_path,transport):
    import queue
    from test_serve import Handler
    deleted=[]
    class Cleanup(Handler):
        def do_DELETE(self):deleted.append(self.headers.get('Mcp-Session-Id'));super().do_DELETE()
    server=ThreadingHTTPServer(('127.0.0.1',0),Cleanup);server.daemon_threads=True;server.events=queue.Queue()
    threading.Thread(target=server.serve_forever,daemon=True).start()
    try:
        p,rows,_=run(tmp_path,{'demo':{'url':f'http://127.0.0.1:{server.server_port}/{transport}','transport':'sse' if transport=='sse' else 'http','enabled_tools':[]}})
        assert p.returncode==0,p.stderr
        assert rows==[{'name':'demo','status':'connected','tools':4}]
        if transport!='sse':assert deleted==['fixture-session']
    finally:server.events.put(None);server.shutdown();server.server_close()

def test_cli_signal_reaps_children(tmp_path):
    marker=tmp_path/'pid'
    path=tmp_path/'.config/comandos/extensions/catalog.json';path.parent.mkdir(parents=True)
    path.write_text(json.dumps({'version':1,'servers':{'demo':spec(env={'MARKER':str(marker),'DELAY':'60'})}}))
    p=subprocess.Popen([str(BIN),'--home',str(tmp_path),'check'],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    try:
        until=time.monotonic()+3
        while not marker.exists() and time.monotonic()<until:time.sleep(.01)
        assert marker.exists()
        child=int(marker.read_text());p.terminate();p.wait(timeout=3)
        assert p.returncode==1
        with pytest.raises(ProcessLookupError):os.kill(child,0)
        assert b'panicked' not in p.stderr.read()
    finally:
        if p.poll() is None:p.kill();p.wait()

def test_stdout_saturation_signal_exits_without_hanging_or_panicking(tmp_path):
    path=tmp_path/'.config/comandos/extensions/catalog.json';path.parent.mkdir(parents=True)
    path.write_text(json.dumps({'version':1,'servers':{str(i)+'x'*2048:{'enabled':False} for i in range(200)}}))
    p=subprocess.Popen([str(BIN),'--home',str(tmp_path),'check'],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    try:
        time.sleep(.2);assert p.poll() is None
        p.terminate();p.wait(timeout=3)
        assert p.returncode==1
        assert b'panicked' not in p.stderr.read()
    finally:
        if p.poll() is None:p.kill();p.wait()

@pytest.mark.parametrize('enabled',[None,False,0,'',[],{}])
def test_python_falsy_disabled_values_do_not_spawn(tmp_path,enabled):
    marker=tmp_path/'pid'
    p,rows,_=run(tmp_path,{'demo':spec(env={'MARKER':str(marker)},enabled=enabled)})
    assert p.returncode==0 and rows==[{'name':'demo','status':'disabled'}] and not marker.exists()

def test_normal_completion_reaps_child(tmp_path):
    marker=tmp_path/'pid'
    p,rows,_=run(tmp_path,{'demo':spec(env={'MARKER':str(marker)})})
    assert p.returncode==0
    with pytest.raises(ProcessLookupError):os.kill(int(marker.read_text()),0)

def test_check_auth_failure_and_static_header_precedence(tmp_path):
    from test_serve import Handler
    seen=[]
    class Capture(Handler):
        def do_POST(self):seen.append(self.headers.get('Authorization'));super().do_POST()
    server=ThreadingHTTPServer(('127.0.0.1',0),Capture);server.daemon_threads=True
    threading.Thread(target=server.serve_forever,daemon=True).start()
    try:
        url=f'http://127.0.0.1:{server.server_port}/mcp'
        path=tmp_path/'.config/comandos/extensions/credentials.json';path.parent.mkdir(parents=True)
        path.write_text(json.dumps({'demo':{'url':'https://foreign.invalid/SENTINEL','access_token':'SENTINEL'}}))
        p,rows,_=run(tmp_path,{'demo':{'url':url}})
        assert rows==[{'name':'demo','status':'failed','errors':['AuthError']}] and p.returncode==1
        assert 'SENTINEL' not in p.stdout+p.stderr and seen==[]
        p,rows,_=run(tmp_path,{'demo':{'url':url,'headers':{'authorization':'Bearer static'}}})
        assert p.returncode==0 and rows[0]['status']=='connected' and set(seen)=={'Bearer static'}
    finally:server.shutdown();server.server_close()

def test_cleanup_uses_latest_rotated_shared_credential(tmp_path):
    from test_serve import Handler
    seen=[];deleted=[]
    path=tmp_path/'.config/comandos/extensions/credentials.json';path.parent.mkdir(parents=True)
    class Rotate(Handler):
        def do_POST(self):
            seen.append(self.headers.get('Authorization'))
            if self.headers.get('Authorization')=='Bearer old':
                self.rfile.read(int(self.headers['Content-Length']))
                path.write_text(json.dumps({'demo':{'url':url,'access_token':'new'}}))
                self.send_response(401);self.send_header('Content-Length','0');self.end_headers();return
            super().do_POST()
        def do_DELETE(self):deleted.append(self.headers.get('Authorization'));super().do_DELETE()
    server=ThreadingHTTPServer(('127.0.0.1',0),Rotate);server.daemon_threads=True
    url=f'http://127.0.0.1:{server.server_port}/mcp'
    path.write_text(json.dumps({'demo':{'url':url,'access_token':'old'}}))
    threading.Thread(target=server.serve_forever,daemon=True).start()
    try:
        p,rows,_=run(tmp_path,{'demo':{'url':url}})
        assert p.returncode==0 and rows[0]['status']=='connected'
        assert seen[:2]==['Bearer old','Bearer new'] and seen.count('Bearer old')==1
        assert deleted==['Bearer new']
    finally:server.shutdown();server.server_close()
