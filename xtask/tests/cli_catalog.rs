//! Actual, unmodified Python scripts are private test oracles only.
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::Command,
    time::{Duration, SystemTime},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "comandos-catalog-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        for child in [
            "config",
            "home",
            "cache",
            "data",
            "state",
            "runtime",
            "tmp",
            "tools/cli-commands/scraped",
        ] {
            fs::create_dir_all(root.join(child)).unwrap();
        }
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_owned();
        for script in ["build", "builtin_filter", "scrape_prefix", "verify_names"] {
            fs::copy(
                repo.join(format!("tools/cli-commands/{script}.py")),
                root.join(format!("tools/cli-commands/{script}.py")),
            )
            .unwrap();
        }
        Self(root)
    }
    fn python(&self, script: &str, args: &[String]) -> Vec<u8> {
        let result = Command::new("/usr/bin/python3")
            .arg(self.0.join(format!("tools/cli-commands/{script}.py")))
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.0.join("home"))
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .env("XDG_CACHE_HOME", self.0.join("cache"))
            .env("XDG_DATA_HOME", self.0.join("data"))
            .env("XDG_STATE_HOME", self.0.join("state"))
            .env("XDG_RUNTIME_DIR", self.0.join("runtime"))
            .env("TMPDIR", self.0.join("tmp"))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            result.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        result.stdout
    }
    fn build_compare(&self, old: &Value, scraped: &[(&str, Value)]) -> Vec<u8> {
        fs::write(
            self.0.join("config/cli-commands.json"),
            serde_json::to_vec(old).unwrap(),
        )
        .unwrap();
        for (name, value) in scraped {
            fs::write(
                self.0
                    .join(format!("tools/cli-commands/scraped/{name}.json")),
                serde_json::to_vec(value).unwrap(),
            )
            .unwrap();
        }
        let actual = xtask::cli_catalog::build_bytes(&self.0).unwrap();
        self.python("build", &[]);
        let expected = fs::read(self.0.join("config/cli-commands.json")).unwrap();
        assert_eq!(actual, expected);
        actual
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn frozen_checkout_regeneration_is_byte_identical() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let before = fs::read(repo.join("config/cli-commands.json")).unwrap();
    assert_eq!(xtask::cli_catalog::build_bytes(&repo).unwrap(), before);
    assert_eq!(
        fs::read(repo.join("config/cli-commands.json")).unwrap(),
        before
    );
}

#[test]
fn build_preserves_metadata_order_duplicate_ids_aliases_chips_and_python_clean() {
    let fixture = Fixture::new();
    let old = json!({"version":100,"clis":[{"id":"grok","name":"first","groups":[],"launch":"remove"},{"id":"claude","extra":{"n":1.0,"s":"ñ😀"}},{"id":"grok","name":"last","pinnedVersion":"old","verified":false,"tail":"yes","launch":{},"groups":[1]}]});
    let grok = json!({"version":"1.0","commands":[["t","skip"],["m","model (currently x)\n"],["model","direct"],["effort","e"],["misc","kept (currently x\n)"],["zz","x\u{001c}(currently a)\u{001f}"],["aa","x (currently a)\n\n"],["bb","x (currently a) tail"]]});
    let claude = json!({"version":"ñ","commands":[["add-dir","dirs"],["effort","e"],["model","m"],["new","new"],["z","first (currently one) second (currently two)"]]});
    fixture.build_compare(&old, &[("grok", grok), ("claude", claude)]);
}

#[test]
fn builtin_filter_matches_unicode_scalar_prefix_binary_bytes_whitespace_and_duplicates() {
    let fixture = Fixture::new();
    let prefix = "界😀".repeat(20);
    let commands = json!([
        ["a", "\u{001c} desc  built-in\u{001f}"],
        ["b", format!("{prefix}suffix")],
        ["c", "missing"],
        ["d", " built-in"],
        ["a", "desc\n  built-in\n"],
        ["e", "x built-in"],
        ["f", "é\u{0085}\u{00a0}built-in"],
        ["g", "\t\u{001f}"]
    ]);
    let binary = format!("\0desc\0{prefix}\0built-in\0x built-in\0é\0").into_bytes();
    fs::write(
        fixture.0.join("commands.json"),
        serde_json::to_vec(&commands).unwrap(),
    )
    .unwrap();
    fs::write(fixture.0.join("binary"), &binary).unwrap();
    let expected = fixture.python(
        "builtin_filter",
        &[
            fixture.0.join("commands.json").display().to_string(),
            fixture.0.join("binary").display().to_string(),
        ],
    );
    let actual = xtask::cli_catalog::filter_bytes(&commands, &binary).unwrap();
    assert_eq!(actual.as_bytes(), expected);
}

