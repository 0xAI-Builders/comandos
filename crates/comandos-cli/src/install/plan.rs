//! Reviewable installation actions. Existing tmux state is never restarted.
use super::{assets, hooks_register, platform::Platform};
use comandos_store::files::{FileLock, write_atomic};
use serde_json::json;
use std::{
    fs, io,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
};

/// Supported reversible service state. Masked/static/transitional states require
/// an explicit installer policy and are rejected before retirement.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UnitState {
    pub enabled: bool,
    pub runtime: bool,
    pub active: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Mkdir(PathBuf, u32),
    Link {
        name: String,
        at: PathBuf,
        target: PathBuf,
    },
    WriteIfAbsent {
        path: PathBuf,
        bytes: &'static [u8],
        mode: u32,
    },
    WriteUnit {
        path: PathBuf,
        bytes: Vec<u8>,
        original: &'static [u8],
    },
    Write {
        path: PathBuf,
        bytes: Vec<u8>,
        mode: u32,
    },
    RegisterClaudeHooks(PathBuf),
    AgentsSetup(PathBuf),
    InstallFonts(PathBuf),
    ExtensionOperation {
        home: PathBuf,
        journal: PathBuf,
        operation: String,
    },
    Systemctl {
        home: PathBuf,
        args: Vec<String>,
    },
    SystemctlState {
        home: PathBuf,
        unit: String,
        require_loaded: bool,
        response: std::rc::Rc<std::cell::RefCell<Option<UnitState>>>,
    },
    LaunchAgent(PathBuf),
}

fn unit(home: &Path, name: &str, source: &'static [u8], old: &str, new: &str) -> Action {
    Action::WriteUnit {
        path: home.join(".config/systemd/user").join(name),
        bytes: String::from_utf8_lossy(source)
            .replace(old, new)
            .into_bytes(),
        original: source,
    }
}

pub fn plan(home: &Path, platform: Platform, release: &Path) -> Vec<Action> {
    let binary = home.join(".local/share/comandos/bin/comandos");
    let mut result = Vec::new();
    for sub in [
        ".local/bin",
        ".claude/hooks/state",
        ".config/kitty",
        ".local/share/comandos/rollback",
        ".local/share/applications",
    ] {
        result.push(Action::Mkdir(home.join(sub), 0o700));
    }
    for name in crate::dispatch::alias_names().chain(std::iter::once("comandos")) {
        let at = super::link_path(home, name);
        result.push(Action::Link {
            name: name.into(),
            at,
            target: binary.clone(),
        });
    }
    // These native daemons are separate artifacts; never alias them to an
    // unsupported CLI command. Explicitly staged artifacts are the source.
    for (name, artifact) in [
        ("cc-app", "comandos-app"),
        ("cc-notifyd", "comandos-notifyd"),
        ("cc-model-proxy", "cc-model-proxy"),
    ] {
        let target = release.join(artifact);
        if target.is_file() {
            result.push(Action::Link {
                name: name.into(),
                at: home.join(".local/bin").join(name),
                target,
            });
        }
    }
    for asset in assets::CONFIG {
        result.push(Action::WriteIfAbsent {
            path: home.join(asset.path),
            bytes: asset.bytes,
            mode: asset.mode,
        });
    }
    let fonts = if platform == Platform::Darwin {
        home.join("Library/Fonts/ComandOS")
    } else {
        home.join(".local/share/fonts/comandos")
    };
    for asset in assets::FONTS {
        result.push(Action::WriteIfAbsent {
            path: fonts.join(asset.path),
            bytes: asset.bytes,
            mode: asset.mode,
        });
    }
    result.push(Action::RegisterClaudeHooks(home.into()));
    if platform == Platform::Darwin {
        result.push(Action::LaunchAgent(home.into()));
    } else {
        result.push(Action::WriteIfAbsent {
            path: home.join(".config/systemd/user/tmux.service"),
            bytes: assets::TMUX_UNIT,
            mode: 0o644,
        });
        result.push(unit(
            home,
            "cc-dash.service",
            assets::DASH_UNIT,
            "%h/.local/bin/cc-dash",
            "%h/.local/share/comandos/bin/comandos dash",
        ));
        result.push(unit(
            home,
            "cc-notifyd.service",
            assets::NOTIFY_UNIT,
            "%h/.local/bin/cc-notifyd",
            "%h/.local/bin/cc-notifyd",
        ));
        result.push(unit(
            home,
            "cc-proxy.service",
            assets::PROXY_UNIT,
            "%h/.local/bin/cc-model-proxy",
            "%h/.local/bin/cc-model-proxy",
        ));
        result.push(unit(
            home,
            "comandos-broker.service",
            assets::BROKER_UNIT,
            "%h/.local/share/comandos/bin/comandos ext broker",
            "%h/.local/share/comandos/bin/comandos ext broker",
        ));
        result.push(Action::Write {
            path: home.join(".local/share/applications/comandos.desktop"),
            bytes: assets::DESKTOP
                .replace("__HOME__", &home.to_string_lossy())
                .into_bytes(),
            mode: 0o644,
        });
        result.push(Action::Systemctl {
            home: home.into(),
            args: vec!["--user".into(), "daemon-reload".into()],
        });
    }
    result.push(Action::InstallFonts(fonts));
    result.push(Action::AgentsSetup(home.into()));
    result
}

