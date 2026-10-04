use comandos_cli::dispatch::{Command, resolve};

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let code = match resolve(&argv[0], &argv[1..]) {
        Command::Ext(args) => comandos_extensions::cli::run(args).unwrap_or_else(|e| {
            eprintln!("{e}");
            1
        }),
        Command::Events(args) => comandos_runtime::events_cli::run(&args).unwrap_or_else(|e| {
            eprintln!("event_intake: {e}");
            1
        }),
        Command::Install(a) => comandos_cli::install::run(&a).unwrap_or_else(|e| {
            eprintln!("{e}");
            1
        }),
        Command::Hook(_) => {
            eprintln!("comandos hook: pendiente (Tarea 10)");
            2
        }
        Command::Version => {
            println!("comandos {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Command::Help => {
            println!("uso: comandos <ext|hook|events|--version>");
            0
        }
        Command::Unknown(w) => {
            eprintln!("comandos: subcomando desconocido: {w}");
            2
        }
    };
    std::process::exit(code);
}
