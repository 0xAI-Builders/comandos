#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use comandos_app::{
    workspace_resize::ResizeQueue,
    workspace_view::{prune_for_view, resize_update, view_signature},
};
use serde_json::{Value, json};
use std::collections::BTreeSet;
fn document() -> Value {
    comandos_app::fixture::mixed(std::path::Path::new("/private-home"))["workspace"].clone()
}
fn present() -> BTreeSet<String> {
    BTreeSet::from(["term-fixture-shell".into(), "missing-fixture".into()])
}
#[test]
fn missing_first_leaf_promotes_inner_callback_without_resizing_hidden_outer_split() {
    let authority = document();
    let visible = prune_for_view(&authority["groups"][1]["tree"], &present());
    assert_eq!(visible["axis"], json!("y"));
    let callback = resize_update("split-fixture", &visible, 0.8).unwrap();
    let mut queue = ResizeQueue::default();
    assert_eq!(queue.record_visual(&authority, &[callback]), 0);
    let out = queue.prepare(&authority, false).unwrap();
    assert_eq!(out.document["groups"][1]["tree"]["ratio"], json!(0.55));
    assert_eq!(
        out.document["groups"][1]["tree"]["second"]["ratio"],
        json!(0.8)
    );
}
#[test]
fn missing_second_leaf_and_multiple_collapses_keep_first_branch_authoritative_path() {
    let mut authority = document();
    let inner = authority["groups"][1]["tree"]["second"].clone();
    authority["groups"][1]["tree"] = json!({"type":"split","axis":"x","ratio":0.55,"first":{"type":"split","axis":"x","ratio":0.3,"first":inner,"second":{"type":"tab","tabId":"term-fixture-mixed"}},"second":{"type":"tab","tabId":"local"}});
    authority["groups"].as_array_mut().unwrap().remove(0);
    assert!(comandos_core::workspace::validate_document(&authority).is_ok());
    let visible = prune_for_view(&authority["groups"][0]["tree"], &present());
    let callback = resize_update("split-fixture", &visible, 0.8).unwrap();
    let mut queue = ResizeQueue::default();
    assert_eq!(queue.record_visual(&authority, &[callback]), 0);
    let out = queue.prepare(&authority, false).unwrap();
    assert_eq!(out.document["groups"][0]["tree"]["ratio"], json!(0.55));
    assert_eq!(
        out.document["groups"][0]["tree"]["first"]["ratio"],
        json!(0.3)
    );
    assert_eq!(
        out.document["groups"][0]["tree"]["first"]["first"]["ratio"],
        json!(0.8)
    );
}
#[test]
fn nested_visual_callback_rebases_across_group_reorder_and_rejects_structural_conflict() {
    let mut authority = document();
    let inner = authority["groups"][1]["tree"]["second"].clone();
    authority["groups"][1]["tree"]["second"] = json!({"type":"split","axis":"x","ratio":0.25,"first":inner,"second":{"type":"tab","tabId":"local"}});
    authority["groups"].as_array_mut().unwrap().remove(0);
    let present = BTreeSet::from([
        "term-fixture-mixed".into(),
        "term-fixture-shell".into(),
        "missing-fixture".into(),
    ]);
    let visible = prune_for_view(&authority["groups"][0]["tree"], &present);
    let callback = resize_update("split-fixture", &visible["second"], 0.8).unwrap();
    let mut queue = ResizeQueue::default();
    assert_eq!(
        queue.record_visual(&authority, std::slice::from_ref(&callback)),
        0
    );
    let mut current = authority.clone();
    current["groups"].as_array_mut().unwrap().reverse();
    current["tabs"]["local"]["authority"] = json!("preserve");
    let out = queue.prepare(&current, false).unwrap();
    let group = out.document["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["id"] == "split-fixture")
        .unwrap();
    assert_eq!(group["tree"]["ratio"], json!(0.55));
    assert_eq!(group["tree"]["second"]["ratio"], json!(0.25));
    assert_eq!(group["tree"]["second"]["first"]["ratio"], json!(0.8));
    assert_eq!(
        out.document["tabs"]["local"]["authority"],
        json!("preserve")
    );
    queue.complete(false);
    queue.authority_received();
    current["groups"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|g| g["id"] == "split-fixture")
        .unwrap()["tree"]["second"]["first"]["axis"] = json!("x");
    let rejected = queue.prepare(&current, false).unwrap();
    assert_eq!(rejected.rejected, 1);
    assert!(!rejected.changed);
    assert_eq!(rejected.document, current);
    assert_eq!(
        queue.record_visual(&current, &[callback]),
        1,
        "a callback drained after authority changed must retain its original shape"
    );
    assert!(queue.prepare(&current, false).is_none());
}
#[test]
fn view_cache_signature_retains_authoritative_identity_when_visual_shape_is_equal() {
    let mut authority = document();
    let tree = &authority["groups"][1]["tree"];
    let before = prune_for_view(tree, &present());
    let first = tree["first"].clone();
    let second = tree["second"].clone();
    authority["groups"][1]["tree"]["first"] = second;
    authority["groups"][1]["tree"]["second"] = first;
    let after = prune_for_view(&authority["groups"][1]["tree"], &present());
    assert_eq!(
        comandos_app::workspace_view::shape(&before),
        comandos_app::workspace_view::shape(&after)
    );
    assert_ne!(view_signature(&before), view_signature(&after));
}

#[test]
fn projected_geometry_matches_original_python_prune_for_every_visibility_subset() {
    use comandos_app::proc::{ProcSpec, run};
    fn strip_identity(node: &mut Value) {
        if let Some(object) = node.as_object_mut() {
            object.remove("_resize");
            for value in object.values_mut() {
                strip_identity(value);
            }
        }
    }
    let authority = document();
    let tree = &authority["groups"][1]["tree"];
    let keys = [
        "term-fixture-mixed",
        "term-fixture-shell",
        "missing-fixture",
    ];
    let sets = (0..8)
        .map(|mask| {
            keys.iter()
                .enumerate()
                .filter(|(index, _)| mask & (1 << index) != 0)
                .map(|(_, key)| (*key).to_string())
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    let script = "import ast,json,sys\nnodes=ast.parse(open(sys.argv[1]).read()).body\nexec(compile(ast.Module(body=[n for n in nodes if isinstance(n,ast.FunctionDef) and n.name=='prune'],type_ignores=[]),sys.argv[1],'exec'))\nv=json.load(sys.stdin)\nprint(json.dumps([prune(v['tree'],set(p)) for p in v['sets']]))";
    let out = run(&ProcSpec {
        program: "python3".into(),
        args: vec![
            "-c".into(),
            script.into(),
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../lib/gtk_workspace.py").into(),
        ],
        stdin: Some(serde_json::to_vec(&json!({"tree":tree,"sets":sets})).unwrap()),
        env: vec![
            (
                "HOME".into(),
                "/tmp/comandos-pruned-workspace-oracle-private-home".into(),
            ),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ],
        clear_env: true,
        env_remove: vec![],
        cwd: None,
        timeout: std::time::Duration::from_secs(5),
    })
    .unwrap();
    assert_eq!(
        out.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let oracle: Value = serde_json::from_slice(&out.stdout).unwrap();
    let projected = sets
        .iter()
        .map(|present| {
            let mut projected = prune_for_view(tree, present);
            strip_identity(&mut projected);
            projected
        })
        .collect::<Vec<_>>();
    assert_eq!(json!(projected), oracle);
}
