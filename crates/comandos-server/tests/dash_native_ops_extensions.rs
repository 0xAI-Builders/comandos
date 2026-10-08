//! Estante por panel: inventario ficticio y sockets tmux privados del gemelo.
mod support;
use serde_json::{Value, json};
use support::{FakeLegacy, TestHome, front, get, request_body, twin::Twin};

#[tokio::test]
async fn standalone_mcp_selection_gates_calls_without_restarting_the_private_agent() {
    let Some(t) = Twin::start_with(
        "ext-live-gate",
        |home| {
            support::ops::seed_fake_codex(home);
            std::fs::write(
                home.root.join(".codex/config.toml"),
                "[mcp_servers.demo]\ncommand='test-only'\n",
            )
            .unwrap();
        },
        support::twin::TwinOpts {
            fakebin_extra: vec![(
                "codex".into(),
                "#!/bin/sh\nexec \"$HOME/bin/codex\" \"$@\"\n".into(),
            )],
            front: Some(Box::new(|opts| opts.standalone = true)),
            ..Default::default()
        },
    )
    .await
    else {
        return;
    };
    let before = t
        .get("/pane-extensions?session=audit&pane=%250")
        .await
        .front;
    assert_eq!(before.status, 200, "{}", before.text());
    let before: Value = serde_json::from_slice(&before.body).unwrap();
    let identity = before["identity"].as_str().unwrap();
    assert!(comandos_store::extension_gate::enabled(&t.a.root, identity, "demo").unwrap());
    let mut request = json!({
        "session":"audit", "pane":"%0", "harness":"codex",
        "expectedIdentity":identity,"expectedConversationId":before["conversationId"],
        "revision":before["revision"],"desired":{"mcps":{"demo":false},"skills":{}}
    });
    let saved = t.post_front("/pane-extensions", &request.to_string()).await;
    assert_eq!(saved.status, 200, "{}", saved.text());
    let saved: Value = serde_json::from_slice(&saved.body).unwrap();
    assert_eq!(saved["mcpGate"]["selection"]["demo"], false);
    assert!(!comandos_store::extension_gate::enabled(&t.a.root, identity, "demo").unwrap());
    assert_eq!(saved["skillsRestartRequired"], false);
    let stale = t.post_front("/pane-extensions", &request.to_string()).await;
    assert_eq!(stale.status, 409);
    request.as_object_mut().unwrap().remove("desired");
    request["revision"] = saved["revision"].clone();
    request["requestId"] = json!("gate-only");
    request["interrupt"] = json!(true);
    let applied = t
        .post_front("/pane-extensions/apply", &request.to_string())
        .await;
    assert_eq!(applied.status, 200, "{}", applied.text());
    let applied: Value = serde_json::from_slice(&applied.body).unwrap();
    assert_eq!(applied["identity"], before["identity"]);
    assert_eq!(applied["conversationId"], before["conversationId"]);
    assert!(applied["operation"].is_null());
    let catalog = t.a.root.join(".config/comandos/extensions/catalog.json");
    std::fs::write(
        &catalog,
        r#"{"servers":{"new":{"command":"unused","enabled":false}}}"#,
    )
    .unwrap();
    let off = t
        .get("/pane-extensions?session=audit&pane=%250")
        .await
        .front;
    assert_eq!(off.status, 200, "{}", off.text());
    let off: Value = serde_json::from_slice(&off.body).unwrap();
    assert_eq!(off["desired"]["mcps"]["new"], false);
    request.as_object_mut().unwrap().remove("requestId");
    request.as_object_mut().unwrap().remove("interrupt");
    request["revision"] = off["revision"].clone();
    request["desired"] = off["desired"].clone();
    let saved_off = t.post_front("/pane-extensions", &request.to_string()).await;
    assert_eq!(saved_off.status, 200, "{}", saved_off.text());
    std::fs::write(
        &catalog,
        r#"{"servers":{"new":{"command":"unused","enabled":true}}}"#,
    )
    .unwrap();
    let on = t
        .get("/pane-extensions?session=audit&pane=%250")
        .await
        .front;
    assert_eq!(on.status, 200, "{}", on.text());
    let on: Value = serde_json::from_slice(&on.body).unwrap();
    assert_eq!(
        on["desired"]["mcps"]["new"], true,
        "global OFF must not become a pane exclusion"
    );
    assert_eq!(
        on["desired"]["mcps"]["demo"], false,
        "explicit pane exclusion survives"
    );
    assert!(t.tmux_mutations_a().is_empty());
    assert_eq!(
        std::fs::read_to_string(t.a.root.join(".codex/config.toml")).unwrap(),
        "[mcp_servers.demo]\ncommand='test-only'\n"
    );
}

