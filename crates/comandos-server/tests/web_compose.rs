use comandos_server::dash::web::{
    ComponentState, Manifest, Selection,
    compose::compose,
    registry::{Entry, Resolved},
};
use std::collections::BTreeSet;

const PAGE: &str = "<meta charset=\"utf-8\">\n<title>ComandOS</title>\n\
<script src=\"/quick-terminal.js?v=2\"></script>\n<script>\n// ---------- red ----------\nfunction api(){}\n// ---------- helpers ----------\nfunction esc(){}\n</script>\n";

fn sha(s: &str) -> String {
    comandos_server::dash::web::registry::sha256_hex(s.as_bytes())
}

#[test]
fn ready_before_gate_subscription_is_retained() {
    use comandos_server::dash::web::gate::{Gate, Inserted};
    let gate = Gate::default();
    let Inserted::Nonce(nonce) = gate.insert() else {
        panic!("fixture nonce")
    };
    assert!(gate.mark_ready(&nonce));
    let receiver = gate
        .subscribe(&nonce)
        .expect("ready temprano sigue disponible");
    assert!(*receiver.borrow());
    assert!(!gate.mark_ready(&nonce), "ready duplicado se rechaza");
}

#[test]
fn compositor_uses_the_boot_name_emitted_by_web_build() {
    let dir = std::env::temp_dir().join(format!("comandos-manifest-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("manifest.json"), r#"{"files":{"comandos_web_boot.js":"fedcba987654/boot.js","comandos_web.js":"fedcba987654/comandos_web.js","comandos_web_bg.wasm":"fedcba987654/comandos_web_bg.wasm"}}"#).unwrap();
    std::fs::create_dir_all(dir.join("fedcba987654")).unwrap();
    for name in ["boot.js", "comandos_web.js", "comandos_web_bg.wasm"] {
        std::fs::write(dir.join("fedcba987654").join(name), b"fixture").unwrap();
    }
    let assets = Manifest::load(&dir).unwrap();
    let src = "(function(){})()";
    let c = compose(
        PAGE.as_bytes(),
        &Resolved::in_memory(entries(&sha(src)), &[("dash/quick-terminal.js", src)]),
        &sel(&["quick-terminal"]),
        false,
        &assets,
        "n1",
    );
    assert!(
        String::from_utf8(c.html)
            .unwrap()
            .contains("/web/fedcba987654/boot.js")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn manifest_with_a_missing_wasm_cannot_remove_the_original_javascript() {
    let dir = std::env::temp_dir().join(format!("comandos-missing-wasm-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("fedcba987654")).unwrap();
    std::fs::write(dir.join("manifest.json"), r#"{"files":{"comandos_web_boot.js":"fedcba987654/boot.js","comandos_web.js":"fedcba987654/comandos_web.js","comandos_web_bg.wasm":"fedcba987654/comandos_web_bg.wasm"}}"#).unwrap();
    std::fs::write(dir.join("fedcba987654/boot.js"), b"fixture").unwrap();
    std::fs::write(dir.join("fedcba987654/comandos_web.js"), b"fixture").unwrap();
    assert!(
        Manifest::load(&dir).is_err(),
        "el manifiesto no prueba que el WASM exista"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn status_hashes_the_real_index_region_instead_of_an_empty_page() {
    use comandos_server::{
        ReplyBody,
        dash::{
            self,
            web::{WebState, routes},
        },
    };
    let dir = std::env::temp_dir().join(format!("comandos-web-status-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".claude/hooks")).unwrap();
    std::fs::write(dir.join("index.html"), PAGE).unwrap();
    std::fs::write(
        dir.join(".claude/hooks/comandos-web.json"),
        r#"{"on":["red"],"shadow":[]}"#,
    )
    .unwrap();
    let mut cfg = dash::parse_args(&[], &dir, None).unwrap();
    cfg.dash_dir = dir.clone();
    cfg.web_dir = dir.clone();
    std::fs::create_dir_all(dir.join("fedcba987654")).unwrap();
    std::fs::write(dir.join("manifest.json"), r#"{"files":{"comandos_web_boot.js":"fedcba987654/boot.js","comandos_web.js":"fedcba987654/comandos_web.js","comandos_web_bg.wasm":"fedcba987654/comandos_web_bg.wasm"}}"#).unwrap();
    for name in ["boot.js", "comandos_web.js", "comandos_web_bg.wasm"] {
        std::fs::write(dir.join("fedcba987654").join(name), b"fixture").unwrap();
    }
    let mut web = WebState::new(&cfg);
    web.registry = Resolved::in_memory(entries("x"), &[]);
    let reply = routes::status(&web).unwrap();
    let ReplyBody::Bytes(bytes) = reply.body else {
        panic!("status bytes")
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["components"]["red"]["state"], "on");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn missing_build_artifacts_keep_the_original_page() {
    let src = "(function(){})()";
    let c = compose(
        PAGE.as_bytes(),
        &Resolved::in_memory(entries(&sha(src)), &[("dash/quick-terminal.js", src)]),
        &sel(&["quick-terminal"]),
        false,
        &Manifest::default(),
        "n1",
    );
    assert_eq!(c.html, PAGE.as_bytes());
    assert!(c.active.is_empty());
}

#[test]
fn script_removal_matches_the_src_attribute_only() {
    let src = "(function(){})()";
    let page = "<script>const text='quick-terminal.js';</script>\n<script data-note='quick-terminal.js' src='/other.js'></script>\n<script src='/quick-terminal.js?v=1'></script>\n";
    let c = compose(
        page.as_bytes(),
        &Resolved::in_memory(entries(&sha(src)), &[("dash/quick-terminal.js", src)]),
        &sel(&["quick-terminal"]),
        false,
        &Manifest::test(),
        "n1",
    );
    let result = String::from_utf8(c.html).unwrap();
    assert!(result.contains("const text='quick-terminal.js'"));
    assert!(result.contains("src='/other.js'"));
    assert!(!result.contains("src='/quick-terminal.js?v=1'"));
}

fn entries(qt_sha: &str) -> Vec<Entry> {
    vec![
        Entry::script(
            "quick-terminal",
            "dash/quick-terminal.js",
            qt_sha,
            &["ComandosQuickTerminal"],
            &[],
        ),
        Entry::region(
            "red",
            "dash/index.html",
            "// ---------- red ----------",
            "// ---------- helpers ----------",
            &sha("// ---------- red ----------\nfunction api(){}\n"),
            &["api", "authToken"],
            &[],
        ),
    ]
}

fn sel(on: &[&str]) -> Selection {
    Selection {
        on: on.iter().map(|s| s.to_string()).collect(),
        shadow: BTreeSet::new(),
    }
}

#[test]
fn active_script_component_is_removed_and_boot_injected_after_charset() {
    let src = "(function(){})()";
    let c = compose(
        PAGE.as_bytes(),
        &Resolved::in_memory(entries(&sha(src)), &[("dash/quick-terminal.js", src)]),
        &sel(&["quick-terminal"]),
        false,
        &Manifest::test(),
        "n1",
    );
    let html = String::from_utf8(c.html).unwrap();
    assert!(!html.contains("quick-terminal.js"));
    let after_charset = html.split_once("<meta charset=\"utf-8\">\n").unwrap().1;
    assert!(after_charset.starts_with("<meta name=\"comandos-web\" content=\"quick-terminal\">"));
    assert!(html.contains(
        "<script type=\"module\" async src=\"/web/0123456789ab/boot.js\" data-k=\"n1\"></script>"
    ));
    assert!(html.contains("<script src=\"/web/gate.js?k=n1\"></script>"));
}

#[test]
fn compose_drift_keeps_legacy() {
    let c = compose(
        PAGE.as_bytes(),
        &Resolved::in_memory(
            entries(&sha("version portada")),
            &[("dash/quick-terminal.js", "version nueva")],
        ),
        &sel(&["quick-terminal"]),
        false,
        &Manifest::test(),
        "n1",
    );
    let html = String::from_utf8(c.html).unwrap();
    assert!(html.contains("quick-terminal.js"));
    assert!(matches!(
        c.states["quick-terminal"],
        ComponentState::Drift { .. }
    ));
    assert!(!html.contains("comandos-web"));
}

#[test]
fn region_is_cut_only_with_matching_hash() {
    let c = compose(
        PAGE.as_bytes(),
        &Resolved::in_memory(entries("x"), &[]),
        &sel(&["red"]),
        false,
        &Manifest::test(),
        "n1",
    );
    let html = String::from_utf8(c.html).unwrap();
    assert!(!html.contains("function api(){}") && html.contains("function esc(){}"));
}

#[test]
fn shadow_only_applies_to_shadow_requests() {
    let mut s = sel(&[]);
    s.shadow.insert("red".into());
    let off = compose(
        PAGE.as_bytes(),
        &Resolved::in_memory(entries("x"), &[]),
        &s,
        false,
        &Manifest::test(),
        "n1",
    );
    assert_eq!(off.html, PAGE.as_bytes());
    let on = compose(
        PAGE.as_bytes(),
        &Resolved::in_memory(entries("x"), &[]),
        &s,
        true,
        &Manifest::test(),
        "n1",
    );
    assert!(
        !String::from_utf8(on.html)
            .unwrap()
            .contains("function api(){}")
    );
}

#[test]
fn missing_dependency_keeps_component_off() {
    let mut es = entries("x");
    es.push(Entry::script(
        "dep-user",
        "dash/quick-terminal.js",
        &sha("q"),
        &[],
        &["no-existe"],
    ));
    let c = compose(
        PAGE.as_bytes(),
        &Resolved::in_memory(es, &[("dash/quick-terminal.js", "q")]),
        &sel(&["dep-user"]),
        false,
        &Manifest::test(),
        "n1",
    );
    assert!(matches!(
        c.states["dep-user"],
        ComponentState::MissingDep(_)
    ));
}

#[test]
fn nested_vendor_scripts_match_exact_path_and_restore_original_on_dependency_failure() {
    let original = "<meta charset=\"utf-8\">\n<script data-src='/vendor/markdown.js' src='/other/markdown.js'></script>\n<script src='/vendor/markdown.js?v=1'></script>\n<script src='/news-reader.js'></script>\n<script>const text='vendor/markdown.js';</script>\n";
    let src = "reader";
    let vendor = "vendor";
    let entries = vec![
        Entry::script(
            "news-reader",
            "dash/news-reader.js",
            &sha(src),
            &["NewsReader"],
            &[],
        ),
        Entry::script(
            "vendor-markdown-it",
            "dash/vendor/markdown.js",
            &sha(vendor),
            &[],
            &["news-reader"],
        ),
    ];
    let registry = Resolved::in_memory(
        entries,
        &[
            ("dash/news-reader.js", src),
            ("dash/vendor/markdown.js", vendor),
        ],
    );
    let ready = compose(
        original.as_bytes(),
        &registry,
        &sel(&["news-reader", "vendor-markdown-it"]),
        false,
        &Manifest::test(),
        "ready",
    );
    let ready = String::from_utf8(ready.html).unwrap();
    assert!(ready.contains("src='/other/markdown.js'"));
    assert!(ready.contains("const text='vendor/markdown.js'"));
    assert!(!ready.contains("src='/vendor/markdown.js?v=1'"));
    assert!(!ready.contains("src='/news-reader.js'"));
    let unavailable = compose(
        original.as_bytes(),
        &registry,
        &sel(&["news-reader", "vendor-markdown-it"]),
        false,
        &Manifest::default(),
        "failed",
    );
    assert_eq!(unavailable.html, original.as_bytes());
    let without_reader = compose(
        original.as_bytes(),
        &registry,
        &sel(&["vendor-markdown-it"]),
        false,
        &Manifest::test(),
        "off",
    );
    assert_eq!(without_reader.html, original.as_bytes());
    let different =
        Entry::script("x", "dash/vendor/other.js", &sha(vendor), &[], &[]).cut(original);
    assert_eq!(different, original);
}
