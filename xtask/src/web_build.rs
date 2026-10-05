//! `cargo run -p xtask -- web-build` (Fase 3, T4): compila los crates WASM con el
//! perfil `release-wasm`, pasa `wasm-bindgen` y `wasm-opt -Oz`, nombra cada crate
//! por el hash de su contenido (`<out>/<hash>/…`) y escribe `<out>/manifest.json`.
//! `<out>` es `<directorio de compilación>/web`, resuelto como cargo (preflight R6),
//! y es lo que `comandos install --stage` copia a la release.
use flate2::{Compression, write::GzEncoder};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

pub use comandos_core::web_assets::{MANIFEST_FILE, Manifest};

/// Cargador generado. Única lógica: instanciar y ceder el control al WASM,
/// que monta, publica sus globales y avisa a la compuerta (`/web/ready`).
pub const BOOT_TEMPLATE: &str = r#"// Generado por `cargo xtask web-build`. No editar.
import init, { boot } from "{{MODULE}}";
const me = document.currentScript || document.querySelector("script[data-k]");
await init({ module_or_path: new URL("{{WASM}}", import.meta.url) });
boot(me ? me.dataset.k || "" : "");
"#;

/// Versiones exactas de las herramientas (el crate `wasm-bindgen` va fijado igual).
pub const WASM_BINDGEN_VERSION: &str = "0.2.129";
pub const WASM_OPT_VERSION: &str = "116";

const USAGE: &str = "uso: cargo run -p xtask -- web-build [--crate comandos-web|comandos-term-web|comandos-web-sw|all] [--check-budget]";

const WASM_TARGET: &str = "wasm32-unknown-unknown";
const PROFILE: &str = "release-wasm";
/// Nombre físico del cargador dentro de `<hash>/` (la página carga `/web/<hash>/boot.js`).
const BOOT_FILE: &str = "boot.js";

pub fn render_boot(module_js: &str, wasm: &str) -> String {
    BOOT_TEMPLATE
        .replace("{{MODULE}}", module_js)
        .replace("{{WASM}}", wasm)
}

/// `sha256` de las partes concatenadas, 12 dígitos hexadecimales.
pub fn content_hash(parts: &[&[u8]]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize()
        .iter()
        .take(6)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Salida de `wasm-bindgen`: módulo ES (`web`) o script clásico (`no-modules`, el
/// service worker).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindgenTarget {
    Web,
    NoModules,
}

impl BindgenTarget {
    fn flag(self) -> &'static str {
        match self {
            BindgenTarget::Web => "web",
            BindgenTarget::NoModules => "no-modules",
        }
    }
}

/// Un crate `cdylib` que se compila a WASM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmCrate {
    /// Nombre del paquete (`-p`).
    pub name: String,
    /// Nombre de la biblioteca: el del paquete con `_` (salida de rustc y bindgen).
    pub lib: String,
    pub target: BindgenTarget,
    /// Presupuesto del `.wasm` comprimido con gzip, en bytes.
    pub budget_gzip: usize,
}

impl WasmCrate {
    pub fn new(name: &str, target: BindgenTarget, budget_gzip: usize) -> Self {
        WasmCrate {
            name: name.to_string(),
            lib: name.replace('-', "_"),
            target,
            budget_gzip,
        }
    }

    /// Nombres lógicos del manifiesto: `<lib>.js`, `<lib>_bg.wasm` y, en módulos
    /// ES, `<lib>_boot.js` (el archivo físico es `<hash>/boot.js`).
    fn files(&self) -> (String, String, Option<String>) {
        let boot = (self.target == BindgenTarget::Web).then(|| format!("{}_boot.js", self.lib));
        (
            format!("{}.js", self.lib),
            format!("{}_bg.wasm", self.lib),
            boot,
        )
    }
}

/// Los tres crates de la Fase 3 y sus presupuestos gzip (600/250/64 KiB).
pub fn known_crates() -> Vec<WasmCrate> {
    vec![
        WasmCrate::new("comandos-web", BindgenTarget::Web, 600 * 1024),
        WasmCrate::new("comandos-term-web", BindgenTarget::Web, 250 * 1024),
        WasmCrate::new("comandos-web-sw", BindgenTarget::NoModules, 64 * 1024),
    ]
}

/// Rutas de las herramientas externas.
#[derive(Debug, Clone)]
pub struct Tools {
    pub cargo: PathBuf,
    pub wasm_bindgen: PathBuf,
    pub wasm_opt: PathBuf,
}

