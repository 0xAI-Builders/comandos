use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppShotArgs {
    pub remote: String,
    pub manifest: PathBuf,
    pub output: PathBuf,
    pub measure: bool,
}

pub fn parse(args: &[String]) -> Result<AppShotArgs, String> {
    let mut remote = None;
    let mut manifest = None;
    let mut output = None;
    let mut measure = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--remote" => {
                remote = args.get(i + 1).cloned();
                i += 2;
            }
            "--manifest" => {
                manifest = args.get(i + 1).map(PathBuf::from);
                i += 2;
            }
            "--output" => {
                output = args.get(i + 1).map(PathBuf::from);
                i += 2;
            }
            "--measure" => {
                measure = true;
                i += 1;
            }
            other => return Err(format!("opción desconocida: {other}")),
        }
    }
    let parsed = AppShotArgs {
        remote: remote.ok_or("--remote requerido")?,
        manifest: manifest.ok_or("--manifest requerido")?,
        output: output.ok_or("--output requerido")?,
        measure,
    };
    if !parsed.manifest.is_absolute() || !parsed.output.is_absolute() {
        return Err("--manifest y --output deben ser rutas absolutas locales".into());
    }
    Ok(parsed)
}

pub fn run(args: &[String]) -> i32 {
    let parsed = match parse(args) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };
    let missing = preflight_missing(&parsed);
    if !missing.is_empty() {
        eprintln!(
            "app-shot: runtime remoto no disponible en {}: {}",
            parsed.remote,
            missing.join(", ")
        );
        return 2;
    }
    eprintln!("app-shot: captura remota pendiente de runtime verificado");
    2
}

fn preflight_missing(parsed: &AppShotArgs) -> Vec<&'static str> {
    let mut missing = vec![
        "linux-gtk3-runtime",
        "webkitgtk-2.50",
        "isolated-display",
        "remote-capture-tool",
        "private-tmux",
        "fixture-http",
        "python-gi-vte-oracle",
        "rust-toolchain",
    ];
    if parsed.remote != "macmini" {
        missing.push("supported-remote-macmini");
    }
    missing
}
