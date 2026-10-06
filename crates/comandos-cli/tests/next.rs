//! Real Python differential; every process uses a private HOME and XDG roots.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "comandos-next-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for sub in [
            "home", "tmp", "data", "config", "cache", "state", "run", "bin",
        ] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(sub))
                .unwrap();
        }
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_comandos"), root.join("bin/cc-next"))
            .unwrap();
        Self(root)
    }
    fn home(&self) -> PathBuf {
        self.0.join("home")
    }
    fn state(&self, name: &str, value: &str) {
        let dir = self.home().join(".claude/hooks/state");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(name), value).unwrap();
    }
    fn command(&self, program: &Path) -> Command {
        let mut command = Command::new(program);
        command
            .env_clear()
            .env("HOME", self.home())
            .env("PATH", "/usr/bin:/bin")
            .env("TMPDIR", self.0.join("tmp"));
        for (key, sub) in [
            ("XDG_DATA_HOME", "data"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_RUNTIME_DIR", "run"),
        ] {
            command.env(key, self.0.join(sub));
        }
        command
    }
    fn compare_dry(&self, args: &[&str]) {
        let original = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bin/cc-next");
        let before = tree(&self.0);
        let python = self
            .command(Path::new("/usr/bin/python3"))
            .arg(original)
            .args(args)
            .output()
            .unwrap();
        assert!(
            python.status.success(),
            "{}",
            String::from_utf8_lossy(&python.stderr)
        );
        assert_eq!(tree(&self.0), before);
        for (program, prefix) in [
            (PathBuf::from(env!("CARGO_BIN_EXE_comandos")), vec!["next"]),
            (self.0.join("bin/cc-next"), vec![]),
        ] {
            let rust = self
                .command(&program)
                .args(prefix)
                .args(args)
                .output()
                .unwrap();
            same(&rust, &python);
            assert_eq!(tree(&self.0), before, "read changed fixture tree");
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn same(a: &Output, b: &Output) {
    assert_eq!(
        a.status.code(),
        b.status.code(),
        "{}",
        String::from_utf8_lossy(&a.stderr)
    );
    assert_eq!(a.stdout, b.stdout);
    assert_eq!(a.stderr, b.stderr);
}
type Stamp = (PathBuf, u64, u32, i64, i64, Vec<u8>);
fn tree(root: &Path) -> Vec<Stamp> {
    fn walk(path: &Path, out: &mut Vec<Stamp>) {
        let Ok(m) = fs::symlink_metadata(path) else {
            return;
        };
        let body = if m.is_file() {
            fs::read(path).unwrap()
        } else if m.file_type().is_symlink() {
            fs::read_link(path)
                .unwrap()
                .as_os_str()
                .as_encoded_bytes()
                .to_vec()
        } else {
            vec![]
        };
        out.push((
            path.to_owned(),
            m.ino(),
            m.mode(),
            m.mtime(),
            m.mtime_nsec(),
            body,
        ));
        if m.is_dir() {
            for e in fs::read_dir(path).unwrap() {
                walk(&e.unwrap().path(), out);
            }
        }
    }
    let mut out = vec![];
    walk(root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn dry_matches_original_priority_defaults_ties_corruption_and_symlinks_without_writes() {
    let f = Fixture::new();
    f.compare_dry(&["ignored", "--dry"]);
    f.state(
        "z.json",
        r#"{"project":"latest","status":"running","ts":99}"#,
    );
    f.state("broken.json", "{");
    f.state(
        ".hidden.json",
        r#"{"project":"hidden","status":"waiting","ts":-100}"#,
    );
    f.state("a.json", r#"{"project":"done.old","status":"done","ts":5}"#);
    f.state("b.json", r#"{"project":"done:new","status":"done","ts":6}"#);
    f.compare_dry(&["--dry"]);
    f.state(
        "missing.json",
        r#"{"project":"first.waiting","status":"waiting"}"#,
    );
    f.state(
        "equal.json",
        r#"{"project":"second.waiting","status":"waiting","ts":0}"#,
    );
    f.compare_dry(&["--dry", "other"]);
    let target = f.0.join("linked.json");
    fs::write(
        &target,
        r#"{"project":"linked:waiting","status":"waiting","ts":-1}"#,
    )
    .unwrap();
    std::os::unix::fs::symlink(&target, f.home().join(".claude/hooks/state/link.json")).unwrap();
    std::os::unix::fs::symlink(
        f.0.join("absent"),
        f.home().join(".claude/hooks/state/dangling.json"),
    )
    .unwrap();
    fs::create_dir(f.home().join(".claude/hooks/state/directory.json")).unwrap();
    f.compare_dry(&["--dry"]);
    fs::remove_dir_all(f.home()).unwrap();
    f.compare_dry(&["--dry"]);
}
#[test]
fn dry_preserves_priority_before_timestamp_validation_and_integer_precision() {
    let f = Fixture::new();
    f.state("first.json", r#"{"project":"first","ts":9007199254740992}"#);
    f.state(
        "second.json",
        r#"{"project":"second","ts":9007199254740993}"#,
    );
    f.compare_dry(&["--dry"]);
    f.state(
        "waiting.json",
        r#"{"project":"waiting","status":"waiting","ts":true}"#,
    );
    f.state(
        "first.json",
        r#"{"project":"ignored","status":"done","ts":null}"#,
    );
    f.compare_dry(&["--dry"]);
    fs::remove_dir_all(f.home().join(".claude/hooks/state")).unwrap();
    f.state("only.json", r#"{"status":null}"#);
    f.compare_dry(&["--dry"]);
}

#[test]
fn dry_preserves_python_nonfinite_large_integer_and_status_representation() {
    let f = Fixture::new();
    f.state("first.json",r#"{"project":"first","ts":999999999999999999999999999999999999999999999999999999999999999999999998}"#);
    f.state("second.json",r#"{"project":"second","ts":999999999999999999999999999999999999999999999999999999999999999999999999}"#);
    f.compare_dry(&["--dry"]);
    f.state(
        "first.json",
        r#"{"project":"first","ts":NaN,"status":[true,null,"text"]}"#,
    );
    f.state("second.json", r#"{"project":"second","ts":Infinity}"#);
    f.compare_dry(&["--dry"]);
}

type Request = (String, String, serde_json::Value);
fn serve(responses: Vec<Vec<u8>>) -> (u16, std::thread::JoinHandle<Vec<Request>>) {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        time::{Duration, Instant},
    };
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    assert_ne!(port, 4777);
    listener.set_nonblocking(true).unwrap();
    let thread = std::thread::spawn(move || {
        let mut calls = vec![];
        for response in responses {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "missing private HTTP call");
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut head = vec![];
            let mut byte = [0];
            while !head.ends_with(b"\r\n\r\n") {
                assert_eq!(stream.read(&mut byte).unwrap(), 1);
                head.push(byte[0]);
            }
            let text = String::from_utf8(head).unwrap();
            let len = text
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .unwrap_or("0")
                .parse::<usize>()
                .unwrap();
            let mut body = vec![0; len];
            stream.read_exact(&mut body).unwrap();
            calls.push((
                text.lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .into(),
                text.lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .into(),
                if body.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::from_slice(&body).unwrap()
                },
            ));
            stream.write_all(&response).unwrap();
        }
        calls
    });
    (port, thread)
}
#[test]
fn http_matches_original_focus_up_http_failures_disconnect_and_session_normalization() {
    use comandos_cli::next::{Config, run};
    use std::time::Duration;
    let f = Fixture::new();
    let project = format!("á.β:{}", "x".repeat(90));
    f.state(
        "one.json",
        &serde_json::json!({"project":project,"status":"waiting","cwd":"/private/cwd"}).to_string(),
    );
    for replies in [
        vec![b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec()],
        vec![
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ],
        vec![
            b"HTTP/1.1 500 Broken\r\nContent-Length: 0\r\n\r\n".to_vec(),
            b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ],
        vec![vec![]],
        vec![b"invalid status\r\n".to_vec()],
        vec![b"HTTP/2.0 200 OK\r\n\r\n".to_vec()],
        vec![b"HTTP/1.1  200 OK\r\n\r\n".to_vec()],
        vec![b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\n\r\n".to_vec()],
        vec![
            b"HTTP/1.1 302 Found\r\nLocation: /moved\r\nContent-Length: 0\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ],
    ] {
        let (port, server) = serve(replies.clone());
        let original = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bin/cc-next");
        let oracle=f.command(Path::new("/usr/bin/python3")).arg("-c").arg("import runpy,sys,urllib.request; original=urllib.request.urlopen; port=sys.argv[2]; urllib.request.urlopen=lambda req,timeout: original(urllib.request.Request(req.full_url.replace(':4777', ':'+port),data=req.data,headers=req.headers),timeout=timeout); sys.argv=[sys.argv[1]]; runpy.run_path(sys.argv[0],run_name='__main__')").arg(original).arg(port.to_string()).output().unwrap();
        let expected_calls = server.join().unwrap();
        let (port, server) = serve(replies);
        let before = tree(&f.0);
        let rust = run(
            &Config {
                home: f.home(),
                port,
                timeout: Duration::from_secs(8),
            },
            &[],
        );
        assert_eq!(rust.code, oracle.status.code().unwrap());
        assert_eq!(rust.stdout.as_bytes(), oracle.stdout);
        assert_eq!(rust.stderr.as_bytes(), oracle.stderr);
        assert_eq!(server.join().unwrap(), expected_calls);
        assert_eq!(tree(&f.0), before);
        assert_eq!(
            expected_calls[0].2["session"],
            project
                .replace(['.', ':'], "-")
                .chars()
                .take(80)
                .collect::<String>()
        );
    }
}

fn redirect_server(
    locations: &[&str],
    targets: &[&str],
) -> (u16, std::thread::JoinHandle<Vec<Request>>) {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        time::{Duration, Instant},
    };
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    assert_ne!(port, 4777);
    listener.set_nonblocking(true).unwrap();
    let locations = locations
        .iter()
        .map(|s| s.replace("{port}", &port.to_string()))
        .collect::<Vec<_>>();
    let targets = targets.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let server = std::thread::spawn(move || {
        let mut calls = vec![];
        for index in 0..=targets.len() {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "missing private redirect request"
                        );
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut head = vec![];
            let mut byte = [0];
            while !head.ends_with(b"\r\n\r\n") {
                assert_eq!(stream.read(&mut byte).unwrap(), 1);
                head.push(byte[0]);
            }
            let text = String::from_utf8(head).unwrap();
            let mut first = text.lines().next().unwrap().split_whitespace();
            let method = first.next().unwrap().to_owned();
            let target = first.next().unwrap().to_owned();
            let len = text
                .lines()
                .find_map(|s| s.strip_prefix("Content-Length: "))
                .unwrap_or("0")
                .parse::<usize>()
                .unwrap();
            let mut body = vec![0; len];
            stream.read_exact(&mut body).unwrap();
            let body = if body.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::from_slice(&body).unwrap()
            };
            let matched = targets.get(index) == Some(&target)
                && method == if index == 0 { "POST" } else { "GET" };
            let up = target == "/up";
            calls.push((method, target, body));
            let response = if matched {
                if let Some(location) = locations.get(index) {
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n"
                    )
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".into()
                }
            } else if up {
                "HTTP/1.1 500 Failed\r\nContent-Length: 0\r\n\r\n".into()
            } else {
                "HTTP/1.1 404 Wrong target\r\nContent-Length: 0\r\n\r\n".into()
            };
            stream.write_all(response.as_bytes()).unwrap();
            if (matched && index >= locations.len()) || up {
                break;
            }
        }
        calls
    });
    (port, server)
}
fn compare_redirects(locations: &[&str], targets: &[&str]) {
    use comandos_cli::next::{Config, run};
    use std::time::Duration;
    let f = Fixture::new();
    f.state(
        "one.json",
        r#"{"project":"review:session","status":"waiting","cwd":"/private"}"#,
    );
    let before = tree(&f.0);
    let (port, server) = redirect_server(locations, targets);
    let original = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bin/cc-next");
    let oracle=f.command(Path::new("/usr/bin/python3")).arg("-c").arg("import runpy,sys,urllib.request; original=urllib.request.urlopen; port=sys.argv[2]; urllib.request.urlopen=lambda req,timeout: original(urllib.request.Request(req.full_url.replace(':4777', ':'+port),data=req.data,headers=req.headers),timeout=timeout); sys.argv=[sys.argv[1]]; runpy.run_path(sys.argv[0],run_name='__main__')").arg(original).arg(port.to_string()).output().unwrap();
    let oracle_calls = server.join().unwrap();
    assert_eq!(
        oracle.status.code(),
        Some(0),
        "{locations:?}: {}",
        String::from_utf8_lossy(&oracle.stdout)
    );
    assert_eq!(oracle_calls.len(), targets.len());
    assert!(oracle_calls.iter().all(|(_, target, _)| target != "/up"));
    assert_eq!(tree(&f.0), before);
    let (port, server) = redirect_server(locations, targets);
    let native = run(
        &Config {
            home: f.home(),
            port,
            timeout: Duration::from_secs(8),
        },
        &[],
    );
    let native_calls = server.join().unwrap();
    assert_eq!(
        native.code,
        oracle.status.code().unwrap(),
        "{locations:?}: calls={native_calls:?}"
    );
    assert_eq!(native.stdout.as_bytes(), oracle.stdout);
    assert_eq!(native.stderr.as_bytes(), oracle.stderr);
    assert_eq!(native_calls, oracle_calls, "{locations:?}");
    assert_eq!(tree(&f.0), before);
}
#[test]
fn local_query_only_redirect_matches_python_and_never_requests_up() {
    compare_redirects(&["?retry=1"], &["/focus", "/focus?retry=1"]);
}
#[test]
fn local_parent_segment_redirect_matches_python_and_never_requests_up() {
    compare_redirects(&["../focus2"], &["/focus", "/focus2"]);
}
#[test]
fn local_redirect_references_and_get_chains_match_python() {
    for (location, target) in [
        ("./directory/../focus2?x=1", "/focus2?x=1"),
        ("#kept", "/focus"),
        ("/root/../focus2?x=1#ignored", "/focus2?x=1"),
        ("//127.0.0.1:{port}/focus2?x=1#ignored", "/focus2?x=1"),
        ("http://127.0.0.1:{port}/focus2?x=1#ignored", "/focus2?x=1"),
        ("", "/focus"),
    ] {
        compare_redirects(&[location], &["/focus", target]);
    }
    for prefix in ["http://127.0.0.1:{port}", "//127.0.0.1:{port}"] {
        for suffix in [
            "/a/../focus2",
            "/a/../focus2?retry=1#ignored",
            "/%2e/../%252e\\%5C?raw=%25%255C#ignored",
        ] {
            let location = format!("{prefix}{suffix}");
            let target = suffix.split('#').next().unwrap();
            compare_redirects(&[&location], &["/focus", target]);
        }
        for (suffix, target) in [
            ("", "/"),
            ("?value=%5C%25#ignored", "/?value=%5C%25"),
            ("#ignored", "/"),
            ("/a/../focus2?", "/a/../focus2"),
        ] {
            compare_redirects(&[&format!("{prefix}{suffix}")], &["/focus", target]);
        }
        let first = format!("{prefix}/a/../focus2?old=%5C%25");
        compare_redirects(
            &[&first, "?again=%252e#ignored", "#ignored", "?", "./end"],
            &[
                "/focus",
                "/a/../focus2?old=%5C%25",
                "/a/../focus2?again=%252e",
                "/a/../focus2?again=%252e",
                "/a/../focus2?again=%252e",
                "/end",
            ],
        );
    }
    compare_redirects(
        &[
            "/dir/item?old=1",
            "?retry=2",
            "../focus2#drop",
            "//127.0.0.1:{port}/tail?q=1",
            "./end?final=1",
        ],
        &[
            "/focus",
            "/dir/item?old=1",
            "/dir/item?retry=2",
            "/focus2",
            "/tail?q=1",
            "/end?final=1",
        ],
    );
}

#[test]
fn encoded_dot_segments_remain_literal_like_python() {
    for (location, target) in [
        ("./%2e/focus2", "/%2e/focus2"),
        ("./%2E%2E/focus2", "/%2E%2E/focus2"),
        ("../%2e%2e/focus2", "/%2e%2e/focus2"),
    ] {
        compare_redirects(&[location], &["/focus", target]);
    }
    for escape in [
        "%5C",
        "%5c",
        "%25",
        "%252e",
        "%252E",
        "%255C",
        "%255c",
        "%2e%2E",
        "%25%5C%252e%255C",
        "%23",
        "%GG",
        "%",
    ] {
        let location = format!("./literal/{escape}?value={escape}#ignored");
        let target = format!("/literal/{escape}?value={escape}");
        compare_redirects(&[&location], &["/focus", &target]);
    }
    compare_redirects(
        &["./%2e/dir?key=%2e", "?retry=1#ignored", "../focus2"],
        &["/focus", "/%2e/dir?key=%2e", "/%2e/dir?retry=1", "/focus2"],
    );
    compare_redirects(
        &[
            "/dir\\%5C/%25/%252e/%255C?mix=%2e%5c%25#ignored",
            "?again=%255C#ignored",
            "./end?literal=%5C%25%252e%255C",
        ],
        &[
            "/focus",
            "/dir\\%5C/%25/%252e/%255C?mix=%2e%5c%25",
            "/dir\\%5C/%25/%252e/%255C?again=%255C",
            "/dir\\%5C/%25/%252e/end?literal=%5C%25%252e%255C",
        ],
    );
}
#[test]
fn local_backslashes_and_query_fragments_match_python() {
    compare_redirects(
        &["./dir\\focus2?retry=1#ignored"],
        &["/focus", "/dir\\focus2?retry=1"],
    );
    compare_redirects(
        &["./dir%5Cfocus2?token=%252e%5C#frag"],
        &["/focus", "/dir%5Cfocus2?token=%252e%5C"],
    );
    compare_redirects(
        &["/dir/item?old=1", "#ignored", "?", "?retry=2#frag"],
        &[
            "/focus",
            "/dir/item?old=1",
            "/dir/item?old=1",
            "/dir/item?old=1",
            "/dir/item?retry=2",
        ],
    );
}

#[test]
fn redirects_outside_configured_authority_or_with_invalid_schemes_are_not_requested() {
    use comandos_cli::next::{Config, run};
    use std::{net::TcpListener, time::Duration};
    let f = Fixture::new();
    f.state("one.json", r#"{"project":"test","status":"waiting"}"#);
    let foreign = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let other_port = foreign.local_addr().unwrap().port();
    assert_ne!(other_port, 4777);
    foreign.set_nonblocking(true).unwrap();
    for location in [
        format!("http://127.0.0.1:{other_port}/foreign"),
        format!("//127.0.0.1:{other_port}/foreign"),
        format!("https://127.0.0.1:{other_port}/foreign"),
        "file:///private/never-opened".into(),
        "javascript:alert(1)".into(),
        "http://[invalid/".into(),
    ] {
        let before = tree(&f.0);
        let response =
            format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n");
        let (port, server) = serve(vec![
            response.into_bytes(),
            b"HTTP/1.1 500 Failed\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ]);
        let native = run(
            &Config {
                home: f.home(),
                port,
                timeout: Duration::from_secs(8),
            },
            &[],
        );
        assert_eq!(native.code, 1);
        let calls = server.join().unwrap();
        assert_eq!(
            calls
                .iter()
                .map(|(method, target, _)| (method.as_str(), target.as_str()))
                .collect::<Vec<_>>(),
            vec![("POST", "/focus"), ("POST", "/up")],
            "{location}"
        );
        assert_eq!(
            foreign.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert_eq!(tree(&f.0), before);
    }
}

#[test]
fn binary_dry_reads_mode_authority_and_rejects_stale_guard_and_future_wal() {
    use comandos_store::unified::{self, Mode, Origin};
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let f = Fixture::new();
        f.state(
            "z.json",
            r#"{"project":"legacy","status":"waiting","ts":-99}"#,
        );
        let path = f.home().join(".local/share/comandos/comandos.sqlite3");
        let db = unified::open_unified(&path).unwrap();
        unified::status_put(
            &db,
            "z.json",
            br#"{"project":"db-z","status":"waiting","ts":0}"#,
            1,
            Origin::Import,
        )
        .unwrap();
        unified::status_put(
            &db,
            "a.json",
            br#"{"project":"db-a","status":"waiting","ts":0}"#,
            1,
            Origin::Import,
        )
        .unwrap();
        unified::set_mode(&db, "session-status", mode, "test", 1).unwrap();
        let expected = if matches!(mode, Mode::Unified | Mode::Sealed) {
            b"eleccion: db-a (waiting)\n".as_slice()
        } else {
            b"eleccion: legacy (waiting)\n".as_slice()
        };
        let check = |success: bool| {
            let before = tree(&f.0);
            for (program, prefix) in [
                (PathBuf::from(env!("CARGO_BIN_EXE_comandos")), vec!["next"]),
                (f.0.join("bin/cc-next"), vec![]),
            ] {
                let out = f
                    .command(&program)
                    .args(prefix)
                    .arg("--dry")
                    .output()
                    .unwrap();
                assert_eq!(
                    out.status.success(),
                    success,
                    "{}",
                    String::from_utf8_lossy(&out.stderr)
                );
                if success {
                    assert_eq!(out.stdout, expected);
                } else {
                    assert!(out.stdout.is_empty());
                }
                assert_eq!(tree(&f.0), before);
            }
        };
        check(true);
        db.execute_batch("PRAGMA user_version=999999").unwrap();
        check(false);
        db.execute_batch("PRAGMA user_version=11").unwrap();
        unified::set_mode(&db, "session-status", Mode::Mirror, "test", 1).unwrap();
        let mut guard = path.into_os_string();
        guard.push(".sealed-session-status");
        fs::write(PathBuf::from(guard), b"comandos-state-protocol-2:sealed\n").unwrap();
        check(false);
    }
}

#[test]
fn connection_failure_and_private_header_timeout_never_request_up() {
    use comandos_cli::next::{Config, run};
    use std::{io::Read, net::TcpListener, time::Duration};
    let f = Fixture::new();
    f.state("one.json", r#"{"project":"test","status":"waiting"}"#);
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    assert_ne!(port, 4777);
    drop(listener);
    let before = tree(&f.0);
    let out = run(
        &Config {
            home: f.home(),
            port,
            timeout: Duration::from_millis(100),
        },
        &[],
    );
    assert_eq!(out.code, 1);
    assert_eq!(
        out.stdout,
        "cc-dash no responde: <urlopen error [Errno 111] Connection refused>\n"
    );
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    assert_ne!(port, 4777);
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut request = [0; 2048];
        assert!(stream.read(&mut request).unwrap() > 0);
        std::thread::sleep(Duration::from_millis(250));
        listener.set_nonblocking(true).unwrap();
        assert!(listener.accept().is_err());
    });
    let out = run(
        &Config {
            home: f.home(),
            port,
            timeout: Duration::from_millis(50),
        },
        &[],
    );
    assert_eq!(out.code, 1);
    assert_eq!(out.stdout, "cc-dash no responde: timed out\n");
    assert!(out.stderr.is_empty());
    server.join().unwrap();
    assert_eq!(tree(&f.0), before);
}
