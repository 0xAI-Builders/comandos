//! Binario `comandos-notifyd`: `POST 127.0.0.1:<puerto>/notify` y los popups.
use crate::actions::{self, Effects, Target, TmuxRunner};
use crate::dash::{CheckoutFiles, DashClient, PrefsCache, PrefsSource, resolve_repo_root};
use crate::http::serve;
use crate::notice::{Lang, Notice, ui_lang};
use crate::popup::{self, Context};
use crate::position::load_anchor;
use crate::sweep;
use gtk::glib;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex, mpsc};

/// Opciones de línea de órdenes (N3 del sub-plan).
#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    pub headless: bool,
    pub port: u16,
    pub hooks: PathBuf,
    pub dash: DashClient,
    pub repo_root: Option<PathBuf>,
    /// `--tmux-socket`: la caída a tmux usa `tmux -S <ruta>` (pruebas); sin
    /// él, `tmux` a secas como el Python.
    pub tmux_socket: Option<PathBuf>,
}

pub fn parse_options(args: &[std::ffi::OsString]) -> Result<Options, String> {
    // `~/.claude/hooks` por omisión; sin `HOME` no hay omisión posible (nunca
    // una ruta relativa al directorio de trabajo) y hace falta `--hooks-dir`.
    let mut hooks = std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(".claude/hooks"));
    let mut options = Options {
        headless: false,
        port: 4778,
        hooks: PathBuf::new(),
        dash: DashClient::parse("http://127.0.0.1:4777").ok_or("URL del tablero")?,
        repo_root: None,
        tmux_socket: None,
    };
    let mut args = args.iter().cloned();
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
                hooks = Some(args.next().ok_or("--hooks-dir necesita una ruta")?.into());
            }
            Some("--dash-url") => {
                let value = args.next().ok_or("--dash-url necesita una URL")?;
                options.dash = value
                    .to_str()
                    .and_then(DashClient::parse)
                    .ok_or("--dash-url necesita http://<host>[:<puerto>]")?;
            }
            Some("--repo-root") => {
                options.repo_root =
                    Some(args.next().ok_or("--repo-root necesita una ruta")?.into());
            }
            Some("--tmux-socket") => {
                options.tmux_socket =
                    Some(args.next().ok_or("--tmux-socket necesita una ruta")?.into());
            }
            _ => return Err(format!("opción desconocida: {}", arg.to_string_lossy())),
        }
    }
    options.hooks = hooks.ok_or("HOME no está definido: hace falta --hooks-dir")?;
    Ok(options)
}

/// Archivos del checkout (`resolve_repo_root`); avisa si no se encontró o no
/// tiene `config/themes.json` (los popups saldrían con el tema base y la
/// viñeta `•` de icono).
fn checkout_files(options: &Options) -> CheckoutFiles {
    let env_repo = std::env::var(comandos_core::repo::REPO_ENV).ok();
    let exe = std::env::current_exe().ok();
    let repo = resolve_repo_root(
        options.repo_root.as_deref(),
        env_repo.as_deref(),
        &options.hooks,
        exe.as_deref(),
    );
    let files = CheckoutFiles::new(repo.as_deref());
    let missing = match &files.themes_file {
        None => Some("no se encontró el checkout".to_string()),
        Some(themes) if !themes.is_file() => Some(format!("no existe {}", themes.display())),
        Some(_) => None,
    };
    if let Some(missing) = missing {
        eprintln!(
            "comandos-notifyd: aviso: {missing} (tema base e iconos «•»); usa --repo-root o {}",
            comandos_core::repo::REPO_ENV
        );
    }
    files
}

/// «Abrir» (`open_session`): tablero y, si no responde, tmux + `wmctrl`, en
/// un hilo propio para no bloquear el de GTK.
fn open_action(effects: Effects) -> popup::OpenAction {
    Box::new(move |session: &str, pane: &str| {
        let effects = effects.clone();
        let target = Target {
            session: session.to_string(),
            pane: pane.to_string(),
        };
        actions::in_background("notifyd-open", move || {
            actions::open_session(&effects, &target);
        });
    })
}

/// Lanza el barrido de «te espera»: pregunta a la pila por el canal del hilo
/// de GTK y le devuelve `/state` con `idle_add_once`.
fn start_sweep(dash: DashClient) {
    let spawned = std::thread::Builder::new()
        .name("notifyd-sweep".into())
        .spawn(move || {
            sweep::run(
                &dash,
                |every| {
                    std::thread::sleep(every);
                    true
                },
                || {
                    let (tx, rx) = mpsc::sync_channel(1);
                    // Siempre como fuente del bucle de GTK (`invoke` podría
                    // ejecutarla en este hilo, que no tiene la pila).
                    glib::idle_add_once(move || {
                        let _ = tx.send(popup::has_waiting());
                    });
                    rx.recv_timeout(sweep::SWEEP_EVERY).unwrap_or(false)
                },
                |state| {
                    glib::idle_add_once(move || popup::close_stale(&state));
                },
            );
        });
    if let Err(err) = spawned {
        eprintln!("comandos-notifyd: no se pudo arrancar el barrido de esperas: {err}");
    }
}

