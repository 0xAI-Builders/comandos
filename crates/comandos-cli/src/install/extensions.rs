//! Native extension synchronization keeps the existing user timer contract.
use super::{
    plan::{self, Action},
    platform::Platform,
};
use std::path::Path;
const OLD_SERVICE: &[u8] = b"[Unit]\nDescription=Synchronize shared MCPs and portable skills\n\n[Service]\nType=oneshot\nExecStart=%h/.local/bin/cc-extensions sync\nTimeoutStartSec=90\nUMask=0077\n";
const SERVICE: &[u8] = b"[Unit]\nDescription=Synchronize shared MCPs and portable skills\n\n[Service]\nType=oneshot\nExecStart=%h/.local/share/comandos/bin/comandos ext sync\nTimeoutStartSec=90\nUMask=0077\n";
const TIMER: &[u8] = b"[Unit]\nDescription=Propagate extension changes to agent CLIs\n\n[Timer]\nOnBootSec=45s\nOnUnitInactiveSec=60s\nAccuracySec=5s\nRandomizedDelaySec=5s\nUnit=comandos-extensions-sync.service\n\n[Install]\nWantedBy=timers.target\n";
pub fn actions(home: &Path, platform: Platform) -> Vec<Action> {
    if platform == Platform::Darwin {
        return vec![];
    }
    let units = home.join(".config/systemd/user");
    vec![
        Action::WriteUnit {
            path: units.join("comandos-extensions-sync.service"),
            bytes: SERVICE.to_vec(),
            original: OLD_SERVICE,
        },
        Action::WriteUnit {
            path: units.join("comandos-extensions-sync.timer"),
            bytes: TIMER.to_vec(),
            original: TIMER,
        },
        Action::Systemctl {
            home: home.into(),
            args: vec!["--user".into(), "daemon-reload".into()],
        },
    ]
}
pub fn apply(home: &Path, platform: Platform, dry: bool) -> Result<(), String> {
    let actions = actions(home, platform);
    // A customized extension unit must never be enabled as if it were ours.
    for action in &actions {
        if let Action::WriteUnit {
            path,
            bytes,
            original,
        } = action
        {
            match std::fs::read(path) {
                Ok(raw) if raw != *bytes && raw != *original => {
                    return Err(format!(
                        "custom extension unit preserved: {}",
                        path.display()
                    ));
                }
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
                _ => {}
            }
        }
    }
    for line in plan::apply(&actions, dry)? {
        println!("{line}");
    }
    if platform != Platform::Darwin {
        if dry {
            println!("dry-run: enable native extension synchronization timer");
        } else {
            super::full::external(&Action::Systemctl {
                home: home.into(),
                args: vec![
                    "--user".into(),
                    "enable".into(),
                    "--now".into(),
                    "comandos-extensions-sync.timer".into(),
                ],
            })?;
        }
    }
    Ok(())
}

/// Nested extension units share the full install preimages and runtime runner.
/// Timer activation is refused until its own durable prior-state adapter exists.
pub(crate) fn apply_with_journal(
    home: &Path,
    platform: Platform,
    journal: &mut super::transaction::Journal,
    run: &mut dyn FnMut(&Action) -> Result<(), String>,
) -> Result<(), String> {
    let actions = actions(home, platform);
    for action in &actions {
        if let Action::WriteUnit {
            path,
            bytes,
            original,
        } = action
            && let Ok(raw) = std::fs::read(path)
            && raw != *bytes
            && raw != *original
        {
            return Err(format!(
                "custom extension unit preserved: {}",
                path.display()
            ));
        }
    }
    for line in plan::apply_with_journal(&actions, run, journal)? {
        println!("{line}");
    }
    if platform != Platform::Darwin {
        return Err("extension timer activation capability unavailable; transaction will restore admitted files/documents".into());
    }
    Ok(())
}
