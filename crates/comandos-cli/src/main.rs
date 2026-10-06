use comandos_cli::dispatch::{Command, resolve};

fn main() {
    // `args()` entra en pánico con un argumento que no es UTF-8: se rechaza con 2.
    let Ok(argv) = (std::env::args_os())
        .map(std::ffi::OsString::into_string)
        .collect::<Result<Vec<String>, _>>()
    else {
        eprintln!("comandos: argumento que no es UTF-8");
        std::process::exit(2);
    };
    let (argv0, args) = argv
        .split_first()
        .map_or(("comandos", &[][..]), |(a, r)| (&**a, r));
    let code = match resolve(argv0, args) {
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
        Command::Hook(args) => comandos_runtime::hooks::run(&args),
        Command::Dash(args) => comandos_server::dash::main(&args),
        Command::Web(args) => comandos_cli::web::main(&args),
        Command::Webterm(args) => comandos_cli::webterm::main(&args),
        Command::WebtermAttach(args) => comandos_cli::webterm_attach::main(&args),
        Command::State(args) => comandos_cli::state::cli::main(&args),
        Command::Keys(args) => comandos_cli::keys::main(&args),
        Command::Browser(args) => comandos_cli::browser::run(&args),
        Command::X(args) => comandos_cli::x::main(&args),
        Command::Raise(args) => comandos_cli::raise::main(&args),
        Command::Winstart(args) => comandos_cli::winstart::main(&args),
        Command::Next(args) => comandos_cli::next::main(&args),
        Command::Version => {
            println!("comandos {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Command::Help => {
            println!(
                "uso: comandos <ext|hook|events|install|dash|browser|web|webterm|webterm-attach|state|keys|x|raise|winstart|next|--version>"
            );
            0
        }
        Command::Unknown(w) => {
            eprintln!("comandos: subcomando desconocido: {w}");
            2
        }
    };
    std::process::exit(code);
}
