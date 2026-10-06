//! Complete install entry point; all platform commands use bounded owned groups.
use super::{
    plan::{self, Action},
    platform::Platform,
    release,
};
use comandos_desktop::proc::{ProcOutput, ProcSpec, run as process_run};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub fn handles(args: &[String]) -> bool {
    !args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--stage"
                | "--stage-app"
                | "--link"
                | "--rollback"
                | "--rollback-release"
                | "--releases"
                | "--darwin-agent"
                | "--app"
        )
    })
}

pub fn run(args: &[String]) -> Result<i32, String> {
    let mut home = std::env::var_os("HOME").map(PathBuf::from);
    let mut source = None;
    let mut dry = false;
    let mut extensions = false;
    let mut retarget = None;
    let mut restore = None;
    let mut cleanup = false;
    let mut cleanup_repo = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--home" => {
                let Some(value) = iter.next() else {
                    return Ok(2);
                };
                home = Some(value.into());
            }
            "--release" => {
                let Some(value) = iter.next() else {
                    return Ok(2);
                };
                if source.replace(PathBuf::from(value)).is_some() {
                    return Ok(2);
                }
            }
            "--dry-run" if !dry => dry = true,
            "--extensions" if !extensions => extensions = true,
            "--cleanup-legacy" if !cleanup => cleanup = true,
            "--cleanup-repo" => {
                let Some(value) = iter.next() else {
                    return Ok(2);
                };
                if cleanup_repo.replace(PathBuf::from(value)).is_some() {
                    return Ok(2);
                }
            }
            "--restore-legacy" => {
                let Some(value) = iter.next() else {
                    return Ok(2);
                };
                if restore.replace(PathBuf::from(value)).is_some() {
                    return Ok(2);
                }
            }
            "--retarget-repo" => {
                let Some(value) = iter.next() else {
                    return Ok(2);
                };
                if retarget.replace(PathBuf::from(value)).is_some() {
                    return Ok(2);
                }
            }
            _ => return Ok(2),
        }
    }
    let Some(home) = home else { return Ok(2) };
    release::check_app_parents(&home.join("placeholder"))?;
    if !home.is_dir() {
        return Err(format!(
            "HOME must be an existing directory: {}",
            home.display()
        ));
    }
    if cleanup {
        if source.is_some() || extensions || retarget.is_some() || restore.is_some() {
            return Ok(2);
        }
        let Some(repo) = cleanup_repo else {
            return Ok(2);
        };
        return super::retirement::run(&home, &repo, dry);
    }
    if cleanup_repo.is_some() {
        return Ok(2);
    }
    if let Some(backup) = restore {
        if source.is_some() || extensions || retarget.is_some() {
            return Ok(2);
        }
        let manifest = if backup.is_absolute() {
            backup
        } else {
            if backup.components().count() != 1
                || !matches!(
                    backup.components().next(),
                    Some(std::path::Component::Normal(_))
                )
            {
                return Ok(2);
            }
            home.join(".local/share/comandos/backups")
                .join(backup)
                .join("manifest.json")
        };
        for line in super::cleanup::restore(&home, &manifest, dry)? {
            println!("{line}");
        }
        return Ok(0);
    }
    let platform = Platform::current();
    if platform == Platform::WslUbuntu && !super::wsl::prepare(&home, dry)? {
        return Ok(0);
    }
    let explicit_source = source.is_some();
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let source = source.unwrap_or_else(|| exe.parent().unwrap_or(Path::new("/")).to_path_buf());
    if !source.is_absolute() {
        return Ok(2);
    }
    let source_binary = source.join("comandos");
    if explicit_source && !source_binary.is_file() {
        return Err(format!(
            "release is missing a native executable: {}",
            source_binary.display()
        ));
    }
    let candidate = if source_binary.is_file() {
        source_binary
    } else {
        exe
    };
    let web = if source.join("web/manifest.json").is_file() {
        release::WebSource::Explicit(source.join("web"))
    } else {
        release::WebSource::None
    };
    let staged = if dry {
        release::preview_release(&home, &candidate, &web)?
    } else {
        release::stage_release(&home, &candidate, &web)?
    };
    let proxy = super::proxy::prepare(&home, &source, dry)?;
    let mut actions = plan::plan(&home, platform, &source);
    if let Some(target) = proxy {
        actions.retain(
            |action| !matches!(action, Action::Link { name, .. } if name == "cc-model-proxy"),
        );
        actions.insert(
            0,
            Action::Link {
                name: "cc-model-proxy".into(),
                at: home.join(".local/bin/cc-model-proxy"),
                target,
            },
        );
    }
    // Freeze separate payloads before switching any public alias. Removing the
    // input release or source checkout cannot invalidate an installed daemon.
    for action in &mut actions {
        if let Action::Link { name, target, .. } = action {
            let artifact = match name.as_str() {
                "cc-notifyd" => Some("comandos-notifyd"),
                "cc-model-proxy" => Some("cc-model-proxy"),
                _ => None,
            };
            if let Some(artifact) = artifact {
                *target = super::components::stage(&home, artifact, target, dry)?;
            }
        }
    }
    // Stage and verify the GTK artifact using its existing reversible installer.
    let mut app_source = source.join(if platform == Platform::Darwin {
        "comandos-app-mac"
    } else {
        "comandos-app"
    });
    if platform == Platform::Darwin && !app_source.is_file() {
        app_source = source.join("ComandOS.app/Contents/MacOS/comandos-app-mac");
    }
    if app_source.is_file() {
        let app = if dry {
            release::preview_app(&home, &app_source)?
        } else {
            release::stage_app(&home, &app_source)?
        };
        if !actions
            .iter()
            .any(|action| matches!(action, Action::Link { name, .. } if name == "cc-app"))
        {
            actions.push(Action::Link {
                name: "cc-app".into(),
                at: home.join(".local/bin/cc-app"),
                target: app.path.clone(),
            });
        }
        for action in &mut actions {
            if let Action::Link { name, target, .. } = action
                && name == "cc-app"
            {
                *target = app.path.clone();
            }
        }
    }
    if platform == Platform::Darwin && source.join("ComandOS.app").is_dir() {
        super::darwin::install_app(&home, &source.join("ComandOS.app"), dry)?;
    }
    println!(
        "{}release {}: {}",
        if dry { "dry-run: " } else { "" },
        staged.id,
        staged.path.display()
    );
    for line in plan::apply(&actions, dry)? {
        println!("{line}");
    }
    if let Some(repo) = retarget {
        for line in super::retarget::apply(&home, &repo, dry)? {
            println!("{line}");
        }
    }
    if extensions {
        if dry {
            println!("dry-run: extensions import/sync");
        } else {
            let catalog = home.join(".config/comandos/extensions/catalog.json");
            if !catalog.exists() {
                command(
                    &home,
                    &staged.path,
                    ["ext", "import"].into_iter().map(String::from).collect(),
                    Duration::from_secs(90),
                )?;
            }
            command(
                &home,
                &staged.path,
                ["ext", "sync"].into_iter().map(String::from).collect(),
                Duration::from_secs(90),
            )?;
        }
        super::extensions::apply(&home, platform, dry)?;
    }
    if std::env::var("COMANDOS_RETIRE_TELEGRAM").as_deref() == Ok("1") {
        super::telegram::apply(&home, dry)?;
    }
    Ok(0)
}

