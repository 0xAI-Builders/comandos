"""Public-encoding metadata memory experiment in rust-python-oracle sandbox."""
import fcntl,hashlib,json,pathlib,subprocess,sys,tempfile,threading,time
from http.server import ThreadingHTTPServer
sys.path.insert(0,str(pathlib.Path(__file__).parent))
from test_serve import Handler,send,receive
import extension_metadata as oracle
BIN='/work/.migration-build/target/release/comandos-extensions'
def rss(pid):
    try:return int(next(l for l in pathlib.Path(f'/proc/{pid}/status').read_text().splitlines() if l.startswith('VmRSS:')).split()[1])
    except (FileNotFoundError,StopIteration):return 0
def children(pid):
    try:return [int(p) for p in pathlib.Path(f'/proc/{pid}/task/{pid}/children').read_text().split()]
    except FileNotFoundError:return []
def start(home,name):return subprocess.Popen([BIN,'--home',str(home),'serve',name],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
def size(home,name):return home/'.local/state/comandos/extensions/sizes'/(oracle._digest(name)+'.json')
def complete(p):send(p,'tools/list');receive(p);send(p,'tools/list',{'cursor':'page2'});receive(p)
def stop(p):p.stdin.close();p.wait(timeout=8);assert p.returncode==0 and not p.stderr.read()
server=ThreadingHTTPServer(('127.0.0.1',0),Handler);server.daemon_threads=True;threading.Thread(target=server.serve_forever,daemon=True).start()
spec={'url':f'http://127.0.0.1:{server.server_port}/mcp','disabled_tools':['blocked']}
defs=[{'name':'echo','description':'Original description','inputSchema':{'type':'object'}}]*2
expected=oracle._offline_counts([json.dumps(defs,sort_keys=True,separators=(',',':'),ensure_ascii=False)])[0]
try:
 with tempfile.TemporaryDirectory(prefix='metadata-rss-') as directory:
    home=pathlib.Path(directory);catalog=home/'.config/comandos/extensions/catalog.json';catalog.parent.mkdir(parents=True);catalog.write_text(json.dumps({'version':1,'servers':{f'demo-{i}':spec for i in range(6)}}))
    p=start(home,'demo-0')
    try:
        send(p,'initialize');receive(p);idle=rss(p.pid);complete(p);helper_peak=0;deadline=time.monotonic()+8
        while not size(home,'demo-0').exists() and time.monotonic()<deadline:
            helper_peak=max([helper_peak]+[rss(c) for c in children(p.pid)]);time.sleep(.002)
        assert json.loads(size(home,'demo-0').read_text())['tokens']==expected
        time.sleep(.05);after=rss(p.pid)
        for i in range(100):complete(p)
        time.sleep(.1);repeated=rss(p.pid);assert not children(p.pid)
        print(json.dumps({'scenario':'single-proxy','idle_kib':idle,'after_full_list_kib':after,'after_100_complete_lists_kib':repeated,'helper_peak_kib':helper_peak,'tokens':expected}),flush=True)
    finally:stop(p)
    for path in (home/'.local/state/comandos/extensions/sizes').glob('*.json'):path.unlink()
    slots=home/'.local/state/comandos/extensions/tokenizer-slots';slots.mkdir(parents=True,exist_ok=True);locks=[open(slots/f'{i}.lock','w') for i in range(2)]
    for lock in locks:fcntl.flock(lock,fcntl.LOCK_EX)
    proxies=[start(home,f'demo-{i}') for i in range(6)]
    try:
        for p in proxies:send(p,'initialize');receive(p);complete(p)
        time.sleep(.1);blocked=[c for p in proxies for c in children(p.pid)];assert len(blocked)==6
        blocked_peak=max(rss(c) for c in blocked);assert blocked_peak<20000
        for lock in locks:fcntl.flock(lock,fcntl.LOCK_UN)
        helper_peak=0;aggregate_peak=0;max_loaded=0;parent_peak=0;deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            counts=[rss(c) for p in proxies for c in children(p.pid)];helper_peak=max([helper_peak]+counts);aggregate_peak=max(aggregate_peak,sum(counts));max_loaded=max(max_loaded,sum(n>24000 for n in counts));parent_peak=max(parent_peak,sum(rss(p.pid) for p in proxies));assert sum(n>24000 for n in counts)<=2
            if all(size(home,f'demo-{i}').exists() for i in range(6)):break
            time.sleep(.002)
        assert all(json.loads(size(home,f'demo-{i}').read_text())['tokens']==expected for i in range(6));assert max_loaded==2
        print(json.dumps({'scenario':'six-distinct-servers-same-home','blocked_helpers':len(blocked),'blocked_helper_peak_kib':blocked_peak,'helper_peak_kib':helper_peak,'helpers_aggregate_peak_kib':aggregate_peak,'parents_aggregate_peak_kib':parent_peak,'max_loaded_tokenizers':max_loaded,'tokens_each':expected}),flush=True)
    finally:
        for lock in locks:lock.close()
        for p in proxies:stop(p)
finally:server.shutdown();server.server_close()
