use comandos_extensions::output_schema::{Validation, validate_with};
use serde_json::json;
use std::{path::Path, time::Duration};
use tokio::time::Instant;
#[tokio::test(flavor = "current_thread")]
async fn required_integer_property_is_validated() {
    let exe = Path::new(env!("CARGO_BIN_EXE_comandos-extensions"));
    let schema = json!({"type":"object","properties":{"n":{"type":"integer"}},"required":["n"]});
    assert_eq!(
        validate_with(
            exe,
            schema.clone(),
            json!({"n":1}),
            Instant::now() + Duration::from_secs(5)
        )
        .await,
        Ok(Validation::Valid)
    );
    for content in [json!({}), json!({"n":"wrong"})] {
        assert_eq!(
            validate_with(
                exe,
                schema.clone(),
                content,
                Instant::now() + Duration::from_secs(5)
            )
            .await,
            Ok(Validation::InvalidContent)
        );
    }
}

async fn validate(
    schema: serde_json::Value,
    content: serde_json::Value,
) -> Result<Validation, comandos_extensions::output_schema::Failure> {
    validate_with(
        Path::new(env!("CARGO_BIN_EXE_comandos-extensions")),
        schema,
        content,
        Instant::now() + Duration::from_secs(5),
    )
    .await
}

#[tokio::test(flavor = "current_thread")]
async fn enums_arrays_and_local_references() {
    let schema = json!({"$defs":{"item":{"enum":[1,2]}},"properties":{"items":{"type":"array","items":{"$ref":"#/$defs/item"},"minItems":1}}});
    assert_eq!(
        validate(schema.clone(), json!({"items":[1,2]})).await,
        Ok(Validation::Valid)
    );
    assert_eq!(
        validate(schema.clone(), json!({"items":[3]})).await,
        Ok(Validation::InvalidContent)
    );
    assert_eq!(
        validate(schema, json!({"items":[]})).await,
        Ok(Validation::InvalidContent)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn known_drafts_and_disabled_instance_formats() {
    for draft in [
        "http://json-schema.org/draft-04/schema#",
        "http://json-schema.org/draft-06/schema#",
        "http://json-schema.org/draft-07/schema#",
        "https://json-schema.org/draft/2019-09/schema",
        "https://json-schema.org/draft/2020-12/schema",
    ] {
        assert_eq!(
            validate(
                json!({"$schema":draft,"properties":{"email":{"type":"string","format":"email"}}}),
                json!({"email":"not-an-email"})
            )
            .await,
            Ok(Validation::Valid)
        );
        let schema = json!({"$schema":draft,"properties":{"n":{"type":"integer"}}});
        assert_eq!(
            validate(schema, json!({"n":1.0})).await,
            Ok(if draft.contains("04") {
                Validation::InvalidContent
            } else {
                Validation::Valid
            })
        );
    }
    assert_eq!(validate(json!({"$schema":"http://json-schema.org/draft-07/schema#","if":{"required":["x"]},"then":{"required":["y"]}}),json!({"x":1})).await,Ok(Validation::InvalidContent));
    assert_eq!(validate(json!({"$schema":"https://json-schema.org/draft/2019-09/schema","unevaluatedProperties":false}),json!({"x":1})).await,Ok(Validation::InvalidContent));
    assert_eq!(
        validate(
            json!({"properties":{"a":{"prefixItems":[{"type":"integer"}],"items":false}}}),
            json!({"a":[1]})
        )
        .await,
        Ok(Validation::Valid)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn schema_checking_and_big_integer_fidelity() {
    for schema in [
        json!({"type":"unknown"}),
        json!({"maxLength":-1}),
        json!({"required":["x","x"]}),
    ] {
        assert_eq!(
            validate(schema, json!({})).await,
            Ok(Validation::InvalidSchema)
        );
    }
    let schema:serde_json::Value=serde_json::from_str(r#"{"properties":{"n":{"type":"integer","minimum":18446744073709551616,"maximum":18446744073709551616}}}"#).unwrap();
    assert_eq!(
        validate(
            schema.clone(),
            serde_json::from_str(r#"{"n":18446744073709551616}"#).unwrap()
        )
        .await,
        Ok(Validation::Valid)
    );
    assert_eq!(
        validate(
            schema,
            serde_json::from_str(r#"{"n":18446744073709551615}"#).unwrap()
        )
        .await,
        Ok(Validation::InvalidContent)
    );
}

use comandos_extensions::output_schema::Failure;
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};
static FIXTURE: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new(mode: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "schema-worker-{}-{}",
            std::process::id(),
            FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let code = format!(
            "#!/usr/bin/python3\nimport os, pathlib, time\npathlib.Path({:?}).write_text(str(os.getpid()))\n{}\n",
            dir.join("pid").to_str().unwrap(),
            match mode {
                "flood" => "os.write(1, b'x'*1000000); time.sleep(30)",
                "bad" => "print('untrusted-detail')",
                _ => "time.sleep(30)",
            }
        );
        fs::write(dir.join("worker"), code).unwrap();
        fs::set_permissions(dir.join("worker"), fs::Permissions::from_mode(0o700)).unwrap();
        Self(dir)
    }
    fn exe(&self) -> PathBuf {
        self.0.join("worker")
    }
    async fn pid(&self) -> u32 {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(s) = fs::read_to_string(self.0.join("pid"))
                    && let Ok(pid) = s.parse()
                {
                    return pid;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("worker did not start")
    }
    fn reaped(&self, pid: u32) {
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "child {pid} survived or remains zombie"
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn deadline_reaps_child_blocked_on_input_and_output() {
    for mode in ["hung", "flood"] {
        let fixture = Fixture::new(mode);
        let exe = fixture.exe();
        let future = tokio::spawn(async move {
            validate_with(
                &exe,
                json!({}),
                json!({"large":"a".repeat(512*1024)}),
                Instant::now() + Duration::from_millis(200),
            )
            .await
        });
        let pid = fixture.pid().await;
        assert_eq!(future.await.unwrap(), Err(Failure::Deadline));
        fixture.reaped(pid);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_reaps_owned_child() {
    let fixture = Fixture::new("hung");
    let exe = fixture.exe();
    let task = tokio::spawn(async move {
        validate_with(
            &exe,
            json!({}),
            json!({}),
            Instant::now() + Duration::from_secs(30),
        )
        .await
    });
    let pid = fixture.pid().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    fixture.reaped(pid);
}

#[test]
fn runtime_shutdown_reaps_child_with_blocked_stdout() {
    let fixture = Fixture::new("flood");
    let exe = fixture.exe();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.spawn(async move {
        validate_with(
            &exe,
            json!({}),
            json!({}),
            Instant::now() + Duration::from_secs(30),
        )
        .await
    });
    let pid = runtime.block_on(fixture.pid());
    drop(runtime);
    fixture.reaped(pid);
}

#[tokio::test(flavor = "current_thread")]
async fn four_workers_allow_healthy_async_progress() {
    let fixtures: Vec<_> = (0..4).map(|_| Fixture::new("hung")).collect();
    let tasks: Vec<_> = fixtures
        .iter()
        .map(|fixture| {
            let exe = fixture.exe();
            tokio::spawn(async move {
                validate_with(
                    &exe,
                    json!({}),
                    json!({}),
                    Instant::now() + Duration::from_millis(250),
                )
                .await
            })
        })
        .collect();
    let mut ticks = 0;
    for _ in 0..10 {
        tokio::time::sleep(Duration::from_millis(5)).await;
        ticks += 1;
    }
    assert_eq!(ticks, 10);
    assert!(tasks.iter().all(|task| !task.is_finished()));
    for (fixture, task) in fixtures.iter().zip(tasks) {
        let pid = fixture.pid().await;
        assert_eq!(task.await.unwrap(), Err(Failure::Deadline));
        fixture.reaped(pid);
    }
    assert_eq!(validate(json!({}), json!({})).await, Ok(Validation::Valid));
}

#[tokio::test(flavor = "current_thread")]
async fn expired_deadline_input_limits_and_unknown_status_are_safe() {
    let fixture = Fixture::new("bad");
    assert_eq!(
        validate_with(&fixture.exe(), json!({}), json!({}), Instant::now()).await,
        Err(Failure::Deadline)
    );
    assert!(!fixture.0.join("pid").exists());
    for (schema, content) in [
        (json!({"description":"s".repeat(8*1024*1024)}), json!({})),
        (json!({}), json!({"large":"x".repeat(8*1024*1024)})),
    ] {
        assert_eq!(
            validate_with(
                &fixture.exe(),
                schema,
                content,
                Instant::now() + Duration::from_secs(5)
            )
            .await,
            Err(Failure::InputLimit)
        );
        assert!(!fixture.0.join("pid").exists());
    }
    assert_eq!(
        validate_with(
            &fixture.exe(),
            json!({}),
            json!({}),
            Instant::now() + Duration::from_secs(5)
        )
        .await,
        Err(Failure::Execution)
    );
}

fn worker(input: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_comandos-extensions"))
        .arg("__schema_worker")
        .env_remove("HOME")
        .env_remove("XDG_CONFIG_HOME")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn native_worker_emits_only_constant_status_and_requires_objects() {
    for (input, expected) in [
        (r#"{"schema":{},"structuredContent":{}}"#, "valid\n"),
        (
            r#"{"schema":{"type":"secret-invalid-type"},"structuredContent":{"secret":"do-not-leak"}}"#,
            "invalid-schema\n",
        ),
        (
            r#"{"schema":{"properties":{"secret":{"type":"integer"}}},"structuredContent":{"secret":"do-not-leak"}}"#,
            "invalid-content\n",
        ),
        (
            r#"{"schema":{},"structuredContent":[]}"#,
            "execution-failure\n",
        ),
        (
            r#"{"schema":false,"structuredContent":{}}"#,
            "execution-failure\n",
        ),
        (
            r#"{"schema":{},"structuredContent":{},"extra":"secret"}"#,
            "execution-failure\n",
        ),
        ("bad-input-secret", "execution-failure\n"),
    ] {
        let result = worker(input);
        assert!(result.status.success());
        assert_eq!(result.stdout, expected.as_bytes());
        assert!(result.stderr.is_empty());
    }
}

#[test]
fn native_worker_sets_only_its_own_memory_cpu_and_core_budget() {
    let before = fs::read_to_string("/proc/self/limits").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_comandos-extensions"))
        .arg("__schema_worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let started = std::time::Instant::now();
    let limits = loop {
        if started.elapsed() > Duration::from_secs(2) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("worker resource budget was not applied");
        }
        let text = fs::read_to_string(format!("/proc/{}/limits", child.id())).unwrap();
        if text
            .lines()
            .any(|line| line.starts_with("Max cpu time") && line.contains("3"))
        {
            break text;
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"schema":{},"structuredContent":{}}"#)
        .unwrap();
    assert!(child.wait_with_output().unwrap().status.success());
    assert!(limits.lines().any(
        |line| line.starts_with("Max address space") && line.matches("402653184").count() == 2
    ));
    assert!(limits.lines().any(|line| line.starts_with("Max cpu time")
        && line.split_whitespace().collect::<Vec<_>>()[3..5] == ["2", "3"]));
    assert!(
        limits
            .lines()
            .any(|line| line.starts_with("Max core file size")
                && line.split_whitespace().collect::<Vec<_>>()[4..6] == ["0", "0"])
    );
    assert_eq!(fs::read_to_string("/proc/self/limits").unwrap(), before);
}

#[tokio::test(flavor = "current_thread")]
async fn external_references_never_touch_network_or_private_file() {
    use std::net::{TcpListener, TcpStream};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    // Confirm that this network marker can detect a real connection.
    let marker = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let _ = listener.accept().unwrap();
    drop(marker);
    let fixture = Fixture::new("hung");
    let secret = fixture.0.join("private.json");
    fs::write(&secret, r#"{"type":"object"}"#).unwrap();
    fs::set_permissions(&secret, fs::Permissions::from_mode(0o600)).unwrap();
    fs::File::open(&secret)
        .unwrap()
        .set_times(
            fs::FileTimes::new()
                .set_accessed(std::time::UNIX_EPOCH)
                .set_modified(std::time::UNIX_EPOCH),
        )
        .unwrap();
    for reference in [
        format!("http://{}/schema", listener.local_addr().unwrap()),
        format!("file://{}", secret.display()),
    ] {
        assert_eq!(
            validate(json!({"$ref":reference}), json!({})).await,
            Ok(Validation::InvalidSchema)
        );
    }
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(
        fs::metadata(&secret).unwrap().accessed().unwrap(),
        std::time::UNIX_EPOCH
    );
    // Reading the private valid schema would accept {}; offline failure proves
    // the worker did not use its contents, even though its permissions allow it.
    assert_eq!(fs::read_to_string(secret).unwrap(), r#"{"type":"object"}"#);
}
