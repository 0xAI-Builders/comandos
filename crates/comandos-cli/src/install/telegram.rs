//! Explicit legacy Telegram retirement preserves credentials and archives links.
use super::plan::Action;
use std::{
    fs,
    path::{Path, PathBuf},
};
fn owned(link: &Path, suffix: &str) -> bool {
    let Ok(target) = fs::read_link(link) else {
        return false;
    };
    let target = if target.is_absolute() {
        target
    } else {
        link.parent().unwrap_or(Path::new("/")).join(target)
    };
    if !target.ends_with(suffix) {
        return false;
    }
    let Some(root) = target
        .ancestors()
        .nth(Path::new(suffix).components().count())
    else {
        return false;
    };
    root.join("bin/cc-dash").is_file() && root.join("lib/platform.sh").is_file()
}
pub fn paths(home: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let unit = home.join(".config/systemd/user/cc-telegram.service");
    if owned(&unit, "systemd/cc-telegram.service") {
        result.push(PathBuf::from(".config/systemd/user/cc-telegram.service"));
        let wants = home.join(".config/systemd/user/default.target.wants/cc-telegram.service");
        if fs::read_link(wants)
            .is_ok_and(|target| target == unit || target == Path::new("../cc-telegram.service"))
        {
            result.push(PathBuf::from(
                ".config/systemd/user/default.target.wants/cc-telegram.service",
            ));
        }
    }
    for (relative, suffix) in [
        (".local/bin/cc-telegram", "bin/cc-telegram"),
        (".claude/hooks/md2tg.py", "hooks/md2tg.py"),
    ] {
        if owned(&home.join(relative), suffix) {
            result.push(PathBuf::from(relative));
        }
    }
    result
}
pub fn apply(home: &Path, dry: bool) -> Result<(), String> {
    apply_with(home, dry, None, &mut super::full::external)
}

pub(crate) fn apply_with(
    home: &Path,
    dry: bool,
    mut journal: Option<&mut super::transaction::Journal>,
    run: &mut dyn FnMut(&Action) -> Result<(), String>,
) -> Result<(), String> {
    let paths = paths(home);
    if let Some(j) = journal.as_deref_mut() {
        for path in &paths {
            j.capture(&home.join(path))?;
        }
    }
    if paths
        .iter()
        .any(|p| p == Path::new(".config/systemd/user/cc-telegram.service"))
    {
        if dry {
            println!(
                "dry-run: disable owned cc-telegram.service; archive its links; preserve credentials"
            );
        } else {
            if let Some(j) = journal.as_deref_mut() {
                j.prepare_unit_retirement(home, "cc-telegram.service", run)?;
            }
            run(&Action::Systemctl {
                home: home.into(),
                args: vec![
                    "--user".into(),
                    "disable".into(),
                    "--now".into(),
                    "cc-telegram.service".into(),
                ],
            })?;
        }
    }
    if let Some(manifest) = super::cleanup::stage_with_journal(
        home,
        &paths,
        dry,
        &mut || Ok(()),
        journal.as_deref_mut(),
    )? {
        println!(
            "Telegram links archived; restore with {}",
            manifest.display()
        );
    }
    if !dry
        && paths
            .iter()
            .any(|p| p.extension().is_some_and(|e| e == "service"))
    {
        let reload = Action::Systemctl {
            home: home.into(),
            args: vec!["--user".into(), "daemon-reload".into()],
        };
        if let Some(j) = journal {
            j.prepare_recompute(&reload)?;
        }
        run(&reload)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::transaction::Journal;
    use std::os::unix::fs::symlink;
    #[test]
    fn injected_reload_failure_restores_archive_without_real_service_commands() {
        let mut id = [0u8; 8];
        getrandom::fill(&mut id).unwrap();
        let home =
            std::env::temp_dir().join(format!("telegram-journal-{:x}", u64::from_ne_bytes(id)));
        let legacy = home.join("legacy");
        for relative in [
            "bin/cc-dash",
            "lib/platform.sh",
            "systemd/cc-telegram.service",
        ] {
            let path = legacy.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"fixture").unwrap();
        }
        let unit = home.join(".config/systemd/user/cc-telegram.service");
        let wants = home.join(".config/systemd/user/default.target.wants/cc-telegram.service");
        fs::create_dir_all(wants.parent().unwrap()).unwrap();
        symlink(legacy.join("systemd/cc-telegram.service"), &unit).unwrap();
        symlink("../cc-telegram.service", &wants).unwrap();
        let secret = home.join(".config/telegram.credentials");
        fs::write(&secret, b"preserve").unwrap();
        let mut journal = Journal::default();
        journal.durable(&home).unwrap();
        let mut calls = Vec::new();
        let mut state = super::super::plan::UnitState {
            enabled: true,
            runtime: false,
            active: true,
        };
        let mut reload_failed = false;
        let mut run = |action: &Action| match action {
            Action::SystemctlState { response, .. } => {
                *response.borrow_mut() = Some(state.clone());
                Ok(())
            }
            Action::Systemctl { args, .. } => {
                calls.push(args.clone());
                if args.iter().any(|arg| arg == "disable") {
                    // Verify the prior state is durable before this mutation.
                    let journals = home.join(".local/share/comandos/install-journals");
                    let entry = fs::read_dir(journals)
                        .unwrap()
                        .next()
                        .unwrap()
                        .unwrap()
                        .path();
                    let manifest: serde_json::Value =
                        serde_json::from_slice(&fs::read(entry.join("manifest.json")).unwrap())
                            .unwrap();
                    assert_eq!(manifest["runtime"][0]["before"]["enabled"], true);
                    assert_eq!(manifest["runtime"][0]["before"]["active"], true);
                    fs::remove_file(&wants).unwrap();
                    state.enabled = false;
                    state.active = false;
                    Ok(())
                } else if args.iter().any(|arg| arg == "daemon-reload") {
                    if !reload_failed {
                        reload_failed = true;
                        Err("private runner reload failure".into())
                    } else {
                        assert_eq!(
                            fs::read_link(&unit).unwrap(),
                            legacy.join("systemd/cc-telegram.service")
                        );
                        Ok(())
                    }
                } else if args.iter().any(|arg| arg == "enable") {
                    state.enabled = true;
                    Ok(())
                } else if args.iter().any(|arg| arg == "start") {
                    state.active = true;
                    Ok(())
                } else {
                    panic!("unexpected action: {args:?}");
                }
            }
            _ => panic!("unexpected action"),
        };
        let result = apply_with(&home, false, Some(&mut journal), &mut run);
        let error = crate::install::transaction::finish_with_runtime(journal, result, &mut run)
            .unwrap_err();
        assert_eq!(error, "private runner reload failure");
        assert!(state.enabled && state.active);
        assert_eq!(calls.len(), 5);
        assert_eq!(
            calls[0],
            ["--user", "disable", "--now", "cc-telegram.service"]
        );
        assert_eq!(calls[2], ["--user", "daemon-reload"]);
        assert_eq!(calls[3], ["--user", "enable", "cc-telegram.service"]);
        assert_eq!(calls[4], ["--user", "start", "cc-telegram.service"]);
        assert_eq!(
            fs::read_link(&unit).unwrap(),
            legacy.join("systemd/cc-telegram.service")
        );
        assert_eq!(
            fs::read_link(&wants).unwrap(),
            Path::new("../cc-telegram.service")
        );
        assert_eq!(fs::read(secret).unwrap(), b"preserve");
        fs::remove_dir_all(home).unwrap();
    }
}
