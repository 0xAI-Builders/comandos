//! Explicit Codex maintenance, account identities and owned subprocesses.
pub mod batch;
mod cli;
mod install;
pub mod policy;
mod process;
pub mod protocol;
pub mod release;
pub mod reports;
pub mod runtime;
pub type Result<T> = std::result::Result<T, String>;
pub fn main(args: &[String]) -> i32 {
    main_for_platform(args, std::env::consts::OS)
}
fn main_for_platform(args: &[String], platform: &str) -> i32 {
    let Some((name, tail)) = args.split_first() else {
        eprintln!("uso: comandos codex <full-access|thread-release|yolo-install|yolo-policy>");
        return 2;
    };
    if tail == ["--help"] {
        println!(
            "uso: comandos codex {name} {}",
            match name.as_str() {
                "yolo-install" => "[--home RUTA] [--executable RUTA] [--dry-run]",
                "yolo-policy" => "-- [argumentos Codex]",
                "thread-release" => "--plan RUTA_ABSOLUTA [--dry-run]",
                "full-access" =>
                    "[--apply|--install-only|--dry-run] [--retry-failed|--retry-report RUTA_ABSOLUTA] [--proc-root RUTA_ABSOLUTA]",
                _ => "",
            }
        );
        return if matches!(
            name.as_str(),
            "full-access" | "thread-release" | "yolo-install" | "yolo-policy"
        ) {
            0
        } else {
            2
        };
    }
    if matches!(name.as_str(), "full-access" | "thread-release")
        && let Err(error) = require_linux(platform)
    {
        eprintln!("{error}");
        return 1;
    }
    let result = match name.as_str() {
        "yolo-policy" => {
            if tail.first().map(String::as_str) != Some("--") {
                Err("yolo-policy requiere -- antes de los argumentos".into())
            } else {
                policy::normalize(&tail[1..])
                    .and_then(|v| comandos_core::json::dumps(&serde_json::json!(v), false, false))
                    .map(|s| {
                        println!("{s}");
                        0
                    })
            }
        }
        "yolo-install" => install::cli(tail),
        "thread-release" => cli::thread(tail),
        "full-access" => cli::full(tail).or_else(|e| {
            if e.starts_with("uso:") {
                eprintln!("{e}");
                Ok(2)
            } else {
                println!("No se pudo completar el lote: {e}");
                Ok(1)
            }
        }),
        _ => Err(format!("comandos codex: subcomando desconocido: {name}")),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            if name == "yolo-policy" { 2 } else { 1 }
        }
    }
}
pub(super) fn require_linux(platform: &str) -> Result<()> {
    if platform == "linux" {
        Ok(())
    } else {
        Err("full-access y thread-release requieren Linux (/proc y pidfd); usa yolo-install para instalar el lanzador en macOS".into())
    }
}
pub fn launch(args: &[String]) -> i32 {
    match install::launch(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            2
        }
    }
}
#[cfg(test)]
mod portability_tests;
