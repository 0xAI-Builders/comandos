"""Stdio facade for shared HTTP MCP transports, without credential-bearing logs."""
from __future__ import annotations

import contextlib
from datetime import timedelta
import logging
import os
from pathlib import Path
import sys

import httpx
from mcp import ClientSession, StdioServerParameters, types
from mcp.client.stdio import stdio_client
from mcp.client.streamable_http import streamable_http_client
from mcp.client.sse import sse_client
from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp.server.models import InitializationOptions
from mcp.shared.exceptions import McpError

from extension_auth import SharedAuth, load_credentials
from extension_catalog import CatalogError


def resolved_env(spec):
    env=dict(os.environ)
    env.update({k:os.path.expandvars(str(v)) for k,v in spec.get('env',{}).items()})
    return env


@contextlib.asynccontextmanager
async def upstream(home,name,spec):
    logging.disable(logging.CRITICAL)
    async with contextlib.AsyncExitStack() as stack:
        if spec.get('command'):
            params=StdioServerParameters(command=os.path.expanduser(spec['command']),args=spec.get('args',[]),env=resolved_env(spec),cwd=spec.get('cwd'))
            # Child stderr can contain tokens or auth URLs. Suppress it from
            # model-visible diagnostics; status reports only exception classes.
            err=stack.enter_context(open(os.devnull,'w'))
            streams=await stack.enter_async_context(stdio_client(params,errlog=err))
        else:
            headers={k:os.path.expandvars(str(v)) for k,v in spec.get('headers',{}).items()}
            if spec.get('bearer_token_env_var'):
                value=os.environ.get(spec['bearer_token_env_var'])
                if value:headers['Authorization']='Bearer '+value
            for key,var in spec.get('env_http_headers',{}).items():
                if var in os.environ:headers[key]=os.environ[var]
            auth=SharedAuth(home,name,spec['url']) if name in load_credentials(home) and not any(k.lower()=='authorization' for k in headers) else None
            if spec.get('transport')=='sse':
                streams=await stack.enter_async_context(sse_client(spec['url'],headers=headers,auth=auth,timeout=20))
            else:
                client=await stack.enter_async_context(httpx.AsyncClient(headers=headers,auth=auth,timeout=httpx.Timeout(90,connect=20),follow_redirects=False))
                streams=await stack.enter_async_context(streamable_http_client(spec['url'],http_client=client))
        session=await stack.enter_async_context(ClientSession(streams[0],streams[1],read_timeout_seconds=timedelta(seconds=60)))
        initialized=await session.initialize()
        yield session,initialized


def make_server(name,spec,session,initialized):
    server=Server('comandos-'+name,version='1',instructions=initialized.instructions)
    pairs=[(types.ListToolsRequest,types.ListToolsResult),(types.CallToolRequest,types.CallToolResult),
           (types.ListResourcesRequest,types.ListResourcesResult),(types.ListResourceTemplatesRequest,types.ListResourceTemplatesResult),
           (types.ReadResourceRequest,types.ReadResourceResult),(types.ListPromptsRequest,types.ListPromptsResult),
           (types.GetPromptRequest,types.GetPromptResult),(types.CompleteRequest,types.CompleteResult)]
    allowed=spec.get('enabled_tools');denied=set(spec.get('disabled_tools',[]))
    def permitted(tool):return tool not in denied and (allowed is None or tool in allowed)
    for request_type,result_type in pairs:
        def handler_for(result_type):
            async def forward(req):
                if isinstance(req,types.CallToolRequest) and not permitted(req.params.name):
                    raise McpError(types.ErrorData(code=-32601,message='Tool disabled in shared catalog'))
                try:
                    payload=req.model_dump(by_alias=True,exclude_none=True)
                    payload.pop('jsonrpc',None)
                    payload.pop('id',None)
                    result=await session.send_request(types.ClientRequest.model_validate(payload),result_type)
                except McpError:
                    raise
                except Exception as exc:
                    raise McpError(types.ErrorData(code=-32603,message='Upstream request failed for '+name+': '+type(exc).__name__)) from None
                if isinstance(result,types.ListToolsResult):
                    result.tools=[t for t in result.tools if permitted(t.name)]
                return types.ServerResult(result)
            return forward
        server.request_handlers[request_type]=handler_for(result_type)
    return server


async def serve_http(home,name,spec):
    async with upstream(home,name,spec) as (session,initialized):
        server=make_server(name,spec,session,initialized)
        # Do not claim unsupported server-initiated sampling or subscriptions.
        caps=types.ServerCapabilities(
            tools=types.ToolsCapability() if initialized.capabilities.tools else None,
            prompts=types.PromptsCapability() if initialized.capabilities.prompts else None,
            resources=types.ResourcesCapability() if initialized.capabilities.resources else None,
            completions=initialized.capabilities.completions)
        async with stdio_server() as (read,write):
            await server.run(read,write,InitializationOptions(server_name=server.name,server_version='1',capabilities=caps,instructions=initialized.instructions))


def serve(home,name,catalog):
    import asyncio
    spec=catalog['servers'].get(name)
    if not spec or not spec.get('enabled',True):
        raise CatalogError('Server unavailable: '+name)
    if spec.get('command') and not spec.get('disabled_tools') and 'enabled_tools' not in spec:
        if spec.get('cwd'):os.chdir(os.path.expanduser(spec['cwd']))
        command=os.path.expanduser(spec['command'])
        with open(os.devnull,'w') as err:
            os.dup2(err.fileno(),2)
        os.execvpe(command,[command,*spec.get('args',[])],resolved_env(spec))
    asyncio.run(serve_http(home,name,spec))


async def check(home,name,spec):
    import asyncio
    # A check must never launch an interactive login flow or a browser runtime.
    browser={'chrome-bg','claude-in-chrome','playwright','x-playwright','lightpanda','obscura','screenwright','teams'}
    if not spec.get('enabled',True):
        return {'name':name,'status':'disabled'}
    if name in browser:
        return {'name':name,'status':'not_probed','reason':'interactive or browser runtime'}
    try:
        async with asyncio.timeout(40):
            async with upstream(home,name,spec) as (session,initialized):
                count=0
                if initialized.capabilities.tools:
                    cursor=None
                    for _ in range(30):
                        result=await session.list_tools(cursor=cursor)
                        count+=len(result.tools);cursor=result.nextCursor
                        if not cursor:break
                # These Google MCPs publish tools without authentication but
                # reject actual access. A minimal read distinguishes the two.
                probes={'google-drive':'list_recent_files','google-calendar':'list_calendars'}
                if name in probes:
                    result=await session.call_tool(probes[name],{'pageSize':1})
                    if result.isError:
                        return {'name':name,'status':'failed','phase':'read_access','tools':count}
                return {'name':name,'status':'connected','tools':count}
    except BaseException as exc:
        if isinstance(exc,(KeyboardInterrupt,SystemExit)):raise
        leaves=[]
        def visit(e):
            if isinstance(e,BaseExceptionGroup):
                for child in e.exceptions:visit(child)
            else:leaves.append(e)
        visit(exc)
        codes=sorted({e.response.status_code for e in leaves if isinstance(e,httpx.HTTPStatusError)})
        return {'name':name,'status':'failed','errors':sorted({type(e).__name__ for e in leaves}),**({'http_status':codes} if codes else {})}
