use comandos_extensions::{Result, command, home_dir};
use serde_json::Value;
use std::{os::unix::process::CommandExt, path::PathBuf};
fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut home = None;
    let mut catalog = None;
    loop {
        match args.next().as_deref() {
            Some("--home") => home = Some(PathBuf::from(args.next().ok_or("Missing home")?)),
            Some("--catalog") => {
                catalog = Some(PathBuf::from(args.next().ok_or("Missing catalog")?))
            }
            Some(action @ ("import" | "sync" | "status")) => {
                if args.next().is_some() {
                    return Err("Unexpected arguments".into());
                }
                let home = home.map(Ok).unwrap_or_else(home_dir)?;
                return catalog_command(&home, catalog.as_deref(), action);
            }
            Some("check") => {
                let home = home.map(Ok).unwrap_or_else(home_dir)?;
                let path =
                    catalog.unwrap_or_else(|| comandos_extensions::config::catalog_path(&home));
                let catalog = comandos_extensions::config::read_config(&path)?;
                if catalog["version"] != 1 {
                    return Err("Shared catalog missing or unsupported".into());
                }
                let names = args.collect();
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .max_blocking_threads(8)
                    .enable_all()
                    .build()
                    .map_err(|_| "Runtime unavailable")?;
                let result = runtime.block_on(async {
                    let result = comandos_extensions::check::run(&home, &catalog, names).await;
                    comandos_extensions::auth::drain_workers().await;
                    result
                });
                runtime.shutdown_timeout(std::time::Duration::from_millis(100));
                if result? {
                    std::process::exit(1);
                }
                return Ok(());
            }
            Some("serve") => break,
            Some("count") if args.next().is_none() => {
                let home = home.map(Ok).unwrap_or_else(home_dir)?;
                return comandos_extensions::tokenizer::count_command(&home);
            }
            _ => {
                return Err(
                    "Usage: comandos-extensions [--home PATH] [--catalog PATH] {import|sync|status|count|check [NAME...]|serve NAME}".into(),
                );
            }
        }
    }
    let name = args.next().ok_or("Missing server name")?;
    if args.next().is_some() {
        return Err("Unexpected arguments".into());
    }
    let home = home.map(Ok).unwrap_or_else(home_dir)?;
    let path = catalog.unwrap_or_else(|| home.join(".config/comandos/extensions/catalog.json"));
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "Catalog unavailable")?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Catalog unavailable")?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("Catalog too large".into());
    }
    let catalog: Value = comandos_extensions::config::parse_json(&bytes)?;
    if catalog["version"] != 1 {
        return Err("Unsupported catalog".into());
    }
    let spec = catalog["servers"]
        .get(&name)
        .filter(|s| s.is_object() && s["enabled"] != false)
        .ok_or("Server unavailable")?;
    if spec["command"].as_str().is_some()
        && spec.get("enabled_tools").is_none()
        && spec
            .get("disabled_tools")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
    {
        let _ = command(spec, true)?.exec();
        return Err("Extension command failed".into());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .max_blocking_threads(4)
        .enable_all()
        .build()
        .map_err(|_| "Runtime unavailable")?;
    let result = runtime.block_on(async {
        let result = comandos_extensions::serve::serve(&home, &name, spec).await;
        // A running refresh must persist its rotated token before process exit.
        comandos_extensions::auth::drain_workers().await;
        result
    });
    runtime.shutdown_timeout(std::time::Duration::from_millis(100));
    result
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn catalog_command(
    home: &std::path::Path,
    path: Option<&std::path::Path>,
    action: &str,
) -> Result<()> {
    use comandos_extensions::{auth, catalog, config, skills};
    use serde_json::json;
    let default = config::catalog_path(home);
    let path = path.unwrap_or(&default);
    let launcher = home
        .join(".local/bin/cc-extensions")
        .to_string_lossy()
        .into_owned();
    let result = match action {
        "import" => {
            let _lock = config::SyncLock::new(home)?;
            if path.exists() || path.is_symlink() {
                return Err("Catalog already exists; refusing to overwrite it".into());
            }
            let c = catalog::import_catalog(home)?;
            let imported = auth::import_credentials(home, &c)?;
            config::save_json(path, &c)?;
            catalog::save_snapshot(home, &c, None)?;
            json!({"servers":c["servers"].as_object().ok_or("Invalid catalog")?.len(),"credentials_imported":imported})
        }
        "sync" => {
            let _lock = config::SyncLock::new(home)?;
            let original =
                config::read_bytes(path)?.ok_or("Shared catalog missing or unsupported")?;
            let mut c = config::Document::parse(path, Some(&original))?.data;
            if c["version"] != 1 {
                return Err("Shared catalog missing or unsupported".into());
            }
            let mut inputs = json!({});
            let revised = catalog::reconcile(home, &c, &launcher, &mut inputs)?;
            if !config::toml::python_equal(&revised, &c) {
                c = revised;
                config::replace_config(home, path, Some(&original), &config::json_bytes(&c)?)?;
            }
            auth::import_credentials(home, &c)?;
            let mut observed = json!({});
            let configs = catalog::sync_configs(home, &c, &launcher, &mut observed, Some(&inputs))?;
            let skills = skills::sync_skills(home)?;
            catalog::save_snapshot(home, &c, Some(&observed))?;
            json!({"configurations_changed":configs.len(),"skill_links_changed":skills.len()})
        }
        "status" => {
            let c = config::read_config(path)?;
            if c["version"] != 1 {
                return Err("Shared catalog missing or unsupported".into());
            }
            let credentials =
                config::read_config(&home.join(".config/comandos/extensions/credentials.json"))?;
            let mut servers = c["servers"]
                .as_object()
                .ok_or("Invalid catalog")?
                .iter()
                .collect::<Vec<_>>();
            servers.sort_by_key(|(n, _)| *n);
            json!({"servers":servers.into_iter().map(|(n,s)|json!({"name":n,"enabled":s.get("enabled").cloned().unwrap_or(json!(true)),"credential_saved":credentials.get(n).is_some(),"transport":if s["command"].as_str().is_some_and(|s|!s.is_empty()){json!("stdio")}else{s.get("transport").cloned().unwrap_or(json!("http"))}})).collect::<Vec<_>>(),"targets":catalog::targets(home)?.iter().map(|t|&t.path).collect::<Vec<_>>()})
        }
        _ => return Err("Unknown command".into()),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&result).map_err(|_| "Invalid command result")?
    );
    Ok(())
}
