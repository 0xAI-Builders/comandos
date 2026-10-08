#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]
use comandos_app::ui::side::SideState;
use serde_json::json;
#[test]
fn side_layout_preserves_unknown_fields_and_bounds_web_geometry() {
    let raw = json!({"leftHidden":true,"leftPos":490,"termShare":0.42,"unknown":{"ñ":"😀"}});
    let mut side = SideState::from_json(&raw).unwrap();
    assert_eq!(side.to_json(), raw);
    side.adopt_web(&json!({"session":"fixture","hidden":false,"cmds":{"open":false,"h":"9000"},"tabs":[{"id":"fixture","label":"á","on":true},{"id":"bad;id"}]}));
    assert_eq!(side.cmds_h, 4000);
    assert_eq!(side.tabs.len(), 1);
    assert_eq!(side.session, "fixture");
    side.collapsed = false;
    assert_eq!(side.pin_position(true, 1.25, 0, 10000), Some(5000));
    side.collapsed = true;
    assert_eq!(side.pin_position(true, 1.25, 0, 10000), None);
    assert_eq!(side.to_json(), raw);
}
#[path = "support/t18_oracle.rs"]
mod oracle;
#[test]
fn actual_original_side_tabs_pin_share_and_drag_match_native() {
    use comandos_app::ui::side::{drag_position, side_tabs};
    let tabs = json!([{"id":"fixture","label":"ñ😀".repeat(30),"title":"t".repeat(150),"on":true,"sel":1,"closing":true},{"id":"bad;id"},null,{"id":"local"}]);
    assert_eq!(
        json!(side_tabs(&tabs)),
        oracle::original(json!({"op":"side-tabs","tabs":tabs}))
    );
    for h in [0, 39, 40, 41, 4000] {
        for z in [0.0, 1.0, 1.25, 2.0] {
            for collapsed in [false, true] {
                let mut s = SideState::default();
                s.cmds_h = h;
                s.cmds_closed = true;
                s.collapsed = collapsed;
                let expected = oracle::original(
                    json!({"op":"pin","state":{"cmds_h":h,"cmds_closed":true,"collapsed":collapsed},"zoom":z}),
                );
                assert_eq!(json!(s.pin_position(true, z, 0, 10000)), expected);
            }
        }
    }
    for share in [-1.0, 0.15, 0.42, 0.85, 2.0] {
        let mut s = SideState::from_json(&json!({"termShare":share})).unwrap();
        s.collapsed = false;
        let expected = oracle::original(
            json!({"op":"share","state":{"termShare":share,"collapsed":false},"total":1000,"position":200}),
        );
        assert_eq!(json!(s.share_position(1000, true, None).unwrap()), expected);
    }
    for y in [-20.5, 0.0, 100.5, 3000.0] {
        assert_eq!(
            json!(drag_position((20.0, 200), y, 0, 700)),
            oracle::original(json!({"op":"drag","drag":[20,200],"y":y,"bounds":[0,700]}))
        );
    }
}
#[test]
fn layout_document_authority_guard_cas_and_cancel_cover_four_modes() {
    use comandos_app::{config::parse_args, guard::WriteGuard, state_files::StateFiles};
    use comandos_store::unified::{self, Mode};
    use std::os::unix::fs::PermissionsExt;
    for (i, mode) in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed]
        .into_iter()
        .enumerate()
    {
        let root = std::env::temp_dir().join(format!("t18-layout-{}-{i}", std::process::id()));
        for d in ["home", "hooks", "run", "tmp"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
            std::fs::set_permissions(root.join(d), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let cfg = parse_args(
            &[
                "--mode".into(),
                "live".into(),
                "--hooks-dir".into(),
                root.join("hooks").display().to_string(),
            ],
            false,
            &|k| match k {
                "HOME" => Some(root.join("home").display().to_string()),
                "XDG_RUNTIME_DIR" => Some(root.join("run").display().to_string()),
                "TMPDIR" => Some(root.join("tmp").display().to_string()),
                _ => None,
            },
        )
        .unwrap();
        let files = StateFiles::new(cfg.clone(), WriteGuard::from_config(&cfg, ":fixture"));
        for name in ["app-layout.json", "app-pane-position.json"] {
            let old = json!({"leftPos":300,"position":310,"unknown":{"ñ":"😀"}});
            let db = unified::open_unified(&unified::unified_path(cfg.home())).unwrap();
            // Each document starts with the same legacy fixture, including the second
            // document after the previous iteration changed this shared domain's mode.
            unified::set_mode(&db, "app-ui", Mode::Legacy, "fixture", 0).unwrap();
            files.write(name, &old).unwrap();
            let path = root.join("hooks").join(name);
            let lock = root.join("hooks").join(format!("{name}.lock"));
            std::fs::remove_file(&lock).unwrap();
            let key = format!("hooks/{name}");
            let authoritative = json!({"leftPos":900,"position":910});
            unified::doc_put(
                &db,
                &key,
                "app-ui",
                comandos_core::json::response_dumps(&authoritative)
                    .unwrap()
                    .as_bytes(),
                unified::Origin::Import,
                1,
            )
            .unwrap();
            unified::set_mode(&db, "app-ui", mode, "fixture", 1).unwrap();
            let expected = if matches!(mode, Mode::Unified | Mode::Sealed) {
                authoritative.clone()
            } else {
                old.clone()
            };
            assert_eq!(
                files.read_ui_document(name).unwrap(),
                expected,
                "{mode:?}/{name}: authority"
            );
            let snapshot = || {
                let origin: String = db
                    .query_row(
                        "SELECT origin FROM documents WHERE name=?1",
                        [&key],
                        |row| row.get(0),
                    )
                    .unwrap();
                (
                    std::fs::read(&path).unwrap(),
                    unified::doc_get(&db, &key).unwrap().unwrap(),
                    origin,
                )
            };
            let initial = snapshot();
            let next = json!({"leftPos":490,"position":510,"unknown":{"ñ":"😀"}});
            if matches!(mode, Mode::Unified | Mode::Sealed) {
                assert!(
                    !files
                        .write_ui_document_when(name, &old, &next, || true)
                        .unwrap(),
                    "{mode:?}/{name}: stale legacy CAS"
                );
                assert_eq!(
                    snapshot(),
                    initial,
                    "{mode:?}/{name}: stale CAS preserves state"
                );
            }
            assert!(
                files
                    .write_ui_document_when(name, &expected, &next, || true)
                    .unwrap(),
                "{mode:?}/{name}: authoritative CAS succeeds"
            );
            assert_eq!(
                files.read_ui_document(name).unwrap(),
                next,
                "{mode:?}/{name}: new authority"
            );
            let published = snapshot();
            assert_eq!(
                comandos_core::json::workspace_loads_bytes(&published.0).unwrap(),
                if mode == Mode::Sealed {
                    old.clone()
                } else {
                    next.clone()
                },
                "{mode:?}/{name}: legacy mirror"
            );
            if mode == Mode::Legacy {
                assert_eq!(published.1, initial.1, "{mode:?}/{name}: DB untouched");
                assert_eq!(published.2, "import", "{mode:?}/{name}: origin untouched");
            } else {
                assert_eq!(
                    comandos_core::json::workspace_loads_bytes(&published.1.body).unwrap(),
                    next,
                    "{mode:?}/{name}: DB body"
                );
                assert_eq!(
                    published.1.revision,
                    initial.1.revision + 1,
                    "{mode:?}/{name}: revision"
                );
                assert_eq!(
                    published.2,
                    if mode == Mode::Mirror {
                        "mirror"
                    } else {
                        "unified"
                    },
                    "{mode:?}/{name}: origin"
                );
            }
            assert_eq!(
                lock.exists(),
                mode != Mode::Sealed,
                "{mode:?}/{name}: lock policy"
            );
            assert!(
                !files
                    .write_ui_document_when(name, &expected, &json!({"bad":true}), || true)
                    .unwrap(),
                "{mode:?}/{name}: outdated CAS"
            );
            assert_eq!(
                snapshot(),
                published,
                "{mode:?}/{name}: outdated CAS preserves state"
            );
            assert!(
                !files
                    .write_ui_document_when(name, &next, &json!({"bad":true}), || false)
                    .unwrap(),
                "{mode:?}/{name}: cancellation"
            );
            assert_eq!(
                snapshot(),
                published,
                "{mode:?}/{name}: cancellation preserves state"
            );
            assert_eq!(
                lock.exists(),
                mode != Mode::Sealed,
                "{mode:?}/{name}: final lock policy"
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn actual_original_pane_initialization_uses_python_numbers_and_rounding() {
    use comandos_app::ui::side::pane_initial_position;
    for width in [800, 900, 1000, 1150, 2000] {
        for position in [
            json!(null),
            json!(false),
            json!(true),
            json!(0),
            json!(-50),
            json!(310.9),
            json!("350"),
            json!("broken"),
            json!(9999),
        ] {
            let doc = json!({"position":position});
            assert_eq!(
                json!(pane_initial_position(width, &doc)),
                oracle::original(json!({"op":"pane-init","width":width,"document":doc}))
            );
        }
    }
}

#[test]
fn actual_original_wheel_prefers_vertical_delta_and_moves_sixty_pixels() {
    for (x, y, discrete) in [
        (2., 1., None),
        (2., 0., None),
        (0., -1., None),
        (0., 0., Some(-1.)),
        (0., 0., Some(1.)),
        (0., 0., None),
    ] {
        let expected = oracle::original(json!({"op":"wheel","dx":x,"dy":y,"discrete":discrete}));
        assert_eq!(
            json!(comandos_app::ui::side::wheel_delta(x, y, discrete)),
            expected
        );
    }
}