impl Tools {
    /// `CARGO` (lo fija cargo), `WASM_BINDGEN` y `WASM_OPT`, o los nombres en `PATH`.
    pub fn from_env() -> Self {
        let pick = |var: &str, default: &str| {
            std::env::var_os(var)
                .filter(|v| !v.is_empty())
                .map_or_else(|| PathBuf::from(default), PathBuf::from)
        };
        Tools {
            cargo: pick("CARGO", "cargo"),
            wasm_bindgen: pick("WASM_BINDGEN", "wasm-bindgen"),
            wasm_opt: pick("WASM_OPT", "wasm-opt"),
        }
    }

    /// Comprueba las versiones exactas; si faltan o no coinciden, el error dice
    /// cómo instalarlas.
    pub fn check(&self) -> Result<(), String> {
        let bindgen = version(&self.wasm_bindgen);
        if bindgen
            .as_deref()
            .map(|v| v.split_whitespace().last() != Some(WASM_BINDGEN_VERSION))
            .unwrap_or(true)
        {
            return Err(format!(
                "wasm-bindgen {WASM_BINDGEN_VERSION} requerido (hallado: {}); instalar con: cargo install --locked wasm-bindgen-cli --version ={WASM_BINDGEN_VERSION}",
                bindgen.unwrap_or_else(|| "ninguno".into())
            ));
        }
        let opt = version(&self.wasm_opt);
        if opt
            .as_deref()
            .map(|v| !v.contains(WASM_OPT_VERSION))
            .unwrap_or(true)
        {
            return Err(format!(
                "wasm-opt {WASM_OPT_VERSION} requerido (hallado: {}); instalar con: cargo install --locked wasm-opt --version =0.116.1",
                opt.unwrap_or_else(|| "ninguno".into())
            ));
        }
        Ok(())
    }
}

/// Primera línea de `<tool> --version`, o `None` si no se pudo ejecutar.
fn version(tool: &Path) -> Option<String> {
    let out = Command::new(tool).arg("--version").output().ok()?;
    out.status.success().then_some(())?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().next().map(|l| l.trim().to_string())
}

/// Qué compilar y adónde.
#[derive(Debug, Clone)]
pub struct Options {
    /// `Cargo.toml` del workspace que contiene los crates.
    pub manifest_path: PathBuf,
    /// `CARGO_TARGET_DIR` de la compilación wasm.
    pub target_dir: PathBuf,
    /// Directorio de salida (`<target>/web`).
    pub out: PathBuf,
    pub crates: Vec<WasmCrate>,
    pub check_budget: bool,
    pub tools: Tools,
}

/// Directorio de compilación del workspace, resuelto como cargo (R6).
pub fn target_dir() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("sin directorio de trabajo: {e}"))?;
    let env = std::env::var_os("CARGO_TARGET_DIR");
    Ok(comandos_core::web_assets::target_dir_from(
        env.as_deref().map(Path::new),
        &cwd,
        &workspace_root(),
    ))
}

/// `<directorio de compilación>/web`: lo que escribe `web-build` y lee
/// `install --stage` (y, más adelante, el `build.rs` de B15).
pub fn out_dir() -> Result<PathBuf, String> {
    target_dir().map(|t| t.join("web"))
}

/// Raíz del workspace: el padre de `xtask/`.
fn workspace_root() -> PathBuf {
    let xtask = Path::new(env!("CARGO_MANIFEST_DIR"));
    xtask.parent().unwrap_or(xtask).to_path_buf()
}

/// Argumentos de `wasm-bindgen` para un crate.
pub fn bindgen_args(c: &WasmCrate, wasm: &Path, out_dir: &Path) -> Vec<OsString> {
    vec![
        "--target".into(),
        c.target.flag().into(),
        "--no-typescript".into(),
        "--out-dir".into(),
        out_dir.into(),
        "--out-name".into(),
        c.lib.clone().into(),
        wasm.into(),
    ]
}

/// Tamaño comprimido con gzip al máximo nivel (como se mide el presupuesto).
pub fn gzip_len(bytes: &[u8]) -> Result<usize, String> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::best());
    enc.write_all(bytes).map_err(|e| format!("gzip: {e}"))?;
    enc.finish()
        .map(|v| v.len())
        .map_err(|e| format!("gzip: {e}"))
}

/// Con `enforce`, error si `gz` supera el presupuesto del crate.
pub fn check_budget(c: &WasmCrate, gz: usize, enforce: bool) -> Result<(), String> {
    if enforce && gz > c.budget_gzip {
        return Err(format!(
            "{}: {} B gzip supera el presupuesto de {} B ({} KiB)",
            c.name,
            gz,
            c.budget_gzip,
            c.budget_gzip / 1024
        ));
    }
    Ok(())
}

