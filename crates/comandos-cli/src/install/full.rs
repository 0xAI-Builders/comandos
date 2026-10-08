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
                | "--extensions-bin"
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
    run_with(args, &mut external)
}

/// Inject platform commands for private full-install regression tests.
pub fn run_with(
    args: &[String],
    run_action: &mut dyn FnMut(&Action) -> Result<(), String>,
) -> Result<i32, String> {
    let mut home = std::env::var_os("HOME").map(PathBuf::from);
    let mut source = None;
    let mut dry = false;
    let mut extensions = false;
    let mut retarget = None;
    let mut restore = None;
    let mut recovery = None;
    let mut cleanup = false;
    let mut cleanup_repo = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--recover-install" => {
                let Some(value) = iter.next() else {
                    return Ok(2);
                };
                if recovery.replace(PathBuf::from(value)).is_some() {
                    return Ok(2);
                }
            }
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
    if recovery.is_some()
        && (restore.is_some()
            || cleanup
            || cleanup_repo.is_some()
            || source.is_some()
            || extensions
            || retarget.is_some()
            || dry)
    {
        return Ok(2);
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
    if let Some(path) = recovery {
        if dry || source.is_some() || extensions || retarget.is_some() || cleanup {
            return Ok(2);
        }
        super::transaction::recover_with(&home, &path, run_action)?;
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
    // Serialize full installers; component/standalone command locks still retain
    // their existing contracts. This lock never exists during a dry preview.
    let _full_lock = if dry {
        None
    } else {
        Some(super::transaction::installation_lock(&home)?)
    };
    // Immutable payloads survive a failed install. Prepare them before the
    // transaction captures parent directories; aliases remain untouched here.
    let extension_source = source.join("comandos-extensions");
    let extension_target = match extension_source.symlink_metadata() {
        Ok(_) => Some(super::components::stage(
            &home,
            "comandos-extensions",
            &extension_source,
            dry,
        )?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            super::components::installed_alias(&home, "comandos-extensions", "cc-extensions")?
        }
        Err(e) => return Err(e.to_string()),
    };
    let mut initial_actions = plan::plan(&home, platform, &source);
    if extensions {
        initial_actions.extend(super::extensions::actions(&home, platform));
    }
    let mut journal = if dry {
        None
    } else {
        Some(super::transaction::Journal::full(
            &home,
            &source,
            &initial_actions,
        )?)
    };
    if let Some(j) = journal.as_mut() {
        j.durable(&home)?;
    }
    let mut external_effects = Vec::new();
    let mut tracked_action = |action: &Action| {
        match action {
            Action::Systemctl { args, .. } => {
                if args != &["--user", "disable", "--now", "cc-telegram.service"]
                    && args
                        != &[
                            "--user",
                            "enable",
                            "--now",
                            "comandos-extensions-sync.timer",
                        ]
                    && args != &["--user", "daemon-reload"]
                {
                    external_effects.push(format!("systemctl {}", args.join(" ")))
                }
            }
            Action::LaunchAgent(_) => external_effects.push("LaunchAgent preparation".into()),
            Action::InstallFonts(_) => {}
            _ => {}
        }
        run_action(action)
    };
    let result = (|| {
        if let Some(j) = journal.as_mut() {
            let expected = release::preview_release(&home, &candidate, &web)?;
            j.prepare_release(&home, &expected.id)?;
        }
        let staged = if dry {
            release::preview_release(&home, &candidate, &web)?
        } else {
            release::stage_release_unpruned(&home, &candidate, &web)?
        };
        if let Some(j) = journal.as_mut() {
            j.checkpoint(&[
                home.join(".local/share/comandos/bin/comandos"),
                home.join(".local/share/comandos/releases/previous"),
            ])?;
        }
        let proxy = super::proxy::prepare(&home, &source, dry)?;
        if let Some(j) = journal.as_mut() {
            j.produced_file(
                &home.join(".local/share/comandos/build/claude-codex/release/claude-codex"),
            )?;
        }
        let mut actions = plan::plan(&home, platform, &source);
        if let Some(extension_target) = extension_target {
            for action in &mut actions {
                if let Action::Link { name, target, .. } = action
                    && name == "cc-extensions"
                {
                    *target = extension_target.clone();
                }
            }
        }
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
            if let Some(j) = journal.as_mut() {
                let expected = release::preview_app(&home, &app_source)?;
                j.expect_link(
                    &home.join(".local/share/comandos/bin/comandos-app"),
                    &Path::new("../releases")
                        .join(expected.id)
                        .join("comandos-app"),
                )?;
            }
            let app = if dry {
                release::preview_app(&home, &app_source)?
            } else {
                release::stage_app_without_install_lock(&home, &app_source)?
            };
            if let Some(j) = journal.as_mut() {
                j.checkpoint(&[home.join(".local/share/comandos/bin/comandos-app")])?;
            }
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
        let report = if let Some(j) = journal.as_mut() {
            plan::apply_with_journal(&actions, &mut tracked_action, j)?
        } else {
            plan::apply_with(&actions, dry, &mut tracked_action)?
        };
        for line in report {
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
                let j = journal.as_mut().ok_or("extension journal absent")?;
                let catalog = home.join(".config/comandos/extensions/catalog.json");
                let operations = if catalog.exists() {
                    vec!["sync"]
                } else {
                    vec!["import", "sync"]
                };
                for operation in operations {
                    j.operation(&home, operation, &mut tracked_action)?;
                }
                super::extensions::apply_with_journal(&home, platform, j, &mut tracked_action)?;
            }
            if dry {
                super::extensions::apply(&home, platform, true)?;
            }
        }
        if std::env::var("COMANDOS_RETIRE_TELEGRAM").as_deref() == Ok("1") {
            super::telegram::apply_with(&home, dry, journal.as_mut(), &mut tracked_action)?;
        }
        if let Some(j) = journal.as_mut() {
            j.commit()?;
        }
        if !dry && let Err(error) = release::prune_after_install(&home, &staged.id) {
            eprintln!("installed successfully; release pruning deferred: {error}");
        }
        Ok(0)
    })();
    let result = if let Some(journal) = journal {
        super::transaction::finish_with_runtime(journal, result, run_action)
    } else {
        result
    };
    result.map_err(|error| {
        if external_effects.is_empty() {
            error
        } else {
            format!(
                "{error}; external runtime state was not reverted: {}",
                external_effects.join(", ")
            )
        }
    })
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
                .nth(4)
                .filter(|home| *path == home.join(".local/share/fonts/comandos"))
                .ok_or("font path outside installation")?;
            let Some(program) = find("fc-cache") else {
                return Ok(());
            };
            command(
                home,
                &program,
                if path.is_dir() {
                    vec!["-f".into(), path.to_string_lossy().into_owned()]
                } else {
                    vec!["-f".into()]
                },
                Duration::from_secs(30),
            )
        }
        Action::ExtensionOperation {
            home,
            journal,
            operation,
            quiescent,
        } => super::extension_worker::run(home, journal, operation, quiescent),
        Action::SystemctlState {
            home,
            unit,
            require_loaded,
            response,
        } => {
            let program = find("systemctl").ok_or("systemctl query unavailable")?;
            let output = capture(
                home,
                &program,
                vec![
                    "--user".into(),
                    "show".into(),
                    "--all".into(),
                    unit.clone(),
                    "--property=LoadState,UnitFileState,ActiveState".into(),
                ],
                Duration::from_secs(15),
            )?;
            if output.code != Some(0) {
                return Err("systemctl state query failed".into());
            }
            let text = std::str::from_utf8(&output.stdout)
                .map_err(|_| "systemctl state query is not UTF8")?;
            *response.borrow_mut() = Some(parse_unit_state(text, *require_loaded)?);
            Ok(())
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

fn parse_unit_state(text: &str, require_loaded: bool) -> Result<super::plan::UnitState, String> {
    let mut fields = std::collections::BTreeMap::new();
    for line in text.lines() {
        let (name, value) = line
            .split_once('=')
            .ok_or("invalid systemctl state query")?;
        if fields.insert(name, value).is_some() {
            return Err("duplicate systemctl state property".into());
        }
    }
    if fields.get("LoadState") != Some(&"loaded")
        && (require_loaded || fields.get("LoadState") != Some(&"not-found"))
    {
        return Err("runtime unit must be loaded before retirement".into());
    }
    let (enabled, runtime) = match fields.get("UnitFileState") {
        Some(&"enabled") => (true, false),
        Some(&"enabled-runtime") => (true, true),
        Some(&"disabled") => (false, false),
        Some(&"") if !require_loaded && fields.get("LoadState") == Some(&"not-found") => {
            (false, false)
        }
        _ => return Err("unsupported unit enablement state; retirement refused".into()),
    };
    let active = match fields.get("ActiveState") {
        Some(&"active") => true,
        Some(&"inactive") => false,
        _ => return Err("transitional/failed unit state; retirement refused".into()),
    };
    Ok(super::plan::UnitState {
        enabled,
        runtime,
        active,
    })
}
#[cfg(test)]
mod runtime_state_tests {
    use super::*;
    #[test]
    fn supported_state_and_archived_unit_absence_are_parsed_strictly() {
        let state = parse_unit_state(
            "ActiveState=active\nLoadState=loaded\nUnitFileState=enabled-runtime\n",
            true,
        )
        .unwrap();
        assert!(state.enabled && state.runtime && state.active);
        let absent = "LoadState=not-found\nUnitFileState=\nActiveState=inactive\n";
        assert!(parse_unit_state(absent, true).is_err());
        assert_eq!(
            parse_unit_state(absent, false).unwrap(),
            super::super::plan::UnitState {
                enabled: false,
                runtime: false,
                active: false
            }
        );
        for enablement in ["masked", "static", "indirect", "linked", "unknown"] {
            assert!(
                parse_unit_state(
                    &format!(
                        "LoadState=loaded\nUnitFileState={enablement}\nActiveState=inactive\n"
                    ),
                    true
                )
                .is_err()
            );
        }
        assert!(
            parse_unit_state(
                "LoadState=loaded\nUnitFileState=enabled\nActiveState=activating\n",
                true
            )
            .is_err()
        );
        assert!(parse_unit_state("LoadState=loaded\nUnitFileState=enabled\nActiveState=active\nActiveState=inactive\n",true).is_err());
    }
}