#[tokio::test]
async fn missing_target_and_extra_fields_match_python_without_fallback() {
    let Some(t) = Twin::start("ext-errors", |_| {}).await else {
        return;
    };
    for path in [
        "/pane-extensions",
        "/pane-extensions-extra?session=s",
        "/pane-extensions?session=s&pane=0",
    ] {
        let r = t.get(path).await;
        r.assert_same();
        assert_eq!(r.front.status, 409);
    }
    for path in [
        "/pane-extensions",
        "/pane-extensions/apply",
        "/pane-extensions/template",
        "/pane-extensions/cancel",
        "/pane-extensions/recover",
    ] {
        let r = t
            .post(path, r#"{"session":"s","pane":"%0","unexpected":true}"#)
            .await;
        r.assert_same();
        assert_eq!(r.front.status, 400);
    }
    // Python compares the full POST target, so query-suffixed paths do not
    // save drafts or enter apply/cancel/recover.
    for path in [
        "/pane-extensions?extra=1",
        "/pane-extensions/apply?extra=1",
        "/session/configure?extra=1",
        "/account/switch?extra=1",
        "/model/switch?extra=1",
    ] {
        let r = t.post(path, r#"{"session":"s","unexpected":true}"#).await;
        r.assert_same();
        assert_eq!(r.front.status, 404);
    }
    assert!(t.tmux_mutations_a().is_empty());
}

#[tokio::test]
async fn extension_errors_are_native_and_cut_can_delegate() {
    let home = TestHome::new("ext-native");
    let legacy = FakeLegacy::start().await;
    let app = front(&home, legacy.port, home.options()).await;
    let r = get(app.port, "/pane-extensions").await;
    assert_eq!(r.status, 409);
    let v: Value = serde_json::from_slice(&r.body).unwrap();
    assert_eq!(
        v,
        json!({"ok":false,"error":"No se pudo identificar el panel y su conversación."})
    );
    let r = request_body(
        app.port,
        "POST",
        "/pane-extensions",
        "",
        r#"{"unexpected":true}"#,
    )
    .await;
    assert_eq!(r.status, 400);
    assert!(legacy.requests().is_empty());
    app.stop().await;
    let mut options = home.options();
    options
        .cuts_off
        .insert(comandos_server::dash::native::Cut::Ops);
    let app = front(&home, legacy.port, options).await;
    get(app.port, "/pane-extensions").await;
    assert_eq!(legacy.requests().len(), 1);
    app.stop().await;
}

fn normalize(value: &mut Value, home: &TestHome) {
    match value {
        Value::String(s) => *s = s.replace(home.root.to_str().unwrap(), "HOME"),
        Value::Array(a) => {
            for v in a {
                normalize(v, home)
            }
        }
        Value::Object(m) => {
            for (k, v) in m.iter_mut() {
                if k == "identity" {
                    *v = json!("IDENTITY");
                } else if ["updated", "observedAt", "evidenceAt", "pid"].contains(&k.as_str()) {
                    *v = json!(0);
                } else {
                    normalize(v, home)
                }
            }
            if m.contains_key("selection") && m.contains_key("id") {
                m.insert("id".into(), json!("TEMPLATE"));
            }
        }
        _ => {}
    }
}
fn same(a: &support::Wire, b: &support::Wire, t: &Twin) -> (Value, Value) {
    assert_eq!(
        a.status,
        b.status,
        "front {} / Python {}",
        a.text(),
        b.text()
    );
    let (a, b): (Value, Value) = (
        serde_json::from_slice(&a.body).unwrap(),
        serde_json::from_slice(&b.body).unwrap(),
    );
    let (mut av, mut bv) = (a.clone(), b.clone());
    normalize(&mut av, &t.a);
    normalize(&mut bv, &t.b);
    assert_eq!(
        av.to_string(),
        bv.to_string(),
        "orden y contenido del cuerpo normalizado"
    );
    (a, b)
}
async fn post_pair(t: &Twin, path: &str, a: &Value, b: &Value) -> (Value, Value) {
    let a = t.post_front(path, &a.to_string()).await;
    let b = t.post_oracle(path, &b.to_string()).await;
    same(&a, &b, t)
}
#[tokio::test]
async fn drafts_templates_guards_and_cancellation_match_live_private_shells() {
    let Some(t) = Twin::start("ext-store", |h| {
        std::fs::create_dir_all(h.root.join(".codex")).unwrap();
        std::fs::write(
            h.root.join(".codex/config.toml"),
            "[mcp_servers.demo]\ncommand='test-only'\n",
        )
        .unwrap();
        support::run_tmux(
            h,
            &[
                "new-session",
                "-d",
                "-s",
                "audit",
                "-c",
                h.root.to_str().unwrap(),
                "/bin/bash --noprofile --norc",
            ],
        );
    })
    .await
    else {
        return;
    };
    let (pa, pb) = (
        t.tmux_a(&["display-message", "-p", "-t", "=audit:", "#{pane_id}"]),
        t.tmux_b(&["display-message", "-p", "-t", "=audit:", "#{pane_id}"]),
    );
    assert_eq!(pa, pb);
    let pane = pa.trim();
    let url =
        format!("/pane-extensions?session=audit&pane={pane}&harness=wrong&harness=codex&harness=");
    let run = t.get(&url).await;
    assert_eq!(run.front.status, 200, "{}", run.front.text());
    let (a, b) = same(&run.front, &run.oracle, &t);
    assert_eq!(a["configurationStatus"], "not_started");
    assert_eq!(a["applySupported"], true);
    let make = |v: &Value| json!({"session":"audit","pane":pane,"harness":"codex","expectedIdentity":v["identity"],"expectedConversationId":v["conversationId"],"revision":v["revision"]});
    let (mut da, mut db) = (make(&a), make(&b));
    da["desired"] = json!({"mcps":{"demo":false},"skills":{}});
    db["desired"] = da["desired"].clone();
    let (a, b) = post_pair(&t, "/pane-extensions", &da, &db).await;
    assert_eq!(a["revision"], 1);
    let stale = post_pair(&t, "/pane-extensions", &da, &db).await;
    assert_eq!(stale.0["ok"], false);
    let (mut da, mut db) = (make(&a), make(&b));
    da["interrupt"] = json!("si");
    db["interrupt"] = json!("si");
    let error = post_pair(&t, "/pane-extensions/apply", &da, &db).await;
    assert_eq!(error.0["error"], "interrupt inválido");
    let (mut da, mut db) = (make(&a), make(&b));
    da["name"] = json!("Portable");
    db["name"] = json!("Portable");
    let templates = post_pair(&t, "/pane-extensions/template", &da, &db).await;
    da["templateId"] = templates.0["template"]["id"].clone();
    db["templateId"] = templates.1["template"]["id"].clone();
    let error = post_pair(&t, "/pane-extensions/template", &da, &db).await;
    assert_eq!(error.0["error"], "indica nombre o plantilla");
    da.as_object_mut().unwrap().remove("name");
    db.as_object_mut().unwrap().remove("name");
    let (a, b) = post_pair(&t, "/pane-extensions/template", &da, &db).await;
    assert_eq!(a["revision"], 2);
    let (mut da, mut db) = (make(&a), make(&b));
    da["expectedConversationId"] = json!("changed");
    db["expectedConversationId"] = json!("changed");
    let error = post_pair(&t, "/pane-extensions", &da, &db).await;
    assert_eq!(
        error.0["error"],
        "cambió el panel o la conversación; vuelve a cargar el estante"
    );
    let (mut da, mut db) = (make(&a), make(&b));
    da["operationId"] = json!("missing");
    db["operationId"] = json!("missing");
    let error = post_pair(&t, "/pane-extensions/cancel", &da, &db).await;
    assert_eq!(error.0["error"], "la operación no pertenece a este panel");
    for (home, state) in [(&t.a, &a), (&t.b, &b)] {
        use comandos_runtime::session_operations::{Journal, OperationStore, open_journal};
        let conn = open_journal(&home.journal_db()).unwrap();
        let owner = || i64::from(std::process::id());
        let store = OperationStore::new(&conn, &owner, &|| 1000.0).unwrap();
        store
            .claim(
                "pending-private",
                state["identity"].as_str().unwrap(),
                &json!({"session":"audit","pane":pane}),
            )
            .unwrap();
        store
            .stage("pending-private", "waiting", None, None)
            .unwrap();
    }
    da["operationId"] = json!("pending-private");
    db["operationId"] = json!("pending-private");
    let cancelled = post_pair(&t, "/pane-extensions/cancel", &da, &db).await;
    assert_eq!(
        cancelled.0,
        json!({"ok":true,"cancelled":true,"operationId":"pending-private"})
    );
    let persisted = |home: &TestHome| {
        let conn = rusqlite::Connection::open_with_flags(
            home.journal_db(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let drafts: Vec<(String, String, String, i64)> = conn.prepare(
            "SELECT conversation,harness,desired,revision FROM pane_extension_drafts ORDER BY key",
        ).unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap().collect::<Result<_, _>>().unwrap();
        let templates: Vec<(String, String)> = conn
            .prepare("SELECT name,selection FROM pane_extension_templates ORDER BY name")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        (drafts, templates)
    };
    let rows = persisted(&t.a);
    assert_eq!(rows, persisted(&t.b), "durable draft and template rows");
    assert_eq!(rows.0.len(), 1);
    assert_eq!(rows.0[0].3, 2);
    assert_eq!(rows.1.len(), 1);
    assert!(t.tmux_mutations_a().is_empty());
    assert!(t.tmux_mutations_b().is_empty());
}

#[tokio::test]
async fn apply_retry_uses_saved_request_after_pane_disappears() {
    use comandos_runtime::session_operations::{Journal, OperationStore, open_journal};
    let original = json!({"session":"gone","pane":"%99","harness":"codex","expectedIdentity":"old-pane","expectedConversationId":"old-conversation","revision":3,"requestId":"extension-complete","interrupt":false,"extensionsOnly":true,"extensionDraftKey":"saved-draft","toHarness":"codex"});
    let seed = original.clone();
    let Some(t) = Twin::start("ext-retry", move |h| {
        let conn = open_journal(&h.journal_db()).unwrap();
        let owner = || i64::from(std::process::id());
        let store = OperationStore::new(&conn, &owner, &|| 1000.0).unwrap();
        store
            .claim("extension-complete", "old-pane", &seed)
            .unwrap();
        store
            .stage(
                "extension-complete",
                "confirmed",
                None,
                Some(&json!({"ok":true})),
            )
            .unwrap();
    })
    .await
    else {
        return;
    };
    let mut posted = original;
    for field in ["extensionsOnly", "extensionDraftKey", "toHarness"] {
        posted.as_object_mut().unwrap().remove(field);
    }
    let run = t.post("/pane-extensions/apply", &posted.to_string()).await;
    run.assert_same();
    assert_eq!(run.front.status, 200);
    posted["revision"] = json!(4);
    let run = t.post("/pane-extensions/apply", &posted.to_string()).await;
    run.assert_same();
    assert_eq!(run.front.status, 409);
    assert_eq!(
        serde_json::from_slice::<Value>(&run.front.body).unwrap()["error"],
        "requestId ya se usó con otra configuración"
    );
    assert!(t.tmux_mutations_a().is_empty());
    assert!(t.tmux_mutations_b().is_empty());
}

#[tokio::test]
async fn apply_relaunches_only_the_private_fake_agent_and_replays_once() {
    let Some(t) = Twin::start_with(
        "ext-apply",
        |home| {
            support::ops::install_extension_launcher(home);
            support::ops::seed_fake_codex(home);
            std::fs::write(
                home.root.join(".codex/config.toml"),
                "[mcp_servers.demo]\ncommand='test-only'\n",
            )
            .unwrap();
        },
        support::twin::TwinOpts {
            fakebin_extra: vec![(
                "codex".into(),
                "#!/bin/sh\nexec \"$HOME/bin/codex\" \"$@\"\n".into(),
            )],
            ..Default::default()
        },
    )
    .await
    else {
        return;
    };
    let path = "/pane-extensions?session=audit&pane=%250";
    let before = t.get(path).await;
    assert_eq!(before.front.status, 200, "{}", before.front.text());
    let (a, b) = same(&before.front, &before.oracle, &t);
    assert_eq!(a["conversationId"], "11111111-1111-1111-1111-111111111111");
    let request = |v: &Value| {
        json!({
            "session":"audit", "pane":"%0", "harness":"codex",
            "expectedIdentity":v["identity"],
            "expectedConversationId":v["conversationId"],
            "revision":v["revision"],
            "desired":{"mcps":{"demo":false},"skills":{}},
        })
    };
    let (a, b) = post_pair(&t, "/pane-extensions", &request(&a), &request(&b)).await;
    let apply = |v: &Value| {
        let mut r = request(v);
        r.as_object_mut().unwrap().remove("desired");
        r["requestId"] = json!("extension-apply-once");
        r["interrupt"] = true.into();
        r
    };
    let (ra, rb) = (apply(&a), apply(&b));
    let (started, _) = post_pair(&t, "/pane-extensions/apply", &ra, &rb).await;
    assert_eq!(started["ok"], true, "{started}");
    support::ops::wait_operations_idle(&t).await;
    let after = t.get(path).await;
    let (a, _) = same(&after.front, &after.oracle, &t);
    assert_eq!(a["operation"]["state"], "confirmed", "{a}");
    assert_eq!(a["loaded"]["mcps"]["demo"], false, "{a}");
    assert_eq!(a["conversationId"], "11111111-1111-1111-1111-111111111111");
    let mutations = (t.tmux_mutations_a(), t.tmux_mutations_b());
    post_pair(&t, "/pane-extensions/apply", &ra, &rb).await;
    assert_eq!(mutations, (t.tmux_mutations_a(), t.tmux_mutations_b()));
    assert_eq!(
        support::ops::journal_summary(&t.a),
        support::ops::journal_summary(&t.b)
    );
    for home in [&t.a, &t.b] {
        assert_eq!(
            std::fs::read_to_string(home.root.join(".codex/config.toml")).unwrap(),
            "[mcp_servers.demo]\ncommand='test-only'\n"
        );
    }
    // Simulate a launch whose process exists but confirmation was interrupted.
    // Recovery must use the saved origin and only stop our private marked agent.
    for home in [&t.a, &t.b] {
        let conn = rusqlite::Connection::open(home.journal_db()).unwrap();
        assert_eq!(conn.execute(
            "UPDATE session_operations SET state='awaiting_confirmation' WHERE id='extension-apply-once'",
            [],
        ).unwrap(), 1);
    }
    let recover = |mut value: Value| {
        let fields = value.as_object_mut().unwrap();
        fields.remove("requestId");
        fields.remove("interrupt");
        fields.insert("operationId".into(), json!("extension-apply-once"));
        value
    };
    let (recovering, _) =
        post_pair(&t, "/pane-extensions/recover", &recover(ra), &recover(rb)).await;
    assert_eq!(recovering["ok"], true, "{recovering}");
    support::ops::wait_operations_idle(&t).await;
    let recovered = t.get(path).await;
    let (a, _) = same(&recovered.front, &recovered.oracle, &t);
    assert_eq!(a["operation"]["state"], "rolled_back", "{a}");
    assert_eq!(a["conversationId"], "11111111-1111-1111-1111-111111111111");
    assert_eq!(a["loaded"], Value::Null, "{a}");
    assert_eq!(
        support::ops::journal_summary(&t.a),
        support::ops::journal_summary(&t.b)
    );
}
