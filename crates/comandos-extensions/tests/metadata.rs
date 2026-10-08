use comandos_extensions::metadata::{
    SkillMetadataCache, ToolListCapture, mcp_size, record_mcp_size, skill_origin,
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static SEQ: AtomicUsize = AtomicUsize::new(0);
fn home() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "metadata-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn complete_ordered_capture_rejects_cursor_cycles_and_recovers() {
    let mut c = ToolListCapture::default();
    assert!(c.add(Some("orphan"), None, &[]).is_none());
    assert!(c.add(None, Some("p2"), &[json!({"name":"a"})]).is_none());
    assert!(c.add(Some("wrong"), None, &[]).is_none());
    assert!(c.add(Some("p2"), None, &[]).is_none());
    assert_eq!(c.add(None, None, &[]), Some(vec![]));
    assert!(c.add(None, Some("p2"), &[]).is_none());
    assert!(c.add(Some("p2"), Some("p2"), &[]).is_none());
    assert!(c.add(Some("p2"), None, &[]).is_none());
    assert!(c.add(None, Some("p2"), &[json!({"name":"a"})]).is_none());
    assert_eq!(
        c.add(Some("p2"), None, &[json!({"name":"b"})]),
        Some(vec![json!({"name":"a"}), json!({"name":"b"})])
    );
}
#[test]
fn capture_rejects_oversized_expected_cursor_and_resets() {
    let mut capture = ToolListCapture::default();
    let cursor = "x".repeat(65_537);
    assert!(capture.add(None, Some(&cursor), &[]).is_none());
    assert!(capture.add(Some(&cursor), None, &[]).is_none());
    assert_eq!(capture.add(None, None, &[]), Some(vec![]));
}
#[test]
fn capture_bounds_consumed_and_expected_cursor_bytes_without_partial_rows() {
    let mut capture = ToolListCapture::default();
    let cursors: Vec<_> = (0..4)
        .map(|i| format!("{i}{}", "x".repeat(20_000)))
        .collect();
    let partial = json!({"name":"partial"});
    assert!(capture.add(None, Some(&cursors[0]), &[partial]).is_none());
    for pair in cursors.windows(2) {
        assert!(capture.add(Some(&pair[0]), Some(&pair[1]), &[]).is_none());
    }
    assert!(capture.add(Some(&cursors[3]), None, &[]).is_none());
    assert!(capture.add(None, Some("small"), &[]).is_none());
    let fresh = json!({"name":"fresh"});
    assert_eq!(
        capture.add(Some("small"), None, std::slice::from_ref(&fresh)),
        Some(vec![fresh])
    );
}
#[test]
fn capture_bounds_cursor_entry_count_for_empty_pages() {
    let mut capture = ToolListCapture::default();
    assert!(capture.add(None, Some("0"), &[]).is_none());
    for i in 0..1024 {
        assert!(
            capture
                .add(Some(&i.to_string()), Some(&(i + 1).to_string()), &[])
                .is_none()
        );
    }
    assert!(capture.add(Some("1024"), None, &[]).is_none());
    assert_eq!(capture.add(None, None, &[]), Some(vec![]));
}
#[test]
fn capture_accepts_cursor_byte_and_entry_limit_boundaries() {
    let mut capture = ToolListCapture::default();
    let cursor = "x".repeat(65_536);
    assert!(capture.add(None, Some(&cursor), &[]).is_none());
    assert_eq!(capture.add(Some(&cursor), None, &[]), Some(vec![]));
    assert!(capture.add(None, Some("0"), &[]).is_none());
    for i in 0..1023 {
        assert!(
            capture
                .add(Some(&i.to_string()), Some(&(i + 1).to_string()), &[])
                .is_none()
        );
    }
    assert_eq!(capture.add(Some("1023"), None, &[]), Some(vec![]));
}
#[test]
fn provenance_is_bound_to_install_path_and_plugin_wins() {
    let h = home();
    let root = h.join(".agents/skills/demo");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("SKILL.md"), "hello").unwrap();
    std::fs::write(
        h.join(".agents/.skill-lock.json"),
        r#"{"skills":{"demo":{"source":"owner/repo","sourceType":"github"}}}"#,
    )
    .unwrap();
    let row = json!({"path":root.join("SKILL.md")});
    assert_eq!(skill_origin(&row, &h)["id"], "github:owner/repo");
    std::os::unix::fs::symlink(&root, h.join("alias")).unwrap();
    assert_eq!(
        skill_origin(&json!({"path":h.join("alias/SKILL.md")}), &h),
        skill_origin(&row, &h)
    );
    assert_eq!(
        skill_origin(&json!({"path":h.join("project/demo/SKILL.md")}), &h)["kind"],
        "unknown"
    );
    assert_eq!(
        skill_origin(&json!({"plugin":"official@market","path":root}), &h)["id"],
        "plugin:official@market"
    );
    std::fs::write(
        h.join(".agents/.skill-lock.json"),
        r#"{"skills":{"demo":[]}}"#,
    )
    .unwrap();
    assert_eq!(skill_origin(&row, &h)["kind"], "unknown");
    std::fs::remove_dir_all(h).unwrap();
}
#[tokio::test]
async fn measurement_reuses_cache_and_invalidates_schema_configuration_and_age() {
    let h = home();
    let spec = json!({"url":"https://example.test","secret":"not-persisted"});
    let tools = vec![
        json!({"name":"b","inputSchema":{}}),
        json!({"name":"a","description":"á😀"}),
    ];
    let calls = AtomicUsize::new(0);
    let counter = |texts: Vec<String>| {
        calls.fetch_add(1, Ordering::Relaxed);
        async move { vec![Some(texts[0].chars().count() as u64)] }
    };
    record_mcp_size(&h, "test", &spec, &tools, 1000.0, &counter).await;
    let first = mcp_size(&h, "test", &spec, 1000.0);
    assert!(first["tokens"].as_u64().unwrap() > 0);
    record_mcp_size(
        &h,
        "test",
        &spec,
        &tools.iter().rev().cloned().collect::<Vec<_>>(),
        1001.0,
        &counter,
    )
    .await;
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(mcp_size(&h, "test", &json!({}), 1001.0)["tokens"].is_null());
    assert!(mcp_size(&h, "test", &spec, 999.0)["tokens"].is_null());
    assert!(mcp_size(&h, "test", &spec, 87400.0)["tokens"].is_null());
    record_mcp_size(
        &h,
        "test",
        &spec,
        &[json!({"name":"changed"})],
        1002.0,
        &counter,
    )
    .await;
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    let dir = h.join(".local/state/comandos/extensions/sizes");
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|s| s == "json") {
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(!text.contains("not-persisted"));
            assert!(!text.contains("inputSchema"));
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            let mut value: Value = serde_json::from_str(&text).unwrap();
            for bad in [json!(true), json!(1.0), json!(-1), Value::Null] {
                value["tokens"] = bad;
                std::fs::write(&path, value.to_string()).unwrap();
                assert!(mcp_size(&h, "test", &spec, 1002.0)["tokens"].is_null());
            }
        }
    }
    std::fs::remove_dir_all(h).unwrap();
}
#[tokio::test]
async fn skill_cache_handles_failures_and_file_replacement() {
    let h = home();
    let p = h.join("SKILL.md");
    std::fs::write(&p, "hello world").unwrap();
    let rows = vec![
        json!({"id":"ok","path":p}),
        json!({"id":"missing","path":h.join("missing")}),
        json!({"id":"group","group":true,"path":p}),
    ];
    let mut cache = SkillMetadataCache::default();
    let calls = AtomicUsize::new(0);
    let counter = |texts: Vec<String>| {
        calls.fetch_add(1, Ordering::Relaxed);
        async move { texts.iter().map(|_| Some(2)).collect() }
    };
    let result = cache.measure(&rows, &h, 0.0, &counter).await;
    assert_eq!(result["ok"]["size"]["tokens"], 2);
    assert!(result["missing"]["size"]["tokens"].is_null());
    assert!(result["group"]["size"]["tokens"].is_null());
    cache.measure(&rows, &h, 1.0, &counter).await;
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    std::fs::write(h.join("new"), "different").unwrap();
    std::fs::rename(h.join("new"), &p).unwrap();
    cache.measure(&rows, &h, 2.0, &counter).await;
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    std::fs::write(&p, [0xff]).unwrap();
    assert!(cache.measure(&rows, &h, 3.0, &counter).await["ok"]["size"]["tokens"].is_null());
    std::fs::remove_dir_all(h).unwrap();
}
#[tokio::test]
async fn oracle_schema_bytes_hashes_and_count_match_python() {
    let fixture = comandos_core::json::parse_value(include_str!("fixtures/metadata.json")).unwrap();
    let h = home();
    let tools = fixture["tools"].as_array().unwrap();
    assert_eq!(
        comandos_extensions::python_json::default_len(&fixture["tools"]).unwrap(),
        fixture["default_len"].as_u64().unwrap() as usize
    );
    record_mcp_size(
        &h,
        fixture["name"].as_str().unwrap(),
        &fixture["spec"],
        tools,
        1234.0,
        &|texts: Vec<String>| {
            assert_eq!(texts[0], fixture["definitions_text"].as_str().unwrap());
            let count = fixture["tokens"].as_u64().unwrap();
            async move { vec![Some(count)] }
        },
    )
    .await;
    let path = h
        .join(".local/state/comandos/extensions/sizes")
        .join(format!("{}.json", fixture["name_digest"].as_str().unwrap()));
    let stored = comandos_core::json::parse_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(stored["content"], fixture["content"]);
    assert_eq!(stored["configuration"], fixture["configuration"]);
    std::fs::remove_dir_all(h).unwrap();
}
fn public_encoding() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.migration-build/public-token-cache")
        .join(comandos_extensions::tokenizer::ENCODING_FILE)
}
fn warm(h: &std::path::Path) {
    let dir = h.join(".cache/comandos/tiktoken");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(
        public_encoding(),
        dir.join(comandos_extensions::tokenizer::ENCODING_FILE),
    )
    .expect(
        "Run metadata_oracle.py --write inside oracle sandbox to prepare readonly public fixture",
    );
}
#[tokio::test]
async fn transient_counter_matches_python_and_corrupt_cache_is_unknown() {
    let h = home();
    let fixture = comandos_core::json::parse_value(include_str!("fixtures/metadata.json")).unwrap();
    let texts: Vec<_> = fixture["texts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["text"].as_str().unwrap().to_owned())
        .collect();
    let expected: Vec<_> = fixture["texts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["tokens"].as_u64())
        .collect();
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_comandos-extensions"));
    assert_eq!(
        comandos_extensions::tokenizer::isolated_token_counts_with(
            exe,
            &h,
            texts.clone(),
            std::time::Duration::from_secs(8)
        )
        .await,
        vec![None; texts.len()]
    );
    // La codificación pública vive en `.migration-build/` (ignorado por git):
    // en un checkout recién clonado no existe. Sin ella solo se prueba la
    // parte en frío; con ella la prueba es completa, como siempre.
    if !public_encoding().is_file() {
        eprintln!(
            "SALTADA la parte en caliente: falta {}. Ejecuta metadata_oracle.py --write \
             dentro del sandbox del oráculo para prepararla.",
            public_encoding().display()
        );
        std::fs::remove_dir_all(h).unwrap();
        return;
    }
    warm(&h);
    assert_eq!(
        comandos_extensions::tokenizer::isolated_token_counts_with(
            exe,
            &h,
            texts.clone(),
            std::time::Duration::from_secs(8)
        )
        .await,
        expected
    );
    std::fs::write(
        h.join(".cache/comandos/tiktoken")
            .join(comandos_extensions::tokenizer::ENCODING_FILE),
        "corrupt",
    )
    .unwrap();
    assert_eq!(
        comandos_extensions::tokenizer::isolated_token_counts_with(
            exe,
            &h,
            texts.clone(),
            std::time::Duration::from_secs(8)
        )
        .await,
        vec![None; texts.len()]
    );
    std::fs::remove_dir_all(h).unwrap();
}
#[tokio::test]
async fn timeout_cancellation_and_bad_output_reap_children() {
    use comandos_extensions::tokenizer::isolated_token_counts_with;
    use std::os::unix::fs::PermissionsExt;
    let h = home();
    let helper = h.join("helper");
    let pidfile = h.join("pid");
    let content = format!(
        "#!/bin/sh\necho $$ > '{}'\nexec /bin/sleep 60\n",
        pidfile.display()
    );
    std::fs::write(&helper, content).unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        isolated_token_counts_with(
            &helper,
            &h,
            vec!["test".into()],
            std::time::Duration::from_millis(80)
        )
        .await,
        vec![None]
    );
    let pid = std::fs::read_to_string(&pidfile).unwrap();
    assert!(!std::path::Path::new(&format!("/proc/{}", pid.trim())).exists());
    let exe = helper.clone();
    let home2 = h.clone();
    let job = tokio::spawn(async move {
        isolated_token_counts_with(
            &exe,
            &home2,
            vec!["test".into()],
            std::time::Duration::from_secs(8),
        )
        .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    job.abort();
    let _ = job.await;
    let pid = std::fs::read_to_string(&pidfile).unwrap();
    assert!(!std::path::Path::new(&format!("/proc/{}", pid.trim())).exists());
    for output in ["[true]", "[1.0]", "[-1]", "[1,2]", "not json"] {
        std::fs::write(
            &helper,
            format!("#!/bin/sh\ncat >/dev/null\nprintf '%s' '{}'\n", output),
        )
        .unwrap();
        assert_eq!(
            isolated_token_counts_with(
                &helper,
                &h,
                vec!["test".into()],
                std::time::Duration::from_secs(1)
            )
            .await,
            vec![None]
        );
    }
    std::fs::remove_dir_all(h).unwrap();
}
#[tokio::test]
async fn simultaneous_measurements_count_once_and_rotate_once() {
    let h = home();
    let spec = json!({"url":"https://fixture.test"});
    let tools = [json!({"name":"same"})];
    let calls = AtomicUsize::new(0);
    let counter = |_: Vec<String>| {
        calls.fetch_add(1, Ordering::Relaxed);
        async {
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
            vec![Some(7)]
        }
    };
    tokio::join!(
        record_mcp_size(&h, "demo", &spec, &tools, 1000.0, &counter),
        record_mcp_size(&h, "demo", &spec, &tools, 1000.0, &counter),
        record_mcp_size(&h, "demo", &spec, &tools, 1000.0, &counter)
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let tools = [json!({"name":"rotated"})];
    tokio::join!(
        record_mcp_size(&h, "demo", &spec, &tools, 1000.0, &counter),
        record_mcp_size(&h, "demo", &spec, &tools, 1000.0, &counter)
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    let saved = mcp_size(&h, "demo", &spec, 1000.0);
    record_mcp_size(
        &h,
        "demo",
        &spec,
        &[json!({"name":"failed"})],
        1001.0,
        &|_| async { vec![None] },
    )
    .await;
    assert_eq!(mcp_size(&h, "demo", &spec, 1001.0), saved);
    std::fs::remove_dir_all(h).unwrap();
}
#[tokio::test]
async fn failed_skill_counts_retry_after_thirty_seconds_and_caches_are_bounded() {
    let h = home();
    let p = h.join("SKILL.md");
    std::fs::write(&p, "hello").unwrap();
    let rows = [json!({"id":"x","path":p})];
    let mut cache = SkillMetadataCache::default();
    let calls = AtomicUsize::new(0);
    let counter = |texts: Vec<String>| {
        calls.fetch_add(1, Ordering::Relaxed);
        async move { vec![None; texts.len()] }
    };
    cache.measure(&rows, &h, 0.0, &counter).await;
    cache.measure(&rows, &h, 29.99, &counter).await;
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    cache.measure(&rows, &h, 30.0, &counter).await;
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    let mut many = Vec::new();
    for i in 0..1030 {
        let p = h.join(format!("skill-{i}"));
        std::fs::write(&p, format!("unique content {i}")).unwrap();
        many.push(json!({"id":format!("{i}"),"path":p}));
    }
    cache.measure(&many, &h, 40.0, &counter).await;
    assert_eq!(cache.cache_lengths(), (1024, 1024));
    std::fs::remove_dir_all(h).unwrap();
}
#[test]
fn capture_limits_default_ascii_size_and_rows() {
    let mut capture = ToolListCapture::default();
    assert!(
        capture
            .add(None, None, &vec![json!({"name":"a"}); 10001])
            .is_none()
    );
    let huge = json!({"name":"😀".repeat(170000)});
    assert!(capture.add(None, None, &[huge]).is_none());
    assert_eq!(capture.add(None, None, &[]), Some(vec![]));
}
#[tokio::test]
async fn skill_read_preserves_python_universal_newlines() {
    let h = home();
    let path = h.join("SKILL.md");
    std::fs::write(&path, "one\r\ntwo\rthree\nfour").unwrap();
    let mut cache = SkillMetadataCache::default();
    let rows = [json!({"id":"x","path":path})];
    cache
        .measure(&rows, &h, 0.0, &|texts: Vec<String>| {
            assert_eq!(texts, vec!["one\ntwo\nthree\nfour"]);
            async { vec![Some(7)] }
        })
        .await;
    std::fs::remove_dir_all(h).unwrap();
}
#[tokio::test]
async fn measurement_timestamp_is_read_after_count() {
    use std::sync::atomic::AtomicU64;
    let h = home();
    let at = AtomicU64::new(1000);
    let spec = json!({});
    comandos_extensions::metadata::record_mcp_size_with_clock(
        &h,
        "x",
        &spec,
        &[json!({"name":"x"})],
        || at.load(Ordering::Relaxed) as f64,
        &|_| {
            at.store(1010, Ordering::Relaxed);
            async { vec![Some(1)] }
        },
    )
    .await;
    assert_eq!(mcp_size(&h, "x", &spec, 1010.0)["measuredAt"], 1010);
    std::fs::remove_dir_all(h).unwrap();
}
#[test]
fn dropping_runtime_reaps_active_counter() {
    use std::os::unix::fs::PermissionsExt;
    let h = home();
    let helper = h.join("helper");
    let pidfile = h.join("pid");
    std::fs::write(
        &helper,
        format!(
            "#!/bin/sh\necho $$ > '{}'\nexec /bin/sleep 60\n",
            pidfile.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let home2 = h.clone();
    runtime.spawn(async move {
        comandos_extensions::tokenizer::isolated_token_counts_with(
            &helper,
            &home2,
            vec!["test".into()],
            std::time::Duration::from_secs(8),
        )
        .await
    });
    runtime.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(80)).await });
    drop(runtime);
    let pid = std::fs::read_to_string(&pidfile).unwrap();
    assert!(!std::path::Path::new(&format!("/proc/{}", pid.trim())).exists());
    std::fs::remove_dir_all(h).unwrap();
}
#[test]
fn provenance_unicode_category_regression() {
    let h = home();
    let root = h.join(".agents/skills/demo");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("SKILL.md"), "skill").unwrap();
    for (source, expected) in [
        ("owner/\u{0345}", "unknown"),
        ("owner/Ⅳ²", "repository"),
        ("owner/\u{11f02}", "unknown"),
    ] {
        std::fs::write(
            h.join(".agents/.skill-lock.json"),
            json!({"skills":{"demo":{"source":source,"sourceType":"github"}}}).to_string(),
        )
        .unwrap();
        assert_eq!(
            skill_origin(&json!({"path":root.join("SKILL.md")}), &h)["kind"],
            expected
        );
    }
    std::fs::remove_dir_all(h).unwrap();
}
#[test]
fn capture_serialization_failure_regression() {
    let mut c = ToolListCapture::default();
    assert!(c.add(None, Some("p2"), &[json!({"name":"a"})]).is_none());
    let mut deep = json!({});
    for _ in 0..130 {
        deep = json!({"nested":deep});
    }
    assert!(
        c.add(
            Some("p2"),
            Some("p3"),
            &[json!({"name":"bad","inputSchema":deep})]
        )
        .is_none()
    );
    assert!(c.add(Some("p2"), None, &[]).is_none());
    assert!(c.add(Some("p3"), None, &[]).is_none());
    assert_eq!(c.add(None, None, &[]), Some(vec![]));
}
#[test]
fn provenance_symlink_errors_and_dangling_external_targets_stay_unknown() {
    let h = home();
    let root = h.join(".agents/skills/demo");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        h.join(".agents/.skill-lock.json"),
        r#"{"skills":{"demo":{"source":"owner/repo","sourceType":"github"}}}"#,
    )
    .unwrap();
    let path = root.join("SKILL.md");
    std::os::unix::fs::symlink("SKILL.md", &path).unwrap();
    assert_eq!(skill_origin(&json!({"path":path}), &h)["kind"], "unknown");
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(h.join("missing-outside"), &path).unwrap();
    assert_eq!(skill_origin(&json!({"path":path}), &h)["kind"], "unknown");
    std::fs::remove_dir_all(h).unwrap();
}
