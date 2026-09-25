import asyncio
import importlib
import json
from pathlib import Path
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import threading

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'lib'))


def test_http_proxy_preserves_tools_and_structured_results(tmp_path):
    proxy=importlib.import_module('extension_proxy')
    from extension_catalog import save_json,catalog_path
    class Handler(BaseHTTPRequestHandler):
        def log_message(self,*args):pass
        def do_POST(self):
            d=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            if 'id' not in d:
                self.send_response(202);self.end_headers();return
            results={
                'initialize':{'protocolVersion':'2025-03-26','capabilities':{'tools':{}},'serverInfo':{'name':'fixture','version':'1'}},
                'tools/list':{'tools':[{'name':'echo','description':'Original description','inputSchema':{'type':'object'}}]},
                'tools/call':{'content':[{'type':'text','text':'ok'}],'structuredContent':{'value':7},'isError':False}}
            body=json.dumps({'jsonrpc':'2.0','id':d['id'],'result':results[d['method']]}).encode()
            self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
        def do_GET(self):self.send_response(405);self.end_headers()
        def do_DELETE(self):self.send_response(200);self.end_headers()
    server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
    thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
    save_json(catalog_path(tmp_path),{'version':1,'servers':{'demo':{'url':f'http://127.0.0.1:{server.server_port}/mcp'}}})
    async def check():
        from mcp import ClientSession,StdioServerParameters
        from mcp.client.stdio import stdio_client
        params=StdioServerParameters(command=sys.executable,args=[str(ROOT/'bin/cc-extensions'),'--home',str(tmp_path),'serve','demo'])
        async with stdio_client(params) as (read,write):
            async with ClientSession(read,write) as session:
                await session.initialize()
                listed=await session.list_tools()
                assert listed.tools[0].description=='Original description'
                result=await session.call_tool('echo',{})
                assert result.structuredContent=={'value':7}
    try:asyncio.run(check())
    finally:server.shutdown();server.server_close()
