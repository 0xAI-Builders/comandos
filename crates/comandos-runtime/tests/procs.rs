use comandos_runtime::procs::{
    ProcSource,
    linux::ProcFs,
    macos::{PsSource, Tools, parse_ps_line},
};
use std::{cell::RefCell, path::PathBuf, time::Duration};
#[test]
fn linux_fixture_retains_nul_argument_boundaries_start_and_links() {
    let fixture = Fixture::new("proc");
    let root = fixture.0.clone();
    std::fs::create_dir_all(root.join("812/fd")).unwrap();
    let mut fields = vec!["0"; 20];
    fields[0] = "S";
    fields[1] = "1";
    fields[19] = "4321";
    std::fs::write(
        root.join("812/stat"),
        format!("812 (name ) spaces) {}", fields.join(" ")),
    )
    .unwrap();
    std::fs::write(
        root.join("812/cmdline"),
        b"/usr/bin/tool\0space value\0\0third\0",
    )
    .unwrap();
    std::os::unix::fs::symlink(root.join("own-cwd"), root.join("812/cwd")).unwrap();
    std::os::unix::fs::symlink(root.join("own-file"), root.join("812/fd/3")).unwrap();
    let p = ProcFs::new(root.clone());
    let v = p.snapshot();
    assert_eq!(v.len(), 1);
    assert_eq!((v[0].pid, v[0].ppid, v[0].start), (812, 1, 4321));
    assert_eq!(v[0].argv, ["/usr/bin/tool", "space value", "third"]);
    assert_eq!(v[0].cwd, Some(root.join("own-cwd")));
    let aliases = std::collections::HashMap::from([("tool".to_owned(), "canonical".to_owned())]);
    let agents = std::collections::BTreeSet::from(["different-canonical".to_owned()]);
    let old = comandos_runtime::agent_procs::agent_procs(&root, &aliases).unwrap();
    assert_eq!(
        comandos_runtime::agent_procs::agent_procs_for_agents(&root, &aliases, &agents).unwrap(),
        old
    );
    assert_eq!(old[0].agent, "canonical");
    assert_eq!(p.open_files(812), vec![root.join("own-file")]);
    assert!(p.open_files(-1).is_empty());
    assert_eq!(
        comandos_runtime::session_configuration::server_start(&root, "812").unwrap(),
        "4321"
    );
    assert_eq!(
        comandos_runtime::session_configuration::server_start(&root, "999").unwrap(),
        ""
    );
}
#[test]
fn ps_parser_matches_plan_and_rejects_incomplete_or_impossible_rows() {
    let p = parse_ps_line(" 812     1 Sat Oct  4 21:10:03 2026 /usr/bin/tmux -L x").unwrap();
    assert_eq!((p.pid, p.ppid), (812, 1));
    assert_eq!(p.argv, ["/usr/bin/tmux", "-L", "x"]);
    assert_eq!(p.start, 1791148203);
    assert_eq!(p.cwd, None);
    for bad in [
        "812 1 broken",
        "-1 1 Sat Oct 4 21:10:03 2026 tool",
        "812 1 Sat Feb 30 21:10:03 2026 tool",
        "812 1 Sat Oct 4 25:10:03 2026 tool",
        "812 1 Sat Oct 4 21:10:03 2026",
    ] {
        assert!(parse_ps_line(bad).is_none(), "{bad}");
    }
}
#[derive(Default)]
struct Fake {
    calls: RefCell<Vec<(String, Vec<String>, Duration)>>,
}
impl Tools for Fake {
    fn run(&self, name: &str, args: &[&str], timeout: Duration) -> Option<String> {
        self.calls.borrow_mut().push((
            name.into(),
            args.iter().map(|s| s.to_string()).collect(),
            timeout,
        ));
        match (name, args) {
            ("ps", _) => {
                Some("812 1 Sat Oct 4 21:10:03 2026 /usr/bin/tmux -L x\ninvalid row\n".into())
            }
            ("lsof", ["-a", "-d", "cwd", "-p", "812", "-Fn"]) => {
                Some("p812\nfcwd\nn/own/cwd\n".into())
            }
            ("lsof", ["-Fn", "-p", "812"]) => {
                Some("p812\nfcwd\nn/own/cwd\nf3\nn/own/file with spaces\nnTCP loopback\n".into())
            }
            _ => None,
        }
    }
}
#[test]
fn darwin_source_uses_injected_ps_and_lsof_with_finite_deadlines() {
    let p = PsSource::new(Fake::default());
    let v = p.snapshot_with_cwd();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].cwd, Some(PathBuf::from("/own/cwd")));
    assert_eq!(
        p.open_files(812),
        vec![
            PathBuf::from("/own/cwd"),
            PathBuf::from("/own/file with spaces")
        ]
    );
    assert!(p.open_files(0).is_empty());
    let calls = p.tools().calls.borrow();
    assert_eq!(calls[0].1, ["-axww", "-o", "pid=,ppid=,lstart=,command="]);
    assert_eq!(calls[2].2, Duration::from_secs(2));
    assert!(calls.iter().all(|(_, _, d)| *d <= Duration::from_secs(5)));
}
#[test]
fn owned_child_poll_does_not_reap_before_group_cleanup() {
    use std::{
        os::unix::process::CommandExt,
        process::{Command, Stdio},
    };
    let mut c = Command::new(env!("CARGO_BIN_EXE_fake-proc-worker"))
        .arg("exit7")
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let until = std::time::Instant::now() + Duration::from_secs(2);
    while !comandos_runtime::procs::child_exited_unreaped(&c).unwrap() {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(comandos_runtime::procs::child_exited_unreaped(&c).unwrap());
    assert_eq!(c.wait().unwrap().code(), Some(7));
    assert!(comandos_runtime::procs::child_exited_unreaped(&c).is_err());
}

struct Fixture(PathBuf);
impl Fixture {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "m1-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        Self(root)
    }
    fn tool(&self, mode: &str) -> comandos_runtime::procs::macos::NativeTools {
        let path = self.0.join(mode);
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_fake-proc-worker"), &path).unwrap();
        comandos_runtime::procs::macos::NativeTools {
            ps: path.clone(),
            lsof: path,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn mac_agent_selection_matches_extracted_python_branch() {
    // Compile only the real function AST with inert collaborators; never import
    // cc-dash or execute its startup, real ps/lsof, or process inventory.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let code = r#"
import ast,json,os,sys,types
source=open(sys.argv[1]).read(); tree=ast.parse(source)
fn=next(n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name=='agent_procs')
ps='812 /usr/bin/node /usr/bin/grok\n813 /usr/bin/grok /usr/bin/claude\n814 /usr/bin/node /opt/grok-wrapper\n815 /usr/bin/node /opt/unknown /usr/bin/claude\n816 /usr/bin/grok\n'
lsof='p812\nn/own/a\np813\nn/own/b\np814\nn/own/c\np815\nn/own/d\n'
path=types.SimpleNamespace(isdir=lambda _:False,basename=os.path.basename)
ns={'os':types.SimpleNamespace(path=path),'agent_set':lambda:{'claude','grok'},'agent_process_aliases':lambda:{'claude':'claude','grok':'grok','grok-wrapper':'grok'},'subprocess':types.SimpleNamespace(run=lambda argv,**kw:types.SimpleNamespace(stdout=ps if argv[0]=='ps' else lsof))}
exec(compile(ast.Module(body=[fn],type_ignores=[]),sys.argv[1],'exec'),ns)
print(json.dumps(ns['agent_procs']()))
"#;
    let out = comandos_oracle::oracle_at(
        &root.join("tests/golden"),
        "runtime-mac-agent-selection",
        &serde_json::json!({"source_commit":"2674f366bb01b9db42f6f64b728929b3acfe8b83",
            "source":include_str!("oracle-src/agent_procs.py"), "script":code}),
        || {
            let out = std::process::Command::new(
                std::env::var("COMANDOS_RUNTIME_ORACLE_PYTHON")
                    .unwrap_or_else(|_| "python3".into()),
            )
            .args(["-c", code])
            .arg(root.join("tests/oracle-src/agent_procs.py"))
            .output()
            .map_err(|e| e.to_string())?;
            if out.status.success() {
                Ok(out.stdout)
            } else {
                Err(String::from_utf8_lossy(&out.stderr).into_owned())
            }
        },
    );
    let expected: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let agents =
        comandos_runtime::providers::agent_set(Some("claude grok"), &serde_json::json!({}));
    let aliases = comandos_runtime::providers::process_aliases(
        &agents,
        &serde_json::json!({"harnesses":{"grok":{"processNames":["claude","grok-wrapper"]}}}),
    );
    assert_eq!(aliases["claude"], "grok");
    let rows = [
        (812, "node grok", Some("/own/a")),
        (813, "grok claude", Some("/own/b")),
        (814, "node grok-wrapper", Some("/own/c")),
        (815, "node unknown claude", Some("/own/d")),
        (816, "grok", None),
    ]
    .map(|(pid, cmd, cwd)| comandos_runtime::procs::ProcInfo {
        pid,
        ppid: 1,
        start: 1,
        argv: cmd
            .split_whitespace()
            .map(|s| format!("/usr/bin/{s}"))
            .collect(),
        cwd: cwd.map(PathBuf::from),
    });
    let actual: Vec<_> = comandos_runtime::agent_procs::agents_from_snapshot(&rows, &agents)
        .iter()
        .map(|p| serde_json::json!([p.pid, p.cwd, p.agent]))
        .collect();
    assert_eq!(serde_json::Value::from(actual), expected);
}

#[test]
fn native_tools_bound_deadlines_and_output_and_release_owned_group() {
    let f = Fixture::new("tools-timeout");
    let t = f.tool("timeout");
    let begin = std::time::Instant::now();
    assert!(t.run("ps", &[], Duration::from_millis(40)).is_none());
    assert!(begin.elapsed() < Duration::from_secs(2));
    assert!(t.run("unknown", &[], Duration::from_secs(1)).is_none());
    let f = Fixture::new("tools-cap");
    let t = f.tool("output-cap");
    let begin = std::time::Instant::now();
    assert!(t.run("ps", &[], Duration::from_secs(5)).is_none());
    assert!(begin.elapsed() < Duration::from_secs(3));
    let f = Fixture::new("tools-group");
    let t = f.tool("group");
    assert_eq!(
        t.run("ps", &[], Duration::from_secs(2)).as_deref(),
        Some("C=C TZ=UTC\n")
    );
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        !f.0.join("survived").exists(),
        "descendant survived normal completion"
    );
}

#[test]
fn partial_tool_stdout_is_retained_like_python_even_with_nonzero_exit() {
    let f = Fixture::new("tools-partial");
    let t = f.tool("partial");
    assert_eq!(
        t.run("lsof", &[], Duration::from_secs(1)).as_deref(),
        Some("p812\nn/own/cwd\n")
    );
}

#[test]
fn targeted_identity_read_uses_one_ps_without_lsof_or_host_scan() {
    let source = PsSource::new(Fake::default());
    let p = source.process(812).unwrap();
    assert_eq!((p.pid, p.ppid, p.start), (812, 1, 1791148203));
    assert_eq!(p.cwd, None);
    assert_eq!(source.tools().calls.borrow().len(), 1);
    assert_eq!(
        source.tools().calls.borrow()[0].1,
        ["-ww", "-p", "812", "-o", "pid=,ppid=,lstart=,command="]
    );
    assert!(source.process(-1).is_none());
}

#[test]
fn inspector_consumes_injected_darwin_children_files_and_start_without_resume_flags() {
    use comandos_runtime::{
        pane_snapshot::{PaneInspector, PaneRef},
        procs::ProcInfo,
    };
    use std::sync::{Arc, Mutex};
    struct Source {
        rows: Vec<ProcInfo>,
        file: PathBuf,
        calls: Arc<Mutex<Vec<i32>>>,
    }
    impl ProcSource for Source {
        fn snapshot(&self) -> Vec<ProcInfo> {
            self.rows.clone()
        }
        fn open_files(&self, pid: i32) -> Vec<PathBuf> {
            self.calls.lock().unwrap().push(pid);
            vec![self.file.clone()]
        }
    }
    let f = Fixture::new("inspector");
    let id = "0f0f0f0f-0000-4000-8000-000000000001";
    let file = f.0.join(format!("rollout-2026-{id}.jsonl"));
    std::fs::write(
        &file,
        serde_json::json!({"type":"session_meta","payload":{"id":id,"source":"cli"}}).to_string()
            + "\n",
    )
    .unwrap();
    std::fs::create_dir_all(f.0.join(".claude/hooks/native-processes")).unwrap();
    let row = |pid, ppid, exe: &str| ProcInfo {
        pid,
        ppid,
        start: 100,
        argv: vec![exe.into(), "--model".into(), "space".into(), "value".into()],
        cwd: None,
    };
    let rows = vec![
        row(812, 1, "/usr/bin/zsh"),
        row(813, 812, "/usr/bin/codex"),
        row(814, 1, "/usr/bin/opencode"),
    ];
    let calls = Arc::new(Mutex::new(Vec::new()));
    let i = PaneInspector::with_source(
        &f.0,
        &f.0.join("fake-proc"),
        Box::new(Source {
            rows,
            file,
            calls: calls.clone(),
        }),
    )
    .unwrap();
    let snap = i
        .inspect(&PaneRef {
            id: "%1",
            pid: 812,
            command: "codex",
        })
        .unwrap();
    assert_eq!(snap["agent"], "codex");
    assert_eq!(snap["resume_id"], id);
    assert_eq!(*calls.lock().unwrap(), vec![813]);
    assert!(
        i.flags(813).is_empty(),
        "lossy ps boundaries must never become resume flags"
    );
    let metadata = serde_json::json!({"pid":814,"start":"100","harness":"opencode","sessionId":"session_1","parentId":"","busy":true,"updatedAt":1});
    let path = f.0.join(".claude/hooks/native-processes/814.json");
    std::fs::write(&path, metadata.to_string()).unwrap();
    assert_eq!(
        i.native_metadata(814, "opencode").unwrap()["sessionId"],
        "session_1"
    );
    let mut wrong = metadata;
    wrong["start"] = serde_json::json!("99");
    std::fs::write(path, wrong.to_string()).unwrap();
    assert!(i.native_metadata(814, "opencode").unwrap().is_empty());
    // Default Linux/fake-root path still reads procfs and cannot adopt the
    // injected inventory or the standalone open pathname.
    let linux = PaneInspector::new(&f.0, &f.0.join("fake-proc")).unwrap();
    assert_eq!(
        linux
            .inspect(&PaneRef {
                id: "%1",
                pid: 812,
                command: "zsh"
            })
            .unwrap()["agent"],
        ""
    );
}

#[test]
fn runtime_acp_close_retains_group_ownership_with_live_or_exited_leader() {
    use comandos_runtime::acp_client::{OpenOptions, Session};
    for exits in [false, true] {
        let f = Fixture::new(if exits {
            "runtime-acp-exited"
        } else {
            "runtime-acp-live"
        });
        let t = f.tool(if exits { "acp-exit" } else { "acp-live" });
        let opts = OpenOptions {
            model: String::new(),
            extra_env: vec![],
            search_path: Some("/usr/bin:/bin".into()),
            home: f.0.clone(),
            base_env: Some(vec![
                ("HOME".into(), f.0.as_os_str().to_owned()),
                ("PATH".into(), "/usr/bin:/bin".into()),
            ]),
        };
        let mut session =
            Session::open(&serde_json::json!({"command":[t.ps]}), &f.0, &opts).unwrap();
        let until = std::time::Instant::now() + Duration::from_secs(2);
        while !f.0.join("ready").exists() {
            assert!(std::time::Instant::now() < until);
            std::thread::sleep(Duration::from_millis(2));
        }
        if exits {
            std::thread::sleep(Duration::from_millis(30));
        }
        session.close();
        session.close();
        drop(session);
        std::thread::sleep(Duration::from_millis(600));
        assert!(
            !f.0.join("survived").exists(),
            "runtime ACP owned descendant survived closure (leader exited: {exits})"
        );
    }
}

#[test]
fn plain_darwin_inventory_avoids_unused_cwd_job() {
    let p = PsSource::new(Fake::default());
    let rows = p.snapshot();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cwd, None);
    assert_eq!(p.tools().calls.borrow().len(), 1);
    assert_eq!(p.tools().calls.borrow()[0].0, "ps");
}

#[derive(Default)]
struct CandidateTools {
    ps: String,
    lsof: Option<String>,
    calls: RefCell<Vec<(String, Vec<String>, Duration)>>,
}
impl Tools for CandidateTools {
    fn run(&self, name: &str, args: &[&str], timeout: Duration) -> Option<String> {
        self.calls.borrow_mut().push((
            name.into(),
            args.iter().map(|s| (*s).to_owned()).collect(),
            timeout,
        ));
        match name {
            "ps" => Some(self.ps.clone()),
            "lsof" => self.lsof.clone(),
            _ => panic!("unexpected fixture command"),
        }
    }
}
fn candidate_tools() -> CandidateTools {
    CandidateTools {
        ps: "700 1 Sat Oct 4 21:10:03 2026 /usr/bin/unrelated\n812 1 Sat Oct 4 21:10:03 2026 /usr/bin/node /opt/claude\n813 1 Sat Oct 4 21:10:03 2026 /opt/grok /opt/claude\n814 1 Sat Oct 4 21:10:03 2026 /usr/bin/node /opt/grok-wrapper\n815 1 Sat Oct 4 21:10:03 2026 /usr/bin/node /opt/other /opt/claude\n816 1 Sat Oct 4 21:10:03 2026 /opt/grok\n".into(),
        lsof: Some("p812\nn/own/a\np813\nn/own/b\np700\nn/ignored\n".into()),
        ..Default::default()
    }
}
#[test]
fn candidate_cwd_lookup_filters_before_one_lsof_with_the_shared_canonical_selector() {
    let source = PsSource::new(candidate_tools());
    let agents = std::collections::BTreeSet::from(["claude".into(), "grok".into()]);
    let rows = comandos_runtime::agent_procs::agents_from_source(&source, &agents);
    assert_eq!(
        rows.iter()
            .map(|r| (r.pid, r.cwd.as_str(), r.agent.as_str()))
            .collect::<Vec<_>>(),
        [(812, "/own/a", "claude"), (813, "/own/b", "claude")]
    );
    let calls = source.tools().calls.borrow();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "ps");
    assert_eq!(calls[0].1, ["-axww", "-o", "pid=,ppid=,lstart=,command="]);
    assert_eq!(calls[1].0, "lsof");
    assert_eq!(calls[1].1, ["-a", "-d", "cwd", "-p", "812,813,816", "-Fn"]);
    assert_eq!(calls[1].2, Duration::from_secs(2));
}
#[test]
fn candidate_cwd_lookup_without_a_canonical_hit_skips_lsof() {
    let source = PsSource::new(candidate_tools());
    let agents = std::collections::BTreeSet::from(["not-a-candidate".into()]);
    assert!(comandos_runtime::agent_procs::agents_from_source(&source, &agents).is_empty());
    let calls = source.tools().calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "ps");
}
#[test]
fn candidate_cwd_lookup_failure_returns_no_attribution_without_extra_jobs() {
    let mut tools = candidate_tools();
    tools.lsof = None;
    let source = PsSource::new(tools);
    let agents = std::collections::BTreeSet::from(["claude".into(), "grok".into()]);
    assert!(comandos_runtime::agent_procs::agents_from_source(&source, &agents).is_empty());
    assert_eq!(source.tools().calls.borrow().len(), 2);
}
