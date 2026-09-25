import importlib
import json
from pathlib import Path
import sys
import concurrent.futures

import pytest

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'lib'))


def test_refresh_rotates_once_and_persists_for_other_clients(tmp_path):
    m=importlib.import_module('extension_auth')
    m.save_credentials(tmp_path,{'demo':{'url':'https://example.com/mcp','access_token':'old',
        'refresh_token':'refresh','expires_at':1,'client_id':'client','issuer':'https://example.com'}})
    calls=[]
    def refresh(credential):
        calls.append(1)
        return {'access_token':'new','refresh_token':'rotated','expires_in':3600}
    def read(_):return m.access_token(tmp_path,'demo','https://example.com/mcp',refresh=refresh)
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        assert list(pool.map(read,range(8)))==['new']*8
    assert len(calls)==1
    assert m.load_credentials(tmp_path)['demo']['refresh_token']=='rotated'


def test_credentials_do_not_cross_endpoints(tmp_path):
    m=importlib.import_module('extension_auth')
    m.save_credentials(tmp_path,{'demo':{'url':'https://example.com/one','access_token':'private'}})
    with pytest.raises(m.AuthError):m.access_token(tmp_path,'demo','https://example.com/two')


def test_failed_refresh_does_not_destroy_token_or_expose_response(tmp_path):
    m=importlib.import_module('extension_auth')
    original={'demo':{'url':'https://example.com/mcp','access_token':'old','refresh_token':'private','expires_at':1}}
    m.save_credentials(tmp_path,original)
    def fail(c):raise RuntimeError('private-response')
    with pytest.raises(m.AuthError,match='Refresh failed') as e:
        m.access_token(tmp_path,'demo','https://example.com/mcp',refresh=fail)
    assert 'private' not in str(e.value)
    assert m.load_credentials(tmp_path)==original


def test_imports_only_mcp_credentials_not_model_login(tmp_path):
    m=importlib.import_module('extension_auth')
    p=tmp_path/'.claude/.credentials.json';p.parent.mkdir()
    p.write_text(json.dumps({'claudeAiOauth':{'accessToken':'model-private'},'mcpOAuth':{
        'demo|hash':{'serverName':'demo','serverUrl':'https://example.com/mcp','accessToken':'mcp-private','expiresAt':9999999999999}}}))
    c={'servers':{'demo':{'url':'https://example.com/mcp'}}}
    m.import_credentials(tmp_path,c)
    assert m.access_token(tmp_path,'demo','https://example.com/mcp')=='mcp-private'
    assert 'model-private' not in m.credentials_path(tmp_path).read_text()


def test_refresh_updates_original_mcp_record_without_touching_model_login(tmp_path):
    m=importlib.import_module('extension_auth')
    p=tmp_path/'.claude/.credentials.json';p.parent.mkdir()
    p.write_text(json.dumps({'claudeAiOauth':{'accessToken':'model-private'},'mcpOAuth':{'demo|hash':{
        'serverName':'demo','serverUrl':'https://example.com/mcp','accessToken':'old','refreshToken':'refresh','expiresAt':1}}}))
    m.import_credentials(tmp_path,{'servers':{'demo':{'url':'https://example.com/mcp'}}})
    m.access_token(tmp_path,'demo','https://example.com/mcp',refresh=lambda c:{'access_token':'new','refresh_token':'next','expires_in':3600})
    d=json.loads(p.read_text())
    assert d['mcpOAuth']['demo|hash']['accessToken']=='new'
    assert d['mcpOAuth']['demo|hash']['refreshToken']=='next'
    assert d['claudeAiOauth']['accessToken']=='model-private'


def test_native_refresh_after_import_is_reused(tmp_path):
    m=importlib.import_module('extension_auth')
    p=tmp_path/'.claude/.credentials.json';p.parent.mkdir()
    d={'mcpOAuth':{'demo|hash':{'serverName':'demo','serverUrl':'https://example.com/mcp','accessToken':'old','refreshToken':'refresh','expiresAt':1}}}
    p.write_text(json.dumps(d));m.import_credentials(tmp_path,{'servers':{'demo':{'url':'https://example.com/mcp'}}})
    d['mcpOAuth']['demo|hash'].update(accessToken='native-new',refreshToken='native-refresh',expiresAt=9999999999999)
    p.write_text(json.dumps(d))
    def fail(c):raise RuntimeError('must reuse native token')
    assert m.access_token(tmp_path,'demo','https://example.com/mcp',refresh=fail)=='native-new'


@pytest.mark.parametrize('received',['2099-01-01T00:00:00Z',{'secs_since_epoch':4070908800}])
def test_grok_native_refresh_timestamp_formats(tmp_path,received):
    m=importlib.import_module('extension_auth')
    p=tmp_path/'.grok/mcp_credentials.json';p.parent.mkdir()
    item={'url':'https://example.com/mcp','access_token':'old','refresh_token':'old-refresh','expires_at':1,'source':str(p)}
    m.save_credentials(tmp_path,{'demo':item})
    p.write_text(json.dumps({'demo:https://example.com/mcp':{'token_received_at':received,'token_response':{'access_token':'new','refresh_token':'new-refresh','expires_in':3600}}}))
    assert m.access_token(tmp_path,'demo','https://example.com/mcp')=='new'


def test_sse_same_origin_message_endpoint_and_foreign_origin(tmp_path):
    import asyncio,httpx
    m=importlib.import_module('extension_auth')
    m.save_credentials(tmp_path,{'demo':{'url':'https://example.com/sse','access_token':'private'}})
    async def run():
        auth=m.SharedAuth(tmp_path,'demo','https://example.com/sse')
        gen=auth.async_auth_flow(httpx.Request('POST','https://example.com/messages?session=fixture'))
        request=await anext(gen)
        assert request.headers['Authorization']=='Bearer private'
        await gen.aclose()
        gen=auth.async_auth_flow(httpx.Request('POST','https://other.example/messages'))
        with pytest.raises(m.AuthError):await anext(gen)
    asyncio.run(run())
