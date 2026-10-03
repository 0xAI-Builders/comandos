"""Synthetic executable-seam tests; run only through scripts/rust-sandbox."""
import json, os, pathlib, subprocess, sys, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import pytest

ROOT = pathlib.Path('/work')
BIN = ROOT / '.migration-build/target/debug/comandos-extensions'


def catalog(home, spec):
    path = home / '.config/comandos/extensions/catalog.json'
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps({'version': 1, 'servers': {'demo': spec}}))


def launch(home, spec, **kwargs):
    assert BIN.exists(), 'native serve executable has not been implemented'
    catalog(home, spec)
    return subprocess.Popen([str(BIN), '--home', str(home), 'serve', 'demo'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, **kwargs)


def send(p, method, params=None, ident=1):
    p.stdin.write(json.dumps({'jsonrpc':'2.0', 'id':ident, 'method':method, 'params':params or {}})+'\n'); p.stdin.flush()


def receive(p):
    import select
    assert select.select([p.stdout], [], [], 8)[0], 'proxy response timed out'
    line=p.stdout.readline()
    assert line, p.stderr.read()
    return json.loads(line)


def close(p):
    p.stdin.close(); p.wait(timeout=8)
    assert p.stderr.read() == ''


def test_direct_exec_preserves_pid_arguments_environment_and_exit(tmp_path):
    script=tmp_path/'fixture.py'
    script.write_text('import os,sys,json\nprint(json.dumps([os.getpid(),sys.argv[1:],os.getenv("EXPANDED"),os.getcwd()]),flush=True)\nsys.exit(17)\n')
    p=launch(tmp_path, {'command':sys.executable, 'args':[str(script),'$(echo leaked)','a b','$SOURCE'], 'env':{'EXPANDED':'${SOURCE}/$MISSING'},'cwd':str(tmp_path)},env={**os.environ,'SOURCE':'value'})
    pid,args,value,cwd=receive(p)
    assert pid==p.pid and args==['$(echo leaked)','a b','$SOURCE'] and value=='value/$MISSING' and cwd==str(tmp_path)
    assert p.wait(timeout=5)==17
    assert p.stderr.read()==''


@pytest.mark.parametrize('spec',[{'enabled':False,'url':'http://secret.invalid/token'},{'command':'/missing/private-token'}])
def test_failures_do_not_expose_configuration(tmp_path,spec):
    p=launch(tmp_path,spec); p.stdin.close(); p.wait(timeout=5)
    assert p.returncode != 0
    assert 'secret' not in p.stderr.read() and p.stdout.read()==''


def result_for(d):
    method=d.get('method'); params=d.get('params',{})
    if method=='initialize':
        result={'protocolVersion':'2025-03-26','instructions':'Keep original instructions','capabilities':{'tools':{'listChanged':True},'resources':{'subscribe':True},'prompts':{},'completions':{},'logging':{}},'serverInfo':{'name':'fixture','version':'1'}}
    elif method=='tools/list':
        if params.get('cursor')=='error-page':
            return {'jsonrpc':'2.0','id':d['id'],'error':{'code':-32099,'message':'Original list error','data':{'cursor':'error-page','extra':[1,2]}},'futureEnvelope':{'keep':True}}
        result={'tools':[{'name':'echo','description':'Original description','inputSchema':{'type':'object'},'futureField':3},{'name':'blocked','inputSchema':{'type':'object'}}], 'futureResult':42}
        if not params.get('cursor'): result['nextCursor']='page2'
    elif method=='tools/call':
        if params.get('arguments',{}).get('fail'): return {'jsonrpc':'2.0','id':d['id'],'error':{'code':-32099,'message':'Original error','data':{'extra':4}}}
        time.sleep(params.get('arguments',{}).get('delay',0))
        result={'content':[{'type':'text','text':'ok'}],'structuredContent':params.get('arguments',{}),'isError':False,'future':7}
    else: result={'echo':method,'params':params,'extra':[1,2]}
    return {'jsonrpc':'2.0','id':d['id'],'result':result}


class Handler(BaseHTTPRequestHandler):
    protocol_version='HTTP/1.1'
    def log_message(self,*args): pass
    def do_GET(self):
        if self.path!='/sse':
            self.send_response(405);self.send_header('Content-Length','0');self.end_headers();return
        self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
        self.wfile.write(b'event: endpoint\r\ndata: /messages?session=fixture\r\n\r\n');self.wfile.flush()
        while True:
            value=self.server.events.get()
            if value is None: return
            try: self.wfile.write(('event: message\ndata: '+json.dumps(value)+'\n\n').encode());self.wfile.flush()
            except (BrokenPipeError,ConnectionResetError): return
    def do_DELETE(self):
        self.send_response(200);self.send_header('Content-Length','0');self.end_headers()
    def do_POST(self):
        d=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        if 'id' not in d:
            self.send_response(202);self.send_header('Content-Length','0');self.end_headers();return
        value=result_for(d)
        if self.path.startswith('/messages'):
            self.server.events.put(value)
            self.send_response(202);self.send_header('Content-Length','0');self.end_headers();return
        body=json.dumps(value).encode()
        if self.path=='/stream': body=b'event: message\ndata: '+body+b'\n\n'
        self.send_response(200);self.send_header('Content-Type','text/event-stream' if self.path=='/stream' else 'application/json');self.send_header('Content-Length',str(len(body)));self.send_header('Mcp-Session-Id','fixture-session');self.end_headers();self.wfile.write(body)


@pytest.fixture(params=['stdio','http','stream','sse'])
def upstream(request):
    if request.param=='stdio':
        yield {'command':sys.executable,'args':[__file__,'--fixture']}
    else:
        import queue
        server=ThreadingHTTPServer(('127.0.0.1',0),Handler);server.daemon_threads=True;server.events=queue.Queue()
        thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        yield {'url':f'http://127.0.0.1:{server.server_port}/'+request.param,'transport':'sse' if request.param=='sse' else 'http'}
        server.events.put(None);server.shutdown();server.server_close()


def initialize(p):
    send(p,'initialize',{'protocolVersion':'2025-03-26','capabilities':{},'clientInfo':{'name':'test','version':'1'}})
    value=receive(p)['result']
    assert value['instructions']=='Keep original instructions'
    assert value['capabilities']=={'tools':{},'prompts':{},'resources':{},'completions':{}}
    p.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n');p.stdin.flush()


def test_proxy_protocol_filter_pagination_and_results(tmp_path,upstream):
    p=launch(tmp_path,{**upstream,'disabled_tools':['blocked']})
    try:
        initialize(p)
        send(p,'tools/list'); first=receive(p)['result']
        assert [t['name'] for t in first['tools']]==['echo'] and first['nextCursor']=='page2' and first['futureResult']==42
        assert first['tools'][0]['futureField']==3
        send(p,'tools/list',{'cursor':'page2'}); assert 'nextCursor' not in receive(p)['result']
        send(p,'tools/call',{'name':'blocked'});assert receive(p)['error']['code']==-32601
        send(p,'tools/call',{'name':'echo','arguments':{'value':7}});result=receive(p)['result']
        assert result['structuredContent']=={'value':7} and result['future']==7
        send(p,'tools/call',{'name':'echo','arguments':{'fail':True}});assert receive(p)['error']=={'code':-32099,'message':'Original error','data':{'extra':4}}
        for method in ['resources/list','resources/templates/list','resources/read','prompts/list','prompts/get','completion/complete']:
            send(p,method,{'cursor':'opaque'});assert receive(p)['result']['params']=={'cursor':'opaque'}
        send(p,'resources/subscribe');assert receive(p)['error']['code']==-32601
    finally: close(p)


def test_empty_enabled_tools_denies_everything(tmp_path,upstream):
    p=launch(tmp_path,{**upstream,'enabled_tools':[]})
    try:
        initialize(p); send(p,'tools/list');assert receive(p)['result']['tools']==[]
        send(p,'tools/call',{'name':'echo'});assert receive(p)['error']['code']==-32601
    finally: close(p)


def test_tools_list_preserves_entire_error_envelope(tmp_path,upstream):
    p=launch(tmp_path,{**upstream,'disabled_tools':['blocked']})
    try:
        initialize(p)
        send(p,'tools/list',{'cursor':'error-page'},ident='list-error')
        assert receive(p)=={'jsonrpc':'2.0','id':'list-error','error':{'code':-32099,'message':'Original list error','data':{'cursor':'error-page','extra':[1,2]}},'futureEnvelope':{'keep':True}}
    finally:close(p)


def test_concurrent_requests_keep_original_ids(tmp_path,upstream):
    p=launch(tmp_path,{**upstream,'disabled_tools':['blocked']})
    try:
        initialize(p)
        send(p,'tools/call',{'name':'echo','arguments':{'value':'slow','delay':.2}},ident='slow')
        send(p,'tools/call',{'name':'echo','arguments':{'value':'fast'}},ident=99)
        values=[receive(p),receive(p)]
        assert {v['id']:v['result']['structuredContent']['value'] for v in values}=={'slow':'slow',99:'fast'}
    finally:close(p)


if __name__=='__main__' and '--fixture' in sys.argv:
    lock=threading.Lock()
    def respond(d):
        value=result_for(d)
        with lock: print(json.dumps(value),flush=True)
    for line in sys.stdin:
        d=json.loads(line)
        if 'id' in d: threading.Thread(target=respond,args=(d,),daemon=True).start()


def test_filtered_child_stops_on_eof_and_signal(tmp_path):
    for signal in [False,True]:
        pidfile=tmp_path/f'pid-{signal}'
        script=tmp_path/'child.py'
        script.write_text('import os,sys,json,time\nopen(sys.argv[1],"w").write(str(os.getpid()))\nfor line in sys.stdin:\n d=json.loads(line)\n if d.get("method")=="initialize": print(json.dumps({"jsonrpc":"2.0","id":d["id"],"result":{"protocolVersion":"2025-03-26","capabilities":{},"serverInfo":{"name":"child","version":"1"}}}),flush=True)\n')
        p=launch(tmp_path,{'command':sys.executable,'args':[str(script),str(pidfile)],'enabled_tools':[]})
        send(p,'initialize');receive(p)
        child=int(pidfile.read_text())
        if signal:p.terminate()
        else:p.stdin.close()
        p.wait(timeout=5)
        with pytest.raises(ProcessLookupError): os.kill(child,0)


def test_upstream_crash_exits_without_downstream_eof(tmp_path):
    p=launch(tmp_path,{'command':sys.executable,'args':['-c','import sys;sys.exit(1)'],'enabled_tools':[]})
    assert p.wait(timeout=5)!=0
    assert p.stdout.read()==''


def test_oversized_downstream_frame_is_bounded_and_closes(tmp_path,upstream):
    p=launch(tmp_path,{**upstream,'enabled_tools':[]})
    try:
        initialize(p)
        try:p.stdin.write('x'*(8*1024*1024+1));p.stdin.flush()
        except BrokenPipeError:pass
        p.wait(timeout=5)
    finally:
        p.kill() if p.poll() is None else None


def test_cancellation_reaches_filtered_upstream(tmp_path):
    marker=tmp_path/'cancelled'
    script=tmp_path/'cancel.py'
    script.write_text('import sys,json,pathlib\nfor line in sys.stdin:\n d=json.loads(line)\n if d.get("method")=="initialize": print(json.dumps({"jsonrpc":"2.0","id":d["id"],"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"child","version":"1"}}}),flush=True)\n if d.get("method")=="notifications/cancelled": pathlib.Path(sys.argv[1]).write_text(str(d["params"]["requestId"]))\n')
    p=launch(tmp_path,{'command':sys.executable,'args':[str(script),str(marker)],'disabled_tools':['blocked']})
    try:
        send(p,'initialize');receive(p)
        send(p,'tools/call',{'name':'echo'},ident='cancel-me')
        time.sleep(.1)
        p.stdin.write('{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"cancel-me"}}\n');p.stdin.flush()
        for _ in range(100):
            if marker.exists():break
            time.sleep(.01)
        assert marker.exists(), 'cancellation must reach upstream with mapped id'
        assert marker.read_text().isdigit()
    finally:close(p)


def credentials(home, value):
    path=home/'.config/comandos/extensions/credentials.json';path.parent.mkdir(parents=True,exist_ok=True);path.write_text(json.dumps({'demo':value}));return path


@pytest.fixture
def custom_server():
    servers=[]
    def start(handler):
        server=ThreadingHTTPServer(('127.0.0.1',0),handler);server.daemon_threads=True
        import queue
        server.events=queue.Queue()
        threading.Thread(target=server.serve_forever,daemon=True).start();servers.append(server)
        return f'http://127.0.0.1:{server.server_port}'
    yield start
    for server in servers:server.events.put(None);server.shutdown();server.server_close()


def test_static_header_precedence_ignores_shared_auth(tmp_path,custom_server):
    seen=[]
    class Capture(Handler):
        def do_POST(self): seen.append(self.headers.get('Authorization'));super().do_POST()
    url=custom_server(Capture)+'/mcp'
    credentials(tmp_path,{'url':'https://different.invalid/secret','access_token':'never-send'})
    p=launch(tmp_path,{'url':url,'headers':{'authorization':'Bearer ${STATIC}'},'bearer_token_env_var':'BEARER','env_http_headers':{'Authorization':'FINAL'}},env={**os.environ,'STATIC':'one','BEARER':'two','FINAL':'Bearer three'})
    try: initialize(p)
    finally:close(p)
    assert seen and set(seen)=={'Bearer three'}


def test_shared_401_retries_once_only_with_new_token(tmp_path,custom_server):
    seen=[]; path=None;url=None
    class Rotate(Handler):
        def do_POST(self):
            seen.append(self.headers.get('Authorization'))
            if self.headers.get('Authorization')=='Bearer old':
                self.rfile.read(int(self.headers['Content-Length']))
                path.write_text(json.dumps({'demo':{'url':url,'access_token':'new'}}))
                self.send_response(401);self.send_header('Content-Length','0');self.end_headers();return
            super().do_POST()
    url=custom_server(Rotate)+'/mcp';path=credentials(tmp_path,{'url':url,'access_token':'old'})
    p=launch(tmp_path,{'url':url})
    try:initialize(p)
    finally:close(p)
    assert seen[:2]==['Bearer old','Bearer new'] and seen.count('Bearer old')==1


def test_redirect_never_forwards_credentials(tmp_path,custom_server):
    leaked=[]
    class Destination(Handler):
        def do_POST(self):leaked.append(self.headers.get('Authorization'));super().do_POST()
    target=custom_server(Destination)+'/mcp'
    class Redirect(Handler):
        def do_POST(self):
            self.rfile.read(int(self.headers['Content-Length']));self.send_response(307);self.send_header('Location',target);self.send_header('Content-Length','0');self.end_headers()
    url=custom_server(Redirect)+'/mcp'
    p=launch(tmp_path,{'url':url,'headers':{'Authorization':'Bearer private-test'}})
    p.wait(timeout=5)
    assert p.returncode!=0 and not leaked
    assert 'private-test' not in p.stderr.read() and p.stdout.read()==''


def test_legacy_sse_foreign_endpoint_rejected(tmp_path,custom_server):
    leaked=[]
    class Destination(Handler):
        def do_POST(self):leaked.append(1);super().do_POST()
    target=custom_server(Destination)+'/messages'
    class Endpoint(Handler):
        def do_GET(self):
            data=('event: endpoint\ndata: '+target+'\n\n').encode()
            self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
    url=custom_server(Endpoint)+'/sse';credentials(tmp_path,{'url':url,'access_token':'secret'})
    p=launch(tmp_path,{'url':url,'transport':'sse'});p.wait(timeout=5)
    assert p.returncode!=0 and not leaked
    assert 'secret' not in p.stderr.read()


@pytest.mark.parametrize('raw',[b'',b'{bad private-secret',b'{"version":1,"version":1,"servers":{}}',b'{"version":2}'])
def test_invalid_catalog_error_is_sanitized(tmp_path,raw):
    path=tmp_path/'catalog.json';path.write_bytes(raw)
    p=subprocess.run([str(BIN),'--home',str(tmp_path),'--catalog',str(path),'serve','demo'],capture_output=True,timeout=5)
    assert p.returncode!=0 and p.stdout==b'' and b'private-secret' not in p.stderr


def test_unsupported_commands_fail_explicitly(tmp_path):
    for name in ['import','sync','status','check']:
        p=subprocess.run([str(BIN),'--home',str(tmp_path),name],capture_output=True,timeout=5)
        assert p.returncode!=0 and not p.stdout


def test_request_limit_is_explicit_and_cancellation_frees_slot(tmp_path):
    marker=tmp_path/'saturated'
    script=tmp_path/'hold_calls.py'
    script.write_text("""import json, pathlib, runpy, sys
result_for=runpy.run_path(sys.argv[1])['result_for']
count=0
for line in sys.stdin:
    d=json.loads(line)
    if 'id' not in d: continue
    if d.get('method')=='tools/call':
        count+=1
        pathlib.Path(sys.argv[2]).write_text(str(count))
        if not d.get('params',{}).get('arguments',{}).get('replacement'): continue
    print(json.dumps(result_for(d)),flush=True)
""")
    p=launch(tmp_path,{'command':sys.executable,'args':[str(script),__file__,str(marker)],'disabled_tools':['blocked']})
    try:
        initialize(p)
        for i in range(65):send(p,'tools/call',{'name':'echo'},ident=i+10)
        value=receive(p)
        assert value['error']['code']==-32000 and value['id']==74
        deadline=time.monotonic()+5
        while time.monotonic()<deadline:
            if marker.exists() and marker.read_text()=='64':break
            time.sleep(.01)
        assert marker.read_text()=='64', 'all 64 calls must reach the upstream and remain unanswered'
        p.stdin.write('{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":10}}\n');p.stdin.flush()
        send(p,'tools/call',{'name':'echo','arguments':{'replacement':True}},ident=75)
        replacement=receive(p)
        assert replacement['id']==75 and replacement.get('result',{}).get('structuredContent')=={'replacement':True}
        assert marker.read_text()=='65', 'replacement must reach upstream while other calls stay unanswered'
    finally:close(p)


def test_oversized_upstream_body_returns_sanitized_error(tmp_path,custom_server):
    class Large(Handler):
        def do_POST(self):
            d=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            if 'id' not in d:self.send_response(202);self.send_header('Content-Length','0');self.end_headers();return
            value=result_for(d)
            if d['method']=='tools/list':value['result']['secret-body']='x'*(8*1024*1024)
            body=json.dumps(value).encode();self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(body)));self.end_headers()
            try:self.wfile.write(body)
            except (BrokenPipeError,ConnectionResetError):pass
    p=launch(tmp_path,{'url':custom_server(Large)+'/mcp'})
    try:
        initialize(p);send(p,'tools/list');value=receive(p)
        assert value=={'jsonrpc':'2.0','id':1,'error':{'code':-32603,'message':'Upstream request failed'}}
    finally:close(p)


