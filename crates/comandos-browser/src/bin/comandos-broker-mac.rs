use comandos_browser::{apparmor, broker::Broker, config::BrokerConfig};
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

fn main() {
    std::process::exit(run(std::env::args().skip(1).collect()));
}

fn run(args: Vec<String>) -> i32 {
    match args.first().map(String::as_str) {
        Some("--version") | Some("-V") => {
            println!("comandos-broker-mac {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("serve") => serve(&args[1..]),
        Some("status") => status(&args[1..]),
        Some("apparmor") => apparmor_cmd(&args[1..]),
        _ => {
            eprintln!(
                "uso: comandos-broker-mac serve --config <ruta> [--port N] | status [--state-dir D] | apparmor print|install | --version"
            );
            2
        }
    }
}

fn serve(args: &[String]) -> i32 {
    let mut config = None;
    let mut port = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--config" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("falta valor para --config");
                    return 2;
                };
                config = Some(PathBuf::from(value));
            }
            "--port" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("falta valor para --port");
                    return 2;
                };
                port = match value.parse::<u16>() {
                    Ok(port) => Some(port),
                    Err(_) => {
                        eprintln!("puerto inválido: {value}");
                        return 2;
                    }
                };
            }
            other => {
                eprintln!("argumento desconocido: {other}");
                return 2;
            }
        }
        i += 1;
    }
    let Some(config_path) = config else {
        eprintln!("falta --config");
        return 2;
    };
    let mut cfg = match BrokerConfig::load(&config_path) {
        Ok(cfg) => cfg,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    if let Some(port) = port {
        cfg.port = port;
    }
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    rt.block_on(async move {
        println!("ComandOS browser broker ready on loopback");
        let _ = io::stdout().flush();
        match Broker::serve(cfg, shutdown_signal()).await {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("{error}");
                1
            }
        }
    })
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

fn status(args: &[String]) -> i32 {
    let mut state_dir = default_state_dir();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--state-dir" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("falta valor para --state-dir");
                    return 2;
                };
                state_dir = PathBuf::from(value);
            }
            other => {
                eprintln!("argumento desconocido: {other}");
                return 2;
            }
        }
        i += 1;
    }
    let path = state_dir.join("status.json");
    let Ok(meta) = std::fs::metadata(&path) else {
        return 1;
    };
    let fresh = meta
        .modified()
        .ok()
        .and_then(|mtime| SystemTime::now().duration_since(mtime).ok())
        .is_some_and(|age| age <= Duration::from_secs(15));
    if !fresh {
        return 1;
    }
    match std::fs::read(&path) {
        Ok(bytes) => {
            if io::stdout().write_all(&bytes).is_err() {
                return 1;
            }
            0
        }
        Err(_) => 1,
    }
}

fn apparmor_cmd(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("print") => {
            if io::stdout().write_all(apparmor::PROFILE).is_err() {
                return 1;
            }
            0
        }
        Some("install") => match apparmor::install() {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("{error}");
                1
            }
        },
        _ => {
            eprintln!("uso: comandos-broker-mac apparmor print|install");
            2
        }
    }
}

fn default_state_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new("/").to_path_buf())
        .join(".local/share/comandos-browser/state")
}
