//! Residuo provisional: archivos sobre HOME temporal, oráculo stdlib sin servidor.
mod support;
use bytes::Bytes;
use comandos_server::{ReplyBody, Request, dash::native::residue};
use http::Method;
use serde_json::{Value, json};
use std::{fs, process::Command};
use support::TestHome;

fn request(method: &str, target: &str) -> Request {
    Request {
        method: Method::from_bytes(method.as_bytes()).unwrap(),
        target: target.into(),
        peer: "127.0.0.1:1".parse().unwrap(),
        headers: vec![],
        data: None,
        body: Bytes::new(),
        internal_producer: false,
    }
}
fn oracle(home: &TestHome, cases: &Value) -> Value {
    let output=Command::new("/usr/bin/python3").args(["-c",r#"
import http.server,io,json,sys,email.message
class Handler(http.server.SimpleHTTPRequestHandler):
 def send_response(self,status,message=None):self.status=status
 def send_header(self,k,v):self.saved[k.lower()]=str(v)
 def end_headers(self):pass
 def log_error(self,*args):pass
out=[]
for case in json.loads(sys.argv[2]):
 h=object.__new__(Handler);h.directory=sys.argv[1];h.path=case['target'];h.command=case['method'];h.wfile=io.BytesIO();h.headers=email.message.Message();h.saved={}
 for k,v in case.get('headers',[]):h.headers[k]=v
 h.request_version='HTTP/1.1';h.close_connection=False
 f=h.send_head()
 if f:
  if h.command!='HEAD':h.wfile.write(f.read())
  f.close()
 out.append({'status':h.status,'headers':h.saved,'body':list(h.wfile.getvalue())})
print(json.dumps(out))
"#]).arg(home.root.join("dash")).arg(cases.to_string()).env_clear().env("HOME",&home.root).env("LANG","C.UTF-8").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[tokio::test]
async fn static_directory_errors_normalization_and_head_match_python() {
    let home = TestHome::new("residue-files");
    let root = home.root.join("dash");
    fs::create_dir_all(root.join("listed/sub")).unwrap();
    fs::create_dir_all(root.join("indexed")).unwrap();
    for (p, v) in [
        ("index.html", "root"),
        ("indexed/index.htm", "index"),
        ("listed/a & ñ.txt", "Hola"),
        ("listed/UPPER.CSS", "body{}"),
        ("literal%zz.txt", "percent"),
    ] {
        fs::write(root.join(p), v).unwrap();
    }
    std::os::unix::fs::symlink("a & ñ.txt", root.join("listed/link")).unwrap();
    let targets = [
        "/",
        "/listed",
        "/listed?x=1",
        "/listed/",
        "/indexed/",
        "/missing",
        "/listed/a%20%26%20%C3%B1.txt",
        "/listed/../index.html",
        "/x/../../index.html",
        "/listed//UPPER.CSS",
        "/literal%zz.txt",
        "/index.html/",
    ];
    let mut cases: Vec<Value> = targets
        .iter()
        .flat_map(|target| ["GET", "HEAD"].map(|method| json!({"method":method,"target":target})))
        .collect();
    cases.push(json!({"method":"GET","target":"/index.html","headers":[["if-modified-since","Sun, 04 Oct 2099 12:00:00 GMT"]]}));
    cases.push(json!({"method":"HEAD","target":"/index.html","headers":[["if-modified-since","Sun, 04 Oct 2099 12:00:00 GMT"],["if-none-match","anything"]]}));
    let expected = oracle(&home, &json!(cases));
    for (case, expect) in cases.iter().zip(expected.as_array().unwrap()) {
        let mut req = request(
            case["method"].as_str().unwrap(),
            case["target"].as_str().unwrap(),
        );
        req.headers = case["headers"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|v| {
                (
                    v[0].as_str().unwrap().to_owned(),
                    v[1].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        let reply = residue::static_response(&root, &req)
            .await
            .unwrap_or_else(|_| panic!("declined {case}"));
        assert_eq!(reply.status.as_u16(), expect["status"], "{case}");
        for (k, v) in expect["headers"].as_object().unwrap() {
            assert_eq!(
                reply.headers.get(k).map(|h| h.to_str().unwrap()),
                v.as_str(),
                "{case} {k} {expect}"
            );
        }
        let ReplyBody::Bytes(body) = reply.body else {
            panic!("expected buffered fixture")
        };
        let actual = if req.method == Method::HEAD {
            vec![]
        } else {
            body.to_vec()
        };
        assert_eq!(json!(actual), expect["body"], "{case}");
    }
}

#[tokio::test]
async fn unknown_operator_and_delete_gate_match_python() {
    use support::{dead_port, front, oracle::oracle, request_body};
    let home = TestHome::new("residue-unknown");
    let rust = front(&home, dead_port(), home.options()).await;
    let py = oracle(&home)
        .await
        .expect("Python requerido para el oráculo");
    for (method, path, body) in [
        ("POST", "/nada", "{}"),
        ("POST", "/news/notes?x=1", "{}"),
        ("POST", "/news/chat/note/", "{}"),
        ("POST", "/news/chat?x=1", "{}"),
        ("POST", "/news/translate/", "{}"),
        ("POST", "/pomodoro?x", "{}"),
        ("DELETE", "/push/subscription?x", "{}"),
        ("POST", "/nada", r#"{"session":"fixture"}"#),
        ("POST", "/operatorX", "{}"),
        ("GET", "/operator/x", ""),
        ("DELETE", "/nada", "{}"),
        ("DELETE", "/nada", "[]"),
        ("DELETE", "/nada", "{"),
    ] {
        let a = request_body(rust.port, method, path, "", body).await;
        let b = request_body(py.port, method, path, "", body).await;
        assert_eq!(
            (a.status, a.text()),
            (b.status, b.text()),
            "{method} {path} {body}"
        );
    }
    let body = " ".repeat(64001);
    let a = request_body(rust.port, "DELETE", "/nada", "", &body).await;
    let b = request_body(py.port, "DELETE", "/nada", "", &body).await;
    assert_eq!((a.status, a.text()), (b.status, b.text()));
    assert_eq!(a.status, 413);
    rust.stop().await;
}
#[tokio::test]
async fn incomplete_routes_and_disabled_residue_still_forward() {
    use comandos_server::dash::native::Cut;
    use support::{FakeLegacy, front, get, request_body};
    let home = TestHome::new("residue-forward");
    let legacy = FakeLegacy::start().await;
    let rust = front(&home, legacy.port, home.options()).await;
    for path in ["/pane-extensionsX", "/eventsX"] {
        assert_eq!(get(rust.port, path).await.text(), r#"{"legacy": true}"#);
    }
    assert_eq!(
        request_body(rust.port, "POST", "/session/recover", "", "{}")
            .await
            .text(),
        r#"{"legacy": true}"#
    );
    rust.stop().await;
    let mut opts = home.options();
    opts.cuts_off.insert(Cut::Residue);
    let rust = front(&home, legacy.port, opts).await;
    assert_eq!(
        get(rust.port, "/missing-file-fixture").await.text(),
        r#"{"legacy": true}"#
    );
    assert_eq!(
        request_body(rust.port, "POST", "/nada", "", "{}")
            .await
            .text(),
        r#"{"legacy": true}"#
    );
    rust.stop().await;
}
#[tokio::test]
async fn static_wire_head_and_native_off_keep_their_contracts() {
    use comandos_server::dash::serve_with;
    use support::{FakeLegacy, config, request_body};
    let home = TestHome::new("residue-wire");
    let root = home.root.join("dash");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("index.html"), "Hello").unwrap();
    let legacy = FakeLegacy::start().await;
    for enabled in [true, false] {
        let mut cfg = config(&home, legacy.port);
        cfg.dash_dir = root.clone();
        cfg.native = enabled;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (stop, shutdown) = tokio::sync::watch::channel(false);
        let server = tokio::spawn(serve_with(listener, cfg, Some(home.options()), shutdown));
        let h = request_body(port, "HEAD", "/index.html", "", "").await;
        assert_eq!(h.status, 200);
        assert_eq!(h.header("content-length"), Some("5"));
        assert!(h.body.is_empty());
        let missing = request_body(port, "GET", "/not-here-fixture", "", "").await;
        if enabled {
            assert_eq!(missing.status, 404);
            assert!(missing.text().contains("Error response"));
            assert_eq!(missing.header("cache-control"), Some("no-store"));
        } else {
            assert_eq!(missing.text(), r#"{"legacy": true}"#);
        }
        let post = request_body(port, "POST", "/nada", "", "{}").await;
        assert_eq!(post.status, if enabled { 400 } else { 200 });
        stop.send(true).unwrap();
        server.await.unwrap().unwrap();
    }
}
#[tokio::test]
async fn fifo_and_large_file_do_not_block_or_allocate_unbounded_body() {
    let home = TestHome::new("residue-bounds");
    let root = home.root.join("dash");
    fs::create_dir(&root).unwrap();
    nix::unistd::mkfifo(
        &root.join("fifo"),
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .unwrap();
    let reply = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        residue::static_response(&root, &request("GET", "/fifo")),
    )
    .await
    .unwrap()
    .unwrap_or_else(|_| panic!("fifo"));
    assert_eq!(reply.status, 404);
    fs::File::create(root.join("large"))
        .unwrap()
        .set_len(17 * 1024 * 1024)
        .unwrap();
    assert!(
        residue::static_response(&root, &request("GET", "/large"))
            .await
            .is_err()
    );
    for path in ["/%00", "/%ED%B2%80"] {
        assert!(
            residue::static_response(&root, &request("GET", path))
                .await
                .is_err()
        );
    }
}
#[test]
fn mime_table_matches_python_for_checkout_extensions() {
    use comandos_server::dash::statics::mime_for;
    let out=Command::new("/usr/bin/python3").args(["-c","import pathlib,mimetypes,json,sys;print(json.dumps({p.suffix:mimetypes.guess_type(str(p))[0] or 'application/octet-stream' for p in pathlib.Path(sys.argv[1]).rglob('*') if p.is_file()}))"]).arg(support::repo().join("dash")).env_clear().env("LANG","C.UTF-8").output().unwrap();
    assert!(out.status.success());
    let types: Value = serde_json::from_slice(&out.stdout).unwrap();
    for (ext, mime) in types.as_object().unwrap() {
        assert_eq!(
            mime_for(&format!("file{ext}")),
            mime.as_str().unwrap(),
            "{ext}"
        );
    }
}
