use crate::{Result, home_dir};
use serde_json::Value;
use std::path::PathBuf;
pub fn run(args: Vec<String>) -> Result<i32> {
    let mut args = args.into_iter();
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
                catalog_command(&home, catalog.as_deref(), action)?;
                return Ok(0);
            }
            Some("check") => {
                let home = home.map(Ok).unwrap_or_else(home_dir)?;
                let path = catalog.unwrap_or_else(|| crate::config::catalog_path(&home));
                let catalog = crate::config::read_config(&path)?;
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
                    let result = crate::check::run(&home, &catalog, names).await;
                    crate::auth::drain_workers().await;
                    result
                });
                runtime.shutdown_timeout(std::time::Duration::from_millis(100));
                return Ok(i32::from(result?));
            }
            Some("serve") => break,
            Some("broker") if args.next().is_none() => {
                let home = home.map(Ok).unwrap_or_else(home_dir)?;
                let path = catalog.unwrap_or_else(|| crate::config::catalog_path(&home));
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|_| "Runtime unavailable")?;
                runtime.block_on(crate::broker::daemon::run(&path))?;
                runtime.shutdown_timeout(std::time::Duration::from_millis(100));
                return Ok(0);
            }
            Some("count") if args.next().is_none() => {
                let home = home.map(Ok).unwrap_or_else(home_dir)?;
                crate::tokenizer::count_command(&home)?;
                return Ok(0);
            }
            _ => {
                return Err(
                    "Usage: comandos-extensions [--home PATH] [--catalog PATH] {import|sync|status|count|check [NAME...]|serve NAME|broker}".into(),
                );
            }
        }
    }
    let name = args.next().ok_or("Missing server name")?;
    if args.next().is_some() {
        return Err("Unexpected arguments".into());
    }
    // El broker es por usuario: con rutas explícitas se usa siempre el proxy directo.
    let explicit = home.is_some() || catalog.is_some();
    let home = home.map(Ok).unwrap_or_else(home_dir)?;
    let path = catalog.unwrap_or_else(|| crate::config::catalog_path(&home));
    let spec = &server_spec(&path, &name)?;
    if crate::broker::direct_stdio(spec) {
        if !explicit
            && crate::config::is_shared(spec, &name)
            && let Some(code) = broker_session(&name)
        {
            return Ok(code);
        }
        crate::exec_direct(spec)?;
        return Err("Extension command failed".into());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .max_blocking_threads(4)
        .enable_all()
        .build()
        .map_err(|_| "Runtime unavailable")?;
    let result = runtime.block_on(async {
        let result = crate::serve::serve(&home, &name, spec).await;
        // A running refresh must persist its rotated token before process exit.
        crate::auth::drain_workers().await;
        result
    });
    runtime.shutdown_timeout(std::time::Duration::from_millis(100));
    result?;
    Ok(0)
}
/// Servidor `name` del catálogo, habilitado. Lo usan `serve` y el broker.
pub(crate) fn server_spec(path: &std::path::Path, name: &str) -> Result<Value> {
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
    let mut catalog: Value = crate::config::parse_json(&bytes)?;
    if catalog["version"] != 1 {
        return Err("Unsupported catalog".into());
    }
    catalog["servers"]
        .get_mut(name)
        .filter(|s| {
            // Python: `not spec` o `not spec.get('enabled', True)` ⇒ no disponible.
            s.as_object().is_some_and(|o| !o.is_empty())
                && s.get("enabled").is_none_or(crate::py_truthy)
        })
        .map(Value::take)
        .ok_or_else(|| format!("Server unavailable: {name}"))
}
/// Sesión como cliente fino del broker. `None` ⇒ proxy directo. Sin socket (broker no
/// instalado) se calla, para no ensuciar el stderr de la sesión.
fn broker_session(name: &str) -> Option<i32> {
    use crate::broker::{client, socket_path};
    let socket = socket_path();
    if !socket.exists() {
        return None;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    let Ok(stream) = runtime.block_on(client::connect(&socket, name)) else {
        eprintln!("broker no disponible, proxy directo");
        return None;
    };
    let relayed = runtime.block_on(client::relay(stream));
    // El hilo bloqueado en stdin no debe retrasar la salida.
    runtime.shutdown_timeout(std::time::Duration::from_millis(100));
    Some(if relayed.is_ok() { 0 } else { 1 })
}
fn catalog_command(
    home: &std::path::Path,
    path: Option<&std::path::Path>,
    action: &str,
) -> Result<()> {
    use crate::{auth, catalog, config, skills};
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
