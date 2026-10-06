#![forbid(unsafe_code)]
fn main() {
    #[cfg(target_os = "macos")]
    {
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        let result = comandos_app_mac::app::parse_args(&args).and_then(comandos_app_mac::ffi::run);
        if let Err(error) = result {
            eprintln!("ComandOS: {error}");
            std::process::exit(2);
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("comandos-app-mac requiere macOS");
        std::process::exit(2);
    }
}
