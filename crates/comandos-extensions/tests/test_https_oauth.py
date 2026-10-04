"""Real TLS refresh exchange, isolated issuer and explicit temporary CA trust."""
import base64
import json
import os
import pathlib
import ssl
import subprocess
import threading
import time
from http.server import ThreadingHTTPServer
from urllib.parse import parse_qs

import pytest
from test_serve import Handler, launch, initialize, close, send, receive


@pytest.fixture(scope='module')
def certificate(tmp_path_factory):
    directory=tmp_path_factory.mktemp('oauth-test-ca')
    def openssl(*args):
        subprocess.run(['openssl',*args],cwd=directory,check=True,capture_output=True,timeout=15)
    openssl('req','-x509','-newkey','rsa:2048','-nodes','-keyout','ca.key','-out','ca.pem',
            '-days','1','-subj','/CN=isolated ComandOS test CA','-config','/dev/null',
            '-addext','basicConstraints=critical,CA:TRUE','-addext','keyUsage=critical,keyCertSign,cRLSign')
    openssl('req','-new','-newkey','rsa:2048','-nodes','-keyout','server.key','-out','server.csr',
            '-subj','/CN=localhost','-config','/dev/null')
    (directory/'extensions.cnf').write_text('basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n')
    openssl('x509','-req','-in','server.csr','-CA','ca.pem','-CAkey','ca.key','-CAcreateserial',
            '-out','server.pem','-days','1','-extfile','extensions.cnf')
    return directory


@pytest.fixture
def issuer(certificate):
    servers=[]
    def start(handler):
        server=ThreadingHTTPServer(('127.0.0.1',0),handler);server.daemon_threads=True
        context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(certificate/'server.pem',certificate/'server.key')
        server.socket=context.wrap_socket(server.socket,server_side=True)
        server.url=f'https://127.0.0.1:{server.server_port}'
        server.calls=[];server.posts=[];server.tokens=[];server.redirects=[]
        threading.Thread(target=server.serve_forever,daemon=True).start();servers.append(server)
        return server
    yield start
    for server in servers:server.shutdown();server.server_close()


class OAuth(Handler):
    def reply(self,code,data=None,**headers):
        body=json.dumps(data).encode() if data is not None else b''
        self.send_response(code)
        for key,value in headers.items():self.send_header(key,value)
        self.send_header('Content-Length',str(len(body)));self.send_header('Content-Type','application/json')
        self.end_headers();self.wfile.write(body)
    def do_GET(self):
        self.server.calls.append(self.path)
        if self.path=='/issuer/.well-known/openid-configuration':
            self.reply(200,{'token_endpoint':self.server.url+'/token'})
        else:self.reply(404)
    def do_POST(self):
        if self.path=='/token':
            payload=parse_qs(self.rfile.read(int(self.headers['Content-Length'])).decode())
            self.server.posts.append((payload,self.headers.get('Authorization')))
            self.reply(200,{'access_token':'new-test-access','expires_in':3600});return
        if self.path=='/mcp':
            self.server.tokens.append(self.headers.get('Authorization'))
            if self.headers.get('Authorization')!='Bearer new-test-access':
                self.rfile.read(int(self.headers.get('Content-Length','0')));self.reply(401);return
        super().do_POST()


def credentials(home,url,**changes):
    path=home/'.config/comandos/extensions/credentials.json';path.parent.mkdir(parents=True,exist_ok=True)
    item={'url':url+'/mcp','issuer':url+'/issuer','access_token':'old-test-access','refresh_token':'old-test-refresh',
          'expires_at':time.time()+3600,'client_id':'test-client','client_secret':'test-secret',**changes}
    path.write_text(json.dumps({'demo':item,'untouched':{'marker':'keep'}}));return path


def trusted_env(certificate):return {**os.environ,'SSL_CERT_FILE':str(certificate/'ca.pem')}


@pytest.mark.parametrize('method',['client_secret_basic','client_secret_post'])
def test_tls_discovery_refresh_post_and_401_retry(tmp_path,issuer,certificate,method):
    server=issuer(OAuth);path=credentials(tmp_path,server.url,token_endpoint_auth_method=method)
    p=launch(tmp_path,{'url':server.url+'/mcp'},env=trusted_env(certificate))
    try:
        initialize(p);send(p,'tools/call',{'name':'echo','arguments':{'value':'verified'}})
        assert receive(p)['result']['structuredContent']=={'value':'verified'}
        assert server.calls==['/.well-known/oauth-authorization-server/issuer','/issuer/.well-known/openid-configuration']
        assert len(server.posts)==1
        form,authorization=server.posts[0]
        assert form['grant_type']==['refresh_token'] and form['refresh_token']==['old-test-refresh']
        assert form['client_id']==['test-client']
        if method=='client_secret_basic':
            assert authorization=='Basic '+base64.b64encode(b'test-client:test-secret').decode()
            assert 'client_secret' not in form
        else:assert authorization is None and form['client_secret']==['test-secret']
        data=json.loads(path.read_text());assert data['untouched']=={'marker':'keep'}
        assert data['demo']['access_token']=='new-test-access'
        assert data['demo']['refresh_token']=='old-test-refresh'
        assert data['demo']['token_endpoint']==server.url+'/token'
        assert server.tokens[0]=='Bearer old-test-access' and all(v=='Bearer new-test-access' for v in server.tokens[1:])
    finally:close(p)


def test_tls_token_redirect_is_not_followed(tmp_path,issuer,certificate):
    class Redirect(OAuth):
        def do_POST(self):
            if self.path=='/token':
                self.rfile.read(int(self.headers['Content-Length']));self.reply(302,Location=self.server.url+'/must-not-follow');return
            self.server.redirects.append(self.path);super().do_POST()
    server=issuer(Redirect);path=credentials(tmp_path,server.url,expires_at=1,token_endpoint=server.url+'/token');before=path.read_bytes()
    p=launch(tmp_path,{'url':server.url+'/mcp'},env=trusted_env(certificate))
    try:
        send(p,'initialize');assert p.wait(timeout=8)!=0
        assert p.stdout.read()=='' and p.stderr.read().strip()=='Refresh failed'
        assert server.redirects==[] and path.read_bytes()==before
    finally:
        p.stdin.close()
        if p.poll() is None:p.kill();p.wait()
