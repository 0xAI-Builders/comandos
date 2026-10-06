#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::ui::hourglass::{Hourglass, hourglass_frame, hourglass_sheet_frames};
use serde_json::json;
#[test]
fn hourglass_clock_and_flip_have_distinct_frames() {
    let block = json!({});
    assert_eq!(hourglass_frame(&block, 0., None), 0);
    assert_eq!(hourglass_frame(&block, 0., Some(0.)), 21);
    assert_eq!(hourglass_frame(&block, 110., Some(0.)), 22);
}
#[test]
fn completed_block_flips_once_and_uses_server_clock() {
    let mut clock = Hourglass::default();
    clock.adopt(
        &json!({"serverNowMs":2000,"block":{"blockId":"a","status":"running"}}),
        1000.,
    );
    assert_eq!(clock.offset, 1000.);
    clock.adopt(
        &json!({"serverNowMs":3000,"block":{"blockId":"a","status":"completed"}}),
        2000.,
    );
    assert_eq!(clock.flip, Some(3000.));
    clock.adopt(
        &json!({"serverNowMs":3100,"block":{"blockId":"a","status":"completed"}}),
        2100.,
    );
    assert_eq!(clock.flip, Some(3000.));
}
#[test]
fn source_rectangles_ignore_output_scale() {
    assert_eq!(hourglass_sheet_frames(864, 32, 0.75).len(), 27);
    assert_eq!(
        hourglass_sheet_frames(64, 12, 2.),
        vec![(0, 0, 32, 12), (32, 0, 32, 12)]
    );
}

#[test]
fn sprites_are_native_frames_and_dpr_reload_replaces_cache() {
    use comandos_app::ui::hourglass::Sprites;
    let mut sprites = Sprites::default();
    sprites
        .load(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/pomodoro/comandos/hourglass.png"
            ),
            1,
        )
        .unwrap();
    assert_eq!(sprites.len(), 27);
    assert_eq!(sprites.frame(26, false).unwrap().width(), 24);
    sprites
        .load(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/pomodoro/comandos/hourglass.png"
            ),
            2,
        )
        .unwrap();
    assert_eq!(sprites.len(), 27);
    assert_eq!(sprites.frame(99, true).unwrap().width(), 48);
    compare_original_sprite_pixels();
}

fn compare_original_sprite_pixels() {
    use comandos_app::proc::{ProcSpec, run};
    use sha2::{Digest, Sha256};
    let script = r#"import ast,json,sys
import gi
gi.require_version('GdkPixbuf','2.0')
from gi.repository import GdkPixbuf
ns={'GdkPixbuf':GdkPixbuf,'HOURGLASS_SHEET':sys.argv[2],'sys':sys}
for n in ast.parse(open(sys.argv[1]).read()).body:
 names={n.name} if isinstance(n,ast.FunctionDef) else {t.id for target in n.targets for t in ast.walk(target) if isinstance(t,ast.Name)} if isinstance(n,ast.Assign) else set()
 if names&{'_HOURGLASS','HOURGLASS_FRAME_PX','hourglass_sheet_frames','_hourglass_frames'}:exec(compile(ast.Module([n],[]),sys.argv[1],'exec'),ns)
import hashlib
out=[]
for scale in [1,2,3]:
 ns['_HOURGLASS']['frames']=[];ns['_HOURGLASS']['dim']=[];ns['_hourglass_frames'](0.75*scale)
 for dim in [False,True]:
  for i,pb in enumerate(ns['_HOURGLASS']['dim' if dim else 'frames']):out.append([scale,dim,i,pb.get_width(),pb.get_height(),hashlib.sha256(pb.get_pixels()).hexdigest()])
print(json.dumps(out))
"#;
    let oracle = std::env::var("COMANDOS_CC_APP_ORACLE")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/cc-app").into());
    let asset = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/pomodoro/comandos/hourglass.png"
    );
    let output = run(&ProcSpec {
        program: "/usr/bin/python3".into(),
        args: vec!["-c".into(), script.into(), oracle.into(), asset.into()],
        stdin: None,
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: None,
        timeout: std::time::Duration::from_secs(10),
    })
    .unwrap();
    assert_eq!(
        output.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut actual = Vec::new();
    let mut sprites = comandos_app::ui::hourglass::Sprites::default();
    for scale in [1, 2, 3] {
        sprites.load(asset, scale).unwrap();
        assert_eq!(sprites.len(), 27);
        for dim in [false, true] {
            for frame in 0..27 {
                let pb = sprites.frame(frame, dim).unwrap();
                let pixels = pb.read_pixel_bytes();
                actual.push(json!([
                    scale,
                    dim,
                    frame,
                    pb.width(),
                    pb.height(),
                    format!("{:x}", Sha256::digest(pixels.as_ref()))
                ]));
            }
        }
    }
    assert_eq!(actual.len(), 162);
    assert_eq!(json!(actual), expected);
}

