//! Only private owned children and fixtures. No tmux, provider, network or session restart.
use comandos_runtime::extension_launch as launch;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    ffi::OsString,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
fn hash(raw: &[u8]) -> String {
    Sha256::digest(raw)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
struct Fixture {
    home: PathBuf,
    stub: PathBuf,
    alias: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let home =
            std::env::temp_dir().join(comandos_runtime::fresh_id("native-extension-exec").unwrap());
        fs::create_dir(&home).unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
        let stub = home.join("private-program");
        let code = home.join("private-program.c");
        fs::write(&code,r#"#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
int main(void) { FILE *f=fopen(getenv("PROOF_PID"),"w"); if(!f)return 23;fprintf(f,"%ld",(long)getpid());fclose(f);char c;read(0,&c,1);return 0; }
"#).unwrap();
        assert!(
            Command::new("/usr/bin/cc")
                .arg(&code)
                .arg("-o")
                .arg(&stub)
                .status()
                .unwrap()
                .success()
        );
        let native = home.join(".local/share/comandos/bin/comandos");
        let alias = launch::helper_for_home(&home);
        fs::create_dir_all(native.parent().unwrap()).unwrap();
        fs::create_dir_all(alias.parent().unwrap()).unwrap();
        fs::copy(env!("CARGO_BIN_EXE_comandos"), &native).unwrap();
        fs::set_permissions(&native, fs::Permissions::from_mode(0o700)).unwrap();
        symlink(&native, &alias).unwrap();
        assert_eq!(launch::require_helper(&home).unwrap(), alias);
        Self { home, stub, alias }
    }
    fn private(&self, name: &str, v: &Value) -> PathBuf {
        let p = self.home.join(name);
        fs::write(&p, serde_json::to_vec(v).unwrap()).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        p
    }
    fn command(&self, alias: bool) -> Command {
        let mut c = Command::new(if alias {
            &self.alias
        } else {
            Path::new(env!("CARGO_BIN_EXE_comandos"))
        });
        if !alias {
            c.arg("extension-session");
        }
        c.env_clear()
            .env("HOME", &self.home)
            .env("PATH", "/usr/bin:/bin")
            .env("PROOF_PID", self.home.join("pid"))
            .current_dir(&self.home);
        c
    }
    fn wait_ready(&self, child: &mut Child) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while fs::read_to_string(self.home.join("pid")).ok().as_deref()
            != Some(child.id().to_string().as_str())
        {
            assert!(
                child.try_wait().unwrap().is_none(),
                "private executor exited before exec proof"
            );
            assert!(Instant::now() < deadline, "private exec timed out");
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            fs::read_to_string(self.home.join("pid")).unwrap(),
            child.id().to_string(),
            "exec retains the launcher's PID"
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}
struct OwnedChild(Child);
impl OwnedChild {
    fn finish(&mut self) {
        drop(self.0.stdin.take());
        assert!(self.0.wait().unwrap().success());
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
#[cfg(target_os = "linux")]
fn process_env(pid: u32) -> HashMap<OsString, OsString> {
    use std::os::unix::ffi::OsStringExt;
    fs::read(format!("/proc/{pid}/environ"))
        .unwrap()
        .split(|b| *b == 0)
        .filter_map(|s| {
            let i = s.iter().position(|b| *b == b'=')?;
            Some((
                OsString::from_vec(s[..i].to_vec()),
                OsString::from_vec(s[i + 1..].to_vec()),
            ))
        })
        .collect()
}
#[cfg(target_os = "linux")]
#[test]
fn manifest_exec_has_same_pid_exact_argv_private_receipt_and_verifier_compatibility() {
    for harness in ["codex", "claude", "opencode"] {
        let f = Fixture::new();
        let settings = f.private("selected.json", &json!({"skillOverrides":{"demo":"off"}}));
        let args = if harness == "claude" {
            json!(["--settings", settings, "--strict-mcp-config"])
        } else {
            json!(["--literal", "two words", "雪 ' $(never-run)"])
        };
        let env = if harness == "opencode" {
            json!({"OPENCODE_CONFIG_CONTENT":r#"{"permission":{"skill":{"demo":"deny"}}}"#})
        } else {
            json!({"PRIVATE_OVERLAY":"selected"})
        };
        let manifest = f.home.join("manifest.json");
        let bundle = json!({"manifest":manifest,"operationId":"private-test-op","harness":harness,"selection":{},"method":"native"});
        let artifacts = if harness == "claude" {
            json!({settings.to_str().unwrap():hash(&fs::read(&settings).unwrap())})
        } else {
            json!({})
        };
        f.private("manifest.json",&json!({"version":1,"bundle":bundle,"args":args,"env":env,"mounts":[],"artifacts":artifacts,"home":f.home,"codexSelectedSkillCount":0}));
        let original = fs::read(&settings).unwrap();
        let mut c = f.command(harness != "claude");
        c.env("UNMANAGED", "keep").env("DROP", "discard");
        if harness == "opencode" {
            c.env(
                "OPENCODE_CONFIG_CONTENT",
                r#"{"theme":"dark","permission":"allow"}"#,
            );
        }
        c.args(["--manifest"])
            .arg(&manifest)
            .args(["--", "env", "-u", "DROP", "ASSIGNED=two words"])
            .arg(&f.stub);
        if harness == "claude" {
            c.args([
                "--settings",
                r#"{"model":"prior","skillOverrides":{"kept":"on","demo":"on"}}"#,
                "--mcp-config",
                "do-not-read",
                "--disable-slash-commands",
                "--resume",
                "private-id",
            ]);
        }
        c.stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = OwnedChild(c.spawn().unwrap());
        f.wait_ready(&mut child.0);
        let live = process_env(child.0.id());
        assert_eq!(
            live.get(&OsString::from("HOME")),
            Some(&f.home.clone().into_os_string())
        );
        assert_eq!(
            live.get(&OsString::from("UNMANAGED")),
            Some(&OsString::from("keep"))
        );
        assert!(!live.contains_key(&OsString::from("DROP")));
        let receipt_path = PathBuf::from(live.get(&OsString::from(launch::RECEIPT_ENV)).unwrap());
        let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
        assert_eq!(
            fs::metadata(&receipt_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let actual: Vec<String> = fs::read(format!("/proc/{}/cmdline", child.0.id()))
            .unwrap()
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8(s.to_vec()).unwrap())
            .collect();
        assert_eq!(json!(actual), receipt["argv"]);
        assert!(launch::verify_launch(child.0.id(), &bundle).unwrap());
        assert_eq!(launch::launch_from_pid(child.0.id()).unwrap(), Some(bundle));
        if harness == "claude" {
            let files = receipt["artifacts"].as_object().unwrap();
            assert_eq!(files.len(), 1);
            let p = Path::new(files.keys().next().unwrap());
            let got: Value = serde_json::from_slice(&fs::read(p).unwrap()).unwrap();
            assert_eq!(
                got,
                json!({"model":"prior","skillOverrides":{"kept":"on","demo":"off"}})
            );
            assert_eq!(fs::metadata(p).unwrap().permissions().mode() & 0o777, 0o600);
        }
        if harness == "opencode" {
            let merged: Value = serde_json::from_str(
                live.get(&OsString::from("OPENCODE_CONFIG_CONTENT"))
                    .unwrap()
                    .to_str()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(
                merged,
                json!({"theme":"dark","permission":{"*":"allow","skill":{"demo":"deny"}}})
            );
            assert_eq!(receipt["env"]["OPENCODE_PERMISSION"], Value::Null);
        }
        assert_eq!(fs::read(&settings).unwrap(), original);
        child.finish();
    }
}
#[cfg(target_os = "linux")]
#[test]
fn environment_capture_exec_clears_four_keys_preserves_unmanaged_bytes_and_pid() {
    use std::os::unix::ffi::OsStringExt;
    let f = Fixture::new();
    let p = f.private(
        "environment.json",
        &json!({"version":1,"values":{"OPENCODE_PERMISSION":"deny"}}),
    );
    let mut c = f.command(true);
    for key in launch::OPENCODE_ENV_KEYS {
        c.env(key, "old-secret-canary");
    }
    c.env("UNMANAGED_BYTES", OsString::from_vec(vec![0xff, b'x']));
    c.arg("--environment-file")
        .arg(&p)
        .arg("--environment-sha256")
        .arg(hash(&fs::read(&p).unwrap()))
        .arg("--")
        .arg(&f.stub)
        .arg("literal ' 雪")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = OwnedChild(c.spawn().unwrap());
    f.wait_ready(&mut child.0);
    let env = process_env(child.0.id());
    assert_eq!(
        env.get(&OsString::from("OPENCODE_PERMISSION")),
        Some(&OsString::from("deny"))
    );
    for k in [
        "OPENCODE_CONFIG",
        "OPENCODE_CONFIG_CONTENT",
        "OPENCODE_CONFIG_DIR",
    ] {
        assert!(!env.contains_key(&OsString::from(k)));
    }
    assert_eq!(
        env.get(&OsString::from("UNMANAGED_BYTES")),
        Some(&OsString::from_vec(vec![0xff, b'x']))
    );
    child.finish();
}
#[test]
fn tamper_modes_links_home_and_bad_cli_are_rejected_without_exec_or_secret_output() {
    for case in [
        "hash",
        "mode",
        "link",
        "parent",
        "manifest-artifact",
        "home",
        "unknown",
    ] {
        let f = Fixture::new();
        let artifact = f.private("artifact.json", &json!({"secret":"secret-canary"}));
        let p = f.private(
            "environment.json",
            &json!({"version":1,"values":{"OPENCODE_PERMISSION":"secret-canary"}}),
        );
        let digest = hash(&fs::read(&p).unwrap());
        let mut c = f.command(true);
        if case == "manifest-artifact" || case == "home" {
            let manifest = f.home.join("manifest.json");
            let bundle = json!({"manifest":manifest,"operationId":"private-op","harness":"codex"});
            f.private("manifest.json",&json!({"version":1,"bundle":bundle,"home":if case=="home"{Path::new("/incorrect-home")}else{&f.home},"args":[],"env":{},"mounts":[],"artifacts":{artifact.to_str().unwrap():hash(&fs::read(&artifact).unwrap())}}));
            if case == "manifest-artifact" {
                fs::write(&artifact, "changed-secret-canary").unwrap();
            }
            c.arg("--manifest").arg(manifest);
        } else if case == "unknown" {
            c.arg("--secret-canary");
        } else {
            if case == "hash" {
                fs::write(&p, "changed-secret-canary").unwrap();
            }
            if case == "mode" {
                fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
            }
            if case == "parent" {
                fs::set_permissions(&f.home, fs::Permissions::from_mode(0o755)).unwrap();
            }
            if case == "link" {
                fs::rename(&p, f.home.join("real.json")).unwrap();
                symlink(f.home.join("real.json"), &p).unwrap();
            }
            c.arg("--environment-file")
                .arg(&p)
                .arg("--environment-sha256")
                .arg(&digest);
        }
        c.arg("--").arg(&f.stub);
        let out = c.output().unwrap();
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        assert_eq!(
            String::from_utf8(out.stderr).unwrap(),
            format!("{}\n", launch::executor::ERROR)
        );
        assert!(!f.home.join("pid").exists());
        assert!(!fs::read_dir(&f.home).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("receipt-")
        }));
    }
}
#[test]
fn namespace_child_proof_never_changes_parent_target() {
    let f = Fixture::new();
    let source = f.home.join("source");
    let target = f.home.join("target");
    fs::write(&source, b"child").unwrap();
    fs::write(&target, b"parent").unwrap();
    let proof = f.private(
        "proof.json",
        &json!({"mounts":[{"source":source,"target":target}],"target":target,"home":f.home}),
    );
    let out = f
        .command(false)
        .arg("--namespace-proof")
        .arg(proof)
        .output()
        .unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"parent");
    if out.status.success() {
        assert_eq!(out.stdout, b"namespace-proof-ok\n");
        assert!(out.stderr.is_empty());
    } else {
        assert!(out.stdout.is_empty());
        assert_eq!(
            out.stderr,
            format!("{}\n", launch::executor::ERROR).as_bytes()
        );
        eprintln!("private user/mount namespace unavailable: executor refused safely");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn namespace_check_rejects_uncleared_bounding_capabilities() {
    let status = fs::read_to_string("/proc/self/status").unwrap();
    let bounding = status
        .lines()
        .find_map(|line| line.strip_prefix("CapBnd:"))
        .map(|value| u64::from_str_radix(value.trim(), 16).unwrap())
        .unwrap();
    if bounding == 0 {
        eprintln!("test process already has an empty capability bounding set");
        return;
    }
    let f = Fixture::new();
    let target = f.home.join("target");
    fs::write(&target, b"child").unwrap();
    let proof = f.private("check.json", &json!({"target":target,"home":f.home}));
    let out = f
        .command(false)
        .arg("--namespace-check")
        .arg(proof)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "a zero effective set must not admit a nonzero bounding set"
    );
    assert!(out.stdout.is_empty());
    assert_eq!(
        out.stderr,
        format!("{}\n", launch::executor::ERROR).as_bytes()
    );
    assert_eq!(fs::read(target).unwrap(), b"child");
}
