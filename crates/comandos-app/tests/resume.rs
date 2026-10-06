#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]
use comandos_app::resume::{exact_resume_command, resume_command, sane_flags};
use serde_json::json;

#[test]
fn sane_flags_keep_codex_config_c_and_drop_resume_ids() {
    assert_eq!(
        sane_flags(
            &json!([
                "-c",
                "model_provider=openai",
                "--resume",
                "old",
                "--last",
                "--model",
                "gpt-5"
            ]),
            "codex"
        ),
        vec!["-c", "model_provider=openai", "--model", "gpt-5"]
    );
}

#[test]
fn exact_resume_never_uses_last_or_continue() {
    assert!(exact_resume_command(&json!({"agent":"claude"})).is_none());
    assert_eq!(
        resume_command(&json!({"agent":"codex","resume_id":"12345678-1234-1234-1234-123456789abc","flags":["-c","x=y"]})).unwrap(),
        "codex resume 12345678-1234-1234-1234-123456789abc -c x=y"
    );
}

#[test]
fn resume_matches_python_oracle_with_private_transcripts_and_accounts() {
    use comandos_app::proc::{ProcSpec, run};
    use std::{os::unix::fs::PermissionsExt, time::Duration};
    let root = std::env::temp_dir().join(format!("comandos-resume-oracle-{}", std::process::id()));
    let home = root.join("home");
    let bins = home.join(".local/bin");
    std::fs::create_dir_all(&bins).unwrap();
    for cli in ["claude", "codex", "grok", "cc-acp"] {
        let p = bins.join(cli);
        std::fs::write(&p, b"#!/bin/sh\nexit 91\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let id = "12345678-1234-1234-1234-123456789abc";
    let claude = root.join("account with ' 日本");
    std::fs::create_dir_all(claude.join("projects/project")).unwrap();
    std::fs::write(claude.join(format!("projects/project/{id}.jsonl")), b"{}").unwrap();
    let codex = home.join(".codex");
    std::fs::create_dir_all(codex.join("sessions/2026/10/05")).unwrap();
    std::fs::write(
        codex.join(format!("sessions/2026/10/05/rollout-fixture-{id}.jsonl")),
        b"{}",
    )
    .unwrap();
    let cases = serde_json::json!([
        {"agent":"claude","claude_config_dir":claude,"resume_id":id,"flags":["--model","模型","--effort","high","--resume","old","--settings",""]},
        {"agent":"codex","resume_id":id,"flags":["-c","name='日本'","--last","-m","gpt-fixture"]},
        {"agent":"codex","resume_id":"87654321-4321-4321-4321-cba987654321"},
        {"agent":"claude","resume_id":"invalid","flags":["--model","--effort"]},
        {"agent":"acp","acp":{"agent":"codex","sessionId":"fixture-session","model":"model with space","account":"main"}}
    ]);
    let script = r#"import ast,json,sys,os,glob,re,shlex,shutil
source=ast.parse(open(sys.argv[1]).read())
names={'_sane_flags','_which_cli','resume_command','exact_resume_command'}
nodes=[n for n in source.body if isinstance(n,ast.FunctionDef) and n.name in names or isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id in {'_RESUME_SKIP_FLAGS','_VALUE_FLAGS'} for t in n.targets)]
GROK_HOME_DEFAULT=os.path.expanduser('~/.grok')
exec(compile(ast.Module(body=nodes,type_ignores=[]),sys.argv[1],'exec'))
print(json.dumps([exact_resume_command(case) for case in json.load(sys.stdin)]))
"#;
    let output = run(&ProcSpec {
        program: "python3".into(),
        args: vec![
            "-c".into(),
            script.into(),
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/cc-app").into(),
        ],
        stdin: Some(serde_json::to_vec(&cases).unwrap()),
        env: vec![
            ("HOME".into(), home.clone().into_os_string()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ],
        clear_env: true,
        env_remove: vec![],
        cwd: Some(root.clone()),
        timeout: Duration::from_secs(5),
    })
    .unwrap();
    assert_eq!(
        output.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let actual: Vec<_> = cases
        .as_array()
        .unwrap()
        .iter()
        .map(|case| comandos_app::resume::verified_resume_command(case, &home))
        .collect();
    assert_eq!(serde_json::json!(actual), expected);
    std::fs::remove_dir_all(root).unwrap();
}
