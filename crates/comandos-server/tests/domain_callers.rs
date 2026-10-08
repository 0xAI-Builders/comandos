#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
//! Autoridad S5 sobre HTTP real y un HOME temporal, sin migración automática.
mod support;
use comandos_store::unified::{self, Mode, Origin};
use serde_json::Value;
use support::{TestHome, dead_port, front, get, request_body};
#[tokio::test]
async fn snippets_and_prefs_use_authoritative_documents_in_every_mode() {
    for (i, mode) in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed]
        .into_iter()
        .enumerate()
    {
        let home = TestHome::new(&format!("domain-docs-{i}"));
        home.write("prefs.json", r#"{"theme":"noche","unknown":"ñ😀"}"#);
        home.write("snippets.json", "[]");
        let db = unified::open_unified(&unified::unified_path(&home.root)).unwrap();
        unified::doc_put(
            &db,
            "hooks/prefs.json",
            "ui-docs",
            br#"{"theme":"dia","unknown":"db"}"#,
            Origin::Import,
            0,
        )
        .unwrap();
        unified::doc_put(
            &db,
            "hooks/snippets.json",
            "ui-docs",
            b"[]",
            Origin::Import,
            0,
        )
        .unwrap();
        unified::set_mode(&db, "ui-docs", mode, "fixture", 1).unwrap();
        let server = front(&home, dead_port(), home.options()).await;
        let prefs: Value = serde_json::from_str(&get(server.port, "/prefs").await.text()).unwrap();
        assert_eq!(
            prefs["theme"],
            if matches!(mode, Mode::Unified | Mode::Sealed) {
                "dia"
            } else {
                "noche"
            }
        );
        let created = request_body(
            server.port,
            "POST",
            "/snippets",
            "",
            r#"{"name":"  Exacto  ","body":"ñ😀\n","tags":[]}"#,
        )
        .await;
        assert_eq!(created.status, 200, "{}", created.text());
        let item: Value = serde_json::from_str(&created.text()).unwrap();
        let wire = get(server.port, "/snippets").await;
        assert_eq!(wire.status, 200);
        let rows: Value = serde_json::from_str(&wire.text()).unwrap();
        assert_eq!(rows[0], item["item"]);
        if mode != Mode::Legacy {
            let body = unified::doc_get(&db, "hooks/snippets.json")
                .unwrap()
                .unwrap()
                .body;
            assert_eq!(
                comandos_core::json::workspace_loads_bytes(&body).unwrap(),
                rows
            );
        }
        if mode != Mode::Sealed {
            assert_eq!(
                comandos_core::json::workspace_loads_bytes(
                    &std::fs::read(home.hooks().join("snippets.json")).unwrap()
                )
                .unwrap(),
                rows
            );
        } else {
            assert_eq!(
                std::fs::read(home.hooks().join("snippets.json")).unwrap(),
                b"[]"
            );
        }
        server.stop().await;
    }
}

#[tokio::test]
async fn snippets_lock_is_shared_with_a_legacy_mirror_writer() {
    let home = TestHome::new("domain-lock");
    home.write("snippets.json", "[]");
    let db = unified::open_unified(&unified::unified_path(&home.root)).unwrap();
    unified::set_mode(&db, "ui-docs", Mode::Mirror, "fixture", 1).unwrap();
    let server = front(&home, dead_port(), home.options()).await;
    let lock = comandos_store::files::FileLock::exclusive(&home.hooks().join("snippets.json.lock"))
        .unwrap();
    let port = server.port;
    let pending = tokio::spawn(async move {
        request_body(
            port,
            "POST",
            "/snippets",
            "",
            r#"{"name":"new","body":"new","tags":[]}"#,
        )
        .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        !pending.is_finished(),
        "the HTTP writer must honor the existing file flock"
    );
    home.write(
        "snippets.json",
        r#"[{"id":"abcdef0123456789","name":"legacy","body":"keep","tags":[]}]"#,
    );
    drop(lock);
    assert_eq!(pending.await.unwrap().status, 200);
    let rows: Value = serde_json::from_str(&get(server.port, "/snippets").await.text()).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 2);
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "abcdef0123456789")
    );
    assert_eq!(
        comandos_core::json::workspace_loads_bytes(
            &unified::doc_get(&db, "hooks/snippets.json")
                .unwrap()
                .unwrap()
                .body
        )
        .unwrap(),
        rows
    );
    server.stop().await;
}

