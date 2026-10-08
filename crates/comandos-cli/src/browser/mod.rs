pub mod expose;
pub mod migrate;
pub mod npx_guard;
pub mod remote;

pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("remote") => remote::run(home()),
        Some("expose") => expose::run(&args[1..], home()),
        Some("npx-guard") => npx_guard::run(&args[1..], home()),
        Some("migrate-config") => migrate::run(&args[1..], home()),
        _ => {
            eprintln!("uso: comandos browser <remote|expose|npx-guard|migrate-config>");
            2
        }
    }
}

fn home() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/"))
}