/// Inject external operations in tests. The callback is never invoked by a preview.
pub fn apply_with(
    actions: &[Action],
    dry_run: bool,
    run: &mut dyn FnMut(&Action) -> Result<(), String>,
) -> Result<Vec<String>, String> {
    if dry_run {
        return apply_inner(actions, true, run, None);
    }
    let home = actions.iter().find_map(|a| match a {
        Action::AgentsSetup(h) | Action::RegisterClaudeHooks(h) | Action::LaunchAgent(h) => Some(h),
        Action::Systemctl { home, .. } => Some(home),
        _ => None,
    });
    let _guard = home
        .map(|home| super::transaction::installation_lock(home))
        .transpose()?;
    let mut journal = super::transaction::Journal::default();
    journal.actions(actions)?;
    if let Some(home) = home {
        journal.durable(home)?;
    }
    let result = apply_inner(actions, false, run, Some(&mut journal));
    super::transaction::finish(journal, result)
}
pub(crate) fn apply_with_journal(
    actions: &[Action],
    run: &mut dyn FnMut(&Action) -> Result<(), String>,
    journal: &mut super::transaction::Journal,
) -> Result<Vec<String>, String> {
    apply_inner(actions, false, run, Some(journal))
}
fn apply_inner(
    actions: &[Action],
    dry_run: bool,
    run: &mut dyn FnMut(&Action) -> Result<(), String>,
    mut journal: Option<&mut super::transaction::Journal>,
) -> Result<Vec<String>, String> {
    let mut report = Vec::new();
    if dry_run {
        for action in actions {
            report.push(format!("dry-run: {}", summary(action)));
        }
        return Ok(report);
    }
    let mut units_changed = false;
    let mut fonts_changed = false;
    for action in actions {
        if let Some(j) = journal.as_deref_mut() {
            j.prepare(action)?;
        }
        let error = |path: &Path, e: io::Error| format!("{}: {e}", path.display());
        match action {
            Action::Mkdir(path, mode) => {
                super::release::check_app_parents(&path.join("placeholder"))?;
                if !path.exists() {
                    fs::DirBuilder::new()
                        .recursive(true)
                        .mode(*mode)
                        .create(path)
                        .map_err(|e| error(path, e))?;
                    report.push(format!("mkdir {}", path.display()));
                }
            }
            Action::Link { name, at, target } => {
                super::release::check_app_parents(at)?;
                if fs::read_link(at).is_ok_and(|previous| previous == *target) {
                    continue;
                }
                if !target.is_file() {
                    return Err(format!("missing installed artifact: {}", target.display()));
                }
                let home = at
                    .parent()
                    .and_then(|p| p.parent())
                    .and_then(|p| p.parent())
                    .ok_or("invalid alias path")?;
                if name == "cc-app" {
                    super::link_app(home, false)?;
                } else {
                    super::link(home, target, name)?;
                }
                if let Some(j) = journal.as_deref_mut() {
                    j.checkpoint(&[
                        at.clone(),
                        super::record::path(home, name),
                        home.join(".local/share/comandos/rollback")
                            .join(format!("{name}.orig")),
                    ])?;
                }
                report.push(format!("link {} -> {}", at.display(), target.display()));
            }
            Action::WriteIfAbsent { path, bytes, mode } => {
                super::release::check_app_parents(path)?;
                match path.symlink_metadata() {
                    Ok(_) => continue,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(error(path, e)),
                }
                write_new(path, bytes, *mode)?;
                fonts_changed |= path.extension().is_some_and(|ext| ext == "ttf");
                units_changed |= path.extension().is_some_and(|ext| ext == "service");
                report.push(format!("write {}", path.display()));
            }
            Action::Write { path, bytes, mode } => {
                if write_changed(path, bytes, *mode)? {
                    report.push(format!("write {}", path.display()));
                }
            }
            Action::WriteUnit {
                path,
                bytes,
                original,
            } => {
                super::release::check_app_parents(path)?;
                match path.symlink_metadata() {
                    Ok(meta) => {
                        let prior = fs::read(path).map_err(|e| error(path, e))?;
                        if prior == *bytes && !meta.file_type().is_symlink() {
                            continue;
                        }
                        if prior != *original && prior != *bytes {
                            report.push(format!("preserved customized unit {}", path.display()));
                            continue;
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(error(path, e)),
                }
                if write_changed(path, bytes, 0o644)? {
                    units_changed = true;
                    report.push(format!("unit {}", path.display()));
                }
            }
            Action::RegisterClaudeHooks(home) => {
                let path = home.join(".claude/settings.json");
                super::release::check_app_parents(&path)?;
                let input = match fs::read(&path) {
                    Ok(bytes) => serde_json::from_slice(&bytes)
                        .map_err(|e| format!("{}: {e}", path.display()))?,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => json!({}),
                    Err(e) => return Err(error(&path, e)),
                };
                let home_text = home.to_str().ok_or("HOME must be UTF-8")?;
                let notify = home.join(".claude/hooks/cc-notify.sh");
                let tool = home.join(".claude/hooks/cc-usage-tool.sh");
                let output = hooks_register::configure(
                    &input,
                    home_text,
                    &notify.to_string_lossy(),
                    &tool.to_string_lossy(),
                )?;
                if output != input || !path.exists() {
                    let bytes = format!(
                        "{}\n",
                        serde_json::to_string_pretty(&output).map_err(|e| e.to_string())?
                    );
                    if let Some(j) = journal.as_deref_mut() {
                        j.expect_file(&path, bytes.as_bytes(), 0o600)?;
                    }
                    write_changed(&path, bytes.as_bytes(), 0o600)?;
                    report.push(format!("registered Claude hooks {}", path.display()));
                }
            }
            Action::Systemctl { .. } if !units_changed => {}
            Action::InstallFonts(_) if !fonts_changed => {}
            external => run(external)?,
        }
    }
    if report.is_empty() {
        report.push("nada que hacer".into());
    }
    Ok(report)
}

pub fn apply(actions: &[Action], dry_run: bool) -> Result<Vec<String>, String> {
    apply_with(actions, dry_run, &mut super::full::external)
}

fn summary(action: &Action) -> String {
    match action {
        Action::Mkdir(path, mode) => format!("mkdir {} {mode:o}", path.display()),
        Action::Link { at, target, .. } => format!("link {} -> {}", at.display(), target.display()),
        Action::WriteIfAbsent { path, bytes, .. } => {
            format!("write-if-absent {} ({} bytes)", path.display(), bytes.len())
        }
        Action::Write { path, bytes, .. } | Action::WriteUnit { path, bytes, .. } => {
            format!("write {} ({} bytes)", path.display(), bytes.len())
        }
        Action::RegisterClaudeHooks(home) => format!("register Claude hooks {}", home.display()),
        Action::AgentsSetup(home) => format!("configure installed agents {}", home.display()),
        Action::InstallFonts(path) => format!("refresh font cache {}", path.display()),
        Action::Systemctl { args, .. } => format!("systemctl {}", args.join(" ")),
        Action::SystemctlState { unit, .. } => format!("query enabled/active state {unit}"),
        Action::LaunchAgent(home) => format!("prepare LaunchAgent {}", home.display()),
        Action::ExtensionOperation {
            home, operation, ..
        } => format!("extension {operation} {}", home.display()),
    }
}

fn write_new(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    let parent = path.parent().ok_or("file without parent")?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)
        .map_err(|e| e.to_string())?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    use std::io::Write;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Preserve originals before changing a managed file, including a repo symlink.
fn write_changed(path: &Path, bytes: &[u8], mode: u32) -> Result<bool, String> {
    super::release::check_app_parents(path)?;
    let current = match fs::read(path) {
        Ok(value) => Some(value),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let is_link = path
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink());
    if current.as_deref() == Some(bytes) && !is_link {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .map_err(|e| e.to_string())?;
    }
    let lock = path.with_file_name(format!(
        "{}.install.lock",
        path.file_name()
            .ok_or("file without name")?
            .to_string_lossy()
    ));
    let _lock = FileLock::exclusive(&lock).map_err(|e| e.to_string())?;
    if fs::read(path).ok() != current {
        return Err(format!("{} changed during installation", path.display()));
    }
    let backup = path.with_file_name(format!(
        "{}.pre-comandos",
        path.file_name()
            .ok_or("file without name")?
            .to_string_lossy()
    ));
    if backup.symlink_metadata().is_err() {
        if let Ok(target) = fs::read_link(path) {
            symlink(target, &backup).map_err(|e| e.to_string())?;
        } else if let Some(prior) = &current {
            write_new(
                &backup,
                prior,
                path.metadata()
                    .map_err(|e| e.to_string())?
                    .permissions()
                    .mode()
                    & 0o777,
            )?;
        }
    }
    write_atomic(path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())?;
    Ok(true)
}
