"""Private terminal DOM fixture: literal production source, isolated transports.

No dashboard/native/PTY broker is launched. The HTML and CSS come directly from
dash/term.html. Only bootstrap script tags and transport inputs differ.
"""
import argparse
import hashlib
import http.server
import json
import pathlib
import urllib.parse

ROOT = pathlib.Path(__file__).resolve().parents[4]
TARGET = pathlib.Path('/home/someguy/codebase/0xJesus/ComandOS/.build/target-fase3/web')
INITIAL = {'drafts': {'tab:private-dom': {'text': 'git status\n😀 borrador local', 'selStart': 4, 'selEnd': 10}}, 'readingAnchors': {}}

BOOTSTRAP = """
localStorage.setItem('comandos.deviceId','private-dom-device');
sessionStorage.removeItem('comandos:terminal-draft:private-dom');
window.productApiCalls=[];window.productSends=[];
window.WebSocket=class extends EventTarget {
 static OPEN=1;static CLOSED=3;readyState=1;protocol='tty';
 constructor(){super();setTimeout(()=>this.dispatchEvent(new Event('open')),0);}
 send(value){productSends.push(typeof value==='string'?value:'binary');}
 close(){this.readyState=3;}
};
window.productBoot=(async()=>{
 if(new URLSearchParams(location.search).get('web')!=='off'){
  const m=await(await fetch('/web/manifest.json')).json();
  const mod=await import('/web/'+m.files.comandos_web_js);
  await mod.default('/web/'+m.files.comandos_web_bg_wasm);
  mod.boot('private-product-terminal');
 }
 for(const name of ['createDrafts','createAnchor']){
  const original=ComandosDeviceDrafts[name];
  ComandosDeviceDrafts[name]=(...args)=>{productApiCalls.push('ComandosDeviceDrafts.'+name);return original(...args);};
 }
})();
"""

class Handler(http.server.SimpleHTTPRequestHandler):
    def translate_path(self, path):
        path = urllib.parse.urlsplit(path).path
        if path.startswith('/web/'):
            return str(TARGET / path[5:])
        if path in ('/device-drafts.js', '/buttons.css', '/icon-192.png'):
            return str(ROOT / 'dash' / path.lstrip('/'))
        return str(ROOT / path.lstrip('/'))

    def send_json(self, value):
        body = json.dumps(value, ensure_ascii=False).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        url = urllib.parse.urlsplit(self.path)
        if url.path == '/web/manifest.json':
            manifest = json.loads((TARGET / 'manifest.json').read_text())['files']
            return self.send_json({'files': {k.replace('.', '_'): v for k, v in manifest.items()}})
        if url.path == '/workspace/client':
            return self.send_json(INITIAL)
        if url.path in ('/term/panes', '/terminal-panes', '/tmux-mouse', '/theme', '/tab-models'):
            return self.send_json({'panes': [], 'models': [], 'enabled': True})
        if url.path == '/product/term.html':
            source = (ROOT / 'dash/term.html').read_text()
            candidate = urllib.parse.parse_qs(url.query).get('web') != ['off']
            if candidate:
                source = source.replace('<script src="../device-drafts.js"></script>', '')
            # Start bootstrap after original script libraries, before the original
            # consumer; module await gates that consumer on actual WASM exports.
            source = source.replace('<script>\n(function () {', '<meta name="comandos-web" content="device-drafts"><script>' + BOOTSTRAP + '</script><script type="module">\nawait window.productBoot;\n(function () {', 1)
            source = source.replace('<meta charset="utf-8">', '<meta charset="utf-8"><meta name="product-source-sha256" content="' + hashlib.sha256((ROOT/'dash/term.html').read_bytes()).hexdigest() + '">', 1)
            body = source.encode()
            self.send_response(200)
            self.send_header('Content-Type', 'text/html; charset=utf-8')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            return self.wfile.write(body)
        return super().do_GET()

    def do_POST(self):
        self.rfile.read(int(self.headers.get('Content-Length', '0')))
        if urllib.parse.urlsplit(self.path).path in ('/terminal-panes', '/tab-models'):
            return self.send_json({'panes': [], 'models': []})
        if urllib.parse.urlsplit(self.path).path in ('/web/ready', '/workspace/client'):
            return self.send_json({'ok': True})
        self.send_error(404)

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--port', type=int, default=7318)
    args = parser.parse_args()
    if 4777 <= args.port <= 4782:
        raise SystemExit('Live dashboard ports are forbidden')
    http.server.ThreadingHTTPServer(('127.0.0.1', args.port), Handler).serve_forever()
