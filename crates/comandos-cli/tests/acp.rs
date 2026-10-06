//! Actual Python ACP client and real private stdio processes, never a vendor CLI.
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "acp-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        for p in [
            "home/.claude/hooks",
            "data",
            "config",
            "cache",
            "state",
            "run",
            "tmp",
            "bin",
            "oracle/bin",
            "oracle/lib",
            "oracle/config",
            "project",
        ] {
            fs::create_dir_all(root.join(p)).unwrap();
        }
        fn private(p: &Path) {
            fs::set_permissions(p, fs::Permissions::from_mode(0o700)).unwrap();
            for e in fs::read_dir(p).unwrap().flatten() {
                if e.file_type().unwrap().is_dir() {
                    private(&e.path());
                }
            }
        }
        private(&root);
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        fs::copy(repo.join("bin/cc-acp"), root.join("oracle/bin/cc-acp")).unwrap();
        for e in fs::read_dir(repo.join("lib")).unwrap().flatten() {
            if e.path().extension().is_some_and(|x| x == "py") {
                fs::copy(e.path(), root.join("oracle/lib").join(e.file_name())).unwrap();
            }
        }
        fs::copy(
            repo.join("config/providers.json"),
            root.join("oracle/config/providers.json"),
        )
        .unwrap();
        fs::write(root.join("fake"), FAKE).unwrap();
        fs::set_permissions(root.join("fake"), fs::Permissions::from_mode(0o700)).unwrap();
        for name in [
            "claude-agent-acp",
            "claude",
            "codex-acp",
            "grok",
            "opencode",
            "agy",
            "tmux",
        ] {
            symlink(root.join("fake"), root.join("bin").join(name)).unwrap();
        }
        fs::write(root.join("scenario"), "normal").unwrap();
        Self(root)
    }
    fn run(&self, python: bool, args: &[&str], input: &str) -> Output {
        let c = if python {
            let mut c = Command::new("/usr/bin/python3");
            c.arg(self.0.join("oracle/bin/cc-acp"));
            c
        } else {
            let mut c = Command::new(env!("CARGO_BIN_EXE_comandos"));
            c.arg("acp");
            c
        };
        self.invoke(c, args, input.as_bytes())
    }
    fn invoke(&self, mut c: Command, args: &[&str], input: &[u8]) -> Output {
        c.args(args)
            .env_clear()
            .env("HOME", self.0.join("home"))
            .env("PATH", self.0.join("bin"))
            .env("ACP_FIXTURE", &self.0)
            .env("TMUX_PANE", "%private")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("XDG_DATA_HOME", self.0.join("data"))
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .env("XDG_CACHE_HOME", self.0.join("cache"))
            .env("XDG_STATE_HOME", self.0.join("state"))
            .env("XDG_RUNTIME_DIR", self.0.join("run"))
            .env("TMPDIR", self.0.join("tmp"))
            .current_dir(self.0.join("project"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }
    fn trace(&self) -> Vec<Value> {
        fs::read_to_string(self.0.join("trace"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
    fn clear(&self) {
        for p in ["trace", "home/.claude/hooks/acp-panes.json"] {
            let _ = fs::remove_file(self.0.join(p));
        }
    }
    fn config(&self) -> comandos_cli::acp::Config {
        let mut registry: Value =
            serde_json::from_slice(&fs::read(self.0.join("oracle/config/providers.json")).unwrap())
                .unwrap();
        for spec in registry["acpAgents"].as_object_mut().unwrap().values_mut() {
            if !spec["env"].is_object() {
                spec["env"] = json!({});
            }
            for (key, tail) in [
                ("ACP_FIXTURE", ""),
                ("HOME", "home"),
                ("XDG_DATA_HOME", "data"),
                ("XDG_CONFIG_HOME", "config"),
                ("XDG_CACHE_HOME", "cache"),
                ("XDG_STATE_HOME", "state"),
                ("XDG_RUNTIME_DIR", "run"),
                ("TMPDIR", "tmp"),
            ] {
                spec["env"][key] = json!(self.0.join(tail));
            }
        }
        comandos_cli::acp::Config {
            home: self.0.join("home"),
            cwd: self.0.join("project"),
            registry,
            path: Some(self.0.join("bin").into_os_string()),
            pane: "%private".into(),
            tmux: None,
            notify: None,
            colors: false,
            timeouts: comandos_cli::acp::protocol::Timeouts::default(),
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
const FAKE: &str = r##"#!/usr/bin/python3
import sys,os,json
root=os.environ['ACP_FIXTURE'];name=os.path.basename(sys.argv[0]);scenario=open(root+'/scenario').read()
def log(obj):
 with open(root+'/trace','a') as f:f.write(json.dumps(obj)+'\n')
def send(obj):
 log({'out':obj});print(json.dumps(obj),flush=True)
if name=='tmux':log({'tmux':sys.argv[1:]});sys.exit(0)
if name=='claude':sys.exit(0)
prior=sum(1 for line in open(root+'/trace') if json.loads(line).get('launch')==name) if os.path.exists(root+'/trace') else 0
log({'launch':name,'args':sys.argv[1:],'env':{k:os.environ[k] for k in ['MAX_THINKING_TOKENS','OPENCODE_CONFIG_CONTENT','CLAUDE_CONFIG_DIR','CODEX_HOME','GROK_HOME'] if k in os.environ}})
if scenario=='procgroup':log({'ownGroup':os.getpgrp()==os.getpid()})
sid='private-session-123';model='vendor-model';mode='default';effort='medium';pending=None
if scenario in ['reconnect-fail','reconnect-ok']:model=json.load(open(root+'/oracle/config/providers.json'))['motors']['claude']['models'][0]['id']
def options():return [{'id':'thinking','category':'thought_level','type':'select','currentValue':effort,'options':[{'value':'low','name':'Low'},{'group':'More','options':[{'value':'medium'},{'value':'high'}]}]}]
for line in sys.stdin:
 msg=json.loads(line);log({'in':msg})
 if name=='agy':
  if scenario=='agy-hang':open(root+'/ready','w').close();continue
  send({'event':'init','conversation_id':sid,'init':{'model':model}})
  send({'event':'step_update','step_update':{'text_delta':'hola'}})
  send({'event':'result','result':{'status':'SUCCESS','conversation_id':sid}});continue
 method=msg.get('method');rid=msg.get('id');p=msg.get('params',{})
 if not method:continue
 if scenario=='control-events' and (method=='initialize' or method.startswith('session/set_')):send({'jsonrpc':'2.0','method':'session/update','params':{'sessionId':sid,'update':{'sessionUpdate':'agent_message_chunk','content':{'type':'text','text':'startup/config hidden'}}}})
 if method=='initialize':
  if scenario=='reconnect-fail' and prior:send({'jsonrpc':'2.0','id':rid,'error':{'code':-32000,'message':'private candidate failed'}});continue
  if scenario=='hang':continue
  if scenario=='array':send([]);continue
  if scenario=='utf8':sys.stdout.buffer.write(b'\xff\n');sys.stdout.flush();continue
  result={'protocolVersion':99 if scenario=='bad-version' else 1,'agentCapabilities':{'loadSession':scenario!='no-load'},'authMethods':[]}
 elif method in ['session/new','session/load']:
  if scenario=='startup-permission':
   send({'jsonrpc':'2.0','id':'startup-perm','method':'session/request_permission','params':{'sessionId':p.get('sessionId',sid),'toolCall':{'title':'startup edit'},'options':[{'optionId':'yes','kind':'allow_once','name':'Allow'},{'optionId':'no','kind':'reject_once','name':'Reject'}]}});answer=json.loads(sys.stdin.readline());log({'in':answer})
  sid=p.get('sessionId',sid);result={'sessionId':sid,'models':{'availableModels':[{'modelId':'vendor-model'},{'modelId':'other-model'}],'currentModelId':model},'modes':{'availableModes':[{'id':'default'},{'id':'bypassPermissions'},{'id':'full-access'}],'currentModeId':mode},'configOptions':options()}
  if scenario=='mismatch-session' and method=='session/load':result['sessionId']=sid+'-different'
  if scenario in ['reconnect-fail','reconnect-ok']:result.pop('configOptions');result['models']['availableModels']=[{'modelId':model}]
  if scenario=='no-models':result['models']['availableModels']=[]
 elif method=='session/set_config_option':effort='medium' if scenario=='wrong-config' else p['value'];result={'configOptions':options()}
 elif method=='session/set_model':model=p['modelId'];result={}
 elif method=='session/set_mode':mode=p['modeId'];result={}
 elif method=='session/prompt':
  if scenario=='cancel':pending=rid;open(root+'/ready','w').close();continue
  if scenario=='paused':
   import time
   open(root+'/ready','w').close();end=time.monotonic()+3
   while not os.path.exists(root+'/release') and time.monotonic()<end:time.sleep(.002)
  try:log({'published':json.load(open(root+'/home/.claude/hooks/acp-panes.json'))['%private']})
  except (FileNotFoundError,json.JSONDecodeError,KeyError):pass
  if scenario=='permission':
   send({'jsonrpc':'2.0','id':'permission-7','method':'session/request_permission','params':{'sessionId':sid,'toolCall':{'title':'private edit'},'options':[{'optionId':'yes','kind':'allow_once','name':'Allow'},{'optionId':'no','kind':'reject_once','name':'Reject'}]}})
   answer=json.loads(sys.stdin.readline());log({'in':answer})
  if scenario=='eof':sys.exit(0)
  if scenario=='takeover':
   state=json.load(open(root+'/home/.claude/hooks/acp-panes.json'));state['%private']={'pid':42,'sessionId':'replacement'}
   with open(root+'/home/.claude/hooks/acp-panes.json','w') as f:json.dump(state,f)
  if scenario=='fs':
   for ident,method,params in [('read-1','fs/read_text_file',{'sessionId':sid,'path':root+'/project/read.txt','line':2,'limit':2}),('write-1','fs/write_text_file',{'sessionId':sid,'path':root+'/project/new/written.txt','content':'private café\n'}),('terminal-1','terminal/create',{'sessionId':sid})]:
    send({'jsonrpc':'2.0','id':ident,'method':method,'params':params});answer=json.loads(sys.stdin.readline());log({'in':answer})
  if scenario=='late':send({'jsonrpc':'2.0','id':rid-1,'result':{'stopReason':'wrong'}})
  send({'jsonrpc':'2.0','method':'session/update','params':{'sessionId':sid,'update':{'sessionUpdate':'agent_message_chunk','content':{'type':'text','text':'hola'}}}})
  result={'stopReason':'end_turn'}
 elif method=='session/cancel':
  if pending is not None:send({'jsonrpc':'2.0','id':pending,'result':{'stopReason':'cancelled'}});pending=None
  continue
 else:result={}
 send({'jsonrpc':'2.0','id':rid,'result':result})
"##;
fn normalize(text: &[u8]) -> String {
    let s = String::from_utf8_lossy(text);
    let re = regex::Regex::new(r"· [0-9]+\.[0-9]s ·").unwrap();
    re.replace_all(&s, "· CLOCKs ·").into_owned()
}
fn transcript(mut values: Vec<Value>) -> Value {
    for v in &mut values {
        if let Some(p) = v.get_mut("published").and_then(Value::as_object_mut) {
            p.remove("pid");
            p.remove("ts");
        }
    }
    // Launch logging and the tmux child are independent processes (Agy has no
    // initialize RPC barrier). Preserve each channel's order, not scheduler order.
    json!({"launch":values.iter().filter(|v|v.get("launch").is_some()).collect::<Vec<_>>(),"tmux":values.iter().filter(|v|v.get("tmux").is_some()).collect::<Vec<_>>(),"wire":values.iter().filter(|v|v.get("in").is_some()||v.get("out").is_some()).collect::<Vec<_>>(),"published":values.iter().filter(|v|v.get("published").is_some()).collect::<Vec<_>>()})
}
#[test]
fn actual_original_incoming_files_and_unsupported_rpc_transcripts() {
    let f = Fixture::new();
    fs::write(f.0.join("scenario"), "fs").unwrap();
    fs::write(
        f.0.join("project/read.txt"),
        "one\r\ntwo\rthree\u{2028}four\n",
    )
    .unwrap();
    let old = f.run(true, &["--once", "read/write"], "");
    assert!(old.status.success());
    let expected = transcript(f.trace());
    assert_eq!(
        fs::read(f.0.join("project/new/written.txt")).unwrap(),
        "private café\n".as_bytes()
    );
    f.clear();
    let new = f.run(false, &["--once", "read/write"], "");
    assert_eq!(new.status.code(), Some(0));
    assert_eq!(transcript(f.trace()), expected);
    assert_eq!(
        fs::read(f.0.join("project/new/written.txt")).unwrap(),
        "private café\n".as_bytes()
    );
}
#[test]
fn replacement_generation_is_not_overwritten_or_removed() {
    let f = Fixture::new();
    fs::write(f.0.join("scenario"), "takeover").unwrap();
    assert!(f.run(true, &["--once", "private"], "").status.success());
    let old: Value =
        serde_json::from_slice(&fs::read(f.0.join("home/.claude/hooks/acp-panes.json")).unwrap())
            .unwrap();
    assert_ne!(old["%private"]["sessionId"], "replacement");
    f.clear();
    let new = f.run(false, &["--once", "private"], "");
    assert_eq!(new.status.code(), Some(2));
    let value: Value =
        serde_json::from_slice(&fs::read(f.0.join("home/.claude/hooks/acp-panes.json")).unwrap())
            .unwrap();
    assert_eq!(
        value,
        json!({"%private":{"pid":42,"sessionId":"replacement"}})
    );
}

#[test]
fn actual_original_once_transcripts_all_five_transports() {
    for agent in ["claude", "codex", "grok", "opencode", "agy"] {
        let f = Fixture::new();
        let args = [
            "--agent",
            agent,
            "--model",
            "vendor-model",
            "--effort",
            "high",
            "--once",
            "hola privado",
        ];
        let old = f.run(true, &args, "");
        assert!(
            old.status.success(),
            "{}",
            String::from_utf8_lossy(&old.stderr)
        );
        let expected = transcript(f.trace());
        f.clear();
        let new = f.run(false, &args, "");
        assert_eq!(
            new.status.code(),
            old.status.code(),
            "{agent}: {}",
            String::from_utf8_lossy(&new.stderr)
        );
        assert_eq!(normalize(&new.stdout), normalize(&old.stdout), "{agent}");
        assert_eq!(new.stderr, old.stderr);
        assert_eq!(transcript(f.trace()), expected, "{agent}");
    }
}
#[test]
fn actual_original_prompt_commands_confirmed_state_and_exact_resume() {
    let f = Fixture::new();
    let input = "/status\n/models\n/effort low\n/model other-model\n/mode bypassPermissions\n/danger\n/status\n/unknown\n/exit\n";
    let old = f.run(true, &["--resume", "private-resumed"], input);
    let expected = transcript(f.trace());
    f.clear();
    let new = f.run(false, &["--resume", "private-resumed"], input);
    assert_eq!(new.status.code(), old.status.code());
    assert_eq!(normalize(&new.stdout), normalize(&old.stdout));
    assert_eq!(new.stderr, old.stderr);
    assert_eq!(transcript(f.trace()), expected);
}
#[test]
fn actual_original_resume_refusal_does_not_create_session() {
    let f = Fixture::new();
    fs::write(f.0.join("scenario"), "no-load").unwrap();
    let old = f.run(true, &["--resume", "private-resumed"], "");
    let expected = f.trace();
    f.clear();
    let new = f.run(false, &["--resume", "private-resumed"], "");
    assert_eq!(new.status.code(), Some(2));
    assert_eq!(new.stdout, old.stdout);
    assert_eq!(new.stderr, old.stderr);
    assert_eq!(f.trace(), expected);
    assert!(
        !f.trace()
            .iter()
            .any(|v| v["in"]["method"] == json!("session/new"))
    );
}
#[test]
fn actual_original_help_and_missing_argument() {
    let f = Fixture::new();
    for args in [vec!["--help"], vec!["--model"]] {
        let old = f.run(true, &args, "");
        let new = f.run(false, &args, "");
        assert_eq!(new.status.code(), old.status.code());
        assert_eq!(new.stdout, old.stdout);
        assert_eq!(new.stderr, old.stderr);
        assert!(f.trace().is_empty());
    }
}
#[test]
fn late_response_id_does_not_finish_current_turn() {
    let f = Fixture::new();
    fs::write(f.0.join("scenario"), "late").unwrap();
    let old = f.run(true, &["--once", "late test"], "");
    let expected = transcript(f.trace());
    f.clear();
    let new = f.run(false, &["--once", "late test"], "");
    assert_eq!(new.status.code(), Some(0));
    assert_eq!(normalize(&new.stdout), normalize(&old.stdout));
    assert_eq!(transcript(f.trace()), expected);
}
#[test]
fn permission_eof_cancels_instead_of_original_automatic_allow() {
    let f = Fixture::new();
    fs::write(f.0.join("scenario"), "permission").unwrap();
    let old = f.run(true, &["--once", "edit"], "");
    assert!(old.status.success());
    assert!(
        f.trace()
            .iter()
            .any(|v| v["in"]["result"]["outcome"]["optionId"] == "yes")
    );
    f.clear();
    let new = f.run(false, &["--once", "edit"], "");
    assert!(
        new.status.success(),
        "{}",
        String::from_utf8_lossy(&new.stdout)
    );
    assert!(
        f.trace()
            .iter()
            .any(|v| v["in"]["result"]["outcome"]["outcome"] == "cancelled")
    );
    assert!(
        !f.trace()
            .iter()
            .any(|v| v["in"]["result"]["outcome"]["optionId"] == "yes")
    );
}
#[test]
fn protocol_version_mismatch_rejects_before_session_creation() {
    let f = Fixture::new();
    fs::write(f.0.join("scenario"), "bad-version").unwrap();
    assert!(f.run(true, &["--once", "private"], "").status.success());
    f.clear();
    let new = f.run(false, &["--once", "private"], "");
    assert_eq!(new.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&new.stdout).contains("versión ACP"));
    assert!(!f.trace().iter().any(|v| v["in"]["method"] == "session/new"));
}
#[test]
fn closed_stdout_prompt_returns_failure_and_removes_only_owned_record() {
    let f = Fixture::new();
    fs::write(f.0.join("scenario"), "eof").unwrap();
    fs::write(
        f.0.join("home/.claude/hooks/acp-panes.json"),
        r#"{"%other":{"sessionId":"unrelated","pid":1}}"#,
    )
    .unwrap();
    let old = f.run(true, &["--once", "private"], "");
    assert_eq!(old.status.code(), Some(0));
    f.clear();
    fs::write(
        f.0.join("home/.claude/hooks/acp-panes.json"),
        r#"{"%other":{"sessionId":"unrelated","pid":1}}"#,
    )
    .unwrap();
    let new = f.run(false, &["--once", "private"], "");
    assert_eq!(new.status.code(), Some(1));
    let state: Value =
        serde_json::from_slice(&fs::read(f.0.join("home/.claude/hooks/acp-panes.json")).unwrap())
            .unwrap();
    assert_eq!(state, json!({"%other":{"sessionId":"unrelated","pid":1}}));
}
#[test]
fn acp_registry_does_not_hydrate_unrelated_vendor_cache() {
    let f = Fixture::new();
    fs::write(f.0.join("scenario"), "no-models").unwrap();
    fs::create_dir_all(f.0.join("home/.codex")).unwrap();
    fs::write(f.0.join("home/.codex/models_cache.json"),r#"{"models":[{"slug":"gpt-5.5-private-observed","visibility":"list","supported_reasoning_levels":[{"effort":"low"}],"default_reasoning_level":"low"}]}"#).unwrap();
    let old = f.run(true, &["--agent", "codex"], "/models\n/exit\n");
    f.clear();
    let new = f.run(false, &["--agent", "codex"], "/models\n/exit\n");
    assert_eq!(new.stdout, old.stdout);
}
#[test]
fn binary_alias_and_argument_boundaries_never_launch_agents() {
    let f = Fixture::new();
    let alias = f.0.join("cc-acp");
    symlink(env!("CARGO_BIN_EXE_comandos"), &alias).unwrap();
    for args in [
        vec!["--help"],
        vec!["--a", "value"],
        vec!["--model=two words", "--help"],
        vec!["--danger=true"],
        vec!["--wat"],
        vec!["--", "positional"],
    ] {
        let old = f.run(true, &args, "");
        let new = f.invoke(Command::new(&alias), &args, b"");
        assert_eq!(new.status.code(), old.status.code());
        assert_eq!(new.stdout, old.stdout);
        assert_eq!(new.stderr, old.stderr);
        assert!(f.trace().is_empty());
    }
}
#[test]
fn invalid_utf8_input_and_protocol_are_errors_without_panics() {
    let f = Fixture::new();
    for scenario in ["array", "utf8"] {
        fs::write(f.0.join("scenario"), scenario).unwrap();
        let old = f.run(true, &["--once", "private"], "");
        assert!(!old.status.success());
        f.clear();
        let new = f.run(false, &["--once", "private"], "");
        assert_eq!(new.status.code(), Some(2));
        assert!(new.stderr.is_empty());
        assert!(!String::from_utf8_lossy(&new.stdout).contains("panic"));
        assert!(!f.trace().iter().any(|v| v["in"]["method"] == "session/new"));
        f.clear();
    }
    fs::write(f.0.join("scenario"), "normal").unwrap();
    let mut native = Command::new(env!("CARGO_BIN_EXE_comandos"));
    native.arg("acp");
    let new = f.invoke(native, &[], b"\xff\n");
    assert_eq!(new.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&new.stderr).contains("entrada no es UTF-8"));
}
#[test]
fn cancellation_uses_original_rpc_and_retains_exact_session() {
    let f = Fixture::new();
    fs::write(f.0.join("scenario"), "cancel").unwrap();
    let driver = f.0.join("cancel-driver.py");
    fs::write(
        &driver,
        r#"import os,runpy,signal,threading,time
root=os.environ['ACP_FIXTURE']
def register(sig,handler):
 def trigger():
  end=time.monotonic()+3
  while not os.path.exists(root+'/ready') and time.monotonic()<end:time.sleep(.002)
  if os.path.exists(root+'/ready'):handler(sig,None)
 threading.Thread(target=trigger,daemon=True).start()
signal.signal=register
runpy.run_path(root+'/oracle/bin/cc-acp',run_name='__main__')
"#,
    )
    .unwrap();
    let mut cmd = Command::new("/usr/bin/python3");
    cmd.arg(driver);
    let old = f.invoke(cmd, &[], b"private\n/status\n/exit\n");
    assert!(
        old.status.success(),
        "{}",
        String::from_utf8_lossy(&old.stderr)
    );
    let expected = transcript(f.trace());
    f.clear();
    fs::remove_file(f.0.join("ready")).unwrap();
    let config = f.config();
    let flag = config.cancel.clone();
    let ready = f.0.join("ready");
    let trigger = std::thread::spawn(move || {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !ready.exists() && std::time::Instant::now() < end {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(ready.exists());
        flag.store(true, Ordering::Relaxed);
    });
    let mut out = vec![];
    let mut err = vec![];
    let code = comandos_cli::acp::run(
        &config,
        &[],
        std::io::Cursor::new(b"private\n/status\n/exit\n".to_vec()),
        &mut out,
        &mut err,
    );
    trigger.join().unwrap();
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    assert_eq!(normalize(&out), normalize(&old.stdout));
    let actual = transcript(f.trace());
    assert_eq!(actual["wire"], expected["wire"]);
    assert!(
        actual["wire"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["in"]["method"] == "session/cancel")
    );
}
#[test]
fn agy_cancel_closes_owned_transport_and_removes_observation() {
    let f = Fixture::new();
    fs::write(f.0.join("scenario"), "agy-hang").unwrap();
    let mut config = f.config();
    config.timeouts.prompt = std::time::Duration::from_secs(1);
    let flag = config.cancel.clone();
    let ready = f.0.join("ready");
    let trigger = std::thread::spawn(move || {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !ready.exists() && std::time::Instant::now() < end {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(ready.exists());
        flag.store(true, Ordering::Relaxed);
    });
    let mut out = vec![];
    let mut err = vec![];
    let code = comandos_cli::acp::run(
        &config,
        &["--agent".into(), "agy".into()],
        std::io::Cursor::new(b"private\nsecond\n/exit\n".to_vec()),
        &mut out,
        &mut err,
    );
    trigger.join().unwrap();
    assert_eq!(code, 1);
    let actual = f.trace();
    assert_eq!(
        actual.iter().filter(|v| v["in"]["event"] == "user").count(),
        1
    );
    assert!(String::from_utf8_lossy(&out).contains("agy no admite cancelación"));
    let state: Value =
        serde_json::from_slice(&fs::read(f.0.join("home/.claude/hooks/acp-panes.json")).unwrap())
            .unwrap();
    assert_eq!(state, json!({}));
}
#[test]
fn initialize_timeout_matches_original_and_does_not_create_session() {
    let f = Fixture::new();
    fs::write(f.0.join("scenario"), "hang").unwrap();
    let driver = f.0.join("timeout-driver.py");
    fs::write(
        &driver,
        r#"import os,sys
root=os.environ['ACP_FIXTURE'];sys.path.insert(0,root+'/oracle/lib');import acp
s=acp.open_session({'command':[root+'/bin/claude-agent-acp']},root+'/project')
try:s.initialize(timeout=.03)
except acp.AcpError as e:print(str(e))
finally:s.close()
"#,
    )
    .unwrap();
    let mut cmd = Command::new("/usr/bin/python3");
    cmd.arg(driver);
    let old = f.invoke(cmd, &[], b"");
    assert!(old.status.success());
    let expected = f.trace();
    f.clear();
    let config = f.config();
    let mut timeouts = config.timeouts.clone();
    timeouts.initialize = std::time::Duration::from_millis(30);
    let env =
        std::collections::BTreeMap::from([("ACP_FIXTURE".into(), f.0.to_str().unwrap().into())]);
    let mut session = comandos_cli::acp::protocol::Session::open(
        &[f.0.join("bin/claude-agent-acp").to_str().unwrap().into()],
        &env,
        &config.cwd,
        false,
        timeouts,
        config.cancel.clone(),
    )
    .unwrap();
    let message = session
        .initialize(&mut comandos_cli::acp::protocol::Quiet)
        .unwrap_err();
    drop(session);
    assert_eq!(format!("{message}\n").as_bytes(), old.stdout);
    assert_eq!(f.trace(), expected);
}

#[test]
fn original_startup_permissions_remain_interactive_and_control_events_are_hidden() {
    for scenario in ["control-events", "startup-permission"] {
        let f = Fixture::new();
        fs::write(f.0.join("scenario"), scenario).unwrap();
        let args = ["--resume", "private-resumed", "--once", "private"];
        let old = f.run(true, &args, "2\n");
        assert!(old.status.success());
        let expected = transcript(f.trace());
        f.clear();
        let new = f.run(false, &args, "2\n");
        assert_eq!(normalize(&new.stdout), normalize(&old.stdout), "{scenario}");
        assert_eq!(transcript(f.trace()), expected);
    }
}
#[test]
fn actual_original_accounts_and_reconnect_commit_or_rollback() {
    for scenario in ["reconnect-fail", "reconnect-ok"] {
        let f = Fixture::new();
        fs::write(f.0.join("scenario"), scenario).unwrap();
        fs::write(
            f.0.join("home/.claude/.credentials.json"),
            r#"{"claudeAiOauth":{"accessToken":"FAKE-PRIVATE-TOKEN"}}"#,
        )
        .unwrap();
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(f.0.join("home/.claude-accounts/private"))
            .unwrap();
        fs::write(
            f.0.join("home/.claude-accounts/private/.credentials.json"),
            r#"{"claudeAiOauth":{"accessToken":"FAKE-PRIVATE-TOKEN"}}"#,
        )
        .unwrap();
        let input =
            "/account\n/effort low\nprivate\n/account private\n/agent codex\n/status\n/exit\n";
        let old = f.run(true, &["--effort", "high"], input);
        assert!(
            old.status.success(),
            "{}",
            String::from_utf8_lossy(&old.stderr)
        );
        let expected = transcript(f.trace());
        f.clear();
        let new = f.run(false, &["--effort", "high"], input);
        assert_eq!(normalize(&new.stdout), normalize(&old.stdout), "{scenario}");
        assert_eq!(new.stderr, old.stderr);
        assert_eq!(transcript(f.trace()), expected);
        assert_eq!(
            f.trace()
                .iter()
                .filter(|v| v["in"]["method"] == "session/new")
                .count(),
            1
        );
    }
    let f = Fixture::new();
    let args = ["--account", "", "--once", "private"];
    let old = f.run(true, &args, "");
    f.clear();
    let new = f.run(false, &args, "");
    assert_eq!(normalize(&new.stdout), normalize(&old.stdout));
}
#[test]
fn actual_original_selected_accounts_build_private_env_for_three_providers() {
    for agent in ["claude", "codex", "grok"] {
        let f = Fixture::new();
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(f.0.join(format!("home/.{agent}-accounts/private")))
            .unwrap();
        let args = [
            "--agent",
            agent,
            "--account",
            "private",
            "--once",
            "private",
        ];
        let old = f.run(true, &args, "");
        assert!(old.status.success());
        let expected = transcript(f.trace());
        f.clear();
        let new = f.run(false, &args, "");
        assert!(new.status.success());
        assert_eq!(normalize(&new.stdout), normalize(&old.stdout));
        assert_eq!(transcript(f.trace()), expected);
    }
}
#[test]
fn configuration_confirmation_and_danger_policy_match_original() {
    for (scenario, args, input) in [
        ("wrong-config", vec![], "/effort high\n/status\n/exit\n"),
        ("normal", vec!["--danger"], "/danger\n/status\n/exit\n"),
        ("permission", vec!["--once", "private"], "2\n"),
    ] {
        let f = Fixture::new();
        fs::write(f.0.join("scenario"), scenario).unwrap();
        let old = f.run(true, &args, input);
        let expected = transcript(f.trace());
        f.clear();
        let new = f.run(false, &args, input);
        assert_eq!(normalize(&new.stdout), normalize(&old.stdout));
        assert_eq!(new.status.code(), old.status.code());
        assert_eq!(transcript(f.trace()), expected);
    }
}

#[test]
fn document_authority_for_live_observations_and_cleanup_in_all_four_modes() {
    use comandos_store::{
        domains::DomainStore,
        unified::{self, Mode},
    };
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let f = Fixture::new();
        fs::write(f.0.join("scenario"), "paused").unwrap();
        let config = f.config();
        let db = unified::open_unified(&config.home.join(".local/share/comandos/comandos.sqlite3"))
            .unwrap();
        if mode != Mode::Legacy {
            unified::set_mode(&db, "ui-docs", mode, "private", 1).unwrap();
        }
        let file = config.home.join(".claude/hooks/acp-panes.json");
        let doc = DomainStore { home: &config.home }.document(
            "hooks/acp-panes.json",
            "ui-docs",
            file.clone(),
        );
        doc.write(
            Some(&db),
            br#"{"%other":{"pid":1,"sessionId":"unrelated"}}"#,
            1,
        )
        .unwrap();
        drop(db);
        if matches!(mode, Mode::Unified | Mode::Sealed) {
            fs::write(&file, b"LEGACY DECOY MUST NOT BE READ").unwrap();
        }
        let old = fs::read(&file).ok();
        let home = config.home.clone();
        let ready = f.0.join("ready");
        let release = f.0.join("release");
        let reader = std::thread::spawn(move || {
            let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while !ready.exists() && std::time::Instant::now() < end {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            assert!(ready.exists());
            let doc = DomainStore { home: &home }.document(
                "hooks/acp-panes.json",
                "ui-docs",
                home.join(".claude/hooks/acp-panes.json"),
            );
            let observed: Value =
                serde_json::from_slice(&doc.read_readonly().unwrap().unwrap()).unwrap();
            fs::write(release, b"ok").unwrap();
            observed
        });
        let mut out = vec![];
        let mut err = vec![];
        let code = comandos_cli::acp::run(
            &config,
            &["--once".into(), "private".into()],
            std::io::Cursor::new(vec![]),
            &mut out,
            &mut err,
        );
        let observed = reader.join().unwrap();
        assert_eq!(
            code,
            0,
            "{mode:?}: {} {}",
            String::from_utf8_lossy(&err),
            String::from_utf8_lossy(&out)
        );
        assert_eq!(observed["%private"]["sessionId"], "private-session-123");
        assert_eq!(observed["%private"]["observedModel"], "vendor-model");
        assert_eq!(observed["%private"]["observedEffort"], "medium");
        assert_eq!(observed["%private"]["effortSource"], "acp-config-options");
        assert_eq!(observed["%private"]["observationVersion"], 1);
        let final_doc: Value =
            serde_json::from_slice(&doc.read_readonly().unwrap().unwrap()).unwrap();
        assert_eq!(
            final_doc,
            json!({"%other":{"pid":1,"sessionId":"unrelated"}})
        );
        if mode == Mode::Sealed {
            assert_eq!(fs::read(&file).ok(), old);
            assert!(!doc.lock.exists());
        }
    }
}
#[test]
fn opencode_model_quotes_remain_json_data_and_empty_home_help_has_no_controls() {
    let f = Fixture::new();
    let model = "vendor\"\\\nprivate";
    let args = ["--agent", "opencode", "--model", model, "--once", "private"];
    assert!(f.run(true, &args, "").status.success());
    let original = f
        .trace()
        .into_iter()
        .find(|v| v.get("launch").is_some())
        .unwrap();
    assert!(
        serde_json::from_str::<Value>(original["env"]["OPENCODE_CONFIG_CONTENT"].as_str().unwrap())
            .is_err()
    );
    f.clear();
    assert!(f.run(false, &args, "").status.success());
    let native = f
        .trace()
        .into_iter()
        .find(|v| v.get("launch").is_some())
        .unwrap();
    let value: Value =
        serde_json::from_str(native["env"]["OPENCODE_CONFIG_CONTENT"].as_str().unwrap()).unwrap();
    assert_eq!(value, json!({"model":model}));
    f.clear();
    fs::remove_dir_all(f.0.join("home")).unwrap();
    assert!(f.run(false, &["--help"], "").status.success());
    assert!(!f.0.join("home").exists());
    assert!(f.trace().is_empty());
}

#[test]
fn invalid_permission_choice_cancels_while_explicit_enter_keeps_original_default() {
    for choice in ["99\n", "not a choice\n", "\n"] {
        let f = Fixture::new();
        fs::write(f.0.join("scenario"), "permission").unwrap();
        assert!(f.run(true, &["--once", "private"], choice).status.success());
        assert!(
            f.trace()
                .iter()
                .any(|v| v["in"]["result"]["outcome"]["optionId"] == "yes")
        );
        f.clear();
        assert!(
            f.run(false, &["--once", "private"], choice)
                .status
                .success()
        );
        let decision = f
            .trace()
            .into_iter()
            .find(|v| v["in"]["id"] == "permission-7")
            .unwrap();
        if choice == "\n" {
            assert_eq!(decision["in"]["result"]["outcome"]["optionId"], "yes");
        } else {
            assert_eq!(decision["in"]["result"]["outcome"]["outcome"], "cancelled");
        }
    }
}
#[test]
fn cwd_that_cannot_be_encoded_in_acp_is_rejected_before_starting_a_child() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let f = Fixture::new();
    let mut config = f.config();
    config.cwd = config.cwd.join(OsString::from_vec(vec![0xff]));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&config.cwd)
        .unwrap();
    let mut out = vec![];
    let mut err = vec![];
    let code = comandos_cli::acp::run(
        &config,
        &[],
        std::io::Cursor::new(vec![]),
        &mut out,
        &mut err,
    );
    assert_eq!(code, 2);
    assert!(String::from_utf8_lossy(&out).contains("cwd no es UTF-8"));
    assert!(f.trace().is_empty());
}
#[test]
fn explicitly_different_resumed_session_is_rejected_without_new_or_persist() {
    let f = Fixture::new();
    fs::write(f.0.join("scenario"), "mismatch-session").unwrap();
    let args = ["--resume", "requested-private", "--once", "private"];
    assert!(f.run(true, &args, "").status.success());
    f.clear();
    let new = f.run(false, &args, "");
    assert_eq!(new.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&new.stdout).contains("no confirmó la conversación solicitada")
    );
    assert!(!f.0.join("home/.claude/hooks/acp-panes.json").exists());
    assert!(
        !f.trace()
            .iter()
            .any(|v| v["in"]["method"] == "session/new" || v["in"]["method"] == "session/prompt")
    );
}
#[test]
fn owned_agent_process_group_is_isolated_from_pane_interrupts() {
    let f = Fixture::new();
    fs::write(f.0.join("scenario"), "procgroup").unwrap();
    assert!(f.run(true, &["--once", "private"], "").status.success());
    assert!(f.trace().iter().any(|v| v["ownGroup"] == false));
    f.clear();
    assert!(f.run(false, &["--once", "private"], "").status.success());
    assert!(f.trace().iter().any(|v| v["ownGroup"] == true));
}