#[test]
fn empty_block_does_not_arm_a_completion_flip() {
    let mut clock = Hourglass::default();
    clock.adopt(&json!({"block":{}}), 0.);
    clock.adopt(&json!({"block":{"status":"completed","blockId":"a"}}), 100.);
    assert_eq!(clock.flip, None);
}

#[test]
fn clocks_rectangles_and_adoption_match_actual_original_ast() {
    use comandos_app::proc::{ProcSpec, run};
    let times: Vec<_> = (-100..=1200).map(|i| i as f64 * 37.).collect();
    let rectangles = vec![
        (0, 0, 0.75),
        (1, 16, 2.),
        (31, 32, 1.),
        (32, 64, 1.),
        (864, 32, 0.75),
        (865, 31, 2.),
    ];
    let sequence = vec![
        json!({}),
        json!({"block":{}}),
        json!({"serverNowMs":3000,"block":{"status":"running","blockId":"a"}}),
        json!({"serverNowMs":4000,"block":{"status":"paused","blockId":"a"}}),
        json!({"serverNowMs":5000,"block":{"status":"completed","blockId":"a"}}),
        json!({"serverNowMs":6000,"block":{"status":"completed","blockId":"a"}}),
        json!({"block":null}),
        json!({"serverNowMs":8000,"block":{"status":"completed","blockId":"b"}}),
    ];
    let script = r#"import ast,json,sys,types
source=open(sys.argv[1]).read();tree=ast.parse(source)
names={'hourglass_sheet_frames','hourglass_frame','_hourglass_adopt','_HOURGLASS','HOURGLASS_FRAME_PX'}
ns={}
for n in tree.body:
 found={n.name} if isinstance(n,ast.FunctionDef) else {x.id for x in ast.walk(n.targets[0]) if isinstance(x,ast.Name)} if isinstance(n,ast.Assign) else set()
 if found&names:exec(compile(ast.Module([n],[]),sys.argv[1],'exec'),ns)
data=json.load(sys.stdin);out={'frames':[],'rectangles':[],'adopt':[]}
for t in data['times']:
 for flip in [None,0,10000]:out['frames'].append(ns['hourglass_frame']({},t,flip))
for w,h,s in data['rectangles']:out['rectangles'].append(ns['hourglass_sheet_frames'](w,h,s))
for i,payload in enumerate(data['sequence']):
 now=(i+1)*1000;ns['time']=types.SimpleNamespace(time=lambda:now/1000);ns['_hourglass_adopt'](payload)
 out['adopt'].append({k:ns['_HOURGLASS'][k] for k in ['block','offset','flip']})
print(json.dumps(out))
"#;
    let path = std::env::var("COMANDOS_CC_APP_ORACLE")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/cc-app").into());
    let output = run(&ProcSpec {
        program: "python3".into(),
        args: vec!["-c".into(), script.into(), path.into()],
        stdin: Some(
            serde_json::to_vec(&json!({"times":times,"rectangles":rectangles,"sequence":sequence}))
                .unwrap(),
        ),
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: None,
        timeout: std::time::Duration::from_secs(5),
    })
    .unwrap();
    assert_eq!(
        output.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let frames: Vec<_> = times
        .iter()
        .flat_map(|t| {
            [None, Some(0.), Some(10000.)].map(|flip| hourglass_frame(&json!({}), *t, flip))
        })
        .collect();
    let rectangles: Vec<_> = rectangles
        .iter()
        .map(|(w, h, s)| hourglass_sheet_frames(*w, *h, *s))
        .collect();
    let mut clock = Hourglass::default();
    let observations:Vec<_>=sequence.iter().enumerate().map(|(i,p)|{clock.adopt(p,(i+1) as f64*1000.);json!({"block":clock.block,"offset":clock.offset as i64,"flip":clock.flip.map(|n|n as i64)})}).collect();
    assert_eq!(json!(observations), expected["adopt"]);
    assert_eq!(json!(rectangles), expected["rectangles"]);
    for (i, frame) in frames.iter().enumerate() {
        assert_eq!(json!(frame), expected["frames"][i], "case {i}");
    }
}
