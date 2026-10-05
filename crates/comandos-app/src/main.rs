use comandos_app::config::{Entry, resolve_entry};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut argv = std::env::args();
    let argv0 = argv.next().unwrap_or_default();
    let rest: Vec<String> = argv.collect();
    if rest.first().is_some_and(|a| a == "--version") {
        println!("comandos-app {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    match resolve_entry(&argv0) {
        // T7 sustituye este brazo por `comandos_app::ui::app::run(&rest, default_live)`.
        Entry::App { .. } => {
            eprintln!("comandos-app: interfaz pendiente (Tarea 7)");
            ExitCode::from(2)
        }
        // T19 sustituye este brazo por `comandos_app::notifyd::run(&rest)`.
        Entry::Notifyd => {
            eprintln!("comandos-app: popups pendientes (Tarea 19)");
            ExitCode::from(2)
        }
    }
}
