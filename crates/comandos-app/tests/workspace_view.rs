#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]
use comandos_app::workspace_view::{DockEdge, dock_target, prune, shape, split_paths};
use serde_json::json;
#[path = "support/inert_oracle.rs"]
mod inert_oracle;
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
    let oracle = inert_oracle::oracle(
        "app-workspace-dock",
        script,
        "lib/gtk_workspace.py",
        &input,
        None,
    );
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

#[test]
fn trays_scroll_and_lift_threshold_match_original_python_functions() {
    use comandos_app::workspace_view::{
        DragGesture, DragPhase, strip_edge_step, tray_at, tray_rects,
    };
    let input = json!({"sizes":[[800,600],[220,90]],"points":[[2,70],[55,900],[56,900],[844,900],[845,900]],"trays":[[160,450],[250,500],[300,45],[1000,900]],"moves":[[6,0],[7,0],[0,7],[4,5]]});
    let script = r#"import ast,json,sys
nodes=ast.parse(open(sys.argv[1]).read()).body
names={'ws_tray_rects','ws_tray_at','ws_strip_edge_step','_ws_pointer_move'}
exec(compile(ast.Module(body=[n for n in nodes if isinstance(n,ast.FunctionDef) and n.name in names],type_ignores=[]),sys.argv[1],'exec'))
_WS={}; WS_TRAYS=(('frozen','Aparcar'),('awaiting_reply','Esperando'),('resolved','Hecho'),('none','Quitar'))
class Layer:
 def queue_draw(self):pass
_ws_layer=Layer()
_ws_source_of=lambda w,k:k
_ws_layer_point=lambda w,e:(e.x_root,e.y_root)
_ws_target=lambda x,y:None
def _ws_drag_begin(source,page):_WS['drag']=source
class E:pass
v=json.load(sys.stdin); r=ws_tray_rects(800,600); lifted=[]
for x,y in v['moves']:
 w=object();_WS={'press':{'widget':w,'key':'a','x':0,'y':0,'page':0},'drag':None}
 e=E();e.x_root=x;e.y_root=y;_ws_pointer_move(w,e);lifted.append(_WS['drag'] is not None)
print(json.dumps({'rects':[ws_tray_rects(*size) for size in v['sizes']],'scroll':[ws_strip_edge_step(x,width) for x,width in v['points']],'hits':[ws_tray_at(r,x,y,40) for x,y in v['trays']],'lifted':lifted}))
"#;
    let oracle = inert_oracle::oracle("app-workspace-drag", script, "bin/cc-app", &input, None);
    let rects: Vec<_> = input["sizes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| tray_rects(p[0].as_f64().unwrap(), p[1].as_f64().unwrap()))
        .collect();
    assert!(comandos_core::json::python_eq(
        &json!(rects),
        &oracle["rects"]
    ));
    let scroll: Vec<_> = input["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| strip_edge_step(p[0].as_f64().unwrap(), p[1].as_f64().unwrap()))
        .collect();
    assert_eq!(json!(scroll), oracle["scroll"]);
    let trays = tray_rects(800., 600.);
    let hits: Vec<_> = input["trays"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| tray_at(&trays, p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), 40.))
        .collect();
    assert_eq!(json!(hits), oracle["hits"]);
    let lifted: Vec<_> = input["moves"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let mut g = DragGesture::default();
            g.press("a".into(), (0., 0.), Some(2));
            g.motion((p[0].as_f64().unwrap(), p[1].as_f64().unwrap()), (12., 24.));
            matches!(g.phase, DragPhase::Lifted | DragPhase::Docking)
        })
        .collect();
    assert_eq!(json!(lifted), oracle["lifted"]);
}

#[test]
fn cancelling_lift_keeps_source_and_restores_original_page_without_a_drop() {
    use comandos_app::workspace_view::{DragGesture, DragPhase, root_to_layer};
    let mut gesture = DragGesture::default();
    gesture.press("group:g".into(), (100., 200.), Some(3));
    assert!(gesture.motion((110., 210.), (10., 10.)));
    assert_eq!(gesture.press_page, Some(3));
    assert_eq!(gesture.cancel(), Some(3));
    assert_eq!(gesture.phase, DragPhase::Cancelled);
    assert!(gesture.source.is_none());
    assert_eq!(
        root_to_layer((120.5, 242.), (100., 200.), (5., 12.)),
        (15.5, 30.)
    );
}
#[test]
fn natural_width_rows_match_original_reflow_without_loading_gtk() {
    let input = json!([{ "width":212,"items":[100,100,20] },{"width":120,"items":[140,20,20]},{"width":400,"items":[80,90,100,110]},{"width":44,"items":[]}]);
    let script = r#"import ast,json,sys,types
class Box:
 def __init__(self,**kw):self.children=[]
 def get_children(self):return self.children
 def pack_start(self,item,*args):self.children.append(item)
 def show(self):pass
 def remove(self,item):self.children.remove(item)
 def destroy(self):pass
class Item:
 def __init__(self,index,width):self.index=index;self.width=width
 def get_preferred_width(self):return (self.width,self.width)
 def show(self):pass
Gtk=types.SimpleNamespace(Box=Box,Orientation=types.SimpleNamespace(HORIZONTAL=0))
node=next(n for n in ast.walk(ast.parse(open(sys.argv[1]).read())) if isinstance(n,ast.FunctionDef) and n.name=='_reflow')
exec(compile(ast.Module(body=[node],type_ignores=[]),sys.argv[1],'exec'))
out=[]
for v in json.load(sys.stdin):
 s=types.SimpleNamespace(rows=True,flow=Box(),rows_view=types.SimpleNamespace(get_allocated_width=lambda:v['width']),_wrap=[Item(i,w) for i,w in enumerate(v['items'])],_wrap_signature=lambda _:None)
 _reflow(s)
 out.append([[i.index for i in row.children] for row in s.flow.children])
print(json.dumps(out))
"#;
    let oracle = inert_oracle::oracle(
        "app-workspace-wrap",
        script,
        "lib/gtk_tabstrip.py",
        &input,
        None,
    );
    let actual: Vec<_> = input
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            comandos_app::workspace_view::wrap_rows(
                v["width"].as_i64().unwrap() as i32 - 12,
                &v["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|w| w.as_i64().unwrap() as i32)
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    assert_eq!(json!(actual), oracle);
}
