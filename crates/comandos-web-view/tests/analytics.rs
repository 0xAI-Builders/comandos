use comandos_web_view::analytics::Renderer;
use serde_json::{Value, json};
#[test]
fn approved_analytics_markup() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/analytics");
    let refs: Value =
        serde_json::from_slice(&std::fs::read(dir.join("reference.json")).unwrap()).unwrap();
    for (key, want) in refs.as_object().unwrap() {
        let pieces: Vec<_> = key.split('|').collect();
        let model: Value = serde_json::from_slice(
            &std::fs::read(dir.join(format!("{}.json", pieces[0]))).unwrap(),
        )
        .unwrap();
        let got = Renderer::new(model, json!({"phone":pieces[2]=="phone"})).html(pieces[1]);
        let want = want.as_str().unwrap();
        if got != want {
            let i = got
                .chars()
                .zip(want.chars())
                .position(|(a, b)| a != b)
                .unwrap_or(got.len().min(want.len()));
            let a: String = got.chars().skip(i.saturating_sub(60)).take(180).collect();
            let b: String = want.chars().skip(i.saturating_sub(60)).take(180).collect();
            panic!("{key}: at {i}\nWANT {b}\nGOT  {a}");
        }
    }
}
