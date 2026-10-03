"""Run with rust-python-oracle; every endpoint and home is synthetic."""
import json, os, pathlib, subprocess, sys, tempfile, threading, time
from http.server import ThreadingHTTPServer
sys.path.insert(0,str(pathlib.Path(__file__).parent))
from test_serve import Handler,catalog,send,receive


def rss(pid):
    for line in pathlib.Path(f'/proc/{pid}/status').read_text().splitlines():
        if line.startswith('VmRSS:'): return int(line.split()[1])
    raise AssertionError('missing RSS')


def run(label, command, endpoint, home):
    catalog(home,{'url':endpoint,'disabled_tools':['blocked']})
    process=subprocess.Popen([*command,'--home',str(home),'serve','demo'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    try:
        send(process,'initialize',{'protocolVersion':'2025-03-26','capabilities':{},'clientInfo':{'name':'measurement','version':'1'}});assert 'result' in receive(process)
        process.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n');process.stdin.flush()
        time.sleep(.3);idle=rss(process.pid)
        for i in range(100):
            send(process,'tools/call',{'name':'echo','arguments':{'value':i}},ident=i+10);assert receive(process)['result']['structuredContent']['value']==i
        repeated=rss(process.pid)
        send(process,'tools/list');receive(process)
        send(process,'tools/list',{'cursor':'page2'});receive(process)
        time.sleep(2)
        measured=rss(process.pid)
        print(json.dumps({'implementation':label,'idle_kib':idle,'after_100_calls_kib':repeated,'after_full_tools_list_and_2s_kib':measured}),flush=True)
    finally:
        process.stdin.close();process.wait(timeout=8)
        assert process.returncode==0,process.stderr.read()


server=ThreadingHTTPServer(('127.0.0.1',0),Handler);server.daemon_threads=True
threading.Thread(target=server.serve_forever,daemon=True).start()
try:
    with tempfile.TemporaryDirectory(prefix='mcp-rss-') as directory:
        base=pathlib.Path(directory);url=f'http://127.0.0.1:{server.server_port}/mcp'
        run('rust-release',['/work/.migration-build/target/release/comandos-extensions'],url,base/'rust')
        run('python-original',['/venv/bin/python','/work/bin/cc-extensions'],url,base/'python')
finally:server.shutdown();server.server_close()