/// Corre un comando y devuelve error con su stderr si falla.
fn run(cmd: &mut Command, what: &str) -> Result<(), String> {
    let out = cmd
        .output()
        .map_err(|e| format!("{what}: no se pudo ejecutar: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "{what} falló ({}):\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    ))
}

/// Características WASM que rustc 1.96 activa por defecto en `wasm32-unknown-unknown`;
/// `wasm-opt` 116 las necesita explícitas para validar el módulo.
const WASM_OPT_FEATURES: &[&str] = &[
    "--enable-bulk-memory",
    "--enable-nontrapping-float-to-int",
    "--enable-sign-ext",
    "--enable-mutable-globals",
    "--enable-reference-types",
    "--enable-multivalue",
];

/// Archivos de un crate ya procesado, listos para copiar.
struct Built {
    hash: String,
    files: Vec<(String, String, Vec<u8>)>, // (lógico, físico, bytes)
}

fn build_one(opts: &Options, c: &WasmCrate) -> Result<Built, String> {
    run(
        Command::new(&opts.tools.cargo)
            .arg("build")
            .arg("--manifest-path")
            .arg(&opts.manifest_path)
            .args([
                "--locked",
                "-p",
                &c.name,
                "--profile",
                PROFILE,
                "--target",
                WASM_TARGET,
            ])
            .env("CARGO_TARGET_DIR", &opts.target_dir),
        &format!("cargo build -p {}", c.name),
    )?;
    let raw = opts
        .target_dir
        .join(WASM_TARGET)
        .join(PROFILE)
        .join(format!("{}.wasm", c.lib));
    let work = opts.target_dir.join("web-build").join(&c.name);
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| format!("no se pudo crear {}: {e}", work.display()))?;
    run(
        Command::new(&opts.tools.wasm_bindgen).args(bindgen_args(c, &raw, &work)),
        &format!("wasm-bindgen {}", c.name),
    )?;
    let (js_name, wasm_name, boot_name) = c.files();
    let wasm_path = work.join(&wasm_name);
    run(
        Command::new(&opts.tools.wasm_opt)
            .arg("-Oz")
            .args(WASM_OPT_FEATURES)
            .arg("-o")
            .arg(&wasm_path)
            .arg(&wasm_path),
        &format!("wasm-opt {}", c.name),
    )?;
    let read = |p: &Path| fs::read(p).map_err(|e| format!("no se pudo leer {}: {e}", p.display()));
    let js = read(&work.join(&js_name))?;
    let wasm = read(&wasm_path)?;
    let gz = gzip_len(&wasm)?;
    println!(
        "{}: {} B wasm, {} B gzip (presupuesto {} KiB)",
        c.name,
        wasm.len(),
        gz,
        c.budget_gzip / 1024
    );
    check_budget(c, gz, opts.check_budget)?;
    let mut files = vec![
        (js_name.clone(), js_name.clone(), js),
        (wasm_name.clone(), wasm_name.clone(), wasm),
    ];
    if let Some(boot) = boot_name {
        let text = render_boot(&format!("./{js_name}"), &format!("./{wasm_name}"));
        files.push((boot, BOOT_FILE.to_string(), text.into_bytes()));
    }
    let parts: Vec<&[u8]> = files.iter().map(|(_, _, b)| b.as_slice()).collect();
    Ok(Built {
        hash: content_hash(&parts),
        files,
    })
}

