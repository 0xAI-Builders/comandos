"""Native metadata/process contract; synthetic homes and public encoding only."""
import fcntl, hashlib, json, os, pathlib, shutil, subprocess, threading, time
from test_serve import launch, send, receive, close, initialize, upstream
BIN=pathlib.Path('/work/.migration-build/target/debug/comandos-extensions')
ENCODING='9b5ad71b2ce5302211f9c61530b329a4922fc6a4'
PUBLIC=pathlib.Path('/work/.migration-build/public-token-cache')/ENCODING

def warm(home):
    target=home/'.cache/comandos/tiktoken'/ENCODING;target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(PUBLIC,target)
def size_path(home,name='demo'):
    return home/'.local/state/comandos/extensions/sizes'/(hashlib.sha256(json.dumps(name,sort_keys=True,separators=(',',':')).encode()).hexdigest()+'.json')
def wait_size(home):
    path=size_path(home);deadline=time.monotonic()+8
    while not path.exists() and time.monotonic()<deadline:time.sleep(.02)
    assert path.exists(),'missing complete metadata'
    return json.loads(path.read_text())
def rss(pid):
    try:return int(next(l for l in pathlib.Path(f'/proc/{pid}/status').read_text().splitlines() if l.startswith('VmRSS:')).split()[1])
    except (FileNotFoundError,StopIteration):return 0

def test_only_complete_filtered_tool_list_records_size(tmp_path,upstream):
    warm(tmp_path);spec={**upstream,'disabled_tools':['blocked']};p=launch(tmp_path,spec)
    try:
        initialize(p);send(p,'tools/list',{'cursor':'page2'});receive(p);time.sleep(.08);assert not size_path(tmp_path).exists()
        send(p,'tools/list');first=receive(p)['result'];assert first['tools'][0]['futureField']==3
        time.sleep(.08);assert not size_path(tmp_path).exists()
        send(p,'tools/list',{'cursor':'wrong'});receive(p);time.sleep(.08);assert not size_path(tmp_path).exists()
        send(p,'tools/list');receive(p);send(p,'tools/list',{'cursor':'page2'});receive(p);stored=wait_size(tmp_path)
        defs=[{k:v for k,v in t.items() if k in ('name','description','inputSchema','outputSchema')} for t in first['tools']]*2
        text=json.dumps(defs,sort_keys=True,separators=(',',':'),ensure_ascii=False)
        count=json.loads(subprocess.check_output([str(BIN),'--home',str(tmp_path),'count'],input=json.dumps([text]).encode()))[0]
        assert stored['tokens']==count
        assert stored['content']==hashlib.sha256(json.dumps(defs,sort_keys=True,separators=(',',':')).encode()).hexdigest()
        assert stored['configuration']==hashlib.sha256(json.dumps(spec,sort_keys=True,separators=(',',':')).encode()).hexdigest()
        serialized=json.dumps(stored);assert 'inputSchema' not in serialized and 'blocked' not in serialized
        before=size_path(tmp_path).stat().st_mtime_ns
        send(p,'tools/list');receive(p);send(p,'tools/list',{'cursor':'page2'});receive(p);time.sleep(.25);assert size_path(tmp_path).stat().st_mtime_ns==before
    finally:close(p)

