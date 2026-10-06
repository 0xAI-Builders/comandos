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
    let paths = paths(home);
    if paths
        .iter()
        .any(|p| p == Path::new(".config/systemd/user/cc-telegram.service"))
    {
        if dry {
            println!(
                "dry-run: disable owned cc-telegram.service; archive its links; preserve credentials"
            );
        } else {
            super::full::external(&Action::Systemctl {
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
    if let Some(manifest) = super::cleanup::stage(home, &paths, dry)? {
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
        super::full::external(&Action::Systemctl {
            home: home.into(),
            args: vec!["--user".into(), "daemon-reload".into()],
        })?;
    }
    Ok(())
}
