//! Subcomandos del arnés de paridad visual y de DOM (Fase 3, T2):
//! `shots`, `png-diff`, `dom-diff` y `fixtures`. Devuelven el código de salida.
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use xtask::{dom_diff, fixtures as fx, mcp, png_diff as pd, shots as sh};

const SHOTS_USAGE: &str = "uso: cargo run -p xtask -- shots pair --base URL --suite NAME --out DIR [--broker CMD]\n     cargo run -p xtask -- shots remote-vs-desktop --base URL --out DIR [--suite NAME] [--remote-base URL] [--broker CMD]";

/// Valor de `--name` en `args`.
fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn usage(text: &str) -> i32 {
    eprintln!("{text}");
    2
}

fn error(msg: impl std::fmt::Display) -> i32 {
    eprintln!("error: {msg}");
    1
}

fn connect(args: &[String]) -> Result<mcp::Client, String> {
    let command = flag(args, "--broker")
        .map(str::to_string)
        .unwrap_or_else(mcp::default_command);
    let parts: Vec<&str> = command.split_whitespace().collect();
    mcp::Client::spawn(&parts)
}

pub fn shots(args: &[String]) -> i32 {
    let mode = args.first().map(String::as_str);
    let (Some(base), Some(out)) = (flag(args, "--base"), flag(args, "--out")) else {
        return usage(SHOTS_USAGE);
    };
    let out = PathBuf::from(out);
    match mode {
        Some("pair") => {
            let Some(name) = flag(args, "--suite") else {
                return usage(SHOTS_USAGE);
            };
            let result = sh::read_suite(&sh::suite_path(name))
                .and_then(|t| sh::parse_pair_suite(&t))
                .and_then(|suite| {
                    sh::check_base(base)?;
                    let mut client = connect(args)?;
                    sh::run_pair(&mut client, base, name, &suite, &out)
                });
            match result {
                Ok(0) => 0,
                Ok(_) => 1,
                Err(e) => error(e),
            }
        }
        Some("remote-vs-desktop") => {
            let name = flag(args, "--suite").unwrap_or("remote-vs-desktop");
            let remote_base = flag(args, "--remote-base").unwrap_or(base);
            let result = sh::read_suite(&sh::suite_path(name))
                .and_then(|t| sh::parse_remote_suite(&t))
                .and_then(|suite| {
                    sh::check_base(base)?;
                    sh::check_base(remote_base)?;
                    let mut client = connect(args)?;
                    sh::run_remote_vs_desktop(&mut client, base, remote_base, &suite, &out)
                });
            match result {
                Ok(0) => 0,
                Ok(_) => 1,
                Err(e) => error(e),
            }
        }
        _ => usage(SHOTS_USAGE),
    }
}

/// Igual que tools/png_diff.py: % de píxeles distintos, diff en rojo, salida
/// 0 si no pasa de 0,1 %.
pub fn png_diff(args: &[String]) -> i32 {
    let [a, b, out] = args else {
        return usage("uso: cargo run -p xtask -- png-diff A.png B.png DIFF.png");
    };
    let load = |p: &String| {
        std::fs::read(p)
            .map_err(|e| format!("{p}: {e}"))
            .and_then(|bytes| pd::decode(&bytes))
    };
    let result = load(a).and_then(|ia| {
        let ib = load(b)?;
        let stats = pd::diff(&ia, &ib, 24)?;
        pd::write_diff_png(&ia, &ib, 24, Path::new(out))?;
        Ok(stats)
    });
    match result {
        Ok(stats) => {
            let pct = 100.0 * stats.ratio();
            println!("{pct:.3}% de píxeles distintos");
            if pct <= 0.1 { 0 } else { 1 }
        }
        Err(e) => error(e),
    }
}

/// Primera diferencia del DOM normalizado de dos archivos HTML.
pub fn dom_diff(args: &[String]) -> i32 {
    let [a, b] = args else {
        return usage("uso: cargo run -p xtask -- dom-diff A.html B.html");
    };
    let read = |p: &String| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"));
    match read(a).and_then(|ta| Ok((ta, read(b)?))) {
        Ok((ta, tb)) => match dom_diff::first_difference(&ta, &tb) {
            None => {
                println!("DOM normalizado igual");
                0
            }
            Some(d) => {
                println!("{d}");
                1
            }
        },
        Err(e) => error(e),
    }
}

/// Servidor de fixtures en 127.0.0.1 (nunca en los puertos del tablero vivo).
pub fn fixtures(args: &[String]) -> i32 {
    let (Some(root), Some(port)) = (flag(args, "--root"), flag(args, "--port")) else {
        return usage("uso: cargo run -p xtask -- fixtures --root DIR --port N");
    };
    let Ok(port) = port.parse::<u16>() else {
        return usage("--port debe ser un número de puerto");
    };
    if sh::LIVE_PORTS.contains(&port) {
        return error(format!("{port} es un puerto del tablero vivo; usa 73xx"));
    }
    let root = PathBuf::from(root);
    if !root.is_dir() {
        return error(format!("{}: no es una carpeta", root.display()));
    }
    println!("fixtures: http://127.0.0.1:{port}/ ← {}", root.display());
    match fx::serve(&root, port, Arc::new(Mutex::new(Vec::new()))) {
        Ok(()) => 0,
        Err(e) => error(format!("fixtures: {e}")),
    }
}
