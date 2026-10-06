//! Private HOME, process tree and fake tools; no personal account or daemon.
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "codex-c5-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        for d in [
            "home", "config", "data", "state", "cache", "runtime", "tmp", "bin", "proc", "lib",
        ] {
            fs::create_dir(root.join(d)).unwrap();
            fs::set_permissions(root.join(d), fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self { root }
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_comandos"));
        c.arg("codex")
            .args(args)
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("PATH", self.root.join("bin"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("XDG_RUNTIME_DIR", self.root.join("runtime"))
            .env("TMPDIR", self.root.join("tmp"))
            .env("TMP", self.root.join("tmp"))
            .env("TEMP", self.root.join("tmp"));
        c
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }
    fn policy(&self, args: &[&str]) -> Output {
        let source = self.root.join("lib/policy.py");
        fs::write(&source, include_bytes!("../../../lib/codex_yolo_policy.py")).unwrap();
        let mut c = Command::new("/usr/bin/python3");
        c.env_clear();
        c.arg("-I").arg("-c").arg("import importlib.util,json,sys\ns=importlib.util.spec_from_file_location('policy',sys.argv[1]);m=importlib.util.module_from_spec(s);s.loader.exec_module(m)\ntry: print(json.dumps(m.normalize_args(sys.argv[2:]),ensure_ascii=False))\nexcept ValueError as e: print(str(e),file=sys.stderr);sys.exit(2)").arg(source).args(args);
        for (k, v) in self.command(&[]).get_envs() {
            if let Some(v) = v {
                c.env(k, v);
            }
        }
        c.output().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn ok(o: &Output) {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}
#[test]
fn policy_matches_original_explicit_yolo_chunks_remote_and_literal_prompt() {
    let f = Fixture::new();
    for args in [
        vec!["resume", "exact"],
        vec!["--", "--yolo"],
        vec!["-m", "--yolo"],
        vec![
            "--yolo",
            "-s",
            "workspace-write",
            "--config=permissions.foo=true",
            "-c",
            "model_reasoning_effort=\"ultra\"",
            "resume",
            "exact",
            "continua",
        ],
        vec!["--model", "resume", "--yolo", "--", "fork"],
        vec!["--remote", "unix://fixture", "--yolo"],
        vec!["--yolo", "fork", "exact", "literal 雪 '& $()"],
        vec![
            "--dangerously-bypass-approvals-and-sandbox",
            "--no-daemon",
            "resume",
            "exact",
        ],
    ] {
        let expected = f.policy(&args);
        let mut argv = vec!["yolo-policy", "--"];
        argv.extend(args);
        let actual = f.run(&argv);
        assert_eq!(actual.status.code(), expected.status.code());
        assert_eq!(actual.stdout, expected.stdout);
        assert_eq!(actual.stderr, expected.stderr);
    }
}
#[test]
fn all_codex_entrypoints_have_help_without_tool_or_home_mutation() {
    let f = Fixture::new();
    for name in [
        "full-access",
        "thread-release",
        "yolo-install",
        "yolo-policy",
    ] {
        let o = f.run(&[name, "--help"]);
        ok(&o);
        assert!(String::from_utf8_lossy(&o.stdout).contains(name));
    }
    let mut version = Command::new(env!("CARGO_BIN_EXE_comandos"));
    version.env_clear().arg("--version");
    for (key, value) in f.command(&[]).get_envs() {
        if let Some(value) = value {
            version.env(key, value);
        }
    }
    version.env("HOME", f.root.join("missing-home"));
    ok(&version.output().unwrap());
    assert!(!f.root.join("missing-home").exists());
    assert!(fs::read_dir(f.root.join("home")).unwrap().next().is_none());
}
#[test]
fn missing_original_launcher_is_rejected_without_installing_or_contacting_tmux() {
    let f = Fixture::new();
    let o = f.run(&["yolo-install"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("ejecutable de Codex"));
    assert!(fs::read_dir(f.root.join("home")).unwrap().next().is_none());
}
#[test]
fn policy_is_idempotent_and_does_not_change_nonexplicit_permissions() {
    let f = Fixture::new();
    let o = f.run(&[
        "yolo-policy",
        "--",
        "--sandbox",
        "read-only",
        "-a",
        "on-request",
    ]);
    ok(&o);
    assert_eq!(
        serde_json::from_slice::<Value>(&o.stdout).unwrap(),
        json!(["--sandbox", "read-only", "-a", "on-request"])
    );
    let o = f.run(&["yolo-policy", "--", "--yolo", "resume", "sid"]);
    ok(&o);
    let args: Vec<String> = serde_json::from_slice(&o.stdout).unwrap();
    let mut refs = vec!["yolo-policy", "--"];
    refs.extend(args.iter().map(String::as_str));
    assert_eq!(f.run(&refs).stdout, o.stdout);
}
impl Fixture {
    fn vendor(&self) -> PathBuf {
        let p = self.root.join("bin/vendor");
        fs::write(&p,"#!/usr/bin/python3\nimport json,os,sys\nif sys.argv[1:]==['--help']: print('--no-daemon --dangerously-bypass-approvals-and-sandbox')\nelse: print(json.dumps({'args':sys.argv[1:],'account':os.getenv('CODEX_HOME')},ensure_ascii=False))\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        p
    }
}
#[test]
fn installed_native_launcher_preserves_origin_accounts_symlinks_and_first_backups() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let vendor = f.vendor();
    let original = fs::read(&vendor).unwrap();
    let entry = f.root.join("bin/codex");
    symlink(&vendor, &entry).unwrap();
    let home = f.root.join("home");
    let z = f.root.join("z target ' $(false)");
    fs::write(&z, b"# untouched\nexport OLD=yes\n").unwrap();
    fs::set_permissions(&z, fs::Permissions::from_mode(0o640)).unwrap();
    symlink(&z, home.join(".zshrc")).unwrap();
    let first = f.run(&["yolo-install", "--executable", entry.to_str().unwrap()]);
    ok(&first);
    let metadata: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(metadata["version"], 2);
    assert_eq!(metadata["type"], "comandos-codex-yolo");
    assert_eq!(fs::read(&vendor).unwrap(), original);
    assert_eq!(fs::read_link(home.join(".zshrc")).unwrap(), z);
    assert_eq!(
        fs::metadata(&z).unwrap().permissions().mode() & 0o777,
        0o640
    );
    let d = home.join(".local/share/comandos/codex-yolo");
    assert_eq!(
        fs::metadata(&d).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::read(d.join("zshrc.before")).unwrap(),
        b"# untouched\nexport OLD=yes\n"
    );
    let mut c = Command::new(&entry);
    c.env_clear();
    for (k, v) in f.command(&[]).get_envs() {
        if let Some(v) = v {
            c.env(k, v);
        }
    }
    let account = f.root.join("account literal '");
    c.env("CODEX_HOME", &account).args([
        "resume",
        "sid",
        "--yolo",
        "-a",
        "on-request",
        "literal '$() 雪",
    ]);
    let output = c.output().unwrap();
    ok(&output);
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["account"], account.to_str().unwrap());
    assert_eq!(
        v["args"],
        json!([
            "resume",
            "--no-daemon",
            "--dangerously-bypass-approvals-and-sandbox",
            "sid",
            "literal '$() 雪"
        ])
    );
    fs::write(&z, format!("{}# later\n", fs::read_to_string(&z).unwrap())).unwrap();
    let second = f.run(&["yolo-install"]);
    ok(&second);
    assert_eq!(
        fs::read(d.join("zshrc.before")).unwrap(),
        b"# untouched\nexport OLD=yes\n"
    );
    assert!(fs::read_to_string(&z).unwrap().ends_with("# later\n"));
    assert_eq!(fs::read(&vendor).unwrap(), original);
}
#[test]
fn installer_preflight_foreign_wrapper_malformed_blocks_and_dry_run_do_not_write() {
    let f = Fixture::new();
    let vendor = f.vendor();
    let home = f.root.join("home");
    fs::write(
        home.join(".bashrc"),
        "# BEGIN COMANDOS CODEX YOLO\nmissing end\n",
    )
    .unwrap();
    let o = f.run(&["yolo-install", "--executable", vendor.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(!home.join(".local").exists());
    fs::remove_file(home.join(".bashrc")).unwrap();
    ok(&f.run(&[
        "yolo-install",
        "--executable",
        vendor.to_str().unwrap(),
        "--dry-run",
    ]));
    assert!(!home.join(".local").exists());
    fs::create_dir_all(home.join(".local/bin")).unwrap();
    fs::write(
        home.join(".local/bin/codex"),
        "foreign owned user launcher\n",
    )
    .unwrap();
    let o = f.run(&["yolo-install", "--executable", vendor.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        fs::read(home.join(".local/bin/codex")).unwrap(),
        b"foreign owned user launcher\n"
    );
    assert!(!home.join(".local/share").exists());
}
#[test]
fn installed_launcher_rejects_tamper_remote_and_unknown_manifest_before_vendor() {
    let f = Fixture::new();
    let vendor = f.vendor();
    ok(&f.run(&["yolo-install", "--executable", vendor.to_str().unwrap()]));
    let wrapper = f.root.join("home/.local/bin/codex");
    let manifest = f
        .root
        .join("home/.local/share/comandos/codex-yolo/launcher.json");
    let mut c = Command::new(&wrapper);
    c.env_clear();
    for (k, v) in f.command(&[]).get_envs() {
        if let Some(v) = v {
            c.env(k, v);
        }
    }
    c.args(["--yolo", "--remote=fixture"]);
    let o = c.output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
    let mut m: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    m["sha256"] = json!("0".repeat(64));
    fs::write(&manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let o = Command::new(&wrapper)
        .arg("resume")
        .env_clear()
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
    let o = f.run(&["yolo-install"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("hash"));
}
#[test]
fn migrate_owned_legacy_launcher_keeps_first_backup_and_raw_original_link() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let vendor = f.vendor();
    let entry = f.root.join("bin/codex");
    symlink(&vendor, &entry).unwrap();
    let home = f.root.join("home");
    let dir = home.join(".local/share/comandos/codex-yolo");
    fs::write(home.join(".zshrc"), "first original\n").unwrap();
    fs::write(
        f.root.join("lib/codex_yolo_install.py"),
        include_bytes!("../../../lib/codex_yolo_install.py"),
    )
    .unwrap();
    fs::write(
        f.root.join("lib/codex_yolo_policy.py"),
        include_bytes!("../../../lib/codex_yolo_policy.py"),
    )
    .unwrap();
    let mut c = Command::new("/usr/bin/python3");
    c.env_clear();
    for (k, v) in f.command(&[]).get_envs() {
        if let Some(v) = v {
            c.env(k, v);
        }
    }
    c.args(["-I","-c","import importlib.util,sys,json; s=importlib.util.spec_from_file_location('installer',sys.argv[1]);m=importlib.util.module_from_spec(s);s.loader.exec_module(m);print(json.dumps(m.install(sys.argv[2],sys.argv[3])))"])
      .arg(f.root.join("lib/codex_yolo_install.py")).arg(&home).arg(&entry);
    let legacy = c.output().unwrap();
    ok(&legacy);
    let previous: Value = serde_json::from_slice(&legacy.stdout).unwrap();
    assert_eq!(previous["version"], 1);
    let old_vendor = fs::read(&vendor).unwrap();
    let o = f.run(&["yolo-install"]);
    ok(&o);
    let m: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(m["version"], 2);
    assert_eq!(m["originalLink"], previous["originalLink"]);
    assert_eq!(m["original"], previous["original"]);
    assert_eq!(m["launcher"], previous["launcher"]);
    assert_eq!(fs::read(&vendor).unwrap(), old_vendor);
    assert_eq!(
        fs::read(dir.join("zshrc.before")).unwrap(),
        b"first original\n"
    );
    assert!(
        home.join(".local/bin/codex")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
#[test]
#[cfg(target_os = "linux")]
fn standalone_free_writer_and_plan_fifo_reject_without_recovery_writes() {
    let f = Fixture::new();
    let p = f.root.join("plan.json");
    let account = f.root.join("account");
    fs::create_dir(&account).unwrap();
    fs::write(&p,serde_json::to_vec(&json!({"sid":"11111111-1111-1111-1111-111111111111","home":account,"binary":f.root.join("bin/codex"),"transcript":account.join("sessions/thread.jsonl")})).unwrap()).unwrap();
    let o = f.run(&["thread-release", "--plan", p.to_str().unwrap()]);
    ok(&o);
    assert!(!f.root.join("state/comandos").exists());
    assert!(!f.root.join("runtime/comandos").exists());
    fs::remove_file(&p).unwrap();
    nix::unistd::mkfifo(
        &p,
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .unwrap();
    let o = f.run(&["thread-release", "--plan", p.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(!f.root.join("state/comandos").exists());
}
#[test]
fn shell_path_with_spaces_apostrophe_and_substitution_stays_literal() {
    let f = Fixture::new();
    let vendor = f.vendor();
    let home = f.root.join("home literal ' $(touch escaped)");
    fs::create_dir(&home).unwrap();
    fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
    ok(&f.run(&[
        "yolo-install",
        "--home",
        home.to_str().unwrap(),
        "--executable",
        vendor.to_str().unwrap(),
    ]));
    let mut bash = Command::new("/bin/bash");
    bash.env_clear();
    for (k, v) in f.command(&[]).get_envs() {
        if let Some(v) = v {
            bash.env(k, v);
        }
    }
    bash.args([
        "--noprofile",
        "--norc",
        "-c",
        "source \"$1\"; printf '%s' \"$PATH\"",
        "fixture",
    ])
    .arg(home.join(".bashrc"))
    .current_dir(&f.root);
    let o = bash.output().unwrap();
    ok(&o);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        format!(
            "{}:{}",
            home.join(".local/bin").display(),
            f.root.join("bin").display()
        )
    );
    assert!(!f.root.join("escaped").exists());
}
#[test]
fn cancellation_during_vendor_help_reaps_owned_child_before_installing() {
    use std::{
        io::{ErrorKind, Read},
        os::unix::{fs::DirBuilderExt, net::UnixListener},
        process::Stdio,
        time::{Duration, Instant},
    };
    let f = Fixture::new();
    // A short private socket path also fits Darwin's sockaddr_un limit when
    // the harness supplies a long TMPDIR. This channel belongs to this exact
    // vendor instance; EOF cannot be confused with a recycled PID or /proc.
    let channel = PathBuf::from("/tmp").join(format!("codex-cancel-owned-{}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&channel).unwrap();
    struct Channel(PathBuf);
    impl Drop for Channel {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _channel = Channel(channel.clone());
    let socket = channel.join("ready.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let vendor = f.root.join("bin/stalled-vendor");
    fs::write(&vendor,format!("#!/usr/bin/python3\nimport socket\ns=socket.socket(socket.AF_UNIX);s.connect({:?});s.sendall(b'R');s.recv(1)\n",socket.to_str().unwrap())).unwrap();
    fs::set_permissions(&vendor, fs::Permissions::from_mode(0o700)).unwrap();
    let mut child = f
        .command(&["yolo-install", "--executable", vendor.to_str().unwrap()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Readiness covers interpreter startup and uses the production help
    // deadline (15 s). Cancellation latency still has the original 2 s bound.
    let startup_end = Instant::now() + Duration::from_secs(15);
    let mut connection = loop {
        match listener.accept() {
            Ok((connection, _)) => break connection,
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => panic!("owned readiness channel: {e}"),
        }
        if child.try_wait().unwrap().is_some() {
            let o = child.wait_with_output().unwrap();
            panic!(
                "CLI exited before vendor readiness: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }
        if Instant::now() >= startup_end {
            nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(child.id() as i32),
                nix::sys::signal::Signal::SIGTERM,
            )
            .unwrap();
            let o = child.wait_with_output().unwrap();
            panic!(
                "vendor readiness exceeded production deadline: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    connection
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut ready = [0];
    assert_eq!(connection.read(&mut ready).unwrap(), 1);
    assert_eq!(ready, [b'R']);
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(child.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() && start.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(5));
    }
    if start.elapsed() >= Duration::from_secs(2) {
        let _ = child.kill();
        let _ = child.wait();
        panic!("cancellation exceeded unchanged two-second deadline");
    }
    let o = child.wait_with_output().unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("cancelado"));
    assert_eq!(
        connection.read(&mut ready).unwrap(),
        0,
        "owned vendor still holds readiness channel"
    );
    assert!(!f.root.join("home/.local").exists());
}
#[test]
#[cfg(target_os = "linux")]
fn completed_help_cleans_descendant_with_closed_pipes_before_reaping_leader() {
    use std::time::{Duration, Instant};
    let f = Fixture::new();
    let vendor = f.root.join("bin/forking-vendor");
    let pidfile = f.root.join("owned-descendant");
    let descendant = format!(
        "import os,time; p=os.getpid(); stat=open('/proc/'+str(p)+'/stat').read().rsplit(')',1)[1].split(); open({:?},'w').write(str(p)+' '+stat[19]); time.sleep(60)",
        pidfile.to_str().unwrap()
    );
    fs::write(&vendor,format!("#!/usr/bin/python3\nimport subprocess,sys,time,os\nsubprocess.Popen([sys.executable,'-c',{:?}],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)\nwhile not os.path.exists({:?}):time.sleep(.002)\nprint('--no-daemon --dangerously-bypass-approvals-and-sandbox')\n",descendant,pidfile.to_str().unwrap())).unwrap();
    fs::set_permissions(&vendor, fs::Permissions::from_mode(0o700)).unwrap();
    let o = f.run(&[
        "yolo-install",
        "--executable",
        vendor.to_str().unwrap(),
        "--dry-run",
    ]);
    ok(&o);
    let identity = fs::read_to_string(&pidfile).unwrap();
    let fields = identity.split_whitespace().collect::<Vec<_>>();
    let pid = fields[0].parse::<i32>().unwrap();
    let stat = PathBuf::from("/proc").join(pid.to_string()).join("stat");
    let is_alive = || {
        fs::read_to_string(&stat).is_ok_and(|s| {
            let f = s
                .rsplit_once(')')
                .unwrap()
                .1
                .split_whitespace()
                .collect::<Vec<_>>();
            f[19] == fields[1] && f[0] != "Z"
        })
    };
    let deadline = Instant::now() + Duration::from_millis(500);
    while is_alive() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let leaked = is_alive();
    if leaked {
        let p = rustix::process::Pid::from_raw(pid).unwrap();
        let fd = rustix::process::pidfd_open(p, rustix::process::PidfdFlags::empty()).unwrap();
        if is_alive() {
            rustix::process::pidfd_send_signal(fd, rustix::process::Signal::KILL).unwrap();
        }
    }
    assert!(!leaked, "owned closed-pipe descendant left alive");
    assert!(!f.root.join("home/.local").exists());
}
