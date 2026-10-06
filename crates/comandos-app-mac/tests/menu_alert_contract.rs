// The pure outcomes compile without AppKit, while the same functions are used
// by native sheet callbacks. No GUI or native framework is initialized here.
#![cfg(not(target_os = "macos"))]
use comandos_app_mac::dialogs;
use comandos_app_mac::dialogs::{DialogOutcome, TabScope};
use comandos_app_mac::strip;
#[cfg(not(target_os = "macos"))]
#[path = "../src/ffi/alert.rs"]
mod alert;
#[cfg(not(target_os = "macos"))]
#[path = "../src/ffi/menu.rs"]
mod menu;

#[cfg(not(target_os = "macos"))]
#[test]
fn menu_and_dialog_copy_match_actual_python_ast() {
    use comandos_desktop::Lang;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = comandos_oracle::oracle_at(
        &root.join("tests/golden"),
        "mac-menu-alert",
        &serde_json::json!({
            "source_ref":"2674f36",
            "source_sha256":"79670ba87d6be456c73059dba36f9c358838ca7fb8ce8fb2eb33424f92ad53bb",
            "harness": include_str!("support_menu_alert_oracle.py"),
            "languages":["es","en"], "tab_label":"雪 '$() «tab»"
        }),
        || {
            let output = std::process::Command::new(
                std::env::var("COMANDOS_MAC_ORACLE_PYTHON").unwrap_or_else(|_| "python3".into()),
            )
            .arg(root.join("tests/support_menu_alert_oracle.py"))
            .arg(root.join("tests/oracle-src/cc-app-mac"))
            .env("PYTHONIOENCODING", "utf-8")
            .output()
            .map_err(|e| e.to_string())?;
            if output.status.success() {
                Ok(output.stdout)
            } else {
                Err(String::from_utf8_lossy(&output.stderr).into_owned())
            }
        },
    );
    let oracle: serde_json::Value = serde_json::from_slice(&output).unwrap();
    for (key, lang) in [("es", Lang::Es), ("en", Lang::En)] {
        let groups = strip::menu_spec(lang).into_iter().map(|g| serde_json::json!({
            "title": g.title,
            "items": g.items.into_iter().map(|i| serde_json::json!({"title":i.title,"action":menu::selector_name(i.action).to_str().unwrap(),"key":i.key,"command":i.command,"shift":i.shift})).collect::<Vec<_>>()
        })).collect::<Vec<_>>();
        assert_eq!(serde_json::json!(groups), oracle[key]["menu"]);
        let context = menu::context_spec(lang).map(|(title, action)| {
            serde_json::json!({"title": title, "action": action.to_str().unwrap(), "key": "", "represented": "tab"})
        });
        assert_eq!(serde_json::json!(context), oracle[key]["context"]);
        let text = dialogs::close_text(lang, "雪 '$() «tab»");
        assert_eq!(
            serde_json::json!([text.message, text.informative, text.accept, text.cancel]),
            oracle[key]["close"]
        );
        let text = dialogs::rename_text(lang);
        assert_eq!(
            serde_json::json!([text.0, text.1, text.2]),
            oracle[key]["rename"]
        );
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn accepted_close_retains_captured_tab_instead_of_current_selection() {
    let scope = TabScope {
        key: "local-$()'雪".into(),
        instance: 91,
    };
    assert_eq!(
        alert::close_outcome(&scope, true),
        DialogOutcome::Close {
            scope: scope.clone(),
            confirmed: true
        }
    );
    assert_eq!(
        alert::close_outcome(&scope, false),
        DialogOutcome::Close {
            scope,
            confirmed: false
        }
    );
}

#[cfg(not(target_os = "macos"))]
#[test]
fn rename_accepts_python_whitespace_and_cancel_discards_input() {
    let scope = TabScope {
        key: "tab".into(),
        instance: 17,
    };
    assert_eq!(
        alert::rename_outcome(&scope, true, "\u{1c} \u{a0}雪 ' $()\u{85}"),
        DialogOutcome::Rename {
            scope: scope.clone(),
            label: Some("雪 ' $()".into())
        }
    );
    for (accepted, text) in [(true, "\u{1c}\u{a0}\u{85}"), (false, "new label")] {
        assert_eq!(
            alert::rename_outcome(&scope, accepted, text),
            DialogOutcome::Rename {
                scope: scope.clone(),
                label: None
            }
        );
    }
}