def test_shared_slots_block_before_allocating_tables_and_release_on_kill(tmp_path):
    warm(tmp_path);directory=tmp_path/'.local/state/comandos/extensions/tokenizer-slots';directory.mkdir(parents=True)
    locks=[open(directory/f'{i}.lock','w') for i in range(2)]
    for lock in locks:fcntl.flock(lock,fcntl.LOCK_EX)
    children=[]
    try:
        for _ in range(4):
            p=subprocess.Popen([str(BIN),'--home',str(tmp_path),'count'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True);p.stdin.write('["hello world"]');p.stdin.close();children.append(p)
        time.sleep(.15);assert all(p.poll() is None for p in children)
        blocked_peak=max(rss(p.pid) for p in children);assert blocked_peak<20000
        children[0].kill();children[0].wait(timeout=2)
        fcntl.flock(locks[0],fcntl.LOCK_UN)
        for p in children[1:]:assert p.wait(timeout=8)==0 and json.loads(p.stdout.read())==[2]
    finally:
        for p in children:
            if p.poll() is None:p.kill();p.wait(timeout=2)
        for lock in locks:lock.close()

def test_many_count_processes_never_hold_more_than_two_slots(tmp_path):
    warm(tmp_path);children=[];outputs=[]
    try:
        for _ in range(6):
            p=subprocess.Popen([str(BIN),'--home',str(tmp_path),'count'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True);children.append(p)
            thread=threading.Thread(target=lambda p=p:outputs.append(p.communicate(json.dumps(['hello world '*80000]),timeout=12)),daemon=True);thread.start()
        peak=0;resident=0;deadline=time.monotonic()+12
        while any(p.poll() is None for p in children) and time.monotonic()<deadline:
            active=sum(rss(p.pid)>24000 for p in children);peak=max(peak,active);resident=max(resident,sum(rss(p.pid) for p in children));assert active<=2
            time.sleep(.005)
        assert all(p.poll()==0 for p in children);assert peak==2
        assert len(outputs)==6 and all(json.loads(out)[0]>0 and not err for out,err in outputs)
        print({'workers':6,'max_tokenizers':peak,'aggregate_peak_kib':resident})
    finally:
        for p in children:
            if p.poll() is None:p.kill();p.wait(timeout=2)

def test_multiple_proxies_measure_once_per_configuration_rotation(tmp_path):
    import sys
    from test_serve import __file__ as upstream_script
    warm(tmp_path)
    slots=tmp_path/'.local/state/comandos/extensions/tokenizer-slots';slots.mkdir(parents=True)
    locks=[open(slots/f'{i}.lock','w') for i in range(2)]
    try:
        previous=None
        for spec in [dict(command=sys.executable,args=[upstream_script,'--fixture'],disabled_tools=['blocked']),dict(command=sys.executable,args=[upstream_script,'--fixture'],enabled_tools=['echo'])]:
            for lock in locks:fcntl.flock(lock,fcntl.LOCK_EX)
            proxies=[launch(tmp_path,spec) for _ in range(3)]
            try:
                for p in proxies:initialize(p);send(p,'tools/list');receive(p);send(p,'tools/list',{'cursor':'page2'});receive(p)
                time.sleep(.15)
                # Each proxy has one upstream child; only one may have a counter.
                def count_children(p):
                    childpids=pathlib.Path(f'/proc/{p.pid}/task/{p.pid}/children').read_text().split()
                    return [pid for pid in childpids if b'\x00count\x00' in pathlib.Path(f'/proc/{pid}/cmdline').read_bytes()]
                assert sum(len(count_children(p)) for p in proxies)==1
                for lock in locks:fcntl.flock(lock,fcntl.LOCK_UN)
                expected=hashlib.sha256(json.dumps(spec,sort_keys=True,separators=(',',':')).encode()).hexdigest();deadline=time.monotonic()+8
                while time.monotonic()<deadline:
                    if size_path(tmp_path).exists() and json.loads(size_path(tmp_path).read_text())['configuration']==expected:break
                    time.sleep(.02)
                stored=json.loads(size_path(tmp_path).read_text());assert stored['configuration']==expected
                assert stored['configuration']!=previous;previous=stored['configuration']
                time.sleep(.1);assert all(not count_children(p) for p in proxies)
            finally:
                for p in proxies:close(p)
    finally:
        for lock in locks:lock.close()

def test_slots_remain_owned_until_blocked_output_helper_exits(tmp_path):
    warm(tmp_path);children=[]
    try:
        for _ in range(3):
            p=subprocess.Popen([str(BIN),'--home',str(tmp_path),'count'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
            fcntl.fcntl(p.stdout,fcntl.F_SETPIPE_SZ,4096)
            p.stdin.write(json.dumps(['hello']*10000));p.stdin.close();children.append(p)
        time.sleep(.5)
        assert all(p.poll() is None for p in children)
        slots=tmp_path/'.local/state/comandos/extensions/tokenizer-slots'
        held=0
        for i in range(2):
            with open(slots/f'{i}.lock','r+') as lock:
                try:fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
                except BlockingIOError:held+=1
        assert held==2,'tokenizer allocations must remain covered while stdout is blocked'
    finally:
        for p in children:
            if p.poll() is None:p.kill()
            p.wait(timeout=2)