@pytest.mark.parametrize("transport",["stream","sse"])
def test_stream_response_answers_upstream_ping_without_claiming_sampling(tmp_path,custom_server,transport):
    replies=[]
    class Ping(Handler):
        def do_POST(self):
            d=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            if not d.get('method'):
                replies.append(d);self.send_response(202);self.send_header('Content-Length','0');self.end_headers();return
            if 'id' not in d:self.send_response(202);self.send_header('Content-Length','0');self.end_headers();return
            values=[]
            if d['method']=='tools/list':
                values=[{'jsonrpc':'2.0','id':'up-ping','method':'ping'}, {'jsonrpc':'2.0','id':'up-sampling','method':'sampling/createMessage','params':{}}]
            values.append(result_for(d))
            if transport=='sse':
                for value in values:self.server.events.put(value)
                self.send_response(202);self.send_header('Content-Length','0');self.end_headers();return
            body=''.join('event: message\ndata: '+json.dumps(v)+'\n\n' for v in values).encode()
            self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
    p=launch(tmp_path,{'url':custom_server(Ping)+('/sse' if transport=='sse' else '/mcp'),'transport':'sse' if transport=='sse' else 'http'})
    try:
        initialize(p);send(p,'tools/list');assert receive(p)['result']['tools']
        assert {v['id']:v.get('result',v.get('error')) for v in replies}=={'up-ping':{},'up-sampling':{'code':-32601,'message':'Unsupported upstream request'}}
    finally:close(p)


def test_stream_sse_accepts_cr_line_endings_and_multiline_data(tmp_path,custom_server):
    class Carriage(Handler):
        def do_POST(self):
            d=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            if 'id' not in d:self.send_response(202);self.send_header('Content-Length','0');self.end_headers();return
            data=json.dumps(result_for(d),indent=2)
            body=('\ufeff: comment\r'+''.join('data: '+line+'\r' for line in data.splitlines())+'\r').encode()
            self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
    p=launch(tmp_path,{'url':custom_server(Carriage)+'/mcp'})
    try:initialize(p)
    finally:close(p)
