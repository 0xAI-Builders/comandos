//! Binario `comandos-notifyd`: `POST 127.0.0.1:<puerto>/notify` y los popups.
use comandos_notifyd::http::serve;
use comandos_notifyd::notice::{Notice, ui_lang};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;

/// Opciones de línea de órdenes (N3 del sub-plan).
struct Options {
    headless: bool,
    port: u16,
    hooks: PathBuf,
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options {
        headless: false,
        port: 4778,
        hooks: std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(".claude/hooks"),
    };
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--headless") => options.headless = true,
            Some("--port") => {
                let value = args.next().ok_or("--port necesita un número")?;
                options.port = value
                    .to_str()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--port necesita un número")?;
            }
            Some("--hooks-dir") => {
                options.hooks = args.next().ok_or("--hooks-dir necesita una ruta")?.into();
            }
            _ => return Err(format!("opción desconocida: {}", arg.to_string_lossy())),
        }
    }
    Ok(options)
}

fn main() -> ExitCode {
    let options = match parse_options() {
        Ok(options) => options,
        Err(err) => {
            eprintln!("comandos-notifyd: {err}");
            return ExitCode::from(2);
        }
    };
    // `UI_LANG` se fija una vez al arrancar, como en el Python; lo usan los popups.
    let _ui_lang = ui_lang(
        &options.hooks.join("cc-notify.conf"),
        std::env::var("LANG").ok().as_deref(),
    );
    if !options.headless {
        eprintln!("comandos-notifyd: los popups GTK aún no están; usa --headless");
        return ExitCode::from(2);
    }
    let listener = match TcpListener::bind(("127.0.0.1", options.port)) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!(
                "comandos-notifyd: no se pudo escuchar en 127.0.0.1:{}: {err}",
                options.port
            );
            return ExitCode::FAILURE;
        }
    };
    let port = listener.local_addr().map_or(options.port, |a| a.port());
    let (sink, notices) = mpsc::channel::<Notice>();
    let hooks = options.hooks.clone();
    let server = std::thread::Builder::new()
        .name("notifyd-accept".into())
        .spawn(move || serve(listener, sink, hooks));
    if let Err(err) = server {
        eprintln!("comandos-notifyd: no se pudo arrancar el servidor: {err}");
        return ExitCode::FAILURE;
    }
    println!("comandos-notifyd listo en 127.0.0.1:{port} (popups propios v3)");
    // Sin pantalla (solo pruebas): cada aviso aceptado es una línea JSON.
    for notice in notices {
        match serde_json::to_string(&notice.fields()) {
            Ok(line) => println!("{line}"),
            Err(err) => eprintln!("comandos-notifyd: aviso sin serializar: {err}"),
        }
    }
    ExitCode::SUCCESS
}
