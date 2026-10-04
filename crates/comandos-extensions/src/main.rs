fn main() {
    // `args()` entra en pánico con un argumento que no es UTF-8: se rechaza con 2.
    let Ok(args) = (std::env::args_os().skip(1))
        .map(std::ffi::OsString::into_string)
        .collect::<Result<Vec<String>, _>>()
    else {
        eprintln!("comandos-extensions: argumento que no es UTF-8");
        std::process::exit(2);
    };
    match comandos_extensions::cli::run(args) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
