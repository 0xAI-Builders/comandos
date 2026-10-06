use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
static SEQ: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new(installed: &[&str]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "comandos-agents-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        for sub in [
            "home", "data", "config", "cache", "state", "run", "tmp", "bin",
        ] {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(root.join(sub))
                .unwrap();
        }
        for (name, target) in [
            ("readlink", "/usr/bin/readlink"),
            ("dirname", "/usr/bin/dirname"),
            ("grep", "/usr/bin/grep"),
            ("mkdir", "/usr/bin/mkdir"),
            ("touch", "/usr/bin/touch"),
            ("ln", "/usr/bin/ln"),
            ("jq", "/usr/bin/jq"),
            ("mv", "/usr/bin/mv"),
            ("cat", "/usr/bin/cat"),
            ("rm", "/usr/bin/rm"),
            ("python3", "/usr/bin/python3"),
        ] {
            symlink(target, root.join("bin").join(name)).unwrap();
        }
        for name in installed {
            let path = root.join("bin").join(name);
            fs::write(
                &path,
                b"#!/usr/bin/bash\nprintf 'agent executed' >&2\nexit 99\n",
            )
            .unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        symlink(env!("CARGO_BIN_EXE_comandos"), root.join("bin/cc-agents")).unwrap();
        Self(root)
    }
    fn home(&self) -> PathBuf {
        self.0.join("home")
    }
    fn command(&self, program: &Path) -> Command {
        let mut c = Command::new(program);
        c.env_clear()
            .current_dir(&self.0)
            .env("HOME", self.home())
            .env("PATH", self.0.join("bin"))
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("LANG", "C.UTF-8");
        for (key, dir) in [
            ("XDG_DATA_HOME", "data"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_RUNTIME_DIR", "run"),
            ("TMPDIR", "tmp"),
        ] {
            c.env(key, self.0.join(dir));
        }
        c
    }
    fn oracle(&self, args: &[&str]) -> Output {
        let original = Path::new("/home/someguy/codebase/0xJesus/ComandOS/bin/cc-agents");
        self.command(Path::new("/usr/bin/bash"))
            .arg("-c")
            .arg(fs::read_to_string(original).unwrap())
            .arg(original)
            .args(args)
            .output()
            .unwrap()
    }
    fn native(&self, alias: bool, args: &[&str]) -> Output {
        let bin = if alias {
            self.0.join("bin/cc-agents")
        } else {
            PathBuf::from(env!("CARGO_BIN_EXE_comandos"))
        };
        let mut c = self.command(&bin);
        if !alias {
            c.arg("agents");
        }
        c.args(args).output().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn status_and_missing_setup_match_actual_bash_through_command_and_alias() {
    let f = Fixture::new(&[]);
    for args in [
        vec![],
        vec!["status"],
        vec!["unknown", "ignored"],
        vec!["--help"],
        vec!["setup", "ignored"],
    ] {
        let expected = f.oracle(&args);
        for alias in [false, true] {
            let actual = f.native(alias, &args);
            assert_eq!(actual.status.code(), expected.status.code());
            assert_eq!(actual.stdout, expected.stdout);
            assert_eq!(actual.stderr, expected.stderr);
            assert_eq!(fs::read_dir(f.home()).unwrap().count(), 0);
        }
    }
}
#[test]
fn installed_disconnected_status_matches_actual_bash_without_running_agents() {
    let f = Fixture::new(&["codex", "grok", "opencode", "gemini", "agy"]);
    let expected = f.oracle(&[]);
    let actual = f.native(false, &[]);
    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
    assert_eq!(fs::read_dir(f.home()).unwrap().count(), 0);
}
fn seed(f: &Fixture, path: &str, text: &str) {
    let path = f.home().join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
#[test]
fn canonical_setup_matches_original_configs_and_preserves_foreign_hooks() {
    let old = Fixture::new(&["codex", "grok", "opencode", "gemini", "agy"]);
    let new = Fixture::new(&["codex", "grok", "opencode", "gemini", "agy"]);
    for f in [&old, &new] {
        fs::create_dir_all(f.home().join(".local/bin")).unwrap();
        seed(
            f,
            ".codex/config.toml",
            "# comment\nmodel = \"m\"\n[other]\nx = 1\n",
        );
        seed(
            f,
            ".codex/hooks.json",
            r#"{"owner":"á","hooks":{"Stop":[{"hooks":[{"type":"command","command":"foreign","timeout":1}]}]}}"#,
        );
        seed(
            f,
            ".gemini/settings.json",
            r#"{"owner":"á","hooks":{"AfterAgent":[{"hooks":[{"command":"foreign"}]}]}}"#,
        );
        seed(
            f,
            ".gemini/config/hooks.json",
            r#"{"foreign":{"Stop":[{"command":"other"}]}}"#,
        );
        seed(
            f,
            ".gemini/antigravity-cli/settings.json",
            r#"{"theme":"á"}"#,
        );
    }
    let expected = old.oracle(&["setup"]);
    assert_eq!(expected.status.code(), Some(0), "{:?}", expected);
    let actual = new.native(false, &["setup"]);
    assert_eq!(actual.status.code(), Some(0));
    assert_eq!(actual.stderr, b"");
    let normalize = |text: String| {
        text.replace(old.home().to_str().unwrap(), new.home().to_str().unwrap())
            .replace(
                "/home/someguy/codebase/0xJesus/ComandOS/adapters",
                new.home().join(".local/bin").to_str().unwrap(),
            )
    };
    for path in [
        ".codex/config.toml",
        ".codex/hooks.json",
        ".grok/hooks/comandos.json",
        ".gemini/settings.json",
        ".gemini/config/hooks.json",
        ".gemini/antigravity-cli/settings.json",
    ] {
        let expected = normalize(fs::read_to_string(old.home().join(path)).unwrap());
        let actual = fs::read_to_string(new.home().join(path)).unwrap();
        assert_eq!(actual, expected, "{path}");
    }
    let expected = normalize(String::from_utf8(expected.stdout).unwrap())
        .replace(
            "adapters/codex-notify.sh",
            new.home()
                .join(".local/bin/codex-notify.sh")
                .to_str()
                .unwrap(),
        )
        .replace(
            "adapters/codex-hooks.sh",
            new.home()
                .join(".local/bin/codex-hooks.sh")
                .to_str()
                .unwrap(),
        );
    assert_eq!(String::from_utf8(actual.stdout).unwrap(), expected);
    let plugin =
        fs::read_to_string(new.home().join(".config/opencode/plugin/comandos.js")).unwrap();
    assert_eq!(plugin.lines().count(), 3);
    assert!(plugin.contains("comandos"));
    assert!(!plugin.contains("session.idle"));
}
#[test]
fn agy_status_uses_the_actual_setup_path_and_ignores_obsolete_path() {
    let f = Fixture::new(&["agy"]);
    seed(&f, ".gemini/config/hooks.json", "agy-hooks.sh");
    let old = f.oracle(&[]);
    assert!(
        String::from_utf8(old.stdout)
            .unwrap()
            .contains("antigravity (agy): instalado pero SIN conectar")
    );
    let new = f.native(true, &[]);
    assert!(
        String::from_utf8(new.stdout)
            .unwrap()
            .contains("antigravity (agy): conectado")
    );
    fs::remove_file(f.home().join(".gemini/config/hooks.json")).unwrap();
    seed(&f, ".gemini/antigravity-cli/hooks.json", "agy-hooks.sh");
    assert!(
        String::from_utf8(f.native(false, &[]).stdout)
            .unwrap()
            .contains("antigravity (agy): instalado pero SIN conectar")
    );
}
#[test]
fn notify_stays_at_toml_root_even_when_first_line_is_a_section() {
    let old = Fixture::new(&["codex"]);
    let new = Fixture::new(&["codex"]);
    for f in [&old, &new] {
        seed(f, ".codex/config.toml", "[project]\nx = 1\n");
    }
    old.oracle(&["setup"]);
    let wrong: toml::Value = fs::read_to_string(old.home().join(".codex/config.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(wrong.get("notify").is_none());
    let out = new.native(false, &["setup"]);
    assert_eq!(out.status.code(), Some(0));
    let right: toml::Value = fs::read_to_string(new.home().join(".codex/config.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(right.get("notify").is_some());
    assert_eq!(right["project"]["x"].as_integer(), Some(1));
}
#[test]
fn writes_backup_originals_follow_symlinks_preserve_mode_and_are_idempotent() {
    let f = Fixture::new(&["codex", "gemini", "agy"]);
    let target = f.home().join("private-config.toml");
    fs::write(&target, b"# mine\nmodel='m'\n").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
    fs::create_dir(f.home().join(".codex")).unwrap();
    symlink(&target, f.home().join(".codex/config.toml")).unwrap();
    seed(
        &f,
        ".gemini/antigravity-cli/settings.json",
        r#"{"statusLine":null,"keep":true}"#,
    );
    let result = f.native(false, &["setup"]);
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(result.stderr, b"");
    assert!(
        fs::symlink_metadata(f.home().join(".codex/config.toml"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o640
    );
    let backups: Vec<_> = fs::read_dir(f.home().join(".codex"))
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            p.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("config.toml.bak-comandos-")
                .then_some(p)
        })
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read(&backups[0]).unwrap(), b"# mine\nmodel='m'\n");
    assert_eq!(
        fs::metadata(&backups[0]).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(
        fs::read_to_string(f.home().join(".gemini/antigravity-cli/settings.json")).unwrap(),
        r#"{"statusLine":null,"keep":true}"#
    );
    let before = fs::read(&target).unwrap();
    let modified = fs::metadata(&target).unwrap().modified().unwrap();
    let result = f.native(true, &["setup"]);
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(fs::read(target.clone()).unwrap(), before);
    assert_eq!(fs::metadata(target).unwrap().modified().unwrap(), modified);
    assert_eq!(
        fs::read_dir(f.home().join(".codex"))
            .unwrap()
            .filter(|e| e
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("config.toml.bak-comandos-"))
            .count(),
        1
    );
}
#[test]
fn replacing_legacy_plugin_link_never_modifies_the_adapter_source() {
    let f = Fixture::new(&["opencode"]);
    let source = f.0.join("adapters/opencode-comandos.js");
    fs::create_dir(source.parent().unwrap()).unwrap();
    fs::write(&source, b"export const Comandos = old;\n").unwrap();
    let plugin = f.home().join(".config/opencode/plugin/comandos.js");
    fs::create_dir_all(plugin.parent().unwrap()).unwrap();
    symlink(&source, &plugin).unwrap();
    let result = f.native(false, &["setup"]);
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(fs::read(source).unwrap(), b"export const Comandos = old;\n");
    assert!(
        !fs::symlink_metadata(&plugin)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(plugin).unwrap().lines().count(), 3);
}
#[test]
fn malformed_configs_and_foreign_notify_are_preserved_without_backups() {
    let f = Fixture::new(&["codex", "gemini"]);
    seed(&f, ".codex/config.toml", "notify = ['foreign']\n");
    seed(&f, ".codex/hooks.json", "{broken");
    seed(
        &f,
        ".gemini/settings.json",
        r#"{"hooks":{"AfterAgent":"foreign"}}"#,
    );
    let out = f.native(false, &["setup"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stderr, b"");
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("no pude configurar")
    );
    assert_eq!(
        fs::read_to_string(f.home().join(".codex/config.toml")).unwrap(),
        "notify = ['foreign']\n"
    );
    assert_eq!(
        fs::read_to_string(f.home().join(".codex/hooks.json")).unwrap(),
        "{broken"
    );
    assert_eq!(
        fs::read_to_string(f.home().join(".gemini/settings.json")).unwrap(),
        r#"{"hooks":{"AfterAgent":"foreign"}}"#
    );
    assert_eq!(fs::read_dir(f.home().join(".codex")).unwrap().count(), 2);
}
#[test]
fn status_is_read_only_for_connected_symlink_configs() {
    let f = Fixture::new(&["codex", "grok", "opencode", "gemini"]);
    for (p, text) in [
        (".codex/config.toml", "codex-notify.sh"),
        (".codex/hooks.json", "codex-hooks.sh"),
        (".grok/hooks/comandos.json", "StopFailure StopCancelled"),
        (".config/opencode/plugin/comandos.js", "comandos"),
        (".gemini/settings.json", "gemini-hooks.sh"),
    ] {
        seed(&f, p, text);
    }
    let expected = f.oracle(&["status"]);
    let paths: [&str; 5] = [
        ".codex/config.toml",
        ".codex/hooks.json",
        ".grok/hooks/comandos.json",
        ".config/opencode/plugin/comandos.js",
        ".gemini/settings.json",
    ];
    let before: Vec<_> = paths
        .iter()
        .map(|p| {
            use std::os::unix::fs::MetadataExt;
            let p = f.home().join(p);
            let m = fs::metadata(&p).unwrap();
            (
                fs::read(p).unwrap(),
                m.ino(),
                m.mode(),
                m.mtime(),
                m.mtime_nsec(),
            )
        })
        .collect();
    let result = f.native(false, &["status"]);
    assert_eq!(result.stdout, expected.stdout);
    assert_eq!(result.stderr, expected.stderr);
    let after: Vec<_> = paths
        .iter()
        .map(|p| {
            use std::os::unix::fs::MetadataExt;
            let p = f.home().join(p);
            let m = fs::metadata(&p).unwrap();
            (
                fs::read(p).unwrap(),
                m.ino(),
                m.mode(),
                m.mtime(),
                m.mtime_nsec(),
            )
        })
        .collect();
    assert_eq!(before, after);
    assert!(!f.home().join(".local").exists());
}
fn node(f: &Fixture) -> Command {
    let binary = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("node"))
        .find(|p| p.is_file())
        .expect("Node oracle required");
    f.command(&binary)
}
#[test]
fn generated_shim_uses_the_exact_native_transport_resource_and_fake_sdk() {
    let f = Fixture::new(&["opencode"]);
    assert_eq!(f.native(false, &["setup"]).status.code(), Some(0));
    let helper = f.home().join(".local/share/comandos/opencode-bridge.mjs");
    use sha2::{Digest, Sha256};
    let resource_hash = format!(
        "{:x}",
        Sha256::digest(include_bytes!("../src/opencode_bridge.mjs"))
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(fs::read(&helper).unwrap())),
        resource_hash
    );
    assert_eq!(
        fs::read(&helper).unwrap(),
        include_bytes!("../src/opencode_bridge.mjs")
    );
    let binary = f.home().join(".local/bin/comandos");
    fs::create_dir_all(binary.parent().unwrap()).unwrap();
    fs::write(&binary,r#"#!/usr/bin/python3 -u
import json,sys,os
with open(os.environ['HOME']+'/trace.jsonl','w') as trace:
 trace.write(json.dumps({'argv':sys.argv[1:]})+'\n');trace.flush()
 for line in sys.stdin:
  value=json.loads(line);trace.write(json.dumps(value)+'\n');trace.flush()
  request=value['event'].get('_request');print(json.dumps(request),flush=True)
  if request is not None:
   response=json.loads(sys.stdin.readline());trace.write(json.dumps({'reply':response})+'\n');trace.flush()
  print('null',flush=True)
"#).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
    let plugin = f.home().join(".config/opencode/plugin/comandos.js");
    let uri = url::Url::from_file_path(plugin).unwrap();
    let run = format!(
        r#"import {{ Comandos }} from {uri};const calls=[];const plugin=await Comandos({{directory:'/private/work',client:{{session:{{get:async(r)=>{{calls.push(r);return {{data:{{id:r.path.id}}}}}}}}}}}});await Promise.all([plugin.event({{event:{{type:'anything',_request:{{path:{{id:'a'}}}}}}}}),plugin.event({{event:{{type:'noise'}}}}),plugin.event({{event:{{type:'anything',_request:{{path:{{id:'b'}}}}}}}})]);console.log(JSON.stringify(calls));"#,
        uri = serde_json::to_string(uri.as_str()).unwrap()
    );
    fs::write(f.0.join("run.mjs"), run).unwrap();
    let out = node(&f)
        .arg("--experimental-default-type=module")
        .arg(f.0.join("run.mjs"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stderr, b"");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap(),
        serde_json::json!([{"path":{"id":"a"}},{"path":{"id":"b"}}])
    );
    let trace: Vec<serde_json::Value> = fs::read_to_string(f.home().join("trace.jsonl"))
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(trace,serde_json::json!([{"argv":["hook","opencode","--bridge"]},{"directory":"/private/work","event":{"type":"anything","_request":{"path":{"id":"a"}}}},{"reply":{"data":{"id":"a"}}},{"directory":"/private/work","event":{"type":"noise"}},{"directory":"/private/work","event":{"type":"anything","_request":{"path":{"id":"b"}}}},{"reply":{"data":{"id":"b"}}}]).as_array().unwrap().clone());
}
#[test]
fn transport_handles_sdk_rejection_timeout_eof_and_abort_without_noise() {
    for mode in ["rejection", "sdk-timeout", "eof", "native-timeout", "abort"] {
        let f = Fixture::new(&[]);
        let helper = f.0.join("transport.mjs");
        fs::write(&helper, include_bytes!("../src/opencode_bridge.mjs")).unwrap();
        let binary = f.0.join("actor");
        let source = match mode {
            "eof" => "#!/usr/bin/python3 -u\nimport sys\nsys.stdin.readline()\n",
            "native-timeout" | "abort" => {
                "#!/usr/bin/python3 -u\nimport sys,os\nsys.stdin.readline()\nfor line in sys.stdin: pass\nopen(os.environ['HOME']+'/eof','w').write('closed')\n"
            }
            _ => {
                "#!/usr/bin/python3 -u\nimport sys,os\nsys.stdin.readline()\nprint('{\"path\":{\"id\":\"x\"}}',flush=True)\nopen(os.environ['HOME']+'/reply','w').write(sys.stdin.readline())\nprint('null',flush=True)\n"
            }
        };
        fs::write(&binary, source).unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
        let sdk = if mode == "rejection" {
            "async()=>{throw new Error('private fixture')}"
        } else {
            "()=>new Promise(()=>{})"
        };
        let run = format!(
            "import {{bridge}} from './transport.mjs';const controller=new AbortController();const plugin=bridge({{directory:'/private/work',client:{{session:{{get:{sdk}}}}}}},{binary},{{signal:controller.signal,timeout:500}});const promise=plugin.event({{event:{{type:'test'}}}});{abort}await promise;console.log('done');",
            binary = serde_json::to_string(binary.to_str().unwrap()).unwrap(),
            abort = if mode == "abort" {
                "setTimeout(()=>controller.abort(),100);"
            } else {
                ""
            }
        );
        fs::write(f.0.join("run.mjs"), run).unwrap();
        let out = node(&f).arg(f.0.join("run.mjs")).output().unwrap();
        assert!(
            out.status.success(),
            "{mode}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stderr, b"", "{mode}");
        assert_eq!(out.stdout, b"done\n", "{mode}");
        if matches!(mode, "rejection" | "sdk-timeout") {
            assert_eq!(
                fs::read(f.home().join("reply")).unwrap(),
                b"null\n",
                "{mode}"
            );
        }
    }
}
#[test]
fn concurrent_setup_serializes_configs_and_backups_then_preserves_the_full_tree() {
    use std::{collections::BTreeMap, os::unix::fs::MetadataExt, process::Stdio};
    fn tree(root: &Path) -> BTreeMap<PathBuf, (Vec<u8>, u64, u32, i64, i64)> {
        fn visit(path: &Path, map: &mut BTreeMap<PathBuf, (Vec<u8>, u64, u32, i64, i64)>) {
            let m = fs::symlink_metadata(path).unwrap();
            let body = if m.file_type().is_symlink() {
                fs::read_link(path)
                    .unwrap()
                    .as_os_str()
                    .as_encoded_bytes()
                    .to_vec()
            } else if m.is_file() {
                fs::read(path).unwrap()
            } else {
                Vec::new()
            };
            map.insert(
                path.to_path_buf(),
                (body, m.ino(), m.mode(), m.mtime(), m.mtime_nsec()),
            );
            if m.is_dir() {
                for e in fs::read_dir(path).unwrap() {
                    visit(&e.unwrap().path(), map)
                }
            }
        }
        let mut map = BTreeMap::new();
        visit(root, &mut map);
        map
    }
    let f = Fixture::new(&["codex", "grok", "gemini", "agy", "opencode"]);
    seed(
        &f,
        ".gemini/config/hooks.json",
        r#"{"comandos":{"PreInvocation":[{"type":"command","command":"foreign","timeout":7}],"other":[1]},"keep":true}"#,
    );
    let mut a = f.command(Path::new(env!("CARGO_BIN_EXE_comandos")));
    a.args(["agents", "setup"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut b = f.command(Path::new(env!("CARGO_BIN_EXE_comandos")));
    b.args(["agents", "setup"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let a = a.spawn().unwrap();
    let b = b.spawn().unwrap();
    for result in [a.wait_with_output().unwrap(), b.wait_with_output().unwrap()] {
        assert_eq!(result.status.code(), Some(0));
        assert_eq!(result.stderr, b"");
    }
    let data: serde_json::Value =
        serde_json::from_slice(&fs::read(f.home().join(".codex/hooks.json")).unwrap()).unwrap();
    for event in ["UserPromptSubmit", "PermissionRequest", "Stop"] {
        assert_eq!(data["hooks"][event].as_array().unwrap().len(), 1);
    }
    let data: serde_json::Value =
        serde_json::from_slice(&fs::read(f.home().join(".gemini/config/hooks.json")).unwrap())
            .unwrap();
    assert_eq!(data["comandos"]["PreInvocation"][0]["command"], "foreign");
    assert_eq!(data["comandos"]["other"], serde_json::json!([1]));
    let before = tree(&f.home());
    let out = f.native(false, &["setup"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(before, tree(&f.home()));
    for path in [
        ".codex/config.toml",
        ".codex/hooks.json",
        ".grok/hooks/comandos.json",
        ".gemini/settings.json",
        ".gemini/config/hooks.json",
        ".gemini/antigravity-cli/settings.json",
        ".config/opencode/plugin/comandos.js",
        ".local/share/comandos/opencode-bridge.mjs",
    ] {
        let p = f.home().join(path);
        let prefix = format!("{}.bak-comandos-", p.file_name().unwrap().to_string_lossy());
        assert_eq!(
            fs::read_dir(p.parent().unwrap())
                .unwrap()
                .filter(|e| e
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&prefix))
                .count(),
            1,
            "{path}"
        );
    }
}
#[test]
fn migration_rewrites_known_legacy_commands_without_duplicating_hooks() {
    let f = Fixture::new(&["codex", "grok", "gemini", "agy"]);
    let ad = "/private/ComandOS/adapters";
    seed(
        &f,
        ".codex/config.toml",
        &format!("notify = [\"{ad}/codex-notify.sh\"]\n"),
    );
    seed(
        &f,
        ".codex/hooks.json",
        &format!(
            r#"{{"hooks":{{"Stop":[{{"hooks":[{{"type":"command","command":"{ad}/codex-hooks.sh","timeout":19}}]}}]}}}}"#
        ),
    );
    seed(
        &f,
        ".grok/hooks/comandos.json",
        r#"{"hooks":{"StopFailure":[{"hooks":[{"type":"command","command":"~/.claude/hooks/cc-notify.sh","timeout":19}]}]}}"#,
    );
    seed(
        &f,
        ".gemini/settings.json",
        &format!(
            r#"{{"hooks":{{"AfterAgent":[{{"hooks":[{{"type":"command","command":"CC_AGENT=gemini {ad}/gemini-hooks.sh","keep":true}}]}}]}}}}"#
        ),
    );
    seed(
        &f,
        ".gemini/config/hooks.json",
        &format!(
            r#"{{"comandos":{{"Stop":[{{"type":"command","command":"{ad}/agy-hooks.sh done","timeout":19}}]}}}}"#
        ),
    );
    let out = f.native(false, &["setup"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stderr, b"");
    for (path, event, wrapped, name, suffix) in [
        (".codex/hooks.json", "Stop", true, "codex-hooks.sh", ""),
        (
            ".grok/hooks/comandos.json",
            "StopFailure",
            true,
            "cc-notify.sh",
            "",
        ),
        (
            ".gemini/settings.json",
            "AfterAgent",
            true,
            "gemini-hooks.sh",
            "",
        ),
        (
            ".gemini/config/hooks.json",
            "Stop",
            false,
            "agy-hooks.sh",
            " done",
        ),
    ] {
        let data: serde_json::Value =
            serde_json::from_slice(&fs::read(f.home().join(path)).unwrap()).unwrap();
        let list = &data[if wrapped { "hooks" } else { "comandos" }][event];
        assert_eq!(list.as_array().unwrap().len(), 1, "{path}");
        let hook = if wrapped {
            &list[0]["hooks"][0]
        } else {
            &list[0]
        };
        let command = hook["command"].as_str().unwrap();
        assert!(
            command.contains(
                f.home()
                    .join(if name == "cc-notify.sh" {
                        ".claude/hooks"
                    } else {
                        ".local/bin"
                    })
                    .to_str()
                    .unwrap()
            ),
            "{path}: {command}"
        );
        assert!(command.ends_with(&format!("{name}{suffix}")));
    }
}
#[test]
fn shell_hook_paths_are_quoted_for_home_with_spaces_and_apostrophes() {
    let mut f = Fixture::new(&["codex", "grok", "gemini", "agy"]);
    let changed = f.0.with_file_name(format!(
        "{} space'quote",
        f.0.file_name().unwrap().to_string_lossy()
    ));
    fs::rename(&f.0, &changed).unwrap();
    f.0 = changed;
    assert_eq!(f.native(false, &["setup"]).status.code(), Some(0));
    let bin = f.home().join(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    for name in [
        "codex-hooks.sh",
        "cc-notify.sh",
        "gemini-hooks.sh",
        "agy-hooks.sh",
        "agy-statusline.py",
    ] {
        let path = if name == "cc-notify.sh" {
            f.home().join(".claude/hooks/cc-notify.sh")
        } else {
            bin.join(name)
        };
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"#!/usr/bin/bash\nprintf 'private hook'\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    for (path, key, event, wrapper) in [
        (".codex/hooks.json", "hooks", "Stop", true),
        (".grok/hooks/comandos.json", "hooks", "StopFailure", true),
        (".gemini/settings.json", "hooks", "AfterAgent", true),
        (".gemini/config/hooks.json", "comandos", "Stop", false),
    ] {
        let data: serde_json::Value =
            serde_json::from_slice(&fs::read(f.home().join(path)).unwrap()).unwrap();
        let hook = if wrapper {
            &data[key][event][0]["hooks"][0]
        } else {
            &data[key][event][0]
        };
        let out = f
            .command(Path::new("/usr/bin/bash"))
            .arg("-c")
            .arg(hook["command"].as_str().unwrap())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{path}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, b"private hook");
    }
    let before = fs::read(f.home().join(".codex/hooks.json")).unwrap();
    assert_eq!(f.native(false, &["setup"]).status.code(), Some(0));
    assert_eq!(
        before,
        fs::read(f.home().join(".codex/hooks.json")).unwrap()
    );
}
#[test]
fn status_matches_original_grep_markers_in_non_utf8_files() {
    let f = Fixture::new(&["codex"]);
    fs::create_dir(f.home().join(".codex")).unwrap();
    fs::write(f.home().join(".codex/config.toml"), b"\xffcodex-notify.sh").unwrap();
    fs::write(f.home().join(".codex/hooks.json"), b"codex-hooksXsh\xff").unwrap();
    let expected = f.oracle(&[]);
    let actual = f.native(false, &[]);
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stderr, expected.stderr);
}
