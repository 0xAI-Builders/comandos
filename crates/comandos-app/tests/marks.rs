#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::ui::marks::{Marks, adopt, indicator_display};
use serde_json::json;
#[test]
fn human_mark_never_hides_ai_channel() {
    assert_eq!(
        indicator_display(&json!("resolved"), "awaiting_input"),
        json!(["ai:need", null, true])
    );
}
#[test]
fn old_poll_and_old_post_cannot_replace_a_new_selection() {
    let mut state =
        adopt(&json!({"marks":[{"scope":"session","key":"a","mark":"none","revision":4}]}));
    let old = state.generation();
    let first = state.begin("session", "a", &json!("frozen")).unwrap();
    assert_eq!(first.body["expectedRevision"], 4);
    let second = state.begin("session", "a", &json!("resolved")).unwrap();
    assert!(!state.adopt_poll(&json!({"marks":[]}), old));
    assert!(!state.finish(
        &first,
        Some(&json!({"scope":"session","key":"a","mark":"frozen","revision":5}))
    ));
    assert!(state.finish(
        &second,
        Some(&json!({"scope":"session","key":"a","mark":"resolved","revision":6}))
    ));
    assert_eq!(state.row("session", "a")["mark"], "resolved");
}
#[test]
fn ambiguous_pane_is_not_a_target_and_local_session_is_refused() {
    let mut state = Marks::default();
    assert!(
        state
            .begin("session", "local", &json!("resolved"))
            .is_none()
    );
    state.adopt_poll(&json!({"panes":[{"session":"a","paneId":"%1","paneKey":"x"},{"session":"a","paneId":"%1","paneKey":"y"}]}),0);
    assert_eq!(state.pane_key("a", "%1"), None);
}

#[test]
fn cached_animation_pixels_are_reused_and_dpr_change_is_bounded() {
    use comandos_app::ui::marks::IndicatorCache;
    let mut cache = IndicatorCache::default();
    let one = cache.pixbuf("ai:need", None, 0, 1, None).unwrap();
    let two = cache.pixbuf("ai:need", None, 0, 1, None).unwrap();
    assert_eq!(one, two);
    assert_eq!(cache.len(), 1);
    for frame in 0..14 {
        cache.pixbuf("ai:need", None, frame, 1, None).unwrap();
    }
    assert_eq!(cache.len(), 14);
    let doubled = cache.pixbuf("ai:need", None, 0, 2, None).unwrap();
    assert_eq!(doubled.width(), 24);
    assert_eq!(cache.len(), 1);
}
#[test]
fn raster_frames_match_actual_python_indicator_functions() {
    use comandos_app::{
        proc::{ProcSpec, run},
        theme::{desktop_theme, themes_from_file},
        ui::marks::IndicatorCache,
    };
    use comandos_core::work_marks as core;
    use sha2::{Digest, Sha256};
    let themes = themes_from_file(None);
    let mut cases = Vec::new();
    for state in ["idle", "work", "need", "done", "error"] {
        for frame in 0..if state == "need" { 14 } else { 1 } {
            for scale in [1, 2, 3] {
                for pixels in [None, Some(8), Some(14)] {
                    cases.push(json!([format!("ai:{state}"), null, frame, scale, pixels]));
                }
            }
        }
    }
    for name in core::MARKS.into_iter().chain(["working", "favorite"]) {
        for frame in 0..core::frame_count(name) {
            for theme in [
                "noche",
                "dia",
                "calido",
                "termius",
                "bruno",
                "superglass",
                "neon",
                "contraste",
                "ubuntu",
            ] {
                let color = desktop_theme(theme, &themes)
                    .unwrap()
                    .values
                    .get("brand")
                    .unwrap()
                    .clone();
                cases.push(json!([name, color, frame, 1, null]));
            }
        }
    }
    let script = r#"import ast,json,sys,os,hashlib
import gi
gi.require_version('GdkPixbuf','2.0')
from gi.repository import GdkPixbuf
sys.path.insert(0,os.path.join(os.path.dirname(os.path.dirname(sys.argv[1])),'lib'))
import work_marks as work_mark_state
ns={'GdkPixbuf':GdkPixbuf,'work_mark_state':work_mark_state,'_INDICATOR_PIXBUFS':{}}
for n in ast.parse(open(sys.argv[1]).read()).body:
 if isinstance(n,ast.FunctionDef) and n.name in {'ai_frame_pixbuf','_indicator_pixbuf','tab_indicator_display'}:exec(compile(ast.Module([n],[]),sys.argv[1],'exec'),ns)
out=[]
for icon,color,frame,scale,pixels in json.load(sys.stdin):
 pb=ns['_indicator_pixbuf'](icon,color,frame,scale,pixels)
 out.append([pb.get_width(),pb.get_height(),hashlib.sha256(pb.get_pixels()).hexdigest()])
print(json.dumps(out))
"#;
    let path = std::env::var("COMANDOS_CC_APP_ORACLE")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/cc-app").into());
    let output = run(&ProcSpec {
        program: "/usr/bin/python3".into(),
        args: vec!["-c".into(), script.into(), path.into()],
        stdin: Some(serde_json::to_vec(&cases).unwrap()),
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: None,
        timeout: std::time::Duration::from_secs(30),
    })
    .unwrap();
    assert_eq!(
        output.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut cache = IndicatorCache::default();
    for (i, case) in cases.iter().enumerate() {
        let pb = cache
            .pixbuf(
                case[0].as_str().unwrap(),
                case[1].as_str(),
                case[2].as_u64().unwrap() as usize,
                case[3].as_i64().unwrap() as i32,
                case[4].as_u64().map(|n| n as u32),
            )
            .unwrap();
        let hash = format!("{:x}", Sha256::digest(pb.read_pixel_bytes().as_ref()));
        assert_eq!(
            json!([pb.width(), pb.height(), hash]),
            expected[i],
            "raster case {i}: {case}"
        );
        assert!(cache.len() <= 48);
    }
}
