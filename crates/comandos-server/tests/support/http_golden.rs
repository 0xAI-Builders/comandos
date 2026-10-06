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
    original: Option<Oracle>,
    family: &'static str,
    files: Vec<&'static str>,
    aliases: Vec<(String, PathBuf)>,
}
impl<'a> FrozenHttp<'a> {
    pub async fn new(home: &'a TestHome, family: &'static str, files: &[&'static str]) -> Self {
        let original = if matches!(
            std::env::var("COMANDOS_ORACLE").as_deref(),
            Ok("record" | "check")
        ) {
            let reference = frozen::reference(&home.root).unwrap();
            let opts = OracleOpts {
                python_prelude: format!(
                    "dash.time.time = lambda: {}\ndash.motor_queue_resume = lambda: None\ndash.start_pomodoro_scheduler = lambda: None",
                    NOW_MS / 1000
                ),
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
            original,
            family,
            files: files.to_vec(),
            aliases: Vec::new(),
        }
    }
    /// Only volatile fixture values explicitly identified by the caller are aliased.
    pub fn alias(&mut self, token: &str, value: &str) {
        self.aliases.push((token.to_owned(), PathBuf::from(value)));
    }
    pub async fn get(&self, target: &str) -> Wire {
        self.request("GET", target, "", "").await
    }
    pub async fn request(&self, method: &str, target: &str, extra: &str, body: &str) -> Wire {
        let capsule = self.home.root.join(".oracle/http-effects");
        fs::create_dir_all(&capsule).unwrap();
        copy_domain(&self.home.hooks(), &capsule, &self.files);
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
        let output = comandos_oracle::text_with_tree_at(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"), self.family,
            &json!({"source_commit":frozen::SOURCE_COMMIT,"source_sha256":"4e4e26305485b4926bd2c77618a4a68eb8da9ea425825c57a9a0fea6847a6f24", "python":"CPython 3.10.12","clock_ms":NOW_MS,"request":request,"effect_files":self.files}),
            &capsule, &roots, || {
                let original = self.original.as_ref().ok_or("original HTTP unavailable outside record/check")?;
                let mut wire = original_request(original.port, method, target, extra, body)?;
                wire.body = comandos_oracle::normalize(&wire.body, &roots);
                copy_domain(&self.home.hooks(), &capsule, &self.files);
                serde_json::to_string(&json!({"status":wire.status,"headers":wire.headers,"body":wire.body})).map_err(|e| e.to_string())
            }).unwrap();
        // The capsule excludes processes, sockets and logs; only recorded domain files return.
        copy_domain(&capsule, &self.home.hooks(), &self.files);
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
        assert!(!file.is_empty() && !file.contains('/') && *file != "." && *file != "..");
        let source = from.join(file);
        let target = to.join(file);
        match fs::symlink_metadata(&source) {
            Ok(metadata) => {
                assert!(metadata.is_file() && !metadata.file_type().is_symlink());
                fs::write(&target, fs::read(&source).unwrap()).unwrap();
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
