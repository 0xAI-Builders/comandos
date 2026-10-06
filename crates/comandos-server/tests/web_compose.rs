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
