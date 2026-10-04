//! `valid_snapshot` de lib/tmux_snapshot.py: válido, inválido o exótico (declinar).
use comandos_core::workspace::snapshot::{Snapshot, check_snapshot, layout_checksum, leaf_ids};
use serde_json::{Value, json};

const ONE: &str = "b25e,80x24,0,0,1";
const TWO: &str = "020a,80x24,0,0{40x24,0,0,1,39x24,41,0,2}";

fn snapshot(layout: &str, panes: Value) -> Value {
    json!({"version": 2, "sessions": {"s1": {"windows": [
        {"index": 0, "name": "w", "active": 1, "width": 80, "height": 24, "layout": layout, "panes": panes}
    ]}}})
}

fn two_panes() -> Value {
    json!([{"id": "%1", "active": true}, {"id": "%2", "active": false}])
}

#[test]
fn checksum_and_leaves_match_python() {
    assert_eq!(format!("{:04x}", layout_checksum("80x24,0,0,1")), "b25e");
    assert_eq!(
        format!(
            "{:04x}",
            layout_checksum("80x24,0,0{40x24,0,0,1,39x24,41,0,2}")
        ),
        "020a"
    );
    assert_eq!(leaf_ids(TWO), ["1", "2"]);
    assert_eq!(
        leaf_ids("80x24,0,0,7\n"),
        ["7"],
        "`$` casa antes del salto final"
    );
    assert_eq!(leaf_ids("80x24,0,0,7x"), Vec::<&str>::new());
}

#[test]
fn valid_snapshots() {
    assert_eq!(check_snapshot(&snapshot(TWO, two_panes())), Snapshot::Valid);
    assert_eq!(
        check_snapshot(&snapshot(ONE, json!([{"id": "%1", "active": 1}]))),
        Snapshot::Valid
    );
    assert_eq!(
        check_snapshot(&json!({"version": 2.0, "sessions": {}})),
        Snapshot::Valid
    );
}

#[test]
fn invalid_snapshots() {
    let mut cases = vec![
        json!({"version": 3, "sessions": {}}),
        json!({"version": 2, "sessions": []}),
        json!({"version": 2, "sessions": {"s": []}}),
        json!({"version": 2, "sessions": {"s": {"windows": []}}}),
        snapshot("0000,80x24,0,0{40x24,0,0,1,39x24,41,0,2}", two_panes()),
        snapshot(
            TWO,
            json!([{"id": "%1", "active": true}, {"id": "%3", "active": false}]),
        ),
        snapshot(
            TWO,
            json!([{"id": "%1", "active": true}, {"id": "%2", "active": true}]),
        ),
        snapshot("80x24", json!([{"id": "%1", "active": true}])),
        snapshot(&format!("{TWO}\n"), two_panes()),
    ];
    let mut no_name = snapshot(TWO, two_panes());
    no_name["sessions"]["s1"]["windows"][0]["name"] = json!(5);
    cases.push(no_name);
    let mut narrow = snapshot(TWO, two_panes());
    narrow["sessions"]["s1"]["windows"][0]["width"] = json!(0);
    cases.push(narrow);
    let mut dup = snapshot(TWO, two_panes());
    let window = dup["sessions"]["s1"]["windows"][0].clone();
    dup["sessions"]["s1"]["windows"] = json!([window.clone(), {"index": 0, "active": 0, "name": "x", "width": 1, "height": 1, "layout": ONE, "panes": [{"id": "%1", "active": true}]}]);
    cases.push(dup);
    for case in cases {
        assert_eq!(check_snapshot(&case), Snapshot::Invalid, "{case}");
    }
}

#[test]
fn exotic_snapshots_decline() {
    // Python acepta `index: true` (bool es int): Rust no lo reproduce, declina.
    let mut flag = snapshot(TWO, two_panes());
    flag["sessions"]["s1"]["windows"][0]["index"] = json!(true);
    assert_eq!(check_snapshot(&flag), Snapshot::Exotic);
    let mut wide = snapshot(TWO, two_panes());
    wide["sessions"]["s1"]["windows"][0]["width"] = json!(true);
    assert_eq!(check_snapshot(&wide), Snapshot::Exotic);
    assert_eq!(
        check_snapshot(&snapshot("020a,80x24,0,0,١", two_panes())),
        Snapshot::Exotic
    );
}
