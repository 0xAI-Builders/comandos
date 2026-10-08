// Filesystem writes below create only injected private fixtures under TMPDIR.
#![allow(clippy::disallowed_methods)]
//! The terminal must route every clipboard interaction through its owned T16 port.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::ui::menu::{MenuRow, OpenIntent, clean_local_path, open_plan, terminal_rows};
#[path = "support/t16_oracle.rs"]
mod oracle;
#[test]
fn clipboard_io_has_one_owned_main_context_boundary() {
    let view = include_str!("../src/term/view.rs");
    assert!(
        !view.contains("gtk::Clipboard::get"),
        "OSC52, selection and middle paste bypass the owned mode/instance clipboard boundary"
    );
}
#[test]
fn local_and_ssh_menu_labels_sensitivity_and_order_match_original_ast() {
    for (session, es) in [
        ("fixture", false),
        ("ssh-fixture", false),
        ("fixture", true),
        ("ssh-fixture", true),
    ] {
        let rows = terminal_rows(!es, Some(session), Some("%1"), 2, true);
        let actual = rows
            .iter()
            .filter_map(|row| match row {
                MenuRow::Action { label, enabled, .. } => {
                    Some(serde_json::json!({"label":label,"enabled":enabled}))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            serde_json::json!(actual),
            oracle::original(serde_json::json!({"op":"menu","session":session,"es":es}))
        );
        assert_eq!(
            actual.len(),
            if session.starts_with("ssh-") { 19 } else { 18 }
        );
    }
}
#[test]
fn paths_decode_utf8_once_strip_line_and_open_with_argv() {
    let root = std::env::temp_dir().join(format!("t16-paths-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("ñ file; $(ignored).txt");
    std::fs::write(&path, "private fixture").unwrap();
    assert_eq!(
        clean_local_path(&format!("{}:42", path.display())),
        Some(path.clone())
    );
    let uri = glib::filename_to_uri(&path, None).unwrap();
    assert_eq!(clean_local_path(&uri), Some(path.clone()));
    let plan = open_plan(&uri, OpenIntent::Open).unwrap();
    assert_eq!(plan.program, "xdg-open");
    assert_eq!(plan.args, vec![path.as_os_str().to_owned()]);
    for bad in [
        "javascript:alert(1)",
        "data:text/html,x",
        "sh -c touch",
        "file://evil.invalid/etc/passwd",
        "file:///tmp/a%00b",
    ] {
        assert!(open_plan(bad, OpenIntent::Open).is_err());
    }
    let plan = open_plan("https://example.invalid/a%20b", OpenIntent::Open).unwrap();
    assert_eq!(plan.args, vec!["https://example.invalid/a%20b"]);
}

#[test]
fn snippets_styles_match_actual_original_ast_for_every_desktop_theme() {
    let catalog: serde_json::Value =
        serde_json::from_str(include_str!("../src/ui/desktop-themes.json")).unwrap();
    let themes =
        comandos_app::theme::themes_from_file(Some(include_bytes!("../../../config/themes.json")));
    for name in catalog.as_object().unwrap().keys() {
        let theme = comandos_app::theme::desktop_theme(name, &themes).unwrap();
        let css = comandos_app::theme::header_css(&theme);
        let expected = oracle::original(serde_json::json!({"op":"snipcss","theme":theme.values}));
        assert!(
            css.contains(expected.as_str().unwrap()),
            "missing original snippet styling: {name}"
        );
    }
}
#[test]
fn reply_selection_skips_empty_match_and_uses_original_detail_or_last() {
    let items = serde_json::json!([{"session":"other","detail":"wrong"},{"session":"fixture","detail":"","last":""},{"session":"fixture","detail":"ñ😀\n","last":"old","project":"private"}]);
    let expected =
        oracle::original(serde_json::json!({"op":"reply","items":items,"session":"fixture"}));
    let (text, project) = comandos_app::ui::menu::reply_from_state(&items, "fixture")
        .expect("empty match must not hide later response");
    assert_eq!(serde_json::json!(["clipboard", text]), expected[0]);
    assert_eq!(project, "private");
}
#[test]
fn opener_and_reveal_use_only_argv_and_cancel_before_fallback() {
    use comandos_app::{
        proc::{ProcError, ProcOutput},
        ui::menu::launch,
    };
    use std::cell::RefCell;
    let root = std::env::temp_dir().join(format!("t16-open-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("ñ'; $(ignored).txt");
    std::fs::write(&path, "fixture").unwrap();
    let plan = open_plan(path.to_str().unwrap(), OpenIntent::Reveal).unwrap();
    let calls = RefCell::new(vec![]);
    launch(
        &plan,
        || true,
        &|spec| {
            assert_eq!(spec.program, "gdbus");
            assert_eq!(spec.timeout, std::time::Duration::from_secs(4));
            calls.borrow_mut().push(spec.args.clone());
            Ok(ProcOutput {
                code: Some(0),
                ..Default::default()
            })
        },
        &|_, _| panic!("successful reveal must not open file"),
    )
    .unwrap();
    assert!(
        calls.borrow()[0]
            .iter()
            .any(|a| a.to_string_lossy().contains("ShowItems"))
    );
    let detached = RefCell::new(vec![]);
    launch(
        &plan,
        || true,
        &|_| Err(ProcError::Spawn("fake unavailable".into())),
        &|p, args| {
            detached.borrow_mut().push((p.to_string(), args.to_vec()));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(detached.borrow()[0].0, "xdg-open");
    assert_eq!(detached.borrow()[0].1, vec![root.as_os_str().to_owned()]);
    assert!(
        launch(
            &plan,
            || false,
            &|_| panic!("cancelled gdbus"),
            &|_, _| panic!("cancelled detached")
        )
        .is_err()
    );
    let raw = format!("{}:19", path.display());
    assert_eq!(
        serde_json::json!(clean_local_path(&raw).unwrap().display().to_string()),
        oracle::original(serde_json::json!({"op":"path","url":raw}))
    );
}

#[test]
fn link_popover_actions_and_notification_payload_match_original_ast() {
    use comandos_app::ui::menu::{link_rows, notification_payload};
    let root = std::env::temp_dir().join(format!("t16-link-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("file.txt");
    std::fs::write(&path, "fixture").unwrap();
    for (url, local) in [
        (path.display().to_string(), true),
        ("https://example.invalid/ñ".into(), false),
    ] {
        for es in [false, true] {
            let rows = link_rows(local, !es);
            let expected = oracle::original(serde_json::json!({"op":"linkrows","url":url,"es":es}));
            assert_eq!(
                serde_json::json!(rows.iter().map(|(_, label, _)| *label).collect::<Vec<_>>()),
                expected["labels"]
            );
            assert_eq!(
                serde_json::json!(
                    rows.iter()
                        .map(|(icon, _, _)| (*icon, 15))
                        .collect::<Vec<_>>()
                ),
                expected["icons"]
            );
        }
    }
    let expected =
        oracle::original(serde_json::json!({"op":"notify","title":"ñ😀","body":"private body"}));
    assert_eq!(notification_payload("ñ😀", "private body"), expected[0][1]);
    assert_eq!(expected[0][0], "http://127.0.0.1:4778/notify");
}

#[test]
fn pane_geometry_matches_original_valid_rows_and_inclusive_boundaries() {
    let listing = "%1|0|0|4|2\nmalformed\n%2|5|0|9|2\n";
    for (col, row) in [(0, 0), (4, 2), (5, 0), (9, 2), (10, 0), (1, 3)] {
        let actual = comandos_app::ui::menu::pane_at(listing, col, row);
        assert_eq!(
            serde_json::json!(actual),
            oracle::original(
                serde_json::json!({"op":"pane", "listing":listing,"col":col,"row":row})
            )
        );
    }
}
#[test]
fn tab_mount_preserves_the_instance_bound_context_menu() {
    let mount = include_str!("../src/ui/app.rs");
    assert!(
        !mount.contains("term.on_context_menu("),
        "add_tab replaces the instance-bound T16 context callback with session-key ownership"
    );
    let t16 = include_str!("../src/ui/app_t16.rs");
    assert!(t16.contains("term.on_context_menu("));
    assert!(t16.contains("a.instance(&key_, &instance)"));
}

#[test]
fn primary_click_uses_press_url_cell_and_original_strict_pixel_threshold() {
    use comandos_app::term::links::PrimaryPress;
    for (x, y, button) in [
        (14.99, 24.99, 1),
        (15., 20., 1),
        (10., 25., 1),
        (6., 16., 1),
        (10., 20., 2),
    ] {
        let press = PrimaryPress::new(
            (10., 20.),
            (2, 3),
            Some("https://example.invalid/old".into()),
        );
        let actual = press.release((x, y), button).map_or_else(Vec::new, |p| {
            assert_eq!(p.point, (2, 3));
            vec![
                serde_json::json!(["select-pane", "-t", "%1"]),
                serde_json::json!(["link", p.link.unwrap()]),
            ]
        });
        assert_eq!(
            serde_json::json!(actual),
            oracle::original(
                serde_json::json!({"op":"release","x":x,"y":y,"button":button,"url":"https://example.invalid/old"})
            )
        );
    }
    let view = include_str!("../src/term/view.rs");
    assert!(view.contains("PrimaryPress::new"));
    assert!(
        !view.contains("press_position"),
        "release must consume the frozen press record instead of recomputing URL/cell"
    );
}

#[test]
fn clicked_split_rechecks_frozen_server_session_pane_before_mutation() {
    use comandos_app::{
        config::RunMode, restore::RestoreTmux, ui::app_commands::execute_split_at_expected,
    };
    use std::cell::RefCell;
    struct Boundary {
        stamps: RefCell<std::collections::VecDeque<String>>,
        calls: RefCell<Vec<Vec<String>>>,
    }
    impl RestoreTmux for Boundary {
        type Ownership = ();
        fn create(&self, _: &[&str]) -> Result<(String, ()), String> {
            panic!("split must not create a session")
        }
        fn cleanup(&self, _: ()) -> Result<(), String> {
            panic!("split has no session ownership")
        }
        fn mode(&self) -> RunMode {
            RunMode::Sandbox
        }
        fn read(&self, args: &[&str]) -> Result<String, String> {
            Ok(match args.last().copied() {
                Some("#{session_name}") => "fixture".into(),
                Some("#{pane_current_path}") => "/private/cwd".into(),
                _ => self
                    .stamps
                    .borrow_mut()
                    .pop_front()
                    .expect("unexpected identity read"),
            })
        }
        fn mutate(&self, args: &[&str], _: Option<&[u8]>) -> Result<String, String> {
            self.calls
                .borrow_mut()
                .push(args.iter().map(|s| s.to_string()).collect());
            Ok(String::new())
        }
    }
    let expected = "fixture|111|$1|42|%7";
    for stamps in [
        vec![expected, expected],
        vec!["fixture|222|$1|42|%7"],
        vec![expected, "fixture|222|$1|42|%7"],
        vec![expected, "moved|111|$1|42|%7"],
    ] {
        let success = stamps == vec![expected, expected];
        let io = Boundary {
            stamps: RefCell::new(stamps.into_iter().map(str::to_string).collect()),
            calls: RefCell::new(vec![]),
        };
        let result = execute_split_at_expected(
            &io,
            "fixture",
            "left",
            false,
            Some("%7"),
            Some(expected),
            || false,
        );
        assert_eq!(result.is_ok(), success);
        if success {
            assert_eq!(
                *io.calls.borrow(),
                vec![vec![
                    "split-window",
                    "-h",
                    "-b",
                    "-t",
                    "%7",
                    "-c",
                    "/private/cwd"
                ]]
            );
        } else {
            assert!(io.calls.borrow().is_empty());
        }
    }
}

#[test]
fn primary_capture_is_readonly_and_release_selects_only_original_pane_identity() {
    use comandos_app::{
        config::RunMode,
        tmux::{TmuxError, TmuxOut},
        ui::{
            clipboard::TmuxIo,
            menu::{capture_click, select_captured_click},
        },
    };
    use std::{
        cell::{Cell, RefCell},
        path::Path,
    };
    struct Io {
        identity: Cell<u32>,
        calls: RefCell<Vec<Vec<String>>>,
    }
    impl TmuxIo for Io {
        fn mode(&self) -> RunMode {
            RunMode::Sandbox
        }
        fn socket(&self) -> &Path {
            Path::new("/tmp/gtk-t16-private-20261006/fake-tmux/click-S")
        }
        fn read(&self, args: &[&str]) -> Result<TmuxOut, TmuxError> {
            let stdout = match args.first().copied() {
                Some("list-clients") => "/private/tty|fixture".into(),
                Some("list-panes") => "%1|0|0|9|9".into(),
                _ => format!("fixture|{}|$1|42|%1", self.identity.get()),
            };
            Ok(TmuxOut {
                code: 0,
                stdout,
                stderr: String::new(),
            })
        }
        fn mutate(&self, args: &[&str], _: Option<&[u8]>) -> Result<TmuxOut, TmuxError> {
            self.calls
                .borrow_mut()
                .push(args.iter().map(|s| s.to_string()).collect());
            Ok(TmuxOut {
                code: 0,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }
    let io = Io {
        identity: Cell::new(111),
        calls: RefCell::new(vec![]),
    };
    let context = capture_click(&io, Some("/private/tty"), (3, 3), || false).unwrap();
    assert!(
        io.calls.borrow().is_empty(),
        "press must never select early or copy buffers"
    );
    select_captured_click(&io, &context, Some("/private/tty"), || false).unwrap();
    assert_eq!(*io.calls.borrow(), vec![vec!["select-pane", "-t", "%1"]]);
    io.calls.borrow_mut().clear();
    io.identity.set(222);
    assert!(select_captured_click(&io, &context, Some("/private/tty"), || false).is_err());
    assert!(io.calls.borrow().is_empty());
    io.identity.set(111);
    assert!(select_captured_click(&io, &context, Some("/private/tty"), || true).is_err());
    assert!(io.calls.borrow().is_empty());
}