/// Deja `<out>/<hash>/` completo: se escribe aparte y entra con un `rename`. Un
/// `<hash>/` existente tiene el mismo contenido y se conserva.
fn place(out: &Path, b: &Built) -> Result<(), String> {
    let dest = out.join(&b.hash);
    if dest.is_dir() {
        return Ok(());
    }
    let tmp = out.join(format!(".{}.tmp.{}", b.hash, std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).map_err(|e| format!("no se pudo crear {}: {e}", tmp.display()))?;
    for (_, physical, bytes) in &b.files {
        let p = tmp.join(physical);
        fs::write(&p, bytes).map_err(|e| format!("no se pudo escribir {}: {e}", p.display()))?;
    }
    fs::rename(&tmp, &dest).map_err(|e| {
        let _ = fs::remove_dir_all(&tmp);
        format!("no se pudo instalar {}: {e}", dest.display())
    })
}

/// Un nombre de directorio de hash: 12 dígitos hexadecimales en minúscula.
fn is_hash_dir(name: &str) -> bool {
    name.len() == 12
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Une `new` sobre `old` (compilar un crate conserva las entradas de los demás) y
/// borra de `out` los directorios de hash que ya nadie referencia. Lo que no tenga
/// forma de hash se respeta.
pub fn merge_and_prune(out: &Path, old: Manifest, new: Manifest) -> Result<Manifest, String> {
    let mut merged = old;
    merged.files.extend(new.files);
    let live: std::collections::BTreeSet<&str> = merged
        .files
        .values()
        .filter_map(|v| v.split('/').next())
        .collect();
    let rd = fs::read_dir(out).map_err(|e| format!("no se pudo leer {}: {e}", out.display()))?;
    for entry in rd {
        let entry = entry.map_err(|e| format!("no se pudo leer {}: {e}", out.display()))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir && is_hash_dir(name) && !live.contains(name) {
            let p = entry.path();
            fs::remove_dir_all(&p)
                .map_err(|e| format!("no se pudo borrar {}: {e}", p.display()))?;
        }
    }
    Ok(merged)
}

/// Compila los crates de `opts`, actualiza `<out>/manifest.json` y lo devuelve.
pub fn build(opts: &Options) -> Result<Manifest, String> {
    opts.tools.check()?;
    fs::create_dir_all(&opts.out)
        .map_err(|e| format!("no se pudo crear {}: {e}", opts.out.display()))?;
    let mut fresh = Manifest::default();
    for c in &opts.crates {
        let b = build_one(opts, c)?;
        place(&opts.out, &b)?;
        for (logical, physical, _) in &b.files {
            fresh
                .files
                .insert(logical.clone(), format!("{}/{physical}", b.hash));
        }
    }
    let path = opts.out.join(MANIFEST_FILE);
    let old = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("{} ilegible: {e}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Manifest::default(),
        Err(e) => return Err(format!("no se pudo leer {}: {e}", path.display())),
    };
    let merged = merge_and_prune(&opts.out, old, fresh)?;
    let text = serde_json::to_string_pretty(&merged).map_err(|e| e.to_string())?;
    let tmp = opts
        .out
        .join(format!(".{MANIFEST_FILE}.tmp.{}", std::process::id()));
    fs::write(&tmp, format!("{text}\n"))
        .and_then(|()| fs::rename(&tmp, &path))
        .map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("no se pudo escribir {}: {e}", path.display())
        })?;
    Ok(merged)
}

/// `(--crate elegido o None = todos, --check-budget)`.
pub fn parse_args(args: &[String]) -> Result<(Option<String>, bool), String> {
    let mut krate = None;
    let mut budget = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--check-budget" => budget = true,
            "--crate" => {
                let v = it.next().ok_or(USAGE)?;
                if v == "all" {
                    krate = None;
                } else if known_crates().iter().any(|c| &c.name == v) {
                    krate = Some(v.clone());
                } else {
                    return Err(format!("crate desconocido: {v}\n{USAGE}"));
                }
            }
            _ => return Err(USAGE.to_string()),
        }
    }
    Ok((krate, budget))
}

/// Punto de entrada del subcomando; devuelve el código de salida.
pub fn main(args: &[String]) -> i32 {
    let (krate, check_budget) = match parse_args(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let ws = workspace_root();
    // `all` compila los crates que ya existen; uno explícito debe existir.
    let present = |c: &WasmCrate| ws.join("crates").join(&c.name).join("Cargo.toml").is_file();
    let crates: Vec<WasmCrate> = match &krate {
        Some(name) => known_crates()
            .into_iter()
            .filter(|c| &c.name == name)
            .collect(),
        None => known_crates().into_iter().filter(present).collect(),
    };
    if let Some(missing) = crates.iter().find(|c| !present(c)) {
        eprintln!("error: crates/{} no existe todavía", missing.name);
        return 1;
    }
    if crates.is_empty() {
        eprintln!("error: no hay crates WASM en el workspace (llegan con A6, B1 y B9)");
        return 1;
    }
    let result = target_dir().and_then(|target_dir| {
        let opts = Options {
            manifest_path: ws.join("Cargo.toml"),
            out: target_dir.join("web"),
            target_dir,
            crates,
            check_budget,
            tools: Tools::from_env(),
        };
        build(&opts).map(|m| (opts.out, m))
    });
    match result {
        Ok((out, m)) => {
            println!(
                "{} archivos en {}",
                m.files.len(),
                out.join(MANIFEST_FILE).display()
            );
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}
