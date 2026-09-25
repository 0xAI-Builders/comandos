"""Shared MCP credentials; token values never belong in status or diagnostics."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import time
from urllib.parse import urlsplit

import httpx

from extension_catalog import ALIASES, locked, read_config, save_json, parse_config, replace_config


class AuthError(Exception):
    pass


def credentials_path(home):
    return Path(home)/'.config/comandos/extensions/credentials.json'


def load_credentials(home):
    return read_config(credentials_path(home))


def save_credentials(home,data):
    save_json(credentials_path(home),data)


def received_timestamp(value):
    if isinstance(value,str):
        try:
            from datetime import datetime
            return datetime.fromisoformat(value.replace('Z','+00:00')).timestamp()
        except ValueError:return 0
    if isinstance(value,dict):return value.get('secs_since_epoch',0)
    return value if isinstance(value,(int,float)) else 0


def sync_source(home,name,item,previous_token=None):
    """Keep the imported store usable for already-open native sessions."""
    if not item.get('source'):
        return
    path=Path(item['source'])
    if not path.exists():
        return
    before=path.read_bytes();data=parse_config(path,before);changed=False
    for value in data.get('mcpOAuth',{}).values():
        if value.get('serverUrl')!=item['url'] or ALIASES.get(value.get('serverName'),value.get('serverName'))!=name:
            continue
        if previous_token is not None and value.get('accessToken')!=previous_token:
            continue
        if previous_token is None and (value.get('expiresAt') or 0)/1000>=item.get('expires_at',0):
            continue
        value.update(accessToken=item['access_token'],expiresAt=int(item.get('expires_at',0)*1000))
        if item.get('refresh_token'):value['refreshToken']=item['refresh_token']
        changed=True
    if path.name=='mcp_credentials.json':
        value=data.get(name+':'+item['url'])
        if value and (previous_token is None or value.get('token_response',{}).get('access_token')==previous_token):
            value['token_response'].update(access_token=item['access_token'],refresh_token=item.get('refresh_token'),expires_in=max(0,int(item.get('expires_at',0)-time.time())))
            value['token_received_at']=int(time.time());changed=True
    if path.name.endswith('_tokens.json') and (previous_token is None or data.get('access_token')==previous_token):
        data.update(access_token=item['access_token'],refresh_token=item.get('refresh_token'),expires_in=max(0,int(item.get('expires_at',0)-time.time())))
        changed=True
    if changed:
        replace_config(home,path,before,(json.dumps(data,ensure_ascii=False,indent=2)+'\n').encode())


def import_credentials(home,catalog):
    """Import matching MCP endpoints once. Never read model authentication."""
    with locked(home,'credentials'):
        return _import_credentials(home,catalog)


def _import_credentials(home,catalog):
    home=Path(home); candidates=[]
    paths=[home/'.claude/.credentials.json']+list(home.glob('.claude-accounts/*/.credentials.json'))
    for path in paths:
        for item in read_config(path).get('mcpOAuth',{}).values():
            if not isinstance(item,dict) or not item.get('accessToken'):
                continue
            candidates.append((ALIASES.get(item.get('serverName'),item.get('serverName')),{
                'url':item.get('serverUrl'), 'access_token':item['accessToken'],
                'refresh_token':item.get('refreshToken'), 'client_id':item.get('clientId'),
                'expires_at':(item.get('expiresAt') or 0)/1000,
                'issuer':item.get('discoveryState',{}).get('authorizationServerUrl'),
                'source':str(path)}))
    for key,item in read_config(home/'.grok/mcp_credentials.json').items():
        if not isinstance(item,dict) or not item.get('token_response',{}).get('access_token'):
            continue
        name,url=key.split(':',1);token=item['token_response'];received=received_timestamp(item.get('token_received_at',0))
        candidates.append((ALIASES.get(name,name),{'url':url,'access_token':token['access_token'],
            'refresh_token':token.get('refresh_token'),'client_id':item.get('client_id'),
            'issuer':item.get('issuer'),'expires_at':received+token.get('expires_in',0),
            'source':str(home/'.grok/mcp_credentials.json')}))
    # mcp-remote caches tokens by endpoint hash and package version. Reuse the
    # freshest cache without launching an OAuth browser or refreshing it here.
    for name,spec in catalog['servers'].items():
        if not spec.get('url'):
            continue
        endpoint=spec['url'];key=hashlib.md5(endpoint.encode()).hexdigest()
        for path in home.glob('.mcp-auth/*/'+key+'_tokens.json'):
            token=read_config(path);client=read_config(path.with_name(key+'_client_info.json'))
            if token.get('access_token'):
                candidates.append((name,{'url':endpoint,'access_token':token['access_token'],
                    'refresh_token':token.get('refresh_token'),'client_id':client.get('client_id'),
                    'client_secret':client.get('client_secret'),
                    'token_endpoint_auth_method':client.get('token_endpoint_auth_method'),
                    'expires_at':path.stat().st_mtime+token.get('expires_in',0),
                    'issuer':endpoint.split('/v1/')[0], 'source':str(path)}))
    data=load_credentials(home)
    for name,item in sorted(candidates,key=lambda x:x[1].get('expires_at',0),reverse=True):
        if name not in catalog['servers']:
            continue
        if name in data and data[name].get('url')==catalog['servers'][name].get('url'):
            continue
        if catalog['servers'][name].get('url') == item['url']:
            data[name]=item
    if load_credentials(home)!=data:
        save_credentials(home,data)
    return sorted(data)


def refresh_oauth(credential):
    issuer=credential.get('issuer') or ''
    url=urlsplit(issuer)
    if url.scheme!='https' or not url.hostname:
        raise AuthError('OAuth issuer unavailable')
    with httpx.Client(timeout=20,follow_redirects=False) as client:
        token_endpoint=credential.get('token_endpoint')
        if not token_endpoint:
            base=url.scheme+'://'+url.netloc
            candidates=[base+'/.well-known/oauth-authorization-server'+url.path.rstrip('/'),
                        issuer.rstrip('/')+'/.well-known/openid-configuration',
                        base+'/.well-known/oauth-authorization-server']
            for address in dict.fromkeys(candidates):
                response=client.get(address)
                if response.status_code==200:
                    metadata=response.json()
                    if metadata.get('token_endpoint'):
                        token_endpoint=metadata['token_endpoint'];break
        if not token_endpoint or urlsplit(token_endpoint).scheme!='https':
            raise AuthError('OAuth token endpoint unavailable')
        payload={'grant_type':'refresh_token','refresh_token':credential['refresh_token']}
        if credential.get('client_id'):
            payload['client_id']=credential['client_id']
        auth=None
        if credential.get('client_secret'):
            if credential.get('token_endpoint_auth_method')=='client_secret_basic':
                auth=httpx.BasicAuth(credential['client_id'],credential['client_secret'])
            else:
                payload['client_secret']=credential['client_secret']
        response=client.post(token_endpoint,data=payload,auth=auth)
        if response.status_code!=200:
            raise AuthError('OAuth refresh rejected (HTTP '+str(response.status_code)+')')
        result=response.json()
        if not isinstance(result.get('access_token'),str) or not result['access_token']:
            raise AuthError('OAuth refresh returned no access token')
        result['_token_endpoint']=token_endpoint
        return result


def newer_source(item):
    """Read a later native refresh without copying unrelated account auth."""
    if not item.get('source'):
        return None
    path=Path(item['source']);data=read_config(path)
    for value in data.get('mcpOAuth',{}).values():
        expiry=(value.get('expiresAt') or 0)/1000
        if value.get('serverUrl')==item['url'] and value.get('accessToken') and expiry>item.get('expires_at',0):
            return {**item,'access_token':value['accessToken'],'refresh_token':value.get('refreshToken'),
                    'expires_at':expiry,'client_id':value.get('clientId',item.get('client_id'))}
    if path.name.endswith('_tokens.json') and data.get('access_token'):
        expiry=path.stat().st_mtime+data.get('expires_in',0)
        if data['access_token']!=item.get('access_token') and expiry>item.get('expires_at',0):
            return {**item,'access_token':data['access_token'],'refresh_token':data.get('refresh_token'),'expires_at':expiry}
    if path.name=='mcp_credentials.json':
        for value in data.values():
            token=value.get('token_response',{}) if isinstance(value,dict) else {}
            if token.get('access_token')==item.get('access_token'):
                continue
            received=received_timestamp(value.get('token_received_at',0))
            if isinstance(received,(int,float)) and token.get('access_token'):
                expiry=received+token.get('expires_in',0)
                # Grok's record must still identify the same endpoint.
                if any(k.endswith(':'+item['url']) and v is value for k,v in data.items()) and expiry>item.get('expires_at',0):
                    return {**item,'access_token':token['access_token'],'refresh_token':token.get('refresh_token'),'expires_at':expiry}
    return None


def access_token(home,name,endpoint,*,refresh=refresh_oauth,rejected_token=None):
    # One cross-process lock protects all read/refresh/write operations. Another
    # client sees the replacement refresh token, never a copied stale token.
    with locked(home,'credentials'):
        data=load_credentials(home);item=data.get(name)
        if item is None:
            return None
        if item.get('url')!=endpoint:
            raise AuthError('Credential endpoint mismatch for '+name)
        latest=newer_source(item)
        if latest:
            item=data[name]=latest
            save_credentials(home,data)
        token=item.get('access_token');expiry=item.get('expires_at',0)
        rejected=bool(rejected_token and token==rejected_token)
        if not rejected and (not expiry or expiry>time.time()+60):
            return token
        if not item.get('refresh_token'):
            if not rejected:
                # Some saved credentials have unreliable expiry metadata.
                # Let the service evaluate them, but do not invent a login.
                return token
            raise AuthError('Credential rejected for '+name)
        try:
            replacement=refresh(dict(item))
            if not replacement.get('access_token'):
                raise ValueError('missing access token')
        except Exception:
            raise AuthError('Refresh failed for '+name) from None
        item['access_token']=replacement['access_token']
        item['refresh_token']=replacement.get('refresh_token') or item['refresh_token']
        item['expires_at']=time.time()+replacement.get('expires_in',3600)
        if replacement.get('_token_endpoint'):
            item['token_endpoint']=replacement['_token_endpoint']
        save_credentials(home,data)
        sync_source(home,name,item,previous_token=token)
        return item['access_token']


class SharedAuth(httpx.Auth):
    def __init__(self,home,name,endpoint):
        self.home,self.name,self.endpoint=home,name,endpoint

    async def async_auth_flow(self,request):
        import anyio
        # Follow neither credential-bearing redirects nor endpoint substitutions.
        actual=urlsplit(str(request.url));expected=urlsplit(self.endpoint)
        if (actual.scheme,actual.hostname,actual.port)!=(expected.scheme,expected.hostname,expected.port):
            raise AuthError('Refusing credential forwarding to another endpoint')
        token=await anyio.to_thread.run_sync(lambda:access_token(self.home,self.name,self.endpoint))
        if token:
            request.headers['Authorization']='Bearer '+token
        response=yield request
        if response.status_code==401 and token:
            await response.aread()
            new=await anyio.to_thread.run_sync(lambda:access_token(self.home,self.name,self.endpoint,rejected_token=token))
            if new and new!=token:
                request.headers['Authorization']='Bearer '+new
                yield request
