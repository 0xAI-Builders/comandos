mod parity;
mod poll;
mod rss;
mod web;

use std::{io::Write, process::exit};

const USAGE: &str = "uso: cargo xtask rss [--samples N] -- <cmd…>";

fn usage() -> ! {
    eprintln!("{USAGE}");
    exit(2);
}

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("error: {msg}");
    exit(1);
}

/// Mediana (elemento central tras ordenar; en par, el superior).
fn median<T: Ord + Copy>(v: &[T]) -> T {
    let mut s = v.to_vec();
    s.sort();
    s[s.len() / 2]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("rss") => {}
        Some("parity") => {
            // Falla cerrado: sin namespace de red propio no se lanza el oráculo.
            parity::ensure_isolated().unwrap_or_else(|e| fail(e));
            match parity::run(&args[1..]) {
                Ok(code) => exit(code),
                Err(e) => fail(e),
            }
        }
        Some("poll") => {
            // Errores de uso antes de entrar al namespace: salida 2, sin lanzar nada.
            if let Err(e) = poll::parse(&args[1..]) {
                eprintln!("error: {e}");
                exit(2);
            }
            parity::ensure_isolated().unwrap_or_else(|e| fail(e));
            match poll::run(&args[1..]) {
                Ok(()) => exit(0),
                Err(e) => fail(e),
            }
        }
        Some("shots") => exit(web::shots(&args[1..])),
        Some("png-diff") => exit(web::png_diff(&args[1..])),
        Some("dom-diff") => exit(web::dom_diff(&args[1..])),
        Some("fixtures") => exit(web::fixtures(&args[1..])),
        Some("web-build") => exit(xtask::web_build::main(&args[1..])),
        Some("browser-e2e") => exit(xtask::browser_e2e::main(&args[1..])),
        Some("web-bench") => exit(xtask::web_bench::main(&args[1..])),
        Some("web-inventory") => exit(xtask::web_inventory::main(&args[1..])),
        Some("web-port") => exit(xtask::web_port::main(&args[1..])),
        Some("lint") => exit(xtask::lint::main(&args[1..])),
        _ => {
            eprintln!(
                "subcomandos: rss, parity, poll, shots, png-diff, dom-diff, fixtures, web-build, web-bench, web-inventory, web-port, browser-e2e, lint"
            );
            exit(2);
        }
    }
    let Some(sep) = args.iter().position(|a| a == "--") else {
        usage()
    };
    let cmd = &args[sep + 1..];
    if cmd.is_empty() {
        usage();
    }
    let mut samples: usize = 1;
    let opts = &args[1..sep];
    let mut i = 0;
    while i < opts.len() {
        if opts[i] == "--samples" {
            samples = match opts.get(i + 1).and_then(|s| s.parse::<usize>().ok()) {
                Some(n) if n >= 1 => n,
                _ => usage(),
            };
            i += 2;
        } else {
            usage();
        }
    }

    let (mut pss, mut rss, mut procs) = (Vec::new(), Vec::new(), Vec::new());
    let mut exact = true;
    for _ in 0..samples {
        let m = rss::measure(cmd, 2).unwrap_or_else(|e| fail(e));
        pss.push(m.pss_kib_total);
        rss.push(m.rss_kib_total);
        procs.push(m.procs);
        exact &= m.pss_exact;
    }
    let unix_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = serde_json::json!({
        "cmd": cmd,
        "samples": samples,
        "pss_kib_min": pss.iter().min(),
        "pss_kib_median": median(&pss),
        "rss_kib_min": rss.iter().min(),
        "rss_kib_median": median(&rss),
        "procs": median(&procs),
        "pss_exact": exact,
        "unix_time": unix_time,
    });
    println!("{line}");
    // Ruta relativa a la raíz del workspace, válida desde cualquier cwd.
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/verification/rss.jsonl");
    if let Some(dir) = path.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        fail(format!("{}: {e}", dir.display()));
    }
    let res = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
        .and_then(|mut f| writeln!(f, "{line}"));
    if let Err(e) = res {
        fail(format!("{}: {e}", path.display()));
    }
}
