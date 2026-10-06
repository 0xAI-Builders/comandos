#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_extensions::config;
use comandos_store::unified::{self, Mode};
use std::{fs, os::unix::fs::DirBuilderExt};
#[test]
fn extension_documents_round_trip_every_mode() {
    let home = std::env::temp_dir().join(format!("ext-domain-{}", std::process::id()));
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&home)
        .unwrap();
    let db = unified::open_unified(&unified::unified_path(&home)).unwrap();
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        unified::set_mode(&db, "extensions", mode, "test", 1).unwrap();
        for name in [
            "snapshot.json",
            "skills.json",
            "client-policies.json",
            "last-check.json",
        ] {
            let path = config::state_dir(&home).join(name);
            if mode == Mode::Sealed {
                fs::remove_file(&path).unwrap();
            }
            let value = serde_json::json!({"mode":format!("{mode:?}"),"unicode":"ñ"});
            config::save_json(&path, &value).unwrap();
            assert_eq!(config::read_config(&path).unwrap(), value);
            if mode != Mode::Legacy {
                assert_eq!(
                    unified::doc_get(&db, &format!("state/extensions/{name}"))
                        .unwrap()
                        .unwrap()
                        .body,
                    config::json_bytes(&value).unwrap()
                );
            }
            assert_eq!(path.exists(), mode != Mode::Sealed);
        }
    }
    fs::remove_dir_all(home).unwrap();
}

#[tokio::test]
async fn extension_sizes_round_trip_every_mode() {
    use comandos_extensions::{metadata, python_json};
    let home = std::env::temp_dir().join(format!("ext-size-domain-{}", std::process::id()));
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&home)
        .unwrap();
    let db = unified::open_unified(&unified::unified_path(&home)).unwrap();
    let spec = serde_json::json!({"command":"private"});
    let tools = [serde_json::json!({"name":"one","inputSchema":{}})];
    let digest = python_json::digest(&serde_json::json!("one")).unwrap();
    let path = config::state_dir(&home)
        .join("sizes")
        .join(format!("{digest}.json"));
    for (i, mode) in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed]
        .into_iter()
        .enumerate()
    {
        unified::set_mode(&db, "extensions", mode, "test", 1).unwrap();
        if mode == Mode::Sealed {
            fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }
        let now = 100000.0 + i as f64 * 100000.0;
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let counter = |_: Vec<String>| {
            calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            async {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                vec![Some(42)]
            }
        };
        tokio::join!(
            metadata::record_mcp_size(&home, "one", &spec, &tools, now, &counter),
            metadata::record_mcp_size(&home, "one", &spec, &tools, now, &counter)
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(metadata::mcp_size(&home, "one", &spec, now)["tokens"], 42);
        assert_eq!(path.exists(), mode != Mode::Sealed);
        if mode != Mode::Legacy {
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(
                    &unified::doc_get(&db, &format!("state/extensions/sizes/{digest}.json"))
                        .unwrap()
                        .unwrap()
                        .body
                )
                .unwrap()["tokens"],
                42
            );
        }
        assert_eq!(path.parent().unwrap().exists(), mode != Mode::Sealed);
    }
    fs::remove_dir_all(home).unwrap();
}