#[test]
fn malformed_catalog_fails_before_writing_input() {
    let fixture = Fixture::new();
    let path = fixture.0.join("config/cli-commands.json");
    fs::write(&path, b"{bad").unwrap();
    assert!(xtask::cli_catalog::build_bytes(&fixture.0).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"{bad");
}

struct Terminal {
    captures: HashMap<String, Vec<String>>,
    counters: HashMap<String, usize>,
    current: String,
    trace: Vec<Value>,
    sleeps: Vec<u64>,
}
impl Terminal {
    fn new(captures: &Value) -> Self {
        Self {
            captures: captures
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        v.as_array()
                            .unwrap()
                            .iter()
                            .map(|x| x.as_str().unwrap().to_string())
                            .collect(),
                    )
                })
                .collect(),
            counters: HashMap::new(),
            current: String::new(),
            trace: Vec::new(),
            sleeps: Vec::new(),
        }
    }
}
impl xtask::cli_catalog::Terminal for Terminal {
    fn send(&mut self, keys: &[String]) -> Result<(), String> {
        self.trace.push(json!(["send", keys]));
        if let [literal, text] = keys
            && literal == "-l"
        {
            self.current = text.clone();
            return Ok(());
        }
        self.current.clear();
        Ok(())
    }
    fn capture(&mut self) -> Result<String, String> {
        self.trace.push(json!(["capture"]));
        let rows = self.captures.get(&self.current);
        let count = self.counters.entry(self.current.clone()).or_insert(0);
        let row = rows
            .and_then(|rows| rows.get((*count).min(rows.len().saturating_sub(1))))
            .cloned()
            .unwrap_or_default();
        *count += 1;
        Ok(row)
    }
    fn wait(&mut self, duration: Duration) {
        self.sleeps.push(duration.as_millis().try_into().unwrap());
    }
}

