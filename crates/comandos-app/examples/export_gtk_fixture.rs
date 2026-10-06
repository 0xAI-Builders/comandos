fn main() -> std::process::ExitCode {
    let Some(root) = std::env::args().nth(1) else {
        eprintln!("Uso: export_gtk_fixture /ruta/absoluta/nueva");
        return std::process::ExitCode::from(2);
    };
    match comandos_app::fixture::export(std::path::Path::new(&root)) {
        Ok(()) => {
            println!("{root}/run/comandos-app-sbx");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::from(2)
        }
    }
}
