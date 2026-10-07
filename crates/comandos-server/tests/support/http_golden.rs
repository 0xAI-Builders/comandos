//! Persistent original HTTP only in record/check; replay applies recorded domain files.
#![allow(dead_code)]
use super::{
    NOW_MS, TOKEN, TestHome, Wire, frozen,
    oracle::{Oracle, OracleOpts, oracle_with},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    time::Duration,
};

pub struct FrozenHttp<'a> {
    home: &'a TestHome,
    domain: PathBuf,
    prelude: String,
    fakebin_extra: Vec<(String, String)>,
    original: Option<Oracle>,
    family: &'static str,
    files: Vec<&'static str>,
    aliases: Vec<(String, PathBuf)>,
    prime_targets: Vec<String>,
}
impl<'a> FrozenHttp<'a> {
    pub async fn new(home: &'a TestHome, family: &'static str, files: &[&'static str]) -> Self {
        Self::new_inner(home, family, files, home.hooks(), "", &[]).await
    }
    pub async fn new_rooted_with(
        home: &'a TestHome,
        family: &'static str,
        files: &[&'static str],
        prelude: &str,
    ) -> Self {
        Self::new_inner(home, family, files, home.root.clone(), prelude, &[]).await
    }
    pub async fn new_rooted_with_fakebin(
        home: &'a TestHome,
        family: &'static str,
        files: &[&'static str],
        prelude: &str,
        fakebin_extra: &[(String, String)],
    ) -> Self {
        Self::new_inner(
            home,
            family,
            files,
            home.root.clone(),
            prelude,
            fakebin_extra,
        )
        .await
    }
    async fn new_inner(
        home: &'a TestHome,
        family: &'static str,
        files: &[&'static str],
        domain: PathBuf,
        prelude: &str,
        fakebin_extra: &[(String, String)],
    ) -> Self {
        let original = if matches!(
            std::env::var("COMANDOS_ORACLE").as_deref(),
            Ok("record" | "check")
        ) {
            let reference = frozen::reference(&home.root).unwrap();
            let opts = OracleOpts {
                fakebin_extra: fakebin_extra.to_vec(),
                python_prelude: format!(
                    "dash.time.time = lambda: {}\ndash.motor_queue_resume = lambda: None\ndash.start_pomodoro_scheduler = lambda: None",
                    NOW_MS / 1000
                ) + "\n"
                    + prelude,
                extra_env: vec![(
                    "COMANDOS_ORACLE_REFERENCE_ROOT".into(),
                    reference.display().to_string(),
                )],
                ..OracleOpts::default()
            };
            Some(
                oracle_with(home, opts)
                    .await
                    .expect("explicit record/check requires original Python"),
            )
        } else {
            None
        };
        Self {
            home,
            domain,
            prelude: prelude.to_owned(),
            fakebin_extra: fakebin_extra.to_vec(),
            original,
            family,
            files: files.to_vec(),
            aliases: Vec::new(),
            prime_targets: Vec::new(),
        }
    }
    /// A real source listener exists only in explicit record/check.
    /// Concurrency fixtures must supply their own real native actor in replay.
    pub fn source_port(&self) -> Option<u16> {
        self.original.as_ref().map(|original| original.port)
    }
    /// Only volatile fixture values explicitly identified by the caller are aliased.
    pub fn alias(&mut self, token: &str, value: &str) {
        self.aliases.push((token.to_owned(), PathBuf::from(value)));
    }
    /// A response discarded by the original fixture can prime its in-memory
    /// cache in record/check. Require identical domain effects before/after;
    /// replay therefore needs no source process or fabricated effects.
    pub async fn prime_unchanged(&mut self, target: &str) {
        let capsule = self.home.root.join(".oracle/http-prime");
        let roots = [("<HOME>", self.home.root.as_path())];
        copy_domain(&self.domain, &capsule, &self.files);
        let before = comandos_oracle::snapshot_tree(&capsule, &roots).unwrap();
        if let Some(original) = &self.original {
            original_request(original.port, "GET", target, "", "").unwrap();
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        copy_domain(&self.domain, &capsule, &self.files);
        let after = comandos_oracle::snapshot_tree(&capsule, &roots).unwrap();
        assert_eq!(before, after, "original cache prime mutated domain effects");
        self.prime_targets.push(target.to_owned());
    }
    pub async fn get(&self, target: &str) -> Wire {
        self.request("GET", target, "", "").await
    }
    pub async fn request(&self, method: &str, target: &str, extra: &str, body: &str) -> Wire {
        let capsule = self.home.root.join(".oracle/http-effects");
        fs::create_dir_all(&capsule).unwrap();
        copy_domain(&self.domain, &capsule, &self.files);
        let mut roots = vec![("<HOME>", self.home.root.as_path())];
        roots.extend(
            self.aliases
                .iter()
                .map(|(token, path)| (token.as_str(), path.as_path())),
        );
        let request = json!({"method":method,"target":target,"extra":extra,"body":body});
        let request: Value = serde_json::from_slice(&comandos_oracle::normalize(
            &serde_json::to_vec(&request).unwrap(),
            &roots,
        ))
        .unwrap();
        let mut input = json!({"source_commit":frozen::SOURCE_COMMIT,"source_sha256":"4e4e26305485b4926bd2c77618a4a68eb8da9ea425825c57a9a0fea6847a6f24", "python":"CPython 3.10.12","clock_ms":NOW_MS,"request":request,"effect_files":self.files});
        if !self.prelude.is_empty() {
            input["fixture_prelude"] = json!(self.prelude);
        }
        if !self.fakebin_extra.is_empty() {
            input["fixture_fakebin"] = json!(self.fakebin_extra);
        }
        if !self.prime_targets.is_empty() {
            input["fixture_cache_primes"] = json!(self.prime_targets);
        }
        let output = comandos_oracle::text_with_tree_at(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
            self.family,
            &input,
            &capsule,
            &roots,
            || {
                let original = self
                    .original
                    .as_ref()
                    .ok_or("original HTTP unavailable outside record/check")?;
                let mut wire = original_request(original.port, method, target, extra, body)?;
                wire.body = comandos_oracle::normalize(&wire.body, &roots);
                copy_domain(&self.domain, &capsule, &self.files);
                serde_json::to_string(
                    &json!({"status":wire.status,"headers":wire.headers,"body":wire.body}),
                )
                .map_err(|e| e.to_string())
            },
        )
        .unwrap();
        // The capsule excludes processes, sockets and logs; only recorded domain files return.
        if !matches!(
            std::env::var("COMANDOS_ORACLE").as_deref(),
            Ok("record" | "check")
        ) {
            copy_domain(&capsule, &self.domain, &self.files);
        }
        let value: Value = serde_json::from_str(&output).unwrap();
        Wire {
            status: value["status"].as_u64().unwrap().try_into().unwrap(),
            headers: serde_json::from_value(value["headers"].clone()).unwrap(),
            body: comandos_oracle::restore(
                &serde_json::from_value::<Vec<u8>>(value["body"].clone()).unwrap(),
                &roots,
            ),
        }
    }
}
fn copy_domain(from: &Path, to: &Path, files: &[&str]) {
    fs::create_dir_all(to).unwrap();
    for file in files {
        assert!(
            !file.is_empty()
                && Path::new(file)
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_)))
        );
        let source = from.join(file);
        let target = to.join(file);
        match fs::symlink_metadata(&source) {
            Ok(metadata) => {
                assert!(metadata.is_file() && !metadata.file_type().is_symlink());
                fs::create_dir_all(target.parent().unwrap()).unwrap();
                if fs::read(&source).unwrap().starts_with(b"SQLite format 3\0") {
                    // SQLite backup observes WAL and applies the logical image in place;
                    // never copy open database pages or discard live native connections.
                    let connection = rusqlite::Connection::open_with_flags(
                        &source,
                        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                    )
                    .unwrap();
                    let same = target.exists()
                        && comandos_oracle::snapshot_sqlite(&source, &[]).unwrap()
                            == comandos_oracle::snapshot_sqlite(&target, &[]).unwrap();
                    if !same {
                        connection.backup("main", &target, None).unwrap();
                    }
                } else {
                    fs::write(&target, fs::read(&source).unwrap()).unwrap();
                }
                fs::set_permissions(&target, metadata.permissions()).unwrap();
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let _ = fs::remove_file(&target);
            }
            Err(e) => panic!("domain snapshot: {e}"),
        }
    }
}
fn original_request(
    port: u16,
    method: &str,
    target: &str,
    extra: &str,
    body: &str,
) -> Result<Wire, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(40)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(|e| e.to_string())?;
    let request = format!(
        "{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{extra}X-Comandos-Token: {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|e| e.to_string())?;
    Ok(super::parse(&response))
}

/// Same native child confinement as Twin, with no original process required.
pub fn confined_front_options(
    home: &TestHome,
    extra: &[(String, String)],
) -> comandos_server::dash::native::NativeOptions {
    use comandos_server::dash::native::quick::scope_program;
    let fakebin = super::oracle::confined_fakebin(home, extra);
    let mut options = home.options();
    options.tmux.program.path = fakebin.join("tmux");
    options.scope = Some(scope_program(fakebin.join("systemd-run")));
    let mut search = fakebin.clone().into_os_string();
    search.push(":");
    search.push(home.root.join("bin"));
    options.search_path = Some(search);
    options.user_bin_dirs = super::twin::home_bin_dirs();
    let child_env: Vec<(std::ffi::OsString, std::ffi::OsString)> = home
        .confined_env()
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect();
    options.child_env = Some(child_env.clone());
    options.display = Some(None);
    options.ssh = options.program(fakebin.join("ssh"));
    if let Some(scope) = &mut options.scope {
        scope.env_clear = true;
        scope.env = child_env;
    }
    super::assert_private_tmux(&options);
    options
}

/// Source module effects limited to the named hook documents. Native callers
/// still execute their operations; recorded expected effects never include actors.
pub fn dash_files(
    home: &TestHome,
    family: &str,
    files: &[&str],
    code: &str,
    opts: &OracleOpts,
) -> String {
    let domain = home.hooks();
    let capsule = home.root.join(".oracle/module-effects");
    copy_domain(&domain, &capsule, files);
    let roots = [("<HOME>", home.root.as_path())];
    let normalized = comandos_oracle::normalize(code.as_bytes(), &roots);
    let input = json!({"source_commit":frozen::SOURCE_COMMIT,
        "source_sha256":"4e4e26305485b4926bd2c77618a4a68eb8da9ea425825c57a9a0fea6847a6f24",
        "python":"CPython 3.10.12", "code":String::from_utf8(normalized).unwrap(),
        "effect_files":files,"fakebin":opts.fakebin_extra,"fixture_env":opts.extra_env});
    let output = comandos_oracle::text_with_tree_at(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
        family,
        &input,
        &capsule,
        &roots,
        || {
            let stdout = frozen::run_dash_original(home, code, opts)?;
            copy_domain(&domain, &capsule, files);
            Ok(stdout)
        },
    )
    .unwrap();
    if !matches!(
        std::env::var("COMANDOS_ORACLE").as_deref(),
        Ok("record" | "check")
    ) {
        copy_domain(&capsule, &domain, files);
    }
    output
}

/// Standalone immutable script with declared hook-document effects. Fixture
/// metadata covers inputs outside those documents; logs and actors stay native.
pub fn python_files(
    home: &Path,
    family: &str,
    files: &[&str],
    script: &str,
    args: &[&std::ffi::OsStr],
    fixture: &Value,
) -> String {
    let domain = home.join(".claude/hooks");
    let capsule = home.join(".oracle/python-effects");
    copy_domain(&domain, &capsule, files);
    let roots = [("<HOME>", home)];
    let input = json!({"source_commit":frozen::SOURCE_COMMIT,
        "source_sha256":"4e4e26305485b4926bd2c77618a4a68eb8da9ea425825c57a9a0fea6847a6f24",
        "python":"CPython 3.10.12", "script":script,
        "args":args.iter().map(|arg| arg.to_str().expect("UTF-8 fixture argument")).collect::<Vec<_>>(),
        "effect_files":files,"fixture":fixture});
    let input: Value = serde_json::from_slice(&comandos_oracle::normalize(
        &serde_json::to_vec(&input).unwrap(),
        &roots,
    ))
    .unwrap();
    let output = comandos_oracle::text_with_tree_at(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
        family,
        &input,
        &capsule,
        &roots,
        || {
            let stdout = frozen::run_python_original(script, args, home)?;
            copy_domain(&domain, &capsule, files);
            Ok(stdout)
        },
    )
    .unwrap();
    if !matches!(
        std::env::var("COMANDOS_ORACLE").as_deref(),
        Ok("record" | "check")
    ) {
        copy_domain(&capsule, &domain, files);
    }
    output
}