fn scraping_oracle(
    fixture: &Fixture,
    script: &str,
    arguments: &[String],
    captures: &Value,
) -> Value {
    let helper = r#"import contextlib,io,json,subprocess,sys,time,types
data=json.load(sys.stdin);source=sys.argv[1];sys.argv=[source,*data['args']];trace=[];sleeps=[];current='';counts={}
def fake(argv,**kwargs):
 global current
 assert argv[:3]==['tmux','-L','fixture-private-socket'] and '-t' in argv and argv[argv.index('-t')+1]=='fixture-private-session',argv
 if argv[3]=='send-keys':
  keys=argv[6:];trace.append(['send',keys]);current=keys[1] if len(keys)==2 and keys[0]=='-l' else ''
  return types.SimpleNamespace(returncode=0)
 trace.append(['capture']);rows=data['captures'].get(current,[]);i=counts.get(current,0);counts[current]=i+1
 return types.SimpleNamespace(stdout=rows[min(i,len(rows)-1)] if rows else '',returncode=0)
subprocess.run=fake;time.sleep=lambda seconds:sleeps.append(round(seconds*1000));out=io.StringIO()
with contextlib.redirect_stdout(out):exec(compile(open(source).read(),source,'exec'),{'__name__':'__main__','__file__':source})
print(json.dumps({'stdout':out.getvalue(),'trace':trace,'sleeps':sleeps},ensure_ascii=False))
"#;
    let mut child = Command::new("/usr/bin/python3")
        .args(["-c", helper])
        .arg(fixture.0.join(format!("tools/cli-commands/{script}.py")))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", fixture.0.join("home"))
        .env("XDG_CONFIG_HOME", fixture.0.join("config"))
        .env("XDG_CACHE_HOME", fixture.0.join("cache"))
        .env("XDG_DATA_HOME", fixture.0.join("data"))
        .env("XDG_STATE_HOME", fixture.0.join("state"))
        .env("XDG_RUNTIME_DIR", fixture.0.join("runtime"))
        .env("TMPDIR", fixture.0.join("tmp"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            serde_json::to_string(&json!({"args":arguments,"captures":captures}))
                .unwrap()
                .as_bytes(),
        )
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    serde_json::from_slice(&result.stdout).unwrap()
}

#[test]
fn prefix_walk_matches_original_recursion_overwrites_depth_and_private_key_trace() {
    let fixture = Fixture::new();
    let captures = json!({"/a":[" /alpha  first\n /apple  second\n /wrong  ignored"],"/aa":[" /alpha  overwritten"],"/ab":[" /abacus  X\n /about  Y"],"/aba":[" /abacus  X\n /abandon  Z"],"/abaa":[" /abaa  deepest\n /abaaz  no further expansion"],"/b":[" /beta  \u{001c} stripped\u{001f}"]});
    let pattern = r"^\s*/([a-z][\w-]*)\s{2,}(.*)$";
    let expected = scraping_oracle(
        &fixture,
        "scrape_prefix",
        &[
            "fixture-private-socket".into(),
            "fixture-private-session".into(),
            pattern.into(),
            "/".into(),
            "2".into(),
        ],
        &captures,
    );
    let mut terminal = Terminal::new(&captures);
    let actual = xtask::cli_catalog::scrape_prefix(&mut terminal, pattern, "/", 2).unwrap();
    assert_eq!(actual, expected["stdout"].as_str().unwrap());
    assert_eq!(json!(terminal.trace), expected["trace"]);
    assert_eq!(json!(terminal.sleeps), expected["sleeps"]);
    assert!(
        terminal
            .trace
            .iter()
            .all(|v| !v.to_string().contains("Enter"))
    );
}

#[test]
fn verify_names_matches_original_prompt_retry_unicode_and_null_order() {
    let fixture = Fixture::new();
    let names = json!([
        "beta",
        "a²",
        "missing",
        "alpha",
        "alpha",
        "later",
        "edgeᲉ",
        "slash:child"
    ]);
    let captures = json!({"/alpha":[" ❯ /alpha \n /alpha  description\u{001f}"],"/beta":[" /beta  wrong prompt", " › /beta\u{00a0}\n /beta  final"],"/later":[" ❯ /later "," ❯ /later \n /later  late"],"/a²":[" ❯ /a²\n /a²  superscript"],"/edgeᲉ":[" ❯ /edgeᲉ\n /edgeᲉ  unknown UCD13"],"/slash:child":[" › /slash:child\n /slash:child  nested"]});
    let names_path = fixture.0.join("names.json");
    fs::write(&names_path, serde_json::to_vec(&names).unwrap()).unwrap();
    let expected = scraping_oracle(
        &fixture,
        "verify_names",
        &[
            "fixture-private-socket".into(),
            "fixture-private-session".into(),
            names_path.display().to_string(),
        ],
        &captures,
    );
    let mut terminal = Terminal::new(&captures);
    let names = names
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    let actual = xtask::cli_catalog::verify_names(&mut terminal, &names).unwrap();
    assert_eq!(actual, expected["stdout"].as_str().unwrap());
    assert_eq!(json!(terminal.trace), expected["trace"]);
    assert_eq!(json!(terminal.sleeps), expected["sleeps"]);
}

#[test]
fn actual_command_transport_includes_private_socket_exact_args_and_ignores_child_status() {
    use xtask::cli_catalog::Terminal as _;
    let fixture = Fixture::new();
    let program = fixture.0.join("fake-tmux");
    let record = fixture.0.join("record.jsonl");
    // Private process double; no real tmux server is started or contacted.
    let script = format!(
        "#!/usr/bin/python3\nimport json,sys\nargs=sys.argv[1:]\nassert args[:2]==['-L','fixture-private-socket']\nwith open({},'a') as f:f.write(json.dumps(args,ensure_ascii=False)+'\\n')\nif args[2]=='capture-pane':sys.stdout.buffer.write(b'\\xc3\\xb1\\r\\nrow\\r')\nraise SystemExit(17)\n",
        serde_json::to_string(record.to_str().unwrap()).unwrap()
    );
    fs::write(&program, script).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
    let mut terminal = xtask::cli_catalog::CommandTerminal::new(
        program,
        "fixture-private-socket".into(),
        "fixture-private-session".into(),
    )
    .unwrap();
    terminal
        .send(&["-l".into(), "/x '; 😀\n literal".into()])
        .unwrap();
    assert_eq!(terminal.capture().unwrap(), "ñ\nrow\n");
    let rows = fs::read_to_string(record)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        vec![
            json!([
                "-L",
                "fixture-private-socket",
                "send-keys",
                "-t",
                "fixture-private-session",
                "-l",
                "/x '; 😀\n literal"
            ]),
            json!([
                "-L",
                "fixture-private-socket",
                "capture-pane",
                "-p",
                "-J",
                "-t",
                "fixture-private-session"
            ])
        ]
    );
}

