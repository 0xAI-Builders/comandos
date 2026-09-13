#!/usr/bin/env python3
"""Real remote MCP smoke test; creates only disposable data-URL pages."""
import argparse
import asyncio
import json
from pathlib import Path
import re

ROOT=Path(__file__).resolve().parents[1]

class Client:
    def __init__(self, process):self.process=process;self.ident=0
    @classmethod
    async def open(cls):
        p=await asyncio.create_subprocess_exec(str(ROOT/'bin/cc-browser-remote'),stdin=asyncio.subprocess.PIPE,stdout=asyncio.subprocess.PIPE,stderr=asyncio.subprocess.PIPE,limit=10*1024*1024)
        c=cls(p);r=await c.call('initialize',{'protocolVersion':'2025-11-25','capabilities':{},'clientInfo':{'name':'comandos-remote-smoke','version':'1'}})
        assert r['serverInfo']['name']=='comandos-browser-macmini';return c
    async def call(self,method,params=None):
        self.ident+=1
        self.process.stdin.write((json.dumps({'jsonrpc':'2.0','id':self.ident,'method':method,'params':params or {}})+'\n').encode());await self.process.stdin.drain()
        while True:
            line=await asyncio.wait_for(self.process.stdout.readline(),150)
            if not line:raise RuntimeError('SSH/MCP connection closed')
            message=json.loads(line)
            if message.get('id')==self.ident:
                if 'error' in message:raise RuntimeError(message['error'])
                return message['result']
    async def tool(self,name,args=None):return await self.call('tools/call',{'name':name,'arguments':args or {}})
    async def close(self):
        if self.process.returncode is None:
            self.process.stdin.close()
            try:await asyncio.wait_for(self.process.wait(),15)
            except asyncio.TimeoutError:self.process.kill();await self.process.wait()

def text(result):return '\n'.join(c.get('text','') for c in result.get('content',[]) if c.get('type')=='text')

async def main(catalog_only):
    clients=[]
    try:
        for _ in range(8 if catalog_only else 3):clients.append(await Client.open())
        catalogs=await asyncio.gather(*(c.call('tools/list') for c in clients));assert all(len(c['tools'])==29 for c in catalogs)
        if catalog_only:
            print('PASS: eight SSH MCP clients initialized and listed 29 tools each.');return
        markers=['COMANDOS_ISOLATION_ALPHA','COMANDOS_ISOLATION_BETA'];pages=[]
        for c,marker in zip(clients,markers):
            result=await c.tool('new_page',{'url':'data:text/html,<title>'+marker+'</title><h1>'+marker+'</h1>'})
            assert not result.get('isError'),text(result)
            matches=re.findall(r'^([0-9]+):.*'+marker,text(result),re.M);assert matches,text(result);pages.append(int(matches[-1]))
        for index,c in enumerate(clients[:2]):
            result=await c.tool('list_pages');assert markers[index] in text(result);assert markers[1-index] not in text(result)
            snapshot=await c.tool('take_snapshot',{'pageId':pages[index]});assert not snapshot.get('isError'),text(snapshot);assert markers[index] in text(snapshot)
        screenshot=await clients[0].tool('take_screenshot',{'pageId':pages[0],'format':'png'})
        assert not screenshot.get('isError'),text(screenshot);assert any(x.get('type')=='image' for x in screenshot['content'])
        busy=await clients[2].tool('new_page',{'url':'data:text/html,<title>THIRD</title>'});assert busy.get('isError') and 'busy' in text(busy)
        await clients[0].close();await asyncio.sleep(2)
        third=await clients[2].tool('new_page',{'url':'data:text/html,<title>THIRD</title>'});assert not third.get('isError'),text(third)
        print('PASS: remote navigation, snapshot, screenshot, isolated pages, two-worker capacity and release after disconnect.')
    finally:await asyncio.gather(*(c.close() for c in clients),return_exceptions=True)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--catalog-only',action='store_true');args=p.parse_args();asyncio.run(main(args.catalog_only))
