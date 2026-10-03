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
            Some("serve") => break,
            _ => {
                return Err(
                    "Usage: comandos-extensions [--home PATH] [--catalog PATH] serve NAME".into(),
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
    let catalog: Value = comandos_extensions::auth::parse_config_bytes(&bytes)?;
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
