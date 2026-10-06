//! Entire 2f JSONL fixture set on paired temporary HOME/private tmux, never live state.
mod support;
use serde_json::Value;
use std::{fs, path::Path};
use support::{
    TestHome,
    twin::{Twin, TwinOpts},
};
fn normalized(text: &str, home: &TestHome, volatile: &[Value]) -> String {
    let text = text.replace(home.root.to_str().unwrap(), "HOME");
    let text = if let Ok(mut value) = serde_json::from_str::<Value>(&text) {
        for pointer in volatile.iter().filter_map(Value::as_str) {
            if let Some(slot) = value.pointer_mut(pointer) {
                *slot = Value::Null;
            }
        }
        // JSON formatting is not this runner's concern; dedicated oracles compare bytes.
        value.to_string()
    } else {
        text
    };
    // Existing timestamp normalization expects Python's colon-space formatting.
    support::twin::normalize(&text.replace("\":", "\": "))
}
#[tokio::test]
async fn all_2f_fixtures_match_in_one_confined_campaign() {
    let twin = Twin::start_with(
        "parity-2f-union",
        |_| {},
        TwinOpts {
            fakebin_extra: vec![("fc-list".into(), "#!/bin/sh\nexit 0\n".into())],
            ..Default::default()
        },
    )
    .await
    .expect("private twin is required for integrated parity");
    let mut total = 0;
    for domain in ["residue", "tabs", "ops", "services", "news"] {
        let file = support::repo().join(format!("xtask/parity/2f/{domain}.jsonl"));
        for line in fs::read_to_string(file)
            .unwrap()
            .lines()
            .filter(|l| !l.trim().is_empty())
        {
            let row: Value = serde_json::from_str(line).unwrap();
            let name = row["name"].as_str().unwrap();
            if let Some(setup) = row["setup"].as_array() {
                for command in setup {
                    let args: Vec<_> = command
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|s| s.as_str().unwrap())
                        .collect();
                    twin.tmux_a(&args);
                    twin.tmux_b(&args);
                }
            }
            let body = row
                .get("body")
                .filter(|v| !v.is_null())
                .map(Value::to_string)
                .unwrap_or_default();
            let pair = twin
                .request(
                    row["method"].as_str().unwrap(),
                    row["path"].as_str().unwrap(),
                    &body,
                )
                .await;
            let volatile = row["volatile"].as_array().cloned().unwrap_or_default();
            let a = (
                pair.front.status,
                normalized(&pair.front.text(), &twin.a, &volatile),
            );
            let b = (
                pair.oracle.status,
                normalized(&pair.oracle.text(), &twin.b, &volatile),
            );
            if row["expect"] == "differs" {
                assert_eq!(a.0, 410, "{name}: declared retirement");
                assert_ne!(a, b, "{name}: expected documented difference");
            } else {
                assert_eq!(a, b, "{domain}/{name}");
            }
            for file in row["files"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                let read = |home: &TestHome| {
                    let path = match file.strip_prefix("~/") {
                        Some(p) => home.root.join(p),
                        None => home.hooks().join(Path::new(file)),
                    };
                    fs::read_to_string(path)
                        .ok()
                        .map(|s| normalized(&s, home, &[]))
                };
                assert_eq!(read(&twin.a), read(&twin.b), "{name}: {file}");
            }
            total += 1;
            eprintln!("parity2f {domain}/{name} PASS");
        }
    }
    assert!(total >= 72);
    eprintln!("parity2f total={total}; private HOME/socket; no live services");
}
