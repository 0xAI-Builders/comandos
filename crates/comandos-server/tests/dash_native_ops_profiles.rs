mod support;
use serde_json::{Value, json};
use support::{FakeLegacy, TestHome, front, get, request_body};
#[tokio::test]
async fn profile_routes_native_crud_inventory_and_apply_errors() {
    let home = TestHome::new("profiles-native");
    let legacy = FakeLegacy::start().await;
    let app = front(&home, legacy.port, home.options()).await;
    let post =
        |path: &'static str, body: &'static str| request_body(app.port, "POST", path, "", body);
    let reply = post(
        "/session-profiles",
        r#"{"id":"p","name":"Demo","harness":"shell"}"#,
    )
    .await;
    assert_eq!(reply.status, 200);
    let saved: Value = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(saved["profile"]["name"], "Demo");
    let reply = get(
        app.port,
        &format!(
            "/session-profiles-extra?harness=shell&cwd={}",
            home.root.display()
        ),
    )
    .await;
    assert_eq!(reply.status, 200);
    let value: Value = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(value["inventory"]["status"], "unsupported");
    assert_eq!(value["profiles"].as_array().unwrap().len(), 1);
    let reply = post("/session-profiles", r#"{"name":""}"#).await;
    assert_eq!(reply.status, 400);
    assert_eq!(
        serde_json::from_slice::<Value>(&reply.body).unwrap(),
        json!({"error":"nombre de perfil inválido"})
    );
    let reply = post("/session-profile-apply", r#"{"profileId":"missing"}"#).await;
    assert_eq!(reply.status, 409);
    assert_eq!(
        serde_json::from_slice::<Value>(&reply.body).unwrap(),
        json!({"error":"perfil no encontrado","code":"profile_unavailable"})
    );
    let reply = post("/session-profiles", r#"{"action":"delete","id":"p"}"#).await;
    assert_eq!(reply.status, 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&reply.body).unwrap(),
        json!({"ok":true})
    );
    assert!(legacy.requests().is_empty());
    assert!(!home.hooks().join("profile-launches").exists());
    app.stop().await;
}
fn write_home(h: &TestHome, name: &str, text: &str) {
    let p = h.root.join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
fn normalize_profile_value(value: &mut Value, root: &std::path::Path) {
    match value {
        Value::String(s) => *s = s.replace(root.to_str().unwrap(), "HOME"),
        Value::Array(a) => {
            for v in a {
                normalize_profile_value(v, root)
            }
        }
        Value::Object(m) => {
            for (k, v) in m.iter_mut() {
                if matches!(k.as_str(), "createdAt" | "updatedAt") {
                    *v = json!(0)
                } else {
                    normalize_profile_value(v, root)
                }
            }
            if m.contains_key("path") && m.contains_key("automaticInvocation") {
                m.insert("id".into(), json!("SKILL-ID"));
            }
        }
        _ => {}
    }
}
#[tokio::test]
async fn profile_routes_match_confined_python_twin() {
    use support::twin::{Twin, TwinOpts};
    let opts = TwinOpts {
        fakebin_extra: vec![("codex".into(), "#!/bin/sh\nexit 0\n".into())],
        ..Default::default()
    };
    let Some(t) = Twin::start_with(
        "profiles-twin",
        |h| {
            write_home(
                h,
                ".codex/auth.json",
                r#"{"tokens":{"access_token":"synthetic-test-only"}}"#,
            );
            write_home(
                h,
                ".codex/skills/demo/SKILL.md",
                "---\nname: demo\ndescription: Example skill\n---\n",
            );
        },
        opts,
    )
    .await
    else {
        return;
    };
    for body in [
        r#"{"id":"p","name":"Demo","harness":"codex"}"#,
        r#"{"name":""}"#,
        r#"{"id":"bad","name":"Bad","skills":{"s":1}}"#,
    ] {
        let run = t.post("/session-profiles", body).await;
        assert_eq!(run.front.status, run.oracle.status);
        let (mut a, mut b) = (
            serde_json::from_slice::<Value>(&run.front.body).unwrap(),
            serde_json::from_slice::<Value>(&run.oracle.body).unwrap(),
        );
        normalize_profile_value(&mut a, &t.a.root);
        normalize_profile_value(&mut b, &t.b.root);
        assert_eq!(a.to_string(), b.to_string());
    }
    for path in [
        "/session-profiles",
        "/session-profiles?harness=claude&account=missing",
        "/session-profiles?harness=shell&harness=codex",
        "/session-profiles?cwd=relative",
    ] {
        let run = t.get(path).await;
        assert_eq!(run.front.status, run.oracle.status, "{path}");
        let (mut a, mut b) = (
            serde_json::from_slice::<Value>(&run.front.body).unwrap(),
            serde_json::from_slice::<Value>(&run.oracle.body).unwrap(),
        );
        normalize_profile_value(&mut a, &t.a.root);
        normalize_profile_value(&mut b, &t.b.root);
        assert_eq!(a.to_string(), b.to_string(), "{path}");
    }
    for body in [r#"{"profileId":"p"}"#, r#"{"profileId":"missing"}"#] {
        let run = t.post("/session-profile-apply", body).await;
        assert_eq!(run.front.status, run.oracle.status);
        let (mut a, mut b) = (
            serde_json::from_slice::<Value>(&run.front.body).unwrap(),
            serde_json::from_slice::<Value>(&run.oracle.body).unwrap(),
        );
        normalize_profile_value(&mut a, &t.a.root);
        normalize_profile_value(&mut b, &t.b.root);
        assert_eq!(a.to_string(), b.to_string(), "apply {body}");
    }
    t.post("/session-profiles", r#"{"action":"delete","id":"p"}"#)
        .await
        .assert_same();
    assert!(!t.a.hooks().join("profile-launches").exists());
    assert!(!t.b.hooks().join("profile-launches").exists());
    drop(t);
}