#[test]
fn catalogued_server_documents_keep_exact_bytes_in_every_mode() {
    use comandos_server::dash::native::files::DomainDocument;
    use comandos_store::domains::catalog::{self, SourcePattern, TargetKind};
    for (i, mode) in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed]
        .into_iter()
        .enumerate()
    {
        let home = TestHome::new(&format!("all-server-documents-{i}"));
        let db = unified::open_unified(&unified::unified_path(&home.root)).unwrap();
        for domain in ["tabs", "app-ui", "ui-docs", "quota-docs", "news-docs"] {
            unified::set_mode(&db, domain, mode, "fixture", 1).unwrap();
        }
        for spec in catalog::catalog() {
            if spec.kind != TargetKind::Document
                || !matches!(
                    spec.domain,
                    "tabs" | "app-ui" | "ui-docs" | "quota-docs" | "news-docs"
                )
            {
                continue;
            }
            let SourcePattern::File(symbolic) = spec.pattern else {
                continue;
            };
            let Some(name) = symbolic.strip_prefix("H/") else {
                continue;
            };
            let doc = DomainDocument::new(&home.root, &home.hooks(), name).unwrap();
            if mode == Mode::Sealed {
                let _ = std::fs::remove_file(&doc.file);
            }
            let exact =
                b"{\r\n  \"unknown\": \"exact\", \"unicode\": \"\\u00f1\\ud83d\\ude00\"\r\n}\n";
            doc.write_bytes(exact, 2).unwrap();
            assert_eq!(
                doc.read_bytes().unwrap().unwrap(),
                exact,
                "{name}: {mode:?}"
            );
            if mode != Mode::Legacy {
                assert_eq!(
                    unified::doc_get(&db, &format!("hooks/{name}"))
                        .unwrap()
                        .unwrap()
                        .body,
                    exact
                );
            }
            if mode == Mode::Sealed {
                assert!(!doc.file.exists(), "sealed recreated {name}");
            } else {
                assert_eq!(std::fs::read(&doc.file).unwrap(), exact);
            }
        }
    }
}

#[test]
fn web_selection_and_native_terminal_toggle_use_the_authoritative_mode() {
    use comandos_server::dash::{term::lifecycle, web::Selection};
    use std::os::unix::fs::PermissionsExt;
    for (i, mode) in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed]
        .into_iter()
        .enumerate()
    {
        let home = TestHome::new(&format!("domain-web-{i}"));
        home.write("comandos-web.json", r#"{"on":["legacy"],"shadow":[]}"#);
        home.write(
            "webterm-mode.json",
            r#"{"mode":"native","ports":[4779,4780]}"#,
        );
        let db = unified::open_unified(&unified::unified_path(&home.root)).unwrap();
        unified::doc_put(
            &db,
            "hooks/comandos-web.json",
            "ui-docs",
            br#"{"on":["sql"],"shadow":["shadow"]}"#,
            Origin::Import,
            0,
        )
        .unwrap();
        unified::doc_put(
            &db,
            "hooks/webterm-mode.json",
            "ui-docs",
            br#"{"mode":"native","ports":[4779,4780]}"#,
            Origin::Import,
            0,
        )
        .unwrap();
        unified::set_mode(&db, "ui-docs", mode, "fixture", 1).unwrap();
        let selected = Selection::load_domain(&home.root, &home.hooks().join("comandos-web.json"));
        assert!(
            selected
                .on
                .contains(if matches!(mode, Mode::Unified | Mode::Sealed) {
                    "sql"
                } else {
                    "legacy"
                })
        );
        if matches!(mode, Mode::Unified | Mode::Sealed) {
            let cfg = comandos_server::dash::parse_args(&[], &home.root, None).unwrap();
            let web = comandos_server::dash::web::WebState::new(&cfg);
            assert!(web.selection().on.contains("sql"));
            unified::doc_put(
                &db,
                "hooks/comandos-web.json",
                "ui-docs",
                br#"{"on":["changed"],"shadow":[]}"#,
                Origin::Unified,
                2,
            )
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1010));
            assert!(
                web.selection().on.contains("changed"),
                "SQL changes need no legacy mtime event"
            );
        }
        lifecycle::set_enabled(&home.root, &home.hooks(), true).unwrap();
        let control = lifecycle::Control::new(&home.root);
        assert!(control.enabled());
        if mode != Mode::Legacy {
            assert_eq!(
                unified::doc_get(&db, "hooks/webterm-enabled")
                    .unwrap()
                    .unwrap()
                    .body,
                b""
            );
        }
        if mode == Mode::Sealed {
            assert!(!home.hooks().join("webterm-enabled").exists());
            assert!(!home.hooks().join("webterm-enabled.lock").exists());
        } else {
            assert_eq!(
                std::fs::read(home.hooks().join("webterm-enabled")).unwrap(),
                b""
            );
            assert_eq!(
                std::fs::metadata(home.hooks().join("webterm-enabled"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        lifecycle::set_enabled(&home.root, &home.hooks(), false).unwrap();
        assert!(!control.enabled());
        assert!(!home.hooks().join("webterm-enabled").exists());
        if mode != Mode::Legacy {
            assert!(
                unified::doc_get(&db, "hooks/webterm-enabled")
                    .unwrap()
                    .is_none()
            );
        }
        if matches!(mode, Mode::Unified | Mode::Sealed) {
            unified::doc_put(
                &db,
                "hooks/webterm-mode.json",
                "ui-docs",
                br#"{"mode":"off"}"#,
                Origin::Unified,
                3,
            )
            .unwrap();
        } else {
            home.write("webterm-mode.json", r#"{"mode":"off"}"#);
        }
        assert!(lifecycle::set_enabled(&home.root, &home.hooks(), true).is_err());
        assert!(!control.enabled());
    }
}