#[test]
fn unsupported_pattern_fails_without_terminal_activity_and_universal_lines_match() {
    let fixture = Fixture::new();
    let captures =
        json!({"/a":[" /alpha  A\u{2028} /apple  B\r\n /a界  界\u{0085} /aᲉ  new-unassigned"]});
    let mut terminal = Terminal::new(&captures);
    assert!(xtask::cli_catalog::scrape_prefix(&mut terminal, r"(\b[a-z]+)(.*)", "/", 8).is_err());
    assert!(terminal.trace.is_empty());
    assert!(terminal.sleeps.is_empty());
    let pattern = r"^\s*/([a-z][\w-]*)\s{2,}(.*)$";
    let expected = scraping_oracle(
        &fixture,
        "scrape_prefix",
        &[
            "fixture-private-socket".into(),
            "fixture-private-session".into(),
            pattern.into(),
            "/".into(),
        ],
        &captures,
    );
    assert_eq!(
        xtask::cli_catalog::scrape_prefix(&mut terminal, pattern, "/", 8).unwrap(),
        expected["stdout"].as_str().unwrap()
    );
    assert_eq!(json!(terminal.trace), expected["trace"]);
    assert_eq!(json!(terminal.sleeps), expected["sleeps"]);
}

#[test]
fn actual_cli_build_and_filter_work_from_arbitrary_cwd() {
    let fixture = Fixture::new();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let before = fs::read(repo.join("config/cli-commands.json")).unwrap();
    let mut build = Command::new(env!("CARGO_BIN_EXE_xtask"));
    build.args(["cli-catalog", "build"]);
    let input = fixture.0.join("commands.json");
    let binary = fixture.0.join("bin-image");
    fs::write(&input, r#"[["a"," kept  built-in"],["z","absent"]]"#).unwrap();
    fs::write(&binary, b"\0kept\0").unwrap();
    let expected = fixture.python(
        "builtin_filter",
        &[input.display().to_string(), binary.display().to_string()],
    );
    let mut filter = Command::new(env!("CARGO_BIN_EXE_xtask"));
    filter
        .args(["cli-catalog", "builtin-filter"])
        .arg(input)
        .arg(binary);
    for (command, wanted) in [
        (
            &mut build,
            format!("{}\n", repo.join("config/cli-commands.json").display()).into_bytes(),
        ),
        (&mut filter, expected),
    ] {
        let result = command
            .current_dir(&fixture.0)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", fixture.0.join("home"))
            .env("XDG_CONFIG_HOME", fixture.0.join("config"))
            .env("XDG_CACHE_HOME", fixture.0.join("cache"))
            .env("XDG_DATA_HOME", fixture.0.join("data"))
            .env("XDG_STATE_HOME", fixture.0.join("state"))
            .env("XDG_RUNTIME_DIR", fixture.0.join("runtime"))
            .env("TMPDIR", fixture.0.join("tmp"))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stderr.is_empty());
        assert_eq!(result.stdout, wanted);
    }
    assert_eq!(
        fs::read(repo.join("config/cli-commands.json")).unwrap(),
        before
    );
}
