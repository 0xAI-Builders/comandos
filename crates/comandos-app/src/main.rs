use comandos_app::config::{Entry, resolve_entry};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut argv = std::env::args_os();
    let argv0 = argv.next().unwrap_or_default();
    let rest: Vec<_> = argv.collect();
    match resolve_entry(&argv0.to_string_lossy()) {
        Entry::App { default_live } => {
            if rest.first().is_some_and(|a| a == "--version") {
                println!("comandos-app {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            let rest = match rest
                .into_iter()
                .map(|a| a.into_string())
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(rest) => rest,
                Err(arg) => {
                    eprintln!(
                        "comandos-app: argumento no UTF-8: {}",
                        arg.to_string_lossy()
                    );
                    return ExitCode::from(2);
                }
            };
            comandos_app::ui::app::run(&rest, default_live)
        }
        Entry::Notifyd => comandos_app::notifyd::run(&rest),
    }
}
