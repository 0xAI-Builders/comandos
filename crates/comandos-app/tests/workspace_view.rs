#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::workspace_view::{DockEdge, dock_target, prune, shape, split_paths};
use serde_json::json;
use std::collections::BTreeSet;

#[test]
fn no_layout_has_no_dock_target() {
    assert!(dock_target(&serde_json::json!([]), 10.0, 10.0, &BTreeSet::new()).is_none());
}

#[test]
fn prune_collapses_absent_leaves_and_keeps_shape() {
    let tree = json!({"type":"split","axis":"x","ratio":0.4,"first":{"type":"tab","tabId":"a"},"second":{"type":"tab","tabId":"b"}});
    assert_eq!(
        prune(&tree, &BTreeSet::from(["b".to_string()])),
        json!({"type":"tab","tabId":"b"})
    );
    assert_eq!(shape(&tree), json!(["x", "a", "b"]));
    assert_eq!(split_paths(&tree), vec![(vec![], 0.4)]);
}

#[test]
fn moved_origin_cannot_be_dock_destination() {
    let doc = json!({"groups":[{"id":"g","tree":{"type":"tab","tabId":"a"}}]});
    assert!(dock_target(&doc, 0.5, 0.5, &BTreeSet::from(["a".to_string()])).is_none());
    assert_eq!(
        dock_target(&doc, 0.5, 0.5, &BTreeSet::new()).unwrap().edge,
        DockEdge::Left
    );
}

#[test]
fn absent_workspace_focus_is_false_and_terminates() {
    let view = comandos_app::workspace_view::WorkspaceView::default();
    assert!(!view.select("missing"));
    view.apply(&json!({"groups":[{"id":"g","tree":{"type":"tab","tabId":"present"}}]}));
    assert!(view.select("present"));
    assert!(!view.select("missing"));
}

#[test]
fn measured_dock_targets_and_shapes_match_python_ast_oracle() {
    use comandos_app::proc::{ProcSpec, run};
    use std::time::Duration;
    let layout = json!({"strip":[0,0,600,40],"entries":[["a",[0,0,100,40]],["b",[100,0,80,40]]],"area":[0,40,600,400],"leaves":{"a":[0,40,300,400],"b":[300,40,300,400]},"active":"group-a","activeTabs":["a","b"]});
    let points = json!([
        [10, 20],
        [130, 20],
        [4, 90],
        [150, 240],
        [450, 100],
        [290, 240],
        [900, 900]
    ]);
    let tree = json!({"type":"split","axis":"x","ratio":0.4,"first":{"type":"tab","tabId":"a"},"second":{"type":"tab","tabId":"b"}});
    let input = json!({"layout":layout,"points":points,"tree":tree});
    let script = r#"import ast,json,sys
nodes=ast.parse(open(sys.argv[1]).read()).body
names={'_inside','_nearest','_half','dock_target','shape','prune','split_paths'}
exec(compile(ast.Module(body=[n for n in nodes if isinstance(n,ast.FunctionDef) and n.name in names],type_ignores=[]),sys.argv[1],'exec'))
OUTER_PX=16
v=json.load(sys.stdin)
print(json.dumps({'targets':[dock_target(v['layout'],x,y,{'a'}) for x,y in v['points']],'shape':shape(v['tree']),'pruned':prune(v['tree'],{'b'}),'paths':split_paths(v['tree'])}))
"#;
    let output = run(&ProcSpec {
        program: "python3".into(),
        args: vec![
            "-c".into(),
            script.into(),
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../lib/gtk_workspace.py").into(),
        ],
        stdin: Some(serde_json::to_vec(&input).unwrap()),
        env: vec![
            (
                "HOME".into(),
                "/tmp/comandos-workspace-oracle-private-home".into(),
            ),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ],
        clear_env: true,
        env_remove: vec![],
        cwd: None,
        timeout: Duration::from_secs(5),
    })
    .unwrap();
    assert_eq!(
        output.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let oracle: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let targets: Vec<_> = points
        .as_array()
        .unwrap()
        .iter()
        .map(|point| {
            comandos_app::workspace_view::dock_hit(
                &layout,
                point[0].as_f64().unwrap(),
                point[1].as_f64().unwrap(),
                &BTreeSet::from(["a".into()]),
            )
            .map(|hit| hit.to_json())
        })
        .collect();
    assert!(comandos_core::json::python_eq(
        &json!(targets),
        &oracle["targets"]
    ));
    assert_eq!(shape(&tree), oracle["shape"]);
    assert_eq!(
        prune(&tree, &BTreeSet::from(["b".into()])),
        oracle["pruned"]
    );
    let paths: Vec<_> = split_paths(&tree)
        .into_iter()
        .map(|(path, _)| {
            path.into_iter()
                .map(|step| if step == 0 { "first" } else { "second" })
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(json!(paths), oracle["paths"]);
}