pub(super) fn command(
    home: &Path,
    program: &Path,
    args: Vec<String>,
    timeout: Duration,
) -> Result<(), String> {
    let output = capture(home, program, args, timeout)?;
    if output.timed_out || output.code != Some(0) {
        return Err(format!(
            "{} failed (exit {:?}, timed out {}): {}",
            program.display(),
            output.code,
            output.timed_out,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

pub(super) fn capture(
    home: &Path,
    program: &Path,
    args: Vec<String>,
    timeout: Duration,
) -> Result<ProcOutput, String> {
    let output = process_run(&ProcSpec {
        program: program.to_str().ok_or("program path must be UTF-8")?.into(),
        args: args.into_iter().map(Into::into).collect(),
        stdin: None,
        env: vec![("HOME".into(), home.as_os_str().into())],
        clear_env: false,
        env_remove: vec![],
        cwd: Some(home.into()),
        timeout,
    })
    .map_err(|e| format!("{e:?}"))?;
    if output.timed_out {
        return Err(format!(
            "{} failed (exit {:?}, timed out {}): {}",
            program.display(),
            output.code,
            output.timed_out,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output)
}

pub(super) fn external(action: &Action) -> Result<(), String> {
    match action {
        Action::AgentsSetup(home) => command(
            home,
            &home.join(".local/share/comandos/bin/comandos"),
            vec!["agents".into(), "setup".into()],
            Duration::from_secs(30),
        ),
        Action::InstallFonts(path) => {
            // Font cache is optional on macOS and minimal Linux installations.
            let home = path
                .ancestors()
                .find(|p| p.join(".local/share/comandos/bin/comandos").is_file())
                .ok_or("font path outside installation")?;
            let Some(program) = find("fc-cache") else {
                return Ok(());
            };
            command(
                home,
                &program,
                vec!["-f".into(), path.to_string_lossy().into_owned()],
                Duration::from_secs(30),
            )
        }
        Action::Systemctl { home, args } => {
            let program =
                find("systemctl").ok_or("systemctl unavailable; install units manually")?;
            command(home, &program, args.clone(), Duration::from_secs(15))
        }
        // Preparing the agent never unloads a current dashboard.
        Action::LaunchAgent(home) => super::darwin::agent(home, false, true),
        _ => Err("not an external install action".into()),
    }
}

pub(super) fn find(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|p| p.join(name))
            .find(|p| {
                p.metadata()
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
    })
}