fn listen(port: u16) -> Option<TcpListener> {
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => Some(listener),
        Err(err) => {
            eprintln!("comandos-notifyd: no se pudo escuchar en 127.0.0.1:{port}: {err}");
            None
        }
    }
}

/// Arranca el hilo del servidor; los avisos aceptados salen por el canal.
fn start_server(listener: TcpListener, hooks: PathBuf) -> Option<mpsc::Receiver<Notice>> {
    let (sink, notices) = mpsc::channel::<Notice>();
    let server = std::thread::Builder::new()
        .name("notifyd-accept".into())
        .spawn(move || serve(listener, sink, hooks));
    match server {
        Ok(_) => Some(notices),
        Err(err) => {
            eprintln!("comandos-notifyd: no se pudo arrancar el servidor: {err}");
            None
        }
    }
}

fn run_headless(listener: TcpListener, port: u16, hooks: PathBuf) -> ExitCode {
    let Some(notices) = start_server(listener, hooks) else {
        return ExitCode::FAILURE;
    };
    println!("comandos-notifyd listo en 127.0.0.1:{port} (popups propios v3)");
    // Sin pantalla (solo pruebas): cada aviso aceptado es una línea JSON.
    for notice in notices {
        match serde_json::to_string(&notice.fields()) {
            Ok(line) => println!("{line}"),
            Err(err) => eprintln!("comandos-notifyd: aviso sin serializar: {err}"),
        }
    }
    // El canal solo se cierra si el hilo del servidor terminó: sin él no hay
    // servicio, así que se sale con error para que systemd lo reinicie.
    eprintln!("comandos-notifyd: el servidor HTTP terminó; se sale");
    ExitCode::FAILURE
}

fn run_gtk(options: Options, lang: Lang, listener: TcpListener, port: u16) -> ExitCode {
    if let Err(err) = gtk::init() {
        eprintln!("comandos-notifyd: GTK no pudo arrancar (¿sin pantalla?): {err}");
        return ExitCode::FAILURE;
    }
    let files = checkout_files(&options);
    let cache = Arc::new(Mutex::new(PrefsCache::default()));
    let source = PrefsSource {
        dash: options.dash.clone(),
        themes_file: files.themes_file,
    };
    // Como `apply_theme_css()` al principio de `main()`: el tema antes del bucle.
    source.refresh(&cache);
    let pos_file = options.hooks.join("notifyd-pos.json");
    popup::install(
        Context {
            lang,
            icons_dir: files.icons_dir,
            pos_file: pos_file.clone(),
            prefs: Arc::clone(&cache),
            prefs_source: source.clone(),
            on_open: open_action(Effects {
                dash: options.dash.clone(),
                tmux: options
                    .tmux_socket
                    .clone()
                    .map_or_else(TmuxRunner::system, TmuxRunner::with_socket),
                wmctrl: "wmctrl".into(),
            }),
        },
        load_anchor(&pos_file),
    );
    let Some(notices) = start_server(listener, options.hooks.clone()) else {
        return ExitCode::FAILURE;
    };
    // Puente al hilo de GTK: `/prefs` se refresca aquí (bloquea) y el popup se
    // crea con `idle_add`, como el `GLib.idle_add(native_notify, …)` del Python.
    let forwarder = std::thread::Builder::new()
        .name("notifyd-forward".into())
        .spawn(move || {
            for notice in notices {
                source.refresh(&cache);
                glib::idle_add_once(move || popup::show(notice));
            }
            eprintln!("comandos-notifyd: el servidor HTTP terminó; se sale");
            glib::idle_add_once(gtk::main_quit);
        });
    if let Err(err) = forwarder {
        eprintln!("comandos-notifyd: no se pudo arrancar el puente a GTK: {err}");
        return ExitCode::FAILURE;
    }
    start_sweep(options.dash.clone());
    println!("comandos-notifyd listo en 127.0.0.1:{port} (popups propios v3)");
    gtk::main();
    // Solo se llega aquí si el servidor terminó.
    ExitCode::FAILURE
}

pub fn run(args: &[std::ffi::OsString]) -> ExitCode {
    let options = match parse_options(args) {
        Ok(options) => options,
        Err(err) => {
            eprintln!("comandos-notifyd: {err}");
            return ExitCode::from(2);
        }
    };
    // `UI_LANG` se fija una vez al arrancar, como en el Python.
    let lang = ui_lang(
        &options.hooks.join("cc-notify.conf"),
        std::env::var("LANG").ok().as_deref(),
    );
    let Some(listener) = listen(options.port) else {
        return ExitCode::FAILURE;
    };
    let port = listener.local_addr().map_or(options.port, |a| a.port());
    if options.headless {
        return run_headless(listener, port, options.hooks);
    }
    run_gtk(options, lang, listener, port)
}
